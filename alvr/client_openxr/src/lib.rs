mod c_api;
mod extra_extensions;
mod graphics;
#[cfg(target_os = "android")]
mod huawei_activity;
#[cfg(target_os = "android")]
mod huawei_runtime;
mod interaction;
mod lobby;
mod passthrough;
mod stream;

use crate::stream::ParsedStreamConfig;
use alvr_client_core::{ClientCapabilities, ClientCoreContext, ClientCoreEvent};
use alvr_common::settings_schema::Switch;
use alvr_common::{
    error,
    glam::{Quat, UVec2, Vec3},
    info,
    parking_lot::RwLock,
    Fov, Pose, HAND_LEFT_ID,
};
use alvr_graphics::{GraphicsContext, SDR_FORMAT_GL};
use alvr_session::{BodyTrackingBDConfig, BodyTrackingSourcesConfig};
use alvr_system_info::Platform;
use extra_extensions::{
    BD_BODY_TRACKING_EXTENSION_NAME, BD_MOTION_TRACKING_EXTENSION_NAME,
    META_BODY_TRACKING_FULL_BODY_EXTENSION_NAME, META_DETACHED_CONTROLLERS_EXTENSION_NAME,
    META_SIMULTANEOUS_HANDS_AND_CONTROLLERS_EXTENSION_NAME, PICO_CONFIGURATION_EXTENSION_NAME,
};
use interaction::{InteractionContext, InteractionSourcesConfig};
use lobby::Lobby;
use openxr as xr;
use passthrough::PassthroughLayer;
#[cfg(target_os = "android")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{ffi::c_void, path::Path, rc::Rc, sync::Arc, thread, time::Duration};
use stream::StreamContext;

#[cfg(target_os = "android")]
static HUAWEI_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "android")]
fn request_huawei_exit() {
    HUAWEI_EXIT_REQUESTED.store(true, Ordering::Release);
}

