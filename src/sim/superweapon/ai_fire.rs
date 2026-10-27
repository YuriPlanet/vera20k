//! A computer house's superweapon use: `HouseClass::AI_TryFireSW @
//! 0x005098F0`, which AI_Building_Strategy runs (`sim::house_strategy`), and
//! the target pickers of its arms.
//!
//! AI_TryFireSW skips a human-controlled house (`+0x1EC`, in game mode 0 also
//! PlayerControl `+0x1ED`), then walks the house's Supers (`+0x254`: one per
//! `[SuperWeaponTypes]` entry in list order, built by the House constructor
//! `0x004F620E..0x004F6290`) and takes, for each charged one (`+0x6F`), the
//! arm of its `Type=` (jump table `0x00509AE8`, [`AiFireArm`]). An arm that
//! finds a cell calls Fire_SW (`0x004FAE50`) with the Super's vector index
//! (its FindIndex, vt+0x10), the type's `[SuperWeaponTypes]` index, so a
//! later Super sees what an earlier launch changed.
//!
//! Evidence: `tools/superweapon_oracle.py` sections `ai_try_fire`,
//! `ai_best_rally_target`, `ai_ground_rally_point`, `ai_genetic_mutator` and
//! `ai_psydom` execute the original code; `ai_fire_tests.rs` replays them.
//!
//! RESIDUALS:
//! - The second preferred defensive cell (`+0x54F8`) keeps its constructor
//!   value: House50DA40's writer is not ported and an active caller is not
//!   established. TAction35's type54EC and TAction135/136's cell54F0 are retained
//!   by HouseState and consumed here; its nonzero value bypasses the
//!   MultiMissile/Storm/GroundRally pickers and suppresses Genetic/Dominator.
//!   TAction140/141 now set/clear the separate timed defensive alert54F4/54FC
//!   through that same HouseState owner. Native141 also clears54F8, whose
//!   current constructor-zero restriction remains part of this residual.
//!   Ares's IsTargetConstraintsEligible10032110 reads it as well, so its
//!   defensive-cell constraints see an unset cell.
//! - A building's cloak stage (`BuildingClass+0x6ED`): a stage of 15 also
//!   draws in AI_FindBestRallyTarget (`0x0050CF99..0x0050CFA8`); VERA has no
//!   writer of the stage, so only CloakState 2 draws.
//! - A difficulty past a short `AIIonCannon*Value=` list reads 0
//!   ([`HouseState::difficulty_value`]); native reads past the vector (a null
//!   one for the three keys retail comments out, which no retail building
//!   reaches).

#[cfg(test)]
#[path = "ai_fire_tests.rs"]
mod tests;

use crate::map::entities::EntityCategory;
use crate::map::houses::is_allied_with;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::rng::SimRng;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::lepton::lepton_to_cell_packed;

/// The empty cell `0x00A8EF98`, every picker's "no target".
const NO_CELL: (i16, i16) = (0, 0);

/// AI_Fire_GenMutator walks the cell-offset table (`0x00ABD490`, filled by
/// the startup initializer `0x00561910`) through the radius-1 band's count
/// (`0x007ED3D4`, 9) inclusive: the cell, its eight neighbours, and
/// `(-1, -2)` (`0x00509FEB..0x0050A0BD`).
const GENETIC_MUTATOR_SPREAD_BAND: usize = 1;

/// AI_Fire_PsyDom walks the same table through the radius-3 band's count
/// (`0x007ED3DC`, 37) inclusive: 38 cells around each Foot
/// (`0x0050A1E1..0x0050A2B3`), whatever `DominatorCaptureRange=` says.
const PSYCHIC_DOMINATOR_SPREAD_BAND: usize = 3;

/// The arm AI_TryFireSW's jump table (`0x00509AE8`) takes for a `Type=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiFireArm {
    /// `0x00509968`: [`rally_target`].
    MultiMissile,
    /// `0x00509A0A`, `AI_Fire_LightningStorm @ 0x00509E00`: the MultiMissile
    /// arm behind the raging-storm gate (`0x0053A100`).
    LightningStorm,
    /// `0x00509A31`: [`ground_rally_point`].
    GroundRallyPoint,
    /// `0x00509A17`: [`psychic_dominator_target`].
    PsychicDominator,
    /// `0x00509A24`: [`genetic_mutator_target`].
    GeneticMutator,
    /// `0x00509A3E`: [`force_shield_target`].
    ForceShield,
    /// `0x00509AD2`: Iron Curtain and the Chronosphere pair are left to team
    /// scripts.
    None,
}

impl AiFireArm {
    const fn of(kind: SuperWeaponKind) -> Self {
        match kind {
            SuperWeaponKind::MultiMissile => Self::MultiMissile,
            SuperWeaponKind::LightningStorm => Self::LightningStorm,
            SuperWeaponKind::ParaDrop
            | SuperWeaponKind::AmerParaDrop
            | SuperWeaponKind::SpyPlane
            | SuperWeaponKind::PsychicReveal => Self::GroundRallyPoint,
            #[cfg(feature = "ares")]
            SuperWeaponKind::UnitDelivery => Self::GroundRallyPoint,
            #[cfg(not(feature = "ares"))]
            SuperWeaponKind::UnitDelivery | SuperWeaponKind::GenericWarhead => Self::None,
            SuperWeaponKind::PsychicDominator => Self::PsychicDominator,
            SuperWeaponKind::GeneticConverter => Self::GeneticMutator,
            SuperWeaponKind::ForceShield => Self::ForceShield,
            #[cfg(feature = "ares")]
            SuperWeaponKind::GenericWarhead => Self::None,
            SuperWeaponKind::IronCurtain
            | SuperWeaponKind::ChronoSphere
            | SuperWeaponKind::ChronoWarp
            | SuperWeaponKind::NoEffect => Self::None,
        }
    }
}

