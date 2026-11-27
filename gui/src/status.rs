//! World-anchored UI: status templates evaluated per object.
//!
//! The host passes only the objects the local player may see, in draw order,
//! with positions already projected. A template is declared once; nothing
//! its author runs is called per object.

use crate::Rect;
use crate::node::{Refused, Rgba};

use crate::origin::{Origin, others_share};

// Every count limit is per world, shared by all origins. Going past one is
// `Refused::Limit` and changes nothing.

/// Templates registered at once.
pub const MAX_TEMPLATES: usize = 256;
pub const MAX_ELEMENTS_PER_TEMPLATE: usize = 32;
pub const MAX_CONDITIONS_PER_TEMPLATE: usize = 16;
/// Counted as supplied, repeats included.
pub const MAX_TYPES_PER_TEMPLATE: usize = 4096;
/// Counted as supplied, repeats included.
pub const MAX_BANDS_PER_BAR: usize = 16;
pub const MAX_SEGMENTS: u16 = 64;
/// The most pictures a [`Element::Pips`] row shows: one object's whole
/// command share, as a host's tallest row may need it.
pub const MAX_PIPS: usize = MAX_COMMANDS_PER_OBJECT;
/// The most pictures an [`Element::Slots`] table names.
pub const MAX_SLOT_ARTS: usize = 256;
/// The most commands one object gets from [`World::paint_status`], of which
/// other origins than the host get three quarters (384) between them. A
/// template that could pass its share alone is refused. Where several do
/// together, the first not to fit whole is left out with those registered
/// after it; one of another origin's takes none of the host's with it.
pub const MAX_COMMANDS_PER_OBJECT: usize = 512;
/// UTF-8 bytes of a template's name.
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

