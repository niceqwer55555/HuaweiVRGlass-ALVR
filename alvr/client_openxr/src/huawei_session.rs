// Session type detection for HUAWEI VR Glass.
//
// Which session the Huawei VR framework starts (6DoF: visual tracking kit + two controllers, or
// 3DoF: glasses IMU + the small remote) is declared by the app itself, through this meta-data in
// AndroidManifest.xml:
//
//     <meta-data android:name="com.huawei.vr.application.freeDegree" android:value="6dof" />
//
// The runtime only accepts bindings for the profile matching that session, and suggesting both
// crashes inside libvrsdk.so!hxrFindProfileForDevice as soon as the action sets are attached.
// Reading the declared degree here lets the client bind exactly one profile. A system property
// keeps being handy to override the value on a device without rebuilding the APK:
//
//     adb shell setprop debug.alvr.degree 3dof
use alvr_common::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HuaweiTrackingDegree {
    ThreeDof,
    SixDof,
}

#[allow(dead_code)]
const FREE_DEGREE_META_DATA: &str = "com.huawei.vr.application.freeDegree";
#[allow(dead_code)]
const DEGREE_OVERRIDE_PROPERTIES: [&str; 2] = ["debug.alvr.degree", "persist.alvr.degree"];

fn degree_from_value(value: &str) -> Option<HuaweiTrackingDegree> {
    let value = value.trim().to_ascii_lowercase();

    if value.contains('3') {
        Some(HuaweiTrackingDegree::ThreeDof)
    } else if value.contains('6') {
        Some(HuaweiTrackingDegree::SixDof)
    } else {
        None
    }
}

// The physical Huawei VR Glass panel and the runtime compositor both run at 70 Hz, and HVRSDK 3.5
// does not expose XR_FB_display_refresh_rate, so 70 Hz is the only rate the runtime can actually
// present. The rate the client advertises also becomes the server's frame pacing target
// (refresh_rate_hint -> duration_until_next_vsync), i.e. it is the video framerate that reaches
// the headset. Streaming at half the panel rate is often the only way to keep a software encoder
// realtime, so let it be overridden without rebuilding the APK:
//
//     adb shell setprop debug.alvr.refresh_rate 35
pub const DEFAULT_REFRESH_RATE: f32 = 70.0;
const REFRESH_RATE_OVERRIDE_PROPERTIES: [&str; 2] =
    ["debug.alvr.refresh_rate", "persist.alvr.refresh_rate"];

fn refresh_rate_from_value(value: &str) -> Option<f32> {
    let rate = value.trim().parse::<f32>().ok()?;

    // Anything outside this window is a typo, not a framerate.
    (1.0..=240.0).contains(&rate).then_some(rate)
}

#[cfg(target_os = "android")]
fn system_property(name: &str) -> Option<String> {
    extern "C" {
        fn __system_property_get(
            name: *const libc::c_char,
            value: *mut libc::c_char,
        ) -> libc::c_int;
    }

    let key = std::ffi::CString::new(name).ok()?;
    // Property values are at most 91 characters long, plus the terminator.
    let mut buffer = [0 as libc::c_char; 92];

    let length = unsafe { __system_property_get(key.as_ptr(), buffer.as_mut_ptr()) };
    if length <= 0 {
        return None;
    }

    Some(
        unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned(),
    )
}

