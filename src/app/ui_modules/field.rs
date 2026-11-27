//! What the mod's and the map's additions draw over the battlefield
//! (VERA, RFC 0001 section 10): the status templates ([`super::status`]) at
//! every object the local player sees, the modules' `Decoration`s at their
//! objects and their `MapMarker`s at their cells (appendix 7.10). Built with the world's instances
//! each frame (`render::build_instances`), in world pixels, and drawn in the
//! tactical pass after the objects, clipped to the view and zoomed with it:
//! the fills from one white texel, the pictures from the modules' named
//! pictures, the numbers in the match's font.
//!
//! An object counts when [`ui_overlays::module_status_anchor`] gives it an
//! anchor: drawn, not in a transport and seen by the local player (the
//! fog's current sight under `FogOfWar`, not disguised to it). Each
//! command's offset from its anchor is divided by the zoom, as the health
//! strip keeps its pixel size.
//!
//! A decoration draws at its object's anchor moved by its `X`, `Y`: its
//! `Art` from there, then its `Number` and `Label` on one line under the
//! picture, in its `Tint` (white without one). An object the player does
//! not see shows none of its decorations. A marker draws at its cell's
//! centre on the ground (or its object, while seen): a dot in its `Tint`,
//! a line through its `Items` cells, a circle of `Width` leptons on the
//! ground, and its picture, number and label as a decoration's. Lines and
//! circles are one-pixel dots a pixel apart, kept to the view.

use vera20k_gui::status::{Amount, WorldCmd, WorldOp};

use crate::app::presentation::frame::PresentationFrame;
use crate::app::presentation::ui_overlays::{self, StatusViewport};
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::rgba_tint as tint;
use crate::render::shell_paint::quad;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

use super::UiModules;
use super::status::{SLOTS, StatusObject};

/// The battlefield layer's instances, one list per atlas, drawn in this
/// order.
#[derive(Default)]
pub(crate) struct FieldInstances {
    pub(crate) fills: Vec<SpriteInstance>,
    pub(crate) pictures: Vec<SpriteInstance>,
    pub(crate) text: Vec<SpriteInstance>,
}

/// What the frame's objects read beside themselves.
pub(crate) struct FieldView<'a> {
    pub(crate) sim: &'a Simulation,
    pub(crate) frame: &'a PresentationFrame,
    pub(crate) rules: Option<&'a RuleSet>,
    /// The local player; `None` for a spectator.
    pub(crate) player: Option<InternedId>,
    pub(crate) hovered: Option<u64>,
    pub(crate) viewport: StatusViewport,
}

/// Pixels past the view an anchor may be and still draw: a template's
/// rects reach this far from it.
const MARGIN: f32 = 128.0;

pub(crate) fn build(ui: &UiModules, view: &FieldView<'_>, font: &BitFont) -> FieldInstances {
    let mut out = FieldInstances::default();
    let attached = ui.tree.attached();
    if !attached.is_empty() {
        attached_instances(ui, view, font, &attached, &mut out);
    }
    if !ui.status.has_templates() {
        return out;
    }
    let zoom = zoom_of(view);
    let objects = status_objects(view);
    let mut values: Vec<[Option<Amount>; SLOTS]> = Vec::new();
    let mut cmds: Vec<WorldCmd> = Vec::new();
    ui.status
        .paint(&objects, view.sim.ext_modules().vars(), &mut values, &mut cmds);
    let mut at = 0;
    for cmd in &cmds {
        // The commands come in the objects' order.
        while objects
            .get(at)
            .is_some_and(|object| Some(object.id) != cmd.object)
        {
            at += 1;
        }
        let Some(object) = objects.get(at) else {
            break;
        };
        let anchor = (object.anchor.0 as f32, object.anchor.1 as f32);
        let place = |x: i32, y: i32| {
            [
                anchor.0 + (x as f32 - anchor.0) / zoom,
                anchor.1 + (y as f32 - anchor.1) / zoom,
            ]
        };
        match cmd.op {
            WorldOp::Rect { rect, color } => {
                let (tint, alpha) = tint(color);
                out.fills.push(quad(
                    place(rect.x, rect.y),
                    [rect.w as f32 / zoom, rect.h as f32 / zoom],
                    ([0.0, 0.0], [1.0, 1.0]),
                    tint,
                    alpha,
                    0.0,
                ));
            }
            WorldOp::Image { rect, art } => {
                let Some(entry) = ui
                    .status
                    .art_name(art)
                    .and_then(|name| ui.art.art().frames(name))
                    .map(|[up, _]| *up)
                else {
                    continue;
                };
                let size = if rect.w > 0 && rect.h > 0 {
                    [rect.w as f32, rect.h as f32]
                } else {
                    entry.pixel_size
                };
                out.pictures.push(quad(
                    place(rect.x, rect.y),
                    [size[0] / zoom, size[1] / zoom],
                    (entry.uv_origin, entry.uv_size),
                    [1.0; 3],
                    1.0,
                    0.0,
                ));
            }
            WorldOp::Number { rect, value, color } => {
                let [x, y] = place(rect.x, rect.y);
                let (tint, _) = tint(color);
                out.text.extend(font.build_text(
                    &value.to_string(),
                    x,
                    y,
                    1.0 / zoom,
                    0.0,
                    tint,
                    [0.0; 2],
                ));
            }
        }
    }
    out
}

