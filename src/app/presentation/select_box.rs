//! Phobos `TechnoExt::DrawSelectBox` decisions and the after-body sprite.
//!
//! Source: `Ext/Techno/Body.Visuals.cpp:333-409`, called by the
//! `TechnoClass_DrawHealthBar` hooks `0x6F65D1`/`0x6F683C` and the
//! RenderLayers before-body hooks `0x6D9076`/`0x6D9134`
//! (`Ext/Techno/Hooks.Pips.cpp:3-20,74-114`). The client switch is
//! `[Phobos] EnableSelectBox=` (`Phobos.INI.cpp:104`).
//!
//! The sprite rides the existing status/Extras material: its texture is the
//! `SelectionOverlay`'s loaded `SelectBoxType` SHP, like the DigitalDisplay
//! custom shapes. `DrawAboveTechno=false` belongs to the before-body pass, which
//! has no custom-SHP slot in this engine (residual).

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::select_box_type::SelectBoxType;
use crate::sim::game_entity::GameEntity;
use crate::util::native_x87::MaskedX87Ordering;

/// `DrawSelectBox`'s health-band choice (`Body.Visuals.cpp:356,376,399`):
/// `healthPercentage > ConditionYellow ? X : healthPercentage > ConditionRed ?
/// Y : Z`, with `(1,1,1)` for infantry and `(0,0,0)` for others when
/// `Frames=`/`GroundFrames=` is unset.
pub(crate) fn select_box_frame(
    select_box: &SelectBoxType,
    is_infantry: bool,
    above_yellow: bool,
    above_red: bool,
) -> usize {
    let frames = select_box
        .frames
        .unwrap_or(if is_infantry { [1, 1, 1] } else { [0, 0, 0] });
    let frame = if above_yellow {
        frames[0]
    } else if above_red {
        frames[1]
    } else {
        frames[2]
    };
    usize::try_from(frame).unwrap_or(0)
}

/// The same choice over `GroundFrames=`.
pub(crate) fn select_box_ground_frame(
    select_box: &SelectBoxType,
    is_infantry: bool,
    above_yellow: bool,
    above_red: bool,
) -> usize {
    let frames =
        select_box
            .ground_frames
            .unwrap_or(if is_infantry { [1, 1, 1] } else { [0, 0, 0] });
    let frame = if above_yellow {
        frames[0]
    } else if above_red {
        frames[1]
    } else {
        frames[2]
    };
    usize::try_from(frame).unwrap_or(0)
}

/// `drawPoint = *pLocation + (isInfantry ? {8,-3} : {1,-4}) + Offset`, then
/// `drawPoint.Y += PixelSelectionBracketDelta` when `DrawAboveTechno`
/// (`Body.Visuals.cpp:401-405`).
pub(crate) fn select_box_draw_point(
    point: [f32; 2],
    select_box: &SelectBoxType,
    is_infantry: bool,
    bracket_delta: i32,
) -> [f32; 2] {
    let base = if is_infantry { [8, -3] } else { [1, -4] };
    let x = point[0] + (base[0] + select_box.offset[0]) as f32;
    let y = point[1] + (base[1] + select_box.offset[1]) as f32;
    let y = if select_box.draw_above_techno {
        y + bracket_delta as f32
    } else {
        y
    };
    [x, y]
}

/// `DrawSelectBox`'s house admission (`Body.Visuals.cpp:350-353`): the
/// observer reads `VisibleToHouses.Observer`, everyone else the
/// `CanTargetHouse` answer against `VisibleToHouses`.
pub(crate) fn select_box_visible(
    select_box: &SelectBoxType,
    observer: bool,
    can_target_house: bool,
) -> bool {
    if observer {
        select_box.visible_to_houses_observer
    } else {
        can_target_house
    }
}

/// The two sprites one selected techno's `SelectBoxType` may add: the box
/// above the body and the ground shape under it.
#[cfg(feature = "phobos")]
#[derive(Default)]
pub(crate) struct SelectBoxSprites {
    pub above: Option<(u32, crate::render::batch::SpriteInstance)>,
    pub ground: Option<(u32, crate::render::batch::SpriteInstance)>,
}

