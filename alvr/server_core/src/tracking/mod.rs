mod body;
mod face;
mod vmc;

pub use body::*;
pub use face::*;
pub use vmc::*;

use crate::{
    connection::STREAMING_RECV_TIMEOUT,
    hand_gestures::{self, HandGestureManager, HAND_GESTURE_BUTTON_SET},
    input_mapping::ButtonMappingManager,
    ConnectionContext, ServerCoreEvent, SESSION_MANAGER,
};
use alvr_common::{
    glam::{EulerRot, Quat, Vec3},
    parking_lot::Mutex,
    ConnectionError, DeviceMotion, Pose, BODY_CHEST_ID, BODY_HIPS_ID, BODY_LEFT_ELBOW_ID,
    BODY_LEFT_FOOT_ID, BODY_LEFT_KNEE_ID, BODY_RIGHT_ELBOW_ID, BODY_RIGHT_FOOT_ID,
    BODY_RIGHT_KNEE_ID, DEVICE_ID_TO_PATH, HAND_LEFT_ID, HAND_RIGHT_ID, HEAD_ID,
};
use alvr_events::{EventType, TrackingEvent};
use alvr_packets::{FaceData, Tracking};
use alvr_session::{
    settings_schema::Switch, BodyTrackingConfig, HeadsetConfig, PositionRecenteringMode,
    RotationRecenteringMode, Settings, ThreeDofMode, VMCConfig,
};
use alvr_sockets::StreamReceiver;
use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    f32::consts::PI,
    sync::Arc,
    time::{Duration, Instant},
};

const DEG_TO_RAD: f32 = PI / 180.0;

// A controller is considered to have real positional tracking (6DoF) once it reports a position
// clearly away from the tracking origin. 3DoF controllers, like the Huawei remote or a phone
// controller, only report rotation, so their position stays at the origin.
const SIX_DOF_POSITION_THRESHOLD_M: f32 = 0.05;
// How long to wait for a 6DoF controller before deciding that the headset is 3DoF only.
const SIX_DOF_DETECTION_TIMEOUT: Duration = Duration::from_secs(4);
// How long a 6DoF controller is remembered after it stops being tracked.
const SIX_DOF_LOST_TIMEOUT: Duration = Duration::from_secs(15);

// Where rotation-only hands are placed relative to the 3DoF viewpoint. Without positional tracking
// the hands would otherwise sit at the tracking origin, i.e. roughly an eye height below the
// viewpoint, which is far outside the field of view.
const THREE_DOF_HAND_LATERAL_M: f32 = 0.18;
const THREE_DOF_HAND_DROP_M: f32 = -0.28;
const THREE_DOF_HAND_FORWARD_M: f32 = -0.35;

// Runtime detection of positional tracking, used by the "Auto" 3DoF mode.
#[derive(Default)]
struct AutoThreeDofState {
    start: Option<Instant>,
    last_seen: Option<Instant>,
    six_dof: bool,
    decided: bool,
}

impl AutoThreeDofState {
    /// Returns true when the head pose must be reduced to rotation only (3DoF).
    fn update(&mut self, six_dof_controller_seen: bool) -> bool {
        let now = Instant::now();
        let start = *self.start.get_or_insert(now);

        if six_dof_controller_seen {
            if !self.six_dof {
                alvr_common::info!("[3DoF] 6DoF controller detected: keeping positional tracking");
            }
            self.six_dof = true;
            self.decided = true;
            self.last_seen = Some(now);

            return false;
        }

        if self.six_dof {
            let lost_for = now.duration_since(self.last_seen.unwrap_or(now));
            if lost_for >= SIX_DOF_LOST_TIMEOUT {
                alvr_common::info!(
                    "[3DoF] 6DoF controller lost for {lost_for:?}: falling back to 3DoF tracking"
                );
                self.six_dof = false;
            }
        } else if !self.decided && now.duration_since(start) >= SIX_DOF_DETECTION_TIMEOUT {
            alvr_common::info!(
                "[3DoF] No 6DoF controller detected within {SIX_DOF_DETECTION_TIMEOUT:?}: enabling 3DoF tracking"
            );
            self.decided = true;
        }

        !self.six_dof && self.decided
    }
}

