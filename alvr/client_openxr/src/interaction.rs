use crate::extra_extensions::MotionTrackerBD;
// FaceTracker2FB needs XR_FB_face_tracking2, which the Huawei OpenXR bindings do not provide.
#[cfg(not(target_os = "android"))]
use crate::extra_extensions::FaceTracker2FB;
use crate::{
    extra_extensions::{
        self, BodyJointSetBD, BodyTrackerBD, BodyTrackerFB, EyeTrackerSocial, FaceTrackerPico,
        FacialTrackerHTC, MultimodalMeta, BODY_JOINT_LEFT_ELBOW_BD,
        BODY_JOINT_LEFT_FOOT_BD, BODY_JOINT_LEFT_KNEE_BD, BODY_JOINT_PELVIS_BD,
        BODY_JOINT_RIGHT_ELBOW_BD, BODY_JOINT_RIGHT_FOOT_BD, BODY_JOINT_RIGHT_KNEE_BD,
        BODY_JOINT_SET_FULL_BODY_META, BODY_JOINT_SPINE3_BD, FULL_BODY_JOINT_COUNT_META,
        FULL_BODY_JOINT_LEFT_FOOT_BALL_META, FULL_BODY_JOINT_LEFT_LOWER_LEG_META,
        FULL_BODY_JOINT_RIGHT_FOOT_BALL_META, FULL_BODY_JOINT_RIGHT_LOWER_LEG_META,
    },
    huawei_input, Platform,
    huawei_session::HuaweiTrackingDegree,
};
use alvr_common::{
    glam::{Quat, Vec3},
    *,
};
use alvr_packets::{ButtonEntry, ButtonValue, StreamConfig};
use alvr_session::{BodyTrackingBDConfig, BodyTrackingSourcesConfig, FaceTrackingSourcesConfig};
use openxr as xr;
use std::{collections::HashMap, time::Duration};
use xr::SpaceLocationFlags;

const IPD_CHANGE_EPS: f32 = 0.001;
const FOV_CHANGE_EPS: f32 = 0.001;

// Most OpenXR runtime, including Meta's one, do not follow perfectly the specification regarding
// controller pose. The Z axis should point down through the center of the controller grip, the X
// axis should go out perpendicular from the palm, and the position should be aligned roughtly with
// the center of the palm.
// https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html
// Note: right controller offsets are calculated from left controller offsets by mirroring along the
// Y-Z plane.
fn get_controller_offset(platform: Platform, is_right_hand: bool) -> Pose {
    const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;

    let left_offset = match platform {
        Platform::Quest1 => Pose {
            position: Vec3::new(-0.013, -0.005, 0.0),
            orientation: Quat::from_rotation_x(-20.0 * DEG_TO_RAD),
        },
        // todo: check Quest 2
        p if p.is_quest() => Pose {
            position: Vec3::new(-0.005, -0.005, 0.00),
            orientation: Quat::from_rotation_x(-15.0 * DEG_TO_RAD),
        },
        Platform::PicoNeo3 => Pose {
            position: Vec3::new(-0.013, -0.035, 0.0),
            orientation: Quat::IDENTITY,
        },
        // todo: check (base) Pico 4
        p if p.is_pico() => Pose {
            position: Vec3::new(-0.01, -0.035, 0.0),
            orientation: Quat::from_rotation_y(6.0 * DEG_TO_RAD)
                * Quat::from_rotation_x(-6.0 * DEG_TO_RAD),
        },
        p if p.is_vive() => Pose {
            position: Vec3::new(0.0, 0.0, -0.02),
            orientation: Quat::IDENTITY,
        },
        _ => Pose::default(),
    };

    if is_right_hand {
        let p = left_offset.position;
        let q = left_offset.orientation;

        Pose {
            position: Vec3::new(-p[0], p[1], p[2]),
            orientation: Quat::from_xyzw(-q.x, q.y, q.z, -q.w),
        }
    } else {
        left_offset
    }
}

fn create_ext_object<T>(
    name: &str,
    enabled: Option<bool>,
    create_cb: impl FnOnce() -> xr::Result<T>,
) -> Option<T> {
    enabled
        .unwrap_or(false)
        .then(|| match create_cb() {
            Ok(obj) => Some(obj),
            Err(xr::sys::Result::ERROR_FEATURE_UNSUPPORTED) => {
                warn!("Cannot create unsupported {name}");
                None
            }
            Err(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT) => None,
            Err(e) => {
                warn!("Failed to create {name}: {e}");
                None
            }
        })
        .flatten()
}

pub enum ButtonAction {
    Binary {
        action: xr::Action<bool>,
        subaction_path: xr::Path,
    },
    Scalar {
        action: xr::Action<f32>,
        subaction_path: xr::Path,
    },
}

pub struct HandInteraction {
    pub controllers_profile_id: u64,
    pub pose_offset: Pose,
    pub subaction_path: xr::Path,

    pub grip_action: xr::Action<xr::Posef>,
    pub grip_space: xr::Space,

    #[expect(dead_code)]
    pub aim_action: xr::Action<xr::Posef>,
    #[expect(dead_code)]
    pub aim_space: xr::Space,

    pub vibration_action: xr::Action<xr::Haptic>,
    pub skeleton_tracker: Option<xr::HandTracker>,
}

pub struct FaceSources {
    pub combined_eyes_source: Option<(xr::Action<xr::Posef>, xr::Space)>,
    pub eye_tracker_fb: Option<EyeTrackerSocial>,
    #[cfg(not(target_os = "android"))]
    pub face_tracker_fb: Option<FaceTracker2FB>,
    pub eye_tracker_htc: Option<FacialTrackerHTC>,
    pub lip_tracker_htc: Option<FacialTrackerHTC>,
    pub face_tracker_pico: Option<FaceTrackerPico>,
}

pub struct BodySources {
    pub body_tracker_fb: Option<(BodyTrackerFB, usize)>,
    pub body_tracker_bd: Option<BodyTrackerBD>,
    pub motion_tracker_bd: Option<MotionTrackerBD>,
}

#[derive(Clone)]
pub struct InteractionSourcesConfig {
    pub face_tracking: Option<FaceTrackingSourcesConfig>,
    pub body_tracking: Option<BodyTrackingSourcesConfig>,
    pub prefers_multimodal_input: bool,
}

impl InteractionSourcesConfig {
    pub fn new(config: &StreamConfig) -> Self {
        Self {
            face_tracking: config
                .settings
                .headset
                .face_tracking
                .as_option()
                .map(|c| c.sources.clone()),
            body_tracking: config
                .settings
                .headset
                .body_tracking
                .as_option()
                .map(|c| c.sources.clone()),
            prefers_multimodal_input: config
                .settings
                .headset
                .controllers
                .as_option()
                .is_some_and(|c| c.multimodal_tracking),
        }
    }
}

pub struct InteractionContext {
    xr_session: xr::Session<xr::OpenGlEs>,
    xr_system: xr::SystemId,
    extra_extensions: Vec<String>,
    platform: Platform,
    pub action_set: xr::ActionSet,
    pub button_actions: HashMap<u64, ButtonAction>,
    pub hands_interaction: [HandInteraction; 2],
    multimodal_handle: Option<MultimodalMeta>,
    pub multimodal_hands_enabled: bool,
    pub face_sources: FaceSources,
    pub body_sources: BodySources,
}

