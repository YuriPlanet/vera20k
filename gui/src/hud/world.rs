//! World-anchored UI: status templates evaluated per object and free draw
//! lists at an anchor.
//!
//! The host passes only the objects the local player may see, in draw order,
//! with positions already projected. A template is declared once; nothing
//! its author runs is called per object.

use crate::Rect;
use crate::node::{Refused, Rgba};

use super::Origin;

// Every count limit is per world, shared by all origins. Going past one is
// `Refused::Limit` and changes nothing.

/// Templates registered at once, hidden ones included.
pub const MAX_TEMPLATES: usize = 256;
pub const MAX_ELEMENTS_PER_TEMPLATE: usize = 32;
pub const MAX_CONDITIONS_PER_TEMPLATE: usize = 16;
/// Counted as supplied, repeats included.
pub const MAX_TYPES_PER_TEMPLATE: usize = 4096;
/// Counted as supplied, repeats included.
pub const MAX_BANDS_PER_BAR: usize = 16;
pub const MAX_SEGMENTS: u16 = 64;
/// The most commands one object gets from [`World::paint_status`]. A template
/// that could pass this alone is refused; where several do together, the
/// first one not to fit whole and those registered after it are left out.
pub const MAX_COMMANDS_PER_OBJECT: usize = 512;
pub const MAX_LISTS: usize = 4096;
pub const MAX_SHAPES_PER_LIST: usize = 128;
/// UTF-8 bytes of a template's name, of one text shape's key or of a node's
/// `Command`.
pub const MAX_TEXT_BYTES: usize = 256;

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

/// A colour from `percent` full upward. Of two with one percent the first
/// supplied counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    pub percent: u8,
    pub color: Rgba,
}

/// Rects and offsets are relative to the object's projected point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Element {
    Bar {
        rect: Rect,
        source: SourceId,
        bands: Vec<Band>,
        background: Option<Rgba>,
    },
    /// `count` pictures `step` apart, `full` up to the amount and `empty` after.
    Segments {
        at: Rect,
        step: (i32, i32),
        count: u16,
        source: SourceId,
        full: ArtId,
        empty: Option<ArtId>,
    },
    Icon {
        rect: Rect,
        art: ArtId,
    },
    Number {
        rect: Rect,
        source: SourceId,
        color: Rgba,
    },
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
    Rect {
        rect: Rect,
        color: Rgba,
    },
    Image {
        rect: Rect,
        art: ArtId,
    },
    Line {
        from: (i32, i32),
        to: (i32, i32),
        width: u16,
        color: Rgba,
    },
    Text {
        rect: Rect,
        key: String,
        color: Rgba,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldOp<'a> {
    Rect {
        rect: Rect,
        color: Rgba,
    },
    Image {
        rect: Rect,
        art: ArtId,
    },
    Line {
        from: (i32, i32),
        to: (i32, i32),
        width: u16,
        color: Rgba,
    },
    Number {
        rect: Rect,
        value: i32,
        color: Rgba,
    },
    Text {
        rect: Rect,
        key: &'a str,
        color: Rgba,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldCmd<'a> {
    /// The object the host keeps this at the draw depth of.
    pub object: Option<u64>,
    pub op: WorldOp<'a>,
}

struct Entry<T> {
    id: u64,
    origin: Origin,
    item: T,
}

/// Rows in registration order, which is also ascending id order.
struct Store<T> {
    rows: Vec<Entry<T>>,
    /// Every id below it was issued.
    next: u64,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Store {
            rows: Vec::new(),
            next: 0,
        }
    }
}

impl<T> Store<T> {
    fn add(&mut self, origin: Origin, item: T, limit: usize) -> Result<u64, Refused> {
        // A quarter of each limit is the host's alone.
        let limit = if origin == Origin::Host {
            limit
        } else {
            limit - limit / 4
        };
        if self.rows.len() >= limit {
            return Err(Refused::Limit);
        }
        let id = self.next;
        self.next = id.checked_add(1).ok_or(Refused::Limit)?;
        self.rows.push(Entry { id, origin, item });
        Ok(id)
    }

    fn index(&self, id: u64) -> Result<usize, Refused> {
        self.rows
            .binary_search_by_key(&id, |row| row.id)
            .map_err(|_| {
                if id < self.next {
                    Refused::Stale
                } else {
                    Refused::NotFound
                }
            })
    }

    fn get_mut(&mut self, id: u64) -> Result<&mut T, Refused> {
        let index = self.index(id)?;
        Ok(&mut self.rows[index].item)
    }

    fn remove(&mut self, id: u64) -> Result<(), Refused> {
        let index = self.index(id)?;
        self.rows.remove(index);
        Ok(())
    }

    /// Whether anything went.
    fn remove_origin(&mut self, origin: Origin) -> bool {
        let before = self.rows.len();
        self.rows.retain(|row| row.origin != origin);
        self.rows.len() != before
    }
}

struct Template {
    /// As [`normalized`] left it.
    def: StatusTemplate,
    shown: bool,
}

impl Template {
    fn matches(&self, object: &ObjectView<'_>) -> bool {
        let def = &self.def;
        // The cheapest tests first: most templates fail one for most objects.
        self.shown
            && mask_matches(def.category, object.category)
            && mask_matches(def.relation, object.relation)
            && mask_matches(def.state, object.state)
            && (def.types.is_empty() || def.types.binary_search(&object.type_id).is_ok())
            && def
                .when
                .iter()
                .all(|condition| holds(*condition, object.values))
    }
}

struct DrawList {
    anchor: Anchor,
    shapes: Vec<Shape>,
}

#[derive(Default)]
pub struct World {
    revision: u64,
    templates: Store<Template>,
    lists: Store<DrawList>,
}

impl World {
    pub fn template_origin(&self, id: TemplateId) -> Result<Origin, Refused> {
        Ok(self.templates.rows[self.templates.index(id.0)?].origin)
    }

    pub fn list_origin(&self, id: ListId) -> Result<Origin, Refused> {
        Ok(self.lists.rows[self.lists.index(id.0)?].origin)
    }