#[derive(Debug)]
pub enum HandType {
    Left = 0,
    Right = 1,
}

// todo: Move this struct to Settings and use it for every tracked device
#[derive(Default)]
struct MotionConfig {
    // Position offset applied after rotation offset
    pose_offset: Pose,
    linear_velocity_cutoff: f32,
    angular_velocity_cutoff: f32,
}

pub struct TrackingManager {
    last_head_pose: Pose,             // client's reference space
    inverse_recentering_origin: Pose, // client's reference space
    device_motions_history: HashMap<u64, VecDeque<(Duration, DeviceMotion)>>,
    hand_skeletons_history: [VecDeque<(Duration, [Pose; 26])>; 2],
    last_face_data: FaceData,
    max_history_size: usize,
    auto_three_dof: AutoThreeDofState,
}

impl TrackingManager {
    pub fn new(max_history_size: usize) -> TrackingManager {
        TrackingManager {
            last_head_pose: Pose::default(),
            inverse_recentering_origin: Pose::default(),
            device_motions_history: HashMap::new(),
            hand_skeletons_history: [VecDeque::new(), VecDeque::new()],
            last_face_data: FaceData::default(),
            max_history_size,
            auto_three_dof: AutoThreeDofState::default(),
        }
    }

    pub fn recenter(
        &mut self,
        position_recentering_mode: PositionRecenteringMode,
        rotation_recentering_mode: RotationRecenteringMode,
    ) {
        let position = match position_recentering_mode {
            PositionRecenteringMode::Disabled => Vec3::ZERO,
            PositionRecenteringMode::LocalFloor => {
                let mut pos = self.last_head_pose.position;
                pos.y = 0.0;

                pos
            }
            PositionRecenteringMode::Local { view_height } => {
                self.last_head_pose.position
                    - self.last_head_pose.orientation * Vec3::new(0.0, view_height, 0.0)
            }
        };

        let orientation = match rotation_recentering_mode {
            RotationRecenteringMode::Disabled => Quat::IDENTITY,
            RotationRecenteringMode::Yaw => {
                let mut rot = self.last_head_pose.orientation;
                // extract yaw rotation
                rot.x = 0.0;
                rot.z = 0.0;
                rot = rot.normalize();

                rot
            }
            RotationRecenteringMode::Tilted => self.last_head_pose.orientation,
        };

        self.inverse_recentering_origin = Pose {
            position,
            orientation,
        }
        .inverse();
    }

    pub fn recenter_pose(&self, pose: Pose) -> Pose {
        self.inverse_recentering_origin * pose
    }

    pub fn recenter_motion(&self, motion: DeviceMotion) -> DeviceMotion {
        self.inverse_recentering_origin * motion
    }