impl InteractionContext {
    pub fn new(
        xr_session: xr::Session<xr::OpenGlEs>,
        extra_extensions: Vec<String>,
        xr_system: xr::SystemId,
        platform: Platform,
    ) -> Self {
        let xr_instance = xr_session.instance();

        let action_set = xr_instance
            .create_action_set("alvr_interaction", "ALVR interaction", 0)
            .unwrap();

        let mut bindings = vec![];

        let left_hand_path = xr_instance.string_to_path(HAND_LEFT_PATH).unwrap();
        let right_hand_path = xr_instance.string_to_path(HAND_RIGHT_PATH).unwrap();
        let left_hand_subaction_paths = [left_hand_path];
        let right_hand_subaction_paths = [right_hand_path];
        let both_hand_subaction_paths = [left_hand_path, right_hand_path];
        let left_action_subaction_paths = if platform.is_huawei() {
            left_hand_subaction_paths.as_slice()
        } else {
            &[]
        };
        let right_action_subaction_paths = if platform.is_huawei() {
            right_hand_subaction_paths.as_slice()
        } else {
            &[]
        };

        fn binding<'a, T: xr::ActionTy>(action: &'a xr::Action<T>, path: &str) -> xr::Binding<'a> {
            xr::Binding::new(action, action.instance().string_to_path(path).unwrap())
        }

        // The Huawei runtime may not expose every path we would like to bind, and a single
        // unsupported path makes the whole suggestion fail. Registering each binding in a "core"
        // and a "minimal" list too allows retrying with a smaller set.
        fn push_huawei_binding<'a>(
            enabled: bool,
            priority: u8,
            binding: xr::Binding<'a>,
            all: &mut Vec<xr::Binding<'a>>,
            core: &mut Vec<xr::Binding<'a>>,
            minimal: &mut Vec<xr::Binding<'a>>,
        ) {
            if !enabled {
                return;
            }
            all.push(binding);
            match priority {
                0 => {
                    core.push(binding);
                    minimal.push(binding);
                }
                1 => core.push(binding),
                _ => (),
            }
        }

        // Huawei VR Glass can run two different session types: 6DoF (with the visual tracking kit
        // and its two tracked controllers) and 3DoF (glasses IMU only, driven by the small 3DoF
        // remote). The session type is selected by the APK itself (see huawei_session), and the
        // runtime only accepts bindings for the profile that matches it. Suggesting both crashes
        // inside libvrsdk.so!hxrFindProfileForDevice while attaching the action sets, so decide
        // here which single profile to use.
        let huawei_tracking_degree = platform
            .is_huawei()
            .then(crate::huawei_session::detect_tracking_degree)
            .flatten();

        let controllers_profile_path = match platform {
            p if p.is_quest() => QUEST_CONTROLLER_PROFILE_PATH, // todo: create new controller profile for quest pro and 3
            Platform::PicoG3 => PICO_G3_CONTROLLER_PROFILE_PATH,
            Platform::PicoNeo3 => PICO_NEO3_CONTROLLER_PROFILE_PATH,
            Platform::Pico4Ultra => PICO4S_CONTROLLER_PROFILE_PATH,
            Platform::Pico4 | Platform::Pico4Pro | Platform::Pico4Enterprise => {
                PICO4_CONTROLLER_PROFILE_PATH
            }
            p if p.is_pico() => PICO4S_CONTROLLER_PROFILE_PATH,
            p if p.is_vive() => FOCUS3_CONTROLLER_PROFILE_PATH,
            p if p.is_yvr() => YVR_CONTROLLER_PROFILE_PATH,
            Platform::HuaweiVrGlass => match huawei_tracking_degree {
                Some(HuaweiTrackingDegree::ThreeDof) => HUAWEI_3DOF_CONTROLLER_PROFILE_PATH,
                // Unknown: keep binding the tracking kit profile, as before.
                None | Some(HuaweiTrackingDegree::SixDof) => HUAWEI_CONTROLLER_PROFILE_PATH,
            },
            _ => QUEST_CONTROLLER_PROFILE_PATH,
        };
        let controllers_profile_id = alvr_common::hash_string(controllers_profile_path);
        let profile_button_set = &CONTROLLER_PROFILE_INFO
            .get(&controllers_profile_id)
            .unwrap()
            .button_set;
        let huawei_action_name = |path: &str| {
            path[1..]
                .replace('/', "_")
                .replace("user_hand_left_", "user_hand_")
                .replace("user_hand_right_", "user_hand_")
        };
        let mut huawei_action_counts = HashMap::new();
        if platform.is_huawei() {
            for button_id in profile_button_set {
                let info = BUTTON_INFO.get(button_id).unwrap();
                *huawei_action_counts
                    .entry(huawei_action_name(info.path))
                    .or_insert(0_usize) += 1;
            }
        }

        let mut huawei_binary_actions = HashMap::<String, xr::Action<bool>>::new();
        let mut huawei_scalar_actions = HashMap::<String, xr::Action<f32>>::new();
        let mut button_actions = HashMap::new();
        for button_id in profile_button_set {
            let info = BUTTON_INFO.get(button_id).unwrap();

            let name = if platform.is_huawei() {
                huawei_action_name(info.path)
            } else {
                info.path[1..].replace('/', "_")
            };
            let display_name = format!(
                "{}{}",
                name[0..1].to_uppercase(),
                name[1..].replace('_', " ")
            );

            let subaction_path = if platform.is_huawei() {
                if info.device_id == *HAND_LEFT_ID {
                    left_hand_path
                } else {
                    right_hand_path
                }
            } else {
                xr::Path::NULL
            };
            let subaction_paths = if platform.is_huawei()
                && huawei_action_counts.get(&name).copied().unwrap_or(0) > 1
            {
                both_hand_subaction_paths.as_slice()
            } else if subaction_path == xr::Path::NULL {
                &[][..]
            } else {
                std::slice::from_ref(&subaction_path)
            };

            let action = match info.button_type {
                ButtonType::Binary => ButtonAction::Binary {
                    action: if platform.is_huawei() {
                        if let Some(action) = huawei_binary_actions.get(&name) {
                            action.clone()
                        } else {
                            let action = action_set
                                .create_action(&name, &display_name, subaction_paths)
                                .unwrap();
                            huawei_binary_actions.insert(name.clone(), action.clone());
                            action
                        }
                    } else {
                        action_set
                            .create_action(&name, &display_name, subaction_paths)
                            .unwrap()
                    },
                    subaction_path,
                },
                ButtonType::Scalar => ButtonAction::Scalar {
                    action: if platform.is_huawei() {
                        if let Some(action) = huawei_scalar_actions.get(&name) {
                            action.clone()
                        } else {
                            let action = action_set
                                .create_action(&name, &display_name, subaction_paths)
                                .unwrap();
                            huawei_scalar_actions.insert(name.clone(), action.clone());
                            action
                        }
                    } else {
                        action_set
                            .create_action(&name, &display_name, subaction_paths)
                            .unwrap()
                    },
                    subaction_path,
                },
            };
            button_actions.insert(*button_id, action);
        }

        let (left_grip_action, right_grip_action, left_aim_action, right_aim_action) =
            if platform.is_huawei() {
                // Huawei's legacy runtime does not reliably distinguish two action spaces that
                // originate from one pose action with left/right subactions. Keep one pose action
                // per hand, while reusing it for that hand's grip and aim spaces, so both
                // controllers get independent tracking without creating four runtime pose actions.
                let left_action = action_set
                    .create_action(
                        "left_controller_aim_pose",
                        "Left controller aim pose",
                        &left_hand_subaction_paths,
                    )
                    .unwrap();
                let right_action = action_set
                    .create_action(
                        "right_controller_aim_pose",
                        "Right controller aim pose",
                        &right_hand_subaction_paths,
                    )
                    .unwrap();
                (
                    left_action.clone(),
                    right_action.clone(),
                    left_action,
                    right_action,
                )
            } else {
                (
                    action_set
                        .create_action(
                            "left_grip_pose",
                            "Left grip pose",
                            left_action_subaction_paths,
                        )
                        .unwrap(),
                    action_set
                        .create_action(
                            "right_grip_pose",
                            "Right grip pose",
                            right_action_subaction_paths,
                        )
                        .unwrap(),
                    action_set
                        .create_action(
                            "left_aim_pose",
                            "Left aim pose",
                            left_action_subaction_paths,
                        )
                        .unwrap(),
                    action_set
                        .create_action(
                            "right_aim_pose",
                            "Right aim pose",
                            right_action_subaction_paths,
                        )
                        .unwrap(),
                )
            };

        let (left_vibration_action, right_vibration_action) = if platform.is_huawei() {
            let action = action_set
                .create_action(
                    "controller_haptic",
                    "Controller haptic",
                    &both_hand_subaction_paths,
                )
                .unwrap();
            (action.clone(), action)
        } else {
            (
                action_set
                    .create_action(
                        "left_hand_vibration",
                        "Left hand vibration",
                        left_action_subaction_paths,
                    )
                    .unwrap(),
                action_set
                    .create_action(
                        "right_hand_vibration",
                        "Right hand vibration",
                        right_action_subaction_paths,
                    )
                    .unwrap(),
            )
        };

        // Create action bindings:

        // On Huawei, one unsupported path rejects the whole suggestion, so every binding is also
        // recorded in a "core" and a "minimal" list: if the runtime refuses the full set, the
        // profile is retried with the smaller ones instead of losing all input.
        let mut bindings_core = vec![];
        let mut bindings_minimal = vec![];

        // Which buttons matter the most on Huawei, whatever the session type: trigger and home
        // (system) are certain, the thumbstick/trackpad comes next, everything else is optional.
        let huawei_binding_priority = |id: &u64| -> u8 {
            if *id == *LEFT_TRIGGER_CLICK_ID
                || *id == *RIGHT_TRIGGER_CLICK_ID
                || *id == *LEFT_TRIGGER_VALUE_ID
                || *id == *RIGHT_TRIGGER_VALUE_ID
                || *id == *LEFT_SYSTEM_CLICK_ID
                || *id == *RIGHT_SYSTEM_CLICK_ID
            {
                0
            } else if *id == *LEFT_TRACKPAD_X_ID
                || *id == *RIGHT_TRACKPAD_X_ID
                || *id == *LEFT_TRACKPAD_Y_ID
                || *id == *RIGHT_TRACKPAD_Y_ID
                || *id == *LEFT_THUMBSTICK_X_ID
                || *id == *RIGHT_THUMBSTICK_X_ID
                || *id == *LEFT_THUMBSTICK_Y_ID
                || *id == *RIGHT_THUMBSTICK_Y_ID
            {
                1
            } else {
                2 // menu / back / touch, drop these before anything else
            }
        };

        for (id, action) in &button_actions {
            let standard_path = BUTTON_INFO.get(id).unwrap().path;
            let path = if platform.is_huawei() {
                standard_path
                    .replace("/input/squeeze/", "/input/grip/")
                    .replace("/input/system/click", "/input/home/click")
            } else {
                standard_path.to_owned()
            };
            let priority = if platform.is_huawei() {
                huawei_binding_priority(id)
            } else {
                u8::MAX
            };
            match action {
                ButtonAction::Binary { action, .. } => {
                    push_huawei_binding(
                        platform.is_huawei(),
                        priority,
                        binding(action, &path),
                        &mut bindings,
                        &mut bindings_core,
                        &mut bindings_minimal,
                    );
                }
                ButtonAction::Scalar { action, .. } => {
                    push_huawei_binding(
                        platform.is_huawei(),
                        priority,
                        binding(action, &path),
                        &mut bindings,
                        &mut bindings_core,
                        &mut bindings_minimal,
                    );
                }
            }
        }

        let left_tracking_pose_path = if platform.is_huawei() {
            "/user/hand/left/input/aim/pose"
        } else {
            "/user/hand/left/input/grip/pose"
        };
        let right_tracking_pose_path = if platform.is_huawei() {
            "/user/hand/right/input/aim/pose"
        } else {
            "/user/hand/right/input/grip/pose"
        };
        // Poses and haptics belong to every hand based profile, but they are kept apart from the
        // button bindings so a Huawei profile can be retried without them.
        let mut bindings_pose = vec![];
        if platform.is_huawei() {
            bindings_pose.push(binding(&left_grip_action, left_tracking_pose_path));
            bindings_pose.push(binding(&right_grip_action, right_tracking_pose_path));
        } else {
            bindings.push(binding(&left_grip_action, left_tracking_pose_path));
            bindings.push(binding(&right_grip_action, right_tracking_pose_path));
            bindings.push(binding(&left_aim_action, "/user/hand/left/input/aim/pose"));
            bindings.push(binding(
                &right_aim_action,
                "/user/hand/right/input/aim/pose",
            ));
        }

        bindings.push(binding(
            &left_vibration_action,
            "/user/hand/left/output/haptic",
        ));
        bindings.push(binding(
            if platform.is_huawei() {
                &left_vibration_action
            } else {
                &right_vibration_action
            },
            "/user/hand/right/output/haptic",
        ));
        if platform.is_huawei() {
            bindings_pose.push(binding(
                &left_vibration_action,
                "/user/hand/left/output/haptic",
            ));
            bindings_pose.push(binding(
                &left_vibration_action,
                "/user/hand/right/output/haptic",
            ));
        }

        let multimodal_handle = create_ext_object("MultimodalMeta", Some(true), || {
            MultimodalMeta::new(xr_session.clone(), &extra_extensions, xr_system)
        });

        let left_detached_controller_pose_action;
        let right_detached_controller_pose_action;
        if multimodal_handle.is_some() {
            // Note: when multimodal input is enabled, both controllers and hands will always be active.
            // To be able to detect when controllers are actually held, we have to register detached
            // controllers pose; the controller pose will be diverted to the detached controllers when
            // they are not held. Currently the detached controllers pose is ignored
            left_detached_controller_pose_action = action_set
                .create_action::<xr::Posef>(
                    "left_detached_controller_pose",
                    "Left detached controller pose",
                    &[],
                )
                .unwrap();
            right_detached_controller_pose_action = action_set
                .create_action::<xr::Posef>(
                    "right_detached_controller_pose",
                    "Right detached controller pose",
                    &[],
                )
                .unwrap();

            bindings.push(binding(
                &left_detached_controller_pose_action,
                "/user/detached_controller_meta/left/input/grip/pose",
            ));
            bindings.push(binding(
                &right_detached_controller_pose_action,
                "/user/detached_controller_meta/right/input/grip/pose",
            ));
        }

        // Apply bindings:
        if platform.is_huawei() {
            // Only ever bind the profile matching the session the runtime started. Suggesting a
            // second one crashes inside libvrsdk.so!hxrFindProfileForDevice while the action sets
            // are attached. Every path of a suggestion must be supported, otherwise the runtime
            // rejects the whole profile, so walk from the richest set down to the bare minimum.
            let profile_path = xr_instance.string_to_path(controllers_profile_path).unwrap();
            let attempts: [(usize, Vec<xr::Binding>); 4] = [
                (
                    bindings.len() + bindings_pose.len(),
                    bindings
                        .iter()
                        .copied()
                        .chain(bindings_pose.iter().copied())
                        .collect(),
                ),
                (
                    bindings_core.len() + bindings_pose.len(),
                    bindings_core
                        .iter()
                        .copied()
                        .chain(bindings_pose.iter().copied())
                        .collect(),
                ),
                (
                    bindings_minimal.len() + bindings_pose.len(),
                    bindings_minimal
                        .iter()
                        .copied()
                        .chain(bindings_pose.iter().copied())
                        .collect(),
                ),
                (bindings_minimal.len(), bindings_minimal.clone()),
            ];
            let mut accepted = false;
            for (attempt_index, (binding_count, bindings)) in attempts.into_iter().enumerate() {
                if binding_count == 0 {
                    continue;
                }
                match xr_instance.suggest_interaction_profile_bindings(profile_path, &bindings) {
                    Ok(()) => {
                        accepted = true;
                        info!(
                            "Huawei {controllers_profile_path} bound ({huawei_tracking_degree:?}, attempt {attempt_index}, {binding_count} bindings)."
                        );
                        break;
                    }
                    Err(e) => warn!(
                        "Huawei {controllers_profile_path} attempt {attempt_index} rejected ({e:?})."
                    ),
                }
            }
            if !accepted {
                warn!(
                    "Huawei {controllers_profile_path} could not be bound: no controller input will reach SteamVR."
                );
            }
        } else {
            xr_instance
                .suggest_interaction_profile_bindings(
                    xr_instance
                        .string_to_path(controllers_profile_path)
                        .unwrap(),
                    &bindings,
                )
                .unwrap();
        }

        // Pico headsets require calling get_system_properties to test for extensions, because all
        // extensions function pointers are available even if the feature is not supported by the
        // hardware. The full checks are done in supports_eye_gaze_interaction. This is required
        // to avoid a crash when requesting the EYE_TRACKING permission.
        let combined_eyes_source = if !platform.is_quest()
            && !platform.is_vive()
            && extra_extensions::supports_eye_gaze_interaction(&xr_session, xr_system)
        {
            // todo: research Pico Neo 3 Pro Eye platform detection
            #[cfg(target_os = "android")]
            if matches!(
                platform,
                Platform::PicoNeo3 | Platform::Pico4Pro | Platform::Pico4Enterprise
            ) {
                alvr_system_info::try_get_permission("com.picovr.permission.EYE_TRACKING")
            }

            let action = action_set
                .create_action("combined_eye_gaze", "Combined eye gaze", &[])
                .unwrap();

            let res = xr_instance.suggest_interaction_profile_bindings(
                xr_instance
                    .string_to_path("/interaction_profiles/ext/eye_gaze_interaction")
                    .unwrap(),
                &[binding(&action, "/user/eyes_ext/input/gaze_ext/pose")],
            );
            if res.is_err() {
                warn!("Failed to register combined eye gaze input: {res:?}");
            }

            let space = action
                .create_space(xr_session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)
                .unwrap();

            Some((action, space))
        } else {
            None
        };

        xr_session.attach_action_sets(&[&action_set]).unwrap();

        // Diagnostic only: the runtime reports which profile it actually selected. Comparing it
        // with the one bound above tells whether the session degree was guessed correctly.
        if platform.is_huawei() {
            for (name, path) in [("left", left_hand_path), ("right", right_hand_path)] {
                match xr_session.current_interaction_profile(path) {
                    Ok(profile) if profile == xr::Path::NULL => warn!(
                        "[HVR-DIAG] Huawei runtime reports no interaction profile for the {name} hand."
                    ),
                    Ok(profile) => info!(
                        "[HVR-DIAG] Huawei runtime active profile for the {name} hand: {} (bound: {controllers_profile_path}).",
                        xr_instance
                            .path_to_string(profile)
                            .unwrap_or_else(|_| "<unknown>".into())
                    ),
                    Err(e) => {
                        warn!("[HVR-DIAG] Huawei active profile query failed for the {name} hand: {e:?}")
                    }
                }
            }
        }

        let left_grip_space = left_grip_action
            .create_space(
                xr_session.clone(),
                if platform.is_huawei() {
                    left_hand_path
                } else {
                    xr::Path::NULL
                },
                xr::Posef::IDENTITY,
            )
            .unwrap();
        let right_grip_space = right_grip_action
            .create_space(
                xr_session.clone(),
                if platform.is_huawei() {
                    right_hand_path
                } else {
                    xr::Path::NULL
                },
                xr::Posef::IDENTITY,
            )
            .unwrap();

        let left_aim_space = left_aim_action
            .create_space(
                xr_session.clone(),
                if platform.is_huawei() {
                    left_hand_path
                } else {
                    xr::Path::NULL
                },
                xr::Posef::IDENTITY,
            )
            .unwrap();
        let right_aim_space = right_aim_action
            .create_space(
                xr_session.clone(),
                if platform.is_huawei() {
                    right_hand_path
                } else {
                    xr::Path::NULL
                },
                xr::Posef::IDENTITY,
            )
            .unwrap();

        let left_hand_tracker = create_ext_object("HandTracker (left)", Some(true), || {
            xr_session.create_hand_tracker(xr::Hand::LEFT)
        });
        let right_hand_tracker = create_ext_object("HandTracker (right)", Some(true), || {
            xr_session.create_hand_tracker(xr::Hand::RIGHT)
        });

        // Note: HTC facial tracking can only be created at startup before xrBeginSession. We don't
        // know the reason.
        let eye_tracker_htc = create_ext_object("FacialTrackerHTC (eyes)", Some(true), || {
            FacialTrackerHTC::new(
                xr_session.clone(),
                xr_system,
                xr::FacialTrackingTypeHTC::EYE_DEFAULT,
            )
        });
        let lip_tracker_htc = create_ext_object("FacialTrackerHTC (lips)", Some(true), || {
            FacialTrackerHTC::new(
                xr_session.clone(),
                xr_system,
                xr::FacialTrackingTypeHTC::LIP_DEFAULT,
            )
        });

        Self {
            xr_session,
            xr_system,
            extra_extensions,
            platform,
            action_set,
            button_actions,
            hands_interaction: [
                HandInteraction {
                    controllers_profile_id,
                    pose_offset: get_controller_offset(platform, false),
                    subaction_path: if platform.is_huawei() {
                        left_hand_path
                    } else {
                        xr::Path::NULL
                    },
                    grip_action: left_grip_action,
                    grip_space: left_grip_space,
                    aim_action: left_aim_action,
                    aim_space: left_aim_space,
                    vibration_action: left_vibration_action,
                    skeleton_tracker: left_hand_tracker,
                },
                HandInteraction {
                    controllers_profile_id,
                    pose_offset: get_controller_offset(platform, true),
                    subaction_path: if platform.is_huawei() {
                        right_hand_path
                    } else {
                        xr::Path::NULL
                    },
                    grip_action: right_grip_action,
                    grip_space: right_grip_space,
                    aim_action: right_aim_action,
                    aim_space: right_aim_space,
                    vibration_action: right_vibration_action,
                    skeleton_tracker: right_hand_tracker,
                },
            ],
            multimodal_handle,
            multimodal_hands_enabled: false,
            face_sources: FaceSources {
                combined_eyes_source,
                eye_tracker_fb: None,
                #[cfg(not(target_os = "android"))]
                face_tracker_fb: None,
                eye_tracker_htc,
                lip_tracker_htc,
                face_tracker_pico: None,
            },
            body_sources: BodySources {
                body_tracker_fb: None,
                body_tracker_bd: None,
                motion_tracker_bd: None,
            },
        }
    }

    pub fn select_sources(&mut self, config: &InteractionSourcesConfig) {
        // First of all, disable/delete all sources. This ensures there are no conflicts
        if let Some(handle) = &mut self.multimodal_handle {
            handle.pause().ok();
        }
        if let Some(face_tracker) = &self.face_sources.face_tracker_pico {
            face_tracker.stop_face_tracking().ok();
        }
        self.multimodal_hands_enabled = false;
        self.face_sources.eye_tracker_fb = None;
        #[cfg(not(target_os = "android"))]
        {
            self.face_sources.face_tracker_fb = None;
        }
        self.face_sources.face_tracker_pico = None;
        self.body_sources.body_tracker_fb = None;
        self.body_sources.body_tracker_bd = None;
        self.body_sources.motion_tracker_bd = None;

        // todo: check which permissions are needed for htc
        if let Some(config) = &config.face_tracking {
            if (config.eye_tracking_fb) && matches!(self.platform, Platform::QuestPro) {
                #[cfg(target_os = "android")]
                alvr_system_info::try_get_permission("com.oculus.permission.EYE_TRACKING")
            }
            if config.face_tracking_fb && matches!(self.platform, Platform::QuestPro) {
                #[cfg(target_os = "android")]
                {
                    alvr_system_info::try_get_permission("android.permission.RECORD_AUDIO");
                    alvr_system_info::try_get_permission("com.oculus.permission.FACE_TRACKING")
                }
            }
            if config.face_tracking_pico && self.platform.is_pico() {
                #[cfg(target_os = "android")]
                {
                    alvr_system_info::try_get_permission("android.permission.RECORD_AUDIO");
                    alvr_system_info::try_get_permission("com.picovr.permission.FACE_TRACKING")
                }
            }
        }

        if let Some(config) = &config.body_tracking {
            if (config.body_tracking_fb.enabled())
                && self.platform.is_quest()
                && self.platform != Platform::Quest1
            {
                #[cfg(target_os = "android")]
                alvr_system_info::try_get_permission("com.oculus.permission.BODY_TRACKING")
            }
        }

        // Note: We cannot enable multimodal if fb body tracking is active. It would result in a
        // ERROR_RUNTIME_FAILURE crash.
        if config.body_tracking.is_none() && config.prefers_multimodal_input {
            if let Some(handle) = &mut self.multimodal_handle {
                if handle.resume().is_ok() {
                    self.multimodal_hands_enabled = true;
                }
            }
        }

        self.face_sources.eye_tracker_fb = create_ext_object(
            "EyeTrackerSocial",
            config.face_tracking.as_ref().map(|s| s.eye_tracking_fb),
            || EyeTrackerSocial::new(&self.xr_session),
        );

        // XR_FB_face_tracking2 is unavailable on the Huawei runtime, see extra_extensions.
        #[cfg(not(target_os = "android"))]
        {
            self.face_sources.face_tracker_fb = create_ext_object(
                "FaceTracker2FB",
                config.face_tracking.as_ref().map(|s| s.face_tracking_fb),
                || FaceTracker2FB::new(self.xr_session.clone(), true, true),
            );
        }

        self.face_sources.face_tracker_pico = create_ext_object(
            "FaceTrackerPico",
            config.face_tracking.as_ref().map(|s| s.face_tracking_pico),
            || FaceTrackerPico::new(self.xr_session.clone()),
        );

        self.body_sources.body_tracker_fb = create_ext_object(
            "BodyTrackerFB (full set)",
            config
                .body_tracking
                .as_ref()
                .and_then(|s| s.body_tracking_fb.as_option())
                .map(|c| c.full_body),
            || BodyTrackerFB::new(&self.xr_session, *BODY_JOINT_SET_FULL_BODY_META),
        )
        .map(|tracker| (tracker, FULL_BODY_JOINT_COUNT_META))
        .or_else(|| {
            create_ext_object(
                "BodyTrackerFB (default set)",
                config
                    .body_tracking
                    .as_ref()
                    .map(|s| s.body_tracking_fb.enabled()),
                || BodyTrackerFB::new(&self.xr_session, xr::BodyJointSetFB::DEFAULT),
            )
            .map(|tracker| (tracker, xr::BodyJointFB::COUNT.into_raw() as usize))
        });

        if let Some(body_tracking_config) = config
            .body_tracking
            .as_ref()
            .and_then(|s| s.body_tracking_bd.as_option())
        {
            match body_tracking_config {
                BodyTrackingBDConfig::BodyTracking {
                    high_accuracy,
                    prompt_calibration_on_start,
                } => {
                    self.body_sources.body_tracker_bd = create_ext_object(
                        "BodyTrackerBD (high accuracy)",
                        Some(*high_accuracy),
                        || {
                            BodyTrackerBD::new(
                                self.xr_session.clone(),
                                BodyJointSetBD::FULL_BODY_JOINTS,
                                &self.extra_extensions,
                                self.xr_system,
                                *prompt_calibration_on_start,
                            )
                        },
                    )
                    .or_else(|| {
                        create_ext_object("BodyTrackerBD (low accuracy)", Some(true), || {
                            BodyTrackerBD::new(
                                self.xr_session.clone(),
                                BodyJointSetBD::BODY_WITHOUT_ARM,
                                &self.extra_extensions,
                                self.xr_system,
                                *prompt_calibration_on_start,
                            )
                        })
                    })
                }
                BodyTrackingBDConfig::ObjectTracking => {
                    self.body_sources.motion_tracker_bd =
                        create_ext_object("MotionTrackerBD (object tracking)", Some(true), || {
                            MotionTrackerBD::new(self.xr_session.clone(), &self.extra_extensions)
                        });
                }
            }
        }

        if let Some(face_tracker) = &self.face_sources.face_tracker_pico {
            face_tracker.start_face_tracking().ok();
        }
    }
}