fn from_xr_vec3(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn to_xr_vec3(v: Vec3) -> xr::Vector3f {
    xr::Vector3f {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

fn from_xr_quat(q: xr::Quaternionf) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

fn to_xr_quat(q: Quat) -> xr::Quaternionf {
    xr::Quaternionf {
        x: q.x,
        y: q.y,
        z: q.z,
        w: q.w,
    }
}

fn from_xr_pose(p: xr::Posef) -> Pose {
    Pose {
        orientation: from_xr_quat(p.orientation),
        position: from_xr_vec3(p.position),
    }
}

fn to_xr_pose(p: Pose) -> xr::Posef {
    xr::Posef {
        orientation: to_xr_quat(p.orientation),
        position: to_xr_vec3(p.position),
    }
}

fn from_xr_fov(f: xr::Fovf) -> Fov {
    Fov {
        left: f.angle_left,
        right: f.angle_right,
        up: f.angle_up,
        down: f.angle_down,
    }
}

fn to_xr_fov(f: Fov) -> xr::Fovf {
    xr::Fovf {
        angle_left: f.left,
        angle_right: f.right,
        angle_up: f.up,
        angle_down: f.down,
    }
}

fn from_xr_time(timestamp: xr::Time) -> Duration {
    Duration::from_nanos(timestamp.as_nanos() as _)
}

fn to_xr_time(timestamp: Duration) -> xr::Time {
    xr::Time::from_nanos(timestamp.as_nanos() as _)
}

fn default_view() -> xr::View {
    xr::View {
        pose: xr::Posef {
            orientation: xr::Quaternionf {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            position: xr::Vector3f::default(),
        },
        fov: xr::Fovf {
            angle_left: -1.0,
            angle_right: 1.0,
            angle_up: 1.0,
            angle_down: -1.0,
        },
    }
}

// This exists to circumvent dead-code analysis
fn create_session(
    xr_instance: &xr::Instance,
    xr_system: xr::SystemId,
    graphics_context: &GraphicsContext,
) -> (
    xr::Session<xr::OpenGlEs>,
    xr::FrameWaiter,
    xr::FrameStream<xr::OpenGlEs>,
) {
    unsafe {
        xr_instance
            .create_session(xr_system, &graphics::session_create_info(graphics_context))
            .unwrap()
    }
}

#[cfg(target_os = "android")]
fn create_huawei_session(
    xr_instance: &xr::Instance,
    xr_system: xr::SystemId,
    graphics_context: &alvr_graphics::HuaweiEglContext,
) -> (
    xr::Session<xr::OpenGlEs>,
    xr::FrameWaiter,
    xr::FrameStream<xr::OpenGlEs>,
) {
    graphics_context.make_current();
    unsafe {
        xr_instance
            .create_session(
                xr_system,
                &graphics::huawei_session_create_info(graphics_context),
            )
            .unwrap()
    }
}

pub fn entry_point() {
    entry_point_with_window(None);
}

fn entry_point_with_window(_huawei_native_window: Option<*mut c_void>) {
    alvr_client_core::init_logging();
    info!("[HVR-DIAG] entered OpenXR entry point");

    let platform = alvr_system_info::platform();
    info!("[HVR-DIAG] detected platform: {platform}");

    let loader_filename = match platform {
        Platform::HuaweiVrGlass => "libxr_loader.so".to_owned(),
        platform => {
            let suffix = match platform {
                Platform::Quest1 => "_quest1",
                Platform::PicoNeo3
                | Platform::PicoG3
                | Platform::Pico4
                | Platform::Pico4Pro
                | Platform::Pico4Enterprise => "_pico_old",
                p if p.is_yvr() => "_yvr",
                Platform::Lynx => "_lynx",
                _ => "",
            };
            format!("libopenxr_loader{suffix}.so")
        }
    };
    #[cfg(target_os = "android")]
    let xr_entry = if platform.is_huawei() {
        info!("[HVR-DIAG] using directly linked OpenXR library: {loader_filename}");
        xr::Entry::linked()
    } else {
        info!("[HVR-DIAG] loading OpenXR library: {loader_filename}");
        unsafe { xr::Entry::load_from(Path::new(&loader_filename)).unwrap() }
    };

    #[cfg(not(target_os = "android"))]
    let xr_entry = {
        info!("[HVR-DIAG] loading OpenXR library: {loader_filename}");
        unsafe { xr::Entry::load_from(Path::new(&loader_filename)).unwrap() }
    };

    info!("[HVR-DIAG] OpenXR library ready");

    #[cfg(target_os = "android")]
    if !platform.is_huawei() {
        xr_entry.initialize_android_loader().unwrap();
    }

    info!("[HVR-DIAG] enumerating OpenXR extensions");
    let available_extensions = match xr_entry.enumerate_extensions() {
        Ok(extensions) => extensions,
        Err(error) => {
            error!("[HVR-DIAG] failed to enumerate OpenXR extensions: {error:?}");
            return;
        }
    };
    alvr_common::info!("OpenXR available extensions: {available_extensions:#?}");

    // todo: switch to vulkan
    assert!(available_extensions.khr_opengl_es_enable);

    let mut exts = xr::ExtensionSet::default();
    exts.bd_controller_interaction = available_extensions.bd_controller_interaction;
    exts.ext_eye_gaze_interaction = available_extensions.ext_eye_gaze_interaction;
    exts.ext_hand_tracking = available_extensions.ext_hand_tracking;
    exts.ext_local_floor = available_extensions.ext_local_floor;
    exts.fb_body_tracking = available_extensions.fb_body_tracking;
    exts.fb_color_space = available_extensions.fb_color_space;
    exts.fb_composition_layer_settings = available_extensions.fb_composition_layer_settings;
    exts.fb_display_refresh_rate = available_extensions.fb_display_refresh_rate;
    exts.fb_eye_tracking_social = available_extensions.fb_eye_tracking_social;
    exts.fb_face_tracking2 = available_extensions.fb_face_tracking2;
    exts.fb_foveation = available_extensions.fb_foveation;
    exts.fb_foveation_configuration = available_extensions.fb_foveation_configuration;
    exts.fb_passthrough = available_extensions.fb_passthrough;
    exts.fb_swapchain_update_state = available_extensions.fb_swapchain_update_state;
    exts.htc_facial_tracking = available_extensions.htc_facial_tracking;
    exts.htc_passthrough = available_extensions.htc_passthrough;
    exts.htc_vive_focus3_controller_interaction =
        available_extensions.htc_vive_focus3_controller_interaction;
    #[cfg(target_os = "android")]
    {
        exts.khr_android_create_instance = true;
    }
    exts.khr_convert_timespec_time = available_extensions.khr_convert_timespec_time;
    exts.khr_opengl_es_enable = true;
    exts.other = available_extensions
        .other
        .into_iter()
        .filter(|ext| {
            [
                META_BODY_TRACKING_FULL_BODY_EXTENSION_NAME,
                META_SIMULTANEOUS_HANDS_AND_CONTROLLERS_EXTENSION_NAME,
                META_DETACHED_CONTROLLERS_EXTENSION_NAME,
                BD_BODY_TRACKING_EXTENSION_NAME,
                BD_MOTION_TRACKING_EXTENSION_NAME,
                PICO_CONFIGURATION_EXTENSION_NAME,
            ]
            .contains(&ext.as_str())
        })
        .collect();

    info!("[HVR-DIAG] enumerating OpenXR API layers");
    let available_layers = match xr_entry.enumerate_layers() {
        Ok(layers) => layers,
        Err(error) => {
            error!("[HVR-DIAG] failed to enumerate OpenXR API layers: {error:?}");
            return;
        }
    };
    alvr_common::info!("OpenXR available layers: {available_layers:#?}");

    if platform.is_huawei() {
        info!("[HVR-DIAG] creating OpenXR instance with API version 1.0.3");
    } else {
        info!(
            "[HVR-DIAG] creating OpenXR instance with API version {}",
            xr::CURRENT_API_VERSION
        );
    }
    let application_info = if platform.is_huawei() {
        xr::ApplicationInfo {
            application_name: "HelloXR",
            application_version: 0,
            engine_name: "",
            engine_version: 0,
        }
    } else {
        xr::ApplicationInfo {
            application_name: "ALVR Client",
            application_version: 0,
            engine_name: "ALVR",
            engine_version: 0,
        }
    };
    let xr_instance = match xr_entry.create_instance(&application_info, &exts, &[]) {
        Ok(instance) => instance,
        Err(error) => {
            error!("[HVR-DIAG] failed to create OpenXR instance: {error:?}");
            return;
        }
    };
    info!("[HVR-DIAG] OpenXR instance created");

    // Huawei's official demo queries the mandatory GLES requirements before it initializes EGL.
    // The legacy runtime uses this call to prepare the graphics path for the later context binding.
    let huawei_xr_system = if platform.is_huawei() {
        let instance_properties = xr_instance.properties().unwrap();
        info!("[HVR-DIAG] Huawei instance properties: {instance_properties:?}");

        let xr_system = xr_instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .unwrap();
        let system_properties = xr_instance.system_properties(xr_system).unwrap();
        let view_configuration_types = xr_instance
            .enumerate_view_configurations(xr_system)
            .unwrap();
        let stereo_properties = xr_instance
            .view_configuration_properties(xr_system, xr::ViewConfigurationType::PRIMARY_STEREO)
            .unwrap();
        let stereo_views = xr_instance
            .enumerate_view_configuration_views(
                xr_system,
                xr::ViewConfigurationType::PRIMARY_STEREO,
            )
            .unwrap();
        let blend_modes = xr_instance
            .enumerate_environment_blend_modes(xr_system, xr::ViewConfigurationType::PRIMARY_STEREO)
            .unwrap();
        info!("[HVR-DIAG] Huawei system properties: {system_properties:?}");
        info!(
            "[HVR-DIAG] Huawei pre-session view configuration: types={view_configuration_types:?}, stereo={stereo_properties:?}, views={stereo_views:?}, blend_modes={blend_modes:?}"
        );

        let requirements = xr_instance
            .graphics_requirements::<xr::OpenGlEs>(xr_system)
            .unwrap();
        info!(
            "[HVR-DIAG] Huawei GLES requirements queried before EGL: min={:?}, max={:?}",
            requirements.min_api_version_supported, requirements.max_api_version_supported
        );
        Some(xr_system)
    } else {
        None
    };

    #[cfg(target_os = "android")]
    let mut huawei_egl = platform.is_huawei().then(|| {
        info!("[HVR-DIAG] creating official-first Huawei EGL context before wgpu");
        alvr_graphics::HuaweiEglContext::new(
            _huawei_native_window.expect("Huawei VR requires an Android native window"),
        )
    });

    #[cfg(target_os = "android")]
    let mut graphics_context_slot: Option<Rc<GraphicsContext>> = if platform.is_huawei() {
        None
    } else {
        info!("[HVR-DIAG] creating OpenGL ES graphics context");
        Some(Rc::new(GraphicsContext::new_gl()))
    };

    #[cfg(not(target_os = "android"))]
    let mut graphics_context_slot: Option<Rc<GraphicsContext>> = {
        info!("[HVR-DIAG] creating OpenGL ES graphics context");
        Some(Rc::new(GraphicsContext::new_gl()))
    };

    let mut last_lobby_message = String::new();

    'session_loop: loop {
        let xr_system = huawei_xr_system.unwrap_or_else(|| {
            xr_instance
                .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
                .unwrap()
        });

        // mandatory call
        if !platform.is_huawei() {
            let _ = xr_instance
                .graphics_requirements::<xr::OpenGlEs>(xr_system)
                .unwrap();
        }

        let huawei_official_first = platform.is_huawei() && graphics_context_slot.is_none();

        if platform.is_huawei() && !huawei_official_first {
            graphics_context_slot.as_ref().unwrap().make_current();
            #[cfg(target_os = "android")]
            graphics_context_slot
                .as_ref()
                .unwrap()
                .log_current_egl("before xrCreateSession");
        }

        #[cfg(target_os = "android")]
        let (xr_session, mut xr_frame_waiter, mut xr_frame_stream) = if huawei_official_first {
            info!("[HVR-DIAG] creating Huawei OpenXR Session before any wgpu initialization");
            create_huawei_session(&xr_instance, xr_system, huawei_egl.as_ref().unwrap())
        } else {
            create_session(
                &xr_instance,
                xr_system,
                graphics_context_slot.as_ref().unwrap(),
            )
        };

        #[cfg(not(target_os = "android"))]
        let (xr_session, mut xr_frame_waiter, mut xr_frame_stream) = create_session(
            &xr_instance,
            xr_system,
            graphics_context_slot.as_ref().unwrap(),
        );

        let views_config = xr_instance
            .enumerate_view_configuration_views(
                xr_system,
                xr::ViewConfigurationType::PRIMARY_STEREO,
            )
            .unwrap();
        assert_eq!(views_config.len(), 2);

        let default_view_resolution = UVec2::new(
            views_config[0].recommended_image_rect_width,
            views_config[0].recommended_image_rect_height,
        );
        info!(
            "[HVR-DIAG] primary stereo view resolution: {}x{}",
            default_view_resolution.x, default_view_resolution.y
        );

        let huawei_lobby_swapchains = if platform.is_huawei() {
            #[cfg(target_os = "android")]
            if huawei_official_first {
                huawei_egl.as_ref().unwrap().make_current();
            } else {
                graphics_context_slot.as_ref().unwrap().make_current();
            }
            let formats = xr_session.enumerate_swapchain_formats().unwrap();
            info!(
                "[HVR-DIAG] Huawei swapchain formats enumerated before interaction setup: {formats:?}"
            );
            assert!(
                formats.contains(&SDR_FORMAT_GL),
                "Huawei runtime does not advertise the required GL_RGBA8 swapchain format"
            );
            #[cfg(target_os = "android")]
            {
                if huawei_official_first {
                    Some(lobby::create_huawei_swapchains(
                        &xr_session,
                        default_view_resolution,
                    ))
                } else {
                    Some(lobby::create_swapchains(
                        &xr_session,
                        graphics_context_slot.as_ref().unwrap(),
                        default_view_resolution,
                        true,
                    ))
                }
            }

            #[cfg(not(target_os = "android"))]
            unreachable!("Huawei VR Glass is only supported on Android")
        } else {
            None
        };

        #[cfg(target_os = "android")]
        if huawei_official_first {
            info!(
                "[HVR-DIAG] OpenXR swapchains enumerated; adopting the proven Huawei EGL context in wgpu"
            );
            graphics_context_slot = Some(Rc::new(GraphicsContext::from_huawei_egl(
                huawei_egl.take().unwrap(),
            )));
        }

        let graphics_context = Rc::clone(graphics_context_slot.as_ref().unwrap());
        graphics_context.make_current();

        let refresh_rates = if platform.is_huawei() {
            // HVRSDK 3.5 does not expose XR_FB_display_refresh_rate, but the physical Huawei VR
            // display and Runtime compositor both run at 70 Hz. Advertising the generic 90 Hz
            // fallback overloads the Android 10 software decoder and mismatches frame pacing.
            vec![70.0]
        } else if exts.fb_display_refresh_rate {
            xr_session.enumerate_display_refresh_rates().unwrap()
        } else {
            vec![90.0]
        };

        if exts.fb_color_space {
            xr_session
                .set_color_space(xr::ColorSpaceFB::REC709)
                .unwrap();
        }

        let capabilities = ClientCapabilities {
            default_view_resolution,
            refresh_rates,
            // The Huawei direct framebuffer renderer does not yet implement ALVR's fixed
            // foveated encoding inverse mapping. Keep the server on a linear stereo stream.
            foveated_encoding: platform != Platform::Unknown && !platform.is_huawei(),
            encoder_high_profile: platform != Platform::Unknown,
            encoder_10_bits: platform != Platform::Unknown,
            encoder_av1: matches!(
                platform,
                Platform::Quest3 | Platform::Quest3S | Platform::Pico4Ultra
            ),
            prefer_10bit: false,
            prefer_full_range: true,
            preferred_encoding_gamma: 1.0,
            prefer_hdr: false,
        };
        let core_context = Arc::new(ClientCoreContext::new(capabilities));

        let interaction_context = Arc::new(RwLock::new(InteractionContext::new(
            xr_session.clone(),
            exts.other.clone(),
            xr_system,
            platform,
        )));

        let mut lobby = Lobby::new(
            xr_session.clone(),
            Rc::clone(&graphics_context),
            Arc::clone(&interaction_context),
            platform,
            default_view_resolution,
            &last_lobby_message,
            huawei_lobby_swapchains,
        );
        let lobby_body_tracking_config = BodyTrackingSourcesConfig {
            body_tracking_fb: Switch::Disabled,
            body_tracking_bd: Switch::Enabled(BodyTrackingBDConfig::BodyTracking {
                high_accuracy: true,
                prompt_calibration_on_start: false,
            }),
        };
        let lobby_interaction_sources = InteractionSourcesConfig {
            face_tracking: None,
            body_tracking: Some(lobby_body_tracking_config),
            prefers_multimodal_input: true,
        };
        interaction_context
            .write()
            .select_sources(&lobby_interaction_sources);

        let mut session_running = false;
        let mut stream_context = None::<StreamContext>;
        let mut passthrough_layer = None;

        let mut event_storage = xr::EventDataBuffer::new();
        'render_loop: loop {
            #[cfg(target_os = "android")]
            if platform.is_huawei() && HUAWEI_EXIT_REQUESTED.load(Ordering::Acquire) {
                info!("[HVR-DIAG] Huawei Activity requested OpenXR shutdown");
                break 'session_loop;
            }

            while let Some(event) = xr_instance.poll_event(&mut event_storage).unwrap() {
                match event {
                    xr::Event::EventsLost(event) => {
                        error!("OpenXR: lost {} events!", event.lost_event_count());
                    }
                    xr::Event::InstanceLossPending(_) => break 'session_loop,
                    xr::Event::SessionStateChanged(event) => match event.state() {
                        xr::SessionState::READY => {
                            xr_session
                                .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                                .unwrap();

                            core_context.resume();

                            passthrough_layer = PassthroughLayer::new(&xr_session, platform).ok();

                            session_running = true;
                        }
                        xr::SessionState::STOPPING => {
                            session_running = false;

                            passthrough_layer = None;

                            core_context.pause();

                            xr_session.end().unwrap();
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            break 'render_loop
                        }
                        _ => (),
                    },
                    xr::Event::ReferenceSpaceChangePending(event) => {
                        info!(
                            "ReferenceSpaceChangePending type: {:?}",
                            event.reference_space_type()
                        );

                        lobby.update_reference_space();

                        if let Some(stream) = &mut stream_context {
                            stream.update_reference_space();
                        }
                    }
                    xr::Event::PerfSettingsEXT(event) => {
                        info!(
                            "Perf: from {:?} to {:?}, domain: {:?}/{:?}",
                            event.from_level(),
                            event.to_level(),
                            event.domain(),
                            event.sub_domain(),
                        );
                    }
                    xr::Event::InteractionProfileChanged(_)
                    | xr::Event::PassthroughStateChangedFB(_) => {
                        // todo
                    }
                    _ => (),
                }
            }

            if !session_running {
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            while let Some(event) = core_context.poll_event() {
                match event {
                    ClientCoreEvent::UpdateHudMessage(message) => {
                        last_lobby_message.clone_from(&message);
                        lobby.update_hud_message(&message);
                    }
                    ClientCoreEvent::StreamingStarted(config) => {
                        let config = ParsedStreamConfig::new(&config);

                        let context = StreamContext::new(
                            Arc::clone(&core_context),
                            xr_session.clone(),
                            Rc::clone(&graphics_context),
                            Arc::clone(&interaction_context),
                            platform,
                            config,
                        );

                        if !context.uses_passthrough() {
                            passthrough_layer = None;
                        }

                        stream_context = Some(context);
                    }
                    ClientCoreEvent::StreamingStopped => {
                        if passthrough_layer.is_none() {
                            passthrough_layer = PassthroughLayer::new(&xr_session, platform).ok();
                        }

                        interaction_context
                            .write()
                            .select_sources(&lobby_interaction_sources);

                        stream_context = None;
                    }
                    ClientCoreEvent::Haptics {
                        device_id,
                        duration,
                        frequency,
                        amplitude,
                    } => {
                        let idx = if device_id == *HAND_LEFT_ID { 0 } else { 1 };
                        let interaction_context_lock = interaction_context.read();
                        let hand_interaction = &interaction_context_lock.hands_interaction[idx];

                        hand_interaction
                            .vibration_action
                            .apply_feedback(
                                &xr_session,
                                hand_interaction.subaction_path,
                                &xr::HapticVibration::new()
                                    .amplitude(amplitude.clamp(0.0, 1.0))
                                    .frequency(frequency.max(0.0))
                                    .duration(xr::Duration::from_nanos(duration.as_nanos() as _)),
                            )
                            .unwrap();
                    }
                    ClientCoreEvent::DecoderConfig { codec, config_nal } => {
                        if let Some(stream) = &mut stream_context {
                            stream.maybe_initialize_decoder(codec, config_nal);
                        }
                    }
                    ClientCoreEvent::RealTimeConfig(config) => {
                        if config.passthrough.is_some() && passthrough_layer.is_none() {
                            passthrough_layer = PassthroughLayer::new(&xr_session, platform).ok();
                        } else if config.passthrough.is_none() && passthrough_layer.is_some() {
                            passthrough_layer = None;
                        }

                        if let Some(stream) = &mut stream_context {
                            stream.update_real_time_config(&config);
                        }
                    }
                }
            }

            let frame_state = match xr_frame_waiter.wait() {
                Ok(state) => state,
                Err(e) => {
                    error!("{e}");
                    panic!();
                }
            };
            let frame_interval =
                Duration::from_nanos(frame_state.predicted_display_period.as_nanos() as _);
            let vsync_time =
                Duration::from_nanos(frame_state.predicted_display_time.as_nanos() as _);

            xr_frame_stream.begin().unwrap();

            if !frame_state.should_render {
                xr_frame_stream
                    .end(
                        frame_state.predicted_display_time,
                        xr::EnvironmentBlendMode::OPAQUE,
                        &[],
                    )
                    .unwrap();

                continue;
            }

            // todo: allow rendering lobby and stream layers at the same time and add cross fade
            let (layer, display_time) = if let Some(stream) = &mut stream_context {
                stream.render(frame_interval, vsync_time)
            } else {
                (lobby.render(vsync_time), vsync_time)
            };

            let layers: &[&xr::CompositionLayerBase<_>] =
                if let Some(passthrough_layer) = &passthrough_layer {
                    &[passthrough_layer, &layer.build()]
                } else {
                    &[&layer.build()]
                };

            // Huawei's legacy runtime moves the application EGL context to its compositor after
            // releasing the acquired images. The official sample calls xrEndFrame directly;
            // trying to reclaim the context here fails with EGL_BAD_ACCESS.
            if !platform.is_huawei() {
                graphics_context.make_current();
            }
            let res = xr_frame_stream.end(
                to_xr_time(display_time),
                xr::EnvironmentBlendMode::OPAQUE,
                layers,
            );

            if let Err(e) = res {
                let time = to_xr_time(display_time);
                error!("End frame failed! {e}, timestamp: {display_time:?}, time: {time:?}");

                if !platform.is_vive() {
                    xr_frame_stream
                        .end(
                            frame_state.predicted_display_time,
                            xr::EnvironmentBlendMode::OPAQUE,
                            &[],
                        )
                        .unwrap();
                }
            }
        }
    }

    // grapics_context is dropped here
}

#[allow(unused)]
fn xr_runtime_now(xr_instance: &xr::Instance) -> Option<xr::Time> {
    if xr_instance.exts().khr_convert_timespec_time.is_none() {
        return None;
    }

    xr_instance
        .now()
        .ok()
        .filter(|&time_nanos| time_nanos.as_nanos() > 0)
}

#[cfg(target_os = "android")]
fn spawn_rendering_thread(
    huawei_window: Option<ndk::native_window::NativeWindow>,
) -> thread::JoinHandle<()> {
    if huawei_window.is_some() {
        HUAWEI_EXIT_REQUESTED.store(false, Ordering::Release);
    }

    thread::Builder::new()
        .name("alvr-render".into())
        .spawn(move || {
            info!("[HVR-DIAG] render thread started");

            // workaround for the Pico runtime
            let context = ndk_context::android_context();
            info!("[HVR-DIAG] Android NDK context acquired");
            let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }.unwrap();
            info!("[HVR-DIAG] Java VM acquired");
            let _env = vm.attach_current_thread().unwrap();
            info!("[HVR-DIAG] render thread attached to Java VM");

            if let Some(native_window) = huawei_window {
                let native_window_ptr = native_window.ptr().as_ptr().cast();
                info!(
                    "[HVR-DIAG] using Huawei Android native window: {}x{}",
                    native_window.width(),
                    native_window.height()
                );
                entry_point_with_window(Some(native_window_ptr));
                drop(native_window);
            } else {
                entry_point();
            }
            info!("[HVR-DIAG] OpenXR entry point returned");
        })
        .unwrap()
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    use android_activity::{InputStatus, MainEvent, PollEvent};

    alvr_client_core::init_logging();
    info!("[HVR-DIAG] android_main entered");

    let is_huawei = alvr_system_info::platform().is_huawei();

    if is_huawei {
        info!("[HVR-DIAG] initializing Huawei VR SDK Service bridge");
        if let Err(error) = huawei_runtime::initialize() {
            error!("[HVR-DIAG] Huawei VR SDK Service bridge initialization failed: {error}");
            return;
        }
        info!("[HVR-DIAG] Huawei VR SDK Service bridge initialized");
    }

    let mut rendering_thread = (!is_huawei).then(|| spawn_rendering_thread(None));

    let mut should_quit = false;
    while !should_quit {
        app.poll_events(Some(Duration::from_millis(100)), |event| match event {
            PollEvent::Main(MainEvent::InitWindow { .. }) if is_huawei => {
                if rendering_thread.is_none() {
                    if let Some(native_window) = app.native_window() {
                        info!("[HVR-DIAG] Huawei InitWindow received");
                        rendering_thread = Some(spawn_rendering_thread(Some(native_window)));
                    } else {
                        error!("[HVR-DIAG] Huawei InitWindow has no native window");
                    }
                }
            }
            PollEvent::Main(MainEvent::Destroy) => {
                should_quit = true;
            }
            PollEvent::Main(MainEvent::InputAvailable) => {
                if let Ok(mut iter) = app.input_events_iter() {
                    while iter.next(|_| InputStatus::Unhandled) {}
                }
            }
            _ => (),
        });
    }

    // Note: the quit event is sent from OpenXR too, this will return rather quicly.
    if let Some(rendering_thread) = rendering_thread {
        rendering_thread.join().unwrap();
    }
}