    // Performs all kinds of tracking transformations, driven by settings.
    pub fn report_device_motions(
        &mut self,
        headset_config: &HeadsetConfig,
        timestamp: Duration,
        device_motions: &[(u64, DeviceMotion)],
    ) {
        // 3DoF mode ignores positional tracking, it is read once per tracking batch. In "Auto" mode
        // the decision is taken at runtime: the head pose is only clamped while no controller
        // reports a real position, which is what happens on Huawei VR Glass without the 6DoF kit.
        let three_dof_config = match &headset_config.three_dof {
            ThreeDofMode::Off => None,
            ThreeDofMode::On(config) => Some(config.clone()),
            ThreeDofMode::Auto(config) => {
                let six_dof_controller_seen = device_motions.iter().any(|(id, motion)| {
                    (*id == *HAND_LEFT_ID || *id == *HAND_RIGHT_ID)
                        && motion.pose.position.length() > SIX_DOF_POSITION_THRESHOLD_M
                });

                self.auto_three_dof
                    .update(six_dof_controller_seen)
                    .then(|| config.clone())
            }
        };

        let mut device_motion_configs = HashMap::new();
        device_motion_configs.insert(*HEAD_ID, MotionConfig::default());
        device_motion_configs.extend([
            (*BODY_CHEST_ID, MotionConfig::default()),
            (*BODY_HIPS_ID, MotionConfig::default()),
            (*BODY_LEFT_ELBOW_ID, MotionConfig::default()),
            (*BODY_RIGHT_ELBOW_ID, MotionConfig::default()),
            (*BODY_LEFT_KNEE_ID, MotionConfig::default()),
            (*BODY_LEFT_FOOT_ID, MotionConfig::default()),
            (*BODY_RIGHT_KNEE_ID, MotionConfig::default()),
            (*BODY_RIGHT_FOOT_ID, MotionConfig::default()),
        ]);

        if let Switch::Enabled(controllers) = &headset_config.controllers {
            let t = controllers.left_controller_position_offset;
            let r = controllers.left_controller_rotation_offset;

            device_motion_configs.insert(
                *HAND_LEFT_ID,
                MotionConfig {
                    pose_offset: Pose {
                        orientation: Quat::from_euler(
                            EulerRot::XYZ,
                            r[0] * DEG_TO_RAD,
                            r[1] * DEG_TO_RAD,
                            r[2] * DEG_TO_RAD,
                        ),
                        position: Vec3::new(t[0], t[1], t[2]),
                    },
                    linear_velocity_cutoff: controllers.linear_velocity_cutoff,
                    angular_velocity_cutoff: controllers.angular_velocity_cutoff * DEG_TO_RAD,
                },
            );

            device_motion_configs.insert(
                *HAND_RIGHT_ID,
                MotionConfig {
                    pose_offset: Pose {
                        orientation: Quat::from_euler(
                            EulerRot::XYZ,
                            r[0] * DEG_TO_RAD,
                            -r[1] * DEG_TO_RAD,
                            -r[2] * DEG_TO_RAD,
                        ),
                        position: Vec3::new(-t[0], t[1], t[2]),
                    },
                    linear_velocity_cutoff: controllers.linear_velocity_cutoff,
                    angular_velocity_cutoff: controllers.angular_velocity_cutoff * DEG_TO_RAD,
                },
            );
        }

        let mut processed: Vec<(u64, DeviceMotion, bool)> = Vec::with_capacity(device_motions.len());
        let mut three_dof_head_pose: Option<Pose> = None;

        for &(device_id, mut motion) in device_motions {
            if device_id == *HEAD_ID {
                self.last_head_pose = motion.pose;
            }

            // A 3DoF device reports rotation only, so its position stays at the tracking origin.
            let rotation_only = motion.pose.position.length() < SIX_DOF_POSITION_THRESHOLD_M;

            if let Some(config) = device_motion_configs.get(&device_id) {
                // Recenter
                motion = self.recenter_motion(motion);

                // 3DoF: keep the orientation only. The viewpoint is locked at the play space
                // origin, at the configured eye height. Both eyes keep their IPD offset, which
                // comes from the view params sent by the client instead.
                if device_id == *HEAD_ID {
                    if let Some(three_dof) = &three_dof_config {
                        let mut height = three_dof.eye_height_m;
                        if three_dof.prefer_runtime_height && motion.pose.position.y > 0.2 {
                            height = motion.pose.position.y;
                        }

                        motion.pose.position = Vec3::new(0.0, height, 0.0);
                        motion.linear_velocity = Vec3::ZERO;
                    }
                }

                // Apply custom transform
                motion.pose.orientation *= config.pose_offset.orientation;
                motion.pose.position += motion.pose.orientation * config.pose_offset.position;

                motion.linear_velocity += motion
                    .angular_velocity
                    .cross(motion.pose.orientation * config.pose_offset.position);
                motion.angular_velocity =
                    motion.pose.orientation.conjugate() * motion.angular_velocity;

                fn cutoff(v: Vec3, threshold: f32) -> Vec3 {
                    if v.length_squared() > threshold * threshold {
                        v
                    } else {
                        Vec3::ZERO
                    }
                }

                motion.linear_velocity =
                    cutoff(motion.linear_velocity, config.linear_velocity_cutoff);
                motion.angular_velocity =
                    cutoff(motion.angular_velocity, config.angular_velocity_cutoff);
            }

            if device_id == *HEAD_ID && three_dof_config.is_some() {
                three_dof_head_pose = Some(motion.pose);
            }

            processed.push((device_id, motion, rotation_only));
        }

        // In 3DoF the hands have no position of their own: they stay at the tracking origin while
        // the viewpoint is lifted to the configured eye height, so they end up buried at floor
        // level and take the SteamVR dashboard (anchored to a controller) down with them. Anchor
        // such rotation-only hands in front of the viewpoint, keeping their reported orientation.
        if let Some(head_pose) = three_dof_head_pose {
            for (device_id, motion, rotation_only) in &mut processed {
                let lateral = if *device_id == *HAND_LEFT_ID {
                    -1.0
                } else if *device_id == *HAND_RIGHT_ID {
                    1.0
                } else {
                    continue;
                };

                if !*rotation_only {
                    continue;
                }

                motion.pose.position = head_pose.position
                    + head_pose.orientation
                        * Vec3::new(
                            THREE_DOF_HAND_LATERAL_M * lateral,
                            THREE_DOF_HAND_DROP_M,
                            THREE_DOF_HAND_FORWARD_M,
                        );
                motion.linear_velocity = Vec3::ZERO;
            }
        }

        // Temporary ground truth for the "everything sits on the floor" investigation: show what
        // is actually pushed to SteamVR once per second or so.
        {
            use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

            static POSE_DIAG_COUNTER: AtomicU32 = AtomicU32::new(0);

            if POSE_DIAG_COUNTER.fetch_add(1, AtomicOrdering::Relaxed) % 35 == 0 {
                let mut summary = String::new();
                for (device_id, motion, rotation_only) in &processed {
                    summary.push_str(&format!(
                        " dev={device_id} pos=({:.2},{:.2},{:.2}) rot_only={rotation_only}",
                        motion.pose.position.x,
                        motion.pose.position.y,
                        motion.pose.position.z,
                    ));
                }

                alvr_common::info!(
                    "[POSE-DIAG] three_dof_active={} devices={}{}",
                    three_dof_head_pose.is_some(),
                    processed.len(),
                    summary
                );
            }
        }

        for (device_id, motion, _) in processed {
            if let Some(motions) = self.device_motions_history.get_mut(&device_id) {
                motions.push_front((timestamp, motion));

                if motions.len() > self.max_history_size {
                    motions.pop_back();
                }
            } else {
                self.device_motions_history
                    .insert(device_id, VecDeque::from(vec![(timestamp, motion)]));
            }
        }
    }

