//! The built-in health bars as `vera20k-gui` status templates.
//!
//! A template decides what one object's bar shows: whether its PIPBRD
//! background stands, which PIPS frame its health band lights, how many pips
//! the row has and how many of them are full. The engine still decides
//! whether a bar is drawn at all (`ui_overlays::techno_extras_for_parent`)
//! and works out the band and the counts with the original arithmetic; it
//! hands them to the templates as plain values and places each picture the
//! templates return at the original point.
//!
//! Original: unit bars `6F683C..6F6A8E` (PIPBRD frame 0/1, PIPS 16/17/18
//! every 2 pixels), building bars `6F64A0..6F6774` (PIPS 4/2/1 then the empty
//! frame 0, walking (-4,+2)).

use std::sync::LazyLock;

use vera20k_gui::Rect;
use vera20k_gui::hud::Origin;
use vera20k_gui::hud::world::{
    Amount, ArtId, Condition, Element, MAX_SLOT_ARTS, ObjectView, Slot, SourceId, StatusTemplate,
    World, WorldCmd, WorldOp,
};

/// Which bar an object has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HealthClass {
    Vehicle,
    Infantry,
    Building,
}

/// Which colour the health pips take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HealthBand {
    Green,
    Yellow,
    Red,
}

/// One picture of a bar, in the order the templates give them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HealthPiece {
    /// PIPBRD `frame`, `offset` from the bar's anchor.
    Background { frame: usize, offset: (i32, i32) },
    /// PIPS `frame`, the row's `index`th pip.
    Pip { frame: usize, index: i32 },
}

/// A unit bar's layout: its background and the start of its pip row.
struct UnitBar {
    class: HealthClass,
    background_frame: u32,
    background_offset: (i32, i32),
    pip_offset: (i32, i32),
}

const UNIT_BARS: [UnitBar; 2] = [
    UnitBar {
        class: HealthClass::Vehicle,
        background_frame: 0,
        background_offset: (1, -26),
        pip_offset: (-15, -25),
    },
    UnitBar {
        class: HealthClass::Infantry,
        background_frame: 1,
        background_offset: (11, -25),
        pip_offset: (-5, -24),
    },
];

/// A unit's pips are this many pixels apart (`UNIT_PIP_STEP_X`).
pub(crate) const UNIT_PIP_STEP: i32 = 2;
/// A building's row walks up the roof's edge.
const BUILDING_PIP_STEP: (i32, i32) = (-4, 2);
/// A building's empty pip.
const BUILDING_EMPTY_FRAME: u32 = 0;

const fn unit_frame(band: HealthBand) -> u32 {
    match band {
        HealthBand::Green => 16,
        HealthBand::Yellow => 17,
        HealthBand::Red => 18,
    }
}

const fn building_frame(band: HealthBand) -> u32 {
    match band {
        HealthBand::Green => 1,
        HealthBand::Yellow => 2,
        HealthBand::Red => 4,
    }
}

const BANDS: [HealthBand; 3] = [HealthBand::Green, HealthBand::Yellow, HealthBand::Red];

/// `values[HEALTH]`: the row's pips (`max`) and how many are full (`value`).
const HEALTH: SourceId = SourceId(0);
/// Present only for the object's band.
const fn band_source(band: HealthBand) -> SourceId {
    match band {
        HealthBand::Green => SourceId(1),
        HealthBand::Yellow => SourceId(2),
        HealthBand::Red => SourceId(3),
    }
}

const fn class_bit(class: HealthClass) -> u32 {
    match class {
        HealthClass::Vehicle => 1,
        HealthClass::Infantry => 2,
        HealthClass::Building => 4,
    }
}

/// State bit: the background is asked for.
const BACKGROUND: u32 = 1;
/// State bit: the scale pips are asked for.
const SCALE: u32 = 2;

