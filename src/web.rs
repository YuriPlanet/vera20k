//! Browser host boundary. App remains the owner of the frontend and match lifecycle.
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static CANVAS: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    static STATUS: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static WRITE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static REMOVE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static REPLAY: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static LIFECYCLE: RefCell<crate::browser_lifecycle::Lifecycle> = RefCell::new(Default::default());
    static ON_LIFECYCLE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static WAKE: RefCell<Option<winit::event_loop::EventLoopProxy<()>>> = const { RefCell::new(None) };
    /// `spawn_leave()` / `spawn_resign()` requests for the running launch.
    static SPAWN_ACTIONS: RefCell<std::collections::VecDeque<crate::app::SpawnAction>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
    static MUSIC: RefCell<Option<crate::audio::music::MusicPlayer>> = const { RefCell::new(None) };
    static SFX: RefCell<Option<crate::audio::sfx::SfxPlayer>> = const { RefCell::new(None) };
}

pub(crate) fn canvas() -> web_sys::HtmlCanvasElement {
    CANVAS.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("browser canvas installed before App")
            .clone()
    })
}

pub(crate) fn status(message: &str) {
    let callback = STATUS.with(|slot| slot.borrow().clone());
    if let Some(callback) = callback {
        let _ = callback.call1(&JsValue::NULL, &message.into());
    }
}

/// Install before starting. Receives structured match-ended, engine-exited,
/// and engine-error objects. Callback exceptions never veto engine shutdown.
#[wasm_bindgen]
pub fn set_lifecycle_callback(callback: js_sys::Function) {
    ON_LIFECYCLE.with(|slot| *slot.borrow_mut() = Some(callback));
}

fn emit_lifecycle(event: Option<serde_json::Value>) {
    let Some(event) = event else { return };
    // Release Rust borrows before calling user JS: it may register a callback
    // or attempt another launch synchronously.
    let callback = ON_LIFECYCLE.with(|slot| slot.borrow().clone());
    if let Some(callback) = callback {
        // Also reached from the panic hook, where a second panic would abort.
        let Ok(value) = js_sys::JSON::parse(&event.to_string()) else {
            return;
        };
        if let Err(error) = callback.call1(&JsValue::NULL, &value) {
            log::error!("Host lifecycle callback failed: {error:?}");
        }
    }
}

pub(crate) fn match_started() {
    LIFECYCLE.with(|slot| slot.borrow_mut().match_started());
}

pub(crate) fn match_running() -> bool {
    LIFECYCLE.with(|slot| slot.borrow().match_running())
}

pub(crate) fn match_ended(report: serde_json::Value) {
    emit_lifecycle(LIFECYCLE.with(|slot| slot.borrow_mut().match_ended(report)));
}

pub(crate) fn request_exit(error: Option<&str>) {
    LIFECYCLE.with(|slot| slot.borrow_mut().request_exit(error));
}

/// Called only once the external launch's network cleanup is finished.
/// The caller exits the event loop immediately after these notifications.
pub(crate) fn spawn_finished(report: &str) {
    if LIFECYCLE.with(|slot| slot.borrow().external_finished()) {
        return;
    }
    let report_value = serde_json::from_str(report).expect("launch report JSON");
    emit_lifecycle(LIFECYCLE.with(|slot| slot.borrow_mut().finish_external(report_value)));
    SPAWN_ACTIONS.with(|actions| actions.borrow_mut().clear());
    // Retained as a result notification for older launchers; repeat launches
    // now require a new page/instance.
    status(&format!("spawn:finished:{report}"));
}

pub(crate) fn engine_exited() {
    crate::audio::web_output::shutdown();
    MUSIC.with(|slot| slot.borrow_mut().take());
    SFX.with(|slot| slot.borrow_mut().take());
    WAKE.with(|slot| slot.borrow_mut().take());
    CANVAS.with(|slot| slot.borrow_mut().take());
    emit_lifecycle(LIFECYCLE.with(|slot| slot.borrow_mut().exited()));
}