    // If the exact sample_timestamp is not found, use the closest one if it's not older. This makes
    // sure that we return None if there is no newer sample and always return Some otherwise.
    pub fn get_device_motion(
        &self,
        device_id: u64,
        sample_timestamp: Duration,
    ) -> Option<DeviceMotion> {
        self.device_motions_history
            .get(&device_id)
            .and_then(|motions| {
                // Get first element to initialize a valid motion reference
                if let Some((_, motion)) = motions.front() {
                    let mut best_timestamp_diff = Duration::MAX;
                    let mut best_motion_ref = motion;

                    // Note: we are iterating from most recent to oldest
                    for (ts, m) in motions {
                        match ts.cmp(&sample_timestamp) {
                            Ordering::Equal => return Some(*m),
                            Ordering::Greater => {
                                let diff = ts.saturating_sub(sample_timestamp);
                                if diff < best_timestamp_diff {
                                    best_timestamp_diff = diff;
                                    best_motion_ref = m;
                                }
                            }
                            Ordering::Less => continue,
                        }
                    }

                    (best_timestamp_diff != Duration::MAX).then_some(*best_motion_ref)
                } else {
                    None
                }
            })
    }

    pub fn report_hand_skeleton(
        &mut self,
        hand_type: HandType,
        timestamp: Duration,
        mut skeleton: [Pose; 26],
    ) {
        for pose in &mut skeleton {
            *pose = self.recenter_pose(*pose);
        }

        let skeleton_history = &mut self.hand_skeletons_history[hand_type as usize];

        skeleton_history.push_back((timestamp, skeleton));

        if skeleton_history.len() > self.max_history_size {
            skeleton_history.pop_front();
        }
    }

    pub fn get_hand_skeleton(
        &self,
        hand_type: HandType,
        sample_timestamp: Duration,
    ) -> Option<&[Pose; 26]> {
        self.hand_skeletons_history[hand_type as usize]
            .iter()
            .find(|(timestamp, _)| *timestamp == sample_timestamp)
            .map(|(_, skeleton)| skeleton)
    }