pub fn get_reference_space(
    xr_session: &xr::Session<xr::OpenGlEs>,
    ty: xr::ReferenceSpaceType,
) -> xr::Space {
    xr_session
        .create_reference_space(ty, xr::Posef::IDENTITY)
        .unwrap()
}

pub fn get_head_data(
    xr_session: &xr::Session<xr::OpenGlEs>,
    platform: Platform,
    stage_reference_space: &xr::Space,
    view_reference_space: &xr::Space,
    time: Duration,
    future_time: Duration,
    last_view_params: &[ViewParams; 2],
    force_view_params_update: bool,
) -> Option<(DeviceMotion, Option<[ViewParams; 2]>)> {
    let xr_time = crate::to_xr_time(time);

    let (head_location, head_velocity) = view_reference_space
        .relate(stage_reference_space, xr_time)
        .ok()?;

    if !head_location
        .location_flags
        .contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
    {
        return None;
    }

    let (view_flags, views) = xr_session
        .locate_views(
            xr::ViewConfigurationType::PRIMARY_STEREO,
            xr_time,
            stage_reference_space,
        )
        .ok()?;

    if !view_flags.contains(xr::ViewStateFlags::POSITION_VALID)
        || !view_flags.contains(xr::ViewStateFlags::ORIENTATION_VALID)
    {
        return None;
    }

    let mut motion = DeviceMotion {
        pose: crate::from_xr_pose(head_location.pose),
        linear_velocity: if head_velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::LINEAR_VALID)
        {
            crate::from_xr_vec3(head_velocity.linear_velocity)
        } else {
            Vec3::ZERO
        },
        angular_velocity: if head_velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::ANGULAR_VALID)
        {
            crate::from_xr_vec3(head_velocity.angular_velocity)
        } else {
            Vec3::ZERO
        },
    };

    // Some headsets use wrong frame of reference for linear and angular velocities.
    if platform.is_pico() || platform.is_vive() || platform.is_yvr() {
        let xr_future_time = crate::to_xr_time(future_time);

        let predicted_location = view_reference_space
            .locate(stage_reference_space, xr_future_time)
            .ok()?;

        if !predicted_location
            .location_flags
            .contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
        {
            return None;
        }

        let time_offset = future_time.saturating_sub(time);

        if !time_offset.is_zero() {
            let time_offset_s = time_offset.as_secs_f32();

            motion.linear_velocity = (crate::from_xr_vec3(predicted_location.pose.position)
                - motion.pose.position)
                / time_offset_s;
            motion.angular_velocity = (crate::from_xr_quat(predicted_location.pose.orientation)
                * motion.pose.orientation.inverse())
            .to_scaled_axis()
                / time_offset_s;
        }
    }

    let last_ipd_m = last_view_params[0]
        .pose
        .position
        .distance(last_view_params[1].pose.position);
    let current_ipd_m = crate::from_xr_vec3(views[1].pose.position)
        .distance(crate::from_xr_vec3(views[0].pose.position));
    let current_view_params = [
        ViewParams {
            pose: motion.pose.inverse() * crate::from_xr_pose(views[0].pose),
            fov: crate::from_xr_fov(views[0].fov),
        },
        ViewParams {
            pose: motion.pose.inverse() * crate::from_xr_pose(views[1].pose),
            fov: crate::from_xr_fov(views[1].fov),
        },
    ];
    let fov_changed =
        current_view_params
            .iter()
            .zip(last_view_params)
            .any(|(current, previous)| {
                (current.fov.left - previous.fov.left).abs() > FOV_CHANGE_EPS
                    || (current.fov.right - previous.fov.right).abs() > FOV_CHANGE_EPS
                    || (current.fov.up - previous.fov.up).abs() > FOV_CHANGE_EPS
                    || (current.fov.down - previous.fov.down).abs() > FOV_CHANGE_EPS
            });
    let view_params = if force_view_params_update
        || f32::abs(current_ipd_m - last_ipd_m) > IPD_CHANGE_EPS
        || fov_changed
    {
        if platform.is_huawei() && force_view_params_update {
            warn!(
                "[HVR-DIAG] Huawei view config: ipd={:.3} mm, left_fov=[{:.2}, {:.2}, {:.2}, {:.2}] deg, right_fov=[{:.2}, {:.2}, {:.2}, {:.2}] deg",
                current_ipd_m * 1000.0,
                current_view_params[0].fov.left.to_degrees(),
                current_view_params[0].fov.right.to_degrees(),
                current_view_params[0].fov.up.to_degrees(),
                current_view_params[0].fov.down.to_degrees(),
                current_view_params[1].fov.left.to_degrees(),
                current_view_params[1].fov.right.to_degrees(),
                current_view_params[1].fov.up.to_degrees(),
                current_view_params[1].fov.down.to_degrees(),
            );
        }

        Some(current_view_params)
    } else {
        None
    };

    Some((motion, view_params))
}