// Reads `<application><meta-data android:name="com.huawei.vr.application.freeDegree" .../>` back
// through PackageManager, so a single native library serves both the 3DoF and the 6DoF APK.
#[cfg(target_os = "android")]
fn declared_degree() -> Option<HuaweiTrackingDegree> {
    use jni::{
        objects::{JObject, JString, JValue},
        JavaVM,
    };

    // PackageManager.GET_META_DATA
    const GET_META_DATA: i32 = 128;

    let android_context = ndk_context::android_context();
    let activity = unsafe { JObject::from_raw(android_context.context().cast()) };
    if activity.is_null() {
        return None;
    }

    // Note: android-activity owns this reference, JObject::from_raw does not take ownership.
    let degree = {
        let Ok(vm) = (unsafe { JavaVM::from_raw(android_context.vm().cast()) }) else {
            return None;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            return None;
        };

        let Ok(package_name) = env
            .call_method(&activity, "getPackageName", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
        else {
            return None;
        };
        let Ok(package_manager) = env
            .call_method(
                &activity,
                "getPackageManager",
                "()Landroid/content/pm/PackageManager;",
                &[],
            )
            .and_then(|value| value.l())
        else {
            return None;
        };
        let Ok(application_info) = env
            .call_method(
                &package_manager,
                "getApplicationInfo",
                "(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;",
                &[JValue::Object(&package_name), JValue::Int(GET_META_DATA)],
            )
            .and_then(|value| value.l())
        else {
            return None;
        };
        let Ok(meta_data) = env
            .get_field(&application_info, "metaData", "Landroid/os/Bundle;")
            .and_then(|value| value.l())
        else {
            return None;
        };
        if meta_data.is_null() {
            return None;
        }

        let Ok(key) = env.new_string(FREE_DEGREE_META_DATA) else {
            return None;
        };
        let Ok(value) = env
            .call_method(
                &meta_data,
                "getString",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&JObject::from(key))],
            )
            .and_then(|value| value.l())
        else {
            return None;
        };
        if value.is_null() {
            return None;
        }

        let value_string = JString::from(value);
        let Ok(java_string) = env.get_string(&value_string) else {
            return None;
        };

        degree_from_value(&java_string.to_string_lossy())
    };

    degree
}

/// Returns the session type the Huawei runtime is going to start, or None when it cannot be
/// determined (the caller then falls back to the 6DoF profile).
pub fn detect_tracking_degree() -> Option<HuaweiTrackingDegree> {
    #[cfg(target_os = "android")]
    {
        for property in DEGREE_OVERRIDE_PROPERTIES {
            if let Some(value) = system_property(property) {
                if let Some(degree) = degree_from_value(&value) {
                    info!(
                        "Huawei tracking degree forced by {property}={value}: {degree:?}; set it to `3dof` or `6dof`."
                    );

                    return Some(degree);
                }
            }
        }

        let degree = declared_degree();
        info!("[HVR-DIAG] Huawei tracking degree declared by this APK: {degree:?}");

        degree
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Display refresh rates to advertise to the server. The first entry is what the server paces
/// SteamVR at, so a single value keeps the video framerate unambiguous.
pub fn refresh_rates() -> Vec<f32> {
    #[cfg(target_os = "android")]
    {
        for property in REFRESH_RATE_OVERRIDE_PROPERTIES {
            if let Some(value) = system_property(property) {
                if let Some(rate) = refresh_rate_from_value(&value) {
                    info!(
                        "[HVR-DIAG] Huawei video framerate forced by {property}={value}: {rate}Hz (panel runs at {DEFAULT_REFRESH_RATE}Hz)."
                    );

                    return vec![rate];
                }
            }
        }
    }

    vec![DEFAULT_REFRESH_RATE]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_degree_from_the_manifest_value() {
        assert_eq!(degree_from_value("6dof"), Some(HuaweiTrackingDegree::SixDof));
        assert_eq!(degree_from_value("3dof"), Some(HuaweiTrackingDegree::ThreeDof));
        assert_eq!(degree_from_value("  6DoF "), Some(HuaweiTrackingDegree::SixDof));
        assert_eq!(degree_from_value(""), None);
    }

    #[test]
    fn reads_a_sane_refresh_rate() {
        assert_eq!(refresh_rate_from_value("35"), Some(35.0));
        assert_eq!(refresh_rate_from_value(" 72 "), Some(72.0));
        assert_eq!(refresh_rate_from_value("0"), None);
        assert_eq!(refresh_rate_from_value("fast"), None);
    }
}
