//! Combat: what an attacker holds as its target, the shots it fires, the
//! receivers they reach, and what a death leaves behind.
//!
//! - Targets: `attack_target` (the held target and its coordinates),
//!   `combat_targeting`, `greatest_threat`, `threat_range`, `threat_mask` and
//!   `threat_posed` (acquisition), `in_range` and `line_of_fire` (the range
//!   gate), `combat_weapon` (weapon choice), and
//!   `fire_error` and `fire_error_world` (legality).
//! - Firing: `world_fire` (each class's FireAt visit and its commit), `projectile_delivery`
//!   (the shot's BulletType facts and the Bullet's admission), `aim_coord`,
//!   `fire_coord`, `burst`, `rof`, `gattling`, `inviso_scatter`, `laser`,
//!   `electric_bolt` and `parasite`.
//! - Damage: `world_receiver` (the receivers in native order), `receiver_call`
//!   (one `ReceiveDamage` call's ABI and stages), `receiver_health`,
//!   `object_health`, `damage` (the numeric receiver), `combat_aoe` and
//!   `cell_spread` (area damage), `shrapnel`, `base_defense_response`, and
//!   `detonation_anim` (the impact's animation).
//! - Deaths: `techno_death` (the shared death block), `destruction_effects`,
//!   `record_kill` and `veterancy`.
//! - `smudge_dispatch` places the marks impacts and deaths leave, and
//!   `effects` carries all of it to the world.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components and rules/ (RuleSet).
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

pub mod aim_coord;
mod attack_target;
mod airburst;
#[cfg(feature = "phobos")]
mod phobos_airburst;
pub(crate) mod base_defense_response;
pub mod burst;
pub(crate) mod cell_spread;
pub(crate) mod combat_aoe;
pub(crate) mod combat_targeting;
pub mod combat_weapon;
#[cfg(feature = "phobos")]
pub(crate) mod crit;
#[cfg(all(test, feature = "phobos"))]
mod crit_tests;
pub(crate) mod damage;
pub(crate) mod destruction_effects;
pub(crate) mod detonation_anim;
mod effects;
pub mod electric_bolt;
pub mod fire_coord;
pub mod fire_error;
pub(crate) mod fire_error_world;
pub mod gattling;
pub(crate) mod greatest_threat;
pub mod in_range;
pub(crate) mod inviso_scatter;
pub mod laser;
pub mod line_of_fire;
mod object_health;
pub(crate) mod parasite;
mod projectile_delivery;
pub mod radiation_beam;
mod receiver_call;
mod receiver_health;
mod record_kill;
pub(crate) mod rof;
mod shrapnel;
pub mod smudge_dispatch;
mod techno_death;
mod threat_mask;
pub(crate) use threat_mask::quarry_mask;
mod threat_posed;
pub(crate) mod threat_range;
mod unit_damage_response;
pub mod veterancy;
pub mod world_fire;
pub(crate) mod world_receiver;

#[cfg(test)]
pub(crate) mod receiver_fixture;

#[cfg(test)]
mod aegis_launch_tests;
#[cfg(all(test, feature = "ares"))]
mod ares_armor_tests;
#[cfg(test)]
mod bridge_cluster_tests;
#[cfg(test)]
mod bridge_launch_tests;
#[cfg(test)]
mod bridge_live_chain_tests;
#[cfg(test)]
mod combat_cloak_damage_tests;
#[cfg(test)]
mod combat_cloak_fire_tests;
#[cfg(test)]
mod combat_tests;
#[cfg(test)]
mod combat_turret_facing_tests;
#[cfg(test)]
mod electric_bolt_tests;
#[cfg(test)]
mod fireat_launch_tests;
#[cfg(test)]
mod garrison_oracle_tests;
#[cfg(test)]
mod homing_shipyard_tests;
// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(all(test, not(feature = "phobos")))]
mod ifv_fireat_tests;
/// Ordinary launch-height, resolved impact presentation and damage-effect
/// wiring. Bridge Cell+58 launch and +48/+58 impact comparisons use the native
/// oracle cases in `bridge_launch_tests` and `world::projectile_collision`.
#[cfg(test)]
mod impact_height_tests;
#[cfg(test)]
mod laser_tests;
#[cfg(test)]
mod mirage_disguise_tests;
#[cfg(test)]
mod open_topped_fire_tests;
#[cfg(test)]
mod prone_damage_tests;
#[cfg(test)]
mod radiation_beam_tests;
#[cfg(test)]
mod shrapnel_chain_tests;
#[cfg(test)]
mod screen_shake_tests;
#[cfg(test)]
mod unit_damage_response_tests;
#[cfg(test)]
mod walk_attack_tests;

