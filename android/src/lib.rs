//! Android entry point for the VERA20k client.
//!
//! Android never runs `main`. `android.app.NativeActivity` loads the cdylib and
//! resolves `android_main` by symbol (the manifest names the library through
//! `android.app.lib_name`), so the process starts here instead. Everything
//! platform-specific that has to happen before the first window — the working
//! directory, logging, panic reporting — runs in this order and then hands over
//! to `vera20k::entry`, the same start-up the desktop binary uses.
//!
//! One process gets one `android_main` call per activity creation; the activity
//! is destroyed and recreated on rotation, a locale change or a configuration
//! change the manifest does not absorb. `EventLoop` can be built only once per
//! process (winit keeps it in a static), so a second call must not reach
//! `EventLoop::builder()` — see the guard below. Killing the process on that
//! path is deliberate for the first milestone: a stale surface is a black
//! screen, while a restart is recoverable. P5 replaces it with real
//! suspend/resume handling.
#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::JavaVM;
use jni::objects::{JObject, JString};
use jni::sys::jstring;
use winit::platform::android::EventLoopBuilderExtAndroid;

/// Run `f` with the activity this process belongs to.
///
/// The picks go through `ndk_context`'s raw pointers rather than
/// `AndroidApp`'s own helpers: the engine calls this from inside winit's event
/// callback, where android-activity's activity lock is already held, and taking
/// it again would deadlock the loop.
fn with_activity<R>(f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> R) -> Option<R> {
    let context = ndk_context::android_context();
    // SAFETY: android-activity published both handles at start-up and keeps the
    // Java VM alive for the process's lifetime; the activity reference is a JNI
    // global reference the loader owns.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(context.context().cast()) };
    Some(f(&mut env, &activity))
}

/// Folder-picker hook for `vera20k::util::folder_picker`.
///
/// The picker is the one thing the NDK cannot do itself: a Storage Access
/// Framework tree is readable only through Java's `ContentResolver`, so the
/// activity starts `ACTION_OPEN_DOCUMENT_TREE` and reports back through
/// [`Java_org_vera20k_VeraActivity_nativeOnFolderPicked`].
fn request_folder_picker() -> bool {
    with_activity(|env, activity| {
        env.call_method(activity, "startFolderPicker", "()V", &[])
            .is_ok()
    })
    .unwrap_or(false)
}

fn cancel_import() -> bool {
    with_activity(|env, activity| {
        env.call_method(activity, "cancelImport", "()Z", &[])
            .and_then(|value| value.z())
            .unwrap_or(false)
    })
    .unwrap_or(false)
}

/// Launcher and app-switcher icon hook for `vera20k::util::app_icon`.
///
/// The choice lives in the two activity aliases, which only `PackageManager`
/// can flip; the activity keeps that knowledge in [`VeraActivity.applyAppIcon`].
fn apply_app_icon(vera_icon: bool) {
    let applied = with_activity(|env, activity| {
        env.call_method(
            activity,
            "applyAppIcon",
            "(Z)V",
            &[jni::objects::JValue::Bool(u8::from(vera_icon))],
        )
        .is_ok()
    });
    if applied != Some(true) {
        log::warn!("Could not apply the app icon");
    }
}

/// LAN network hooks for `vera20k::util::lan_network`.
///
/// Android routes a datagram by the process's default network, so the LAN visit
/// runs bound to Wi-Fi while the lobby is open; the activity reports changes
/// back through [`Java_org_vera20k_VeraActivity_nativeOnLanNetworkChanged`].
fn bind_lan_network() {
    let bound = with_activity(|env, activity| {
        env.call_method(activity, "bindLanNetwork", "()V", &[]).is_ok()
    });
    if bound != Some(true) {
        log::warn!("Could not bind the LAN visit to the Wi-Fi network");
    }
}

fn release_lan_network() {
    let _ = with_activity(|env, activity| {
        env.call_method(activity, "releaseLanNetwork", "()V", &[])
            .is_ok()
    });
}