#[expect(clippy::too_many_arguments)]
pub fn get_hand_data(
    xr_session: &xr::Session<xr::OpenGlEs>,
    platform: Platform,
    reference_space: &xr::Space,
    time: Duration,
    future_time: Duration,
    hand_source: &HandInteraction,
    last_controller_pose: &mut Pose,
    last_palm_pose: &mut Pose,
) -> (Option<DeviceMotion>, Option<[Pose; 26]>) {
    let xr_time = crate::to_xr_time(time);

    let controller_motion = if hand_source
        .grip_action
        .is_active(xr_session, hand_source.subaction_path)
        .unwrap_or(false)
    {
        // Huawei's legacy OpenXR runtime corrupts the optional XrSpaceVelocity payload: values can
        // contain controller positions or enormous but finite numbers. Query only XrSpaceLocation,
        // matching Huawei's official sample, and report zero velocity rather than letting SteamVR
        // reject an otherwise valid controller pose.
        let location_and_velocity = if platform.is_huawei() {
            hand_source
                .grip_space
                .locate(reference_space, xr_time)
                .map(|location| (location, None))
        } else {
            hand_source
                .grip_space
                .relate(reference_space, xr_time)
                .map(|(location, velocity)| (location, Some(velocity)))
        };

        if let Ok((location, velocity)) = location_and_velocity {
            let orientation_valid = location
                .location_flags
                .contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
            let position_valid = location
                .location_flags
                .contains(xr::SpaceLocationFlags::POSITION_VALID);

            if orientation_valid {
                let orientation = crate::from_xr_quat(location.pose.orientation);
                if orientation.is_finite() {
                    last_controller_pose.orientation = orientation;
                }
            }

            if position_valid {
                let position = crate::from_xr_vec3(location.pose.position);
                if position.is_finite() {
                    last_controller_pose.position = position;
                }
            }

            let pose = *last_controller_pose * hand_source.pose_offset;

            let mut linear_velocity = velocity
                .as_ref()
                .filter(|velocity| {
                    velocity
                        .velocity_flags
                        .contains(xr::SpaceVelocityFlags::LINEAR_VALID)
                })
                .map(|velocity| crate::from_xr_vec3(velocity.linear_velocity))
                .unwrap_or(Vec3::ZERO);
            let mut angular_velocity = velocity
                .as_ref()
                .filter(|velocity| {
                    velocity
                        .velocity_flags
                        .contains(xr::SpaceVelocityFlags::ANGULAR_VALID)
                })
                .map(|velocity| crate::from_xr_vec3(velocity.angular_velocity))
                .unwrap_or(Vec3::ZERO);
            if !linear_velocity.is_finite() {
                linear_velocity = Vec3::ZERO;
            }
            if !angular_velocity.is_finite() {
                angular_velocity = Vec3::ZERO;
            }

            // Some headsets use wrong frame of reference for linear and angular velocities.
            if platform.is_pico() || platform.is_vive() {
                let xr_future_time = crate::to_xr_time(future_time);

                let maybe_future_location = hand_source
                    .grip_space
                    .locate(reference_space, xr_future_time);

                if let Ok(future_location) = maybe_future_location {
                    if future_location.location_flags.contains(
                        xr::SpaceLocationFlags::ORIENTATION_VALID
                            | xr::SpaceLocationFlags::POSITION_VALID,
                    ) {
                        let time_offset = future_time.saturating_sub(time);

                        if !time_offset.is_zero() {
                            let time_offset_s = time_offset.as_secs_f32();

                            linear_velocity = (crate::from_xr_vec3(future_location.pose.position)
                                - last_controller_pose.position)
                                / time_offset_s;
                            angular_velocity =
                                (crate::from_xr_quat(future_location.pose.orientation)
                                    * last_controller_pose.orientation.inverse())
                                .to_scaled_axis()
                                    / time_offset_s;
                        }
                    }
                }
            }

            Some(DeviceMotion {
                pose,
                linear_velocity,
                angular_velocity,
            })
        } else {
            None
        }
    } else {
        None
    };

    let hand_joints = if let Some(tracker) = &hand_source.skeleton_tracker {
        let xr_now = crate::xr_runtime_now(xr_session.instance()).unwrap_or(xr_time);

        if let Some(joint_locations) = reference_space
            .locate_hand_joints(tracker, xr_now)
            .ok()
            .flatten()
        {
            if joint_locations[0]
                .location_flags
                .contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
            {
                last_palm_pose.orientation =
                    crate::from_xr_quat(joint_locations[0].pose.orientation);
            }

            if joint_locations[0]
                .location_flags
                .contains(xr::SpaceLocationFlags::POSITION_VALID)
            {
                last_palm_pose.position = crate::from_xr_vec3(joint_locations[0].pose.position);
            }

            let mut joints: [_; 26] = joint_locations
                .iter()
                .map(|j| crate::from_xr_pose(j.pose))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();

            joints[0] = *last_palm_pose;

            Some(joints)
        } else {
            None
        }
    } else {
        None
    };

    (controller_motion, hand_joints)
}

