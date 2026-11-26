use std::{
    collections::VecDeque,
    ffi::c_void,
    ptr::NonNull,
    sync::{atomic::Ordering, Arc, Mutex},
};

use crate::{
    cursor::Cursor,
    dpi::{PhysicalPosition, PhysicalSize, Position, Size},
    error::{ExternalError, NotSupportedError},
    icon::Icon,
    window::{self, ImePurpose},
};

use super::{
    ActiveEventLoop, Fullscreen, MonitorHandle, PlatformSpecificWindowAttributes, Shared, TouchPan,
    WindowId,
};

const DISPLAY_WIDTH: u32 = 1280;
const DISPLAY_HEIGHT: u32 = 720;

unsafe extern "C" {
    fn platform_window_init() -> *mut c_void;
}

struct WindowState {
    title: String,
    visible: bool,
    fullscreen: Option<Fullscreen>,
}

pub struct Window {
    n_window: NonNull<c_void>,
    shared: Arc<Shared>,
    state: Mutex<WindowState>,
}

// SAFETY: `platform_window_init` returns libnx's process-lifetime default NWindow. This backend
// only stores and returns its opaque identity; all mutable Rust state is protected by `Mutex`.
unsafe impl Send for Window {}

// SAFETY: Same lifetime and access guarantees as the `Send` implementation above. No method
// dereferences the native pointer or calls window APIs from a worker thread.
unsafe impl Sync for Window {}

impl Window {
    pub(crate) fn new(
        event_loop: &ActiveEventLoop,
        attributes: window::WindowAttributes,
    ) -> Result<Self, crate::error::OsError> {
        if event_loop
            .shared
            .window_created
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(os_error!(std::io::Error::other(
                "Horizon supports one window"
            )));
        }

        // The client initializes libnx before entering winit. The returned default NWindow
        // remains owned by libnx until process shutdown.
        let Some(n_window) = NonNull::new(unsafe { platform_window_init() }) else {
            event_loop
                .shared
                .window_created
                .store(false, Ordering::Release);
            return Err(os_error!(std::io::Error::other(
                "platform_window_init returned a null NWindow"
            )));
        };

