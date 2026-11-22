//! World-anchored UI: status templates evaluated per object and free draw
//! lists at an anchor.
//!
//! The host passes only the objects the local player may see, in draw order,
//! with positions already projected. A template is declared once; nothing
//! its author runs is called per object.

use crate::Rect;
use crate::node::{Refused, Rgba};

use super::Origin;

/// A picture the host registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArtId(pub u32);

/// An index into [`ObjectView::values`]; the host defines what each means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceId(pub u16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Amount {
    pub value: i32,
    pub max: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ObjectView<'a> {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub type_id: u32,
    /// Bit sets the host defines.
    pub category: u32,
    pub relation: u32,
    pub state: u32,
    /// By [`SourceId`]; a missing one hides what reads it.
    pub values: &'a [Option<Amount>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Present(SourceId),
    BelowMax(SourceId),
    AtLeast(SourceId, i32),
}

/// A colour from `percent` full upward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    pub percent: u8,
    pub color: Rgba,
}

/// Rects and offsets are relative to the object's projected point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Element {
    Bar { rect: Rect, source: SourceId, bands: Vec<Band>, background: Option<Rgba> },
    /// `count` pictures `step` apart, `full` up to the amount and `empty` after.
    Segments { at: Rect, step: (i32, i32), count: u16, source: SourceId, full: ArtId, empty: Option<ArtId> },
    Icon { rect: Rect, art: ArtId },
    Number { rect: Rect, source: SourceId, color: Rgba },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusTemplate {
    pub name: String,
    /// Empty matches every type.
    pub types: Vec<u32>,
    /// Zero matches everything; otherwise any common bit matches.
    pub category: u32,
    pub relation: u32,
    pub state: u32,
    /// All must hold.
    pub when: Vec<Condition>,
    pub elements: Vec<Element>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TemplateId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ListId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    Object(u64),
    Cell { x: i32, y: i32 },
    Screen { x: i32, y: i32 },
}

/// Coordinates are relative to the anchor's projected point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect { rect: Rect, color: Rgba },
    Image { rect: Rect, art: ArtId },
    Line { from: (i32, i32), to: (i32, i32), width: u16, color: Rgba },
    Text { rect: Rect, key: String, color: Rgba },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldOp<'a> {
    Rect { rect: Rect, color: Rgba },
    Image { rect: Rect, art: ArtId },
    Line { from: (i32, i32), to: (i32, i32), width: u16, color: Rgba },
    Number { rect: Rect, value: i32, color: Rgba },
    Text { rect: Rect, key: &'a str, color: Rgba },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldCmd<'a> {
    /// The object the host keeps this at the draw depth of.
    pub object: Option<u64>,
    pub op: WorldOp<'a>,
}

#[derive(Default)]
pub struct World {
    revision: u64,
}

impl World {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn add_template(&mut self, origin: Origin, template: StatusTemplate) -> Result<TemplateId, Refused> {
        todo!("add {origin:?} {template:?}")
    }

    pub fn set_template(&mut self, id: TemplateId, template: StatusTemplate) -> Result<(), Refused> {
        todo!("set {id:?} {template:?}")
    }

    /// A hidden template stays registered, so a replacement can be undone.
    pub fn show_template(&mut self, id: TemplateId, shown: bool) -> Result<(), Refused> {
        todo!("show {id:?} {shown}")
    }

    pub fn remove_template(&mut self, id: TemplateId) -> Result<(), Refused> {
        todo!("remove {id:?}")
    }

    pub fn find_template(&self, from: Origin, name: &str) -> Option<TemplateId> {
        todo!("find {from:?} {name}")
    }

    pub fn add_list(&mut self, origin: Origin, anchor: Anchor, shapes: Vec<Shape>) -> Result<ListId, Refused> {
        todo!("add {origin:?} {anchor:?} {shapes:?}")
    }

    pub fn set_list(&mut self, id: ListId, anchor: Anchor, shapes: Vec<Shape>) -> Result<(), Refused> {
        todo!("set {id:?} {anchor:?} {shapes:?}")
    }

    pub fn remove_list(&mut self, id: ListId) -> Result<(), Refused> {
        todo!("remove {id:?}")
    }

    pub fn remove_origin(&mut self, origin: Origin) {
        todo!("remove {origin:?}")
    }

    /// Appends the templates' commands for `objects`: object order first,
    /// then the templates' registration order.
    pub fn paint_status<'a, 'v>(&'a self, objects: &[ObjectView<'v>], out: &mut Vec<WorldCmd<'a>>) {
        todo!("status of {} into {}", objects.len(), out.len())
    }

    /// Appends the draw lists in registration order. `project` gives an
    /// anchor's point, or none while it is not to be seen.
    pub fn paint_lists<'a>(&'a self, project: impl Fn(Anchor) -> Option<(i32, i32)>, out: &mut Vec<WorldCmd<'a>>) {
        let _ = project;
        todo!("lists into {}", out.len())
    }
}
