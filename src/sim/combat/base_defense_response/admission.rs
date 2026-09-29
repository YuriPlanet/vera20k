//! Destination, reachability and exact-5 fire-admission helpers.

use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::world::Simulation;
use crate::util::lepton::ground_height_leptons;

use super::super::TargetKind;
use super::super::combat_weapon::is_armed;
use super::super::fire_error::FireError;
use super::super::fire_error_world::FireSubject;
use super::ExistingTargetDisposition;
use crate::sim::movement::ground_pose::object_world_z_leptons;

pub(super) fn entity_coord(entity: &GameEntity, terrain: Option<&ResolvedTerrainGrid>) -> [i32; 3] {
    [
        i32::from(entity.position.rx as i16)
            .wrapping_mul(256)
            .wrapping_add(entity.position.sub_x.to_num::<i32>()),
        i32::from(entity.position.ry as i16)
            .wrapping_mul(256)
            .wrapping_add(entity.position.sub_y.to_num::<i32>()),
        object_world_z_leptons(entity, terrain),
    ]
}

fn nav_target_coord(
    target: crate::sim::components::NavTargetRef,
    entities: &EntityStore,
    terrain: Option<&ResolvedTerrainGrid>,
) -> Option<[i32; 3]> {
    use crate::sim::components::NavTargetRef;
    match target {
        NavTargetRef::Cell { rx, ry } => Some([
            i32::from(rx as i16).wrapping_mul(256).wrapping_add(128),
            i32::from(ry as i16).wrapping_mul(256).wrapping_add(128),
            0,
        ]),
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => {
            entities.get(id).map(|entity| entity_coord(entity, terrain))
        }
    }
}

fn destination_coord(
    entity: &GameEntity,
    entities: &EntityStore,
    terrain: Option<&ResolvedTerrainGrid>,
) -> [i32; 3] {
    if let Some(nav_com) = entity.navigation.nav_com
        && let Some(coord) = nav_target_coord(nav_com, entities, terrain)
    {
        return coord;
    }
    if let (Some(tube_state), Some(terrain)) = (entity.low_bridge_tube_state, terrain)
        && let Some(tube) = terrain.tube(tube_state.tube_id)
    {
        return [
            i32::from(tube.exit.0 as i16)
                .wrapping_mul(256)
                .wrapping_add(128),
            i32::from(tube.exit.1 as i16)
                .wrapping_mul(256)
                .wrapping_add(128),
            0,
        ];
    }
    entity_coord(entity, terrain)
}

fn lepton_to_cell_component(value: i32) -> i32 {
    value.wrapping_add((value >> 31) & 255) >> 8
}

pub(super) fn destination_cell(
    entity: &GameEntity,
    entities: &EntityStore,
    terrain: Option<&ResolvedTerrainGrid>,
) -> (i32, i32) {
    let coord = destination_coord(entity, entities, terrain);
    (
        i32::from(lepton_to_cell_component(coord[0]) as i16),
        i32::from(lepton_to_cell_component(coord[1]) as i16),
    )
}

fn ground_height_at_coord(terrain: &ResolvedTerrainGrid, coord: [i32; 3]) -> Option<i32> {
    let cell = (
        lepton_to_cell_component(coord[0]),
        lepton_to_cell_component(coord[1]),
    );
    if cell.0 < 0 || cell.1 < 0 {
        return None;
    }
    let cell = terrain.cell(cell.0 as u16, cell.1 as u16)?;
    ground_height_leptons(cell.level, cell.slope_type, coord[0], coord[1]).ok()
}

/// Response-local implementation of `ObjectClass::ShouldBeOnBridge` using the
/// exact destination returned above. The general movement path still carries
/// its separately recorded wider signature residual.
///
/// gamemd-derived: `ObjectClass::ShouldBeOnBridge @ 0x005F6A70` and the Foot
/// override `0x004DDC40`; the height threshold is `3 * 104` leptons.
pub(super) fn should_be_on_bridge_for_response(
    entity: &GameEntity,
    entities: &EntityStore,
    terrain: &ResolvedTerrainGrid,
) -> Option<bool> {
    const HEIGHT_THRESHOLD: i32 = 3 * 104;
    let current = entity_coord(entity, Some(terrain));
    let destination = destination_coord(entity, entities, Some(terrain));
    let current_ground = ground_height_at_coord(terrain, current)?;
    let destination_ground = ground_height_at_coord(terrain, destination)?;
    let destination_cell = (
        lepton_to_cell_component(destination[0]),
        lepton_to_cell_component(destination[1]),
    );
    let destination_has_bridge = terrain
        .cellclass_bridge_flags_0x1180(destination_cell.0, destination_cell.1)
        & BRIDGE_FLAG_STRUCTURAL
        != 0;

    if !entity.on_bridge
        && current_ground.wrapping_sub(destination_ground) > HEIGHT_THRESHOLD
        && destination_has_bridge
    {
        return Some(true);
    }
    if entity.on_bridge && destination_ground.wrapping_sub(current_ground) > HEIGHT_THRESHOLD {
        return Some(false);
    }
    Some(entity.on_bridge)
}