/// The status marks an object shows one of each at most: the bomb clock,
/// the repair wrench, Ares' power-off marker, the rank insignia and the
/// self-heal marker. Each is a built-in template of its own (`status.<mark>`),
/// a single slot whose code is the frame the host picked, so a template can
/// hide a mark, give its frames other pictures or move it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    BombClock,
    Wrench,
    PowerOff,
    Rank,
    SelfHeal,
    /// Phobos' mobile shield PIPBRD.
    ShieldBackground,
    /// The status texts an object shows (`ui_overlays::status_text_instances`):
    /// its control group's digit, a building's Super timer lines, VERA's
    /// info rows, a building's power, primary and Ares labels. Their code is
    /// always 0; a template can hide a family or move it.
    GroupLabel,
    SuperLabel,
    InfoLabel,
    StatusLabel,
    /// Phobos' DigitalDisplay texts and glyphs.
    DigitalDisplay,
    /// The texts players put on their beacons.
    BeaconLabel,
    /// The rising bounty and refund texts.
    FlyingCredits,
    /// The speech bubble over a talking unit; its code is the frame.
    TalkBubble,
}

const MARKS: [(Mark, &str); 14] = [
    (Mark::BombClock, "status.bomb_clock"),
    (Mark::Wrench, "status.wrench"),
    (Mark::PowerOff, "status.power_off"),
    (Mark::Rank, "status.rank"),
    (Mark::SelfHeal, "status.self_heal"),
    (Mark::ShieldBackground, "status.shield_background"),
    (Mark::GroupLabel, "label.group"),
    (Mark::SuperLabel, "label.super"),
    (Mark::InfoLabel, "label.info"),
    (Mark::StatusLabel, "label.status"),
    (Mark::DigitalDisplay, "label.digital"),
    (Mark::BeaconLabel, "label.beacon"),
    (Mark::FlyingCredits, "label.credits"),
    (Mark::TalkBubble, "status.talk_bubble"),
];

/// State bit: the shield's pips are asked for, clear of the marks' bits.
const SHIELD: u32 = 1 << 20;

/// A mark's state bit, after the bars' and the scale's.
const fn mark_bit(mark: Mark) -> u32 {
    4 << mark as u32
}

const _: () = assert!(mark_bit(Mark::TalkBubble) < SHIELD, "the marks' bits stay clear of the shield's");

/// PIPBRD frames are told apart from PIPS frames by this bit of the art id.
const BORDER_ART: u32 = 1 << 16;

fn pips(frame: u32) -> ArtId {
    ArtId(frame)
}

fn border(frame: u32) -> ArtId {
    ArtId(BORDER_ART | frame)
}

/// A one-pixel rect at `(x, y)`: the host sizes each picture itself.
fn at(x: i32, y: i32) -> Rect {
    Rect::new(x, y, 1, 1)
}