pub fn update_buttons(
    xr_session: &xr::Session<xr::OpenGlEs>,
    platform: Platform,
    button_actions: &HashMap<u64, ButtonAction>,
) -> Vec<ButtonEntry> {
    let mut button_entries = Vec::with_capacity(2);
    for (id, action) in button_actions {
        match action {
            ButtonAction::Binary {
                action,
                subaction_path,
            } => {
                let Ok(state) = action.state(xr_session, *subaction_path) else {
                    continue;
                };

                if state.changed_since_last_sync {
                    button_entries.push(ButtonEntry {
                        path_id: *id,
                        value: ButtonValue::Binary(state.current_state),
                    });
                }
            }
            ButtonAction::Scalar {
                action,
                subaction_path,
            } => {
                if platform.is_huawei()
                    && (*id == *LEFT_THUMBSTICK_X_ID
                        || *id == *LEFT_THUMBSTICK_Y_ID
                        || *id == *RIGHT_THUMBSTICK_X_ID
                        || *id == *RIGHT_THUMBSTICK_Y_ID)
                {
                    continue;
                }

                let Ok(state) = action.state(xr_session, *subaction_path) else {
                    continue;
                };

                if state.changed_since_last_sync {
                    button_entries.push(ButtonEntry {
                        path_id: *id,
                        value: ButtonValue::Scalar(state.current_state),
                    });
                }
            }
        }
    }

    if platform.is_huawei() {
        append_huawei_thumbstick_entries(
            xr_session,
            button_actions,
            *LEFT_THUMBSTICK_X_ID,
            *LEFT_THUMBSTICK_Y_ID,
            &mut button_entries,
        );
        append_huawei_thumbstick_entries(
            xr_session,
            button_actions,
            *RIGHT_THUMBSTICK_X_ID,
            *RIGHT_THUMBSTICK_Y_ID,
            &mut button_entries,
        );
    }

    button_entries
}