/// What [`try_fire`] did, and each Fire_SW, in the oracle's terms
/// (observation only).
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiFireEvent {
    /// AI_TryFireSW ran (the Strategy replay's gate).
    TryFire,
    /// AI_FindBestRallyTarget ran.
    BestRallyTarget,
    /// A Super of this type took the GroundRallyPoint, PsychicDominator or
    /// GeneticMutator arm, a tail call that fires on its own.
    TailArm(AiFireArm, InternedId),
    /// Fire_SW ([`Simulation::fire_super_weapon`], from any caller) for a
    /// Super of this type at this cell.
    Fire(InternedId, (u16, u16)),
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Observation only: what [`try_fire`] and Fire_SW did on this thread
    /// while a test holds `Some`.
    pub(crate) static AI_FIRE_LOG: std::cell::RefCell<Option<Vec<AiFireEvent>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn observe(event: AiFireEvent) {
    AI_FIRE_LOG.with(|log| {
        if let Some(log) = log.borrow_mut().as_mut() {
            log.push(event);
        }
    });
}

/// `HouseClass::AI_TryFireSW @ 0x005098F0` for `owner`: see the module doc.
pub(crate) fn try_fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    #[cfg(any(test, feature = "test-support"))]
    observe(AiFireEvent::TryFire);
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    let human = house.is_controlled_by_human(sim.session.game_mode_nonzero());
    #[cfg(not(feature = "ares"))]
    if human {
        return;
    }
    for name in &rules.super_weapon_order {
        let Some(sw) = rules.super_weapon(name) else {
            continue;
        };
        let Some(sw_type_id) = sim.interner.get(name) else {
            continue;
        };
        #[cfg(feature = "ares")]
        if human
            && rules
                .super_weapon_target_policy(name)
                .is_none_or(|policy| !policy.auto_fire())
        {
            continue;
        }
        if !super::is_charged(sim, owner, sw_type_id) {
            continue;
        }
        // Reject extension-gated types before target search can consume RNG.
        #[cfg(any(feature = "ares", feature = "phobos"))]
        if !super::super_weapon_available(sim, rules, owner, &sw.id) {
            continue;
        }
        #[cfg(feature = "ares")]
        if sw.kind == SuperWeaponKind::UnitDelivery
            && !super::unit_delivery::configured(sim, rules, owner, sw)
        {
            continue;
        }
        // Do not spend targeting RNG or attempt an unaffordable paid launch.
        // The Fire_SW transaction checks the current wallet again.
        #[cfg(feature = "ares")]
        if !super::money::configuration_supported(sw)
            || !super::money::can_afford(&sim.houses, owner, sw)
        {
            continue;
        }
        #[cfg(feature = "ares")]
        if rules.super_weapon_target_policy(name).is_some() {
            // House AI hook10038E10 also excludes drain state2.
            if sim.super_weapons[&owner][&sw_type_id].charge_drain_state() != 2 {
                fire_extension_automatic(
                    sim,
                    rules,
                    owner,
                    sw_type_id,
                    registry,
                    frame_effects,
                    false,
                );
            }
            continue;
        }
        let arm = AiFireArm::of(sw.kind);
        #[cfg(any(test, feature = "test-support"))]
        if matches!(
            arm,
            AiFireArm::GroundRallyPoint | AiFireArm::PsychicDominator | AiFireArm::GeneticMutator
        ) {
            observe(AiFireEvent::TailArm(arm, sw_type_id));
        }
        let cell = match arm {
            AiFireArm::MultiMissile => rally_target(sim, rules, owner, registry),
            AiFireArm::LightningStorm if !super::lightning_storm::raging(sim) => {
                rally_target(sim, rules, owner, registry)
            }
            AiFireArm::GroundRallyPoint => ground_rally_point(sim, rules, owner, registry),
            AiFireArm::PsychicDominator => psychic_dominator_target(sim, rules, owner),
            AiFireArm::GeneticMutator => genetic_mutator_target(sim, rules, owner),
            AiFireArm::ForceShield => sim.houses.get(&owner).and_then(|house| {
                force_shield_target(house, rules, sim.session.binary_frame() as i32)
            }),
            AiFireArm::LightningStorm | AiFireArm::None => None,
        };
        if let Some(cell) = cell {
            sim.fire_super_weapon(
                rules,
                owner,
                sw_type_id,
                cell,
                registry,
                frame_effects,
                true,
            );
        }
    }
}

/// `IsTargetConstraintsEligible`10032110 for `owner`, with the manual return
/// of a SPECIAL_PLACE launch, through the one [`TargetConstraints`] owner.
///
/// [`TargetConstraints`]: crate::rules::superweapon_targeting::TargetConstraints
#[cfg(feature = "ares")]
fn extension_constraints(
    sim: &Simulation,
    owner: InternedId,
    policy: crate::rules::superweapon_targeting::AresSuperWeaponTargetingState,
    manual: bool,
) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let constraints = policy.constraints();
    let observations = house.target_constraints_observations();
    if manual {
        return constraints.allows_manual_fire(observations);
    }
    let power = sim.power_states.get(&owner);
    constraints.allows_automatic_fire(
        observations,
        crate::rules::superweapon_targeting::AutomaticConstraintObservations {
            lightning_storm_active: super::lightning_storm::raging(sim),
            dominator_active: super::psychic_dominator::active(sim),
            last_building_attack_frame: house.strategy_emergency().last_building_attack_frame(),
            frame: sim.session.binary_frame() as i32,
            power_output: power.map_or(0, |power| power.total_output()),
            power_drain: power.map_or(0, |power| power.total_drain()),
        },
    )
}

