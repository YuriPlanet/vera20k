//! The built-in battlefield status as `vera20k-gui` status templates.
//!
//! A template decides how one object's status is laid out: whether its
//! PIPBRD background stands, which PIPS frame its health band lights, where
//! each picture sits from the object's anchor and how a row walks. The
//! engine decides the state: whether a bar is drawn at all
//! (`ui_overlays::techno_extras_for_parent`), the band, the counts, the
//! slots and which mark frame an animation is on. It hands them to the
//! templates as plain values and places each picture the templates return
//! at its anchor moved by the picture's rect.
//!
//! Original: unit bars `6F683C..6F6A8E` (PIPBRD frame 0/1, PIPS 16/17/18
//! every 2 pixels), building bars `6F64A0..6F6774` (PIPS 4/2/1 then the empty
//! frame 0, walking (-4,+2)).

use std::cell::RefCell;
use std::sync::LazyLock;

use vera20k_gui::Rect;
use vera20k_gui::origin::Origin;
use vera20k_gui::status::{
    Amount, ArtId, Condition, Element, MAX_SLOT_ARTS, ObjectView, Slot, SourceId, StatusTemplate,
    World, WorldCmd, WorldOp,
};

use crate::app::presentation::ui_overlays::ScalePipDraw;
use crate::map::entities::EntityCategory;

/// Which bar an object has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HealthClass {
    Vehicle,
    Infantry,
    Building,
}

impl HealthClass {
    /// An object's class by its category: aircraft take the vehicles' bar.
    pub(crate) fn of(category: EntityCategory) -> Self {
        match category {
            EntityCategory::Infantry => Self::Infantry,
            EntityCategory::Structure => Self::Building,
            _ => Self::Vehicle,
        }
    }
}

/// Which colour the health pips take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HealthBand {
    Green,
    Yellow,
    Red,
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
const UNIT_PIP_STEP: i32 = 2;
/// A building's row walks up the roof's edge.
const BUILDING_PIP_STEP: (i32, i32) = (-4, 2);
/// A building's empty pip.
const BUILDING_EMPTY_FRAME: u32 = 0;
/// PIPS.SHP's unit health pips. Original RA2's copy (ra2.mix -> conquer.mix)
/// has them at 15/16/17; YR's (conqmd) inserts an insignia at 15 and shifts
/// them to 16/17/18. The `-ra2` launch flag says which strip is loaded.
fn unit_frame(band: HealthBand) -> u32 {
    let first = if crate::util::ra2_launch::enabled() { 15 } else { 16 };
    first
        + match band {
            HealthBand::Green => 0,
            HealthBand::Yellow => 1,
            HealthBand::Red => 2,
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
/// self-heal marker. Each is a built-in template of its own (`status.<mark>`,
/// one a category where the categories place it apart), a single slot at the
/// mark's place whose code is the frame the host picked, so a template can
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

/// The built-in world a template belongs to.
#[derive(Clone, Copy)]
enum Family {
    Bars,
    Scale,
    Shields,
    Mark(Mark),
}

/// Where a mark sits from the point the engine draws it from, by category:
/// a name suffix, the category bits (zero for every one) and the offset.
type Places = &'static [(&'static str, u32, (i32, i32))];

/// Everywhere at the point itself.
const AT_POINT: Places = &[("", 0, (0, 0))];

/// The rank insignia from the object's point: the stock leaf
/// (`70AA20..AA4F`) and Ares' (`1004E1D7`) put an infantry's at (5,2) and
/// every other object's at (10,6).
#[cfg(not(feature = "phobos"))]
const RANK_PLACES: Places = &[
    (".infantry", class_bit(HealthClass::Infantry), (5, 2)),
    (
        ".other",
        class_bit(HealthClass::Vehicle) | class_bit(HealthClass::Building),
        (10, 6),
    ),
];
/// Phobos' `DrawInsignia` places the insignia from its own INI
/// adjustments, which the engine adds to the point.
#[cfg(feature = "phobos")]
const RANK_PLACES: Places = AT_POINT;

