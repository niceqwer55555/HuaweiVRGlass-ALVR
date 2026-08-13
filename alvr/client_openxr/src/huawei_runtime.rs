use jni::{
    objects::{JClass, JObject, JValue},
    JavaVM,
};

pub fn initialize() -> Result<(), String> {
    let android_context = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(android_context.vm().cast()) }
        .map_err(|error| format!("无法访问 Java VM：{error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("无法附加 Java 线程：{error}"))?;

    let activity = unsafe { JObject::from_raw(android_context.context().cast()) };
    let class_loader = env
        .call_method(
            &activity,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .and_then(|value| value.l())
        .map_err(|error| format!("无法取得应用 ClassLoader：{error}"))?;
    let class_name = env
        .new_string("com.huawei.hvr.LibUpdateClient")
        .map_err(|error| format!("无法创建 bridge 类名：{error}"))?;
    let class_name_object = JObject::from(class_name);
    let client_class_object = env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name_object)],
        )
        .and_then(|value| value.l())
        .map_err(|error| format!("APK 缺少华为 hvrbridge：{error}"))?;
    let client_class = JClass::from(client_class_object);
    let client = env
        .new_object(
            &client_class,
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .map_err(|error| format!("无法创建华为运行时更新客户端：{error}"))?;
    let initialized = env
        .call_method(&client, "runUpdate", "()Z", &[])
        .and_then(|value| value.z())
        .map_err(|error| format!("华为运行时更新调用失败：{error}"))?;

    // android-activity owns this global reference. Avoid treating it as a local JNI reference.
    let _ = activity.into_raw();

    initialized
        .then_some(())
        .ok_or_else(|| "华为 VR SDK Service 初始化返回失败".into())
}