// Diagnostic helper: the Huawei remote can only be verified locally, so mirror every button
// transition into logcat. Requires xrSyncActions to have been called for the current frame.
pub fn log_huawei_button_changes(
    xr_session: &xr::Session<xr::OpenGlEs>,
    button_actions: &HashMap<u64, ButtonAction>,
) {
    for (id, action) in button_actions {
        let Some(button_info) = BUTTON_INFO.get(id) else {
            continue;
        };

        match action {
            ButtonAction::Binary {
                action,
                subaction_path,
            } => {
                if let Ok(state) = action.state(xr_session, *subaction_path) {
                    if state.changed_since_last_sync {
                        info!(
                            "[HVR-INPUT] {} = {}",
                            button_info.path, state.current_state
                        );
                    }
                }
            }
            ButtonAction::Scalar {
                action,
                subaction_path,
            } => {
                if let Ok(state) = action.state(xr_session, *subaction_path) {
                    if state.changed_since_last_sync {
                        info!(
                            "[HVR-INPUT] {} = {:.3}",
                            button_info.path, state.current_state
                        );
                    }
                }
            }
        }
    }
}

fn append_huawei_thumbstick_entries(
    xr_session: &xr::Session<xr::OpenGlEs>,
    button_actions: &HashMap<u64, ButtonAction>,
    x_id: u64,
    y_id: u64,
    button_entries: &mut Vec<ButtonEntry>,
) {
    let Some(ButtonAction::Scalar {
        action: x_action,
        subaction_path: x_subaction_path,
    }) = button_actions.get(&x_id)
    else {
        return;
    };
    let Some(ButtonAction::Scalar {
        action: y_action,
        subaction_path: y_subaction_path,
    }) = button_actions.get(&y_id)
    else {
        return;
    };
    let Ok(x_state) = x_action.state(xr_session, *x_subaction_path) else {
        return;
    };
    let Ok(y_state) = y_action.state(xr_session, *y_subaction_path) else {
        return;
    };

    if !x_state.changed_since_last_sync && !y_state.changed_since_last_sync {
        return;
    }

    let (x, y) = huawei_input::normalize_thumbstick(x_state.current_state, y_state.current_state);
    button_entries.extend([
        ButtonEntry {
            path_id: x_id,
            value: ButtonValue::Scalar(x),
        },
        ButtonEntry {
            path_id: y_id,
            value: ButtonValue::Scalar(y),
        },
    ]);
}

