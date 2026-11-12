//! The system clipboard, for VERA's random-map share codes.
//!
//! Desktop builds reach it through the `arboard` crate: the Win32 clipboard
//! on Windows, NSPasteboard on macOS and the X11 selection on Linux (Wayland
//! sessions through XWayland). One handle lives as long as the interface
//! thread, because an X11 selection is served by the program that set it and
//! ends with its handle. A clipboard that cannot be opened or holds no text is
//! a `false` or a `None`, which the dialog reports; nothing here can stop the
//! game. Other builds (the browser, Android) have no clipboard.

/// Put `text` on the clipboard; false when the clipboard refused it.
pub(crate) fn set_text(text: &str) -> bool {
    platform::set_text(text)
}

/// The clipboard's text, trimmed; `None` when it holds none.
pub(crate) fn text() -> Option<String> {
    platform::text().map(|text| text.trim().to_string())
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod platform {
    use std::cell::RefCell;

    thread_local! {
        /// Opened on first use and kept, so a selection outlives the call.
        static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
    }

    /// Run `act` on the clipboard, opening it first if need be.
    fn with_clipboard<T>(act: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
        CLIPBOARD.with(|cell| {
            let mut handle = cell.try_borrow_mut().ok()?;
            if handle.is_none() {
                match arboard::Clipboard::new() {
                    Ok(clipboard) => *handle = Some(clipboard),
                    Err(error) => {
                        log::warn!("The clipboard could not be opened: {error}");
                        return None;
                    }
                }
            }
            act(handle.as_mut()?)
        })
    }

    pub(super) fn set_text(text: &str) -> bool {
        with_clipboard(|clipboard| clipboard.set_text(text).ok()).is_some()
    }

    pub(super) fn text() -> Option<String> {
        with_clipboard(|clipboard| clipboard.get_text().ok())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    pub(super) fn set_text(_text: &str) -> bool {
        false
    }

    pub(super) fn text() -> Option<String> {
        None
    }
}
