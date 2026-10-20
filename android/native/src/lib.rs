//! Android's process and APK asset bootstrap. Gameplay uses the shared App.
use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::android::{EventLoopBuilderExtAndroid, activity::AndroidApp};
use winit::window::WindowId;

static ACTIVITY_RUNNING: AtomicBool = AtomicBool::new(false);
static ACTIVITY_DESTROYED: AtomicBool = AtomicBool::new(false);
static DESTROY_WAKE: Mutex<Option<EventLoopProxy<()>>> = Mutex::new(None);

// winit 0.30 does not forward Android MainEvent::Destroy. The Java owner
// signals before NativeActivity.onDestroy waits for this thread to finish.
#[unsafe(no_mangle)]
extern "system" fn Java_com_wannafire_vera20k_MainActivity_nativeActivityDestroy(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) {
    if !ACTIVITY_RUNNING.load(Ordering::Acquire) {
        return;
    }
    ACTIVITY_DESTROYED.store(true, Ordering::Release);
    let wake = DESTROY_WAKE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(proxy) = wake.as_ref() {
        let _ = proxy.send_event(());
    }
}

#[derive(Deserialize)]
struct AssetManifest {
    asset_set: String,
    files: Vec<AssetFile>,
}

#[derive(Deserialize)]
struct AssetFile {
    path: String,
    bytes: u64,
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    ACTIVITY_DESTROYED.store(false, Ordering::Release);
    ACTIVITY_RUNNING.store(true, Ordering::Release);
    if let Err(error) = run(app) {
        log::error!("Android startup failed: {error:#}");
        eprintln!("Android startup failed: {error:#}");
    }
    ACTIVITY_RUNNING.store(false, Ordering::Release);
    *DESTROY_WAKE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = None;
}

fn run(android: AndroidApp) -> Result<()> {
    let runtime = android
        .internal_data_path()
        .context("Android internal files directory is unavailable")?
        .join("runtime");
    fs::create_dir_all(runtime.join("logs")).context("create Android bootstrap log directory")?;
    std::env::set_current_dir(&runtime).context("select writable Android runtime")?;
    // Publish once before starting engine workers; the shared subtitle owner
    // uses the same extracted retail root as the ordinary config loader.
    unsafe {
        std::env::set_var("RA2_DIR", runtime.join("retail-assets"));
    }
    let log_path = match vera20k::util::logging::init_file_logger("ra2") {
        Ok(path) => Some(path),
        Err(error) => {
            eprintln!("Android file logger unavailable: {error}");
            vera20k::util::logging::init_stderr_logger("info")
                .map_err(|error| anyhow::anyhow!("Android fallback logger failed: {error}"))?;
            None
        }
    };
    vera20k::util::logging::install_panic_hook(log_path.as_deref());
    let preparation_app = android.clone();
    let event_loop = EventLoop::builder().with_android_app(android).build()?;
    let proxy = event_loop.create_proxy();
    *DESTROY_WAKE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(proxy.clone());
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (sender, receiver) = mpsc::channel();
    bootstrap_log(
        "Android native bootstrap: background asset preparation started; lifecycle polling active",
    );
    let worker = std::thread::Builder::new()
        .name("vera-android-assets".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                prepare(&preparation_app, &worker_cancel)
            }))
            .unwrap_or_else(|panic| {
                Err(anyhow::anyhow!(
                    "Android asset worker panicked: {}",
                    panic_text(panic)
                ))
            });
            // Publish before waking: the poller must never sleep after observing
            // a wake whose worker result has not yet become visible.
            if sender.send(result).is_ok() {
                let _ = proxy.send_event(());
            }
        })
        .context("start Android asset preparation worker")?;
    let mut bootstrap = Bootstrap {
        app: None,
        worker: Some(worker),
        receiver,
        cancel,
        ready: false,
        resumed: false,
        failure: None,
    };
    let event_result = event_loop.run_app(&mut bootstrap);
    bootstrap.stop_worker();
    event_result?;
    if let Some(error) = bootstrap.failure.take() {
        return Err(error);
    }
    if let Some(app) = bootstrap.app.as_mut() {
        app.finish_capture()?;
    }
    log::logger().flush();
    Ok(())
}