    /// Changes whenever a template or list changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn add_template(
        &mut self,
        origin: Origin,
        template: StatusTemplate,
    ) -> Result<TemplateId, Refused> {
        let item = Template {
            def: normalized(template)?,
            shown: true,
        };
        let id = self.templates.add(origin, item, MAX_TEMPLATES)?;
        self.revision += 1;
        Ok(TemplateId(id))
    }

    /// Keeps the template's origin, place in the order and whether it shows.
    pub fn set_template(
        &mut self,
        id: TemplateId,
        template: StatusTemplate,
    ) -> Result<(), Refused> {
        let current = self.templates.get_mut(id.0)?;
        let def = normalized(template)?;
        if current.def != def {
            current.def = def;
            self.revision += 1;
        }
        Ok(())
    }

    /// A hidden template stays registered, so a replacement can be undone.
    pub fn show_template(&mut self, id: TemplateId, shown: bool) -> Result<(), Refused> {
        let current = self.templates.get_mut(id.0)?;
        if current.shown != shown {
            current.shown = shown;
            self.revision += 1;
        }
        Ok(())
    }

    pub fn remove_template(&mut self, id: TemplateId) -> Result<(), Refused> {
        self.templates.remove(id.0)?;
        self.revision += 1;
        Ok(())
    }

    /// The template named `name`, hidden ones included, by the precedence of
    /// [`super::Hud::find`]; the first registered among one origin's.
    pub fn find_template(&self, from: Origin, name: &str) -> Option<TemplateId> {
        self.templates
            .rows
            .iter()
            .filter(|row| row.item.def.name.eq_ignore_ascii_case(name))
            // The first of equal minima wins.
            .min_by_key(|row| (row.origin != from, row.origin))
            .map(|row| TemplateId(row.id))
    }

    pub fn add_list(
        &mut self,
        origin: Origin,
        anchor: Anchor,
        shapes: Vec<Shape>,
    ) -> Result<ListId, Refused> {
        check_shapes(&shapes)?;
        let id = self
            .lists
            .add(origin, DrawList { anchor, shapes }, MAX_LISTS)?;
        self.revision += 1;
        Ok(ListId(id))
    }

    /// Keeps the list's origin and place in the order.
    pub fn set_list(
        &mut self,
        id: ListId,
        anchor: Anchor,
        shapes: Vec<Shape>,
    ) -> Result<(), Refused> {
        let current = self.lists.get_mut(id.0)?;
        check_shapes(&shapes)?;
        if current.anchor != anchor || current.shapes != shapes {
            *current = DrawList { anchor, shapes };
            self.revision += 1;
        }
        Ok(())
    }

    pub fn remove_list(&mut self, id: ListId) -> Result<(), Refused> {
        self.lists.remove(id.0)?;
        self.revision += 1;
        Ok(())
    }

    pub fn remove_origin(&mut self, origin: Origin) {
        let templates = self.templates.remove_origin(origin);
        let lists = self.lists.remove_origin(origin);
        if templates || lists {
            self.revision += 1;
        }
    }

    /// Appends the templates' commands for `objects`: object order first,
    /// then the templates' registration order, at most
    /// [`MAX_COMMANDS_PER_OBJECT`] for each object. Allocates only to grow
    /// `out`, so a host that keeps it between frames pays nothing.
    pub fn paint_status<'a, 'v>(&'a self, objects: &[ObjectView<'v>], out: &mut Vec<WorldCmd<'a>>) {
        for object in objects {
            let first = out.len();
            for row in &self.templates.rows {
                if !row.item.matches(object) {
                    continue;
                }
                let before = out.len();
                for element in &row.item.def.elements {
                    paint_element(object, element, out);
                }
                // No template alone passes the bound, so neither does the overshoot.
                if out.len() - first > MAX_COMMANDS_PER_OBJECT {
                    out.truncate(before);
                    break;
                }
            }
        }
    }

    /// Appends the draw lists in registration order. `project` gives an
    /// anchor's point, or none while it is not to be seen.
    pub fn paint_lists<'a>(
        &'a self,
        project: impl Fn(Anchor) -> Option<(i32, i32)>,
        out: &mut Vec<WorldCmd<'a>>,
    ) {
        for row in &self.lists.rows {
            let DrawList { anchor, shapes } = &row.item;
            let Some(point) = project(*anchor) else {
                continue;
            };
            let object = match *anchor {
                Anchor::Object(id) => Some(id),
                Anchor::Cell { .. } | Anchor::Screen { .. } => None,
            };
            out.extend(shapes.iter().map(|shape| WorldCmd {
                object,
                op: placed(shape, point),
            }));
        }
    }
}

/// The template within its limits, with types sorted for the lookup and
/// bands from the highest percent down, both without repeats.
fn normalized(mut template: StatusTemplate) -> Result<StatusTemplate, Refused> {
    if template.name.len() > MAX_TEXT_BYTES
        || template.types.len() > MAX_TYPES_PER_TEMPLATE
        || template.when.len() > MAX_CONDITIONS_PER_TEMPLATE
        || template.elements.len() > MAX_ELEMENTS_PER_TEMPLATE
    {
        return Err(Refused::Limit);
    }
    let mut most_commands = 0;
    for element in &template.elements {
        most_commands += match element {
            Element::Bar { bands, .. } if bands.len() > MAX_BANDS_PER_BAR => {
                return Err(Refused::Limit);
            }
            Element::Segments { count, .. } if *count > MAX_SEGMENTS => return Err(Refused::Limit),
            Element::Bar {
                bands, background, ..
            } => usize::from(background.is_some()) + usize::from(!bands.is_empty()),
            Element::Segments { count, .. } => usize::from(*count),
            Element::Icon { .. } | Element::Number { .. } => 1,
        };
    }
    if most_commands > MAX_COMMANDS_PER_OBJECT {
        return Err(Refused::Limit);
    }
    template.types.sort_unstable();
    template.types.dedup();
    for element in &mut template.elements {
        if let Element::Bar { bands, .. } = element {
            // Stable, so the first supplied of a percent is the one kept.
            bands.sort_by_key(|band| std::cmp::Reverse(band.percent));
            bands.dedup_by_key(|band| band.percent);
        }
    }
    Ok(template)
}

fn check_shapes(shapes: &[Shape]) -> Result<(), Refused> {
    let too_long =
        |shape: &Shape| matches!(shape, Shape::Text { key, .. } if key.len() > MAX_TEXT_BYTES);
    if shapes.len() > MAX_SHAPES_PER_LIST || shapes.iter().any(too_long) {
        return Err(Refused::Limit);
    }
    Ok(())
}

fn mask_matches(mask: u32, bits: u32) -> bool {
    mask == 0 || mask & bits != 0
}

fn amount(values: &[Option<Amount>], source: SourceId) -> Option<Amount> {
    values.get(usize::from(source.0)).copied().flatten()
}

/// `(value, max)` with the value within `0..=max`; none without a positive max.
fn ratio(values: &[Option<Amount>], source: SourceId) -> Option<(i64, i64)> {
    let Amount { value, max } = amount(values, source)?;
    (max > 0).then(|| (i64::from(value.clamp(0, max)), i64::from(max)))
}