/// The self-heal marker from the object's point (`70A5EC`): health's
/// (-10,+10), scale's -5 X (+11 for infantry), then (38,-32) for a unit and
/// (19,-35) for an infantry. The bracket delta is not passed. Only units and
/// infantry draw one.
const SELF_HEAL_PLACES: Places = &[
    (".vehicle", class_bit(HealthClass::Vehicle), (23, -22)),
    (
        ".infantry",
        class_bit(HealthClass::Infantry) | class_bit(HealthClass::Building),
        (15, -25),
    ),
];

const fn mark_places(mark: Mark) -> Places {
    match mark {
        Mark::Rank => RANK_PLACES,
        Mark::SelfHeal => SELF_HEAL_PLACES,
        _ => AT_POINT,
    }
}

fn templates() -> Vec<(Family, StatusTemplate)> {
    let mut all = Vec::new();
    for bar in &UNIT_BARS {
        let name = match bar.class {
            HealthClass::Vehicle => "vehicle",
            _ => "infantry",
        };
        all.push((
            Family::Bars,
            StatusTemplate {
                name: format!("health.{name}.background"),
                category: class_bit(bar.class),
                state: BACKGROUND,
                elements: vec![Element::Icon {
                    rect: at(bar.background_offset.0, bar.background_offset.1),
                    art: border(bar.background_frame),
                }],
                ..Default::default()
            },
        ));
        for band in BANDS {
            all.push((
                Family::Bars,
                StatusTemplate {
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
                },
            ));
        }
    }
    for band in BANDS {
        all.push((
            Family::Bars,
            StatusTemplate {
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
            },
        ));
    }
    // The scale pips (`709A90`): each slot four pixels on, a building's
    // walking up its roof two pixels a slot; the codes are the strip's own
    // frames. A mobile row starts at the health anchor's (-10,+10), then
    // -5 X with infantry's +11 (`709B0F`/`6F6A8E`); a building's at its
    // foundation edge's (+6,-1) (`6F682C`).
    let frames: Vec<ArtId> = (0..MAX_SLOT_ARTS as u32).map(pips).collect();
    for (name, class, start, step) in [
        ("scale.vehicle", HealthClass::Vehicle, (-15, 10), (4, 0)),
        ("scale.infantry", HealthClass::Infantry, (-4, 10), (4, 0)),
        ("scale.building", HealthClass::Building, (6, -1), (4, 2)),
    ] {
        all.push((
            Family::Scale,
            StatusTemplate {
                name: name.into(),
                category: class_bit(class),
                state: SCALE,
                elements: vec![Element::Slots {
                    at: at(start.0, start.1),
                    step,
                    arts: frames.clone(),
                }],
                ..Default::default()
            },
        ));
    }
    for (mark, name) in MARKS {
        for &(suffix, category, (x, y)) in mark_places(mark) {
            all.push((
                Family::Mark(mark),
                StatusTemplate {
                    name: format!("{name}{suffix}"),
                    category,
                    state: mark_bit(mark),
                    elements: vec![Element::Slots {
                        at: at(x, y),
                        step: (0, 0),
                        arts: frames.clone(),
                    }],
                    ..Default::default()
                },
            ));
        }
    }
    // Phobos shield bars (`DrawShieldBar_Other`, `_Building`): the codes
    // are the frames the shield type picks for full and empty pips. A
    // unit's row starts two pixels right of and one under its bracket,
    // a building's walks up its roof from the bracket.
    all.push((
        Family::Shields,
        StatusTemplate {
            name: "shield.unit".into(),
            category: class_bit(HealthClass::Vehicle) | class_bit(HealthClass::Infantry),
            state: SHIELD,
            elements: vec![Element::Slots {
                at: at(2, 1),
                step: (2, 0),
                arts: frames.clone(),
            }],
            ..Default::default()
        },
    ));
    all.push((
        Family::Shields,
        StatusTemplate {
            name: "shield.building".into(),
            category: class_bit(HealthClass::Building),
            state: SHIELD,
            elements: vec![Element::Slots {
                at: at(0, 0),
                step: (-4, 2),
                arts: frames,
            }],
            ..Default::default()
        },
    ));
    all
}

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
    for (family, template) in templates() {
        let world = match family {
            Family::Bars => &mut built.bars,
            Family::Scale => &mut built.scale,
            Family::Shields => &mut built.shields,
            Family::Mark(mark) => &mut built.marks[mark as usize],
        };
        world
            .add_template(Origin::Host, template)
            .expect("the built-in status templates are within the limits");
    }
    built
});