pub fn get_eye_gazes(
    xr_session: &xr::Session<xr::OpenGlEs>,
    sources: &FaceSources,
    reference_space: &xr::Space,
    time: Duration,
) -> [Option<Pose>; 2] {
    let xr_time = crate::to_xr_time(time);

    'fb_eyes: {
        let Some(tracker) = &sources.eye_tracker_fb else {
            break 'fb_eyes;
        };

        if let Ok(gazes) = tracker.get_eye_gazes(reference_space, xr_time) {
            return [
                gazes[0].map(crate::from_xr_pose),
                gazes[1].map(crate::from_xr_pose),
            ];
        }
    };

    let Some((eyes_action, eyes_space)) = &sources.combined_eyes_source else {
        return [None, None];
    };
    if !eyes_action
        .is_active(xr_session, xr::Path::NULL)
        .unwrap_or(false)
    {
        return [None, None];
    }

    if let Ok(location) = eyes_space.locate(reference_space, xr_time) {
        [
            location
                .location_flags
                .contains(xr::SpaceLocationFlags::ORIENTATION_TRACKED)
                .then(|| crate::from_xr_pose(location.pose)),
            None,
        ]
    } else {
        [None, None]
    }
}

pub fn get_fb_face_expression(context: &FaceSources, time: Duration) -> Option<Vec<f32>> {
    #[cfg(target_os = "android")]
    {
        // XR_FB_face_tracking2 is unavailable on the Huawei runtime.
        let _ = (context, time);
        None
    }

    #[cfg(not(target_os = "android"))]
    {
        let xr_time = crate::to_xr_time(time);

        context
            .face_tracker_fb
            .as_ref()
            .and_then(|t| t.get_face_expression_weights(xr_time).ok().flatten())
            .map(|weights| weights.into_iter().collect())
    }
}