fn holds(condition: Condition, values: &[Option<Amount>]) -> bool {
    match condition {
        Condition::Present(source) => amount(values, source).is_some(),
        Condition::BelowMax(source) => {
            ratio(values, source).is_some_and(|(value, max)| value < max)
        }
        Condition::AtLeast(source, least) => {
            amount(values, source).is_some_and(|a| a.value >= least)
        }
    }
}

fn saturate(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn moved(rect: Rect, (x, y): (i32, i32)) -> Rect {
    Rect {
        x: rect.x.saturating_add(x),
        y: rect.y.saturating_add(y),
        ..rect
    }
}

fn paint_element(object: &ObjectView<'_>, element: &Element, out: &mut Vec<WorldCmd<'_>>) {
    let point = (object.x, object.y);
    let mut push = |op| {
        out.push(WorldCmd {
            object: Some(object.id),
            op,
        })
    };
    match *element {
        Element::Bar {
            rect,
            source,
            ref bands,
            background,
        } => {
            let Some((value, max)) = ratio(object.values, source) else {
                return;
            };
            let rect = moved(rect, point);
            if let Some(color) = background {
                push(WorldOp::Rect { rect, color });
            }
            if value == 0 || rect.w <= 0 {
                return;
            }
            let percent = value * 100 / max;
            let Some(band) = bands.iter().find(|band| i64::from(band.percent) <= percent) else {
                return;
            };
            let w = saturate((i64::from(rect.w) * value / max).max(1));
            push(WorldOp::Rect {
                rect: Rect { w, ..rect },
                color: band.color,
            });
        }
        Element::Segments {
            at,
            step,
            count,
            source,
            full,
            empty,
        } => {
            let Some((value, max)) = ratio(object.values, source) else {
                return;
            };
            let count = i64::from(count);
            let lit = (count * value + max - 1) / max;
            let shown = if empty.is_some() { count } else { lit };
            // Summed wide, so only the final position saturates.
            let x = i64::from(at.x) + i64::from(point.0);
            let y = i64::from(at.y) + i64::from(point.1);
            for i in 0..shown {
                let Some(art) = (if i < lit { Some(full) } else { empty }) else {
                    break;
                };
                let rect = Rect {
                    x: saturate(x + i * i64::from(step.0)),
                    y: saturate(y + i * i64::from(step.1)),
                    ..at
                };
                push(WorldOp::Image { rect, art });
            }
        }
        Element::Icon { rect, art } => push(WorldOp::Image {
            rect: moved(rect, point),
            art,
        }),
        Element::Number {
            rect,
            source,
            color,
        } => {
            let Some(Amount { value, .. }) = amount(object.values, source) else {
                return;
            };
            push(WorldOp::Number {
                rect: moved(rect, point),
                value,
                color,
            });
        }
    }
}

fn placed(shape: &Shape, point: (i32, i32)) -> WorldOp<'_> {
    let end = |(x, y): (i32, i32)| (x.saturating_add(point.0), y.saturating_add(point.1));
    match *shape {
        Shape::Rect { rect, color } => WorldOp::Rect {
            rect: moved(rect, point),
            color,
        },
        Shape::Image { rect, art } => WorldOp::Image {
            rect: moved(rect, point),
            art,
        },
        Shape::Line {
            from,
            to,
            width,
            color,
        } => WorldOp::Line {
            from: end(from),
            to: end(to),
            width,
            color,
        },
        Shape::Text {
            rect,
            ref key,
            color,
        } => WorldOp::Text {
            rect: moved(rect, point),
            key,
            color,
        },
    }
}