/// gamemd-derived: `FootClass::Evaluate_Target_Threat @ 0x004D97A0` returns
/// `-Cost` when the current `TarCom (+0x2B4)` is the requester, and 0 when it
/// is some other Techno (`+0x14 & 1`) that `Is_Armed (vt+0x2AC)`.
pub(super) fn current_target_disposition(
    candidate: &GameEntity,
    attacker_id: u64,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> ExistingTargetDisposition {
    match candidate.attack_target.as_ref().map(|target| target.target) {
        Some(TargetKind::Entity(id)) if id == attacker_id => {
            ExistingTargetDisposition::RequestedAttacker
        }
        Some(TargetKind::Entity(id))
            if entities.get(id).is_some_and(|target| {
                rules
                    .object(interner.resolve(target.type_ref()))
                    .is_some_and(|object| is_armed(target, object))
            }) =>
        {
            ExistingTargetDisposition::OtherArmedTarget
        }
        _ => ExistingTargetDisposition::NoneOrUnarmed,
    }
}

/// `GetWeaponRange(0)` (`0x004D9840`), the open-topped cargo minimum included.
pub(super) fn primary_range_leptons(
    candidate: &GameEntity,
    object: &ObjectType,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> i32 {
    crate::sim::combat::combat_weapon::weapon_range(candidate, object, 0, entities, rules, interner)
}

/// Whether a candidate enters the response scan: the gates of
/// `TechnoClass__RespondToBaseAttack @ 0x007081D4..0x0070828B` (Infantry)
/// and `0x0070840A..0x007084E3` (Unit), plus the Unit loop's victim-slave
/// test (`0x007085AD`). Those gates are pure reads, so their order is free.
///
/// The weapon-0 peek is GetFireError itself, through vt+0x3BC (`0x006FC090`:
/// the class override without the range test) against the attacker
/// (`0x00708282` / `0x007084B8`, `PUSH 0; PUSH attacker`). Only ILLEGAL (5)
/// refuses (`CMP EAX,0x5 / JZ`); every other code admits. It is not pure (its
/// lazy map queries can stamp the shared Dummy), so it runs where native
/// runs it: after the recruitability gates, before the Unit-only ones.
pub(super) fn candidate_admitted(
    sim: &Simulation,
    rules: &RuleSet,
    candidate: &GameEntity,
    candidate_object: &ObjectType,
    victim: &GameEntity,
    attacker_id: u64,
) -> bool {
    if !candidate.is_object_alive()
        || candidate.owner() != victim.owner()
        || sim
            .team_script_vm
            .team_for_member(candidate.stable_id())
            .is_some_and(|(_, is_base_defense)| !is_base_defense)
        || !candidate.base_defense_response.recruitable_a
        || !candidate.base_defense_response.recruitable_b
        || !is_armed(candidate, candidate_object)
        || (!sim.session.game_mode_nonzero
            && !candidate
                .mission
                .current()
                .known()
                .and_then(|mission| rules.mission_control.entry(mission))
                .is_some_and(|entry| entry.recruitable))
    {
        return false;
    }
    // An Infantry or Unit candidate: BuildingClass::GetWeapon's occupant
    // substitution is never reached.
    let peek = FireSubject {
        world: sim,
        rules,
        overlay_registry: None,
        fog: Some(&sim.fog),
        firer: candidate,
        obj: candidate_object,
        target: Some(TargetKind::Entity(attacker_id)),
        weapon_index: 0,
        garrison: None,
    }
    .fire_error(false);
    if peek == FireError::Illegal {
        return false;
    }
    if candidate.category == EntityCategory::Unit
        && (candidate_object.resource_gatherer
            || candidate.bunker_link.installed_in().is_some()
            || victim.slave.owner().is_some())
    {
        return false;
    }
    true
}