        event_loop.shared.redraw.store(true, Ordering::Release);
        let _platform_attributes: PlatformSpecificWindowAttributes = attributes.platform_specific;
        Ok(Self {
            n_window,
            shared: Arc::clone(&event_loop.shared),
            state: Mutex::new(WindowState {
                title: attributes.title,
                visible: attributes.visible,
                fullscreen: attributes.fullscreen.map(Into::into),
            }),
        })
    }

    pub(crate) fn maybe_queue_on_main(&self, f: impl FnOnce(&Self) + Send + 'static) {
        f(self);
    }

    pub(crate) fn maybe_wait_on_main<R: Send>(&self, f: impl FnOnce(&Self) -> R + Send) -> R {
        f(self)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, WindowState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn id(&self) -> WindowId {
        WindowId(1)
    }

    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
        VecDeque::from([MonitorHandle])
    }

    pub fn current_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    pub fn scale_factor(&self) -> f64 {
        1.0
    }

    pub fn request_redraw(&self) {
        self.shared.redraw.store(true, Ordering::Release);
    }

    pub fn pre_present_notify(&self) {}

    pub fn reset_dead_keys(&self) {}

    pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
        Ok(PhysicalPosition::new(0, 0))
    }

    pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, NotSupportedError> {
        self.inner_position()
    }

    pub fn set_outer_position(&self, _position: Position) {}

    pub fn inner_size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(DISPLAY_WIDTH, DISPLAY_HEIGHT)
    }

    pub fn request_inner_size(&self, _size: Size) -> Option<PhysicalSize<u32>> {
        Some(self.inner_size())
    }

    pub fn outer_size(&self) -> PhysicalSize<u32> {
        self.inner_size()
    }

    pub fn set_min_inner_size(&self, _size: Option<Size>) {}

    pub fn set_max_inner_size(&self, _size: Option<Size>) {}

    pub fn resize_increments(&self) -> Option<PhysicalSize<u32>> {
        None
    }

    pub fn set_resize_increments(&self, _increments: Option<Size>) {}

    pub fn title(&self) -> String {
        self.state().title.clone()
    }

    pub fn set_title(&self, title: &str) {
        self.state().title = title.to_owned();
    }

    pub fn set_transparent(&self, _transparent: bool) {}

    pub fn set_blur(&self, _blur: bool) {}

    pub fn set_visible(&self, visible: bool) {
        self.state().visible = visible;
    }

    pub fn is_visible(&self) -> Option<bool> {
        Some(self.state().visible)
    }

    pub fn set_resizable(&self, _resizable: bool) {}

    pub fn is_resizable(&self) -> bool {
        false
    }

    pub fn set_enabled_buttons(&self, _buttons: window::WindowButtons) {}

    pub fn enabled_buttons(&self) -> window::WindowButtons {
        window::WindowButtons::all()
    }

    pub fn set_minimized(&self, _minimized: bool) {}

    pub fn is_minimized(&self) -> Option<bool> {
        Some(false)
    }

    pub fn set_maximized(&self, _maximized: bool) {}

    pub fn is_maximized(&self) -> bool {
        false
    }

    pub(crate) fn set_fullscreen(&self, fullscreen: Option<Fullscreen>) {
        self.state().fullscreen = fullscreen;
    }

    pub(crate) fn fullscreen(&self) -> Option<Fullscreen> {
        self.state().fullscreen.clone()
    }

    /// Two-finger midpoint movement in window pixels since the previous poll.
    /// `active` is true for the whole two-finger gesture, including the frame
    /// that started it, so the app can suppress the synthetic left click.
    pub fn touch_pan(&self) -> TouchPan {
        *self.shared.touch_pan.lock().unwrap()
    }

    /// Horizon-only right-stick camera sample shared with the native input poll.
    pub fn camera_pan(&self) -> super::event_loop::CameraPan {
        *self.shared.camera_pan.lock().unwrap()
    }

    pub fn set_decorations(&self, _decorations: bool) {}

    pub fn is_decorated(&self) -> bool {
        false
    }

    pub fn set_window_level(&self, _level: window::WindowLevel) {}

    pub fn set_window_icon(&self, _window_icon: Option<Icon>) {}

    pub fn set_ime_cursor_area(&self, _position: Position, _size: Size) {}

    /// Text the software keyboard should start from. Not part of the winit
    /// IME surface, so it is a Horizon-backend extension the app sets when an
    /// edit takes focus.
    pub fn set_ime_initial_text(&self, text: &str) {
        if let Ok(mut slot) = self.shared.ime_initial_text.lock() {
            slot.clear();
            slot.push_str(text);
        }
    }

    pub fn set_ime_allowed(&self, allowed: bool) {
        // Polling owns the blocking swkbd call so opening it cannot stall a
        // state transition. While this is set, a fresh Plus edge opens it.
        // Tapping an edit control should also open it, so clearing to false
        // and then back to true (one focus acquisition) queues one open.
        self.shared.ime_allowed.store(allowed, Ordering::Release);
        if allowed {
            self.shared
                .ime_open_requested
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }

    pub fn set_ime_purpose(&self, _purpose: ImePurpose) {}

    pub fn focus_window(&self) {}

    pub fn has_focus(&self) -> bool {
        self.shared.focused.load(Ordering::Relaxed)
    }

    pub fn request_user_attention(&self, _request_type: Option<window::UserAttentionType>) {}

    pub fn set_theme(&self, _theme: Option<window::Theme>) {}

    pub fn theme(&self) -> Option<window::Theme> {
        None
    }

    pub fn set_content_protected(&self, _protected: bool) {}

    pub fn set_cursor(&self, _cursor: Cursor) {}

    pub fn set_cursor_position(&self, position: Position) -> Result<(), ExternalError> {
        let position = position.to_physical::<f64>(self.scale_factor());
        *self
            .shared
            .cursor_position
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = position;
        Ok(())
    }

    pub fn set_cursor_grab(&self, _mode: window::CursorGrabMode) -> Result<(), ExternalError> {
        Err(ExternalError::NotSupported(NotSupportedError::new()))
    }

    pub fn set_cursor_visible(&self, _visible: bool) {}

    pub fn drag_window(&self) -> Result<(), ExternalError> {
        Err(ExternalError::NotSupported(NotSupportedError::new()))
    }

    pub fn drag_resize_window(
        &self,
        _direction: window::ResizeDirection,
    ) -> Result<(), ExternalError> {
        Err(ExternalError::NotSupported(NotSupportedError::new()))
    }

    pub fn show_window_menu(&self, _position: Position) {}

    pub fn set_cursor_hittest(&self, _hittest: bool) -> Result<(), ExternalError> {
        Err(ExternalError::NotSupported(NotSupportedError::new()))
    }

    #[cfg(feature = "rwh_06")]
    pub fn raw_window_handle_rwh_06(
        &self,
    ) -> Result<crate::raw_window_handle::RawWindowHandle, crate::raw_window_handle::HandleError>
    {
        use crate::raw_window_handle::{HorizonWindowHandle, RawWindowHandle};

        Ok(RawWindowHandle::Horizon(HorizonWindowHandle::new(
            self.n_window,
        )))
    }

    #[cfg(feature = "rwh_06")]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<crate::raw_window_handle::RawDisplayHandle, crate::raw_window_handle::HandleError>
    {
        use crate::raw_window_handle::{HorizonDisplayHandle, RawDisplayHandle};

        Ok(RawDisplayHandle::Horizon(HorizonDisplayHandle::new()))
    }
}