/// Runs of neighbouring commands one draw call can take (`material` names
/// the host's texture or pipeline). Order is never changed.
pub fn batches<'c, 'a: 'c, M: PartialEq + 'c>(
    cmds: &'c [WorldCmd<'a>],
    material: impl Fn(&WorldOp<'a>) -> M + 'c,
) -> impl Iterator<Item = std::ops::Range<usize>> + 'c {
    let mut start = 0;
    std::iter::from_fn(move || {
        let kind = material(&cmds.get(start)?.op);
        let more = cmds[start + 1..]
            .iter()
            .take_while(|cmd| material(&cmd.op) == kind)
            .count();
        let run = start..start + 1 + more;
        start = run.end;
        Some(run)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rgba = Rgba(1, 0, 0, 255);
    const B: Rgba = Rgba(2, 0, 0, 255);
    const C: Rgba = Rgba(3, 0, 0, 255);
    const BACK: Rgba = Rgba(9, 9, 9, 255);
    const FULL: ArtId = ArtId(1);
    const EMPTY: ArtId = ArtId(2);
    const S0: SourceId = SourceId(0);
    const S1: SourceId = SourceId(1);
    const UNIT: Rect = Rect::new(0, 0, 1, 1);

    fn some(value: i32, max: i32) -> [Option<Amount>; 1] {
        [Some(Amount { value, max })]
    }

    fn object(values: &[Option<Amount>]) -> ObjectView<'_> {
        ObjectView {
            id: 7,
            x: 0,
            y: 0,
            type_id: 0,
            category: 0,
            relation: 0,
            state: 0,
            values,
        }
    }

    fn icon(art: u32) -> Element {
        Element::Icon {
            rect: UNIT,
            art: ArtId(art),
        }
    }

    fn bar(w: i32, bands: &[(u8, Rgba)], background: Option<Rgba>) -> Element {
        let bands = bands
            .iter()
            .map(|&(percent, color)| Band { percent, color })
            .collect();
        Element::Bar {
            rect: Rect::new(10, 20, w, 4),
            source: S0,
            bands,
            background,
        }
    }

    fn segments(count: u16, empty: Option<ArtId>) -> Element {
        Element::Segments {
            at: UNIT,
            step: (2, 3),
            count,
            source: S0,
            full: FULL,
            empty,
        }
    }

    fn template(elements: Vec<Element>) -> StatusTemplate {
        StatusTemplate {
            elements,
            ..Default::default()
        }
    }

    fn named(name: &str) -> StatusTemplate {
        StatusTemplate {
            name: name.into(),
            ..Default::default()
        }
    }

    fn world(template: StatusTemplate) -> World {
        let mut world = World::default();
        world.add_template(Origin::Host, template).unwrap();
        world
    }

    fn status<'a>(world: &'a World, objects: &[ObjectView<'_>]) -> Vec<WorldCmd<'a>> {
        let mut out = Vec::new();
        world.paint_status(objects, &mut out);
        out
    }

    fn ops<'a>(world: &'a World, values: &[Option<Amount>]) -> Vec<WorldOp<'a>> {
        status(world, &[object(values)])
            .into_iter()
            .map(|cmd| cmd.op)
            .collect()
    }

    fn arts(cmds: &[WorldCmd<'_>]) -> Vec<(Option<u64>, u32)> {
        cmds.iter()
            .filter_map(|cmd| match cmd.op {
                WorldOp::Image { art, .. } => Some((cmd.object, art.0)),
                _ => None,
            })
            .collect()
    }

    /// The only fill's width and colour.
    fn fill(world: &World, value: i32, max: i32) -> Option<(i32, Rgba)> {
        match ops(world, &some(value, max))[..] {
            [] => None,
            [WorldOp::Rect { rect, color }] => Some((rect.w, color)),
            ref other => panic!("{other:?}"),
        }
    }

    #[test]
    fn removed_ids_are_stale_and_never_reused() {
        let mut world = World::default();
        let template_id = world.add_template(Origin::Host, named("one")).unwrap();
        let list_id = world
            .add_list(Origin::Host, Anchor::Object(1), vec![])
            .unwrap();
        world.remove_template(template_id).unwrap();
        world.remove_list(list_id).unwrap();
        let revision = world.revision();
        assert_eq!(
            world.set_template(template_id, named("two")),
            Err(Refused::Stale)
        );
        assert_eq!(world.show_template(template_id, false), Err(Refused::Stale));
        assert_eq!(world.remove_template(template_id), Err(Refused::Stale));
        assert_eq!(
            world.set_list(list_id, Anchor::Object(1), vec![]),
            Err(Refused::Stale)
        );
        assert_eq!(world.remove_list(list_id), Err(Refused::Stale));
        assert_eq!(world.revision(), revision);
        assert_ne!(
            world.add_template(Origin::Host, named("one")).unwrap(),
            template_id
        );
        assert_ne!(
            world
                .add_list(Origin::Host, Anchor::Object(1), vec![])
                .unwrap(),
            list_id
        );
    }

    #[test]
    fn ids_taken_by_remove_origin_are_stale() {
        let mut world = World::default();
        let template_id = world.add_template(Origin::Player, named("one")).unwrap();
        let list_id = world
            .add_list(Origin::Player, Anchor::Object(1), vec![])
            .unwrap();
        world.remove_origin(Origin::Player);
        assert_eq!(world.remove_template(template_id), Err(Refused::Stale));
        assert_eq!(world.remove_list(list_id), Err(Refused::Stale));
    }

    #[test]
    fn ids_never_issued_are_not_found() {
        let mut world = World::default();
        for id in [0, 5, u64::MAX] {
            let (template_id, list_id) = (TemplateId(id), ListId(id));
            assert_eq!(
                world.set_template(template_id, named("one")),
                Err(Refused::NotFound)
            );
            assert_eq!(
                world.show_template(template_id, true),
                Err(Refused::NotFound)
            );
            assert_eq!(world.remove_template(template_id), Err(Refused::NotFound));
            assert_eq!(
                world.set_list(list_id, Anchor::Object(1), vec![]),
                Err(Refused::NotFound)
            );
            assert_eq!(world.remove_list(list_id), Err(Refused::NotFound));
        }
        assert_eq!(world.revision(), 0);
    }

    #[test]
    fn exhausted_ids_are_refused() {
        let mut world = World::default();
        world.templates.next = u64::MAX;
        world.lists.next = u64::MAX;
        assert_eq!(
            world.add_template(Origin::Host, named("one")),
            Err(Refused::Limit)
        );
        assert_eq!(
            world.add_list(Origin::Host, Anchor::Object(1), vec![]),
            Err(Refused::Limit)
        );
        assert_eq!(world.revision(), 0);
    }

    #[test]
    fn revision_moves_only_on_a_change() {
        let mut world = World::default();
        let original = StatusTemplate {
            types: vec![3, 1, 3],
            elements: vec![bar(9, &[(0, A), (50, B)], None)],
            ..named("one")
        };
        let id = world
            .add_template(Origin::Player, original.clone())
            .unwrap();
        assert_eq!(world.revision(), 1);
        let same = StatusTemplate {
            types: vec![1, 3],
            elements: vec![bar(9, &[(50, B), (0, A)], None)],
            ..named("one")
        };
        world.set_template(id, original).unwrap();
        world.set_template(id, same).unwrap();
        world.show_template(id, true).unwrap();
        assert_eq!(world.revision(), 1);
        world.show_template(id, false).unwrap();
        assert_eq!(world.revision(), 2);
        world.set_template(id, named("two")).unwrap();
        assert_eq!(world.revision(), 3);

        let list = world
            .add_list(Origin::Player, Anchor::Object(1), vec![])
            .unwrap();
        assert_eq!(world.revision(), 4);
        world.set_list(list, Anchor::Object(1), vec![]).unwrap();
        assert_eq!(world.revision(), 4);
        world.set_list(list, Anchor::Object(2), vec![]).unwrap();
        assert_eq!(world.revision(), 5);
        let shapes = vec![Shape::Rect {
            rect: UNIT,
            color: A,
        }];
        world.set_list(list, Anchor::Object(2), shapes).unwrap();
        assert_eq!(world.revision(), 6);

        world.remove_origin(Origin::Layout);
        assert_eq!(world.revision(), 6);
        world.remove_origin(Origin::Player);
        assert_eq!(world.revision(), 7);
        world.remove_origin(Origin::Player);
        assert_eq!(world.revision(), 7);

        let id = world.add_template(Origin::Host, named("one")).unwrap();
        world.remove_template(id).unwrap();
        assert_eq!(world.revision(), 9);
        let list = world
            .add_list(Origin::Host, Anchor::Object(1), vec![])
            .unwrap();
        world.remove_list(list).unwrap();
        assert_eq!(world.revision(), 11);
    }

    #[test]
    fn remove_origin_takes_only_that_origins() {
        let mut world = World::default();
        for origin in [Origin::External(1), Origin::External(2)] {
            world.add_template(origin, named("one")).unwrap();
            world.add_list(origin, Anchor::Object(1), vec![]).unwrap();
        }
        world.remove_origin(Origin::External(1));
        assert_eq!(world.templates.rows.len(), 1);
        assert_eq!(world.templates.rows[0].origin, Origin::External(2));
        assert_eq!(world.lists.rows.len(), 1);
        assert_eq!(world.lists.rows[0].origin, Origin::External(2));
    }

    #[test]
    fn find_follows_the_origin_precedence() {
        let mut world = World::default();
        let order = [
            Origin::External(9),
            Origin::External(2),
            Origin::Player,
            Origin::Layout,
            Origin::Host,
        ];
        let ids = order.map(|origin| world.add_template(origin, named("Gauge")).unwrap());
        world.add_template(Origin::Host, named("gauge")).unwrap();
        for (origin, id) in order.into_iter().zip(ids) {
            assert_eq!(world.find_template(origin, "gAUGE"), Some(id), "{origin:?}");
        }
        // For a caller with none of its own: host, layout, player, then external by number.
        let outsider = Origin::External(5);
        assert_eq!(world.find_template(outsider, "gauge"), Some(ids[4]));
        world.remove_origin(Origin::Host);
        for id in [ids[3], ids[2], ids[1], ids[0]] {
            assert_eq!(world.find_template(outsider, "gauge"), Some(id));
            world.remove_template(id).unwrap();
        }
        assert_eq!(world.find_template(outsider, "gauge"), None);
    }

    #[test]
    fn find_is_ascii_caseless_and_sees_hidden_templates() {
        let mut world = World::default();
        let id = world.add_template(Origin::Host, named("\u{e9}T")).unwrap();
        world.show_template(id, false).unwrap();
        assert_eq!(world.find_template(Origin::Host, "\u{e9}t"), Some(id));
        assert_eq!(world.find_template(Origin::Host, "\u{c9}T"), None);
    }

    #[test]
    fn types_match_whatever_their_order() {
        let types = vec![u32::MAX, 4, 0, 4, 9];
        let world = world(StatusTemplate {
            types,
            ..template(vec![icon(1)])
        });
        for (type_id, hit) in [
            (0, true),
            (4, true),
            (9, true),
            (u32::MAX, true),
            (5, false),
        ] {
            let seen = ObjectView {
                type_id,
                ..object(&[])
            };
            assert_eq!(!status(&world, &[seen]).is_empty(), hit, "{type_id}");
        }
    }

    #[test]
    fn each_mask_needs_a_common_bit() {
        let def = StatusTemplate {
            category: 6,
            relation: 1,
            state: 1 << 31,
            ..template(vec![icon(1)])
        };
        let world = world(def);
        let hit = ObjectView {
            category: 2,
            relation: 3,
            state: u32::MAX,
            ..object(&[])
        };
        assert_eq!(status(&world, &[hit]).len(), 1);
        for miss in [
            ObjectView { category: 9, ..hit },
            ObjectView { relation: 2, ..hit },
            ObjectView { state: 0, ..hit },
        ] {
            assert!(status(&world, &[miss]).is_empty(), "{miss:?}");
        }
    }

    #[test]
    fn empty_filters_match_every_object() {
        let world = world(template(vec![icon(1)]));
        let any = ObjectView {
            type_id: 8,
            category: 1,
            relation: 2,
            state: 4,
            ..object(&[])
        };
        assert_eq!(status(&world, &[any, object(&[])]).len(), 2);
    }

    #[test]
    fn conditions() {
        let cases: [(Condition, &[Option<Amount>], bool); 16] = [
            (Condition::Present(S0), &some(0, 0), true),
            (Condition::Present(S0), &[None], false),
            (Condition::Present(S0), &[], false),
            (Condition::Present(SourceId(u16::MAX)), &some(1, 1), false),
            (Condition::BelowMax(S0), &some(4, 5), true),
            (Condition::BelowMax(S0), &some(i32::MIN, 1), true),
            (Condition::BelowMax(S0), &some(5, 5), false),
            (Condition::BelowMax(S0), &some(6, 5), false),
            (Condition::BelowMax(S0), &some(-2, 0), false),
            (Condition::BelowMax(S0), &some(-2, -1), false),
            (Condition::BelowMax(S0), &[None], false),
            (Condition::AtLeast(S0, 3), &some(3, 0), true),
            (Condition::AtLeast(S0, 3), &some(2, 9), false),
            (Condition::AtLeast(S0, i32::MIN), &some(i32::MIN, 0), true),
            (Condition::AtLeast(S0, i32::MAX), &some(i32::MAX, 0), true),
            (Condition::AtLeast(S0, i32::MIN), &[], false),
        ];
        for (condition, values, hit) in cases {
            let world = world(StatusTemplate {
                when: vec![condition],
                ..template(vec![icon(1)])
            });
            assert_eq!(
                !ops(&world, values).is_empty(),
                hit,
                "{condition:?} {values:?}"
            );
        }
    }

    #[test]
    fn every_condition_must_hold() {
        let when = vec![Condition::Present(S0), Condition::Present(S1)];
        let world = world(StatusTemplate {
            when,
            ..template(vec![icon(1)])
        });
        let one = Some(Amount { value: 1, max: 1 });
        assert!(ops(&world, &[one, None]).is_empty());
        assert!(ops(&world, &[None, one]).is_empty());
        assert_eq!(ops(&world, &[one, one]).len(), 1);
    }

    #[test]
    fn bar_is_background_then_fill_at_the_projected_point() {
        let world = world(template(vec![bar(40, &[(0, A)], Some(BACK))]));
        let values = some(1, 2);
        let seen = ObjectView {
            x: 100,
            y: 200,
            ..object(&values)
        };
        let cmd = |w, color| WorldCmd {
            object: Some(7),
            op: WorldOp::Rect {
                rect: Rect::new(110, 220, w, 4),
                color,
            },
        };
        assert_eq!(status(&world, &[seen]), [cmd(40, BACK), cmd(20, A)]);
    }

    #[test]
    fn bar_width_is_floored_clamped_and_never_lost() {
        let world = world(template(vec![bar(10, &[(0, A)], None)]));
        for (value, max, w) in [
            (1, 1000, 1),
            (1, 3, 3),
            (2, 3, 6),
            (3, 3, 10),
            (4, 3, 10),
            (1, i32::MAX, 1),
            (i32::MAX, i32::MAX, 10),
            (i32::MAX, 1, 10),
        ] {
            assert_eq!(fill(&world, value, max), Some((w, A)), "{value}/{max}");
        }
        for value in [0, -1, i32::MIN] {
            assert_eq!(fill(&world, value, 3), None, "{value}");
        }
    }

    #[test]
    fn band_is_the_greatest_not_above_whatever_the_order() {
        for bands in [
            [(0, A), (25, B), (75, C)],
            [(75, C), (0, A), (25, B)],
            [(25, B), (75, C), (0, A)],
        ] {
            let world = world(template(vec![bar(100, &bands, None)]));
            for (value, color) in [(1, A), (249, A), (250, B), (749, B), (750, C), (5000, C)] {
                assert_eq!(
                    fill(&world, value, 1000).map(|f| f.1),
                    Some(color),
                    "{value}"
                );
            }
        }
    }

    #[test]
    fn bands_of_one_percent_keep_the_first_supplied() {
        let world = world(template(vec![bar(10, &[(50, B), (0, A), (50, C)], None)]));
        assert_eq!(fill(&world, 1, 1), Some((10, B)));
    }

    #[test]
    fn no_band_means_no_fill() {
        let cases: [&[(u8, Rgba)]; 3] = [&[], &[(60, A)], &[(u8::MAX, A)]];
        for bands in cases {
            let world = world(template(vec![bar(10, bands, Some(BACK))]));
            let expected = WorldOp::Rect {
                rect: Rect::new(10, 20, 10, 4),
                color: BACK,
            };
            assert_eq!(ops(&world, &some(59, 100)), [expected]);
        }
    }

    #[test]
    fn a_bar_without_width_has_no_fill() {
        for w in [0, -5, i32::MIN] {
            let world = world(template(vec![bar(w, &[(0, A)], None)]));
            assert_eq!(fill(&world, 1, 1), None, "{w}");
        }
    }

    #[test]
    fn segments_round_up_and_clamp() {
        let world = world(template(vec![segments(4, Some(EMPTY))]));
        for (value, max, lit) in [
            (i32::MIN, 8, 0),
            (0, 8, 0),
            (1, 8, 1),
            (2, 8, 1),
            (3, 8, 2),
            (8, 8, 4),
            (9, 8, 4),
            (1, i32::MAX, 1),
            (i32::MAX - 1, i32::MAX, 4),
            (i32::MAX, 1, 4),
        ] {
            let expected: Vec<_> = (0..4)
                .map(|i| WorldOp::Image {
                    rect: Rect::new(2 * i, 3 * i, 1, 1),
                    art: if i < lit { FULL } else { EMPTY },
                })
                .collect();
            assert_eq!(ops(&world, &some(value, max)), expected, "{value}/{max}");
        }
    }

    #[test]
    fn segments_without_an_empty_picture_stop_at_the_amount() {
        let some_lit = world(template(vec![segments(4, None)]));
        assert_eq!(ops(&some_lit, &some(3, 8)).len(), 2);
        assert!(ops(&some_lit, &some(0, 8)).is_empty());
        let no_count = world(template(vec![segments(0, Some(EMPTY))]));
        assert!(ops(&no_count, &some(8, 8)).is_empty());
    }

    #[test]
    fn unreadable_elements_hide_alone() {
        let number = Element::Number {
            rect: UNIT,
            source: S0,
            color: C,
        };
        let world = world(template(vec![
            bar(10, &[(0, A)], Some(BACK)),
            segments(3, Some(EMPTY)),
            number,
            icon(5),
        ]));
        let icon = WorldOp::Image {
            rect: UNIT,
            art: ArtId(5),
        };
        assert_eq!(ops(&world, &[]), [icon]);
        assert_eq!(ops(&world, &[None]), [icon]);
        for max in [0, -1, i32::MIN] {
            let number = WorldOp::Number {
                rect: UNIT,
                value: -30,
                color: C,
            };
            assert_eq!(ops(&world, &some(-30, max)), [number, icon], "{max}");
        }
    }

    #[test]
    fn numbers_are_raw() {
        let world = world(template(vec![Element::Number {
            rect: UNIT,
            source: S0,
            color: C,
        }]));
        for value in [i32::MIN, -1, 0, 12, i32::MAX] {
            let expected = WorldOp::Number {
                rect: UNIT,
                value,
                color: C,
            };
            assert_eq!(ops(&world, &some(value, 5)), [expected]);
        }
    }

    #[test]
    fn output_is_by_object_then_registration_then_element() {
        let mut world = World::default();
        let first = world
            .add_template(Origin::External(3), template(vec![icon(1), icon(2)]))
            .unwrap();
        let second = world
            .add_template(Origin::Host, template(vec![icon(3)]))
            .unwrap();
        let objects = [
            ObjectView {
                id: 20,
                ..object(&[])
            },
            ObjectView {
                id: 10,
                ..object(&[])
            },
        ];
        let all = [(20, 1), (20, 2), (20, 3), (10, 1), (10, 2), (10, 3)];
        let all = all.map(|(id, art)| (Some(id), art));
        assert_eq!(arts(&status(&world, &objects)), all);

        // Setting keeps the place; hiding is undone without losing it.
        world.set_template(first, template(vec![icon(4)])).unwrap();
        assert_eq!(
            arts(&status(&world, &objects[..1])),
            [(Some(20), 4), (Some(20), 3)]
        );
        world.show_template(first, false).unwrap();
        world.set_template(first, template(vec![icon(6)])).unwrap();
        assert_eq!(arts(&status(&world, &objects[..1])), [(Some(20), 3)]);
        world.show_template(first, true).unwrap();
        assert_eq!(
            arts(&status(&world, &objects[..1])),
            [(Some(20), 6), (Some(20), 3)]
        );
        world.remove_template(second).unwrap();
        assert_eq!(arts(&status(&world, &objects[..1])), [(Some(20), 6)]);
    }

    #[test]
    fn painting_appends() {
        let mut world = world(template(vec![icon(1)]));
        world
            .add_list(
                Origin::Host,
                Anchor::Object(2),
                vec![Shape::Rect {
                    rect: UNIT,
                    color: A,
                }],
            )
            .unwrap();
        let earlier = WorldCmd {
            object: None,
            op: WorldOp::Rect {
                rect: UNIT,
                color: C,
            },
        };
        let mut out = vec![earlier];
        world.paint_status(&[object(&[])], &mut out);
        world.paint_lists(|_| Some((0, 0)), &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], earlier);
    }

    #[test]
    fn lists_keep_their_order_and_follow_the_anchor() {
        let mut world = World::default();
        let cell = Anchor::Cell { x: 3, y: 4 };
        let screen = Anchor::Screen { x: 5, y: 6 };
        let shapes = vec![
            Shape::Rect {
                rect: UNIT,
                color: A,
            },
            Shape::Image {
                rect: UNIT,
                art: FULL,
            },
            Shape::Line {
                from: (-1, -2),
                to: (1, 2),
                width: 3,
                color: B,
            },
            Shape::Text {
                rect: UNIT,
                key: "label".into(),
                color: C,
            },
        ];
        world.add_list(Origin::External(1), screen, shapes).unwrap();
        let unseen = vec![Shape::Rect {
            rect: UNIT,
            color: BACK,
        }];
        world
            .add_list(Origin::Host, Anchor::Object(404), unseen)
            .unwrap();
        let dot = || {
            vec![Shape::Image {
                rect: UNIT,
                art: EMPTY,
            }]
        };
        world
            .add_list(Origin::Host, Anchor::Object(8), dot())
            .unwrap();
        world.add_list(Origin::Player, cell, dot()).unwrap();

        let mut out = Vec::new();
        let project = |anchor| match anchor {
            Anchor::Object(8) => Some((10, 10)),
            Anchor::Object(_) => None,
            Anchor::Cell { x, y } => Some((x * 10, y * 10)),
            // Not the anchor's own numbers: screen anchors are projected too.
            Anchor::Screen { x, y } => Some((x + 100, y + 100)),
        };
        world.paint_lists(project, &mut out);
        let at = Rect::new(105, 106, 1, 1);
        let cmd = |object, op| WorldCmd { object, op };
        let expected = [
            cmd(None, WorldOp::Rect { rect: at, color: A }),
            cmd(
                None,
                WorldOp::Image {
                    rect: at,
                    art: FULL,
                },
            ),
            cmd(
                None,
                WorldOp::Line {
                    from: (104, 104),
                    to: (106, 108),
                    width: 3,
                    color: B,
                },
            ),
            cmd(
                None,
                WorldOp::Text {
                    rect: at,
                    key: "label",
                    color: C,
                },
            ),
            cmd(
                Some(8),
                WorldOp::Image {
                    rect: Rect::new(10, 10, 1, 1),
                    art: EMPTY,
                },
            ),
            cmd(
                None,
                WorldOp::Image {
                    rect: Rect::new(30, 40, 1, 1),
                    art: EMPTY,
                },
            ),
        ];
        assert_eq!(out, expected);
    }

    #[test]
    fn a_changed_list_keeps_its_place() {
        let mut world = World::default();
        let image = |art| {
            vec![Shape::Image {
                rect: UNIT,
                art: ArtId(art),
            }]
        };
        let first = world
            .add_list(Origin::Host, Anchor::Object(1), image(1))
            .unwrap();
        world
            .add_list(Origin::Host, Anchor::Object(2), image(2))
            .unwrap();
        world.set_list(first, Anchor::Object(3), image(3)).unwrap();
        let mut out = Vec::new();
        world.paint_lists(|_| Some((0, 0)), &mut out);
        assert_eq!(arts(&out), [(Some(3), 3), (Some(2), 2)]);
    }

    #[test]
    fn template_limits() {
        let cases: [(fn(usize) -> StatusTemplate, usize); 6] = [
            (|n| named(&"n".repeat(n)), MAX_TEXT_BYTES),
            (
                |n| StatusTemplate {
                    types: vec![1; n],
                    ..named("t")
                },
                MAX_TYPES_PER_TEMPLATE,
            ),
            (
                |n| StatusTemplate {
                    when: vec![Condition::Present(S0); n],
                    ..named("t")
                },
                MAX_CONDITIONS_PER_TEMPLATE,
            ),
            (|n| template(vec![icon(1); n]), MAX_ELEMENTS_PER_TEMPLATE),
            (
                |n| template(vec![bar(1, &vec![(0, A); n], None)]),
                MAX_BANDS_PER_BAR,
            ),
            (
                |n| template(vec![segments(n as u16, None)]),
                usize::from(MAX_SEGMENTS),
            ),
        ];
        for (make, limit) in cases {
            let mut world = World::default();
            let id = world.add_template(Origin::Host, make(limit)).unwrap();
            let kept = world.templates.rows[0].item.def.clone();
            assert_eq!(
                world.set_template(id, make(limit + 1)),
                Err(Refused::Limit),
                "{limit}"
            );
            assert_eq!(
                world.add_template(Origin::Host, make(limit + 1)),
                Err(Refused::Limit)
            );
            assert_eq!(world.revision(), 1);
            assert_eq!(world.templates.rows.len(), 1);
            assert_eq!(world.templates.rows[0].item.def, kept);
        }
    }

    #[test]
    fn name_limit_counts_bytes() {
        let name = "\u{e9}".repeat(MAX_TEXT_BYTES / 2 + 1);
        assert_eq!(
            World::default().add_template(Origin::Host, named(&name)),
            Err(Refused::Limit)
        );
    }

    #[test]
    fn a_template_may_not_pass_the_output_bound_alone() {
        let mut elements =
            vec![segments(MAX_SEGMENTS, None); MAX_COMMANDS_PER_OBJECT / usize::from(MAX_SEGMENTS)];
        let mut world = world(template(elements.clone()));
        assert_eq!(ops(&world, &some(1, 1)).len(), MAX_COMMANDS_PER_OBJECT);
        elements.push(icon(1));
        assert_eq!(
            world.add_template(Origin::Host, template(elements)),
            Err(Refused::Limit)
        );
        assert_eq!(world.revision(), 1);
    }

    #[test]
    fn templates_past_the_output_bound_are_left_out_whole() {
        let per_element = usize::from(MAX_SEGMENTS);
        let nearly = MAX_COMMANDS_PER_OBJECT / per_element - 1;
        let mut world = world(template(vec![segments(MAX_SEGMENTS, Some(EMPTY)); nearly]));
        let wide = vec![segments(MAX_SEGMENTS, Some(EMPTY)); 2];
        let second = world.add_template(Origin::Host, template(wide)).unwrap();
        world
            .add_template(Origin::Host, template(vec![icon(9)]))
            .unwrap();
        let values = some(1, 1);
        let objects = [object(&values), object(&values)];
        assert_eq!(status(&world, &objects).len(), 2 * nearly * per_element);

        // Exactly at the bound fits; the next template does not.
        world
            .set_template(second, template(vec![segments(MAX_SEGMENTS, Some(EMPTY))]))
            .unwrap();
        assert_eq!(status(&world, &objects).len(), 2 * MAX_COMMANDS_PER_OBJECT);
    }

    #[test]
    fn template_count_is_limited() {
        let mut world = World::default();
        let first = world.add_template(Origin::Host, named("t")).unwrap();
        world.show_template(first, false).unwrap();
        for _ in 1..MAX_TEMPLATES {
            world.add_template(Origin::Host, named("t")).unwrap();
        }
        let revision = world.revision();
        assert_eq!(
            world.add_template(Origin::Player, named("t")),
            Err(Refused::Limit)
        );
        assert_eq!(world.revision(), revision);
        world.remove_template(first).unwrap();
        world.add_template(Origin::Host, named("t")).unwrap();
    }

    #[test]
    fn list_count_is_limited() {
        let mut world = World::default();
        let first = world
            .add_list(Origin::Host, Anchor::Object(0), vec![])
            .unwrap();
        for _ in 1..MAX_LISTS {
            world
                .add_list(Origin::Host, Anchor::Object(0), vec![])
                .unwrap();
        }
        let revision = world.revision();
        assert_eq!(
            world.add_list(Origin::Player, Anchor::Object(0), vec![]),
            Err(Refused::Limit)
        );
        assert_eq!(world.revision(), revision);
        world.remove_list(first).unwrap();
        world
            .add_list(Origin::Host, Anchor::Object(0), vec![])
            .unwrap();
    }

    #[test]
    fn shape_limits() {
        let cases: [(fn(usize) -> Vec<Shape>, usize); 2] = [
            (
                |n| {
                    vec![
                        Shape::Rect {
                            rect: UNIT,
                            color: A
                        };
                        n
                    ]
                },
                MAX_SHAPES_PER_LIST,
            ),
            (
                |n| {
                    vec![Shape::Text {
                        rect: UNIT,
                        key: "k".repeat(n),
                        color: A,
                    }]
                },
                MAX_TEXT_BYTES,
            ),
        ];
        for (make, limit) in cases {
            let anchor = Anchor::Object(1);
            let mut world = World::default();
            let id = world.add_list(Origin::Host, anchor, make(limit)).unwrap();
            assert_eq!(
                world.set_list(id, Anchor::Object(2), make(limit + 1)),
                Err(Refused::Limit)
            );
            assert_eq!(
                world.add_list(Origin::Host, anchor, make(limit + 1)),
                Err(Refused::Limit)
            );
            assert_eq!(world.revision(), 1);
            assert_eq!(world.lists.rows.len(), 1);
            assert_eq!(world.lists.rows[0].item.anchor, anchor);
            assert_eq!(world.lists.rows[0].item.shapes, make(limit));
        }
    }

    #[test]
    fn extreme_values_do_not_panic() {
        let edges = [i32::MIN, -1, 0, 1, i32::MAX];
        for edge in edges {
            let rect = Rect::new(edge, edge, edge, edge);
            let bands = vec![Band {
                percent: 0,
                color: A,
            }];
            let world = world(template(vec![
                Element::Bar {
                    rect,
                    source: S0,
                    bands,
                    background: Some(BACK),
                },
                Element::Segments {
                    at: rect,
                    step: (edge, edge),
                    count: MAX_SEGMENTS,
                    source: S0,
                    full: FULL,
                    empty: Some(EMPTY),
                },
                Element::Icon { rect, art: FULL },
                Element::Number {
                    rect,
                    source: S0,
                    color: C,
                },
            ]));
            for value in edges {
                for max in edges {
                    for at in edges {
                        let values = some(value, max);
                        let seen = ObjectView {
                            id: u64::MAX,
                            x: at,
                            y: at,
                            ..object(&values)
                        };
                        for cmd in status(&world, &[seen]) {
                            if let WorldOp::Rect { rect, color: A } = cmd.op {
                                assert!((1..=edge).contains(&rect.w), "{value}/{max} in {edge}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_widest_bar_fills_exactly() {
        let world = world(template(vec![bar(i32::MAX, &[(100, A)], None)]));
        assert_eq!(fill(&world, i32::MAX, i32::MAX), Some((i32::MAX, A)));
        assert_eq!(fill(&world, i32::MAX - 1, i32::MAX), None);
    }

    #[test]
    fn positions_saturate() {
        let edge = Rect::new(i32::MAX, i32::MIN, 1, 1);
        let world = world(template(vec![Element::Icon {
            rect: edge,
            art: FULL,
        }]));
        let seen = ObjectView {
            x: i32::MAX,
            y: i32::MIN,
            ..object(&[])
        };
        assert_eq!(
            status(&world, &[seen])[0].op,
            WorldOp::Image {
                rect: edge,
                art: FULL
            }
        );
    }

    #[test]
    fn segment_positions_saturate_only_at_the_end() {
        let world = world(template(vec![Element::Segments {
            at: Rect::new(i32::MAX, 0, 1, 1),
            step: (i32::MAX, i32::MIN),
            count: 3,
            source: S0,
            full: FULL,
            empty: None,
        }]));
        let values = some(1, 1);
        let seen = ObjectView {
            x: i32::MIN,
            ..object(&values)
        };
        let expected = [(-1, 0), (i32::MAX - 1, i32::MIN), (i32::MAX, i32::MIN)].map(|(x, y)| {
            WorldOp::Image {
                rect: Rect::new(x, y, 1, 1),
                art: FULL,
            }
        });
        let found: Vec<_> = status(&world, &[seen])
            .into_iter()
            .map(|cmd| cmd.op)
            .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn list_points_saturate() {
        let mut world = World::default();
        let (low, high) = ((i32::MIN, i32::MIN), (i32::MAX, i32::MAX));
        let line = Shape::Line {
            from: low,
            to: high,
            width: u16::MAX,
            color: A,
        };
        world
            .add_list(Origin::Host, Anchor::Object(0), vec![line])
            .unwrap();
        for (point, from, to) in [(low, low, (-1, -1)), (high, (-1, -1), high)] {
            let mut out = Vec::new();
            world.paint_lists(|_| Some(point), &mut out);
            assert_eq!(
                out[0].op,
                WorldOp::Line {
                    from,
                    to,
                    width: u16::MAX,
                    color: A
                }
            );
        }
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn five_thousand_objects_stay_under_a_millisecond() {
        let mut world = World::default();
        for group in 0..6 {
            let def = StatusTemplate {
                types: (0..64).map(|i| i * 6 + group).collect(),
                when: vec![Condition::Present(S0)],
                elements: vec![
                    bar(40, &[(0, A), (30, B), (70, C)], Some(BACK)),
                    icon(group),
                ],
                ..Default::default()
            };
            world.add_template(Origin::Host, def).unwrap();
        }
        let values = some(55, 100);
        let objects: Vec<_> = (0..5000)
            .map(|i| ObjectView {
                id: u64::from(i),
                type_id: i % 384,
                ..object(&values)
            })
            .collect();
        let mut out = Vec::new();
        let mut best = std::time::Duration::MAX;
        for _ in 0..50 {
            out.clear();
            let start = std::time::Instant::now();
            world.paint_status(std::hint::black_box(&objects), &mut out);
            best = best.min(start.elapsed());
        }
        assert_eq!(out.len(), 15_000);
        assert!(best < std::time::Duration::from_millis(1), "{best:?}");
    }
}