/// The next `spawn_leave()` / `spawn_resign()` request, oldest first.
pub(crate) fn take_spawn_action() -> Option<crate::app::SpawnAction> {
    SPAWN_ACTIONS.with(|actions| actions.borrow_mut().pop_front())
}

fn request_spawn_action(action: crate::app::SpawnAction) -> Result<(), JsValue> {
    if !LIFECYCLE.with(|slot| slot.borrow().spawn_running()) {
        return Err(JsValue::from_str("No start_spawn launch is running"));
    }
    SPAWN_ACTIONS.with(|actions| actions.borrow_mut().push_back(action));
    WAKE.with(|slot| {
        if let Some(proxy) = slot.borrow().as_ref() {
            let _ = proxy.send_event(());
        }
    });
    Ok(())
}

/// docs/spawn.md "Scripted leave and resign": leave the running multiplayer
/// launch as the abort box's Leave does (on the score screen: Continue).
/// The request waits while the match loads or an outcome is being shown.
#[wasm_bindgen]
pub fn spawn_leave() -> Result<(), JsValue> {
    request_spawn_action(crate::app::SpawnAction::Leave)
}

/// docs/spawn.md "Scripted leave and resign": resign the running multiplayer
/// launch as the abort box's Observe does (the local House is destroyed and
/// the player keeps watching); ignored when the box offers no Observe.
#[wasm_bindgen]
pub fn spawn_resign() -> Result<(), JsValue> {
    request_spawn_action(crate::app::SpawnAction::Resign)
}

#[wasm_bindgen(start)]
pub fn initialize() {
    console_error_panic_hook::set_once();
    let console = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let error = format!("Runtime error: {info}");
        // A panic need not reach ApplicationHandler::exiting. Best effort
        // notification must not recursively borrow a lifecycle operation.
        let event = LIFECYCLE.with(|slot| {
            slot.try_borrow_mut()
                .ok()
                .and_then(|mut state| state.fatal(&error))
        });
        crate::audio::web_output::shutdown();
        emit_lifecycle(event);
        status(&error);
        console(info);
    }));
    let _ = console_log::init_with_level(log::Level::Info);
    crate::util::file_system::set_persistence_callbacks(
        |path, bytes, modified_ms| {
            WRITE.with(|slot| {
                if !path.starts_with("/saves/") {
                    return;
                }
                if let Some(callback) = slot.borrow().as_ref() {
                    if let Err(error) = callback.call3(
                        &JsValue::NULL,
                        &path.into(),
                        &js_sys::Uint8Array::from(bytes),
                        &(modified_ms as f64).into(),
                    ) {
                        log::error!("Host persistence callback failed: {error:?}");
                    }
                }
            })
        },
        |path| {
            REMOVE.with(|slot| {
                if !path.starts_with("/saves/") {
                    return;
                }
                if let Some(callback) = slot.borrow().as_ref() {
                    let _ = callback.call1(&JsValue::NULL, &path.into());
                }
            })
        },
    );
}

fn relative_path(name: &str) -> Result<(), JsValue> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', ':', '\0'])
        || name
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
    {
        return Err(JsValue::from_str("Expected a relative resource path"));
    }
    Ok(())
}

/// Import user-selected bytes before starting. The engine never fetches retail assets.
#[wasm_bindgen]
pub fn import_file(name: &str, bytes: Vec<u8>) -> Result<(), JsValue> {
    relative_path(name)?;
    if LIFECYCLE.with(|slot| slot.borrow().started()) {
        return Err(JsValue::from_str("Reload before replacing game resources"));
    }
    crate::util::file_system::import_file(format!("/retail/{name}"), bytes);
    Ok(())
}

/// Directory standing in for the desktop launcher's cwd: spawn.ini and the
/// client map beside it (docs/spawn.md).
const SPAWN_DIRECTORY: &str = "/spawn";