thread_local! {
    /// The commands of the last query, kept from call to call: a frame asks
    /// thousands of times and, once grown, allocates nothing.
    static CMDS: RefCell<Vec<WorldCmd>> = const { RefCell::new(Vec::new()) };
    /// The slots of the last scale or shield row, kept likewise.
    static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
}

/// `world`'s pictures for `object`, in order: each image's rect and art.
/// `out` runs while the shared commands are borrowed, so it must not ask
/// the templates again.
fn painted(world: &World, object: &ObjectView<'_>, mut out: impl FnMut(Rect, ArtId)) {
    CMDS.with_borrow_mut(|cmds| {
        cmds.clear();
        world.paint_status(std::slice::from_ref(object), cmds);
        for cmd in cmds.iter() {
            if let WorldOp::Image { rect, art } = cmd.op {
                out(rect, art);
            }
        }
    });
}

/// An object of `class` showing `state`, with `values` and `slots`.
fn view<'v>(
    class: Option<HealthClass>,
    state: u32,
    values: &'v [Option<Amount>],
    slots: &'v [Slot],
) -> ObjectView<'v> {
    ObjectView {
        id: 0,
        x: 0,
        y: 0,
        type_id: 0,
        category: class.map_or(0, class_bit),
        relation: 0,
        state,
        values,
        slots,
    }
}

/// `anchor` moved by a template's `rect`. Anchors and rects are whole
/// pixels, so the sum is the original's to the bit, whatever order the
/// original added its parts in.
fn placed(anchor: (f32, f32), rect: Rect) -> (f32, f32) {
    (anchor.0 + rect.x as f32, anchor.1 + rect.y as f32)
}

/// One picture of a bar: PIPBRD when `border`, else PIPS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HealthDraw {
    pub(crate) border: bool,
    pub(crate) frame: usize,
    pub(crate) point: (f32, f32),
}

/// The bar the templates give an object of `class`, drawn from `anchor`
/// (a unit's draw point lowered by its type's bracket delta, a building's
/// first pip): the background when `background` asks for it, then the pip
/// row when `health` gives its band, its full pips and its length. Each
/// picture goes to `out` in order, at the anchor moved by its rect.
pub(crate) fn bar(
    class: HealthClass,
    background: bool,
    health: Option<(HealthBand, i32, i32)>,
    anchor: (f32, f32),
    mut out: impl FnMut(HealthDraw),
) {
    let mut values = [None; 4];
    if let Some((band, full, count)) = health {
        values[usize::from(HEALTH.0)] = Some(Amount {
            value: full,
            max: count,
        });
        values[usize::from(band_source(band).0)] = Some(Amount { value: 1, max: 1 });
    }
    let object = view(
        Some(class),
        if background { BACKGROUND } else { 0 },
        &values,
        &[],
    );
    // The bars' templates ask for health or the background, never for the
    // scale (its own state bit), so they take only their own commands.
    painted(&BUILT_IN.bars, &object, |rect, art| {
        out(HealthDraw {
            border: art.0 & BORDER_ART != 0,
            frame: (art.0 & !BORDER_ART) as usize,
            point: placed(anchor, rect),
        })
    });
}