/// Sidebar select hook10034DA0: base CanFire, which `SW.AutoFire` without
/// `SW.ManualFire` skips, precedes the wallet, which precedes the UseAI
/// constraint warning; that AutoFire-only type is then refused silently.
/// The constraint result is the view's `constraints_blocked`. The
/// AutoFire-only refusal applies to the two extension types whose automatic
/// launch VERA ports.
#[cfg(feature = "ares")]
pub(super) fn manual_cameo_refusal(
    rules: &RuleSet,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    native_can_fire: bool,
    can_afford: bool,
    constraints_blocked: bool,
) -> Option<super::AresCameoRefusal> {
    use super::AresCameoRefusal as R;
    let manual_disabled = rules
        .super_weapon_target_policy(&sw.id)
        .is_some_and(|policy| policy.auto_fire() && !policy.manual_fire());
    if !native_can_fire && !manual_disabled {
        None
    } else if !can_afford {
        Some(R::InsufficientFunds)
    } else if constraints_blocked {
        Some(R::CannotFire)
    } else {
        manual_disabled.then_some(R::ManualFireDisabled)
    }
}

/// Shared automatic launch100382D0 for House AI and SPECIAL_PLACE. Target
/// selection owns any RNG; Fire_SW remains the one charge/effect transaction.
#[cfg(feature = "ares")]
pub(super) fn fire_extension_automatic(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_id: InternedId,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
    manual: bool,
) -> bool {
    let Some(sw) = rules.super_weapon(sim.interner.resolve(sw_id)) else {
        return false;
    };
    let Some(policy) = rules.super_weapon_target_policy(&sw.id) else {
        return false;
    };
    if !super::money::can_afford(&sim.houses, owner, sw) {
        return false;
    }
    if !super::super_weapon_available(sim, rules, owner, &sw.id) {
        return false;
    }
    if !extension_constraints(sim, owner, policy, manual) {
        return false;
    }
    let Some(cell) = extension_target(sim, rules, owner, sw, policy, registry) else {
        return false;
    };
    let _ = sim.fire_super_weapon_selected(
        rules,
        owner,
        sw_id,
        cell,
        registry,
        effects,
        super::SuperWeaponTargetSource::SelectedAutomatic,
        None,
    );
    // AutoFire10038373..10038386 returns Fire_SW's AL via bridge100385A0.
    // Native Fire_SW4FAE50 returns1 even when ClickFire declines; the Rust
    // transaction above instead reports whether it called Launch. A selected
    // but now-unready event is handled, not a CannotFire selector refusal.
    true
}

#[cfg(feature = "ares")]
fn extension_target(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    policy: crate::rules::superweapon_targeting::AresSuperWeaponTargetingState,
    registry: Option<&OverlayTypeRegistry>,
) -> Option<(u16, u16)> {
    use crate::rules::superweapon_targeting::{AiTargetPreference as P, AiTargetingMode as M};
    // PickTarget10038640 prefers these cells without a CanFireAt call. It
    // constructs the same ephemeral data even when a preference wins.
    let data = super::targeting::prepare(sim, rules, owner, sw);
    let house = sim.houses.get(&owner)?;
    if policy.preference() == P::Offensive && house.preferred_super_weapon_target() != (0, 0) {
        return Some(house.preferred_super_weapon_target());
    }
    // Defensive preference reads +54F8, currently the constructor-zero residual.
    let kind = house.preferred_super_weapon_target_kind();
    match policy.mode() {
        M::None | M::IronCurtain => None,
        M::NoTarget | M::HunterSeeker | M::Attack | M::LowPower | M::LowPowerAttack => Some((0, 0)),
        M::PsychicDominator => ares_spread_target(sim, rules, owner, sw, policy, &data, false),
        M::GeneticMutator => ares_spread_target(sim, rules, owner, sw, policy, &data, true),
        M::Offensive => offensive_target(sim, rules, owner, sw, policy, &data),
        M::Nuke | M::LightningStorm => {
            //100386CE: type1 uses the shared rating selector. Team quarry
            // results bypass CanFireAt, like the preferred-cell path.
            if kind == 1 {
                offensive_target(sim, rules, owner, sw, policy, &data)
            } else {
                let cell = find_team_target(sim, rules, owner, kind, registry);
                (cell != NO_CELL).then_some((cell.0 as u16, cell.1 as u16))
            }
        }
        M::ParaDrop => {
            let cell = ground_rally_point(sim, rules, owner, registry)?;
            super::targeting::check_with_data(sim, rules, owner, sw, cell, true, &data)
                .is_ok()
                .then_some(cell)
        }
        M::ForceShield => {
            let cell = force_shield_target(
                sim.houses.get(&owner)?,
                rules,
                sim.session.binary_frame() as i32,
            )?;
            super::targeting::check_with_data(sim, rules, owner, sw, cell, true, &data)
                .is_ok()
                .then_some(cell)
        }
        // These distinct3.0 selectors are still pending native ports. Do not
        // silently replace a parsed mode with Offensive or another behavior.
        M::Stealth
        | M::SelfTarget
        | M::Base
        | M::MultiMissile
        | M::EnemyBase
        | M::DropPod
        | M::LightningRandom => None,
    }
}