/// The frame's objects the templates may draw at, in the frame's order.
fn status_objects<'f>(view: &FieldView<'f>) -> Vec<StatusObject<'f>> {
    let camera = view.viewport.camera;
    let size = view.viewport.size;
    let zoom = view.viewport.zoom.max(f32::MIN_POSITIVE);
    let (left, top) = (camera[0] - MARGIN, camera[1] - MARGIN);
    let (right, bottom) = (
        camera[0] + size[0] / zoom + MARGIN,
        camera[1] + size[1] / zoom + MARGIN,
    );
    let sim = view.sim;
    view.frame
        .objects()
        .filter_map(|entity| {
            let anchor = ui_overlays::module_status_anchor(
                sim,
                entity,
                view.rules,
                view.player,
                view.viewport.ignore_visibility,
            )?;
            if !(left..right).contains(&anchor.0) || !(top..bottom).contains(&anchor.1) {
                return None;
            }
            // A spy or Mirage disguised to this player shows as what it
            // wears, as the screen and client reads show it: the worn type
            // and house, the real health share of the worn `Strength=`, and
            // nothing of its own (`client_reads`). One worn as terrain has
            // no anchor.
            let worn = entity
                .seen
                .disguised_to_observer
                .then(|| entity.disguise.as_ref())
                .flatten()
                .and_then(|disguise| Some((disguise.type_id()?, disguise.house()?)));
            if entity.seen.disguised_to_observer && worn.is_none() {
                return None;
            }
            let (type_ref, owner) = worn.unwrap_or((entity.type_ref(), entity.owner()));
            let drawn_type = view.rules.and_then(|rules| sim.interner().object_type(type_ref, rules));
            let strength = drawn_type.map_or(0, |object| object.strength.max(0));
            let health = match worn {
                None => (entity.health.current(), strength),
                Some(_) => {
                    let own = view
                        .rules
                        .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules))
                        .map_or(0, |object| object.strength);
                    let share = if own > 0 {
                        i64::from(entity.health.current()) * i64::from(strength) / i64::from(own)
                    } else {
                        0
                    };
                    (share.clamp(0, i64::from(strength)) as i32, strength)
                }
            };
            let own = view.player == Some(owner);
            // Both ways, as client reads share `Allies` variables.
            let allied = !own
                && view.player.is_some_and(|player| {
                    let (player, owner) =
                        (sim.interner().resolve(player), sim.interner().resolve(owner));
                    let alliances = sim.house_alliances().as_map();
                    crate::map::houses::is_allied_with(alliances, player, owner)
                        && crate::map::houses::is_allied_with(alliances, owner, player)
                });
            let real = worn.is_none();
            let status = entity.status();
            Some(StatusObject {
                id: entity.stable_id(),
                anchor: (anchor.0.round() as i32, anchor.1.round() as i32),
                type_id: type_ref.index(),
                category: drawn_type.map_or(entity.category(), |object| object.category.into()),
                own,
                allied,
                selected: entity.selection.is_selected(),
                hovered: view.hovered == Some(entity.stable_id()),
                health,
                shield: shield(entity, view.rules).filter(|_| real),
                ammo: status.and_then(|status| status.ammo).filter(|_| real),
                passengers: status.and_then(|status| status.passengers).filter(|_| real),
                rank: real.then(|| i32::from(entity.veterancy.rank().min(2))),
                vars: if real {
                    status.and_then(|status| status.vars.as_deref()).unwrap_or_default()
                } else {
                    &[]
                },
            })
        })
        .collect()
}

