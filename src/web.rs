//! Browser host boundary. App remains the owner of the frontend and match lifecycle.
use std::cell::{Cell, RefCell};
use wasm_bindgen::prelude::*;

thread_local! {
    static CANVAS: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    static STATUS: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static WRITE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static REMOVE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static STARTED: Cell<bool> = const { Cell::new(false) };
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
    if STARTED.with(Cell::get) {
        return Err(JsValue::from_str("Reload before replacing game resources"));
    }
    crate::util::file_system::import_file(format!("/retail/{name}"), bytes);
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
    if STARTED.with(Cell::get) {
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
    use winit::platform::web::EventLoopExtWebSys;
    if STARTED.with(Cell::get) {
        return Err(JsValue::from_str("Game already started; reload to restart"));
    }
    let event_loop =
        winit::event_loop::EventLoop::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    crate::util::file_system::import_file(
        "/config.toml",
        b"[paths]\nra2_dir = \"/retail\"\n".to_vec(),
    );
    CANVAS.with(|slot| *slot.borrow_mut() = Some(canvas));
    STATUS.with(|slot| *slot.borrow_mut() = Some(on_status));
    STARTED.with(|started| started.set(true));
    status("Starting engine");
    event_loop.spawn_app(crate::app::App::new(
        crate::app::frontend::startup_options::RetailStartupOptions::default(),
    ));
    Ok(())
}
