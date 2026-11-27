//! What the mod's and the map's additions draw over the battlefield
//! (VERA, RFC 0001 section 10): the status templates ([`super::status`]) at
//! every object the local player sees. Built with the world's instances
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
    if ui.status.is_empty() {
        return out;
    }
    let zoom = if view.viewport.zoom.is_finite() && view.viewport.zoom > 0.0 {
        view.viewport.zoom
    } else {
        1.0
    };
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
            let object = view
                .rules
                .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules));
            let strength = object.map_or(0, |object| object.strength);
            let owner = entity.owner();
            let own = view.player == Some(owner);
            let allied = !own
                && view.player.is_some_and(|player| {
                    crate::map::houses::is_allied_with(
                        sim.house_alliances().as_map(),
                        sim.interner().resolve(player),
                        sim.interner().resolve(owner),
                    )
                });
            Some(StatusObject {
                id: entity.stable_id(),
                anchor: (anchor.0.round() as i32, anchor.1.round() as i32),
                type_id: entity.type_ref().index(),
                category: entity.category(),
                own,
                allied,
                selected: entity.selection.is_selected(),
                hovered: view.hovered == Some(entity.stable_id()),
                health: (entity.health.current(), strength),
                shield: shield(entity, view.rules),
                ammo: entity.status.ammo,
                passengers: entity.status.passengers,
                rank: i32::from(entity.veterancy.rank().min(2)),
                vars: entity.status.vars.as_deref().unwrap_or_default(),
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