fn templates() -> Vec<StatusTemplate> {
    let mut all = Vec::new();
    for bar in &UNIT_BARS {
        let name = match bar.class {
            HealthClass::Vehicle => "vehicle",
            _ => "infantry",
        };
        all.push(StatusTemplate {
            name: format!("health.{name}.background"),
            category: class_bit(bar.class),
            state: BACKGROUND,
            elements: vec![Element::Icon {
                rect: at(bar.background_offset.0, bar.background_offset.1),
                art: border(bar.background_frame),
            }],
            ..Default::default()
        });
        for band in BANDS {
            all.push(StatusTemplate {
                name: format!("health.{name}.{band:?}").to_lowercase(),
                category: class_bit(bar.class),
                when: vec![Condition::Present(band_source(band))],
                elements: vec![Element::Pips {
                    at: at(bar.pip_offset.0, bar.pip_offset.1),
                    step: (UNIT_PIP_STEP, 0),
                    source: HEALTH,
                    full: pips(unit_frame(band)),
                    empty: None,
                }],
                ..Default::default()
            });
        }
    }
    for band in BANDS {
        all.push(StatusTemplate {
            name: format!("health.building.{band:?}").to_lowercase(),
            category: class_bit(HealthClass::Building),
            when: vec![Condition::Present(band_source(band))],
            elements: vec![Element::Pips {
                at: at(0, 0),
                step: BUILDING_PIP_STEP,
                source: HEALTH,
                full: pips(building_frame(band)),
                empty: Some(pips(BUILDING_EMPTY_FRAME)),
            }],
            ..Default::default()
        });
    }
    // The scale pips (`709A90`): each slot four pixels on, a building's
    // walking up its roof two pixels a slot; the codes are the strip's own
    // frames. A unit's row is ten pixels below its anchor.
    let frames: Vec<ArtId> = (0..MAX_SLOT_ARTS as u32).map(pips).collect();
    all.push(StatusTemplate {
        name: "scale.unit".into(),
        category: class_bit(HealthClass::Vehicle) | class_bit(HealthClass::Infantry),
        state: SCALE,
        elements: vec![Element::Slots {
            at: at(0, 10),
            step: (4, 0),
            arts: frames.clone(),
        }],
        ..Default::default()
    });
    for (mark, name) in MARKS {
        all.push(StatusTemplate {
            name: name.to_owned(),
            state: mark_bit(mark),
            elements: vec![Element::Slots {
                at: at(0, 0),
                step: (0, 0),
                arts: (0..MAX_SLOT_ARTS as u32).map(pips).collect(),
            }],
            ..Default::default()
        });
    }
    // Phobos shield bars (`DrawShieldBar_Other`, `_Building`): the codes
    // are the frames the shield type picks for full and empty pips. A
    // unit's row starts two pixels right of and one under its bracket,
    // a building's walks up its roof from the bracket.
    all.push(StatusTemplate {
        name: "shield.unit".into(),
        category: class_bit(HealthClass::Vehicle) | class_bit(HealthClass::Infantry),
        state: SHIELD,
        elements: vec![Element::Slots {
            at: at(2, 1),
            step: (2, 0),
            arts: (0..MAX_SLOT_ARTS as u32).map(pips).collect(),
        }],
        ..Default::default()
    });
    all.push(StatusTemplate {
        name: "shield.building".into(),
        category: class_bit(HealthClass::Building),
        state: SHIELD,
        elements: vec![Element::Slots {
            at: at(0, 0),
            step: (-4, 2),
            arts: (0..MAX_SLOT_ARTS as u32).map(pips).collect(),
        }],
        ..Default::default()
    });
    all.push(StatusTemplate {
        name: "scale.building".into(),
        category: class_bit(HealthClass::Building),
        state: SCALE,
        elements: vec![Element::Slots {
            at: at(0, 0),
            step: (4, 2),
            arts: frames,
        }],
        ..Default::default()
    });
    all
}

/// The built-in bars, the same for every match.
/// The built-in templates, the same for every match, one world per family:
/// a query matches only its own family's few templates, which keeps a
/// frame of 5000 units' status well inside its budget.
struct BuiltIn {
    bars: World,
    scale: World,
    shields: World,
    /// By `Mark as usize`.
    marks: Vec<World>,
}

static BUILT_IN: LazyLock<BuiltIn> = LazyLock::new(|| {
    let mut built = BuiltIn {
        bars: World::default(),
        scale: World::default(),
        shields: World::default(),
        marks: MARKS.iter().map(|_| World::default()).collect(),
    };
    for template in templates() {
        let world = if template.name.starts_with("health.") {
            &mut built.bars
        } else if template.name.starts_with("scale.") {
            &mut built.scale
        } else if template.name.starts_with("shield.") {
            &mut built.shields
        } else {
            let (mark, _) = MARKS
                .iter()
                .find(|(_, name)| *name == template.name)
                .expect("every other built-in template is a mark's");
            &mut built.marks[*mark as usize]
        };
        world
            .add_template(Origin::Host, template)
            .expect("the built-in status templates are within the limits");
    }
    built
});

