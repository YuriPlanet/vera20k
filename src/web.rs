//! Browser host boundary. App remains the owner of the frontend and match lifecycle.
pub(crate) mod ime;
#[cfg(feature = "wasm-threads")]
pub(crate) mod sim_worker;

use std::cell::RefCell;
use wasm_bindgen::prelude::*;

// The host awaits `initThreadPool(workers)` after `init()`, before any
// simulation work. Without it the same package runs that work serially.
#[cfg(feature = "wasm-threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

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
    /// The mod folder the next launch plays (`set_mod`); `None` is the original game.
    static MOD: RefCell<Option<String>> = const { RefCell::new(None) };
    /// A file arriving in parts (`import_begin`, `import_part`).
    static PENDING: RefCell<Option<PendingImport>> = const { RefCell::new(None) };
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
    crate::audio::web_output::pause();
    remove_spawn_files();
    ime::remove();
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
    wake();
    Ok(())
}

/// Run the event loop's next pass now, even while it waits.
fn wake() {
    WAKE.with(|slot| {
        if let Some(proxy) = slot.borrow().as_ref() {
            let _ = proxy.send_event(());
        }
    });
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
        // A worker reaches neither the host nor the page: its message waits
        // for the main thread, which panics with it (`SimHost`).
        #[cfg(feature = "wasm-threads")]
        if web_sys::window().is_none() {
            sim_worker::note_worker_panic(&info.to_string());
            console(info);
            return;
        }
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
    // The page's first status lines come before the engine reads its
    // settings: they follow the browser's languages until then.
    crate::i18n::init(None, None);
    // Fetched while the player picks the game folder; `fonts_ready` waits.
    let _ = crate::render::text_engine::web_fonts::preload();
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
    if !LIFECYCLE.with(|slot| slot.borrow().idle()) {
        return Err(JsValue::from_str(
            "Import launch files before starting or after the engine exited",
        ));
    }
    crate::util::file_system::import_file(format!("{SPAWN_DIRECTORY}/{name}"), bytes);
    Ok(())
}

/// A mod's folder name in the in-memory Mods folder, as config.toml's
/// `[paths] mod` names it: one plain folder, nothing a TOML string or a path
/// would read differently.
fn mod_folder(folder: &str) -> Result<(), JsValue> {
    if folder.is_empty()
        || folder.len() > 64
        || folder.starts_with('.')
        || folder.contains(['/', '\\', ':', '"', '\0'])
        || folder.chars().any(char::is_control)
    {
        return Err(JsValue::from_str("Expected a mod folder name"));
    }
    Ok(())
}

fn idle(what: &str) -> Result<(), JsValue> {
    if LIFECYCLE.with(|slot| slot.borrow().idle()) {
        Ok(())
    } else {
        Err(JsValue::from_str(&format!(
            "{what} before starting or after the engine exited"
        )))
    }
}

fn mod_path(folder: &str, name: &str) -> String {
    format!("{}/{folder}/{name}", crate::util::config::WEB_MODS_DIR)
}

/// Import one file of a mod into the in-memory Mods folder, as `name` inside
/// its folder. Between launches, as launch files are: a page plays one mod
/// in one game and another in the next (`set_mod`, `remove_mod`).
#[wasm_bindgen]
pub fn import_mod_file(folder: &str, name: &str, bytes: Vec<u8>) -> Result<(), JsValue> {
    mod_folder(folder)?;
    relative_path(name)?;
    idle("Import mod files")?;
    crate::util::file_system::import_file(mod_path(folder, name), bytes);
    Ok(())
}

/// Drop a mod's files from engine memory, before importing another. Returns
/// how many went. wasm memory does not shrink, but the next import reuses it.
#[wasm_bindgen]
pub fn remove_mod(folder: &str) -> Result<u32, JsValue> {
    mod_folder(folder)?;
    idle("Remove mod files")?;
    if MOD.with(|slot| slot.borrow().as_deref() == Some(folder)) {
        MOD.with(|slot| *slot.borrow_mut() = None);
    }
    let path = format!("{}/{folder}", crate::util::config::WEB_MODS_DIR);
    Ok(crate::util::file_system::forget_tree(path) as u32)
}

