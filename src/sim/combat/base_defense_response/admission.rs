//! Destination, reachability and exact-5 fire-admission helpers.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::world::Simulation;

use super::super::TargetKind;
use super::super::combat_weapon::is_armed;
use super::super::fire_error::FireError;
use super::super::fire_error_world::FireSubject;
use super::ExistingTargetDisposition;

/// Native70829C/7082DB and7084F4/708533 query virtual+4C with a null
/// requester. The shared coordinate owner supplies the retained locomotor head
/// or the protected Building's class-specific coordinate.
pub(super) fn destination_cell(
    entity: &GameEntity,
    sim: &Simulation,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: &RuleSet,
    interner: &StringInterner,
) -> Result<(i32, i32), String> {
    let coord = crate::sim::movement::nav_target_coordinate(
        crate::sim::components::NavTargetRef::object(entity.stable_id()),
        None,
        sim,
        terrain,
        Some((rules, interner)),
    )?;
    Ok((
        i32::from(crate::util::lepton::lepton_to_cell_packed(coord.x)),
        i32::from(crate::util::lepton::lepton_to_cell_packed(coord.y)),
    ))
}

/// gamemd-derived: `FootClass::Evaluate_Target_Threat @ 0x004D97A0` returns
/// negative ThreatPosed when the current `TarCom (+0x2B4)` is the requester, and 0 when it
/// is some other Techno (`+0x14 & 1`) that `Is_Armed (vt+0x2AC)`.
pub(super) fn current_target_disposition(
    candidate: &GameEntity,
    attacker_id: u64,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> ExistingTargetDisposition {
    match candidate.attack_target.target() {
        Some(TargetKind::Entity(id)) if id == attacker_id => {
            ExistingTargetDisposition::RequestedAttacker
        }
        Some(TargetKind::Entity(id))
            if entities.get(id).is_some_and(|target| {
                interner
                    .object_type(target.type_ref(), rules)
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
/// and `0x0070840A..0x007084E3` (Unit). The victim's SlaveOwner test runs
/// later, after coordinate/layer/reach and threat evaluation (`0x007085AD`).
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
        || (!sim.session.game_mode_nonzero()
            && !crate::sim::mission::control::current_control(rules, &candidate.mission)
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
        fog: Some(sim.fog.fire_view(sim.house_alliances.as_map())),
        firer: candidate,
        obj: candidate_object,
        target: Some(TargetKind::Entity(attacker_id)),
        weapon_index: 0,
    }
    .fire_error(false);
    if peek == FireError::Illegal {
        return false;
    }
    if candidate.category() == EntityCategory::Unit
        && (candidate_object.resource_gatherer || candidate.bunker.link().installed_in().is_some())
    {
        return false;
    }
    true
}