/// GetPotentialAITargets100328D0 chooses native registries from the effective
/// automatic target mask. All arrays retain construction/removal order.
#[cfg(feature = "ares")]
fn potential_targets(
    sim: &Simulation,
    mask: crate::rules::superweapon_targeting::TargetMask,
) -> impl Iterator<Item = &GameEntity> {
    use crate::rules::superweapon_targeting::TargetMask as T;
    let entities = &sim.substrate.entities;
    let buildings = mask.intersects(T::BUILDING);
    let feet = mask.intersects(T::INFANTRY) || mask.intersects(T::UNIT);
    let array = if buildings && !feet {
        1
    } else if buildings || !feet {
        0
    } else if mask.intersects(T::INFANTRY) && !mask.intersects(T::UNIT) {
        2
    } else {
        3
    };
    let count = match array {
        1 => entities.building_registry_len(),
        2 => entities.infantry_registry_len(),
        3 => entities.foot_registry_len(),
        _ => entities.techno_registry_len(),
    };
    (0..count)
        .filter_map(move |index| match array {
            1 => entities.building_registry_at(index),
            2 => entities.infantry_registry_at(index),
            3 => entities.foot_registry_at(index),
            _ => entities.techno_registry_at(index),
        })
        .filter_map(move |id| entities.get(id))
}

/// The MultiMissile arm (`0x00509968..0x005099E6`), also
/// AI_Fire_LightningStorm's body: with an enemy (`+0x5600`), the preferred
/// cell, else for preferred type 1 [`find_best_rally_target`]; the empty cell
/// fires nothing.
fn rally_target(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&OverlayTypeRegistry>,
) -> Option<(u16, u16)> {
    let house = sim.houses.get(&owner)?;
    let enemy = house.enemy_house()?;
    let preferred = house.preferred_super_weapon_target();
    let kind = house.preferred_super_weapon_target_kind();
    //509968..5099E6 /509E00: the enemy gate precedes this preference.
    if preferred != (0, 0) {
        return Some(preferred);
    }
    let (x, y) = if kind == 1 {
        #[cfg(any(test, feature = "test-support"))]
        observe(AiFireEvent::BestRallyTarget);
        find_best_rally_target(sim, rules, owner, enemy)
    } else {
        find_team_target(sim, rules, owner, kind, registry)
    };
    // A cell left of or above the map wraps like the native CellStruct.
    ((x, y) != NO_CELL).then_some((x as u16, y as u16))
}

/// HouseClass::AI_FindTeamTarget50D170 (Steam gamemd):
/// first own Team in registration order, shared GetLeader6EC3D0, then its
/// GreatestThreat with QuarryToThreat645BB0 and OnlyTargetHouseEnemy=true.
/// The caller stages that true argument BEFORE GetCoords(vt48); it is not
/// a GetCoords parameter. Signed coordinates truncate toward zero to cells.
fn find_team_target(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    quarry: i32,
    registry: Option<&OverlayTypeRegistry>,
) -> (i16, i16) {
    let Some(team) = sim
        .team_script_vm
        .teams_in_order()
        .find(|team| team.owner() == owner)
        .map(|team| team.id())
    else {
        return NO_CELL;
    };
    let Some(leader) = sim.team_leader(team, rules) else {
        return NO_CELL;
    };
    let Some(entity) = sim.substrate.entities.get(leader) else {
        return NO_CELL;
    };
    let origin =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    let Some(target) = sim.greatest_threat_represented(
        rules,
        registry,
        leader,
        crate::sim::combat::ScanMission::Quarry {
            mask: crate::sim::combat::quarry_mask(quarry),
            only_target_house_enemy: true,
        },
        Some([origin.x, origin.y, origin.z]),
    ) else {
        return NO_CELL;
    };
    let Some(entity) = sim.substrate.entities.get(target) else {
        return NO_CELL;
    };
    let coord =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    (
        crate::util::lepton::lepton_to_cell_packed(coord.x),
        crate::util::lepton::lepton_to_cell_packed(coord.y),
    )
}

/// `HouseClass::AI_FindBestRallyTarget @ 0x0050CBF0`: every Techno of
/// TechnoClass::Array (stable-id order) read as a [`RallyEntry`], then
/// [`pick_best_rally_target`].
fn find_best_rally_target(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    enemy: InternedId,
) -> (i16, i16) {
    let Some(house) = sim.houses.get(&owner) else {
        return NO_CELL;
    };
    let entries: Vec<RallyEntry> = sim
        .substrate
        .entities
        .values()
        .map(|entity| rally_entry(sim, rules, house, Some(enemy), entity))
        .collect();
    pick_best_rally_target(&entries, &mut sim.scenario_rng)
}

