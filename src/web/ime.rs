//! Browser input-method text entry for the chat edits (`app::input::ime`).
//!
//! winit 0.30's web backend sends no `Ime` events and its `set_ime_allowed` is
//! empty, and a browser only runs an input method for an editable element. So
//! while a chat edit takes keys, an invisible textarea over that edit holds the
//! page's focus. Its composition events become `Ime` events; every other key is
//! copied to the canvas, where winit reads it as usual. To the engine the canvas
//! and the textarea are one window: focus moving between them is hidden from
//! winit, which would report a focus loss, and focus leaving the pair is
//! reported through the canvas.
//!
//! It lives here rather than in a host page because launchers bring their own.

use crate::browser_ime::{Composition, input_method_owns_key};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    CompositionEvent, Event, EventTarget, FocusEvent, HtmlCanvasElement, HtmlTextAreaElement,
    KeyboardEvent, KeyboardEventInit,
};
use winit::event::Ime;

thread_local! {
    static EDIT: RefCell<Option<Edit>> = const { RefCell::new(None) };
    static EVENTS: RefCell<VecDeque<Ime>> = const { RefCell::new(VecDeque::new()) };
}

struct Listener {
    target: EventTarget,
    kind: &'static str,
    capture: bool,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback_and_bool(
            self.kind,
            self.callback.as_ref().unchecked_ref(),
            self.capture,
        );
    }
}

struct Edit {
    canvas: HtmlCanvasElement,
    textarea: HtmlTextAreaElement,
    _listeners: Vec<Listener>,
}

impl Drop for Edit {
    fn drop(&mut self) {
        self.textarea.remove();
    }
}

/// Invisible but laid out over the canvas, so the system's candidate window
/// opens at the edit; the game draws the composition itself, as on desktop.
const STYLE: &str = "position:fixed;margin:0;padding:0;border:0;outline:0;resize:none;\
    overflow:hidden;white-space:pre;opacity:0;pointer-events:none;background:transparent;\
    color:transparent;caret-color:transparent;font-size:16px;line-height:1;z-index:2147483647";

fn same(target: Option<EventTarget>, element: &JsValue) -> bool {
    target.is_some_and(|target| JsValue::from(target) == *element)
}

fn push(event: Ime) {
    EVENTS.with(|events| events.borrow_mut().push_back(event));
    super::wake();
}

/// Copy a key the game owns to the canvas, unbubbled: winit reads this one, and
/// the original already reaches winit's window-level device listener.
fn forward_key(canvas: &HtmlCanvasElement, kind: &str, event: &KeyboardEvent) {
    let init = KeyboardEventInit::new();
    init.set_key(&event.key());
    init.set_code(&event.code());
    init.set_location(event.location());
    init.set_repeat(event.repeat());
    init.set_ctrl_key(event.ctrl_key());
    init.set_shift_key(event.shift_key());
    init.set_alt_key(event.alt_key());
    init.set_meta_key(event.meta_key());
    if let Ok(copy) = KeyboardEvent::new_with_keyboard_event_init_dict(kind, &init) {
        let _ = canvas.dispatch_event(&copy);
    }
}

impl Edit {
    fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let textarea: HtmlTextAreaElement = document.create_element("textarea")?.unchecked_into();
        textarea.set_attribute("style", STYLE)?;
        textarea.set_attribute("tabindex", "-1")?;
        textarea.set_attribute("aria-label", "Chat")?;
        textarea.set_attribute("autocomplete", "off")?;
        textarea.set_attribute("autocapitalize", "off")?;
        textarea.set_attribute("autocorrect", "off")?;
        textarea.set_attribute("spellcheck", "false")?;
        textarea.set_hidden(true);
        document.body().ok_or("no body")?.append_child(&textarea)?;

        let mut listeners = Vec::new();
        let mut listen = |target: &EventTarget,
                          kind: &'static str,
                          capture: bool,
                          callback: Box<dyn FnMut(Event)>|
         -> Result<(), JsValue> {
            let callback = Closure::wrap(callback);
            target.add_event_listener_with_callback_and_bool(
                kind,
                callback.as_ref().unchecked_ref(),
                capture,
            )?;
            listeners.push(Listener {
                target: target.clone(),
                kind,
                capture,
                callback,
            });
            Ok(())
        };

        let edit: &EventTarget = textarea.as_ref();
        let target = canvas.clone();
        listen(
            edit,
            "keydown",
            false,
            Box::new(move |event| {
                let event: KeyboardEvent = event.unchecked_into();
                if !input_method_owns_key(event.is_composing(), event.key_code(), &event.key()) {
                    event.prevent_default();
                    forward_key(&target, "keydown", &event);
                }
            }),
        )?;
        // Releases always pass: on desktop only presses stop at a composition.
        let target = canvas.clone();
        listen(
            edit,
            "keyup",
            false,
            Box::new(move |event| {
                let event: KeyboardEvent = event.unchecked_into();
                event.prevent_default();
                forward_key(&target, "keyup", &event);
            }),
        )?;