thread_local! {
    /// [`mark`]'s commands, kept from call to call.
    static MARK_CMDS: std::cell::RefCell<Vec<WorldCmd<'static>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Where a unit class's pip row starts from the bar's anchor, as the
/// original adds it to the anchor: in floating point.
pub(crate) fn pip_offset(class: HealthClass) -> [f32; 2] {
    let ((x, y), _) = row(class);
    [x as f32, y as f32]
}

/// Where a class's pip row starts and walks, from the bar's anchor.
fn row(class: HealthClass) -> ((i32, i32), (i32, i32)) {
    match UNIT_BARS.iter().find(|bar| bar.class == class) {
        Some(bar) => (bar.pip_offset, (UNIT_PIP_STEP, 0)),
        None => ((0, 0), BUILDING_PIP_STEP),
    }
}

/// The bar the templates give an object of `class`: the background when
/// `background` asks for it, then the pip row when `health` gives its band,
/// its full pips and its length. Each picture goes to `piece` in order.
pub(crate) fn paint(
    class: HealthClass,
    background: bool,
    health: Option<(HealthBand, i32, i32)>,
    cmds: &mut Vec<WorldCmd<'static>>,
    mut piece: impl FnMut(HealthPiece),
) {
    let mut values = [None; 4];
    if let Some((band, full, count)) = health {
        values[usize::from(HEALTH.0)] = Some(Amount {
            value: full,
            max: count,
        });
        values[usize::from(band_source(band).0)] = Some(Amount { value: 1, max: 1 });
    }
    let object = ObjectView {
        id: 0,
        x: 0,
        y: 0,
        type_id: 0,
        category: class_bit(class),
        relation: 0,
        state: if background { BACKGROUND } else { 0 },
        values: &values,
        slots: &[],
    };
    // The bars' templates ask for health or the background, never for the
    // scale (its own state bit), so they take only their own commands.
    cmds.clear();
    BUILT_IN.bars.paint_status(std::slice::from_ref(&object), cmds);
    let (start, step) = row(class);
    for cmd in cmds.iter() {
        let WorldOp::Image { rect, art } = cmd.op else {
            continue;
        };
        let frame = (art.0 & !BORDER_ART) as usize;
        if art.0 & BORDER_ART != 0 {
            piece(HealthPiece::Background {
                frame,
                offset: (rect.x, rect.y),
            });
        } else {
            let index = if step.0 != 0 {
                (rect.x - start.0) / step.0
            } else {
                (rect.y - start.1) / step.1
            };
            piece(HealthPiece::Pip { frame, index });
        }
    }
}

/// Where a bar is drawn from.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HealthAnchor {
    /// A unit's draw point and its type's bracket delta.
    Unit { sx: f32, sy: f32, delta: f32 },
    /// A building's first pip (`BuildingBarGeometry::first_pip`).
    Building { start: (f32, f32) },
}

/// One picture to draw: PIPBRD when `border`, else PIPS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HealthDraw {
    pub(crate) border: bool,
    pub(crate) frame: usize,
    pub(crate) point: (f32, f32),
}

/// [`paint`] at `anchor`: each picture at the point the original computes,
/// with its own floating-point steps.
pub(crate) fn draw(
    class: HealthClass,
    background: bool,
    health: Option<(HealthBand, i32, i32)>,
    anchor: HealthAnchor,
    cmds: &mut Vec<WorldCmd<'static>>,
    mut out: impl FnMut(HealthDraw),
) {
    paint(class, background, health, cmds, |piece| {
        let (border, frame, point) = match (piece, anchor) {
            (HealthPiece::Background { frame, offset }, HealthAnchor::Unit { sx, sy, delta }) => {
                (true, frame, (sx + offset.0 as f32, sy + delta + offset.1 as f32))
            }
            (HealthPiece::Pip { frame, index }, HealthAnchor::Unit { sx, sy, delta }) => {
                let [x, y] = pip_offset(class);
                (
                    false,
                    frame,
                    (sx + x + index as f32 * UNIT_PIP_STEP as f32, sy + delta + y),
                )
            }
            (HealthPiece::Pip { frame, index }, HealthAnchor::Building { start }) => (
                false,
                frame,
                (start.0 - (index * 4) as f32, start.1 + (index * 2) as f32),
            ),
            (HealthPiece::Background { .. }, HealthAnchor::Building { .. }) => return,
        };
        out(HealthDraw {
            border,
            frame,
            point,
        });
    });
}