pub fn get_pico_face_expression(context: &FaceSources, time: Duration) -> Option<Vec<f32>> {
    let xr_time = crate::to_xr_time(time);

    context
        .face_tracker_pico
        .as_ref()
        .and_then(|t| t.get_face_tracking_data(xr_time).ok().flatten())
        .map(|weights| weights.into_iter().collect())
}

pub fn get_htc_eye_expression(context: &FaceSources, time: Duration) -> Option<Vec<f32>> {
    let xr_time = crate::to_xr_time(time);

    context
        .eye_tracker_htc
        .as_ref()
        .and_then(|t| t.get_facial_expressions(xr_time).ok().flatten())
}

pub fn get_htc_lip_expression(context: &FaceSources, time: Duration) -> Option<Vec<f32>> {
    let xr_time = crate::to_xr_time(time);

    context
        .lip_tracker_htc
        .as_ref()
        .and_then(|t| t.get_facial_expressions(xr_time).ok().flatten())
}

pub fn get_fb_body_skeleton(
    reference_space: &xr::Space,
    time: xr::Time,
    body_tracker: &BodyTrackerFB,
    joint_count: usize,
) -> Option<Vec<Option<Pose>>> {
    body_tracker
        .locate_body_joints(time, reference_space, joint_count)
        .ok()
        .flatten()
        .map(|joints| {
            let valid_flags: SpaceLocationFlags =
                SpaceLocationFlags::ORIENTATION_VALID | SpaceLocationFlags::POSITION_VALID;

            joints
                .iter()
                .map(|joint| {
                    joint
                        .location_flags
                        .contains(valid_flags)
                        .then(|| crate::from_xr_pose(joint.pose))
                })
                .collect()
        })
}

pub fn get_fb_body_tracking_points(
    reference_space: &xr::Space,
    time: Duration,
    body_tracker: &BodyTrackerFB,
    joint_count: usize,
) -> Vec<(u64, DeviceMotion)> {
    let xr_time = crate::to_xr_time(time);

    if let Some(joint_locations) = body_tracker
        .locate_body_joints(xr_time, reference_space, joint_count)
        .ok()
        .flatten()
    {
        let valid_flags: SpaceLocationFlags =
            SpaceLocationFlags::ORIENTATION_VALID | SpaceLocationFlags::POSITION_VALID;

        let mut joints = Vec::<(u64, DeviceMotion)>::with_capacity(8);

        if let Some(joint) = joint_locations.get(xr::BodyJointFB::CHEST.into_raw() as usize) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_CHEST_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(xr::BodyJointFB::HIPS.into_raw() as usize) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_HIPS_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) =
            joint_locations.get(xr::BodyJointFB::LEFT_ARM_LOWER.into_raw() as usize)
        {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_LEFT_ELBOW_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) =
            joint_locations.get(xr::BodyJointFB::RIGHT_ARM_LOWER.into_raw() as usize)
        {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_RIGHT_ELBOW_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(FULL_BODY_JOINT_LEFT_LOWER_LEG_META) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_LEFT_KNEE_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(FULL_BODY_JOINT_LEFT_FOOT_BALL_META) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_LEFT_FOOT_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(FULL_BODY_JOINT_RIGHT_LOWER_LEG_META) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_RIGHT_KNEE_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(FULL_BODY_JOINT_RIGHT_FOOT_BALL_META) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_RIGHT_FOOT_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        return joints;
    }

    Vec::new()
}

pub fn get_bd_motion_trackers(
    time: Duration,
    motion_tracker: &MotionTrackerBD,
) -> Vec<(u64, DeviceMotion)> {
    let xr_time = crate::to_xr_time(time);

    if let Some(mut trackers) = motion_tracker
        .locate_motion_trackers(xr_time)
        .ok()
        .flatten()
    {
        let mut joints = Vec::<(u64, DeviceMotion)>::with_capacity(3);

        let joints_ids = [*BODY_HIPS_ID, *BODY_LEFT_FOOT_ID, *BODY_RIGHT_FOOT_ID];

        trackers.sort_by(|a, b| a.serial.cmp(&b.serial));

        for (i, item) in trackers.iter().enumerate() {
            joints.push((
                joints_ids[i],
                DeviceMotion {
                    pose: crate::from_xr_pose(item.local_pose.pose),
                    linear_velocity: crate::from_xr_vec3(item.local_pose.linear_velocity),
                    angular_velocity: crate::from_xr_vec3(item.local_pose.angular_velocity),
                },
            ))
        }

        return joints;
    }

    Vec::new()
}

pub fn get_bd_body_skeleton(
    reference_space: &xr::Space,
    time: xr::Time,
    body_tracker: &BodyTrackerBD,
) -> Option<Vec<Option<Pose>>> {
    body_tracker
        .locate_body_joints(time, reference_space)
        .ok()
        .flatten()
        .map(|joints| {
            let valid_flags: SpaceLocationFlags =
                SpaceLocationFlags::ORIENTATION_VALID | SpaceLocationFlags::POSITION_VALID;

            joints
                .iter()
                .map(|joint| {
                    joint
                        .location_flags
                        .contains(valid_flags)
                        .then(|| crate::from_xr_pose(joint.pose))
                })
                .collect()
        })
}

pub fn get_bd_body_tracking_points(
    reference_space: &xr::Space,
    time: Duration,
    body_tracker: &BodyTrackerBD,
) -> Vec<(u64, DeviceMotion)> {
    let xr_time = crate::to_xr_time(time);

    if let Some(joint_locations) = body_tracker
        .locate_body_joints(xr_time, reference_space)
        .ok()
        .flatten()
    {
        let valid_flags: SpaceLocationFlags =
            SpaceLocationFlags::ORIENTATION_VALID | SpaceLocationFlags::POSITION_VALID;

        let mut joints = Vec::<(u64, DeviceMotion)>::with_capacity(8);

        if let Some(joint) = joint_locations.get(BODY_JOINT_SPINE3_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_CHEST_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(BODY_JOINT_PELVIS_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_HIPS_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(BODY_JOINT_LEFT_ELBOW_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_LEFT_ELBOW_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(BODY_JOINT_RIGHT_ELBOW_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_RIGHT_ELBOW_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(BODY_JOINT_LEFT_KNEE_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_LEFT_KNEE_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(BODY_JOINT_LEFT_FOOT_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_LEFT_FOOT_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(BODY_JOINT_RIGHT_KNEE_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_RIGHT_KNEE_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        if let Some(joint) = joint_locations.get(BODY_JOINT_RIGHT_FOOT_BD) {
            if joint.location_flags.contains(valid_flags) {
                joints.push((
                    *BODY_RIGHT_FOOT_ID,
                    DeviceMotion {
                        pose: crate::from_xr_pose(joint.pose),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                    },
                ))
            }
        }

        return joints;
    }

    Vec::new()
}