pub use attack_target::{TarCom, TargetKind};
pub(crate) use attack_target::{object_distance_to, resolve_target_coords, target_present};
pub(crate) use combat_targeting::build_attacker_snapshot;
pub(crate) use detonation_anim::projectile_impact_cell;
pub(crate) use effects::DeathEffects;
pub use effects::{
    CombatLightRequest, ExplosionEffect, ScreenShakeUpdate, SmudgeSpawnRequest,
    TiberiumReductionRequest, UnderAttackEvent, UnitLostEvent,
};
pub use in_range::can_fire_at_target;
pub use in_range::{MaplessTarget, can_fire_at_target_in_query, mapless_in_range};
pub use object_health::{Health, health_category};
pub(crate) use projectile_delivery::{FiredBullet, admit_fired_bullet};
pub use receiver_call::{DamageWriteback, TerrainDamageEvent};
pub(crate) use receiver_call::{
    BaseDefenseResponseCallSite, EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags,
    receiver_divisors, wave_post_object_damage,
};
pub(crate) use receiver_health::BerserkState;
pub(crate) use record_kill::{KillCallback, KillRecord, KillSource};
pub use techno_death::FatalLifecycleStage;
pub(crate) use techno_death::{
    death_announcement_event, death_arm_explodes, death_weapon_detonation, select_death_sound,
};
pub(crate) use threat_posed::live_threat_posed;
/// The threat mask an acquisition callsite pushes into
/// `TechnoClass::Greatest_Threat @ 0x006F8DF0`. Re-exported because the mask
/// is chosen by the mission handlers in `sim/world/`, not inside `combat/`.
pub use threat_range::ScanMission;
pub(crate) use world_fire::estimated_damage_on;

// Shared between combat's children, which reach them through `super::`.
use attack_target::{attack_world_z_leptons, cell_center_coords, target_coords};
use projectile_delivery::{
    ProjectileDelivery, bullet_type, classify_projectile_delivery, projectile_arm_delay,
};
#[cfg(feature = "phobos")]
use receiver_call::resolve_receive_damage_with_input;
use receiver_call::{
    BuildingReceivePrelude, ResolvedReceiveDamage, apply_building_receive_prelude,
    area_near_center_ic_isolation_armed, near_center_ic_isolation_armed,
    postmortem_duration_for_event, receiver_anger_delta, resolve_receive_damage,
};
use shrapnel::emit_projectile_shrapnel;
use techno_death::{ConcreteDeathSmudgePlan, throw_debris_for_death};

#[cfg(any(test, feature = "test-support"))]
pub use attack_target::{
    install_cell_attack_target_for_test, install_entity_attack_target_for_test,
};
#[cfg(any(test, feature = "test-support"))]
use destruction_effects::building_center_smudge_request;
#[cfg(any(test, feature = "test-support"))]
use detonation_anim::impact_z_byte;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use effects::ReceiverStageTrace;
#[cfg(any(test, feature = "test-support"))]
use effects::tiberium_reduction_amount;
#[cfg(any(test, feature = "test-support"))]
use receiver_call::infantry_prone_raw_damage;
#[cfg(test)]
pub(crate) use receiver_fixture::{
    BaseDefenseResponseTraceEntry, FixtureTrace, commit_area_damage_receivers,
    commit_damage_events, emit_projectile_detonations, handle_entity_deaths, resolve_attacker_fire,
    tick_combat, tick_combat_with_fog, tick_combat_with_fog_and_main_rng,
};
#[cfg(any(test, feature = "test-support"))]
use techno_death::fire_death_weapon_payload;

use std::collections::BTreeSet;