/// The scale pips of an object of `class`, as the built-in scale templates
/// lay them out: each pip's frame and its offset from the row's anchor,
/// which for the stock templates is the pip's own `offset_pixels`.
/// `pips` are the frames and offsets the record captured
/// (`ui_overlays::ScalePipDraw`): slot `offset_pixels[0] / 4`, lifted by
/// what its offset differs from the stock row.
pub(crate) fn scale_pips(
    class: HealthClass,
    pips: impl IntoIterator<Item = (usize, [i32; 2])>,
    slots: &mut Vec<Slot>,
    cmds: &mut Vec<WorldCmd<'static>>,
    mut out: impl FnMut(usize, [i32; 2]),
) {
    slots.clear();
    // The stock row puts slot n at (4n, 10) under a unit and (4n, 2n) on a
    // building; an arm on another line (the ammo pips, three pixels up)
    // keeps the difference as the slot's lift.
    let building = class == HealthClass::Building;
    slots.extend(pips.into_iter().map(|(frame, offset)| {
        let index = offset[0] / 4;
        let line = if building { index * 2 } else { 10 };
        Slot {
            index: u16::try_from(index).unwrap_or(u16::MAX),
            code: u16::try_from(frame).unwrap_or(u16::MAX),
            lift: i16::try_from(offset[1] - line).unwrap_or(i16::MAX),
        }
    }));
    if slots.is_empty() {
        return;
    }
    let object = ObjectView {
        id: 0,
        x: 0,
        y: 0,
        type_id: 0,
        category: class_bit(class),
        relation: 0,
        state: SCALE,
        values: &[],
        slots,
    };
    cmds.clear();
    BUILT_IN.scale.paint_status(std::slice::from_ref(&object), cmds);
    for cmd in cmds.iter() {
        if let WorldOp::Image { rect, art } = cmd.op {
            out(art.0 as usize, [rect.x, rect.y]);
        }
    }
}

/// The `mark` showing `frame`, as its template has it: the frame to draw
/// and the offset to move it by, or none where the template hides it.
pub(crate) fn mark(mark: Mark, frame: usize) -> Option<(usize, [i32; 2])> {
    let slot = [Slot {
        index: 0,
        code: u16::try_from(frame).unwrap_or(u16::MAX),
        lift: 0,
    }];
    let object = ObjectView {
        id: 0,
        x: 0,
        y: 0,
        type_id: 0,
        category: 0,
        relation: 0,
        state: mark_bit(mark),
        values: &[],
        slots: &slot,
    };
    MARK_CMDS.with_borrow_mut(|cmds| {
        cmds.clear();
        BUILT_IN.marks[mark as usize].paint_status(std::slice::from_ref(&object), cmds);
        cmds.iter().find_map(|cmd| match cmd.op {
            WorldOp::Image { rect, art } => Some((art.0 as usize, [rect.x, rect.y])),
            _ => None,
        })
    })
}

