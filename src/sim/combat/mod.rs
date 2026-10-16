//! Combat: what an attacker holds as its target, the shots it fires, the
//! receivers they reach, and what a death leaves behind.
//!
//! - Targets: `attack_target` (the held target and its coordinates),
//!   `combat_targeting`, `greatest_threat`, `threat_range`, `threat_mask` and
//!   `threat_posed` (acquisition), `in_range` and `line_of_fire` (the range
//!   gate), `combat_weapon` (weapon choice), and
//!   `fire_error` and `fire_error_world` (legality).
//! - Firing: `world_fire` (FireAt and the combat tick), `projectile_delivery`
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

pub(crate) mod aim_coord;
mod attack_target;
pub(crate) mod base_defense_response;
pub mod burst;
pub(crate) mod cell_spread;
pub(crate) mod combat_aoe;
pub(crate) mod combat_targeting;
pub(crate) mod combat_weapon;
pub(crate) mod damage;
pub(crate) mod destruction_effects;
pub(crate) mod detonation_anim;
mod effects;
pub(crate) mod electric_bolt;
pub(crate) mod fire_coord;
pub(crate) mod fire_error;
pub(crate) mod fire_error_world;
pub(crate) mod gattling;
pub(crate) mod greatest_threat;
pub(crate) mod in_range;
pub(crate) mod inviso_scatter;
pub(crate) mod laser;
pub(crate) mod line_of_fire;
mod object_health;
pub(crate) mod parasite;
mod projectile_delivery;
mod receiver_call;
mod receiver_health;
mod record_kill;
pub(crate) mod rof;
mod shrapnel;
pub mod smudge_dispatch;
mod techno_death;
mod threat_mask;
mod threat_posed;
pub(crate) mod threat_range;
mod unit_damage_response;
pub(crate) mod veterancy;
pub(crate) mod world_fire;
pub(crate) mod world_receiver;

#[cfg(test)]
pub(crate) mod receiver_fixture;

#[cfg(test)]
mod aegis_launch_tests;
#[cfg(test)]
mod homing_shipyard_tests;
#[cfg(test)]
mod shrapnel_chain_tests;
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
mod combat_cloak_legality_tests;
#[cfg(test)]
mod combat_force_fire_cell_tests;
#[cfg(test)]
mod combat_tests;
#[cfg(test)]
mod combat_turret_facing_tests;
#[cfg(test)]
mod delayed_building_fire_tests;
#[cfg(test)]
mod electric_bolt_tests;
#[cfg(test)]
mod fireat_launch_tests;
#[cfg(test)]
mod garrison_oracle_tests;
#[cfg(test)]
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
mod unit_damage_response_tests;
#[cfg(test)]
mod walk_attack_tests;