/// A Phobos shield's points and its type's `Strength=`.
fn shield(
    entity: &crate::app::presentation::frame::ObjectRecord,
    rules: Option<&RuleSet>,
) -> Option<(i32, i32)> {
    #[cfg(feature = "phobos")]
    {
        let shield = entity.shield().filter(|shield| shield.is_active())?;
        let strength = rules?.shield_type(shield.type_name())?.strength();
        Some((shield.hp(), strength))
    }
    #[cfg(not(feature = "phobos"))]
    {
        let _ = (entity, rules);
        None
    }
}

/// Zoom as the view gives it, 1.0 where it gives none.
fn zoom_of(view: &FieldView<'_>) -> f32 {
    if view.viewport.zoom.is_finite() && view.viewport.zoom > 0.0 {
        view.viewport.zoom
    } else {
        1.0
    }
}

/// A seen object's anchor ([`ui_overlays::module_status_anchor`]).
fn object_anchor(view: &FieldView<'_>, id: u64) -> Option<((f32, f32), (u16, u16))> {
    let entity = view.frame.object(id)?;
    let point = ui_overlays::module_status_anchor(
        view.sim,
        entity,
        view.rules,
        view.player,
        view.viewport.ignore_visibility,
    )?;
    Some((point, (entity.position.rx(), entity.position.ry())))
}

/// The world-pixel point of a cell's centre on the ground, and of a point
/// `(dx, dy)` leptons from it at the same height.
fn ground_point(view: &FieldView<'_>, (x, y): (u16, u16), (dx, dy): (i32, i32)) -> (f32, f32) {
    let level = view
        .sim
        .resolved_terrain()
        .and_then(|terrain| terrain.cell(x, y))
        .map_or(0, |cell| i32::from(cell.level));
    crate::util::lepton::absolute_leptons_to_screen(
        i32::from(x) * 256 + 128 + dx,
        i32::from(y) * 256 + 128 + dy,
        level * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
    )
}

/// A cell reference's cell (kind 2, `x << 16 | y`).
fn cell_of(reference: i64) -> Option<(u16, u16)> {
    (reference >> 48 == 2).then(|| ((reference >> 16) as u16, reference as u16))
}

fn attached_instances(
    ui: &UiModules,
    view: &FieldView<'_>,
    font: &BitFont,
    attached: &[(bool, super::tree::Attached)],
    out: &mut FieldInstances,
) {
    let zoom = zoom_of(view);
    let camera = view.viewport.camera;
    let size = view.viewport.size;
    let shown = |(x, y): (f32, f32)| {
        x >= camera[0] - MARGIN
            && y >= camera[1] - MARGIN
            && x <= camera[0] + size[0] / zoom + MARGIN
            && y <= camera[1] + size[1] / zoom + MARGIN
    };
    let dot = |out: &mut FieldInstances, (x, y): (f32, f32), side: f32, tint: [f32; 3]| {
        if shown((x, y)) {
            out.fills.push(quad(
                [x - side / 2.0, y - side / 2.0],
                [side, side],
                ([0.0, 0.0], [1.0, 1.0]),
                tint,
                1.0,
                0.0,
            ));
        }
    };
    // Bound total raster work for the layer. Each sample can emit a full
    // SpriteInstance, and every marker may supply up to 256 segments.
    let mut samples_left = 65_536;
    for (decoration, item) in attached {
        let tint = item.tint.map_or([1.0; 3], |rgb| {
            [
                ((rgb >> 16) & 0xFF) as f32 / 255.0,
                ((rgb >> 8) & 0xFF) as f32 / 255.0,
                (rgb & 0xFF) as f32 / 255.0,
            ]
        });
        let object = u64::try_from(item.anchor)
            .ok()
            .filter(|&id| id != 0 && item.anchor >> 46 == 0);
        let anchor = match (decoration, object, cell_of(item.anchor)) {
            (_, Some(id), _) => object_anchor(view, id),
            (false, None, Some(cell)) => Some((ground_point(view, cell, (0, 0)), cell)),
            _ => None,
        };
        let Some((anchor, cell)) = anchor else {
            continue;
        };
        if !decoration {
            // Hold 2: the radar alone.
            if item.place == 2 {
                continue;
            }
            dot(out, anchor, 3.0 / zoom, tint);
            let mut last = anchor;
            for &cell in &item.cells {
                if samples_left == 0 {
                    break;
                }
                let next = ground_point(view, cell, (0, 0));
                line(last, next, zoom, &mut samples_left, |point| dot(out, point, 1.0 / zoom, tint));
                last = next;
            }
            if item.radius > 0 && samples_left != 0 {
                circle(view, cell, item.radius, zoom, &mut samples_left, |point| {
                    dot(out, point, 1.0 / zoom, tint)
                });
            }
        }
        if !shown(anchor) {
            continue;
        }
        let at = [anchor.0 + item.x as f32 / zoom, anchor.1 + item.y as f32 / zoom];
        let mut text_y = at[1];
        if !item.art.is_empty()
            && let Some([up, _]) = ui.art.art().frames(&item.art)
        {
            out.pictures.push(quad(
                at,
                [up.pixel_size[0] / zoom, up.pixel_size[1] / zoom],
                (up.uv_origin, up.uv_size),
                [1.0; 3],
                1.0,
                0.0,
            ));
            text_y += up.pixel_size[1] / zoom;
        }
        let mut text = item.number.map(|number| number.to_string()).unwrap_or_default();
        if !item.label.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&item.label);
        }
        if !text.is_empty() {
            out.text
                .extend(font.build_text(&text, at[0], text_y, 1.0 / zoom, 0.0, tint, [0.0; 2]));
        }
    }
}