/// A shield bar's pips of an object of `class`, in order, as the built-in
/// shield templates lay them out: each pip's frame and offset from the
/// bracket (the unit's base). `frames` are the frames the shield type picks
/// for each pip in turn.
#[cfg_attr(not(feature = "phobos"), allow(dead_code))]
pub(crate) fn shield_pips(
    class: HealthClass,
    frames: impl IntoIterator<Item = usize>,
    slots: &mut Vec<Slot>,
    cmds: &mut Vec<WorldCmd<'static>>,
    mut out: impl FnMut(usize, [i32; 2]),
) {
    slots.clear();
    slots.extend(frames.into_iter().enumerate().map(|(index, frame)| Slot {
        index: u16::try_from(index).unwrap_or(u16::MAX),
        code: u16::try_from(frame).unwrap_or(u16::MAX),
        lift: 0,
    }));
    if slots.is_empty() {
        return;
    }
    let object = ObjectView {
        id: 0,
        x: 0,
        y: 0,
        type_id: 0,
        category: class_bit(class),
        relation: 0,
        state: SHIELD,
        values: &[],
        slots,
    };
    cmds.clear();
    BUILT_IN.shields.paint_status(std::slice::from_ref(&object), cmds);
    for cmd in cmds.iter() {
        if let WorldOp::Image { rect, art } = cmd.op {
            out(art.0 as usize, [rect.x, rect.y]);
        }
    }
}

/// Whether `family`'s template shows it this frame, and how far it moves
/// it: a text family's one question to its template.
pub(crate) fn family(family: Mark) -> Option<[i32; 2]> {
    mark(family, 0).map(|(_, offset)| offset)
}

/// `instances` moved by a template's `offset`, where it has one.
pub(crate) fn shift_instances<'a>(
    instances: impl IntoIterator<Item = &'a mut crate::render::batch::SpriteInstance>,
    offset: [i32; 2],
) {
    if offset == [0, 0] {
        return;
    }
    for instance in instances {
        instance.position[0] += offset[0] as f32;
        instance.position[1] += offset[1] as f32;
    }
}

/// `point` moved by a template's `offset`; untouched where there is none,
/// so a stock mark keeps the original arithmetic to the last bit.
pub(crate) fn shifted(point: (f32, f32), offset: [i32; 2]) -> (f32, f32) {
    if offset == [0, 0] {
        point
    } else {
        (point.0 + offset[0] as f32, point.1 + offset[1] as f32)
    }
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    fn pieces(
        class: HealthClass,
        background: bool,
        health: Option<(HealthBand, i32, i32)>,
    ) -> Vec<HealthPiece> {
        let mut out = Vec::new();
        paint(class, background, health, &mut Vec::new(), |piece| out.push(piece));
        out
    }

    #[test]
    fn a_vehicle_bar_is_its_background_then_its_full_pips() {
        assert_eq!(
            pieces(HealthClass::Vehicle, true, Some((HealthBand::Yellow, 3, 17))),
            [
                HealthPiece::Background { frame: 0, offset: (1, -26) },
                HealthPiece::Pip { frame: 17, index: 0 },
                HealthPiece::Pip { frame: 17, index: 1 },
                HealthPiece::Pip { frame: 17, index: 2 },
            ]
        );
        assert_eq!(
            pieces(HealthClass::Infantry, true, None),
            [HealthPiece::Background { frame: 1, offset: (11, -25) }]
        );
        assert!(pieces(HealthClass::Infantry, false, None).is_empty());
    }

    #[test]
    fn a_stock_mark_keeps_its_frame_and_place() {
        for (mark, _) in MARKS {
            assert_eq!(super::mark(mark, 14), Some((14, [0, 0])));
            assert_eq!(super::mark(mark, 0), Some((0, [0, 0])));
            assert_eq!(super::mark(mark, MAX_SLOT_ARTS), None, "past the table");
        }
        let point = (-0.0f32, 12.5f32);
        let kept = shifted(point, [0, 0]);
        assert_eq!((kept.0.to_bits(), kept.1.to_bits()), (point.0.to_bits(), point.1.to_bits()));
    }

    #[test]
    fn a_building_bar_lights_its_full_pips_and_shows_the_rest_empty() {
        let bar = pieces(HealthClass::Building, true, Some((HealthBand::Red, 2, 4)));
        assert_eq!(
            bar,
            [
                HealthPiece::Pip { frame: 4, index: 0 },
                HealthPiece::Pip { frame: 4, index: 1 },
                HealthPiece::Pip { frame: 0, index: 2 },
                HealthPiece::Pip { frame: 0, index: 3 },
            ]
        );
    }
}