/// One place of an object's row of slots and what shows there: the host
/// defines the codes, a template's [`Element::Slots`] the pictures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    pub index: u16,
    pub code: u16,
    /// Pixels this slot sits below its place in the row (negative above):
    /// a host's row whose arms do not share one line.
    pub lift: i16,
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
    /// The object's row of slots, in drawing order ([`Element::Slots`]).
    pub slots: &'a [Slot],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Present(SourceId),
    BelowMax(SourceId),
    AtLeast(SourceId, i32),
    /// The amount is present and its value at most this.
    AtMost(SourceId, i32),
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
    /// A row as long as the amount's `max`: its first `value` pictures
    /// `full`, the rest `empty` (none without it), `step` apart from `at`.
    /// At most [`MAX_PIPS`] are shown.
    Pips {
        at: Rect,
        step: (i32, i32),
        source: SourceId,
        full: ArtId,
        empty: Option<ArtId>,
    },
    /// The object's [`ObjectView::slots`]: each at `at` moved `index` times
    /// `step`, showing `arts[code]`; a code past the table shows nothing.
    /// At most [`MAX_PIPS`] are shown.
    Slots {
        at: Rect,
        step: (i32, i32),
        arts: Vec<ArtId>,
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

/// No rect has a negative size, and `x + w` and `y + h` fit an i32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldOp {
    Rect {
        rect: Rect,
        color: Rgba,
    },
    Image {
        rect: Rect,
        art: ArtId,
    },
    Number {
        rect: Rect,
        value: i32,
        color: Rgba,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldCmd {
    /// The object the host keeps this at the draw depth of.
    pub object: Option<u64>,
    pub op: WorldOp,
}

/// A registered template, as [`normalized`] left it.
struct Entry {
    origin: Origin,
    def: StatusTemplate,
}

impl Entry {
    fn matches(&self, object: &ObjectView<'_>) -> bool {
        let def = &self.def;
        // The cheapest tests first: most templates fail one for most objects.
        mask_matches(def.category, object.category)
            && mask_matches(def.relation, object.relation)
            && mask_matches(def.state, object.state)
            && (def.types.is_empty() || def.types.binary_search(&object.type_id).is_ok())
            && def
                .when
                .iter()
                .all(|condition| holds(*condition, object.values))
    }
}

#[derive(Default)]
pub struct World {
    /// In registration order. A template's id is its index when it was
    /// added; a removal moves the later ones.
    templates: Vec<Entry>,
}

impl World {
    pub fn add_template(
        &mut self,
        origin: Origin,
        template: StatusTemplate,
    ) -> Result<TemplateId, Refused> {
        let def = normalized(template, origin)?;
        if self.templates.len() >= origin.share(MAX_TEMPLATES) {
            return Err(Refused::Limit);
        }
        let id = TemplateId(self.templates.len() as u64);
        self.templates.push(Entry { origin, def });
        Ok(id)
    }

    /// Replaces a named template at the end of registration order. A rejected
    /// replacement preserves both the old definition and its priority.
    pub fn replace_template(&mut self, origin: Origin, template: StatusTemplate) -> Result<(), Refused> {
        let def = normalized(template, origin)?;
        let previous = self.templates.iter().position(|row| row.origin == origin && row.def.name == def.name);
        if previous.is_none() && self.templates.len() >= origin.share(MAX_TEMPLATES) {
            return Err(Refused::Limit);
        }
        if let Some(index) = previous {
            self.templates.remove(index);
        }
        self.templates.push(Entry { origin, def });
        Ok(())
    }

    /// Removes `origin`'s templates named `name`; whether there was one.
    /// The others keep their order.
    pub fn remove(&mut self, origin: Origin, name: &str) -> bool {
        let before = self.templates.len();
        self.templates
            .retain(|row| !(row.origin == origin && row.def.name == name));
        self.templates.len() != before
    }

    /// Removes every template of `origin` (its module stopped).
    pub fn remove_origin(&mut self, origin: Origin) {
        self.templates.retain(|row| row.origin != origin);
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Appends the templates' commands for `objects`: object order first,
    /// then the templates' registration order, at most
    /// [`MAX_COMMANDS_PER_OBJECT`] for each object. Allocates only to grow
    /// `out`, so a host that keeps it between frames pays nothing.
    pub fn paint_status(&self, objects: &[ObjectView<'_>], out: &mut Vec<WorldCmd>) {
        for object in objects {
            let first = out.len();
            // What is left for other origins than the host.
            let mut spare = others_share(MAX_COMMANDS_PER_OBJECT);
            for row in &self.templates {
                let host = row.origin == Origin::Host;
                if (!host && spare == 0) || !row.matches(object) {
                    continue;
                }
                let before = out.len();
                for element in &row.def.elements {
                    paint_element(object, element, out);
                }
                let used = out.len() - before;
                // No template alone passes the bound, so neither does the overshoot.
                let fits = out.len() - first <= MAX_COMMANDS_PER_OBJECT && (host || used <= spare);
                if !fits {
                    out.truncate(before);
                    if host {
                        break;
                    }
                    spare = 0;
                } else if !host {
                    spare -= used;
                }
            }
        }
    }
}

/// The template within its limits, with types sorted for the lookup and
/// bands from the highest percent down, both without repeats.
fn normalized(mut template: StatusTemplate, origin: Origin) -> Result<StatusTemplate, Refused> {
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
            Element::Pips { .. } => MAX_PIPS,
            Element::Slots { arts, .. } if arts.len() > MAX_SLOT_ARTS => return Err(Refused::Limit),
            Element::Slots { .. } => MAX_PIPS,
            Element::Icon { .. } | Element::Number { .. } => 1,
        };
    }
    if most_commands > origin.share(MAX_COMMANDS_PER_OBJECT) {
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
        Condition::AtMost(source, most) => amount(values, source).is_some_and(|a| a.value <= most),
        Condition::AtLeast(source, least) => {
            amount(values, source).is_some_and(|a| a.value >= least)
        }
    }
}

fn saturate(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn moved(rect: Rect, (x, y): (i32, i32)) -> Rect {
    rect.translate(x, y).bounded()
}

fn paint_element(object: &ObjectView<'_>, element: &Element, out: &mut Vec<WorldCmd>) {
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
                }
                .bounded();
                push(WorldOp::Image { rect, art });
            }
        }
        Element::Pips {
            at,
            step,
            source,
            full,
            empty,
        } => {
            let Some(Amount { value, max }) = amount(object.values, source) else {
                return;
            };
            let shown = i64::from(max.clamp(0, MAX_PIPS as i32));
            let lit = i64::from(value.clamp(0, max.max(0)));
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
                }
                .bounded();
                push(WorldOp::Image { rect, art });
            }
        }
        Element::Slots { at, step, ref arts } => {
            let x = i64::from(at.x) + i64::from(point.0);
            let y = i64::from(at.y) + i64::from(point.1);
            for slot in object.slots.iter().take(MAX_PIPS) {
                let Some(&art) = arts.get(usize::from(slot.code)) else {
                    continue;
                };
                let i = i64::from(slot.index);
                let rect = Rect {
                    x: saturate(x + i * i64::from(step.0)),
                    y: saturate(y + i * i64::from(step.1) + i64::from(slot.lift)),
                    ..at
                }
                .bounded();
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
            slots: &[],
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

    fn status(world: &World, objects: &[ObjectView<'_>]) -> Vec<WorldCmd> {
        let mut out = Vec::new();
        world.paint_status(objects, &mut out);
        out
    }

    fn ops(world: &World, values: &[Option<Amount>]) -> Vec<WorldOp> {
        status(world, &[object(values)])
            .into_iter()
            .map(|cmd| cmd.op)
            .collect()
    }

    fn arts(cmds: &[WorldCmd]) -> Vec<(Option<u64>, u32)> {
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
        let cases: [(Condition, &[Option<Amount>], bool); 19] = [
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
            (Condition::AtMost(S0, 3), &some(3, 0), true),
            (Condition::AtMost(S0, 3), &some(4, 9), false),
            (Condition::AtMost(S0, i32::MAX), &[], false),
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

    fn pips(empty: Option<ArtId>) -> Element {
        Element::Pips {
            at: UNIT,
            step: (-4, 2),
            source: S0,
            full: FULL,
            empty,
        }
    }

    #[test]
    fn pips_show_max_pictures_with_value_of_them_full() {
        let with_empty = world(template(vec![pips(Some(EMPTY))]));
        for (value, max, shown, lit) in [
            (0, 5, 5, 0),
            (3, 5, 5, 3),
            (5, 5, 5, 5),
            (9, 5, 5, 5),
            (-2, 5, 5, 0),
            (1, 0, 0, 0),
            (1, -3, 0, 0),
            (7, 600, MAX_PIPS as i32, 7),
        ] {
            let expected: Vec<_> = (0..shown)
                .map(|i| WorldOp::Image {
                    rect: Rect::new(-4 * i, 2 * i, 1, 1),
                    art: if i < lit { FULL } else { EMPTY },
                })
                .collect();
            assert_eq!(ops(&with_empty, &some(value, max)), expected, "{value}/{max}");
        }
        let lit_only = world(template(vec![pips(None)]));
        assert_eq!(ops(&lit_only, &some(3, 17)).len(), 3);
        assert!(ops(&lit_only, &some(0, 17)).is_empty());
        assert!(ops(&lit_only, &[]).is_empty());
    }

    #[test]
    fn slots_place_each_code_at_its_index_and_skip_unknown_codes() {
        let row = template(vec![Element::Slots {
            at: Rect::new(3, 10, 1, 1),
            step: (4, 2),
            arts: vec![EMPTY, FULL],
        }]);
        let shown = world(row);
        let slots = [
            Slot { index: 0, code: 1, lift: 0 },
            Slot { index: 2, code: 0, lift: -3 },
            Slot { index: 3, code: 7, lift: 0 },
            Slot { index: 5, code: 1, lift: 0 },
        ];
        let with_slots = ObjectView {
            slots: &slots,
            ..object(&[])
        };
        let ops: Vec<_> = status(&shown, &[with_slots]).into_iter().map(|cmd| cmd.op).collect();
        assert_eq!(
            ops,
            [
                WorldOp::Image { rect: Rect::new(3, 10, 1, 1), art: FULL },
                WorldOp::Image { rect: Rect::new(11, 11, 1, 1), art: EMPTY },
                WorldOp::Image { rect: Rect::new(23, 20, 1, 1), art: FULL },
            ]
        );
        assert!(status(&shown, &[object(&[])]).is_empty(), "no slots, nothing");
        let mut shared = World::default();
        let row = template(vec![Element::Slots { at: UNIT, step: (1, 0), arts: vec![FULL] }]);
        assert_eq!(shared.add_template(Origin::Layout, row), Err(Refused::Limit));
    }

    #[test]
    fn a_pips_row_takes_a_whole_share_so_only_the_host_adds_one() {
        let mut shared = World::default();
        let row = template(vec![pips(None)]);
        assert!(shared.add_template(Origin::Host, row.clone()).is_ok());
        assert_eq!(
            shared.add_template(Origin::Layout, row),
            Err(Refused::Limit)
        );
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
        world
            .add_template(Origin::External(3), template(vec![icon(1), icon(2)]))
            .unwrap();
        world
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
    }

    #[test]
    fn painting_appends() {
        let world = world(template(vec![icon(1)]));
        let earlier = WorldCmd {
            object: None,
            op: WorldOp::Rect {
                rect: UNIT,
                color: C,
            },
        };
        let mut out = vec![earlier];
        world.paint_status(&[object(&[])], &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], earlier);
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
            world.add_template(Origin::Host, make(limit)).unwrap();
            let kept = world.templates[0].def.clone();
            assert_eq!(
                world.add_template(Origin::Host, make(limit + 1)),
                Err(Refused::Limit),
                "{limit}"
            );
            assert_eq!(world.templates.len(), 1);
            assert_eq!(world.templates[0].def, kept);
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
        assert_eq!(world.templates.len(), 1);
    }

    #[test]
    fn templates_past_the_output_bound_are_left_out_whole() {
        let per_element = usize::from(MAX_SEGMENTS);
        let nearly = MAX_COMMANDS_PER_OBJECT / per_element - 1;
        let values = some(1, 1);
        let objects = [object(&values), object(&values)];
        for (second, painted) in [(2, nearly * per_element), (1, MAX_COMMANDS_PER_OBJECT)] {
            let mut world = world(template(vec![segments(MAX_SEGMENTS, Some(EMPTY)); nearly]));
            let wide = vec![segments(MAX_SEGMENTS, Some(EMPTY)); second];
            world.add_template(Origin::Host, template(wide)).unwrap();
            world
                .add_template(Origin::Host, template(vec![icon(9)]))
                .unwrap();
            // Exactly at the bound fits; past it the template and the next are left out.
            assert_eq!(status(&world, &objects).len(), 2 * painted, "{second}");
        }
    }

    #[test]
    fn others_templates_leave_the_host_its_share() {
        let per_element = usize::from(MAX_SEGMENTS);
        let wide = |count| template(vec![segments(MAX_SEGMENTS, Some(EMPTY)); count]);
        let client = Origin::External(1);
        let values = some(1, 1);
        let count = |world: &World| status(world, &[object(&values)]).len();
        assert_eq!(World::default().add_template(client, wide(7)), Err(Refused::Limit));
        for (first, painted) in [(5, 7 * per_element), (6, MAX_COMMANDS_PER_OBJECT)] {
            let mut world = World::default();
            world.add_template(client, wide(first)).unwrap();
            // The first not to fit, and the one after it that would.
            world.add_template(client, wide(2)).unwrap();
            world.add_template(client, template(vec![icon(9)])).unwrap();
            world.add_template(Origin::Host, wide(2)).unwrap();
            assert_eq!(count(&world), painted, "{first}");
        }
    }

    #[test]
    fn template_count_is_limited() {
        let mut world = World::default();
        for _ in 0..MAX_TEMPLATES {
            world.add_template(Origin::Host, named("t")).unwrap();
        }
        assert_eq!(
            world.add_template(Origin::Host, named("t")),
            Err(Refused::Limit)
        );
        assert_eq!(world.templates.len(), MAX_TEMPLATES);
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
                            let (WorldOp::Rect { rect, .. }
                            | WorldOp::Image { rect, .. }
                            | WorldOp::Number { rect, .. }) = cmd.op;
                            assert!(rect.w >= 0 && rect.x.checked_add(rect.w).is_some());
                            assert!(rect.h >= 0 && rect.y.checked_add(rect.h).is_some());
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
        // It starts at x 10, and its far edge fits an i32.
        assert_eq!(fill(&world, i32::MAX, i32::MAX), Some((i32::MAX - 10, A)));
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
                rect: Rect::new(i32::MAX, i32::MIN, 0, 1),
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
        let expected = [
            (-1, 0, 1),
            (i32::MAX - 1, i32::MIN, 1),
            (i32::MAX, i32::MIN, 0),
        ]
        .map(|(x, y, w)| WorldOp::Image {
            rect: Rect::new(x, y, w, 1),
            art: FULL,
        });
        let found: Vec<_> = status(&world, &[seen])
            .into_iter()
            .map(|cmd| cmd.op)
            .collect();
        assert_eq!(found, expected);
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