use self::combat_targeting::{AttackerSnapshot, GarrisonSnapshot};
use self::combat_weapon::WeaponSlot;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::animation_sequence::SequenceSet;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::{HouseCostFactors, RuleSet};
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::sim::bridge_state::BridgeDamageEvent;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::entity_store::EntityStore;
use crate::sim::house_state::HouseState;
use crate::sim::house_strategy::update_anger_nodes;
use crate::sim::infantry;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::authority::queue_entity_mission_deferred;
use crate::sim::mission::concrete_effects::represented_assign_target;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::object_world_z_leptons;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::overlay_grid::OverlayGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::overlay_grid::WallMutation;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::power_system::PowerState;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectileDetonation, ProjectileGuidance,
    ProjectilePayload, ProjectileSpawn, ProjectileTarget, ProjectileTrajectory,
    ProjectileVisualState, SpecialDetonationAction, SpecialDetonationFlags,
    SpecialDetonationTarget, TargetExpiryPolicy, projectile_next_cluster_coord,
    projectile_random_shrapnel_cell, projectile_shrapnel_count,
    projectile_special_detonation_action,
};
use crate::sim::rng::SimRng;
use crate::sim::terrain_object::TerrainAreaReceiveResult;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::terrain_object::TerrainAreaState;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::vision::FogState;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::world::FireOriginSnapshot;
use crate::sim::world::{SimFireEvent, SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::LEPTONS_PER_LEVEL;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

use super::game_entity::GameEntity;
#[cfg(any(test, feature = "test-support"))]
use super::occupancy::OccupancyGrid;

/// One Unit's post-Foot Facing slot output for this tick — the write half of
/// `UnitClass::Facing_Update @ 0x00736990` plus the `Fire_At_Target @
/// 0x00736DF0` case-2 hull turn, carried from the combat read window to
/// `unit_post::apply_unit_facing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitFacingUpdate {
    pub entity_id: u64,
    /// Turret (`+0x3A0`) destination. `None` means native calls no `Set` on the
    /// turret this frame — the difference between holding an aim and swinging
    /// back to the hull.
    pub turret_destination: Option<u16>,
    /// Apply the native `TurretSpins` arm after the optional arm-A aim Set.
    /// The writer samples the barrel's Current and performs this second Set.
    pub turret_spins: bool,
    /// Hull (`+0x388`) destination. Set by `Fire_At_Target` case 2 when a
    /// TURRETLESS vehicle is refused for facing while stationary, and by the
    /// `Facing_Update` arm-A mid-arc pin.
    pub hull_destination: Option<u16>,
    /// True when `turret_destination` is arm B's idle return (`Set` at
    /// `0x00736BDD`), which native runs AFTER the `+0x6AF` store at
    /// `0x00736B16`; false for arm A's aim `Set` at `0x00736A89`, which runs
    /// before it. `apply_unit_facing` commits the latch between the two.
    pub turret_destination_is_idle_return: bool,
}

impl UnitFacingUpdate {
    fn from_facing_update(
        entity_id: u64,
        update: crate::sim::movement::turret::FacingUpdate,
    ) -> Self {
        Self {
            entity_id,
            turret_destination: update.turret_destination,
            turret_spins: update.turret_spins,
            hull_destination: update.hull_destination,
            turret_destination_is_idle_return: update.turret_destination_is_idle_return,
        }
    }
}

/// TechnoClass vt+0x80, whether an object counts as a vehicle, by its class:
/// a Unit that is not `NonVehicle=` (`0x00746400`, UnitType `+0xE1B`), a
/// building that is a 1x1 `UndeploysInto=` (`0x00457620` → `0x00465D40`,
/// [`ObjectType::is_1x1_with_undeploy`]), an aircraft its vt+0x50 calls low
/// (`0x0041B910`), never an infantryman (`0x004263B0`). `low_flying` is that
/// vt+0x50 in the caller's own map lookup, asked only for an aircraft.
pub(crate) fn vehicle_like(
    class: EntityCategory,
    non_vehicle: bool,
    is_1x1_with_undeploy: bool,
    low_flying: impl FnOnce() -> bool,
) -> bool {
    match class {
        EntityCategory::Unit => !non_vehicle,
        EntityCategory::Structure => is_1x1_with_undeploy,
        EntityCategory::Aircraft => low_flying(),
        EntityCategory::Infantry => false,
    }
}

/// Call-local arguments of Building Update's two native FireAt sites.
/// This value is delivered immediately; no future-shot queue retains it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingShot {
    /// Mission_Attack44B6D0, before its Gattling charge44B6EF.
    Mission { weapon: i32, target: TargetKind },
    /// ProcessDelayedFire450492 uses the slot saved at admission. A
    /// successfully launched bullet consumes the master's support bonus.
    Delayed {
        slot: combat_weapon::WeaponSlot,
        target: TargetKind,
    },
}

impl BuildingShot {
    pub(crate) fn target(self) -> TargetKind {
        match self {
            Self::Mission { target, .. } | Self::Delayed { target, .. } => target,
        }
    }
}

