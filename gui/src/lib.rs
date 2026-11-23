//! A retained-mode, game-agnostic GUI (docs/gui-library-api.md).
//!
//! The host owns the window, the GPU and the game; this crate owns the node
//! tree, input routing, the dialog stack and widget state. The host reads the
//! laid-out tree and paints it with its own renderer.

pub mod gui;
pub mod node;
pub mod shell;
pub mod spec;
pub mod time;
mod widget;

pub use shell::geom::RectPx as Rect;
pub use gui::{
    DialogId, DialogKind, DragView, Gui, InputEvent, InputResult, Key, MouseButton, UiEvent, UiMs, append_events,
};
pub use node::{NodeId, NodeKind, Prop, Refused, Rgba, Tree, Value};
pub use widget::combo::ComboDropDown;
pub use widget::list::ListView;
pub use widget::scrollbar::ScrollBarView;
pub use spec::NodeSpec;