/// Default-handle suffix for `vera20k::util::device_identity`.
///
/// A LAN visit drops advertisements whose name matches the local one, and the
/// stock default is `[New Player]` on every install. Android has no text input
/// yet, so the engine appends the activity's Android ID suffix to the default
/// and two fresh installs no longer hide each other's games.
fn handle_suffix() -> Option<String> {
    let suffix = with_activity(|env, activity| {
        let value = env
            .call_method(activity, "defaultHandleSuffix", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        if value.is_null() {
            return None;
        }
        env.get_string(&JString::from(value))
            .ok()
            .map(|text| text.to_string_lossy().into_owned())
    })?;
    suffix.filter(|text| !text.is_empty())
}

/// The activity's folder-picker answer: the chosen tree as a `content://` URI,
/// or an empty string when the player cancelled.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vera20k_VeraActivity_nativeOnFolderPicked(
    mut env: jni::JNIEnv,
    activity: JObject,
    tree_uri: JString,
) {
    let picked = env
        .get_string(&tree_uri)
        .ok()
        .map(|text| text.to_string_lossy().into_owned())
        .filter(|text| !text.is_empty());
    match &picked {
        Some(uri) => {
            log::info!("asset import: folder picked: {uri}");
            // The dialog switches to its copying step before the copy's first
            // progress callback can arrive.
            vera20k::util::import_status::note(vera20k::util::import_status::ImportEvent::Started);
            // The copy itself runs on the Java side (only ContentResolver can
            // read a SAF tree); this starts it and returns immediately.
            if let Err(error) = env.call_method(
                &activity,
                "startImport",
                "(Ljava/lang/String;)V",
                &[jni::objects::JValue::Object(&tree_uri)],
            ) {
                let _ = env.exception_clear();
                vera20k::util::import_status::note(
                    vera20k::util::import_status::ImportEvent::Failed(format!(
                        "Could not start the copy: {error}"
                    )),
                );
            }
        }
        None => log::info!("asset import: folder picker cancelled"),
    }
    vera20k::util::folder_picker::deliver(picked);
}

/// The import finished: `ok:<path>` after the copy and config.toml are written,
/// `failed:<reason>` otherwise. Runs on the copy's own thread, so it only
/// records the outcome.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vera20k_VeraActivity_nativeImportFinished(
    mut env: jni::JNIEnv,
    _activity: JObject,
    result: JString,
) {
    let text = env
        .get_string(&result)
        .ok()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    use vera20k::util::import_status::{self, ImportEvent};
    if text == "cancelled" {
        import_status::note(ImportEvent::Cancelled);
    } else if let Some(path) = text.strip_prefix("ok:") {
        log::info!("asset import: copied into {path}; config.toml written");
        import_status::note(ImportEvent::Done(path.to_owned()));
    } else {
        let reason = text.strip_prefix("failed:").unwrap_or(&text).to_owned();
        log::error!("asset import: {reason}");
        import_status::note(ImportEvent::Failed(reason));
    }
    let _ = env.exception_clear();
}

/// Prepare the next config using the same editor as the desktop folder picker.
/// Java atomically publishes it only after the complete copy is ready.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vera20k_VeraActivity_nativeConfigForImport(
    mut env: jni::JNIEnv,
    _activity: JObject,
    directory: JString,
) -> jstring {
    let result = (|| -> anyhow::Result<jstring> {
        let directory: String = env.get_string(&directory)?.into();
        let config_path = vera20k::util::platform_dir::get()
            .ok_or_else(|| anyhow::anyhow!("The app data directory is unavailable"))?
            .join("config.toml");
        let previous = match std::fs::read_to_string(config_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        let contents = vera20k::util::config::set_paths_key(&previous, "ra2_dir", &directory);
        Ok(env.new_string(contents)?.into_raw())
    })();
    match result {
        Ok(contents) => contents,
        Err(error) => {
            let _ = env.throw_new("java/io/IOException", error.to_string());
            std::ptr::null_mut()
        }
    }
}

/// The import's copy progress, in bytes. Runs on the copy's own thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vera20k_VeraActivity_nativeImportProgress(
    _env: jni::JNIEnv,
    _activity: JObject,
    copied_bytes: jni::sys::jlong,
) {
    use vera20k::util::import_status::{self, ImportEvent};
    import_status::note(ImportEvent::Progress(copied_bytes.max(0) as u64));
}