/// The mod the next `start_spawn`, `start_game` or `probe` plays: a folder
/// imported with `import_mod_file`, or none for the original game. It reads
/// as config.toml's `[paths] mod` does on the desktop (app::mods).
#[wasm_bindgen]
pub fn set_mod(folder: Option<String>) -> Result<(), JsValue> {
    if let Some(folder) = &folder {
        mod_folder(folder)?;
    }
    idle("Choose the mod")?;
    MOD.with(|slot| *slot.borrow_mut() = folder);
    Ok(())
}

/// A file being imported in parts: its place and its bytes so far.
struct PendingImport {
    path: String,
    bytes: std::sync::Arc<[std::mem::MaybeUninit<u8>]>,
    filled: usize,
}

/// The largest file `import_begin` takes: under 2 GiB, as no game or mod
/// file comes near.
const MAX_IMPORT_BYTES: f64 = 2_147_483_647.0;

/// Begin importing a file in parts, so a large one need not be held whole in
/// the page as well: `kind` is "retail" (as `import_file`), "spawn" (as
/// `import_spawn_file`) or "mod:<folder>" (as `import_mod_file`). The engine
/// sets aside `length` bytes at once; `import_part` fills them in order and
/// the last part stores the file. A new begin drops an unfinished one.
#[wasm_bindgen]
pub fn import_begin(kind: &str, name: &str, length: f64) -> Result<(), JsValue> {
    relative_path(name)?;
    if !length.is_finite() || length < 0.0 || length > MAX_IMPORT_BYTES || length.fract() != 0.0 {
        return Err(JsValue::from_str("Expected a file length"));
    }
    let path = match kind {
        "retail" => {
            if LIFECYCLE.with(|slot| slot.borrow().started()) {
                return Err(JsValue::from_str("Reload before replacing game resources"));
            }
            format!("/retail/{name}")
        }
        "spawn" => {
            idle("Import launch files")?;
            format!("{SPAWN_DIRECTORY}/{name}")
        }
        _ => {
            let folder = kind
                .strip_prefix("mod:")
                .ok_or_else(|| JsValue::from_str("Expected retail, spawn or mod:<folder>"))?;
            mod_folder(folder)?;
            idle("Import mod files")?;
            mod_path(folder, name)
        }
    };
    PENDING.with(|slot| *slot.borrow_mut() = None);
    let pending = PendingImport {
        path,
        bytes: std::sync::Arc::new_uninit_slice(length as usize),
        filled: 0,
    };
    if pending.bytes.is_empty() {
        finish_import(pending);
    } else {
        PENDING.with(|slot| *slot.borrow_mut() = Some(pending));
    }
    Ok(())
}

/// The next part of the file `import_begin` started. Returns whether the
/// file is now whole and imported.
#[wasm_bindgen]
pub fn import_part(bytes: &[u8]) -> Result<bool, JsValue> {
    let done = PENDING.with(|slot| -> Result<bool, JsValue> {
        let mut slot = slot.borrow_mut();
        let pending = slot
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No import begun"))?;
        let end = pending.filled + bytes.len();
        let Some(target) = std::sync::Arc::get_mut(&mut pending.bytes)
            .and_then(|all| all.get_mut(pending.filled..end))
        else {
            *slot = None;
            return Err(JsValue::from_str("A part runs past the file's length"));
        };
        for (to, from) in target.iter_mut().zip(bytes) {
            to.write(*from);
        }
        pending.filled = end;
        Ok(end == pending.bytes.len())
    })?;
    if done {
        if let Some(pending) = PENDING.with(|slot| slot.borrow_mut().take()) {
            finish_import(pending);
        }
    }
    Ok(done)
}

fn finish_import(pending: PendingImport) {
    debug_assert_eq!(pending.filled, pending.bytes.len());
    // SAFETY: every byte was written, in order, before the import finished.
    let bytes = unsafe { pending.bytes.assume_init() };
    crate::util::file_system::import_shared(pending.path, bytes);
}

