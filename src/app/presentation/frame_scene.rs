//! What the scene's drawers other than the object bodies read of the
//! world's objects, captured into the presentation frame
//! (`presentation::frame`): projectiles, anims, debris, fogged buildings,
//! bridges, the radar, the sidebar and the other world overlays.

use crate::app::presentation::frame::Capture;
use std::collections::{HashMap, HashSet};

use crate::sim::game_entity::GameEntity;

/// One object's part.
#[derive(Clone, Debug, Default)]
pub(crate) struct SceneFacts {
    /// What the radar reads of it (`render::radar`).
    pub(crate) radar: crate::render::radar::RadarFacts,
}

impl SceneFacts {
    pub(crate) fn capture(entity: &GameEntity, at: &Capture<'_>) -> Self {
        Self {
            radar: crate::render::radar::RadarFacts::capture(
                entity,
                at.sim,
                at.rules(),
                at.cells.as_ref(),
                at.binary_frame,
                at.observer_id,
            ),
        }
    }
}

/// The frame's part that belongs to no one object.
#[derive(Clone, Debug, Default)]
pub(crate) struct SceneFrame {
    /// What the radar reads beside its objects (`render::radar`).
    pub(crate) radar: crate::render::radar::RadarFrame,
    /// What the sidebar, the house UI and the sounds read of the objects
    /// (`presentation::sidebar_render`).
    pub(crate) ui: crate::app::presentation::sidebar_render::UiFrame,
    /// Each drawn building spotlight's draw inputs, by its id
    /// (`Simulation::building_spotlight` for the local player), for the
    /// Display members that are spotlights.
    pub(crate) spotlights: HashMap<u64, crate::sim::world::BuildingSpotlight>,
    /// The PsiWarning anims the local player detects
    /// (`Simulation::psi_warning_detected_by`), whose hidden byte
    /// `AnimClass::AI` clears.
    pub(crate) psi_warnings_detected: HashSet<u64>,
    /// The planning graph the current house draws, without a hovered or
    /// highlighted node (`render::planning_lines`).
    pub(crate) planning: Option<PlanningGraph>,
    /// Each control group's members, with the queued assignments
    /// (`Simulation::projected_control_groups`), for the group digits
    /// (`ui_overlays`).
    pub(crate) control_groups: [Vec<u64>; 10],
}

/// `Simulation::planning_draw_graph` for `viewer`, with no hovered or
/// highlighted node: the draw takes those, local input, from its nodes.
#[derive(Clone, Debug)]
pub(crate) struct PlanningGraph {
    pub(crate) viewer: crate::sim::intern::InternedId,
    pub(crate) graph: crate::sim::world::PlanningDrawGraph,
}

impl SceneFrame {
    pub(crate) fn capture(at: &Capture<'_>) -> Self {
        Self {
            radar: crate::render::radar::RadarFrame::default(),
            ui: crate::app::presentation::sidebar_render::UiFrame::capture(at),
            spotlights: spotlights(at),
            psi_warnings_detected: psi_warnings_detected(at),
            control_groups: at.sim.projected_control_groups(),
            planning: at
                .sim
                .session()
                .current_house()
                .map(|viewer| PlanningGraph {
                    viewer,
                    graph: at.sim.planning_draw_graph(viewer, None, None),
                }),
        }
    }
}

/// The spotlights among the Display members, with what drawing reads of
/// each: every object store shares one id namespace, and
/// `Simulation::building_spotlight` answers only a spotlight's id.
fn spotlights(at: &Capture<'_>) -> HashMap<u64, crate::sim::world::BuildingSpotlight> {
    let sim = at.sim;
    sim.display_layers()
        .ordered_ids()
        .filter(|&&id| !sim.entities().contains(id) && sim.anim(id).is_none())
        .filter_map(|&id| Some((id, sim.building_spotlight(id, at.observer_id, at.rules())?)))
        .collect()
}

/// The PsiWarning anims (`AnimType.PsiWarning`) the local player detects.
fn psi_warnings_detected(at: &Capture<'_>) -> HashSet<u64> {
    let sim = at.sim;
    let rules = at.rules();
    let Some(viewer) = at.observer_id else {
        return HashSet::new();
    };
    sim.anims()
        .filter(|(_, anim)| {
            rules
                .art()
                .anim_runtime_config(sim.interner().resolve(anim.type_id()))
                .is_some_and(|config| config.psi_warning)
        })
        .filter(|(_, anim)| sim.psi_warning_detected_by(viewer, anim, rules))
        .map(|(&id, _)| id)
        .collect()
}