/// Ares Hooks.Targeting.cpp: OffensiveTargetSelector/ PickIonCannonTarget.
/// Shares native eligibility and values, but ignores cloaked objects and
/// scripted target preferences. Only a final tie selection draws Scenario RNG.
#[cfg(feature = "ares")]
fn offensive_target(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    policy: crate::rules::superweapon_targeting::AresSuperWeaponTargetingState,
    data: &super::targeting::TargetingData,
) -> Option<(u16, u16)> {
    let house = sim.houses.get(&owner)?;
    let enemy = house.enemy_house();
    let mut best = 0;
    let mut picks = Vec::new();
    let mask = policy.automatic().target;
    for entity in potential_targets(sim, mask) {
        if enemy.is_some_and(|enemy| entity.owner() != enemy) {
            continue;
        }
        let entry = rally_entry(sim, rules, house, enemy, entity);
        let cell = (entry.cell.0 as u16, entry.cell.1 as u16);
        if !entry.candidate
            || !entry.in_playfield
            || entry.cloaked
            || entry.value < best
            || super::targeting::check_with_data(sim, rules, owner, sw, cell, true, data).is_err()
        {
            continue;
        }
        if entry.value > best {
            best = entry.value;
            picks.clear();
        }
        picks.push(cell);
    }
    if picks.is_empty() {
        return None;
    }
    let last = i32::try_from(picks.len() - 1).unwrap_or(i32::MAX);
    let cell = picks[sim.scenario_rng.next_range_i32_inclusive(0, last) as usize];
    (cell != (0, 0)).then_some(cell)
}

/// Ares PsychicDominator10038070 and GeneticMutator10037EB0 share the
/// density kernel with retail, but use forward potential registries, exclude
/// cloak2, and require CanFireAt before changing the maximum. Ares iterates
/// exactly37/9 cells, unlike the retail inclusive38/10 loops.
#[cfg(feature = "ares")]
fn ares_spread_target(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    policy: crate::rules::superweapon_targeting::AresSuperWeaponTargetingState,
    data: &super::targeting::TargetingData,
    genetic: bool,
) -> Option<(u16, u16)> {
    let terrain = sim.resolved_terrain.as_ref()?;
    let entities = &sim.substrate.entities;
    let infantry = |member: &CellObjectMember| {
        counted(sim, member, |entity| {
            entity.category() == EntityCategory::Infantry
        })
    };
    let foot = |member: &CellObjectMember| {
        counted(sim, member, |entity| {
            entity.category() != EntityCategory::Structure
        })
    };
    let mask = policy.automatic().target;
    let (id, _) = best_spread_center(
        terrain,
        potential_targets(sim, mask),
        crate::sim::combat::cell_spread::sweep(if genetic { 1 } else { 3 }),
        |entity| {
            if genetic && entity.on_bridge() {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            }
        },
        |cell, layer| {
            sim.cell_objects(cell, layer)
                .skip_while(|member| genetic && !infantry(member))
                .take_while(|member| {
                    if genetic {
                        infantry(member)
                    } else {
                        foot(member)
                    }
                })
                .filter(|member| {
                    counted(sim, member, |entity| {
                        hostile_and_low(sim, rules, terrain, owner, entity)
                            && !entity
                                .cloak
                                .as_ref()
                                .is_some_and(|cloak| cloak.is_fully_cloaked())
                            && (genetic
                                || sim.can_be_perma_mind_controlled(entity.stable_id(), rules))
                    })
                })
                .count()
        },
        |cell| super::targeting::check_with_data(sim, rules, owner, sw, cell, true, data).is_ok(),
    )?;
    // Convert the chosen object's GetCoords, not its GetCell center; buildings
    // selected through an explicit mask can have different foundation centers.
    let at = crate::sim::movement::ground_pose::object_get_coords(entities.get(id)?, Some(terrain));
    Some(((at.x / 256) as u16, (at.y / 256) as u16))
}

/// What AI_FindBestRallyTarget reads of one TechnoClass::Array entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RallyEntry {
    /// An enemy object on the ground (InWhichLayer, vt+0x78, is 2), alive
    /// (`+0x90`) and out of limbo (`+0x81`); or, for a Hard house (`+0x184`
    /// 0), one a running factory is building
    /// (`FactoryRegistry::is_building`, `0x0050CC98..0x0050CCDC`).
    candidate: bool,
    /// [`rally_value`] for an enemy object, else 0.
    value: i32,
    /// GetCoords (vt+0x48) over 256, toward zero, as 16 bits
    /// (`0x0050CF44..0x0050CF6E`).
    cell: (i16, i16),
    /// `MapClass::IsCellInPlayfield(cell, 1) @ 0x00578460`; off it the value
    /// is 0.
    in_playfield: bool,
    /// CloakState (`+0x220`) 2: the value becomes a Scenario draw.
    cloaked: bool,
}

/// What the walk reads of one object (`0x0050CC4E..0x0050CFA8`). Each one's
/// cell goes through the playfield test, whose cell lookup stamps the shared
/// dummy on a miss, as native.
fn rally_entry(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    enemy: Option<InternedId>,
    entity: &crate::sim::game_entity::GameEntity,
) -> RallyEntry {
    let id = entity.stable_id();
    let (candidate, value) = if enemy.is_none_or(|enemy| entity.owner() == enemy) {
        let standing = sim.entity_display_layer(id, rules)
            == Some(crate::sim::world::display_layers::DisplayLayer::GROUND)
            && entity.is_object_alive()
            && !entity.lifecycle.in_limbo();
        let candidate = standing
            || (house.difficulty() == HouseDifficulty::Hard
                && sim.production.factories().is_building(id));
        let ty = sim.interner.object_type(entity.type_ref(), rules);
        (candidate, rally_value(entity.category(), ty, rules, house))
    } else {
        (false, 0)
    };
    let coords =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    let cell = (
        lepton_to_cell_packed(coords.x),
        lepton_to_cell_packed(coords.y),
    );
    RallyEntry {
        candidate,
        value,
        cell,
        in_playfield: crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
            (i32::from(cell.0), i32::from(cell.1)),
            sim.playfield_bounds(),
            sim.resolved_terrain.as_ref(),
            None,
        ),
        cloaked: entity
            .cloak
            .as_ref()
            .is_some_and(|cloak| cloak.is_fully_cloaked()),
    }
}

