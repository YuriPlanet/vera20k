//! Browser host boundary. App remains the owner of the frontend and match lifecycle.
use std::cell::{Cell, RefCell};
use wasm_bindgen::prelude::*;

thread_local! {
    static CANVAS: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    static STATUS: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static WRITE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static REMOVE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    /// Engine start and repeat-launch state (docs/spawn.md).
    static GATE: Cell<crate::app::BrowserSpawnGate> = Cell::new(Default::default());
    static QUEUED_SPAWN: RefCell<Option<crate::app::frontend::cncnet_spawn::SpawnRequest>> =
        const { RefCell::new(None) };
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
    STATUS.with(|slot| {
        if let Some(callback) = slot.borrow().as_ref() {
            let _ = callback.call1(&JsValue::NULL, &message.into());
        }
    });
}

/// The external launch ended: `spawn:finished:<report json>` (the plain
/// `spawn:finished` prefix is kept for existing consumers), after which the
/// page may import a new map and call `start_spawn` again.
pub(crate) fn spawn_finished(report: &str) {
    GATE.with(|gate| {
        let mut state = gate.get();
        state.finished();
        gate.set(state);
    });
    SPAWN_ACTIONS.with(|actions| actions.borrow_mut().clear());
    status(&format!("spawn:finished:{report}"));
}

/// The next `spawn_leave()` / `spawn_resign()` request, oldest first.
pub(crate) fn take_spawn_action() -> Option<crate::app::SpawnAction> {
    SPAWN_ACTIONS.with(|actions| actions.borrow_mut().pop_front())
}

fn request_spawn_action(action: crate::app::SpawnAction) -> Result<(), JsValue> {
    if !gate().spawn_running() {
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

/// The launch the page queued with `start_spawn` after the previous ended.
pub(crate) fn take_queued_spawn() -> Option<crate::app::frontend::cncnet_spawn::SpawnRequest> {
    QUEUED_SPAWN.with(|slot| slot.borrow_mut().take())
}

fn gate() -> crate::app::BrowserSpawnGate {
    GATE.with(Cell::get)
}

#[wasm_bindgen(start)]
pub fn initialize() {
    console_error_panic_hook::set_once();
    let console = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        status(&format!("Runtime error: {info}"));
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
    if !gate().may_replace_resources() {
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
    if !gate().may_replace_launch_files() {
        return Err(JsValue::from_str(
            "Launch files can change only before a launch or after spawn:finished",
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

#[wasm_bindgen]
pub fn restore_user_file(path: &str, bytes: Vec<u8>, modified_ms: f64) -> Result<(), JsValue> {
    if gate().started() {
        return Err(JsValue::from_str("Restore user data before starting"));
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
    let mut next = gate();
    next.start_game().map_err(JsValue::from_str)?;
    GATE.with(|gate| gate.set(next));
    run(
        canvas,
        on_status,
        crate::app::App::new(
            crate::app::frontend::startup_options::RetailStartupOptions::default(),
        ),
    )
}

/// Start directly into the match a launcher described. The text is read exactly
/// like the desktop `-SPAWN` file, with the map resolved in the spawn directory;
/// `on_status` receives `spawn:finished:<json>` once the match ends. After
/// that, the same page may call it again with the same canvas: the running
/// engine (and its imported retail files) starts the next launch.
#[wasm_bindgen]
pub fn start_spawn(
    canvas: web_sys::HtmlCanvasElement,
    on_status: js_sys::Function,
    spawn_ini: String,
) -> Result<(), JsValue> {
    let start = gate().start_spawn().map_err(JsValue::from_str)?;
    if start == crate::app::BrowserSpawnStart::QueueNext {
        let same_canvas = CANVAS.with(|slot| {
            slot.borrow().as_ref().is_some_and(|installed| {
                AsRef::<JsValue>::as_ref(installed) == AsRef::<JsValue>::as_ref(&canvas)
            })
        });
        if !same_canvas {
            return Err(JsValue::from_str(
                "A repeat launch must use the first launch's canvas",
            ));
        }
        let request = read_spawn(spawn_ini)?;
        STATUS.with(|slot| *slot.borrow_mut() = Some(on_status));
        QUEUED_SPAWN.with(|slot| *slot.borrow_mut() = Some(request));
        GATE.with(|gate| {
            let mut state = gate.get();
            state.commit(start);
            gate.set(state);
        });
        status("Starting launch");
        // Wake the event loop so the App takes the request.
        WAKE.with(|slot| {
            if let Some(proxy) = slot.borrow().as_ref() {
                let _ = proxy.send_event(());
            }
        });
        return Ok(());
    }
    let request = read_spawn(spawn_ini)?;
    let mut app = crate::app::App::new(
        crate::app::frontend::startup_options::RetailStartupOptions::default(),
    );
    app.set_spawn_request(request);
    GATE.with(|gate| {
        let mut state = gate.get();
        state.commit(start);
        gate.set(state);
    });
    run(canvas, on_status, app)
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
/// between launches. It loads the rules synchronously (seconds).
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
) -> Result<(), JsValue> {
    use winit::platform::web::EventLoopExtWebSys;
    let event_loop =
        winit::event_loop::EventLoop::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    install_config();
    WAKE.with(|slot| *slot.borrow_mut() = Some(event_loop.create_proxy()));
    CANVAS.with(|slot| *slot.borrow_mut() = Some(canvas));
    STATUS.with(|slot| *slot.borrow_mut() = Some(on_status));
    status("Starting engine");
    event_loop.spawn_app(app);
    Ok(())
}
