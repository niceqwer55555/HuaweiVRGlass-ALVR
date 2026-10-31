pub mod commands;
mod parse;

use alvr_common::anyhow::Result;
use alvr_common::{dbg_connection, error};
use alvr_system_info::{
    ClientFlavor, PACKAGE_NAME_GITHUB_DEV, PACKAGE_NAME_GITHUB_STABLE, PACKAGE_NAME_STORE,
};
use std::collections::HashSet;

pub enum WiredConnectionStatus {
    Ready,
    NotReady(String),
}

pub struct WiredConnection {
    adb_path: String,
}

impl WiredConnection {
    /// Picks the device to drive: USB first, then WiFi-connected (`ip:port`
    /// serials); emulators and loopback entries are never eligible.
    fn pick_device_serial(&self) -> Result<Option<String>> {
        let devices = commands::list_devices(&self.adb_path)?;

        let eligible = |d: &crate::parse::Device| {
            d.serial
                .as_deref()
                .map_or(false, |s| !s.starts_with("127.0.0.1"))
        };
        let mut serials: Vec<String> = devices
            .iter()
            .filter(|d| {
                eligible(d)
                    && matches!(
                        d.connection_state,
                        Some(crate::parse::ConnectionState::Device)
                    )
            })
            .filter_map(|d| d.serial.clone())
            .collect();
        if serials.is_empty() {
            // Be lenient when the state line cannot be parsed (or the device is
            // mid-connection): the later per-device commands fail loudly anyway.
            serials = devices.iter().filter(|d| eligible(d)).filter_map(|d| d.serial.clone()).collect();
        }
        // USB serials contain no ':'; sort them ahead of "ip:port" ones.
        serials.sort_by_key(|s| s.contains(':'));

        Ok(serials.into_iter().next())
    }

    pub fn new(
        layout: &alvr_filesystem::Layout,
        download_progress_callback: impl Fn(usize, Option<usize>),
    ) -> Result<Self> {
        let adb_path = commands::require_adb(layout, download_progress_callback)?;

        Ok(Self { adb_path })
    }

    pub fn setup(
        &self,
        control_port: u16,
        stream_port: u16,
        client_type: &ClientFlavor,
        client_autolaunch: bool,
        wireless_candidates: &[String],
    ) -> Result<WiredConnectionStatus> {
        let mut device_serial = self.pick_device_serial()?;

        // No USB phone (the glasses occupy its only USB-C port anyway): try the
        // known client IPs over WiFi adb. Forwarding works identically there,
        // so the rest of the wired flow does not care how the device is attached.
        if device_serial.is_none() {
            for candidate in wireless_candidates {
                let address = if candidate.contains(':') {
                    candidate.clone()
                } else {
                    format!("{candidate}:5555")
                };

                match commands::connect_wireless(&self.adb_path, &address) {
                    Ok(true) => {
                        dbg_connection!("setup: adb connect {address} ok");
                    }
                    Ok(false) => {
                        dbg_connection!("setup: adb connect {address} refused");
                    }
                    Err(e) => {
                        dbg_connection!("setup: adb connect {address} failed: {e:?}");
                    }
                }

                device_serial = self.pick_device_serial()?;
                if device_serial.is_some() {
                    break;
                }
            }
        }

        let Some(device_serial) = device_serial else {
            return Ok(WiredConnectionStatus::NotReady(
                "No USB or wireless adb devices found".to_owned(),
            ));
        };

        let ports = HashSet::from([control_port, stream_port]);
        let forwarded_ports: HashSet<u16> =
            commands::list_forwarded_ports(&self.adb_path, &device_serial)?
                .into_iter()
                .map(|f| f.local)
                .collect();
        let missing_ports = ports.difference(&forwarded_ports);
        for port in missing_ports {
            commands::forward_port(&self.adb_path, &device_serial, *port)?;
            dbg_connection!(
                "setup_wired_connection: Forwarded port {port} of device {device_serial}"
            );
        }

        let installed_packages =
            get_installed_client_packages(&self.adb_path, &device_serial, client_type);
        if installed_packages.is_empty() {
            return Ok(WiredConnectionStatus::NotReady(
                "No suitable ALVR client is installed".to_owned(),
            ));
        }

        // The 3DoF and 6DoF variants can be installed side by side; drive the
        // one that is actually running (they cannot run at the same time).
        let running_package = installed_packages
            .iter()
            .find(|package| {
                commands::get_process_id(&self.adb_path, &device_serial, package)
                    .map_or(false, |pid| pid.is_some())
            })
            .cloned();

        match running_package {
            Some(package) => {
                if !commands::is_activity_resumed(&self.adb_path, &device_serial, &package)? {
                    Ok(WiredConnectionStatus::NotReady("ALVR client is paused".to_owned()))
                } else {
                    Ok(WiredConnectionStatus::Ready)
                }
            }
            None => {
                if client_autolaunch {
                    commands::start_application(
                        &self.adb_path,
                        &device_serial,
                        &installed_packages[0],
                    )?;
                    Ok(WiredConnectionStatus::NotReady(
                        "Starting ALVR client".to_owned(),
                    ))
                } else {
                    Ok(WiredConnectionStatus::NotReady(
                        "ALVR client is not running".to_owned(),
                    ))
                }
            }
        }
    }
}

impl Drop for WiredConnection {
    fn drop(&mut self) {
        dbg_connection!("wired_connection: Killing ADB server");
        if let Err(e) = commands::kill_server(&self.adb_path) {
            error!("{e:?}");
        }
    }
}

pub fn get_installed_client_packages(
    adb_path: &str,
    device_serial: &str,
    flavor: &ClientFlavor,
) -> Vec<String> {
    let fallbacks = match flavor {
        ClientFlavor::Store => {
            if alvr_common::is_stable() {
                vec![
                    PACKAGE_NAME_STORE.to_owned(),
                    PACKAGE_NAME_GITHUB_STABLE.to_owned(),
                ]
            } else {
                vec![PACKAGE_NAME_GITHUB_DEV.to_owned()]
            }
        }
        ClientFlavor::Github => {
            if alvr_common::is_stable() {
                vec![
                    PACKAGE_NAME_GITHUB_STABLE.to_owned(),
                    PACKAGE_NAME_STORE.to_owned(),
                ]
            } else {
                vec![PACKAGE_NAME_GITHUB_DEV.to_owned()]
            }
        }
        ClientFlavor::Custom(name) => {
            // The 3DoF coexistence variant is the same app with ".3dof" appended
            // to the applicationId; accept either one.
            let sibling = format!("{name}.3dof");
            if alvr_common::is_stable() {
                vec![
                    name.clone(),
                    sibling,
                    PACKAGE_NAME_STORE.to_owned(),
                    PACKAGE_NAME_GITHUB_STABLE.to_owned(),
                ]
            } else {
                vec![name.clone(), sibling, PACKAGE_NAME_GITHUB_DEV.to_owned()]
            }
        }
    };

    fallbacks
        .into_iter()
        .filter(|name| {
            commands::is_package_installed(adb_path, device_serial, name)
                .is_ok_and(|installed| installed)
        })
        .collect()
}
