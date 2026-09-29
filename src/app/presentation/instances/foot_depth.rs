//! Read-only production adapter for FootClass draw depth.
//! Native identities and pure arithmetic live in render::foot_depth. All unit
//! pieces consume one result; camera visibility never mutates the sim's dummy.

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::foot_depth::{DepthCell, FootDepthCoefficients, FootDepthContext};
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::overlay_grid::OverlayGrid;
use crate::util::native_x87::adjust_for_z_standard;

/// Exact fixed-slot aliases and fallback values, without GetCell's simulation
/// dummy stamp. The pure evaluator retains dummy identity/coordinates locally.
pub(crate) fn depth_cell(
    terrain: &ResolvedTerrainGrid,
    overlays: Option<&OverlayGrid>,
    registry: Option<&OverlayTypeRegistry>,
    coord: [i16; 2],
) -> DepthCell {
    let rock = |id: Option<u8>| {
        id.map(|id| {
            registry
                .and_then(|r| r.flags(id))
                .is_some_and(|f| f.is_a_rock)
        })
    };
    if let Some(index) = terrain.native_fixed_cell_index(coord[0], coord[1]) {
        let cell = &terrain.cells()[index];
        let overlay_id = overlays.map_or(cell.bridge_facts.overlay_id, |o| {
            o.cell(cell.rx, cell.ry).overlay_id
        });
        DepthCell {
            coord: [cell.rx as i16, cell.ry as i16],
            level: cell.level as i8,
            ramp: cell.slope_type,
            flags: cell.bridge_facts.raw_flags,
            iso_tile_index: cell.final_tile_index,
            low_bridge: cell.yr_cell_land_type == 10
                && terrain.tube_at_cell(cell.rx, cell.ry).is_some(),
            overlay_rock: rock(overlay_id),
            tmp_height: terrain.native_tmp_draw_height(cell.final_tile_index, cell.final_sub_tile),
            is_dummy: false,
        }
    } else {
        let dummy = terrain.shared_cell_dummy();
        let snapshot = dummy.snapshot();
        DepthCell {
            coord,
            level: snapshot.level,
            ramp: snapshot.slope_type,
            flags: snapshot.bridge_flags_0x1180,
            overlay_rock: rock(dummy.overlay_fields().0),
            tmp_height: terrain.native_tmp_draw_height(0xffff, 0),
            is_dummy: true,
            ..Default::default()
        }
    }
}

fn coefficients(object: Option<&ObjectType>) -> FootDepthCoefficients {
    object.map_or_else(FootDepthCoefficients::default, |o| FootDepthCoefficients {
        cliff: o.zfudge_cliff,
        column: o.zfudge_column,
        tunnel: o.zfudge_tunnel,
        bridge: o.zfudge_bridge,
    })
}

fn world_xy(entity: &GameEntity) -> [i32; 2] {
    [
        i32::from(entity.position.rx)
            .wrapping_mul(256)
            .wrapping_add(entity.position.sub_x.to_num::<i32>()),
        i32::from(entity.position.ry)
            .wrapping_mul(256)
            .wrapping_add(entity.position.sub_y.to_num::<i32>()),
    ]
}

/// Unit DrawExtras temporarily selects UnloadingClass (0x73D2C4). Art-only
/// camouflage/NoSpawnAlt substitutions are not native UnitType substitutions.
fn draw_type<'a>(
    runtime: &'a crate::sim::runtime::SimRuntime,
    entity: &GameEntity,
    unloading_body: bool,
) -> Option<&'a ObjectType> {
    let view = runtime.view();
    let actual = runtime
        .resources
        .rules
        .object(view.interner().resolve(entity.type_ref()));
    if unloading_body && actual.is_some_and(|o| o.harvester) {
        entity
            .display_type_override
            .and_then(|id| runtime.resources.rules.object(view.interner().resolve(id)))
            .or(actual)
    } else {
        actual
    }
}

/// Raw Unit composite predicate (0x73B1A8..0x73B207), not a draw-layer route.
/// SHP infantry and aircraft never pass through this Unit vtable slot +0x55C.
pub(crate) fn unit_bridge_split(
    state: &AppState,
    entity: &GameEntity,
    unloading_body: bool,
) -> bool {
    unit_bridge_split_in_runtime(
        state.match_state.sim_runtime.as_ref(),
        entity,
        unloading_body,
    )
}