/// What the receiver fixture's combat pass (`world_fire::tick_combat`) did:
/// its observations plus one consuming deferred-consequence packet.
#[cfg(test)]
pub struct CombatTickResult {
    /// Test adapter observations of actual inline AnimStore construction.
    pub(crate) fixture_anims: Vec<receiver_fixture::ConstructedAnimObservation>,
    /// Test observations of bullets already admitted through production owners.
    pub projectile_spawns: Vec<ProjectileSpawn>,
    pub(crate) consequences: crate::sim::world::damage_consequences::DamageConsequences,
}

/// Transient outputs of one firer's fire visit. The fire body and inline
/// receiver boundary collect through one handle; each host consumes its
/// deliveries after committing per-firer effects.
/// Never stored on `Simulation`, serialized or hashed. Hosts destructure all
/// fields so adding an output requires an explicit delivery decision.
#[derive(Default)]
pub(crate) struct CombatEmit {
    /// One receiver-ordered consequence accumulator shared by weapon emission
    /// and fatal damage. Radiation is drained at its earlier ordinary phase.
    pub(crate) effects: DeathEffects,
    /// Native-order ReceiveDamage calls, including raw area records.
    pub(crate) damage_events: Vec<combat_aoe::AreaDamageReceiver>,
    pub(crate) remove_attack: Vec<u64>,
    pub(crate) fire_events: Vec<SimFireEvent>,
    /// Unit Facing outputs committed immediately at each live object slot.
    /// Component fixtures retain them here for observation.
    pub(crate) unit_facing: Vec<UnitFacingUpdate>,
    /// (parent_id, target) — a `Spawner=yes` weapon reached its fire point.
    /// gamemd's `Fire_At` hands the target to the parent's `SpawnManager` and
    /// returns NULL, so no bullet, damage or rearm follows.
    pub(crate) spawn_target_updates: Vec<(u64, TargetKind)>,
    /// (drainer_id, victim_id) — a `DrainWeapon=yes` weapon reached its fire
    /// point against a `Drainable=yes` Techno. `TechnoClass::Fire_At @
    /// 0x006FDF5D..0x006FDF9D` hands the pair to `0x0070FD70` (link install,
    /// gated on the drainer's cell holding the victim) and returns NULL: no
    /// bullet, no damage, no rearm.
    pub(crate) drain_links: Vec<(u64, u64)>,
}

/// Outputs produced by one Bullet Logic slot after its detonation receivers
/// have committed, but before the world retires the Bullet object itself.
pub(crate) struct LogicProjectileCommit {
    pub(crate) effects: DeathEffects,
    pub(crate) under_attack_events: Vec<UnderAttackEvent>,
}

/// `TechnoExt::IsHealthInThreshold` (`Ext/Techno/Body.cpp:752..759`): the
/// interval `(above, below]`, `above` inclusive at zero health; a
/// zero-health, zero-strength object matches unconditionally. A positive
/// health over a zero strength is the native `inf` and misses every finite
/// bound.
#[cfg(feature = "phobos")]
pub(crate) fn health_in_threshold(health: i32, strength: i32, above: f64, below: f64) -> bool {
    if health == 0 && strength == 0 {
        return true;
    }
    let ratio = f64::from(health) / f64::from(strength);
    (if ratio > 0.0 { ratio > above } else { ratio >= above }) && ratio <= below
}

/// `EnumFunctions::CanTargetHouse` (`EnumFunctions.cpp:5..16`): `All` always
/// matches, the owner takes its bucket, a neutral target its Neutral flag, an
/// ally's, otherwise the enemy's. Presentation's DigitalDisplay visibility
/// asks it with the viewer as the target.
#[cfg(feature = "phobos")]
pub fn can_target_house(
    flags: crate::rules::phobos_shields::AffectedHouse,
    source: InternedId,
    target: InternedId,
    houses: &crate::sim::houses::Houses,
    alliances: &crate::map::houses::HouseAllianceMap,
    interner: &StringInterner,
) -> bool {
    use crate::rules::phobos_shields::AffectedHouse;
    if flags == AffectedHouse::ALL {
        return true;
    }
    if source == target {
        return flags.contains(AffectedHouse::OWNER);
    }
    if houses
        .get(&target)
        .is_some_and(|house| house.multiplay_passive())
        && flags.contains(AffectedHouse::NEUTRAL)
    {
        return true;
    }
    if crate::map::houses::is_allied_with(
        alliances,
        interner.resolve(source),
        interner.resolve(target),
    ) {
        flags.contains(AffectedHouse::ALLIES)
    } else {
        flags.contains(AffectedHouse::ENEMIES)
    }
}