/// `pips`, the scale row of an object of `class`, drawn from `anchor` (a
/// unit's draw point, a building's foundation edge) as the built-in scale
/// templates lay it out: each pip's frame and point to `out`, in order.
pub(crate) fn scale_pips(
    class: HealthClass,
    pips: &[ScalePipDraw],
    anchor: (f32, f32),
    mut out: impl FnMut(usize, (f32, f32)),
) {
    if pips.is_empty() {
        return;
    }
    SLOTS.with_borrow_mut(|slots| {
        slots.clear();
        slots.extend(pips.iter().map(|pip| Slot {
            index: u16::try_from(pip.slot).unwrap_or(u16::MAX),
            code: u16::try_from(pip.frame).unwrap_or(u16::MAX),
            lift: pip.lift,
        }));
        let object = view(Some(class), SCALE, &[], slots);
        painted(&BUILT_IN.scale, &object, |rect, art| {
            out(art.0 as usize, placed(anchor, rect))
        });
    });
}

/// The `mark` showing `frame` on an object of `class` (none for a mark
/// every object places alike), as its template has it: the frame to draw
/// and the offset to move the mark's point by, or none where the template
/// hides it.
pub(crate) fn mark(
    mark: Mark,
    class: Option<HealthClass>,
    frame: usize,
) -> Option<(usize, [i32; 2])> {
    let slot = [Slot {
        index: 0,
        code: u16::try_from(frame).unwrap_or(u16::MAX),
        lift: 0,
    }];
    let object = view(class, mark_bit(mark), &[], &slot);
    let mut first = None;
    painted(&BUILT_IN.marks[mark as usize], &object, |rect, art| {
        first.get_or_insert((art.0 as usize, [rect.x, rect.y]));
    });
    first
}

/// A shield bar's pips of an object of `class`, drawn from `anchor` (the
/// bracket, a unit's lowered by its bracket delta) as the built-in shield
/// templates lay them out: each pip's frame and point to `out`, in order.
/// `frames` are the frames the shield type picks for each pip in turn.
#[cfg(feature = "phobos")]
pub(crate) fn shield_pips(
    class: HealthClass,
    frames: impl IntoIterator<Item = usize>,
    anchor: (f32, f32),
    mut out: impl FnMut(usize, (f32, f32)),
) {
    SLOTS.with_borrow_mut(|slots| {
        slots.clear();
        slots.extend(frames.into_iter().enumerate().map(|(index, frame)| Slot {
            index: u16::try_from(index).unwrap_or(u16::MAX),
            code: u16::try_from(frame).unwrap_or(u16::MAX),
            lift: 0,
        }));
        if slots.is_empty() {
            return;
        }
        let object = view(Some(class), SHIELD, &[], slots);
        painted(&BUILT_IN.shields, &object, |rect, art| {
            out(art.0 as usize, placed(anchor, rect))
        });
    });
}

/// Whether `family`'s template shows it this frame, and how far it moves
/// it: a text family's one question to its template, asked once a frame.
pub(crate) fn family(family: Mark) -> Option<[i32; 2]> {
    mark(family, None, 0).map(|(_, offset)| offset)
}

/// `instances` moved by a template's `offset`, where it has one.
#[cfg(feature = "phobos")]
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
/// so a mark at its point keeps the original arithmetic to the last bit.
pub(crate) fn shifted(point: (f32, f32), offset: [i32; 2]) -> (f32, f32) {
    if offset == [0, 0] {
        point
    } else {
        (point.0 + offset[0] as f32, point.1 + offset[1] as f32)
    }
}