fn unit_bridge_split_in_runtime(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    entity: &GameEntity,
    unloading_body: bool,
) -> bool {
    unit_bridge_condition_in_runtime(runtime, entity, unloading_body, true)
}

/// Direct SHP Unit no-turret branch 0x73CE0D..0x73CE7F (stock SQD).
/// It changes the whole body's a7 by -16 and a8 to gradient zero, with
/// neither the composite's factory alternative nor its height > 16 test.
pub(crate) fn shp_unit_bridge_fudge(state: &AppState, entity: &GameEntity) -> bool {
    shp_unit_bridge_fudge_in_runtime(state.match_state.sim_runtime.as_ref(), entity)
}

fn shp_unit_bridge_fudge_in_runtime(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    entity: &GameEntity,
) -> bool {
    if runtime
        .and_then(|rt| draw_type(rt, entity, true))
        .is_some_and(|object| object.has_turret)
    {
        // SHP+Turret uses the offscreen composite instead; that missing SHP
        // composition path is not replaced by the direct-body treatment.
        return false;
    }
    unit_bridge_condition_in_runtime(runtime, entity, true, false)
}

fn unit_bridge_condition_in_runtime(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    entity: &GameEntity,
    unloading_body: bool,
    allow_factory: bool,
) -> bool {
    if entity.category != EntityCategory::Unit {
        return false;
    }
    let Some(runtime) = runtime else {
        return false;
    };
    let Some(object) = draw_type(runtime, entity, unloading_body) else {
        return false;
    };
    if !object.too_big_to_fit_under_bridge {
        return false;
    }
    let view = runtime.view();
    let factory = allow_factory
        && entity.navigation.nav_com.is_some()
        && entity
            .radio_contacts
            .slot(0)
            .and_then(|id| view.entities().get(id))
            .filter(|contact| contact.category == EntityCategory::Structure)
            .and_then(|contact| {
                runtime
                    .resources
                    .rules
                    .object(view.interner().resolve(contact.type_ref()))
            })
            .is_some_and(|object| object.weapons_factory);
    let Some(terrain) = view.resolved_terrain() else {
        return factory;
    };
    let xy = world_xy(entity);
    crate::render::foot_depth::unit_composite_split(
        FootDepthContext {
            cell: [(xy[0] / 256) as i16, (xy[1] / 256) as i16],
            on_bridge: entity.on_bridge,
            bridge_set_base: terrain.concrete_bridge_set_base(),
            ..Default::default()
        },
        object.too_big_to_fit_under_bridge,
        factory,
        |coord| {
            depth_cell(
                terrain,
                view.overlay_grid(),
                Some(&runtime.resources.overlay_registry),
                coord,
            )
        },
    )
}

/// SHP's 0x705E00 gate is GetHeight()==0, not a display-layer test. Aircraft
/// always bypass Foot. Raw VXL and OREGATH callers use Foot directly instead.
pub(crate) fn shp_z_adjust(state: &AppState, entity: &GameEntity) -> f32 {
    shp_z_adjust_in_runtime(state.match_state.sim_runtime.as_ref(), entity)
}

fn shp_z_adjust_in_runtime(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    entity: &GameEntity,
) -> f32 {
    let world_z = crate::sim::movement::ground_pose::object_world_z_leptons(entity, None);
    let height_only = adjust_for_z_standard(world_z)
        .wrapping_neg()
        .wrapping_sub(2);
    if !matches!(
        entity.category,
        EntityCategory::Unit | EntityCategory::Infantry
    ) {
        return height_only as f32;
    }
    let Some(terrain) = runtime.and_then(|rt| rt.view().resolved_terrain()) else {
        return height_only as f32;
    };
    let xy = world_xy(entity);
    let cell = depth_cell(
        terrain,
        None,
        None,
        [(xy[0] / 256) as i16, (xy[1] / 256) as i16],
    );
    let Ok(ground) =
        crate::util::lepton::ground_height_leptons(cell.level as u8, cell.ramp, xy[0], xy[1])
    else {
        return height_only as f32;
    };
    let surface = ground.wrapping_add(if entity.on_bridge {
        crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32
    } else {
        0
    });
    // Ordinary ground movement supplies exact sampled ramp Z. Legacy or
    // independent-altitude inputs can still lack that coordinate; retain their
    // semantic altitude for the grounded gate. Exact coordinates use the full
    // native GetHeight subtraction without a second render-side terrain snap.
    let height_above_ground = if entity.position.exact_z_leptons.is_some() {
        world_z.wrapping_sub(surface)
    } else {
        world_z.wrapping_sub(
            i32::from(entity.position.z as i8)
                * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS as i32,
        )
    };
    if height_above_ground == 0 {
        unit_z_adjust_in_runtime(runtime, entity, true).wrapping_sub(2) as f32
    } else {
        height_only as f32
    }
}