        let composition = Rc::new(RefCell::new(Composition::default()));
        let state = composition.clone();
        listen(
            edit,
            "compositionstart",
            false,
            Box::new(move |_| state.borrow_mut().start()),
        )?;
        let (state, element) = (composition.clone(), textarea.clone());
        listen(
            edit,
            "input",
            false,
            Box::new(move |_| {
                let caret = element.selection_start().ok().flatten();
                let result = state.borrow_mut().input(&element.value(), caret);
                if let Some((event, empty)) = result {
                    if empty {
                        element.set_value("");
                    }
                    push(event);
                }
            }),
        )?;
        let (state, element) = (composition, textarea.clone());
        listen(
            edit,
            "compositionend",
            false,
            Box::new(move |event| {
                let event: CompositionEvent = event.unchecked_into();
                let event = state.borrow_mut().end(&event.data().unwrap_or_default());
                element.set_value("");
                push(event);
            }),
        )?;

        // Capturing at the window runs before winit's listener on the canvas.
        for kind in ["blur", "focus"] {
            let (canvas_value, edit_value) = (JsValue::from(&canvas), JsValue::from(&textarea));
            listen(
                window.as_ref(),
                kind,
                true,
                Box::new(move |event| {
                    let target = event.target();
                    let event: FocusEvent = event.unchecked_into();
                    if same(target, &canvas_value) && same(event.related_target(), &edit_value) {
                        event.stop_propagation();
                    }
                }),
            )?;
            let (target, canvas_value) = (canvas.clone(), JsValue::from(&canvas));
            listen(
                edit,
                kind,
                false,
                Box::new(move |event| {
                    let event: FocusEvent = event.unchecked_into();
                    if !same(event.related_target(), &canvas_value)
                        && let Ok(copy) = FocusEvent::new(kind)
                    {
                        let _ = target.dispatch_event(&copy);
                    }
                }),
            )?;
        }

        Ok(Self {
            canvas,
            textarea,
            _listeners: listeners,
        })
    }
}

/// The handles, with the slot released: moving focus runs the listeners above,
/// and through them winit, synchronously.
fn edit_elements() -> Option<(HtmlCanvasElement, HtmlTextAreaElement)> {
    EDIT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|edit| (edit.canvas.clone(), edit.textarea.clone()))
    })
}

fn is_active(element: &web_sys::Element) -> bool {
    element
        .owner_document()
        .and_then(|document| document.active_element())
        .is_some_and(|active| active == *element)
}

/// Show the edit while a chat edit takes keys, hand focus back when it stops.
pub(crate) fn set_allowed(allowed: bool) {
    if allowed && edit_elements().is_none() {
        match Edit::new(super::canvas()) {
            Ok(edit) => EDIT.with(|slot| *slot.borrow_mut() = Some(edit)),
            Err(error) => log::error!("Browser input method unavailable: {error:?}"),
        }
    }
    let Some((canvas, textarea)) = edit_elements() else {
        return;
    };
    if allowed {
        textarea.set_hidden(false);
        keep_focus();
    } else {
        if is_active(&textarea) {
            let _ = canvas.focus();
        }
        textarea.set_hidden(true);
        textarea.set_value("");
        // Losing focus ends an open composition; its text has no edit to go to.
        EVENTS.with(|events| events.borrow_mut().clear());
    }
}

/// Place the edit over `x, y, width, height` in window pixels.
pub(crate) fn set_area(x: i32, y: i32, width: u32, height: u32) {
    let Some((canvas, textarea)) = edit_elements() else {
        return;
    };
    let bounds = canvas.get_bounding_client_rect();
    let sx = bounds.width() / f64::from(canvas.width().max(1));
    let sy = bounds.height() / f64::from(canvas.height().max(1));
    let style = textarea.style();
    for (property, value) in [
        ("left", bounds.left() + f64::from(x) * sx),
        ("top", bounds.top() + f64::from(y) * sy),
        ("width", f64::from(width) * sx),
        ("height", f64::from(height) * sy),
    ] {
        let _ = style.set_property(property, &format!("{value}px"));
    }
}

/// A click on the canvas, or a host focusing it, must not end text entry. Focus
/// the player gave to something else on the page stays there.
pub(crate) fn keep_focus() {
    if let Some((canvas, textarea)) = edit_elements()
        && is_active(&canvas)
    {
        let _ = textarea.focus();
    }
}

/// The input-method events since the last call, oldest first.
pub(crate) fn take_events() -> VecDeque<Ime> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

/// Remove the edit and its listeners with the engine that owned them.
pub(crate) fn remove() {
    let edit = EDIT.with(|slot| slot.borrow_mut().take());
    if let Some((canvas, textarea)) = edit.as_ref().map(|edit| (&edit.canvas, &edit.textarea))
        && is_active(textarea)
    {
        let _ = canvas.focus();
    }
    drop(edit);
    EVENTS.with(|events| events.borrow_mut().clear());
}