/// `point` moved by a template's whole-pixel `offset`.
pub(crate) fn shifted_px(point: [i32; 2], offset: [i32; 2]) -> [i32; 2] {
    [
        point[0].saturating_add(offset[0]),
        point[1].saturating_add(offset[1]),
    ]
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    fn bars(
        class: HealthClass,
        background: bool,
        health: Option<(HealthBand, i32, i32)>,
    ) -> Vec<HealthDraw> {
        let mut out = Vec::new();
        bar(class, background, health, (100.0, 50.0), |draw| out.push(draw));
        out
    }

    fn draw(border: bool, frame: usize, x: f32, y: f32) -> HealthDraw {
        HealthDraw {
            border,
            frame,
            point: (x, y),
        }
    }

    #[test]
    fn a_vehicle_bar_is_its_background_then_its_full_pips() {
        assert_eq!(
            bars(HealthClass::Vehicle, true, Some((HealthBand::Yellow, 3, 17))),
            [
                draw(true, 0, 101.0, 24.0),
                draw(false, 17, 85.0, 25.0),
                draw(false, 17, 87.0, 25.0),
                draw(false, 17, 89.0, 25.0),
            ]
        );
        assert_eq!(
            bars(HealthClass::Infantry, true, None),
            [draw(true, 1, 111.0, 25.0)]
        );
        assert!(bars(HealthClass::Infantry, false, None).is_empty());
    }

    #[test]
    fn a_building_bar_lights_its_full_pips_and_shows_the_rest_empty() {
        assert_eq!(
            bars(HealthClass::Building, true, Some((HealthBand::Red, 2, 4))),
            [
                draw(false, 4, 100.0, 50.0),
                draw(false, 4, 96.0, 52.0),
                draw(false, 0, 92.0, 54.0),
                draw(false, 0, 88.0, 56.0),
            ]
        );
    }

    #[test]
    fn a_scale_row_starts_at_its_class_place_and_keeps_its_lift() {
        let row = [
            ScalePipDraw { slot: 0, frame: 1, lift: 0 },
            ScalePipDraw { slot: 2, frame: 13, lift: -3 },
        ];
        let mut out = Vec::new();
        for class in [HealthClass::Vehicle, HealthClass::Infantry, HealthClass::Building] {
            scale_pips(class, &row, (0.0, 0.0), |frame, point| out.push((frame, point)));
        }
        assert_eq!(
            out,
            [
                (1, (-15.0, 10.0)),
                (13, (-7.0, 7.0)),
                (1, (-4.0, 10.0)),
                (13, (4.0, 7.0)),
                (1, (6.0, -1.0)),
                (13, (14.0, 0.0)),
            ]
        );
    }

    #[test]
    fn a_mark_keeps_its_frame_at_its_place() {
        for (mark, _) in MARKS {
            if mark_places(mark).len() > 1 {
                continue;
            }
            assert_eq!(super::mark(mark, None, 14), Some((14, [0, 0])));
            assert_eq!(super::mark(mark, None, 0), Some((0, [0, 0])));
            assert_eq!(super::mark(mark, None, MAX_SLOT_ARTS), None, "past the table");
        }
        assert_eq!(
            super::mark(Mark::SelfHeal, Some(HealthClass::Vehicle), 0x14),
            Some((0x14, [23, -22]))
        );
        assert_eq!(
            super::mark(Mark::SelfHeal, Some(HealthClass::Infantry), 0x0D),
            Some((0x0D, [15, -25]))
        );
        #[cfg(not(feature = "phobos"))]
        {
            assert_eq!(
                super::mark(Mark::Rank, Some(HealthClass::Infantry), 14),
                Some((14, [5, 2]))
            );
            assert_eq!(
                super::mark(Mark::Rank, Some(HealthClass::Building), 15),
                Some((15, [10, 6]))
            );
        }
        let point = (-0.0f32, 12.5f32);
        let kept = shifted(point, [0, 0]);
        assert_eq!((kept.0.to_bits(), kept.1.to_bits()), (point.0.to_bits(), point.1.to_bits()));
    }
}