    // todo: send eyes in head local space from client directly
    pub fn report_face_data(&mut self, mut face_data: FaceData) {
        face_data.eye_gazes = [
            face_data.eye_gazes[0].map(|e| self.last_head_pose.inverse() * self.recenter_pose(e)),
            face_data.eye_gazes[1].map(|e| self.last_head_pose.inverse() * self.recenter_pose(e)),
        ];

        self.last_face_data = face_data;
    }

    pub fn get_face_data(&self) -> &FaceData {
        &self.last_face_data
    }
}

pub fn tracking_loop(
    ctx: &ConnectionContext,
    initial_settings: Settings,
    multimodal_protocol: bool,
    hand_gesture_manager: Arc<Mutex<HandGestureManager>>,
    mut tracking_receiver: StreamReceiver<Tracking>,
    is_streaming: impl Fn() -> bool,
) {
    let mut gestures_button_mapping_manager =
        initial_settings
            .headset
            .controllers
            .as_option()
            .map(|config| {
                ButtonMappingManager::new_automatic(
                    &HAND_GESTURE_BUTTON_SET,
                    &config.emulation_mode,
                    &config.button_mapping_config,
                )
            });

    let mut face_tracking_sink = initial_settings
        .headset
        .face_tracking
        .into_option()
        .and_then(|config| {
            FaceTrackingSink::new(config.sink, initial_settings.connection.osc_local_port).ok()
        });

    let mut body_tracking_sink = initial_settings
        .headset
        .body_tracking
        .into_option()
        .and_then(|config| {
            BodyTrackingSink::new(config.sink, initial_settings.connection.osc_local_port).ok()
        });

    let mut vmc_sink = initial_settings
        .headset
        .vmc
        .into_option()
        .and_then(|config| VMCSink::new(config).ok());

    while is_streaming() {
        let data = match tracking_receiver.recv(STREAMING_RECV_TIMEOUT) {
            Ok(tracking) => tracking,
            Err(ConnectionError::TryAgain(_)) => continue,
            Err(ConnectionError::Other(_)) => return,
        };
        let Ok(mut tracking) = data.get_header() else {
            return;
        };

        let timestamp = tracking.target_timestamp;

        if let Some(stats) = &mut *ctx.statistics_manager.write() {
            stats.report_tracking_received(timestamp);
        }

        if !multimodal_protocol {
            if tracking.hand_skeletons[0].is_some() {
                tracking
                    .device_motions
                    .retain(|(id, _)| *id != *HAND_LEFT_ID);
            }

            if tracking.hand_skeletons[1].is_some() {
                tracking
                    .device_motions
                    .retain(|(id, _)| *id != *HAND_RIGHT_ID);
            }
        }

        let controllers_config = {
            let data_lock = SESSION_MANAGER.read();
            data_lock
                .settings()
                .headset
                .controllers
                .clone()
                .into_option()
        };

        let device_motion_keys = {
            let mut tracking_manager_lock = ctx.tracking_manager.write();
            let session_manager_lock = SESSION_MANAGER.read();
            let headset_config = &session_manager_lock.settings().headset;

            let device_motion_keys = tracking
                .device_motions
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();

            tracking_manager_lock.report_device_motions(
                headset_config,
                timestamp,
                &tracking.device_motions,
            );

            if let Some(skeleton) = tracking.hand_skeletons[0] {
                tracking_manager_lock.report_hand_skeleton(HandType::Left, timestamp, skeleton);
            }
            if let Some(skeleton) = tracking.hand_skeletons[1] {
                tracking_manager_lock.report_hand_skeleton(HandType::Right, timestamp, skeleton);
            }

            tracking_manager_lock.report_face_data(tracking.face_data);
            if let Some(sink) = &mut face_tracking_sink {
                sink.send_tracking(tracking_manager_lock.get_face_data().clone());
            }

            if session_manager_lock.settings().extra.logging.log_tracking {
                let face_data = tracking_manager_lock.get_face_data().clone();

                let device_motions = device_motion_keys
                    .iter()
                    .filter_map(move |id| {
                        Some((
                            (*DEVICE_ID_TO_PATH.get(id)?).into(),
                            tracking_manager_lock
                                .get_device_motion(*id, timestamp)
                                .unwrap(),
                        ))
                    })
                    .collect::<Vec<(String, DeviceMotion)>>();

                alvr_events::send_event(EventType::Tracking(Box::new(TrackingEvent {
                    device_motions,
                    hand_skeletons: tracking.hand_skeletons,
                    eye_gazes: face_data.eye_gazes,
                    fb_face_expression: face_data.fb_face_expression,
                    htc_eye_expression: face_data.htc_eye_expression,
                    htc_lip_expression: face_data.htc_lip_expression,
                })))
            }

            device_motion_keys
        };

        // Handle hand gestures
        if let (Some(gestures_config), Some(gestures_button_mapping_manager)) = (
            controllers_config
                .as_ref()
                .and_then(|c| c.hand_tracking_interaction.as_option()),
            &mut gestures_button_mapping_manager,
        ) {
            let mut hand_gesture_manager_lock = hand_gesture_manager.lock();

            if !device_motion_keys.contains(&*HAND_LEFT_ID) {
                if let Some(hand_skeleton) = tracking.hand_skeletons[0] {
                    ctx.events_sender
                        .send(ServerCoreEvent::Buttons(
                            hand_gestures::trigger_hand_gesture_actions(
                                gestures_button_mapping_manager,
                                *HAND_LEFT_ID,
                                &hand_gesture_manager_lock.get_active_gestures(
                                    &hand_skeleton,
                                    gestures_config,
                                    *HAND_LEFT_ID,
                                ),
                                gestures_config.only_touch,
                            ),
                        ))
                        .ok();
                }
            }
            if !device_motion_keys.contains(&*HAND_RIGHT_ID) {
                if let Some(hand_skeleton) = tracking.hand_skeletons[1] {
                    ctx.events_sender
                        .send(ServerCoreEvent::Buttons(
                            hand_gestures::trigger_hand_gesture_actions(
                                gestures_button_mapping_manager,
                                *HAND_RIGHT_ID,
                                &hand_gesture_manager_lock.get_active_gestures(
                                    &hand_skeleton,
                                    gestures_config,
                                    *HAND_RIGHT_ID,
                                ),
                                gestures_config.only_touch,
                            ),
                        ))
                        .ok();
                }
            }
        }

        ctx.events_sender
            .send(ServerCoreEvent::Tracking {
                sample_timestamp: tracking.target_timestamp,
            })
            .ok();

        let publish_vmc = matches!(
            SESSION_MANAGER.read().settings().headset.vmc,
            Switch::Enabled(VMCConfig { publish: true, .. })
        );
        if publish_vmc {
            let orientation_correction = matches!(
                SESSION_MANAGER.read().settings().headset.vmc,
                Switch::Enabled(VMCConfig {
                    orientation_correction: true,
                    ..
                })
            );

            if let Some(sink) = &mut vmc_sink {
                let tracking_manager_lock = ctx.tracking_manager.read();
                let device_motions = device_motion_keys
                    .iter()
                    .map(move |id| {
                        (
                            *id,
                            tracking_manager_lock
                                .get_device_motion(*id, timestamp)
                                .unwrap(),
                        )
                    })
                    .collect::<Vec<(u64, DeviceMotion)>>();

                if let Some(skeleton) = tracking.hand_skeletons[0] {
                    sink.send_hand_tracking(HandType::Left, &skeleton, orientation_correction);
                }
                if let Some(skeleton) = tracking.hand_skeletons[1] {
                    sink.send_hand_tracking(HandType::Right, &skeleton, orientation_correction);
                }
                sink.send_tracking(&device_motions, orientation_correction);
            }
        }

        let track_body = matches!(
            SESSION_MANAGER.read().settings().headset.body_tracking,
            Switch::Enabled(BodyTrackingConfig { tracked: true, .. })
        );
        if track_body {
            if let Some(sink) = &mut body_tracking_sink {
                let tracking_manager_lock = ctx.tracking_manager.read();
                let device_motions = device_motion_keys
                    .iter()
                    .map(move |id| {
                        (
                            *id,
                            tracking_manager_lock
                                .get_device_motion(*id, timestamp)
                                .unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                sink.send_tracking(&device_motions);
            }
        }
    }
}