/// One point a screen pixel apart along `from..to` (world pixels).
fn line(from: (f32, f32), to: (f32, f32), zoom: f32, samples_left: &mut usize, mut point: impl FnMut((f32, f32))) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let steps = ((dx.abs().max(dy.abs()) * zoom).ceil() as usize).clamp(1, 4096);
    let count = (steps + 1).min(*samples_left);
    *samples_left -= count;
    for step in 0..count {
        let t = step as f32 / steps as f32;
        point((from.0 + dx * t, from.1 + dy * t));
    }
}

/// A ground circle of `radius` leptons round a cell's centre, as points.
fn circle(
    view: &FieldView<'_>,
    cell: (u16, u16),
    radius: i32,
    zoom: f32,
    samples_left: &mut usize,
    mut point: impl FnMut((f32, f32)),
) {
    // Its projected circumference in screen pixels bounds the points.
    let around = (std::f32::consts::TAU * radius as f32 / 256.0 * 30.0 * zoom).ceil();
    let count = (around as usize).clamp(16, 4096);
    let samples = count.min(*samples_left);
    *samples_left -= samples;
    for index in 0..samples {
        let angle = std::f32::consts::TAU * index as f32 / count as f32;
        let offset = (
            (angle.cos() * radius as f32) as i32,
            (angle.sin() * radius as f32) as i32,
        );
        point(ground_point(view, cell, offset));
    }
}

/// The markers' radar dots: each marker held for the radar (`Hold` 0 or 2)
/// at its cell and at its line's cells, in its tint.
pub(crate) fn radar_markers(
    ui: &UiModules,
    object_cell: impl Fn(u64) -> Option<(u16, u16)>,
) -> Vec<((u16, u16), [f32; 3])> {
    let mut dots = Vec::new();
    for (decoration, item) in ui.tree.attached() {
        if decoration || item.place == 1 {
            continue;
        }
        let cell = cell_of(item.anchor).or_else(|| {
            let id = u64::try_from(item.anchor).ok()?;
            (id != 0 && id >> 46 == 0).then(|| object_cell(id)).flatten()
        });
        let Some(cell) = cell else {
            continue;
        };
        let tint = item.tint.map_or([1.0; 3], |rgb| {
            [
                ((rgb >> 16) & 0xFF) as f32 / 255.0,
                ((rgb >> 8) & 0xFF) as f32 / 255.0,
                (rgb & 0xFF) as f32 / 255.0,
            ]
        });
        dots.push((cell, tint));
        dots.extend(item.cells.iter().map(|&cell| (cell, tint)));
    }
    dots
}