/// The activity reports that its Wi-Fi network appeared, left or changed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_vera20k_VeraActivity_nativeOnLanNetworkChanged(
    _env: jni::JNIEnv,
    _activity: JObject,
) {
    vera20k::util::lan_network::note_changed();
}

/// Bind `nativeOnFolderPicked` to this activity's class.
///
/// Registered from `android_main`, not `JNI_OnLoad`: `FindClass` on the loading
/// thread uses the system class loader and cannot see `org.vera20k.VeraActivity`,
/// so the earlier attempt silently fell through to the symbol lookup that fails
/// on this device (`UnsatisfiedLinkError` for a name `readelf --dyn-syms` shows
/// in the packaged library). Here the activity object is at hand, and the class
/// it reports carries the app's own loader.
///
/// Returns whether the binding took; a caller reports it rather than letting a
/// later crash say so.
fn register_folder_picker_native() -> bool {
    with_activity(|env, activity| {
        let Ok(class) = env.get_object_class(activity) else {
            let _ = env.exception_clear();
            return false;
        };
        let methods = [
            jni::NativeMethod {
                name: "nativeOnFolderPicked".into(),
                sig: "(Ljava/lang/String;)V".into(),
                fn_ptr: Java_org_vera20k_VeraActivity_nativeOnFolderPicked as *mut std::ffi::c_void,
            },
            jni::NativeMethod {
                name: "nativeImportFinished".into(),
                sig: "(Ljava/lang/String;)V".into(),
                fn_ptr: Java_org_vera20k_VeraActivity_nativeImportFinished as *mut std::ffi::c_void,
            },
            jni::NativeMethod {
                name: "nativeImportProgress".into(),
                sig: "(J)V".into(),
                fn_ptr: Java_org_vera20k_VeraActivity_nativeImportProgress as *mut std::ffi::c_void,
            },
            jni::NativeMethod {
                name: "nativeConfigForImport".into(),
                sig: "(Ljava/lang/String;)Ljava/lang/String;".into(),
                fn_ptr: Java_org_vera20k_VeraActivity_nativeConfigForImport
                    as *mut std::ffi::c_void,
            },
            jni::NativeMethod {
                name: "nativeOnLanNetworkChanged".into(),
                sig: "()V".into(),
                fn_ptr: Java_org_vera20k_VeraActivity_nativeOnLanNetworkChanged
                    as *mut std::ffi::c_void,
            },
        ];
        let registered = env.register_native_methods(class, &methods).is_ok();
        if !registered {
            let _ = env.exception_clear();
        }
        registered
    })
    .unwrap_or(false)
}

/// Ask the system to keep the display on while this window is visible.
///
/// The intro movie runs for minutes with nothing to touch, and Android's own
/// display timeout would otherwise cut it off; no synthetic input event needs
/// to stand in for a tap.
///
/// Only called before the event loop starts, on purpose:
/// `AndroidApp::set_window_flags` takes the activity's lock, and winit holds it
/// for the whole of an event callback, so setting the flag from inside one (say,
/// when a movie begins) deadlocks the loop. The flag lives with the window, so
/// one call at start-up covers every screen until the activity is destroyed.
fn keep_screen_on(app: &AndroidApp) {
    use android_activity::WindowManagerFlags;
    app.set_window_flags(
        WindowManagerFlags::KEEP_SCREEN_ON,
        WindowManagerFlags::empty(),
    );
}