/// The after-body box and ground sprites (`Body.Visuals.cpp:363-408`). Empty
/// for an unselected/hidden object, an unresolved type or a failed visibility
/// test. The ground arm needs a `GroundShape=` and a non-building for which
/// `Ground.AlwaysDraw` or an airborne body admits it.
#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn select_box_sprites(
    sim: &crate::sim::world::Simulation,
    entity: &GameEntity,
    rules: &RuleSet,
    overlay: &crate::render::selection_overlay::SelectionOverlay,
    local_owner: Option<&str>,
    viewport: crate::app::presentation::ui_overlays::StatusViewport,
) -> SelectBoxSprites {
    let mut sprites = SelectBoxSprites::default();
    if !entity.selection.is_selected()
        || entity.sinking.is_active()
        || entity.passenger_role.is_inside_transport()
    {
        return sprites;
    }
    let Some(object) = sim.interner().object_type(entity.type_ref(), rules) else {
        return sprites;
    };
    if object.phobos_techno_extras.hide_select_box {
        return sprites;
    }
    let Some(index) = rules.select_box_index_for(object) else {
        return sprites;
    };
    let Some(select_box) = rules.select_box_types().types().get(index) else {
        return sprites;
    };
    let viewer = local_owner.and_then(|name| sim.interner().get(name));
    let observer = viewer.is_none();
    let can_target = viewer.is_some_and(|viewer| {
        crate::sim::combat::can_target_house(
            select_box.visible_to_houses,
            entity.owner(),
            viewer,
            sim.houses(),
            sim.house_alliances(),
            sim.interner(),
        )
    });
    if !select_box_visible(select_box, observer, can_target) {
        return sprites;
    }
    let above_yellow = entity
        .health
        .compare_ratio(object.strength, rules.general.condition_yellow)
        == MaskedX87Ordering::Greater;
    let above_red = entity
        .health
        .compare_ratio(object.strength, rules.general.condition_red)
        == MaskedX87Ordering::Greater;
    let is_infantry = object.category == ObjectCategory::Infantry;
    let is_building = object.category == ObjectCategory::Building;
    let coord =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain());
    let (sx, sy) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    let visible = |instance: &crate::render::batch::SpriteInstance| {
        crate::app::presentation::instances::in_view(
            instance.position[0],
            instance.position[1],
            instance.size[0],
            instance.size[1],
            viewport.camera[0],
            viewport.camera[1],
            viewport.size[0],
            viewport.size[1],
            48.0,
        )
    };
    let catalog_index = u32::try_from(index).unwrap_or(u32::MAX);
    // `DrawAboveTechno=false` draws in the RenderLayers before-body hook
    // (`Hooks.Pips.cpp:3-20`); this engine's before slot carries surface lines
    // only, so that arm is a recorded residual.
    if select_box.draw_above_techno
        && let Some(shape) = overlay.select_box_shape(index)
    {
        let frame = select_box_frame(select_box, is_infantry, above_yellow, above_red);
        let point = select_box_draw_point(
            [sx, sy],
            select_box,
            is_infantry,
            object.pixel_selection_bracket_delta,
        );
        if let Some(mut instance) = shape.instance(frame, (point[0], point[1]), 0.0) {
            instance.alpha = select_box.translucency_alpha();
            if visible(&instance) {
                sprites.above = Some((catalog_index, instance));
            }
        }
    }
    let airborne =
        crate::sim::movement::air_movement::current_fly_height(entity, sim.resolved_terrain()) > 0;
    if !is_building
        && (select_box.ground_always_draw || airborne)
        && let Some(shape) = overlay.select_box_ground_shape(index)
    {
        let frame = select_box_ground_frame(select_box, is_infantry, above_yellow, above_red);
        let point = [
            sx + select_box.ground_offset[0] as f32,
            sy + select_box.ground_offset[1] as f32,
        ];
        if let Some(mut instance) = shape.instance(frame, (point[0], point[1]), 0.0) {
            instance.alpha = select_box.translucency_alpha();
            if visible(&instance) {
                sprites.ground = Some((catalog_index, instance));
            }
        }
    }
    sprites
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    fn boxed(frames: Option<[i32; 3]>, draw_above: bool, offset: [i32; 2]) -> SelectBoxType {
        SelectBoxType {
            name: "TEST".to_owned(),
            shape: None,
            palette: None,
            frames,
            offset,
            translucency: 0,
            visible_to_houses: crate::rules::phobos_shields::AffectedHouse::ALL,
            visible_to_houses_observer: true,
            draw_above_techno: draw_above,
            ground_shape: None,
            ground_palette: None,
            ground_frames: None,
            ground_offset: [0, 0],
            ground_always_draw: true,
            ground_line: false,
            ground_line_color: crate::rules::ini_value::PhobosDamageableColor {
                base: [0, 255, 0],
                condition_yellow: None,
                condition_red: None,
            },
            ground_line_dashed: false,
        }
    }

    #[test]
    fn health_bands_choose_the_frame_with_strict_greater_than() {
        let select_box = boxed(Some([5, 6, 7]), true, [0, 0]);
        assert_eq!(select_box_frame(&select_box, false, true, true), 5);
        // Equal to yellow is not "above" it: the native test is `>`.
        assert_eq!(select_box_frame(&select_box, false, false, true), 6);
        assert_eq!(select_box_frame(&select_box, false, false, false), 7);
    }

    #[test]
    fn an_unset_frames_uses_the_infantry_or_other_default() {
        let select_box = boxed(None, true, [0, 0]);
        assert_eq!(select_box_frame(&select_box, true, false, false), 1);
        assert_eq!(select_box_frame(&select_box, false, false, false), 0);
    }

    #[test]
    fn the_draw_point_adds_the_class_offset_then_the_bracket_delta() {
        let select_box = boxed(None, true, [2, 3]);
        assert_eq!(
            select_box_draw_point([100.0, 50.0], &select_box, false, -10),
            [103.0, 39.0]
        );
        assert_eq!(
            select_box_draw_point([100.0, 50.0], &select_box, true, 0),
            [110.0, 50.0]
        );
        let below = boxed(None, false, [2, 3]);
        assert_eq!(
            select_box_draw_point([100.0, 50.0], &below, false, -10),
            [103.0, 49.0],
            "a below-techno box ignores the bracket delta"
        );
    }

    #[test]
    fn visibility_reads_the_observer_switch_or_the_house_ladder() {
        let mut select_box = boxed(None, true, [0, 0]);
        select_box.visible_to_houses_observer = false;
        assert!(!select_box_visible(&select_box, true, true));
        assert!(select_box_visible(&select_box, false, true));
        assert!(!select_box_visible(&select_box, false, false));
        select_box.visible_to_houses_observer = true;
        assert!(select_box_visible(&select_box, true, false));
    }

    #[test]
    fn translucency_maps_only_the_three_native_levels() {
        for (raw, alpha) in [(0, 1.0), (25, 0.75), (50, 0.5), (75, 0.25), (30, 1.0)] {
            let mut select_box = boxed(None, true, [0, 0]);
            select_box.translucency = raw;
            assert_eq!(select_box.translucency_alpha(), alpha, "{raw}");
        }
    }
}