/// Foot 0x4DAFC0 used by VXL body/shadow and OREGATH. `unloading_body` selects
/// only the proven Unit+6C4 temporary UnloadingClass substitution at 0x73D2C4;
/// camouflage/NoSpawnAlt art must not replace the depth coefficients.
pub(crate) fn unit_z_adjust(state: &AppState, entity: &GameEntity, unloading_body: bool) -> i32 {
    unit_z_adjust_in_runtime(
        state.match_state.sim_runtime.as_ref(),
        entity,
        unloading_body,
    )
}

fn unit_z_adjust_in_runtime(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    entity: &GameEntity,
    unloading_body: bool,
) -> i32 {
    let world_z = crate::sim::movement::ground_pose::object_world_z_leptons(entity, None);
    if !matches!(
        entity.category,
        EntityCategory::Unit | EntityCategory::Infantry
    ) {
        return adjust_for_z_standard(world_z).wrapping_neg();
    }
    let Some(runtime) = runtime else {
        return adjust_for_z_standard(world_z).wrapping_neg();
    };
    let view = runtime.view();
    let Some(terrain) = view.resolved_terrain() else {
        return adjust_for_z_standard(world_z).wrapping_neg();
    };
    let sim = &runtime.simulation;
    let object = draw_type(runtime, entity, unloading_body);
    let xy = world_xy(entity);
    // ObjectClass cell getter 0x41BEA0 truncates signed coordinates toward zero.
    let coord = [(xy[0] / 256) as i16, (xy[1] / 256) as i16];
    let special = if entity.category == EntityCategory::Unit {
        let contact = entity
            .dock_entered_with
            .and_then(|_| entity.radio_contacts.slot(0))
            .and_then(|id| view.entities().get(id));
        if contact.is_some_and(|other| {
            other.category == EntityCategory::Structure && other.mission.effective().raw() == 16
        }) {
            Some(-3)
        } else if object.is_some_and(|o| o.harvester) {
            let cell = depth_cell(terrain, None, None, coord);
            sim.substrate
                .occupancy
                .first_building_on_layer(
                    cell.coord[0] as u16,
                    cell.coord[1] as u16,
                    MovementLayer::Ground,
                )
                .and_then(|id| view.entities().get(id))
                .and_then(|building| {
                    runtime
                        .resources
                        .rules
                        .object(view.interner().resolve(building.type_ref()))
                })
                .filter(|o| o.refinery)
                .map(|_| -14)
        } else {
            None
        }
    } else {
        None
    };
    let context = FootDepthContext {
        cell: coord,
        on_bridge: entity.on_bridge,
        facing_u16: entity.body_facing_current(view.session().binary_frame),
        world_z_leptons: world_z,
        // Drive 0x4B4870, Ship 0x6A3EA0, and the shared 0x55ABA0 inherited
        // by Walk/Hover/Fly/Jumpjet/Teleport all return zero at loco slot +0x38.
        locomotor_z: 0,
        bridge_set_base: terrain.concrete_bridge_set_base(),
        unit_special_adjustment: special,
    };
    crate::render::foot_depth::foot_z_adjust(context, coefficients(object), |requested| {
        depth_cell(
            terrain,
            view.overlay_grid(),
            Some(&runtime.resources.overlay_registry),
            requested,
        )
    })
}

#[cfg(test)]
#[path = "foot_depth_tests.rs"]
mod tests;
