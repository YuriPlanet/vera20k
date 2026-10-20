//! Optional window-space unit information. Display membership, fog, discovery,
//! cloak and disguise continue to use their existing simulation/render owners.

use super::AppState;
use super::input::commands::preferred_local_owner_name;
use super::presentation::{instances, ui_overlays};
use crate::assets::csf_file::type_ui_name;
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::util::lepton::absolute_leptons_to_screen;

pub(super) fn build(state: &AppState) -> Vec<SpriteInstance> {
    if !state.match_state.input.ios_controls.show_tags || !super::ios_controls::enabled(state) {
        return Vec::new();
    }
    let (Some(runtime), Some(rules), Some(local_owner)) = (
        state.match_state.sim_runtime.as_ref(),
        state.rules(),
        preferred_local_owner_name(state),
    ) else {
        return Vec::new();
    };
    let sim = &runtime.simulation;
    let local_owner_id = sim.interner().get(&local_owner);
    let layout = super::ios_controls::layout(state);
    let world = layout.touch.world;
    let (tactical_width, tactical_height) = super::input::camera::tactical_viewport_size(state);
    let tactical_zoom = state.tactical_zoom();
    let to_window = |x, y| tactical_zoom.map_or((x, y), |zoom| zoom.source_to_window(x, y));
    let viewport_start = to_window(0.0, 0.0);
    let viewport_end = to_window(tactical_width as f32, tactical_height as f32);
    let clip = [
        world.x.max(viewport_start.0),
        world.y.max(viewport_start.1),
        (world.x + world.width).min(viewport_end.0),
        (world.y + world.height).min(viewport_end.1),
    ];
    if clip[2] <= clip[0] || clip[3] <= clip[1] {
        return Vec::new();
    }
    let font = &state.renderer.bit_font;
    let scale = layout.touch.scale_factor;
    let text_scale = 11.0 * scale / font.glyph_height().max(1.0);
    let shadow = scale;
    let height = font.glyph_height() * text_scale + shadow;
    let input = &state.match_state.input;
    let mut labels = Vec::new();
    for entity in sim.entities().values() {
        let coord = object_get_coords(entity, sim.resolved_terrain());
        let projected = absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        let source = (
            (projected.0 - input.camera_x) * input.zoom_level,
            (projected.1 - input.camera_y) * input.zoom_level,
        );
        let anchor = to_window(source.0, source.1);
        if anchor.0 < clip[0] || anchor.0 >= clip[2] || anchor.1 < clip[1] || anchor.1 >= clip[3] {
            continue;
        }
        // Body admission deliberately delegates shroud clipping to ABuffer.
        // This window-space label has no shroud mask, so also ask the existing
        // anchor-visibility owner and require the retained discovery byte.
        if !entity.discovery.discovered_by_current_house()
            || !instances::is_entity_visible_for_local_owner(
                Some(&local_owner),
                sim.fog(),
                sim.house_alliances(),
                &entity.position,
                sim.interner().resolve(entity.owner()),
                state.match_state.sandbox_full_visibility,
                local_owner_id,
            )
            || entity
                .cloak
                .as_ref()
                .is_some_and(|cloak| cloak.is_fully_cloaked())
        {
            continue;
        }
        let Some(observer) =
            ui_overlays::admitted_techno_observer(sim, entity, Some(&local_owner), Some(rules))
        else {
            continue;
        };
        if observer.invisible {
            continue;
        }
        // A label must not identify a disguised Spy/Mirage by its real type.
        // The shared body query decides when the viewer sees the disguise.
        let type_ref = if DrawState::draws_disguise(entity, sim.session().binary_frame(), observer)
        {
            entity
                .disguise
                .as_ref()
                .and_then(|disguise| disguise.type_id())
        } else {
            Some(entity.type_ref())
        };
        let Some(object) = type_ref.and_then(|id| sim.interner().object_type(id, rules)) else {
            continue;
        };
        let name = type_ui_name(object.ui_name.as_deref(), state.process_assets.csf.as_ref());
        if name.is_empty() {
            continue;
        }
        // Cost is the type's base Rules price, not a promise of the current
        // owner's final production price after side/house modifiers.
        let text = format!("{name} ${}", object.cost);
        let width = font.text_width(&text) as f32 * text_scale + shadow;
        if width > clip[2] - clip[0] || height > clip[3] - clip[1] {
            continue;
        }
        let x = (anchor.0 - width / 2.0).clamp(clip[0], clip[2] - width);
        let y = (anchor.1 - height - 14.0 * scale).clamp(clip[1], clip[3] - height);
        labels.extend(font.build_text(
            &text,
            x + shadow,
            y + shadow,
            text_scale,
            0.0,
            [0.0; 3],
            [0.0; 2],
        ));
        labels.extend(font.build_text(&text, x, y, text_scale, 0.0, [0.96, 0.94, 0.82], [0.0; 2]));
    }
    labels
}