/// The value AI_FindBestRallyTarget gives an enemy object
/// (`0x0050CCE2..0x0050CF34`): by its WhatAmI (vt+0x2C), its kind's
/// `[General] AIIonCannon*Value=` entry at the firing house's difficulty,
/// tested in this order; an aircraft is worth 1.
/// - Unit (type `+0x6C4`): Harvester= (`+0xE0E`); DeploysInto= (`+0x404`)
///   in `[AI] BuildConst=` (MCV); Passengers= (`+0x5E0`) above 0 (APC); else 2.
/// - Building (type `+0x520`): Factory= (`+0xEB8`) BuildingType (ConYard);
///   UnitType and not Naval= (`+0xCCE`; WarFactory); Power= above 0 (bonus
///   `+0xEE0` over drain `+0xEE4`); IsBaseDefense=; IsPlug=; IsTemple=;
///   HoverPad= (Helipad); in `[AI] BuildTech=` (TechCenter); else 4.
/// - Infantry (type `+0x6C0`): Engineer= (`+0xEC3`); VehicleThief= (`+0xEC6`,
///   Thief); else 2.
fn rally_value(
    category: EntityCategory,
    ty: Option<&ObjectType>,
    rules: &RuleSet,
    house: &HouseState,
) -> i32 {
    let values = &rules.general.ai_ion_cannon_values;
    let value = |list: &[i32]| house.difficulty_value(list);
    // Every Techno of these classes has a type.
    let Some(ty) = ty.filter(|_| category != EntityCategory::Aircraft) else {
        return 1;
    };
    match category {
        EntityCategory::Unit if ty.harvester => value(&values.harvester),
        EntityCategory::Unit
            if ty
                .deploys_into
                .as_deref()
                .and_then(|name| rules.object(name))
                .is_some_and(|building| building.build_const_eligible) =>
        {
            value(&values.mcv)
        }
        EntityCategory::Unit if ty.passengers > 0 => value(&values.apc),
        EntityCategory::Unit => 2,
        EntityCategory::Structure => match ty.factory_type() {
            Some(FactoryType::BuildingType) => value(&values.con_yard),
            Some(FactoryType::UnitType) if !ty.naval => value(&values.war_factory),
            // `Power=` as the reader stores it (`0x00461080..0x0046109A`):
            // the bonus when not negative, else the negated drain.
            _ if ty.power.max(0)
                > crate::sim::power_system::native_building_power_drain(ty.power) =>
            {
                value(&values.power)
            }
            _ if ty.is_base_defense => value(&values.base_defense),
            _ if ty.is_plug => value(&values.plug),
            _ if ty.is_temple => value(&values.temple),
            _ if ty.hover_pad => value(&values.helipad),
            _ if rules.is_build_tech(&ty.id) => value(&values.tech_center),
            _ => 4,
        },
        EntityCategory::Infantry if ty.engineer => value(&values.engineer),
        EntityCategory::Infantry if ty.vehicle_thief => value(&values.thief),
        EntityCategory::Infantry => 2,
        EntityCategory::Aircraft => 1,
    }
}

/// AI_FindBestRallyTarget's choice (`0x0050CF38..0x0050D16B`): in array
/// order, an entry off the playfield is worth 0 and a cloaked one (of any
/// house) a Scenario draw `RandomRanged(0, best + 10)`; candidates worth more
/// than the best so far replace the picks, as much join them. The final
/// Scenario draw `RandomRanged(0, picks - 1)` chooses; none is the empty cell.
fn pick_best_rally_target(entries: &[RallyEntry], rng: &mut SimRng) -> (i16, i16) {
    let mut best = 0i32;
    let mut picks: Vec<(i16, i16)> = Vec::new();
    for entry in entries {
        let mut value = if entry.in_playfield { entry.value } else { 0 };
        if entry.cloaked {
            value = rng.next_range_i32_inclusive(0, best.wrapping_add(10));
        }
        if !entry.candidate {
            continue;
        }
        if value > best {
            picks.clear();
            best = value;
            picks.push(entry.cell);
        } else if value == best {
            picks.push(entry.cell);
        }
    }
    if picks.is_empty() {
        return NO_CELL;
    }
    let last = i32::try_from(picks.len() - 1).unwrap_or(i32::MAX);
    picks[rng.next_range_i32_inclusive(0, last) as usize]
}

/// `HouseClass::AI_GroundRallyPoint @ 0x00509CD0`, the ParaDrop,
/// AmerParaDrop, SpyPlane and PsychicReveal arm: a nonempty scripted
/// preference returns directly; otherwise a passable cell near [`ground_rally_seed`]
/// (`Find_Nearby_Passable_Cell(seed, Foot, no zone, Normal, 5x5, bridges
/// allowed)`, `0x00509D61..0x00509D9A`; none found is the empty cell: its
/// `0x0056E7A1` reads `0x00ABD480`, which only the startup `0x005618B0`
/// writes, with zero), then [`ground_rally_cell`].
fn ground_rally_point(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    registry: Option<&OverlayTypeRegistry>,
) -> Option<(u16, u16)> {
    if let Some(preferred) = sim
        .houses
        .get(&owner)
        .map(HouseState::preferred_super_weapon_target)
        .filter(|&cell| cell != (0, 0))
    {
        //509CF8: neither FNPC nor the two-cell offset applies to this branch.
        return Some(preferred);
    }
    let kind = sim.houses.get(&owner)?.preferred_super_weapon_target_kind();
    if kind != 1 {
        let cell = find_team_target(sim, rules, owner, kind, registry);
        //509D34 jumps straight to the empty-cell gate, bypassing FNPC/+2.
        return (cell != NO_CELL).then_some((cell.0 as u16, cell.1 as u16));
    }
    let (x, y) = ground_rally_seed(&sim.houses, owner);
    let found = sim
        .nearby_passable_cell(
            (i32::from(x as i16), i32::from(y as i16)),
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: SpeedType::Foot,
                    required_zone_id: None,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::new(5, 5),
            ),
        )
        .unwrap_or((0, 0));
    ground_rally_cell(found)
}