/// Resolved by `android-activity`'s loader trampoline; the name is the ABI.
///
/// Android calls the symbol with the activity's handle, which
/// `android-activity` turns into an `AndroidApp`.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    keep_screen_on(&app);
    vera20k::util::folder_picker::install(request_folder_picker, cancel_import);
    // The launcher icon is a compiled resource, so the VERA Icon box's choice
    // reaches the two activity aliases only through PackageManager.
    vera20k::util::app_icon::install(apply_app_icon);
    // LAN discovery rides the Wi-Fi network: the lobby binds this process to it
    // and the activity reports network changes back through the JNI callback.
    vera20k::util::lan_network::install(vera20k::util::lan_network::Hooks {
        acquire: bind_lan_network,
        release: release_lan_network,
    });
    // The stock LAN default name is the same on every install and two peers left
    // at it never see each other, so the activity supplies an Android ID suffix
    // for the handle until text input lands on this platform.
    vera20k::util::device_identity::install(vera20k::util::device_identity::Hooks {
        handle_suffix,
    });
    // Android starts every process in "/" with no writable home. Hand the app's
    // private folder to the engine before anything opens a file: it becomes
    // both the working directory and the settings folder, so logs, config.toml,
    // VERA.INI, saves and replays all persist there.
    match app.internal_data_path() {
        Some(dir) => {
            if let Err(error) = vera20k::entry::adopt_android_data_dir(&dir) {
                eprintln!("cannot use the app data directory: {error:#}");
            }
        }
        None => eprintln!("no internal data path; logs and settings will not persist"),
    }

    let log_path = vera20k::entry::init_logging();
    vera20k::entry::log_banner(log_path.as_deref());
    // Bind the picker's callback before the player can reach the button, and say
    // whether it took: a silent failure here is what turned a missing folder
    // picker into an `UnsatisfiedLinkError` at the worst moment.
    log::info!(
        "asset import: folder-picker callback registered: {}",
        register_folder_picker_native()
    );

    // Android can start a second activity in this same, still-cached process
    // after the player exits to the launcher and taps the icon again. A second
    // winit event loop is impossible (winit keeps process-wide state) and the
    // guard says so, but returning would leave the new activity on its splash
    // for ever. End the process instead, so that tap is a cold start.
    if !claim_event_loop() {
        log::error!("android_main called again; ending this process for a cold relaunch");
        log::logger().flush();
        std::process::exit(0);
    }

    let event_loop: winit::event_loop::EventLoop<()> = match winit::event_loop::EventLoop::builder()
        .with_android_app(app)
        .build()
    {
        Ok(event_loop) => event_loop,
        Err(error) => {
            log::error!("cannot create the Android event loop: {error}");
            return;
        }
    };

    // Android has no retail command line: the launcher starts an ordinary
    // interactive game. `-SPAWN`, replay and capture launches are desktop
    // automation and stay out of this path.
    let launch_mode = vera20k::app::frontend::launch::AppLaunchMode::Interactive(
        vera20k::app::frontend::startup_options::RetailStartupOptions::default(),
    );

    match vera20k::entry::run_with_loop(launch_mode, false, event_loop) {
        Ok(0) => {}
        // Android has no `-SPAWN` status to propagate; a non-zero one still
        // belongs in the log before the activity finishes.
        Ok(status) => log::warn!("VERA20k exited with status {status}"),
        Err(error) => log::error!("VERA20k stopped: {error:#}"),
    }
    log::logger().flush();

    // One process runs one game. Exiting here (rather than returning and letting
    // Android cache the process) is what makes the launcher's next tap start
    // from a clean slate: the event loop above cannot be rebuilt, so a cached
    // process would serve the new activity nothing but its splash.
    std::process::exit(0);
}

/// Claims this process's single event loop.
///
/// winit keeps its event loop in a static and panics on a second build, and a
/// panic inside the JNI callback aborts the process with no log line. The first
/// caller wins; every later one is told so the activity can finish quietly.
fn claim_event_loop() -> bool {
    use std::sync::OnceLock;
    static CLAIMED: OnceLock<()> = OnceLock::new();
    CLAIMED.set(()).is_ok()
}