/// This host owns only readiness/lifecycle. It creates exactly one ordinary
/// App, and every event after handoff keeps that same gameplay owner.
struct Bootstrap {
    app: Option<vera20k::app::App>,
    worker: Option<JoinHandle<()>>,
    receiver: mpsc::Receiver<Result<PathBuf>>,
    cancel: Arc<AtomicBool>,
    ready: bool,
    resumed: bool,
    failure: Option<anyhow::Error>,
}

impl Bootstrap {
    fn poll_ready(&mut self, event_loop: &ActiveEventLoop) {
        if ACTIVITY_DESTROYED.load(Ordering::Acquire) {
            self.cancel.store(true, Ordering::Release);
            bootstrap_log(
                "Android Activity destroyed; cancelling bootstrap and exiting native loop",
            );
            event_loop.exit();
            return;
        }
        if event_loop.exiting() || self.failure.is_some() {
            return;
        }
        if !self.ready {
            match self.receiver.try_recv() {
                Ok(Ok(runtime)) => {
                    self.ready = true;
                    bootstrap_log(&format!(
                        "Android assets ready at {}; waiting for active SurfaceView={}",
                        runtime.display(),
                        !self.resumed
                    ));
                    self.stop_worker();
                    if self.failure.is_some() {
                        event_loop.exit();
                        return;
                    }
                }
                Ok(Err(error)) => {
                    self.fail(event_loop, error);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.fail(
                        event_loop,
                        anyhow::anyhow!("Android asset worker exited without a result"),
                    );
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.ready && self.resumed && self.app.is_none() {
            bootstrap_log(
                "Android bootstrap handoff: initializing shared App on the native event thread",
            );
            let mut app = vera20k::app::App::new(Default::default());
            app.resumed(event_loop);
            self.app = Some(app);
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        eprintln!("Android bootstrap initialization failed: {error:#}");
        log::error!("Android bootstrap initialization failed: {error:#}");
        self.failure = Some(error);
        self.cancel.store(true, Ordering::Release);
        event_loop.exit();
    }

    fn stop_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            if let Err(panic) = worker.join() {
                let error =
                    anyhow::anyhow!("Android asset worker join failed: {}", panic_text(panic));
                eprintln!("{error:#}");
                log::error!("{error:#}");
                self.failure.get_or_insert(error);
            }
        }
    }
}

impl Drop for Bootstrap {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.stop_worker();
    }
}

impl ApplicationHandler for Bootstrap {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let had_app = self.app.is_some();
        self.resumed = true;
        bootstrap_log("Android bootstrap received Resumed");
        self.poll_ready(event_loop);
        if had_app
            && !event_loop.exiting()
            && let Some(app) = self.app.as_mut()
        {
            app.resumed(event_loop);
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed = false;
        if let Some(app) = self.app.as_mut() {
            app.suspended(event_loop);
        } else {
            bootstrap_log("Android bootstrap suspended; App handoff deferred until next Resumed");
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        self.poll_ready(event_loop);
        if !event_loop.exiting()
            && let Some(app) = self.app.as_mut()
        {
            app.user_event(event_loop, event);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if ACTIVITY_DESTROYED.load(Ordering::Acquire) {
            self.poll_ready(event_loop);
            return;
        }
        if matches!(event, WindowEvent::Destroyed) {
            self.cancel.store(true, Ordering::Release);
            self.resumed = false;
            if let Some(app) = self.app.as_mut() {
                app.suspended(event_loop);
            }
            event_loop.exit();
        } else if self.app.is_none() && matches!(event, WindowEvent::CloseRequested) {
            self.cancel.store(true, Ordering::Release);
            bootstrap_log("Android bootstrap closed before App handoff");
            event_loop.exit();
        }
        if let Some(app) = self.app.as_mut() {
            app.window_event(event_loop, id, event);
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if let Some(app) = self.app.as_mut() {
            app.new_events(event_loop, cause);
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        if let Some(app) = self.app.as_mut() {
            app.device_event(event_loop, id, event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_ready(event_loop);
        if event_loop.exiting() {
            return;
        }
        if let Some(app) = self.app.as_mut() {
            app.about_to_wait(event_loop);
        } else if !event_loop.exiting() {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = self.app.as_mut() {
            app.memory_warning(event_loop);
        }
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.cancel.store(true, Ordering::Release);
        if let Some(app) = self.app.as_mut() {
            app.exiting(event_loop);
        }
        self.stop_worker();
        bootstrap_log("Android native event loop stopped; asset worker joined");
    }
}

fn bootstrap_log(message: &str) {
    log::info!("{message}");
    eprintln!("{message}");
}

fn panic_text(panic: Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|value| value.to_string()))
        .unwrap_or_else(|| "non-string panic payload".into())
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Acquire),
        "Android asset preparation cancelled by lifecycle exit"
    );
    Ok(())
}

fn prepare(app: &AndroidApp, cancel: &AtomicBool) -> Result<PathBuf> {
    let started = Instant::now();
    check_cancel(cancel)?;
    let runtime = app
        .internal_data_path()
        .context("Android internal files directory is unavailable")?
        .join("runtime");
    let assets = runtime.join("retail-assets");
    let profile = runtime.join("profile");
    for directory in [
        &assets,
        &profile,
        &runtime.join("saves"),
        &runtime.join("logs"),
    ] {
        fs::create_dir_all(directory).context("create Android runtime directories")?;
    }
    let manager = app.asset_manager();
    let mut manifest_asset = manager
        .open(c"retail-manifest.json")
        .context("APK retail asset manifest is missing")?;
    let mut bytes = Vec::new();
    manifest_asset.read_to_end(&mut bytes)?;
    let manifest: AssetManifest =
        serde_json::from_slice(&bytes).context("parse APK asset manifest")?;
    ensure!(
        !manifest.asset_set.is_empty() && !manifest.files.is_empty(),
        "APK asset manifest is empty"
    );
    let marker = runtime.join("asset-set.txt");
    let installed = fs::read_to_string(&marker).unwrap_or_default() == manifest.asset_set;
    bootstrap_log(&format!(
        "Android asset manifest: {} files, {} MiB; existing asset-set={installed}",
        manifest.files.len(),
        manifest.files.iter().map(|file| file.bytes).sum::<u64>() / (1024 * 1024)
    ));
    let mut copy_buffer = vec![0; 1024 * 1024];
    let mut copied_bytes = 0_u64;
    let mut copied_files = 0;
    let mut progress_at = Instant::now();
    for file in &manifest.files {
        check_cancel(cancel)?;
        let relative = Path::new(&file.path);
        ensure!(
            !file.path.is_empty()
                && relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "unsafe asset path in APK manifest"
        );
        let target = assets.join(relative);
        if installed
            && target
                .metadata()
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() == file.bytes)
        {
            continue;
        }
        let name = CString::new(format!("retail-assets/{}", file.path))?;
        let mut source = manager
            .open(&name)
            .with_context(|| format!("APK asset missing: {}", file.path))?;
        ensure!(
            source.length() as u64 == file.bytes,
            "APK asset length differs from manifest"
        );
        fs::create_dir_all(target.parent().context("asset parent is missing")?)?;
        let temporary = target.with_extension("vera-installing");
        let mut output = fs::File::create(&temporary)?;
        loop {
            check_cancel(cancel)?;
            let count = source.read(&mut copy_buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&copy_buffer[..count])?;
            copied_bytes += count as u64;
            if progress_at.elapsed() >= Duration::from_secs(4) {
                bootstrap_log(&format!(
                    "Android assets: copied {copied_files} files / {} MiB; current={}",
                    copied_bytes / (1024 * 1024),
                    file.path
                ));
                progress_at = Instant::now();
            }
        }
        output.flush()?;
        ensure!(
            output.metadata()?.len() == file.bytes,
            "extracted asset length differs from manifest"
        );
        drop(output);
        fs::rename(temporary, target)?;
        copied_files += 1;
    }
    check_cancel(cancel)?;
    fs::write(marker, &manifest.asset_set)?;
    let settings = profile.join("RA2MD.INI");
    if !settings.exists() && assets.join("RA2MD.INI").is_file() {
        fs::copy(assets.join("RA2MD.INI"), settings)?;
    }
    fs::write(
        runtime.join("config.toml"),
        format!(
            "[paths]\nra2_dir = {:?}\nsettings_dir = {:?}\n\n[graphics]\nextra_animations = true\n\n[gameplay]\ninput_delay_ticks = 2\npause_on_focus_loss = true\n\n[profile]\nname = \"BXY\"\n",
            assets.to_string_lossy(),
            profile.to_string_lossy(),
        ),
    )?;
    bootstrap_log(&format!(
        "Android asset preparation completed: copied {copied_files} files / {} MiB in {:.1}s",
        copied_bytes / (1024 * 1024),
        started.elapsed().as_secs_f32()
    ));
    Ok(runtime)
}