/// AI_GroundRallyPoint's seed: the enemy's base cell (`+0x5600`), the
/// house's own without an enemy ([`HouseState::base_origin`],
/// `0x00509D41..0x00509D5B`).
fn ground_rally_seed(houses: &crate::sim::houses::Houses, owner: InternedId) -> (u16, u16) {
    let Some(house) = houses.get(&owner) else {
        return (0, 0);
    };
    house
        .enemy_house()
        .and_then(|enemy| houses.get(&enemy))
        .unwrap_or(house)
        .base_origin()
}

/// AI_GroundRallyPoint's cell: the found one moved two cells along both axes
/// (`0x00509DA1..0x00509DAF`, 16-bit adds); the empty cell fires nothing
/// (`0x00509DB4..0x00509DCD`).
fn ground_rally_cell((x, y): (u16, u16)) -> Option<(u16, u16)> {
    let cell = (x.wrapping_add(2), y.wrapping_add(2));
    (cell != (0, 0)).then_some(cell)
}

/// `HouseClass::AI_Fire_GenMutator @ 0x00509F60` with an
/// empty preferred cell (a set one returns at once, `0x00509F78..
/// 0x00509F93`): [`densest_spread_cell`] around each infantry of
/// InfantryClass::Array with [`GENETIC_MUTATOR_SPREAD_BAND`]. Per cell it
/// reads the list the centre's own OnBridge (`+0x8C`) picks, from its first
/// infantry (CellClass::GetInfantry, `0x0047EC40`) up to the next object that
/// is not one, and counts each infantry that passes [`hostile_and_low`].
fn genetic_mutator_target(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
) -> Option<(u16, u16)> {
    if sim.houses.get(&owner)?.preferred_super_weapon_target() != (0, 0) {
        return None;
    }
    let terrain = sim.resolved_terrain.as_ref()?;
    let entities = &sim.substrate.entities;
    let is_infantry = |member: &CellObjectMember| match member {
        CellObjectMember::Entity(id) => entities
            .get(*id)
            .is_some_and(|object| object.category() == EntityCategory::Infantry),
        CellObjectMember::Terrain(_) => false,
    };
    let centres = (0..entities.infantry_registry_len())
        .rev()
        .filter_map(|index| entities.infantry_registry_at(index))
        .filter_map(|id| entities.get(id));
    densest_spread_cell(
        sim,
        terrain,
        centres,
        GENETIC_MUTATOR_SPREAD_BAND,
        |infantry| {
            if infantry.on_bridge() {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            }
        },
        |cell, layer| {
            sim.cell_objects(cell, layer)
                .skip_while(|member| !is_infantry(member))
                .take_while(is_infantry)
                .filter(|member| {
                    counted(sim, member, |object| {
                        hostile_and_low(sim, rules, terrain, owner, object)
                    })
                })
                .count()
        },
    )
}

/// `HouseClass::AI_Fire_PsyDom @ 0x0050A150`: nothing while a Dominator runs
/// (`PsyDom::Active`, `0x0050A157`) or without an enemy (`+0x5600`); with the
/// empty preferred cell (a set one returns at once,
/// `0x0050A171..0x0050A19D`), [`densest_spread_cell`]
/// around each Foot of FootClass::Array (`0x008B3DC4`, which the Foot
/// constructor appends to at `0x004D34D7`: stable-id order) with
/// [`PSYCHIC_DOMINATOR_SPREAD_BAND`]. Per cell it reads the ground list
/// (`+0xE4`) whatever the bridge, from its head while each object is a Foot
/// (`+0x14 & 4`, set at `0x004D34DD`; a list that starts with another object
/// counts nothing), and counts each that passes [`hostile_and_low`] and
/// CanBePermaMindControlled (`0x0053C450`). No random draws.
fn psychic_dominator_target(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
) -> Option<(u16, u16)> {
    if super::psychic_dominator::active(sim) {
        return None;
    }
    sim.houses.get(&owner)?.enemy_house()?;
    if sim.houses.get(&owner)?.preferred_super_weapon_target() != (0, 0) {
        return None;
    }
    let terrain = sim.resolved_terrain.as_ref()?;
    let entities = &sim.substrate.entities;
    let is_foot = |member: &CellObjectMember| match member {
        CellObjectMember::Entity(id) => entities
            .get(*id)
            .is_some_and(|object| object.category() != EntityCategory::Structure),
        CellObjectMember::Terrain(_) => false,
    };
    let feet: Vec<&GameEntity> = entities
        .values()
        .filter(|object| object.category() != EntityCategory::Structure)
        .collect();
    densest_spread_cell(
        sim,
        terrain,
        feet.into_iter().rev(),
        PSYCHIC_DOMINATOR_SPREAD_BAND,
        |_| MovementLayer::Ground,
        |cell, layer| {
            sim.cell_objects(cell, layer)
                .take_while(is_foot)
                .filter(|member| {
                    counted(sim, member, |object| {
                        hostile_and_low(sim, rules, terrain, owner, object)
                            && sim.can_be_perma_mind_controlled(object.stable_id(), rules)
                    })
                })
                .count()
        },
    )
}