/// Import a file an external launch references, such as its client map.
#[wasm_bindgen]
pub fn import_spawn_file(name: &str, bytes: Vec<u8>) -> Result<(), JsValue> {
    relative_path(name)?;
    if LIFECYCLE.with(|slot| slot.borrow().started()) {
        return Err(JsValue::from_str(
            "Import launch files before starting; reload for another launch",
        ));
    }
    crate::util::file_system::import_file(format!("{SPAWN_DIRECTORY}/{name}"), bytes);
    Ok(())
}

/// Install durability callbacks before starting. A callback queues asynchronous host storage;
/// a successful engine write means the in-memory write completed, not that storage was flushed.
#[wasm_bindgen]
pub fn set_persistence_callbacks(write: js_sys::Function, remove: js_sys::Function) {
    WRITE.with(|slot| *slot.borrow_mut() = Some(write));
    REMOVE.with(|slot| *slot.borrow_mut() = Some(remove));
}

/// Install before starting to receive each closed replay segment as `(name, bytes)`.
/// Persistence is separate: whatever its callbacks report must be restorable.
#[wasm_bindgen]
pub fn set_replay_callback(callback: js_sys::Function) {
    REPLAY.with(|slot| *slot.borrow_mut() = Some(callback));
}

pub(crate) fn has_replay_callback() -> bool {
    REPLAY.with(|slot| slot.borrow().is_some())
}

pub(crate) fn hand_over_replay(name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    // Release the slot before calling user JS, which may register a callback.
    let Some(callback) = REPLAY.with(|slot| slot.borrow().clone()) else {
        return Ok(());
    };
    callback
        .call2(
            &JsValue::NULL,
            &name.into(),
            &js_sys::Uint8Array::from(bytes),
        )
        .map_err(|error| anyhow::anyhow!("Host replay callback failed: {error:?}"))?;
    Ok(())
}

#[wasm_bindgen]
pub fn restore_user_file(path: &str, bytes: Vec<u8>, modified_ms: f64) -> Result<(), JsValue> {
    if LIFECYCLE.with(|slot| slot.borrow().started()) {
        return Err(JsValue::from_str("Restore user data before starting"));
    }
    // Builds that sent replays through the persistence callback left them in
    // hosts' storage. Refusing one would abort that host's whole restore.
    if path.starts_with("/replays/") {
        return Ok(());
    }
    if !path.starts_with("/saves/") {
        return Err(JsValue::from_str("User data must be below /saves/"));
    }
    relative_path(&path[1..])?;
    if !modified_ms.is_finite() || modified_ms < 0.0 || modified_ms > 8_640_000_000_000_000.0 {
        return Err(JsValue::from_str("Invalid file timestamp"));
    }
    crate::util::file_system::restore_file(path, bytes, modified_ms as u64);
    Ok(())
}

/// Call synchronously from the user's Start click, before asynchronous file/storage work.
#[wasm_bindgen]
pub fn prepare_audio() -> Result<(), JsValue> {
    MUSIC.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = crate::audio::music::MusicPlayer::new();
        }
    });
    SFX.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = crate::audio::sfx::SfxPlayer::new();
        }
    });
    if MUSIC.with(|slot| slot.borrow().is_some()) && SFX.with(|slot| slot.borrow().is_some()) {
        Ok(())
    } else {
        Err(JsValue::from_str("Browser audio output unavailable"))
    }
}

pub(crate) fn take_music_output() -> Option<crate::audio::music::MusicPlayer> {
    MUSIC.with(|slot| slot.borrow_mut().take())
}
pub(crate) fn take_sfx_output() -> Option<crate::audio::sfx::SfxPlayer> {
    SFX.with(|slot| slot.borrow_mut().take())
}

#[wasm_bindgen]
pub fn start_game(
    canvas: web_sys::HtmlCanvasElement,
    on_status: js_sys::Function,
) -> Result<(), JsValue> {
    run(
        canvas,
        on_status,
        crate::app::App::new(
            crate::app::frontend::startup_options::RetailStartupOptions::default(),
        ),
        false,
    )
}

