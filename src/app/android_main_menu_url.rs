//! Fixed official-site action through the existing Android Activity.
pub(super) fn open_website() -> bool {
    let result = (|| -> jni::errors::Result<()> {
        let context = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }?;
        let mut env = vm.attach_current_thread()?;
        let result = env.with_local_frame(2, |env| {
            let raw_env = env.get_raw();
            // SAFETY: This attached JNIEnv and the Activity global are live.
            // Copy the borrowed global into this frame without owning or
            // deleting the Activity's original reference.
            let raw_activity = unsafe {
                let new_local_ref = (**raw_env)
                    .NewLocalRef
                    .ok_or(jni::errors::Error::JNIEnvMethodNotFound("NewLocalRef"))?;
                new_local_ref(raw_env, context.context().cast())
            };
            if env.exception_check()? {
                return Err(jni::errors::Error::JavaException);
            }
            if raw_activity.is_null() {
                return Err(jni::errors::Error::NullPtr(
                    "Android Activity local reference",
                ));
            }
            // SAFETY: This fresh local belongs to the active local frame and
            // has no other JObject wrapper.
            let activity = unsafe { jni::objects::JObject::from_raw(raw_activity) };
            env.call_method(&activity, "openWebsite", "()V", &[])?;
            Ok(())
        });
        if result.is_err() {
            // This native callback returns to the engine, rather than Java.
            // Do not leave a failed call's exception on its render thread.
            let _ = env.exception_clear();
        }
        result
    })();
    if let Err(error) = result {
        log::warn!("Could not open official website: {error}");
        return false;
    }
    true
}
