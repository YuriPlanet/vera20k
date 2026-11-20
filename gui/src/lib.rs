//! VERA20k's retained-mode GUI (docs/gui-library-api.md).
//!
//! The host owns the window, the GPU and the game; this crate owns the node
//! tree, input routing, the dialog stack and widget state. The host reads the
//! laid-out tree and paints it with its own renderer.

pub mod geom;
pub mod gui;
pub mod node;

pub use geom::Rect;
pub use gui::{
    DialogId, DialogKind, Gui, InputEvent, InputResult, Key, MouseButton, UiEvent, UiMs, hover_flash_on,
};
pub use node::{NodeId, NodeKind, Prop, Refused, Rgba, Tree, Value};