pub use attack_target::{TarCom, TargetKind};
pub(crate) use attack_target::{object_distance_to, resolve_target_coords, target_present};
pub(crate) use combat_targeting::{acquire_best_target_for_entity, build_attacker_snapshot};
pub(crate) use detonation_anim::projectile_impact_cell;
pub(crate) use effects::DeathEffects;
pub use effects::{
    CombatLightRequest, ExplosionEffect, SmudgeSpawnRequest, TiberiumReductionRequest,
    UnderAttackEvent, UnitLostEvent,
};
pub(crate) use in_range::{
    can_fire_at_target, can_fire_at_target_in_query, is_within_range_leptons,
    lepton_distance_sq_raw,
};
pub use object_health::{Health, health_category};
pub(crate) use projectile_delivery::{FiredBullet, admit_fired_bullet};
pub use receiver_call::TerrainDamageEvent;
pub(crate) use receiver_call::{
    BaseDefenseResponseCallSite, EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags,
    wave_post_object_damage,
};
pub(crate) use receiver_health::BerserkState;
pub(crate) use record_kill::{KillCallback, KillRecord, KillSource};
pub(crate) use techno_death::{
    FatalLifecycleStage, death_announcement_event, death_arm_explodes, death_weapon_detonation,
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
use receiver_call::{
    BuildingReceivePrelude, ResolvedReceiveDamage, apply_building_receive_prelude,
    area_near_center_ic_isolation_armed, has_active_area_invulnerability,
    near_center_ic_isolation_armed, postmortem_duration_for_event, receiver_anger_delta,
    resolve_receive_damage,
};
use shrapnel::emit_projectile_shrapnel;
use techno_death::{ConcreteDeathSmudgePlan, append_selected_death_sounds, throw_debris_for_death};

#[cfg(test)]
pub(crate) use attack_target::{
    install_cell_attack_target_for_test, install_entity_attack_target_for_test,
};
#[cfg(test)]
use destruction_effects::building_center_smudge_request;
#[cfg(test)]
use detonation_anim::impact_z_byte;
#[cfg(test)]
pub(crate) use effects::ReceiverStageTrace;
#[cfg(test)]
use effects::tiberium_reduction_amount;
#[cfg(test)]
use receiver_call::infantry_prone_raw_damage;
#[cfg(test)]
pub(crate) use receiver_fixture::{
    BaseDefenseResponseTraceEntry, FixtureTrace, commit_area_damage_receivers,
    commit_damage_events, emit_projectile_detonations, handle_entity_deaths, resolve_attacker_fire,
    tick_combat, tick_combat_with_fog, tick_combat_with_fog_and_main_rng,
};
#[cfg(test)]
use techno_death::fire_death_weapon_payload;

use std::collections::{BTreeMap, BTreeSet};

use self::combat_targeting::{AttackerSnapshot, GarrisonSnapshot};
use self::combat_weapon::WeaponSlot;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::animation_sequence::SequenceSet;
use crate::rules::foundation::foundation_dimensions;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::{HouseCostFactors, RuleSet};
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::sim::bridge_state::BridgeDamageEvent;
#[cfg(test)]
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
use crate::sim::overlay_grid::OverlayGrid;
#[cfg(test)]
use crate::sim::overlay_grid::WallMutation;
#[cfg(test)]
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
#[cfg(test)]
use crate::sim::terrain_object::TerrainAreaState;
#[cfg(test)]
use crate::sim::vision::FogState;
use crate::sim::world::{FireOriginSnapshot, SimFireEvent, SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::LEPTONS_PER_LEVEL;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

use super::game_entity::GameEntity;
#[cfg(test)]
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
            hull_destination: update.hull_destination,
            turret_destination_is_idle_return: update.turret_destination_is_idle_return,
        }
    }
}

/// Armor type name → Verses index mapping.
/// Matches the order defined in warhead_type.rs: none(0), flak(1), plate(2),
/// light(3), medium(4), heavy(5), wood(6), steel(7), concrete(8),
/// special_1(9), special_2(10).
const ARMOR_NAMES: &[&str] = &[
    "none",
    "flak",
    "plate",
    "light",
    "medium",
    "heavy",
    "wood",
    "steel",
    "concrete",
    "special_1",
    "special_2",
];

/// Look up the Verses array index for an armor type name.
/// Returns 0 ("none") for unrecognized armor strings.
/// Used by combat_weapon.rs for weapon selection.
pub fn armor_index(armor: &str) -> usize {
    // The names are lowercase, so this matches the lowercased armor string.
    ARMOR_NAMES
        .iter()
        .position(|name| name.eq_ignore_ascii_case(armor))
        .unwrap_or(0)
}

/// Aircraft retain their existing phased Mission_Attack delivery. Other
/// class FireAt callers enter the live receiver inside their own Logic visit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FireRequests {
    pub aircraft: std::collections::BTreeSet<u64>,
}

/// Call-local arguments of Building Update's two native FireAt sites.
/// This value is delivered immediately; no future-shot queue retains it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildingShot {
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

/// Ordinary fire prelude plus one consuming deferred-consequence packet.
/// The frame admits bullets and applies facing before committing the packet at
/// its existing post-SpawnManager boundary.
pub struct CombatTickResult {
    /// Test adapter observations of actual inline AnimStore construction.
    #[cfg(test)]
    pub(crate) fixture_anims: Vec<receiver_fixture::ConstructedAnimObservation>,
    /// Test observations of bullets already admitted through production owners.
    #[cfg(test)]
    pub projectile_spawns: Vec<ProjectileSpawn>,
    /// Facing observations for component combat fixtures. Production Unit
    /// facing commits within that Unit's live Logic slot.
    pub unit_facing: Vec<UnitFacingUpdate>,
    pub(crate) consequences: crate::sim::world::damage_consequences::DamageConsequences,
}

/// Transient outputs shared by live object fire and the remaining class hosts.
/// The fire body and inline receiver boundary collect through one handle;
/// each host consumes its deliveries after committing per-firer effects.
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
