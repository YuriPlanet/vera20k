//! Android's own Medium sans-serif glyphs, through public Paint/Canvas APIs.
//! Only coverage changes; GAME.FNT continues to own widths and row layout.
//! System fonts stay on the device and are not redistributed in the APK.

use jni::JavaVM;
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JValue};

pub(crate) struct Rasterizer {
    vm: JavaVM,
    helper: GlobalRef,
    rows: u32,
    reported_error: bool,
}

impl Rasterizer {
    pub(crate) fn new(rows: u32) -> Option<Self> {
        if rows == 0 || rows > 128 {
            return None;
        }
        let context = ndk_context::android_context();
        // SAFETY: NativeActivity owns the live VM and its global Activity
        // reference for the entire application/render thread lifetime.
        let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.ok()?;
        let helper = {
            let mut env = vm.attach_current_thread_permanently().ok()?;
            let result = env.with_local_frame(8, |env| {
                // A native-attached thread's FindClass cannot resolve APK
                // classes reliably. Use this Activity's application loader.
                let raw_env = env.get_raw();
                // SAFETY: The attached JNIEnv and borrowed Activity global are
                // live here. NewLocalRef creates a separate local owned by
                // this frame; the Activity's original global stays borrowed.
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
                // SAFETY: NewLocalRef returned a fresh, non-null local in the
                // current frame. No wrapper for this local exists elsewhere.
                let activity = unsafe { JObject::from_raw(raw_activity) };
                let loader = env
                    .call_method(
                        &activity,
                        "getClassLoader",
                        "()Ljava/lang/ClassLoader;",
                        &[],
                    )?
                    .l()?;
                let name = env.new_string("com.wannafire.vera20k.GlyphRasterizer")?;
                let class = env
                    .call_method(
                        loader,
                        "loadClass",
                        "(Ljava/lang/String;)Ljava/lang/Class;",
                        &[JValue::Object(name.as_ref())],
                    )?
                    .l()?;
                let object =
                    env.new_object(JClass::from(class), "(I)V", &[JValue::Int(rows as i32)])?;
                env.new_global_ref(object)
            });
            match result {
                Ok(helper) => helper,
                Err(error) => {
                    let _ = env.exception_describe();
                    let _ = env.exception_clear();
                    log::warn!("Android UI font helper unavailable: {error}");
                    return None;
                }
            }
        };
        Some(Self {
            vm,
            helper,
            rows,
            reported_error: false,
        })
    }

    pub(super) fn face_names(&self) -> (String, String) {
        (
            "Android sans-serif-medium".into(),
            "Android sans-serif-medium zh-CN".into(),
        )
    }

    pub(crate) fn glyph(&mut self, codepoint: u16, width: u32) -> Option<Vec<u8>> {
        if width == 0 || width > 128 || (0xd800..=0xdfff).contains(&codepoint) {
            return None;
        }
        let expected = (width as usize).checked_mul(self.rows as usize)?;
        let mut env = self.vm.attach_current_thread_permanently().ok()?;
        let result: jni::errors::Result<Option<Vec<u8>>> = env.with_local_frame(4, |env| {
            let bitmap = env
                .call_method(
                    self.helper.as_obj(),
                    "rasterize",
                    "(II)[B",
                    &[JValue::Int(i32::from(codepoint)), JValue::Int(width as i32)],
                )?
                .l()?;
            if bitmap.is_null() {
                return Ok(None);
            }
            env.convert_byte_array(JByteArray::from(bitmap)).map(Some)
        });
        let alpha = match result {
            Ok(Some(alpha)) if alpha.len() == expected => alpha,
            Ok(_) => return None,
            Err(error) => {
                let _ = env.exception_clear();
                if !self.reported_error {
                    log::warn!(
                        "Android glyph rasterization failed; using retail fallback: {error}"
                    );
                    self.reported_error = true;
                }
                return None;
            }
        };
        let mut rgba = vec![255; expected.checked_mul(4)?];
        for (pixel, coverage) in rgba.chunks_exact_mut(4).zip(alpha) {
            pixel[3] = coverage;
        }
        Some(rgba)
    }
}