/// One launch's map and spawn.ini, which holds its tokens, end with it.
fn remove_spawn_files() {
    let Ok(entries) = crate::util::file_system::read_dir(SPAWN_DIRECTORY) else {
        return;
    };
    for entry in entries.flatten() {
        let _ = crate::util::file_system::remove_file(entry.path());
    }
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
    if !LIFECYCLE.with(|slot| slot.borrow().idle()) {
        return Err(JsValue::from_str(
            "Restore user data before starting or after the engine exited",
        ));
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
        Err(JsValue::from_str(&crate::i18n::tr(
            "VERA:BrowserAudioUnavailable",
        )))
    }
}

pub(crate) fn take_music_output() -> Option<crate::audio::music::MusicPlayer> {
    MUSIC.with(|slot| slot.borrow_mut().take())
}
pub(crate) fn take_sfx_output() -> Option<crate::audio::sfx::SfxPlayer> {
    SFX.with(|slot| slot.borrow_mut().take())
}

/// Resolves once the fonts the engine's first text draws with have been
/// fetched from `pkg/fonts/` (`render::text_engine::web_fonts`); rejects if
/// one cannot be. Await it before `start_game` or `start_spawn`.
#[wasm_bindgen]
pub fn fonts_ready() -> js_sys::Promise {
    crate::render::text_engine::web_fonts::preload()
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
/// followed by engine-exited; once that has returned, another launch may
/// import its files and call this again. A panic requires a new instance.
#[wasm_bindgen]
pub fn start_spawn(
    canvas: web_sys::HtmlCanvasElement,
    on_status: js_sys::Function,
    spawn_ini: String,
) -> Result<(), JsValue> {
    if !LIFECYCLE.with(|slot| slot.borrow().may_start(true)) {
        return Err(JsValue::from_str(&crate::i18n::tr(
            "VERA:GameAlreadyStarted",
        )));
    }
    let request = read_spawn(spawn_ini)?;
    let mut app = crate::app::App::new(
        crate::app::frontend::startup_options::RetailStartupOptions::default(),
    );
    app.set_spawn_request(request);
    run(canvas, on_status, app, true)
}

/// Whether `start_spawn` may run now: before the first launch and after a
/// clean exit, not while a game runs, after `start_game` or after a panic.
#[wasm_bindgen]
pub fn can_start_spawn() -> bool {
    LIFECYCLE.with(|slot| slot.borrow().may_start(true))
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
        // The mod `set_mod` chose, as config.toml names it on the desktop.
        crate::app::mods::launch_dir(&config.paths, None),
    )
    .map_err(error)?;
    serde_json::to_string(&report).map_err(|error| JsValue::from_str(&error.to_string()))
}

fn install_config() {
    crate::util::file_system::import_file("/config.toml", config_text().into_bytes());
}

/// The config.toml the next launch reads: the imported game folder, and the
/// mod `set_mod` chose (its name holds nothing to escape: `mod_folder`).
fn config_text() -> String {
    let mut text = String::from("[paths]\nra2_dir = \"/retail\"\n");
    if let Some(folder) = MOD.with(|slot| slot.borrow().clone()) {
        text.push_str(&format!("mod = \"{folder}\"\n"));
    }
    text
}

fn run(
    canvas: web_sys::HtmlCanvasElement,
    on_status: js_sys::Function,
    app: crate::app::App,
    external: bool,
) -> Result<(), JsValue> {
    use winit::platform::web::EventLoopExtWebSys;
    if !LIFECYCLE.with(|slot| slot.borrow().may_start(external)) {
        return Err(JsValue::from_str(&crate::i18n::tr(
            "VERA:GameAlreadyStarted",
        )));
    }
    if !crate::render::text_engine::web_fonts::preloads_arrived() {
        return Err(JsValue::from_str("Await fonts_ready() before starting"));
    }
    let event_loop =
        winit::event_loop::EventLoop::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    LIFECYCLE
        .with(|slot| slot.borrow_mut().start(external))
        .map_err(|error| JsValue::from_str(&error))?;
    install_config();
    WAKE.with(|slot| *slot.borrow_mut() = Some(event_loop.create_proxy()));
    CANVAS.with(|slot| *slot.borrow_mut() = Some(canvas));
    STATUS.with(|slot| *slot.borrow_mut() = Some(on_status));
    status(&crate::i18n::tr("VERA:StartingEngine"));
    event_loop.spawn_app(app);
    Ok(())
}