/// What AI_Fire_GenMutator and AI_Fire_PsyDom share. From the array's last
/// centre to its first (`centres`, in that order), each one out of limbo
/// (`+0x81`) sums `count` over the real cells (`MapClass::operator[] @
/// 0x005657A0`; the shared dummy lists nothing) of `band`'s inclusive spread
/// sweep around its own cell (vt+0x1BC), in the ground or bridge list `list`
/// picks for it. The first centre with strictly the most gives its cell,
/// which fires when one counted at all, it is not the empty cell and
/// `MapClass::Is_Cell_In_Playfield(cell, 1) @ 0x00578460` passes
/// (`0x0050A0E7..0x0050A120`, `0x0050A2DD..0x0050A316`).
///
/// Nothing changes during the walk, so each list is counted once and reused
/// by every centre that sweeps it (natively each centre recounts it). Every
/// lookup still runs: a miss stamps the shared dummy's coordinates.
fn densest_spread_cell<'a>(
    sim: &Simulation,
    terrain: &ResolvedTerrainGrid,
    centres: impl Iterator<Item = &'a GameEntity>,
    band: usize,
    list: impl Fn(&GameEntity) -> MovementLayer,
    count: impl FnMut((u16, u16), MovementLayer) -> usize,
) -> Option<(u16, u16)> {
    let (_, at) = best_spread_center(
        terrain,
        centres,
        crate::sim::combat::cell_spread::inclusive_sweep(band),
        list,
        count,
        |_| true,
    )?;
    (at != NO_CELL
        && crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
            (i32::from(at.0), i32::from(at.1)),
            sim.playfield_bounds(),
            Some(terrain),
            None,
        ))
    .then_some((at.0 as u16, at.1 as u16))
}

/// Shared density kernel; callers retain their own registry, spread boundary,
/// eligibility and final conversion. Counts are read-only per-attempt caches.
fn best_spread_center<'a>(
    terrain: &ResolvedTerrainGrid,
    centres: impl Iterator<Item = &'a GameEntity>,
    offsets: &[(i16, i16)],
    list: impl Fn(&GameEntity) -> MovementLayer,
    mut count: impl FnMut((u16, u16), MovementLayer) -> usize,
    accept: impl Fn((u16, u16)) -> bool,
) -> Option<(u64, (i16, i16))> {
    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    // Per real cell, its ground and bridge counts once read.
    let mut counts = vec![[None; 2]; terrain.cells().len()];
    let mut best = 0usize;
    let mut best_cell = NO_CELL;
    let mut best_id = None;
    for centre in centres {
        if centre.lifecycle.in_limbo() {
            continue;
        }
        let layer = list(centre);
        let at = (centre.position.rx() as i16, centre.position.ry() as i16);
        let mut total = 0usize;
        for &(dx, dy) in offsets {
            let looked = cells.lookup((at.0.wrapping_add(dx), at.1.wrapping_add(dy)));
            let crate::map::cell_index::NativeCellIdentity::Real(index) = looked else {
                continue;
            };
            let slot = &mut counts[index][usize::from(layer == MovementLayer::Bridge)];
            total += *slot.get_or_insert_with(|| {
                let (x, y) = cells.coord(looked);
                count((x as u16, y as u16), layer)
            });
        }
        if total > best && accept((at.0 as u16, at.1 as u16)) {
            best = total;
            best_cell = at;
            best_id = Some(centre.stable_id());
        }
    }
    best_id.map(|id| (id, best_cell))
}

/// Whether a cell-list member is an object `test` accepts.
fn counted(
    sim: &Simulation,
    member: &CellObjectMember,
    test: impl FnOnce(&GameEntity) -> bool,
) -> bool {
    match member {
        CellObjectMember::Entity(id) => sim.substrate.entities.get(*id).is_some_and(test),
        CellObjectMember::Terrain(_) => false,
    }
}

/// The test both pickers apply to an object they count: owned by neither the
/// house nor an ally of it (`+0x5788`; natively an object of no house also
/// passes) and not high-flying (vt+0x54).
fn hostile_and_low(
    sim: &Simulation,
    rules: &RuleSet,
    terrain: &ResolvedTerrainGrid,
    owner: InternedId,
    object: &GameEntity,
) -> bool {
    object.owner() != owner
        && !is_allied_with(
            sim.house_alliances.as_map(),
            sim.interner.resolve(owner),
            sim.interner.resolve(object.owner()),
        )
        && !crate::sim::movement::air_movement::is_high_flying(
            object,
            Some(terrain),
            Some((rules, &sim.interner)),
        )
}

/// The ForceShield arm (`0x00509A3E..0x00509AAF`): with no second preferred
/// defensive cell (module RESIDUALS), the launch alert's cell (`+0x54F4`)
/// while the alert's frame (`+0x54FC`) plus `[General]
/// AISuperDefenseFrames=` is later than now (`0x00509A7F..0x00509A99`, a
/// wrapping add and a signed compare).
fn force_shield_target(house: &HouseState, rules: &RuleSet, frame: i32) -> Option<(u16, u16)> {
    let (cell, alerted) = house.super_weapon_defense();
    (cell != (0, 0) && alerted.wrapping_add(rules.general.ai_super_defense_frames) > frame)
        .then_some(cell)
}