/// Start directly into spawn.ini's match. Its match-ended callback is
/// followed by engine-exited; starting another game requires a new instance.
#[wasm_bindgen]
pub fn start_spawn(
    canvas: web_sys::HtmlCanvasElement,
    on_status: js_sys::Function,
    spawn_ini: String,
) -> Result<(), JsValue> {
    if LIFECYCLE.with(|slot| slot.borrow().started()) {
        return Err(JsValue::from_str("Game already started; reload to restart"));
    }
    let request = read_spawn(spawn_ini)?;
    let mut app = crate::app::App::new(
        crate::app::frontend::startup_options::RetailStartupOptions::default(),
    );
    app.set_spawn_request(request);
    run(canvas, on_status, app, true)
}

fn read_spawn(
    spawn_ini: String,
) -> Result<crate::app::frontend::cncnet_spawn::SpawnRequest, JsValue> {
    let path = format!("{SPAWN_DIRECTORY}/spawn.ini");
    crate::util::file_system::import_file(&path, spawn_ini.into_bytes());
    crate::app::frontend::cncnet_spawn::SpawnRequest::read(std::path::Path::new(&path))
        .map_err(|error| JsValue::from_str(&format!("{error:#}")))
}

/// docs/spawn.md "Probe": the same report as desktop `vera20k --probe`, as a
/// JSON string, from the retail files imported with `import_file` and, when
/// `map_name` is given, the launch file imported with `import_spawn_file`.
/// It needs no running engine: call it before `start_game`/`start_spawn` or
/// after exit. It loads the rules synchronously (seconds).
#[wasm_bindgen]
pub fn probe(map_name: Option<String>) -> Result<String, JsValue> {
    let error = |error: anyhow::Error| JsValue::from_str(&format!("{error:#}"));
    install_config();
    let map = match map_name {
        Some(name) => {
            relative_path(&name)?;
            let path = format!("{SPAWN_DIRECTORY}/{name}");
            if !crate::util::file_system::is_file(&path) {
                return Err(JsValue::from_str(&format!("map not found: {name}")));
            }
            Some(path)
        }
        None => None,
    };
    let config = crate::util::config::GameConfig::load().map_err(error)?;
    let personal_rules =
        crate::app::spawn_probe::existing_personal_rules(config.paths.settings_dir())
            .map_err(error)?;
    let report = crate::app::spawn_probe::probe(
        &config,
        personal_rules,
        crate::app::frontend::startup_options::RetailStartupOptions::default().media_archive_mode,
        map.as_deref(),
    )
    .map_err(error)?;
    serde_json::to_string(&report).map_err(|error| JsValue::from_str(&error.to_string()))
}

fn install_config() {
    crate::util::file_system::import_file(
        "/config.toml",
        b"[paths]\nra2_dir = \"/retail\"\n".to_vec(),
    );
}

fn run(
    canvas: web_sys::HtmlCanvasElement,
    on_status: js_sys::Function,
    app: crate::app::App,
    external: bool,
) -> Result<(), JsValue> {
    use winit::platform::web::EventLoopExtWebSys;
    if LIFECYCLE.with(|slot| slot.borrow().started()) {
        return Err(JsValue::from_str("Game already started; reload to restart"));
    }
    let event_loop =
        winit::event_loop::EventLoop::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    LIFECYCLE
        .with(|slot| slot.borrow_mut().start(external))
        .map_err(JsValue::from_str)?;
    install_config();
    WAKE.with(|slot| *slot.borrow_mut() = Some(event_loop.create_proxy()));
    CANVAS.with(|slot| *slot.borrow_mut() = Some(canvas));
    STATUS.with(|slot| *slot.borrow_mut() = Some(on_status));
    status("Starting engine");
    event_loop.spawn_app(app);
    Ok(())
}
