use alvr_common::{error, info};
use jni::{objects::JObject, JNIEnv};
use std::sync::{Mutex, OnceLock};

static ACTIVITY_REF: OnceLock<jni::objects::GlobalRef> = OnceLock::new();
static RENDER_THREAD: OnceLock<Mutex<Option<std::thread::JoinHandle<()>>>> = OnceLock::new();

fn start(env: &JNIEnv<'_>, activity: &JObject<'_>, surface: &JObject<'_>) -> Result<(), String> {
    if RENDER_THREAD
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "无法锁定 Huawei 渲染线程状态".to_owned())?
        .is_some()
    {
        return Ok(());
    }

    let vm = env
        .get_java_vm()
        .map_err(|error| format!("无法取得 Java VM：{error}"))?;
    let activity_ref = env
        .new_global_ref(activity)
        .map_err(|error| format!("无法持有 Huawei Activity：{error}"))?;
    let activity_ptr = activity_ref.as_obj().as_raw();
    ACTIVITY_REF
        .set(activity_ref)
        .map_err(|_| "Huawei Activity 已经初始化".to_owned())?;

    unsafe {
        ndk_context::initialize_android_context(
            vm.get_java_vm_pointer().cast(),
            activity_ptr.cast(),
        );
    }

    let native_window = unsafe {
        ndk::native_window::NativeWindow::from_surface(env.get_raw().cast(), surface.as_raw())
    }
    .ok_or_else(|| "无法从 SurfaceHolder 取得 ANativeWindow".to_owned())?;

    info!(
        "[HVR-DIAG] Huawei Java Surface acquired: {}x{}",
        native_window.width(),
        native_window.height()
    );
    let rendering_thread = super::spawn_rendering_thread(Some(native_window));
    *RENDER_THREAD
        .get()
        .unwrap()
        .lock()
        .map_err(|_| "无法保存 Huawei 渲染线程状态".to_owned())? = Some(rendering_thread);

    Ok(())
}

fn stop() -> Result<(), String> {
    super::request_huawei_exit();

    let rendering_thread = RENDER_THREAD
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "无法锁定 Huawei 渲染线程状态".to_owned())?
        .take();

    if let Some(rendering_thread) = rendering_thread {
        rendering_thread
            .join()
            .map_err(|_| "Huawei 渲染线程退出时发生 panic".to_owned())?;
    }

    unsafe { ndk_context::release_android_context() };
    info!("[HVR-DIAG] Huawei Java Activity native resources stopped");

    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_lht_huaweivr_alvr_HuaweiVrActivity_nativeStart(
    env: JNIEnv<'_>,
    activity: JObject<'_>,
    surface: JObject<'_>,
) {
    alvr_client_core::init_logging();
    info!("[HVR-DIAG] Huawei Java Activity JNI entry entered");
    if let Err(error) = start(&env, &activity, &surface) {
        error!("[HVR-DIAG] Huawei Java Activity startup failed: {error}");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_lht_huaweivr_alvr_HuaweiVrActivity_nativeStop(
    _env: JNIEnv<'_>,
    _activity: JObject<'_>,
) {
    info!("[HVR-DIAG] Huawei Java Activity JNI stop entered");
    if let Err(error) = stop() {
        error!("[HVR-DIAG] Huawei Java Activity shutdown failed: {error}");
    }
}
