//! Combat system — attack targeting, weapon firing, and damage application.
//!
//! Handles the combat loop: units with an AttackTarget component fire their
//! primary weapon at the target each tick (respecting ROF cooldown). Damage
//! is computed from weapon damage * warhead verses[armor_index]. Entities
//! at 0 health are despawned.
//!
//! ## RA2 damage formula
//! `actual_damage = weapon.damage * warhead.verses[armor_index]`
//! where armor_index is looked up from the target's Armor string.
//!
//! ## Rate of fire
//! ROF in rules.ini is measured in game frames (at 15 fps in original RA2).
//! We convert to simulation ticks using integer math.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components and rules/ (RuleSet).
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

pub(crate) mod aim_coord;
pub(crate) mod base_defense_response;
pub mod burst;
pub(crate) mod cell_spread;
pub(crate) mod combat_aoe;
pub(crate) mod combat_fire_gate;
pub(crate) mod combat_targeting;
pub(crate) mod combat_weapon;
pub(crate) mod damage;
pub(crate) mod destruction_effects;
pub(crate) mod detonation_anim;
pub(crate) mod fire_coord;
pub(crate) mod fire_error;
pub(crate) mod fire_error_world;
#[cfg(test)]
mod garrison_oracle_tests;
pub(crate) mod gattling;
pub(crate) mod greatest_threat;
pub(crate) mod in_range;
pub(crate) mod inviso_scatter;
mod object_health;
#[cfg(test)]
pub(crate) mod receiver_fixture;
mod receiver_health;
#[cfg(test)]
pub(crate) use receiver_fixture::{
    BaseDefenseResponseTraceEntry, FixtureTrace, commit_area_damage_receivers,
    commit_damage_events, emit_projectile_detonations, handle_entity_deaths, resolve_attacker_fire,
    tick_combat, tick_combat_with_fog, tick_combat_with_fog_and_main_rng,
};
pub(crate) mod line_of_fire;
pub(crate) mod laser;
#[cfg(test)]
mod laser_tests;
pub(crate) mod parasite;
pub(crate) mod rof;
pub mod smudge_dispatch;
mod threat_mask;
mod threat_posed;
pub(crate) use threat_posed::live_threat_posed;
pub(crate) mod threat_range;
pub(crate) mod veterancy;
pub(crate) mod world_receiver;

#[cfg(test)]
#[path = "combat_tests.rs"]
mod combat_tests;

#[cfg(test)]
#[path = "combat_force_fire_cell_tests.rs"]
mod combat_force_fire_cell_tests;

#[cfg(test)]
#[path = "combat_pursuit_tests.rs"]
mod combat_pursuit_tests;

#[cfg(test)]
#[path = "combat_turret_facing_tests.rs"]
mod combat_turret_facing_tests;

#[cfg(test)]
#[path = "combat_cloak_fire_tests.rs"]
mod combat_cloak_fire_tests;

#[cfg(test)]
#[path = "combat_cloak_legality_tests.rs"]
mod combat_cloak_legality_tests;

#[cfg(test)]
#[path = "combat_cloak_damage_tests.rs"]
mod combat_cloak_damage_tests;

#[cfg(test)]
mod mirage_disguise_tests;

#[cfg(test)]
#[path = "delayed_building_fire_tests.rs"]
mod delayed_building_fire_tests;

#[cfg(test)]
#[path = "prone_damage_tests.rs"]
mod prone_damage_tests;

#[cfg(test)]
mod bridge_cluster_tests;
#[cfg(test)]
mod bridge_launch_tests;
#[cfg(test)]
mod bridge_live_chain_tests;
#[cfg(test)]
#[path = "fireat_launch_tests.rs"]
mod fireat_launch_tests;

#[cfg(test)]
mod ifv_fireat_tests;
#[cfg(test)]
#[path = "open_topped_fire_tests.rs"]
mod open_topped_fire_tests;

use std::collections::{BTreeMap, BTreeSet};

use self::combat_weapon::{WeaponSlot, select_weapon_against};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
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
use crate::sim::vision::FogState;
use crate::sim::world::{FireOriginSnapshot, SimFireEvent, SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::LEPTONS_PER_LEVEL;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

use super::game_entity::GameEntity;
use super::occupancy::OccupancyGrid;
use crate::rules::animation_sequence::SequenceSet;
use crate::rules::foundation::foundation_dimensions;

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

/// Fire 468A49 dispatches target WhatAmI (+2C); Aircraft's 41C180
/// returns 2 and selects zero Arm independently of current altitude/layer.
fn projectile_arm_delay(arm: i32, target: ProjectileTarget, entities: &EntityStore) -> i32 {
    if matches!(target, ProjectileTarget::Entity(id)
        if entities.get(id).is_some_and(|entity| entity.category == EntityCategory::Aircraft))
    {
        0
    } else {
        arm
    }
}

/// The BulletType facts FireAt and `BulletClass::Fire` read for one shot.
/// Every shot is a bullet; an `Inviso=` one is placed at its target and
/// detonates on its first AI visit (`Projectile` `fire_inviso`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectileDelivery {
    arm_frames: i32,
    tracks_target: bool,
    collision: ProjectileCollisionPolicy,
    ballistic: bool,
    /// `Vertical=` (`BulletTypeClass+0x2C0`) selects the third
    /// `BulletClass::AI` arm. Carries `DetonationAltitude=` (`+0x2BC`).
    vertical: Option<i32>,
    /// `BulletTypeClass::Acceleration` (`+0x2D0`), constructor default 3.
    acceleration: i32,
    /// `Inaccurate= && Arcing=` — the launch-time scatter gate at
    /// `TechnoClass::FireAt 0x006FE67D`/`0x006FE68B`. `Some(true)` takes
    /// the range-scaled flak arm, `Some(false)` the plain arm.
    launch_scatter_is_flak: Option<bool>,
    guidance: Option<ProjectileGuidance>,
    /// `Inviso=` (`+0x29E`): `BulletClass::Fire` places the bullet on its
    /// target with no speed (`0x004688B7..0x00468A39`).
    inviso: bool,
}

/// VERA-internal: a weapon naming no BulletType, or one that does not
/// resolve, never occurs in retail rules (native would dereference NULL at
/// `0x006FE55D`). Such a shot is fired as a default BulletType with
/// `Inviso=yes`, so fixtures land in the firing frame like the small-arms shots
/// they stand for; its detonation takes no scatter or cluster draws.
fn missing_projectile_fallback() -> &'static crate::rules::projectile_type::ProjectileType {
    static FALLBACK: std::sync::OnceLock<crate::rules::projectile_type::ProjectileType> =
        std::sync::OnceLock::new();
    FALLBACK.get_or_init(|| {
        let ini = crate::rules::ini_parser::IniFile::from_str("[MissingBulletType]\nInviso=yes\n");
        crate::rules::projectile_type::ProjectileType::from_ini_section(
            "MissingBulletType",
            ini.section("MissingBulletType").expect("fallback section"),
            None,
        )
    })
}

/// The BulletType facts a weapon's shot is fired with, and its flight arm.
///
/// gamemd-derived: `BulletClass::AI @ 0x004666E0` has exactly two branches,
/// keyed on `ROT < 1`; the non-homing arm then splits on `Vertical` (`+0x2C0`).
/// `Arcing`, `SubjectToCliffs`, `SubjectToElevation`, `SubjectToWalls`,
/// `Proximity`, `FlakScatter`, `Inviso` and `Cluster` never select an arm —
/// they are launch-time, collision-probe or detonation-time keys — so only
/// `ROT` and `Vertical` pick a flight model.
///
/// Evidence-backed exclusions (verified 2026-09-03, exhaustive over the direct
/// `[base + displacement]` operand forms via `search_instructions`) — these keys
/// must NOT divert a shot off authoritative flight, because native never reads
/// them in the flight loop at all:
/// - `Bouncy=` (`+0x2A7`) has **no consumer anywhere in the binary**. The
///   apparent hits at `0x0070D2D2`/`0x0070D2EE` are `TechnoTypeClass+0x2A7`
///   behind `VeterancyClass::IsVeteran/IsElite`. Stock `[Lobbed]` (the dog
///   discus) is therefore an ordinary `Arcing` shell.
/// - `Proximity=` (`+0x29F`) likewise has no consumer; `0x00702BC0`ff. are
///   `TechnoTypeClass+0x29F` behind the same veterancy pattern.
/// - `SubjectToCliffs=` (`+0x296`) is consumed only by the AI collision probe
///   `FUN_00468BB0 @ 0x00468BEC`, and `SubjectToElevation=` (`+0x297`) only by
///   `TechnoClass::InRange @ 0x006F72EF`/`0x006F7459` — a targeting key, not a
///   flight key.
/// - `VeryHigh=` (`+0x299`) is read only as a `HomingTrack` argument on the
///   `ROT >= 1` arm, so `VeryHigh` with `ROT <= 0` is unreachable; the one
///   stock user `[ChemMissile]` has `ROT=4`.
/// - `Degenerates=` (`+0x2A6`) IS live code — `BulletClass::AI 0x00467C86`
///   decrements the bullet's damage on every non-detonating frame while it
///   exceeds 5 — but stock `rulesmd.ini` has zero users, so it is recorded and
///   not implemented.
/// - `Elasticity=` (`+0x2C8`) is live in the ARM B reflection block but has
///   zero stock projectile users (the `[PIECE]`/`[TIRE]` hits are VoxelAnims).
///
/// RESIDUAL (GSI-08.07) — the second scatter site is not ported.
/// `BulletClass::Fire @ 0x0046874E..0x004688A9` offsets an
/// `Inviso && FlakScatter` bullet's placement before the Inviso body
/// ([`crate::sim::projectile::ProjectileStore::fire_inviso`]): magnitude
/// `(RandomRanged(0, RulesClass+0x1734 << 1) * ftol(dist)) / Range`, angle
/// `RandomRanged(0, 0x7FFFFFFE)`, then `x += cos*mag`, `y -= sin*mag`
/// (`0x00468864..0x00468890`, the launch site's shape at
/// `0x006FE7E5`/`0x006FE7C0`; the decompiler drops the `FIADD`). The divisor
/// is the weapon's `Range=` in leptons: `Bullet+0x130` is the WeaponType
/// FireAt installs through `SetWeaponType @ 0x0046B260` (`0x006FE573`), and
/// WeaponType `+0xB4` is `Range=` (`ReadRange` at `0x00772336`). The x87
/// distance, magnitude and angle conversion want a native oracle first.
///
/// Trigger: every Flak Cannon / Flak Track shot at an aircraft (`[FlakProj]`,
/// 6 weapons). Player effect: flak never misses. Frequency: any skirmish with
/// air units. Downstream risk: two Scenario RNG draws are missing per shot.
///
/// Ordinary `ROT < 1, Vertical = no` AI subtracts gravity every visit
/// (467402..467429), independently of `Arcing`. Production gives all such
/// persistent shots the ordinary gravity/collision arm. Scalar FireAt math
/// retains binary64 velocity; upstream FLH/pivot and homing producers remain
/// explicitly bounded in their owners.
/// The BulletType a shot of `weapon` carries: its `Projectile=`, or
/// [`missing_projectile_fallback`] for one naming none.
fn bullet_type<'r>(
    weapon: &crate::rules::weapon_type::WeaponType,
    rules: &'r RuleSet,
) -> &'r crate::rules::projectile_type::ProjectileType {
    weapon
        .projectile
        .as_deref()
        .and_then(|projectile_id| rules.projectile(projectile_id))
        .unwrap_or_else(|| missing_projectile_fallback())
}

fn classify_projectile_delivery(
    weapon: &crate::rules::weapon_type::WeaponType,
    rules: &RuleSet,
) -> ProjectileDelivery {
    let projectile = bullet_type(weapon, rules);
    // `BulletClass::AI @ 0x004666E0` selects an arm exactly twice: `ROT < 1` at
    // `0x004668D1`, then `Vertical` (`+0x2C0`) at `0x004671D0`. Nothing else
    // participates.
    let ballistic = projectile.arcing;
    ProjectileDelivery {
        inviso: projectile.inviso,
        arm_frames: projectile.arm,
        tracks_target: projectile.rot > 0,
        collision: ProjectileCollisionPolicy {
            level: projectile.level,
            subject_to_walls: projectile.subject_to_walls,
            native_cell_collision: projectile.rot <= 0 && !projectile.vertical,
            dropping: projectile.dropping,
            subject_to_cliffs: projectile.subject_to_cliffs,
            flak_scatter: projectile.flak_scatter,
            anti_air: projectile.aa,
            airburst: projectile.airburst,
            inaccurate: projectile.inaccurate,
            floater: projectile.floater,
            elasticity_bits: projectile.elasticity.to_bits(),
            arcing: projectile.arcing,
        },
        ballistic,
        vertical: projectile
            .vertical
            .then_some(projectile.detonation_altitude),
        acceleration: projectile.acceleration,
        // `0x006FE67D`/`0x006FE68B`: the outer gate is `Inaccurate && Arcing`.
        // Inside, `FlakScatter && !Inviso` takes the range-scaled arm at
        // `0x006FE6AD` and everything else the plain arm at `0x006FE7FE`.
        launch_scatter_is_flak: (projectile.inaccurate && projectile.arcing)
            .then_some(projectile.flak_scatter && !projectile.inviso),
        guidance: (projectile.rot > 0).then_some(ProjectileGuidance {
            rot: projectile.rot,
            missile_rot_var: crate::util::native_x87::NativeF64Bits::from_bits(
                rules.general.missile_rot_var.to_bits(),
            ),
            course_lock_duration: projectile.course_lock_duration,
            course_frames: 0,
            course_locked: true,
            very_high: projectile.very_high,
            max_speed: weapon.speed,
            acceleration: projectile.acceleration,
            // Replaced at construction with the launch-time target coord.
            fuse_reference: ProjectileCoord::new(0, 0, 0),
            closing_frames: 0,
            closing_accumulator_bits: 0,
        }),
    }
}

/// One bullet as `BulletClass::Construct @ 0x004664C0` builds it and its
/// launch, `BulletClass::Fire @ 0x00468670` (vt+0x1F0), places it, whoever
/// fires it: FireAt, a silo's nuclear missile (`0x0044CAED`), the warhead
/// `NukeMaker=` drops (`0x0046B408`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct FiredBullet {
    /// The AbstractClass identity the constructor took.
    native_unique_id: i32,
    /// Construct's owner (`+0xB0`).
    source_id: u64,
    /// Construct's target (`+0x10C`).
    target: ProjectileTarget,
    /// The target coordinate Fire keeps (`+0x140`), also a guided bullet's
    /// fuse reference.
    target_position: ProjectileCoord,
    payload: ProjectilePayload,
    /// Construct's speed (`+0x110`): the ceiling of a Vertical or guided
    /// flight.
    max_speed: i32,
    /// Fire's coordinate and velocity.
    origin: ProjectileCoord,
    velocity: crate::sim::projectile::ProjectileVelocity,
    /// The current speed.
    speed: u16,
}

impl FiredBullet {
    /// A bullet `BulletClass::Fire` launches from `origin` at `velocity` and
    /// that moves at the velocity's magnitude, as the silo's missile and
    /// NukeMaker's warhead do (FireAt keeps its own launch speed and its own
    /// read of the aim). Fire keeps the target's vt+0x58 (`0x00468707`, into
    /// `+0x140`): a cell's aim, raised over a structural bridge deck
    /// ([`cell_target_coord`]), the shared dummy cell's, or an object's
    /// GetCoords (Object vt+0x58 forwards vt+0x48). `None` for a missing
    /// target, which native dereferences.
    ///
    /// [`cell_target_coord`]: crate::sim::projectile::cell_target_coord
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn launched(
        world: &Simulation,
        native_unique_id: i32,
        source_id: u64,
        target: ProjectileTarget,
        payload: ProjectilePayload,
        max_speed: i32,
        origin: ProjectileCoord,
        velocity: crate::sim::projectile::ProjectileVelocity,
    ) -> Option<Self> {
        let terrain = world.resolved_terrain.as_ref();
        let target_position = match target {
            ProjectileTarget::Cell { rx, ry } => {
                crate::sim::projectile::cell_target_coord(terrain, rx, ry)
            }
            ProjectileTarget::DummyCell => crate::sim::projectile::dummy_cell_target_coord(
                &world.effective_shared_cell_dummy(),
            ),
            ProjectileTarget::Entity(id) => {
                let coords = crate::sim::movement::ground_pose::object_get_coords(
                    world.substrate.entities.get(id)?,
                    terrain,
                );
                ProjectileCoord::new(coords.x, coords.y, coords.z)
            }
            ProjectileTarget::None => return None,
        };
        Some(Self {
            native_unique_id,
            source_id,
            target,
            target_position,
            payload,
            max_speed,
            origin,
            velocity,
            speed: crate::sim::projectile::projectile_velocity_magnitude(velocity)
                .clamp(0.0, f64::from(u16::MAX)) as u16,
        })
    }
}

impl ProjectileDelivery {
    /// The spawn of `bullet`, a bullet of this delivery's BulletType
    /// (`projectile_type`, absent for a weapon naming none).
    fn spawn(
        &self,
        entities: &EntityStore,
        rules: &RuleSet,
        projectile_type: Option<&crate::rules::projectile_type::ProjectileType>,
        bullet: FiredBullet,
    ) -> ProjectileSpawn {
        let guidance = self.guidance.map(|guidance| ProjectileGuidance {
            fuse_reference: bullet.target_position,
            max_speed: bullet.max_speed,
            ..guidance
        });
        ProjectileSpawn {
            native_unique_id: bullet.native_unique_id,
            line_trail: projectile_type.and_then(|kind| {
                crate::sim::projectile::ProjectileLineTrail::from_type(
                    kind,
                    rules.general.line_trail_color_override,
                )
            }),
            flat: projectile_type.is_some_and(|projectile| projectile.flat),
            source_id: bullet.source_id,
            origin: bullet.origin,
            target: bullet.target,
            initial_target_position: bullet.target_position,
            payload: bullet.payload,
            speed_leptons_per_frame: bullet.speed,
            velocity: bullet.velocity,
            trajectory: match self.vertical {
                Some(detonation_altitude) => ProjectileTrajectory::Vertical {
                    detonation_altitude,
                    acceleration: self.acceleration,
                    max_speed: bullet.max_speed,
                },
                None if self.ballistic || guidance.is_none() => ProjectileTrajectory::Ballistic,
                None => ProjectileTrajectory::Straight,
            },
            guidance,
            visual: projectile_type.map_or(ProjectileVisualState::new(0, 0, 0), |projectile| {
                ProjectileVisualState::new(
                    projectile.anim_low as u8,
                    projectile.anim_high as u8,
                    projectile.anim_rate as u8,
                )
            }),
            arm_frames: projectile_arm_delay(self.arm_frames, bullet.target, entities),
            fuse_frames: None,
            // AI 467C0C calls Check for ROT>0 or Ranged even when
            // Dropping later suppresses detector-only admission.
            ranged_fuse: self.tracks_target
                || projectile_type.is_some_and(|projectile| projectile.ranged),
            tracks_target: self.tracks_target,
            target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
            collision: self.collision,
        }
    }
}

/// Admit `bullet`, fired with `weapon`'s BulletType, as `bullet_id`. Fire
/// Unlimbos it at its coordinate, which appends it to the Logic vector
/// (`0x005F5040`): it takes its first AI later in this frame's pass. An
/// `Inviso=` bullet then stands on its target ([`ProjectileStore::fire_inviso`]
/// with the target's OnBridge, as FireAt copies it at `0x006FF08B`).
///
/// [`ProjectileStore::fire_inviso`]: crate::sim::projectile::ProjectileStore::fire_inviso
pub(crate) fn admit_fired_bullet(
    world: &mut Simulation,
    rules: &RuleSet,
    weapon: &WeaponType,
    bullet_id: u64,
    bullet: FiredBullet,
) {
    let projectile_type = weapon
        .projectile
        .as_deref()
        .and_then(|projectile_id| rules.projectile(projectile_id));
    let delivery = classify_projectile_delivery(weapon, rules);
    let spawn = delivery.spawn(&world.substrate.entities, rules, projectile_type, bullet);
    world.admit_projectile(bullet_id, spawn);
    world.construct_bullet_scheme(bullet_id, rules);
    #[cfg(test)]
    if let Some(fixture) = world.receiver_fixture.as_mut() {
        fixture
            .admitted_spawns
            .push((bullet_id, delivery.inviso, spawn));
    }
    if delivery.inviso {
        let on_bridge = match bullet.target {
            ProjectileTarget::Entity(id) => world
                .substrate
                .entities
                .get(id)
                .is_some_and(|target| target.on_bridge),
            _ => false,
        };
        world
            .projectiles
            .fire_inviso(bullet_id, bullet.target_position, on_bridge);
    }
}

#[cfg(test)]
mod projectile_delivery_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// `BulletClass::AI @ 0x004666E0` branches only on `ROT < 1` and, on the
    /// non-homing arm, on `Vertical`. `Proximity`, `SubjectToCliffs` and
    /// `SubjectToElevation` are never read there, so none of them may push a
    /// shot off authoritative flight.
    #[test]
    fn gsi_08_07_proximity_and_cliff_keys_keep_authoritative_flight() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=PROXER\n1=CLIFFER\n[PROXER]\nStrength=100\nArmor=heavy\nPrimary=ProxGun\n[CLIFFER]\nStrength=100\nArmor=heavy\nPrimary=CliffGun\n[ProxGun]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=Prox\nWarhead=WH\n[CliffGun]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=Cliffy\nWarhead=WH\n[Prox]\nProximity=yes\nROT=8\n[Cliffy]\nSubjectToCliffs=yes\nSubjectToElevation=yes\nROT=0\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile fixture parses");

        // Only ROT picks the homing arm; Proximity= and SubjectToCliffs= pick
        // no flight arm at all.
        for (weapon_name, homing) in [("ProxGun", true), ("CliffGun", false)] {
            let weapon = rules.weapon(weapon_name).expect("weapon");
            let delivery = classify_projectile_delivery(weapon, &rules);
            assert_eq!(delivery.tracks_target, homing, "{weapon_name}");
            assert_eq!(delivery.vertical, None, "{weapon_name}");
            assert!(!delivery.inviso, "{weapon_name}");
        }
    }

    #[test]
    fn wall_projectile_uses_the_authoritative_collision_path() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n\n[TEST]\nStrength=100\nArmor=heavy\nPrimary=GUN\n\n[GUN]\nDamage=20\nROF=10\nRange=5\nSpeed=30\nProjectile=SHELL\nWarhead=WH\n\n[SHELL]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile rules");
        let weapon = rules.weapon("GUN").expect("weapon");

        assert_eq!(
            classify_projectile_delivery(weapon, &rules),
            ProjectileDelivery {
                inviso: false,
                arm_frames: 0,
                tracks_target: false,
                collision: ProjectileCollisionPolicy {
                    level: false,
                    subject_to_walls: true,
                    native_cell_collision: true,
                    ..ProjectileCollisionPolicy::NONE
                },
                ballistic: false,
                vertical: None,
                acceleration: 3,
                launch_scatter_is_flak: None,
                guidance: None,
            }
        );
    }

    /// `Bouncy=` (`+0x2A7`), `Proximity=` (`+0x29F`) and `Degenerates=`
    /// (`+0x2A6`) never divert a shot: the first two have no consumer anywhere
    /// in `gamemd.exe`, and the third is a damage decay inside
    /// `BulletClass::AI 0x00467C86`, not a trajectory selector. `Inaccurate=`
    /// and `FlakScatter=` are launch-time target offsets at
    /// `TechnoClass::FireAt 0x006FE67D`, and `Dropping=` (`+0x29C`) only
    /// suppresses the proximity fuse at `0x00467C78`.
    #[test]
    fn gsi_08_08_dead_and_launch_time_keys_keep_authoritative_flight() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=TA\n1=TB\n\
             [TA]\nStrength=100\nArmor=heavy\nPrimary=W0\nSecondary=W1\n\
             [TB]\nStrength=100\nArmor=heavy\nPrimary=W2\nSecondary=W3\n\
             [W0]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P0\nWarhead=WH\n\
             [W1]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P1\nWarhead=WH\n\
             [W2]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P2\nWarhead=WH\n\
             [W3]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=P3\nWarhead=WH\n\
             [P0]\nBouncy=yes\nArcing=yes\n[P1]\nDegenerates=yes\n\
             [P2]\nInaccurate=yes\nFlakScatter=yes\nArcing=true\n[P3]\nDropping=yes\nROT=4\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile fixture parses");
        // Bouncy=, Degenerates=, Inaccurate=/FlakScatter= and Dropping= select
        // no flight arm: ROT alone does.
        for (weapon_name, homing) in [("W0", false), ("W1", false), ("W2", false), ("W3", true)] {
            let weapon = rules.weapon(weapon_name).expect("weapon");
            let delivery = classify_projectile_delivery(weapon, &rules);
            assert_eq!(delivery.tracks_target, homing, "{weapon_name}");
            assert_eq!(delivery.vertical, None, "{weapon_name}");
        }
        // The flak arm is only selected when `FlakScatter && !Inviso`.
        let flak = rules.weapon("W2").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(flak, &rules),
            ProjectileDelivery {
                launch_scatter_is_flak: Some(true),
                ..
            }
        ));
        // `Bouncy` + `Arcing` is an ordinary ballistic shell, with no scatter.
        let bouncy = rules.weapon("W0").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(bouncy, &rules),
            ProjectileDelivery {
                ballistic: true,
                launch_scatter_is_flak: None,
                ..
            }
        ));
    }

    /// `Vertical=` selects the third `BulletClass::AI` arm and carries
    /// `DetonationAltitude=` (`+0x2BC`) and `Acceleration=` (`+0x2D0`).
    #[test]
    fn gsi_08_08_vertical_projectile_takes_the_vertical_arm() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=T\n[T]\nStrength=100\nArmor=heavy\nPrimary=NUKE\n\
             [NUKE]\nDamage=10\nROF=20\nRange=5\nSpeed=40\nProjectile=GiantNukeUp\nWarhead=WH\n\
             [GiantNukeUp]\nArm=2\nAcceleration=1\nVertical=yes\nDetonationAltitude=20000\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("projectile fixture parses");
        let weapon = rules.weapon("NUKE").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(weapon, &rules),
            ProjectileDelivery {
                vertical: Some(20000),
                acceleration: 1,
                arm_frames: 2,
                guidance: None,
                ballistic: false,
                ..
            }
        ));
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
    let lower: String = armor.to_ascii_lowercase();
    ARMOR_NAMES.iter().position(|&a| a == lower).unwrap_or(0)
}

/// Return the active wall-overlay flags at a cell, if available.
fn wall_overlay_flags_at<'a>(
    overlay_grid: Option<&OverlayGrid>,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    rx: u16,
    ry: u16,
) -> Option<&'a crate::rules::overlay_types::OverlayTypeFlags> {
    let (Some(grid), Some(registry)) = (overlay_grid, overlay_registry) else {
        return None;
    };
    grid.cell(rx, ry)
        .overlay_id
        .and_then(|id| registry.flags(id))
        .filter(|flags| flags.wall)
}

fn warhead_damages_wall(
    warhead: &WarheadType,
    wall_flags: &crate::rules::overlay_types::OverlayTypeFlags,
) -> bool {
    warhead.wall || warhead.wall_absolute_destroyer || (warhead.wood && wall_flags.armor_is_wood)
}

/// InfantryClass::ReceiveDamage's prone head (`0x00517FC1..0x00517FEF`): a
/// prone infantryman's positive raw damage, unless defenses are ignored,
/// becomes `ftol(fild damage * fmul qword [warhead+0xF8])`, at least 1.
/// Math__ftol (`0x007C5F00`) stores a qword and the head keeps EAX, so a
/// product beyond 32 bits wraps before the clamp, and an infinite product
/// (`1e39%`) converts to the indefinite qword, whose low dword 0 the clamp
/// makes 1. Native rows: tools/rules_oracle/read_double_percent (group B).
fn infantry_prone_raw_damage(
    target: &GameEntity,
    warhead: &WarheadType,
    damage: i32,
    ignore_defenses: bool,
) -> i32 {
    use crate::util::native_x87::MaskedX87Chop53 as X87;
    if target.category != EntityCategory::Infantry
        || !infantry::is_prone_for_damage(target)
        || damage <= 0
        || ignore_defenses
    {
        return damage;
    }
    let multiplier = X87::load_f64(NativeF64Bits::from_bits(warhead.prone_damage_f64.to_bits()));
    X87::ftol_i32_low_masked(X87::mul(X87::load_i32(damage), multiplier)).max(1)
}

/// InfantryClass::ReceiveDamage's next head (`0x00517FF5..0x00518010`): an
/// `InfDeath=9` warhead (`+0x120`) does no damage to an infantryman above the
/// ground or its deck (GetHeight, vt+0x1C8, `ObjectClass::GetHeight @
/// 0x005F5F40`), such as a paratrooper in its fall or a Rocketeer in flight.
/// Retail's two InfDeath 9 warheads are the Genetic Mutator's.
fn infantry_spared_mutation(
    target: &GameEntity,
    warhead: &WarheadType,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    const MUTATE_INF_DEATH: u8 = 9;
    target.category == EntityCategory::Infantry
        && warhead.inf_death == MUTATE_INF_DEATH
        && crate::sim::movement::air_movement::current_fly_height(target, terrain) > 0
}

/// What an `AttackTarget` is pointing at — an entity or a ground cell.
///
/// Force-fire on empty terrain (Ctrl + click cell) sets the `Cell` variant.
/// Auto-acquired and explicit attack-on-unit orders set `Entity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TargetKind {
    /// Entity-targeted attack (normal Attack / ForceAttack on a unit/building).
    Entity(u64),
    /// Ground-targeted attack (force-fire on a cell). Cell coord in map space.
    Cell(u16, u16),
}

impl From<crate::sim::components::NavTargetRef> for TargetKind {
    /// A NavCom (`Foot+0x5A4`) as a target: its cell, or the object it names.
    fn from(target: crate::sim::components::NavTargetRef) -> Self {
        use crate::sim::components::NavTargetRef;
        match target {
            NavTargetRef::Cell { rx, ry } => Self::Cell(rx, ry),
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id } => Self::Entity(id),
        }
    }
}

impl From<TargetKind> for crate::sim::components::NavTargetRef {
    /// A target as a destination (a Target handed to `Assign_Destination`).
    fn from(target: TargetKind) -> Self {
        match target {
            TargetKind::Entity(id) => Self::Entity { id },
            TargetKind::Cell(rx, ry) => Self::cell(rx, ry),
        }
    }
}

impl TargetKind {
    /// Recover an already-retained native Cell allocation without a map lookup.
    /// A Cell TarCom is a pointer: later queries may have changed Dummy.coord,
    /// so reading the target must not restamp it with the command coordinate.
    /// GetFireError6FC197 and CanFireAt6F77B0 share this identity authority.
    pub(crate) fn cell_identity(
        self,
        terrain: &ResolvedTerrainGrid,
    ) -> Option<crate::map::cell_index::NativeCellIdentity> {
        let Self::Cell(x, y) = self else {
            return None;
        };
        Some(terrain.native_fixed_cell_index(x as i16, y as i16).map_or(
            crate::map::cell_index::NativeCellIdentity::Dummy,
            crate::map::cell_index::NativeCellIdentity::Real,
        ))
    }
}

/// Sentinel attacker id for sourceless damage (the radiation field). Stable
/// entity ids start at 1, so 0 is never a live attacker; the receiver asks no
/// retaliation for it.
pub(crate) const RAD_NO_ATTACKER: u64 = 0;

/// The two receiver booleans carried by one native concrete `ReceiveDamage`
/// call. Their semantic names are intentionally limited to the verified ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReceiverCallFlags {
    pub(crate) ignore_defenses: bool,
    pub(crate) arg6: bool,
}

/// One ordered damage call. Area and direct-receiver records retain the raw
/// signed damage, native lepton distance, and concrete receiver flags until
/// dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntityDamageEvent {
    pub(crate) target_id: u64,
    pub(crate) damage: i32,
    pub(crate) attacker_id: u64,
    /// ReceiveDamage's sourceHouse ABI argument captured when the detonation
    /// enters Apply_area_damage. This remains valid if the source object is
    /// uninitialized by an earlier ordered receiver record.
    pub(crate) source_house: Option<InternedId>,
    pub(crate) warhead_ref: InternedId,
    pub(crate) distance_leptons: i32,
    pub(crate) receiver_flags: ReceiverCallFlags,
    /// This record belongs to an Apply_area_damage transaction whose captured
    /// CellSpread is at most 0.5. The receiver commit uses this transient fact
    /// to reproduce the native near-center Iron Curtain isolation scan; it is
    /// deliberately false for direct-receiver calls.
    pub(crate) near_center_ic_isolation_eligible: bool,
}

impl EntityDamageEvent {
    pub(crate) fn area(
        target_id: u64,
        raw_damage: i32,
        distance_leptons: i32,
        attacker_id: u64,
        source_house: Option<InternedId>,
        warhead_ref: InternedId,
    ) -> Self {
        Self {
            target_id,
            damage: raw_damage,
            attacker_id,
            source_house,
            warhead_ref,
            distance_leptons,
            receiver_flags: ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
            near_center_ic_isolation_eligible: false,
        }
    }

    pub(crate) fn direct_receiver(
        target_id: u64,
        raw_damage: i32,
        distance_leptons: i32,
        attacker_id: u64,
        source_house: Option<InternedId>,
        warhead_ref: InternedId,
        receiver_flags: ReceiverCallFlags,
    ) -> Self {
        Self {
            target_id,
            damage: raw_damage,
            attacker_id,
            source_house,
            warhead_ref,
            distance_leptons,
            receiver_flags,
            near_center_ic_isolation_eligible: false,
        }
    }
}

/// Component: this entity is attacking a specific target.
///
/// Installed by the concrete target authority for entity or cell targets.
/// The combat system fires the
/// attacker's weapon at the resolved target each tick. The reload between
/// shots is the object's own `GameEntity::rearm_timer`, not the target's.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttackTarget {
    /// What this attacker is firing at: an entity or a ground cell (force-fire).
    pub target: TargetKind,
}

/// Infantry52083E..520904: deployment uses actual Doing27..30; secondary
/// action availability is its signed native count, never a discharge-frame
/// comparison. Jumpjet's earlier class arm is resolved by the live caller.
fn infantry_fire_action(
    sequences: Option<&SequenceSet>,
    weapon_index: i32,
    is_prone: bool,
    deploying: bool,
) -> i32 {
    if deploying {
        return 29;
    }
    let has = |action| {
        sequences
            .and_then(|set| set.infantry_action(action))
            .is_some_and(|record| record.frames_per_facing != 0)
    };
    if weapon_index != 0 {
        if is_prone && has(41) {
            return 41;
        }
        if has(40) {
            return 40;
        }
    }
    if is_prone { 8 } else { 4 }
}

/// Infantry52094C..5209A0: signed live type fields; a deployed action does
/// not imply prone. An absent secondary sequence falls back to the matching
/// primary discharge frame, independently of action admission.
fn infantry_fire_frame(
    obj: &ObjectType,
    sequences: Option<&SequenceSet>,
    weapon_index: i32,
    is_prone: bool,
) -> i32 {
    let mut frame = if is_prone {
        obj.fire_prone_frame as i32
    } else {
        obj.fire_up_frame as i32
    };
    if weapon_index != 0 {
        let action = if is_prone { 41 } else { 40 };
        if sequences
            .and_then(|set| set.infantry_action(action))
            .is_some_and(|record| record.frames_per_facing != 0)
        {
            frame = if is_prone {
                obj.secondary_prone_frame as i32
            } else {
                obj.secondary_fire_frame as i32
            };
        }
    }
    frame
}

impl AttackTarget {
    /// Entity-targeted attack: fire at a specific entity by stable ID.
    pub fn new(target_stable_id: u64) -> Self {
        Self {
            target: TargetKind::Entity(target_stable_id),
        }
    }

    /// Ground-targeted attack: fire at a specific cell coord (force-fire on terrain).
    pub fn for_cell(rx: u16, ry: u16) -> Self {
        Self {
            target: TargetKind::Cell(rx, ry),
        }
    }
}

/// An entity's GetCoords XY ([`object_center_xy`]) as a cell and in-cell
/// leptons: a structure's foundation centre, any other object's Location.
/// Callers which consume stored Location rather than this virtual point keep
/// that distinction.
///
/// [`object_center_xy`]: crate::sim::movement::ground_pose::object_center_xy
fn target_coords(entity: &GameEntity) -> (u16, u16, SimFixed, SimFixed) {
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    (
        (x / 256) as u16,
        (y / 256) as u16,
        SimFixed::from_num(x % 256),
        SimFixed::from_num(y % 256),
    )
}

/// Compute lepton-precise coordinates for a cell target (force-fire on terrain).
///
/// Cell-center convention: leptons = `cell_index * 256 + 128`. Returns the
/// shape `target_coords` returns for entities (rx, ry, sub_x, sub_y) so
/// callers can branch on `TargetKind` and feed the result into the same
/// projectile-spawn pipeline.
fn cell_center_coords(rx: u16, ry: u16) -> (u16, u16, SimFixed, SimFixed) {
    (rx, ry, SimFixed::from_num(128), SimFixed::from_num(128))
}

/// Resolve target coords from a `TargetKind`, looking up entity position when
/// needed and using cell-center for `Cell` targets.
///
/// Returns `None` if the target is `Entity(id)` and the entity no longer
/// exists (despawned). `Cell` targets always resolve.
///
/// Shared by the combat tick and the pursuit pre-combat stage so range
/// decisions stay consistent.
pub(crate) fn resolve_target_coords(
    target: &TargetKind,
    entities: &EntityStore,
) -> Option<(u16, u16, SimFixed, SimFixed)> {
    match *target {
        TargetKind::Entity(id) => entities.get(id).map(target_coords),
        TargetKind::Cell(rx, ry) => Some(cell_center_coords(rx, ry)),
    }
}

/// ObjectClass::Distance_To5F6440: planar GetCoords distance, then the target
/// building's (foundation width + height)*64 discount, clamped to zero. This
/// differs from the weapon CanFireAt/InRange gate and has no altitude bonus.
/// Reuse the coordinate projection and deterministic native sqrt owner: exact
/// integer sqrt changes observable lepton ties (1281 becomes1280 natively).
/// Native comparisons: tools/spatial_oracle/aircraft_approach_range.{py,json}.
/// The discount reads the same stamped foundation as the target's GetCoords.
pub(crate) fn object_distance_to(
    source: &GameEntity,
    target: &TargetKind,
    entities: &EntityStore,
) -> Option<i32> {
    let planar = |(rx, ry, sx, sy): (u16, u16, SimFixed, SimFixed)| {
        [
            i32::from(rx) * 256 + sx.to_num::<i32>(),
            i32::from(ry) * 256 + sy.to_num::<i32>(),
            0,
        ]
    };
    let from = planar(target_coords(source));
    let to = planar(resolve_target_coords(target, entities)?);
    let distance = crate::util::native_x87::distance_3d_leptons(from, to);
    if let TargetKind::Entity(id) = *target
        && let Some(building) = entities.get(id)
        && building.category == EntityCategory::Structure
    {
        let (width, height) = foundation_dimensions(&building.foundation);
        // Height query45ECA0 receives false: Bib never adds to this discount.
        return Some(
            distance
                .wrapping_sub((i32::from(width) + i32::from(height)) * 64)
                .max(0),
        );
    }
    Some(distance)
}

/// Whether the attacker's normally selected weapon can currently reach this
/// target through the authoritative 3D `InRange` path.
///
/// gamemd-derived: SpawnManager mode 0 in `SpawnManagerClass::AI` @
/// `0x006B7230` calls the Unit owner's `TechnoClass::CanFireAtTarget` vslot
/// `0x006F7780`, which is `CanFireAt(target, SelectWeapon(target))`
/// (`0x006F77B0`) and ordinary `TechnoClass::InRange` @ `0x006F7220`. It asks
/// no legality: a target the weapon may not fire at is still in range.
pub(crate) fn can_fire_at_target(
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker_id: u64,
    target: &TargetKind,
    terrain: &ResolvedTerrainGrid,
    alliances: Option<&HouseAllianceMap>,
    los: &line_of_fire::LineOfFireInputs<'_>,
) -> bool {
    let Some(attacker) = entities.get(attacker_id) else {
        return false;
    };
    let Some(attacker_obj) = rules.object(interner.resolve(attacker.type_ref())) else {
        return false;
    };
    let Some((_, Some(selected))) = select_weapon_against(
        rules,
        attacker,
        attacker_obj,
        &combat_weapon::attacker_facts(attacker, attacker_obj),
        Some(target),
        entities,
        interner,
        Some(terrain),
        alliances,
    ) else {
        return false;
    };
    let Some(source) = in_range::fire_source_coords(
        attacker,
        target,
        selected.weapon,
        entities,
        terrain,
        (rules, interner),
    ) else {
        return false;
    };
    in_range::compute_in_range(
        attacker,
        source,
        target,
        selected.weapon,
        rules,
        interner,
        entities,
        terrain,
        los,
    )
}

/// Resolve the weapon an attacker would use against a `TargetKind` while
/// pursuing it.
///
/// Uses the same weapon-select inputs as the combat tick's Phase 2 weapon
/// selection so pursuit and combat agree on "in range" at the boundary.
///
/// Returns `None` only when the selected slot names no weapon. Legality is
/// not asked: `FootClass::Approach_Target @ 0x004D5690` selects (`0x004D56CA`)
/// and measures with CanFireAt without GetFireError. The fire routine asks
/// GetFireError itself, and `TechnoClass::AI`'s 16-frame check drops an
/// ILLEGAL or CANT target (a building drops it at once).
pub(crate) fn pursuit_selected_weapon<'a>(
    entity: &GameEntity,
    target: &TargetKind,
    entities: &EntityStore,
    rules: &'a RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<&'a WeaponType> {
    pursuit_selection(
        entity, target, entities, rules, interner, terrain, alliances,
    )
    .map(|selected| selected.weapon)
}

/// [`pursuit_selected_weapon`]'s whole selection, native weapon index
/// included.
pub(crate) fn pursuit_selection<'a>(
    entity: &GameEntity,
    target: &TargetKind,
    entities: &EntityStore,
    rules: &'a RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<combat_weapon::SelectedWeapon<'a>> {
    let attacker_obj = rules.object(interner.resolve(entity.type_ref()))?;
    select_weapon_against(
        rules,
        entity,
        attacker_obj,
        &combat_weapon::attacker_facts(entity, attacker_obj),
        Some(target),
        entities,
        interner,
        terrain,
        alliances,
    )
    .and_then(|(_, selected)| selected)
}

/// What the pursuit stage should do with an attacker that is holding a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PursuitRangeVerdict {
    /// The full `InRange` gate passes — halt and let the combat tick fire.
    CanFire,
    /// Refused, and closing the distance is what native's approach search does
    /// about it: too far, or a wall or a cliff on the line.
    CloseIn,
    /// Refused because the attacker stands INSIDE the weapon's `MinimumRange`.
    ///
    /// **VERA-internal, and a deliberate deferral of a native mechanism.**
    /// Native's approach search 0x004D5690 scans candidate coordinates and
    /// takes one where `InRange` holds, so a V3 that has been closed on backs
    /// off to five cells. VERA's pursuit produces exactly one candidate — the
    /// target's own cell — which for a too-close refusal is the WORST cell in
    /// the set. So this verdict holds position instead, which is bit-for-bit
    /// the behaviour this stage had before the walk landed.
    /// - Trigger: `MinimumRange=` weapon whose target has come inside it —
    ///   `V3Launcher` (5), `DredLauncher`/`CruiseLauncher` (8), `MagneticBeam`
    ///   (3), `HowitzerGun` (2), the `MissileLauncher`/`HoverMissile` family (1).
    /// - Player effect: the V3 stops rather than backing off, and the fire gate
    ///   keeps refusing until the target moves away again.
    /// - Frequency: ordinary — a V3 shelling an advancing column meets it every
    ///   time the column closes.
    /// - Downstream risk: none to deterministic state; it is a hold, and the
    ///   fire gate already refused before this stage ran. Cured by porting the
    ///   candidate-coordinate scan in 0x004D5690, which is its own mechanism.
    HoldInsideMinimumRange,
}

/// Whether a pursuing attacker can already shoot its target from where it
/// stands — the predicate that decides "halt and fire" against "keep closing".
///
/// gamemd-derived: `FootClass::Mission_Attack @ 0x004D4DC0` dispatches to the
/// approach search — the "walk to somewhere I can shoot my target from" routine
/// at vtable slot `+0x53C`, labelled `FootClass::Greatest_Threat_Scan @
/// 0x004D5690` in the database — through `CALL [EAX+0x53c]` at `0x004D4E6A`,
/// on the arm where TarCom (`[this+0x2B4]`) is non-null; `InfantryClass`'s
/// override `0x00522340` chains straight into the same body at `0x0052236E`.
/// That body decides with `TechnoClass::InRange @ 0x006F7220`, called at
/// `0x004D622C` and `0x004D6550` with a candidate coordinate as arg1
/// (`LEA EAX,[ESP+0x30]`) and TarCom as arg2 — and `InRange` ends in the
/// line-of-fire walk (`CALL 0x004CC310` at `0x006F7642`).
///
/// So the approach predicate and the fire gate are the SAME test in gamemd.
/// Using the 2-D twin here instead would let pursuit believe it had arrived
/// while `resolve_attacker_fire` refuses the shot, and the unit would stand
/// still under a standing order forever.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pursuit_in_range(
    entity: &GameEntity,
    target: &TargetKind,
    weapon: &WeaponType,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    los: &line_of_fire::LineOfFireInputs<'_>,
) -> PursuitRangeVerdict {
    let Some(terrain) = terrain else {
        // No resolved terrain (headless fixtures, pre-map bring-up): the 3-D
        // gate has nothing to measure against, so fall back to the same 2-D
        // twin `resolve_attacker_fire` falls back to in that situation. The
        // two stages still agree, which is the property that matters. That twin
        // has no MinimumRange arm either, so it cannot report the too-close
        // verdict.
        let Some((trx, try_, tsx, tsy)) = resolve_target_coords(target, entities) else {
            return PursuitRangeVerdict::CloseIn;
        };
        let dist_sq = lepton_distance_sq_raw(
            entity.position.rx,
            entity.position.ry,
            entity.position.sub_x,
            entity.position.sub_y,
            trx,
            try_,
            tsx,
            tsy,
        );
        return if is_within_range_leptons(dist_sq, weapon.range) {
            PursuitRangeVerdict::CanFire
        } else {
            PursuitRangeVerdict::CloseIn
        };
    };

    let Some(src) =
        in_range::fire_source_coords(entity, target, weapon, entities, terrain, (rules, interner))
    else {
        // The attacker has no resolvable ground height (off-grid). Report
        // "keep closing" rather than freezing; the fire gate returns without
        // firing on the same condition.
        return PursuitRangeVerdict::CloseIn;
    };
    if in_range::compute_in_range(
        entity, src, target, weapon, rules, interner, entities, terrain, los,
    ) {
        return PursuitRangeVerdict::CanFire;
    }
    if in_range::inside_minimum_range(src, target, weapon, rules, interner, entities, terrain) {
        return PursuitRangeVerdict::HoldInsideMinimumRange;
    }
    PursuitRangeVerdict::CloseIn
}

/// Install a bare combat fixture's entity target. Production commands use
/// `Simulation::assign_target_represented`, including concrete class effects.
/// This adapter stages the admitted Techno base state without running an event.
#[cfg(test)]
pub(crate) fn install_entity_attack_target_for_test(
    entities: &mut EntityStore,
    attacker_id: u64,
    target_id: u64,
) -> bool {
    if entities.get(target_id).is_none() {
        return false;
    }
    let attacker = match entities.get_mut(attacker_id) {
        Some(a) => a,
        None => return false,
    };

    // Preserve historical fixture setup: discard a non-Walk movement adapter
    // before installing the initial target. This is not a native setter effect.
    if !attacker
        .locomotor
        .as_ref()
        .is_some_and(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
    {
        attacker.movement_target = None;
    }

    crate::sim::mission::concrete_effects::represented_assign_target_admitted(
        attacker,
        Some(TargetKind::Entity(target_id)),
        true,
    );

    true
}

/// `TechnoClass::EstimateDamage @ 0x006FDB80` for `attacker_id` shooting
/// `target_id` with `weapon` (see [`damage::estimate`]): the target house's
/// category multiplier for the attacker's type, the attacker's
/// `ArmorMultiplier`, the attacker's FIREPOWER and the target's STRONGER ranks.
///
/// RESIDUAL: `House+0x188` is 1.0, as in FireAt's damage build
/// (`world_receiver::fireat_damage`); `Techno+0x160` is the attacker's live
/// `firepower_multiplier`.
pub(crate) fn estimated_damage_on(
    sim: &crate::sim::world::Simulation,
    rules: &RuleSet,
    attacker_id: u64,
    target_id: u64,
    weapon: &crate::rules::weapon_type::WeaponType,
) -> i32 {
    use crate::rules::object_type::Ability;
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    let entities = &sim.substrate.entities;
    let (Some(attacker), Some(target)) = (entities.get(attacker_id), entities.get(target_id))
    else {
        return 0;
    };
    let Some(attacker_obj) = rules.object(sim.interner.resolve(attacker.type_ref())) else {
        return 0;
    };
    let target_obj = rules.object(sim.interner.resolve(target.type_ref()));
    let house_type_armor = sim.houses.get(&target.owner()).map_or(1.0, |house| {
        rules.country_armor_mult_for_type(sim.interner.resolve(house.house_type_id()), attacker_obj)
    });
    let rank_firepower = self::veterancy::has_weapon_ability(
        self::veterancy::rank_from_u16(attacker.veterancy()),
        attacker_obj,
        Ability::Firepower,
    )
    .then(|| NativeF64Bits::from_bits(rules.general.veteran_combat.to_bits()));
    let rank_armor = target_obj
        .is_some_and(|object| {
            self::veterancy::has_weapon_ability(
                self::veterancy::rank_from_u16(target.veterancy()),
                object,
                Ability::Stronger,
            )
        })
        .then(|| NativeF64Bits::from_bits(rules.general.veteran_armor.to_bits()));
    let warhead = combat_weapon::warhead_of(rules, weapon);
    damage::estimate::estimated_damage(&damage::estimate::EstimateInputs {
        damage: weapon.damage,
        zeroed: weapon.is_sonic || weapon.use_fire_particles,
        stages: damage::attacker::FireDamageStages {
            house_firepower: NativeF64Bits::ONE,
            unit_firepower: attacker.firepower_multiplier,
            rank_firepower,
            occupied: None,
            bunkered: None,
            open_topped: None,
        },
        divisors: damage::DefenceDivisors {
            house_type_armor: NativeF32Bits::from_bits(house_type_armor.to_bits()),
            unit_armor: attacker.armor_multiplier,
            rank_armor,
        },
        warhead: warhead.map(|warhead| damage::estimate::EstimateWarhead {
            cell_spread: warhead.cell_spread_f64,
            percent_at_max: warhead.percent_at_max_f64,
            verses: &warhead.verses_f64,
        }),
        armor: damage::ArmorClass(target_obj.map_or(0, |object| armor_index(&object.armor) as u8)),
        scenario_no_damage: sim.session.no_damage,
        max_damage: rules.combat_damage.max_damage,
    })
}

/// Install a bare combat fixture's cell target. Keep its current-weapon gate
/// through the shared weapon reader; production ForceAttackCell uses the
/// concrete class authority in `world_commands`, not this setup adapter.
#[cfg(test)]
pub(crate) fn install_cell_attack_target_for_test(
    entities: &mut EntityStore,
    attacker_id: u64,
    target_rx: u16,
    target_ry: u16,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> bool {
    // Read weapon presence before the mutable borrow.
    let has_weapon = match entities.get(attacker_id) {
        Some(a) => rules
            .and_then(|r| r.object(interner.resolve(a.type_ref())))
            .is_some_and(|obj| combat_weapon::is_armed(a, obj)),
        None => return false,
    };

    if !has_weapon {
        // Defensive: client-side filter should have routed this to Move.
        // Warn-log so the desync is visible rather than silent.
        log::warn!(
            "ForceAttackCell rejected for unarmed attacker {} (target cell {},{})",
            attacker_id,
            target_rx,
            target_ry
        );
        return false;
    }

    let attacker = match entities.get_mut(attacker_id) {
        Some(a) => a,
        None => return false,
    };

    if !attacker
        .locomotor
        .as_ref()
        .is_some_and(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
    {
        attacker.movement_target = None;
    }
    crate::sim::mission::concrete_effects::represented_assign_target(
        attacker,
        Some(TargetKind::Cell(target_rx, target_ry)),
    );
    true
}

use self::combat_targeting::{AttackerSnapshot, GarrisonSnapshot};

/// A `CanBeOccupied` building destroyed in combat with live occupants —
/// gamemd routes this through `BuildingClass::SellBuilding @ 0x00457DE0`, the
/// same occupant-eject helper used by sell. The world fatal prelude consumes
/// this plan synchronously, before the nested death weapon and carrier UnInit.
pub struct DestroyedGarrisonBuilding {
    pub building_id: u64,
    pub type_id: InternedId,
    /// Building's owner at time of death — ejected infantry inherit this.
    pub owner: InternedId,
    pub rx: u16,
    pub ry: u16,
    pub z: u8,
    pub foundation_w: u16,
    pub foundation_h: u16,
    /// Snapshot of `cargo.passengers` at time of death. LIFO order preserved
    /// (eject helper iterates in reverse).
    pub passenger_ids: Vec<u64>,
}

/// Explosion animation to spawn at a world position (deferred to caller
/// which has access to `Simulation` for AnimClass construction).
pub struct ExplosionEffect {
    pub shp_name: InternedId,
    pub rx: u16,
    pub ry: u16,
    /// Sub-cell impact X in leptons. Preserves the CoordStruct-level impact
    /// point for warhead AnimList placement.
    pub sub_x: SimFixed,
    /// Sub-cell impact Y in leptons.
    pub sub_y: SimFixed,
    pub z: u8,
    /// Exact absolute Z in leptons; the anim is constructed there (its Middle
    /// height gate reads it). `z` is the level byte of the same point.
    pub world_z: i32,
    /// A death producer's own constructor call (`Death_Explosion`, the
    /// Aircraft death arm, `DestructionEffects`, the infantry InfDeath arms):
    /// `AnimClass(type, coord, delay, 1, 0x600, 0, 0)` at an exact
    /// coordinate. `None` rows construct with the warhead impact's
    /// `(0, 1, 0x2600, -15)` at the impact's cell, sub-cell and exact
    /// `world_z`. The TechnoClass debris anims are death constructions at
    /// `center + 0x14 Z` (`0x007024AA`, `0x00702566`) that already took their
    /// constructor draws.
    pub death: Option<destruction_effects::DeathAnimSpawn>,
}

/// One transient combat-light request, the inputs of one `FUN_0048A620` call.
/// It creates an unowned screen-space light, not an AnimClass/ParticleSystem,
/// so this record keeps the exact call inputs without inventing an attachment
/// or house. Callers:
/// - an active IronCurtain or ForceShield rejecting a positive receiver call
///   (the damage shifted left once; flags IC=1, ForceShield=6);
/// - a `Bright=` bullet's detonation (`BulletClass::DetonateAtCoord
///   0x00469BD6..0x00469C41`: the bullet's damage `+0x6C`, flags from the
///   warhead's `CLDisableRed/Green/Blue=` as 2/4/8);
/// - every rocket impact (`RocketLocomotion::Detonate 0x006632AF`, not forced)
///   and a bouncing chunk's dry landing (`AnimClass::AI 0x00423EF8`, not
///   forced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatLightRequest {
    /// Receiver provenance only; this is not native effect ownership.
    pub target_id: Option<u64>,
    /// The damage the helper sizes the light from.
    pub damage: i32,
    pub warhead_ref: InternedId,
    pub coord: ProjectileCoord,
    /// Native helper force/create argument (literal true on both callsites).
    pub force_create: bool,
    /// Native raw draw flags.
    pub flags: u32,
}

/// One smudge producer payload. Production commits it synchronously through
/// the world receiver; phase-level combat fixtures retain it in their result as
/// a test adapter.
#[derive(Debug, Clone)]
pub enum SmudgeSpawnRequest {
    /// `AnimClass::Middle @ 0x00424F00` past its height gate: the anim's
    /// coordinate (vt+0x48) and its marks. The anim runtime emits it at Start
    /// or at the anim's middle frame.
    AnimMiddle {
        coord: crate::sim::smudge_grid::SimCoord,
        marks: smudge_dispatch::AnimMiddleMarks,
    },
    /// Emitted once per >=2x2 building destruction (DestructionEffects path).
    BuildingCenter {
        rx: u16,
        ry: u16,
        building_z: i32,
        foundation_w: u8,
        foundation_h: u8,
    },
    /// One foundation cell's mark, committed by `SpawnSurvivors` after that
    /// cell's survivor roll (never for a building owing no survivor).
    BuildingSurvivor { cell_rx: u16, cell_ry: u16 },
}

/// `BuildingClass::DestructionEffects`' centre mark for a destroyed building.
fn building_center_smudge_request(
    rx: u16,
    ry: u16,
    building_z: i32,
    foundation: &str,
) -> SmudgeSpawnRequest {
    let (foundation_w, foundation_h) = foundation_dimensions(foundation);
    SmudgeSpawnRequest::BuildingCenter {
        rx,
        ry,
        building_z,
        foundation_w: foundation_w as u8,
        foundation_h: foundation_h as u8,
    }
}

/// One captured TerrainClass receiver in a fixed Apply_area_damage transaction.
///
/// Transient only: stable identity, cell, distance, and isolation scope are
/// captured during collection so dispatch never rescans a later world state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainDamageEvent {
    pub stable_id: u64,
    pub rx: u16,
    pub ry: u16,
    pub damage: i32,
    pub distance_leptons: i32,
    pub warhead_ref: InternedId,
    /// True when the parent AoE used native binary32 CellSpread <= 0.5.
    /// Terrain cannot arm IC isolation, but an armed transaction skips it.
    pub near_center_ic_isolation_eligible: bool,
}

/// Hookless compatibility record for one admitted per-cell tiberium reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TiberiumReductionRequest {
    pub rx: u16,
    pub ry: u16,
    pub amount: i32,
}

/// The shots an object's own mission asked the combat phase for this frame,
/// where VERA's FireAt lives. Filled by the live object pass, drained by
/// combat in the same frame; never a permission carried to a later frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FireRequests {
    /// Aircraft whose Mission_Attack strike state (4..9) asked for its visit.
    pub aircraft: std::collections::BTreeSet<u64>,
    /// Buildings whose Update asked for a FireAt: GetFireError answered OK in
    /// that visit, so combat emits the shot without asking again.
    pub buildings: std::collections::BTreeMap<u64, BuildingShot>,
}

/// Which FireAt of `BuildingClass::Update` a building's request stands for.
/// Both FireAts shoot the `TarCom` (`+0x2B4`) of the visit that asked, so the
/// request carries it: the combat phase fires at it, and FireAt's own TarCom
/// reads see it, whatever retargets the building later in the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildingShot {
    /// Mission_Attack's FireAt arm (`0x0044B6D0`), with the weapon index
    /// SelectWeapon answered in that visit (`0x0044AFF2`). The visit's
    /// Gattling charge after the FireAt (`0x0044B6EF`) may step the stage
    /// before the combat phase fires, so the index is not asked again.
    Mission { weapon: i32, target: TargetKind },
    /// ProcessDelayedFire's mode-1 FireAt (`0x00450492`), with the weapon
    /// Mission_Attack saved when it armed the shot (`+0x708`). A launched
    /// bullet takes the building's support bonus
    /// (`Simulation::take_support_bonus`).
    Delayed {
        slot: combat_weapon::WeaponSlot,
        target: TargetKind,
    },
}

impl BuildingShot {
    /// The visit's TarCom the shot goes at.
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
    /// Shrapnel bullets from detonations the combat pass committed; the frame
    /// admits them before its Logic tail visits the new objects. FireAt admits
    /// its own bullets directly.
    pub projectile_spawns: Vec<ProjectileSpawn>,
    /// Facing observations for component combat fixtures. Production Unit
    /// facing commits within that Unit's live Logic slot.
    pub unit_facing: Vec<UnitFacingUpdate>,
    pub(crate) consequences: crate::sim::world::damage_consequences::DamageConsequences,
}

/// A "your asset is being shot" ping: a Structure or harvester took damage
/// this tick.
///
/// Native has two producers and neither tests the attacker's house:
/// `BuildingClass::ReceiveDamage @ 0x00442230`'s retaliation block calls
/// `HouseClass::NotifyUnderAttack` for a living building hit by a source
/// object with a non-zero result, unless its type is `Insignificant=`
/// (`+0x232`) or 1x1 with `UndeploysInto=` (vt+0x80, `0x00465D40`)
/// ([`crate::sim::world::Simulation::building_hit_response`]); own-fire on an
/// own building announces. `UnitClass::ReceiveDamage 0x007384B9..0x00738530`
/// pings a `Harvester=` unit on any non-zero, non-fatal result with or
/// without a source.
#[derive(Debug, Clone, Copy)]
pub struct UnderAttackEvent {
    pub rx: u16,
    pub ry: u16,
    /// The VICTIM's owner — the player whose radar/EVA should react.
    pub owner: InternedId,
    /// True for the ore-miner line: a `Harvester=` unit, or (via
    /// `NotifyUnderAttack 0x004F9491..0x004F94A3`) a building whose type has
    /// `UndeploysInto=` and `ResourceGatherer=yes` — the deployed slave miner.
    pub miner: bool,
    /// True when produced by the building path, which is the only one that
    /// reaches `NotifyUnderAttack`'s ally branch.
    pub structure: bool,
}

/// `TechnoClass::Death_Announcement @ 0x004D98C0` input: a non-building
/// techno was killed at a `ReceiveDamage` kill site (`AircraftClass
/// 0x00416613`, infantry `0x005180F4`, `UnitClass 0x00737DC9/0x00737E39/
/// 0x00737E68`, all vtable slot `+0x3B8`) and its type is not `Spawned=`.
/// The world applies the owner-is-human gate and the radar type-7 dedupe.
#[derive(Debug, Clone, Copy)]
pub struct UnitLostEvent {
    pub rx: u16,
    pub ry: u16,
    pub owner: InternedId,
}

/// The `Death_Announcement` (`+0x3B8`) input for one death site.
///
/// Every native caller sits in a non-building `ReceiveDamage` override
/// (`BuildingClass` has none), and `0x004D98DD` skips `Spawned=` types.
/// Called by the damage kill loop only. The non-damage death sites that
/// natively route through `+0x16C` (`ReceiveDamage`) with `C4Warhead=`,
/// `InfantryClass::IronCurtain 0x00522632` and `CellClass::BlowUpBridge
/// 0x0047DDAE`, reach it the same way: VERA kills through that loop too.
/// Paths that natively skip `ReceiveDamage` (crush
/// `0x007416A0` → `RecordKill` only, `AircraftClass::Enter_Idle_Mode
/// 0x004179FD/0x00417B88` → `Crash` slot `+0x3DC`, the off-playfield
/// `UnInit` at `AircraftClass::AI 0x00414F93/0x00414FD1`) must not call it.
pub(crate) fn death_announcement_event(
    obj: &crate::rules::object_type::ObjectType,
    category: EntityCategory,
    rx: u16,
    ry: u16,
    owner: InternedId,
) -> Option<UnitLostEvent> {
    (category != EntityCategory::Structure && !obj.spawned).then_some(UnitLostEvent {
        rx,
        ry,
        owner,
    })
}

use crate::sim::movement::ground_pose::object_world_z_leptons;

fn attack_world_z_leptons(
    target: TargetKind,
    entities: &EntityStore,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    match target {
        TargetKind::Entity(entity_id) => entities
            .get(entity_id)
            .map(|entity| object_world_z_leptons(entity, terrain))
            .unwrap_or(0),
        // FireAt6FE1FF and Get_Led_Target_Coords70BCB0 read Cell+58;
        // Bullet::Fire468707 freezes that same unled aim. The flight/impact
        // owners separately resolve ground contact and the final detonation.
        TargetKind::Cell(rx, ry) => crate::sim::projectile::cell_target_coord(terrain, rx, ry).z,
    }
}

/// Narrow an impact z into the byte the presentation path carries it in.
///
/// The projection that turns that byte into a pixel decodes it with `as i8`
/// (`util::lepton::lepton_to_screen`), so the byte is a *signed* level count
/// and the only correct saturation is into `i8` range: clamping into `u8`
/// range instead would let 200 through, which decodes as -56 levels and throws
/// the sprite most of a screen away. One definition, so the sim's animation
/// height and the app's tracer endpoint cannot narrow the same number
/// differently.
///
/// Known mismatch, outside this file: the sprite depth key reads the same byte
/// as unsigned. The two readings agree over 0..=127, which covers every map
/// height, so it is latent — but a negative impact z would sort by one rule
/// and draw by the other.
pub(crate) fn impact_z_byte(impact_z: i32) -> u8 {
    impact_z.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8 as u8
}

fn death_weapon_ftol_product_i32_f32(value: i32, multiplier: f32) -> Option<i32> {
    let multiplier = X87Chop53::load_f32(NativeF32Bits::from_bits(multiplier.to_bits())).ok()?;
    let product = X87Chop53::mul(X87Chop53::load_i32(value), multiplier);
    i32::try_from(X87Chop53::ftol_i64(product).ok()?).ok()
}

fn death_weapon_half_strength(strength: i32) -> Option<i32> {
    let half = X87Chop53::load_f64(NativeF64Bits::HALF).ok()?;
    let product = X87Chop53::mul(X87Chop53::load_i32(strength), half);
    i32::try_from(X87Chop53::ftol_i64(product).ok()?).ok()
}

/// The death arm's gate (`TechnoClass::ReceiveDamage 0x00702572..0x00702601`):
/// Explodes (`+0xD15`), the veteran or elite `EXPLODES` ability (IsVeteran
/// `0x0074FF90` with `+0x2A6`; IsElite `0x00750010` with `+0x2A6` or
/// `+0x2B8`), or `numbered_weapon` is `Suicide=` (`+0x144`): the weapon GetWeapon
/// (vtable `+0x3F8`, [`combat_weapon::get_weapon`]) answers for
/// `CurrentWeaponNumber` (`+0x138`, pushed at `0x0070257A`). `+0x138` is a
/// Gunner transport's passenger IFVMode (SetGunnerWeapon `0x0070DC70`), else
/// 0: it is not the last-fired slot. When it holds, KillPassengers
/// (`0x00702603..0x00702667`) and then Fire_Death_Weapon (`0x00702669`) run;
/// otherwise neither does.
pub(crate) fn death_arm_explodes(
    obj: &ObjectType,
    veterancy: u16,
    numbered_weapon: Option<&WeaponType>,
) -> bool {
    obj.explodes
        || self::veterancy::has_weapon_ability(
            self::veterancy::rank_from_u16(veterancy),
            obj,
            crate::rules::object_type::Ability::Explodes,
        )
        || numbered_weapon.is_some_and(|weapon| weapon.suicide)
}

/// `TechnoClass::Fire_Death_Weapon @ 0x0070D690`'s weapon and damage, before
/// its caller's extra damage: `DeathWeapon=` (`+0xD18`), else the current
/// weapon (`GetCurrentWeapon`, vtable `+0x3F4`, the caller's `current_weapon`),
/// each at `ftol(Damage * DeathWeaponDamageModifier)`
/// (`0x0070D6EB..0x0070D6F7`); else `[CombatDamage] DeathWeapon=`
/// (`Rules+0xFDC`) at `ftol(Strength * 0.5)` (`0x0070D6FE..0x0070D71F`). No
/// weapon fires nothing. The function has no `Explodes=` gate: the death arm
/// gates it ([`death_arm_explodes`]); a crash impact calls it bare.
pub(crate) fn fire_death_weapon_payload(
    rules: &RuleSet,
    obj: &ObjectType,
    current_weapon: Option<&WeaponType>,
    interner: &mut StringInterner,
) -> Option<(i32, InternedId, InternedId)> {
    let chosen = obj
        .death_weapon
        .as_deref()
        .and_then(|weapon_id| rules.weapon(weapon_id))
        .or(current_weapon);
    if let Some(weapon) = chosen {
        let damage =
            death_weapon_ftol_product_i32_f32(weapon.damage, obj.death_weapon_damage_modifier)?;
        let warhead_ref = interner.intern(weapon.warhead.as_ref()?);
        let weapon_ref = interner.intern(&weapon.id);
        return Some((damage, warhead_ref, weapon_ref));
    }
    let fallback = rules
        .combat_damage
        .death_weapon
        .as_deref()
        .and_then(|weapon_id| rules.weapon(weapon_id))?;
    let warhead_ref = interner.intern(fallback.warhead.as_ref()?);
    let weapon_ref = interner.intern(&fallback.id);
    Some((
        death_weapon_half_strength(obj.strength)?,
        warhead_ref,
        weapon_ref,
    ))
}

/// `TechnoClass::Fire_Death_Weapon @ 0x0070D690` with no extra damage: a real
/// bullet of the [`fire_death_weapon_payload`] weapon (`CreateBullet @
/// 0x0046B050`, owner and target the object itself) detonated at the object's
/// GetCoords (vt+0x48, `0x0070D77C`; a building's foundation centre) through
/// `DetonateAtCoord @ 0x004690B0`. `None` when no weapon fires.
pub(crate) fn death_weapon_detonation(
    world: &mut Simulation,
    id: u64,
    rules: &RuleSet,
) -> Option<ProjectileDetonation> {
    let entity = world.substrate.entities.get(id)?;
    let object = rules.object(world.interner.resolve(entity.type_ref()))?;
    let current_weapon = combat_weapon::current_weapon(
        entity,
        object,
        &world.substrate.entities,
        rules,
        &world.interner,
    );
    let impact = crate::sim::movement::ground_pose::object_get_coords(
        entity,
        world.resolved_terrain.as_ref(),
    );
    let (damage, warhead, weapon) =
        fire_death_weapon_payload(rules, object, current_weapon, &mut world.interner)?;
    Some(ProjectileDetonation {
        projectile_id: id,
        source_id: id,
        target: ProjectileTarget::Entity(id),
        impact: ProjectileCoord::new(impact.x, impact.y, impact.z),
        payload: ProjectilePayload::new(damage, warhead, weapon),
        reason: crate::sim::projectile::ProjectileDetonationReason::DeathWeapon,
    })
}

/// One ordered accumulator for weapon emission and recursive receiver effects.
/// DamageConsequences consumes its deferred work at the world delivery boundary.
#[derive(Default)]
pub(crate) struct DeathEffects {
    /// Fatal receivers, including SHP deaths that remain represented for animation.
    pub(crate) despawned_ids: Vec<u64>,
    /// Remaining world UnInit requests; distinct from all fatal receiver IDs.
    pub(crate) immediate_uninit_ids: Vec<u64>,
    pub(crate) structure_destroyed: bool,
    pub(crate) explosion_effects: Vec<ExplosionEffect>,
    /// `VoxelAnimClass` debris planned by the death block for admission at the
    /// world consequence boundary. The live receiver has allocator access;
    /// deferred admission preserves the existing allocation and Logic order.
    pub(crate) voxel_debris: Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    pub(crate) combat_light_requests: Vec<CombatLightRequest>,
    /// Receipt of synchronous Apply_area_damage bridge callbacks. Gameplay
    /// already ran before the detonation's animation and cluster successor.
    pub(crate) bridge_state_changed: bool,
    #[cfg(test)]
    pub(crate) wall_mutations: Vec<WallMutation>,
    #[cfg(test)]
    pub(crate) cell_target_detaches: Vec<combat_aoe::CellTargetDetach>,
    pub(crate) tiberium_reduction_requests: Vec<TiberiumReductionRequest>,
    pub(crate) death_sounds: Vec<(String, u16, u16)>,
    pub(crate) smudge_spawn_requests: Vec<SmudgeSpawnRequest>,
    pub(crate) rad_detonations: Vec<crate::sim::radiation::RadDetonation>,
    pub(crate) under_attack_events: Vec<UnderAttackEvent>,
    /// `Death_Announcement` inputs from this tick's damage kills; the world
    /// applies the human-owner gate and the radar type-7 dedupe.
    pub(crate) unit_lost_events: Vec<UnitLostEvent>,
    #[cfg(test)]
    pub(crate) receiver_stage_trace: Vec<ReceiverStageTrace>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiverStageTrace {
    HouseThreat { target_id: u64, delta: i32 },
    PostMortem { target_id: u64 },
    ShouldRetaliate { target_id: u64 },
}

/// World-owned lifecycle work that brackets the native death helper for a
/// concrete fatal receiver. Passenger teardown precedes the nested death
/// weapon; represented UnInit follows it before the next outer receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FatalLifecycleStage {
    /// Surviving Techno ReceiveDamage postlude, before Infantry scatter and
    /// synchronous retaliation. The world owns ParticleSystem storage and the
    /// shared LogicVector, so maintenance crosses the existing inline hook.
    MaintainDamageSmoke {
        state: damage::DamageState,
    },
    /// ObjectClass's exact-zero callback transaction for an eligible delayed
    /// death. It runs while the target is still represented and Health is
    /// exactly zero, before TechnoClass arms/shortens the shared C4 timer and
    /// restores Alive/Health=1.
    PostMortemExactZero {
        killer_owner: Option<InternedId>,
    },
    BeforeDeathEffects,
    AfterDeathEffects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BaseDefenseResponseCallSite {
    BuildingPrelude,
    ProtectedTechno,
}

#[cfg(test)]
fn tiberium_reduction_amount(
    base_damage: i32,
    affect_resource: bool,
    warhead: &WarheadType,
) -> Option<i32> {
    if !affect_resource || !warhead.tiberium {
        return None;
    }
    let amount = base_damage / 10;
    (amount > 0).then_some(amount)
}

impl DeathEffects {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.despawned_ids.append(&mut other.despawned_ids);
        self.immediate_uninit_ids
            .append(&mut other.immediate_uninit_ids);
        self.structure_destroyed |= other.structure_destroyed;
        self.explosion_effects.append(&mut other.explosion_effects);
        self.voxel_debris.append(&mut other.voxel_debris);
        self.combat_light_requests
            .append(&mut other.combat_light_requests);
        self.bridge_state_changed |= other.bridge_state_changed;
        #[cfg(test)]
        self.wall_mutations.append(&mut other.wall_mutations);

        #[cfg(test)]
        self.cell_target_detaches
            .append(&mut other.cell_target_detaches);
        self.tiberium_reduction_requests
            .append(&mut other.tiberium_reduction_requests);
        self.death_sounds.append(&mut other.death_sounds);
        self.smudge_spawn_requests
            .append(&mut other.smudge_spawn_requests);
        self.rad_detonations.append(&mut other.rad_detonations);
        self.under_attack_events
            .append(&mut other.under_attack_events);
        self.unit_lost_events.append(&mut other.unit_lost_events);
        #[cfg(test)]
        self.receiver_stage_trace
            .append(&mut other.receiver_stage_trace);
    }
}

/// The debris block of `TechnoClass::ReceiveDamage @ 0x00701900`, wired to the
/// dying object. Every draw is on the Scenario stream (`[0x00A8B230]+0x218`:
/// the count at `0x007022BA..0x007022C8`, the per-piece pick at `0x0070232B`,
/// and the VoxelAnim constructor's seven), not the death sounds' stream.
///
/// The entry gate is `0x00702232`..`0x0070227B` and it is a *drop-in* gate, not
/// a water gate. `0x0070223D MOV AL,[ESI+0x8F] / TEST AL,AL / JZ 0x00702281`
/// jumps **past** the cell lookup and into the `MaxDebris` test whenever the
/// byte is CLEAR, so the `CMP [cell+0xEC],2` water skip at `0x00702274` is
/// reached only while the byte is set. `ObjectClass+0x8F` is written 0 by
/// `ObjectClass::Constructor @ 0x005F3981` (`MOV [ESI+0x8F],BL` after
/// `XOR EBX,EBX` at `0x005F3909`) and set to 1 only by
/// `ObjectClass::DropIn @ 0x005F4171` — the paradrop / free-fall entry, which
/// sets `+0x8D` in the same breath — and `ObjectClass::AI @ 0x005F4021` reads
/// it as the gate on its fall arm. A program-wide `search_instructions` for
/// `+ 0x8f]` finds no third writer. An object that is not currently falling
/// from a drop-in therefore has the byte clear, so an ordinary death over water
/// DOES throw debris and consumes the block's draws.
///
/// The SHP half constructs a bouncing `AnimClass` per piece (`0x00421EA0`):
/// every stock debris AnimType is `Bouncer=yes` — all 26 named by `[General]
/// MetallicDebris=` or by any `DebrisAnims=` line, authored in `artmd.ini`
/// (`AnimTypeClass+0x35A`, read at `0x004286A7`). Each piece's constructor
/// draws run right after its pick and the piece enters Logic before the next
/// pick. Stock DBRIS actively reads RandomRate=220,600; native428784/42879F
/// divides by900 and4287D6 clamps to[1,1], so that coincident ranged request
/// advances no raw word. Bouncer's velocity/BounceInit draws are independent.
/// Original execution:
/// `tools/spatial_oracle/anim_bouncer_launch.py` (`debris_loop` rows).
/// Counts on gamemd's case-exact key read, which `ObjectType::from_ini_section`
/// matches (`CCINIClass::ReadInt @ 0x005276D0` CRCs the raw key bytes): 324 of
/// the 356 stock sections that throw reach an SHP arm (of 439 authoring
/// `MaxDebris=`, 83 author 0); the 17 `[VehicleTypes]` spelling `Maxdebris=`
/// (the Rhino among them) take the constructor default 0 and throw nothing.
fn throw_debris_for_death(
    world: &mut Simulation,
    object_type: &ObjectType,
    rules: &RuleSet,
    owner: InternedId,
    origin: glam::IVec3,
    voxel_debris: &mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    explosion_effects: &mut Vec<ExplosionEffect>,
) {
    use crate::sim::voxel_anim::{DebrisTypeData, throw_death_debris};

    if object_type.max_debris <= 0 {
        return;
    }
    let debris_types: Vec<_> = object_type
        .debris_types
        .iter()
        .map(|name| {
            rules
                .voxel_anim_type_id_by_name(name)
                .map(|id| (id, rules.voxel_anim_type(id)))
        })
        .collect();
    let data = DebrisTypeData {
        max_debris: object_type.max_debris,
        min_debris: object_type.min_debris,
        debris_types: &debris_types,
        debris_maximums: &object_type.debris_maximums,
        debris_anim_count: object_type.debris_anims.len(),
        metallic_debris_count: rules.general.metallic_debris.len(),
    };
    // Native702443/70254B lifts SHP debris by20 leptons. Voxel74950C
    // applies its own10-lepton lift through the existing numeric owner.
    let anim_coord = crate::sim::anim_class::AnimWorldCoord {
        x: origin.x,
        y: origin.y,
        z: origin.z.wrapping_add(0x14),
    };
    let mut host = DeathDebrisWorldHost {
        world,
        object_type,
        rules,
        anim_coord,
        voxel_debris,
        explosion_effects,
    };
    if let Err(error) = throw_death_debris(&data, Some(owner), origin, &mut host) {
        // Out-of-domain modded velocities retain the pieces already emitted;
        // the original ordered block has no rollback of earlier constructors.
        log::debug!("death debris stopped outside verified numeric domain: {error}");
    }
}

/// Live constructor adapter for the single debris-loop port. Numeric fixtures
/// collect the same port's results; production admits each piece at its call.
struct DeathDebrisWorldHost<'w, 'r> {
    world: &'w mut Simulation,
    object_type: &'r ObjectType,
    rules: &'r RuleSet,
    anim_coord: crate::sim::anim_class::AnimWorldCoord,
    voxel_debris: &'w mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    explosion_effects: &'w mut Vec<ExplosionEffect>,
}

impl<'r> DeathDebrisWorldHost<'_, 'r> {
    fn debris_name(
        &self,
        source: crate::sim::voxel_anim::ShpDebrisSource,
        index: usize,
    ) -> Option<&'r str> {
        use crate::sim::voxel_anim::ShpDebrisSource;
        match source {
            ShpDebrisSource::TypeDebrisAnims => self.object_type.debris_anims.get(index),
            ShpDebrisSource::RulesMetallicDebris => self.rules.general.metallic_debris.get(index),
        }
        .map(String::as_str)
    }
}

impl crate::sim::voxel_anim::DeathDebrisHost for DeathDebrisWorldHost<'_, '_> {
    fn rng(&mut self) -> &mut SimRng {
        &mut self.world.scenario_rng
    }

    fn construct_anim(
        &mut self,
        source: crate::sim::voxel_anim::ShpDebrisSource,
        index: usize,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let native_unique_id = crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
            &mut self.world.native_unique_ids,
        );
        let name = self.debris_name(source, index);
        let config = name.and_then(|name| {
            self.rules
                .art()
                .anim_runtime_config(&name.to_ascii_uppercase())
        });
        let mut draws = if let Some(config) = config {
            crate::sim::anim_class::anim_constructor_draws(
                config,
                self.anim_coord,
                &mut self.world.scenario_rng,
            )?
        } else {
            crate::sim::anim_class::AnimConstructorDraws {
                native_unique_id: None,
                random_rate: None,
                bounce: None,
            }
        };
        draws.native_unique_id = Some(native_unique_id);
        if let Some(name) = name {
            let shp_name = self.world.interner.intern(name);
            let (rx, ry, sub_x, sub_y, z) = self.anim_coord.to_cell_sub_z();
            let effect = ExplosionEffect {
                shp_name,
                rx,
                ry,
                sub_x,
                sub_y,
                z,
                world_z: self.anim_coord.z,
                death: Some(destruction_effects::DeathAnimSpawn {
                    draws: Some(draws),
                    ..destruction_effects::DeathAnimSpawn::at(self.anim_coord, 0)
                }),
            };
            if world_receiver::callbacks_enabled(self.world) {
                crate::sim::world::damage_consequences::admit_explosion_effect(
                    self.world, self.rules, effect,
                );
            } else {
                // Explicit callback-disabled tests retain their packet seam.
                self.explosion_effects.push(effect);
            }
        }
        Ok(())
    }

    fn admit_voxel(&mut self, spawn: &crate::sim::voxel_anim::VoxelDebrisSpawn) {
        if world_receiver::callbacks_enabled(self.world) {
            self.world
                .admit_death_debris(std::iter::once(spawn.clone()));
        } else {
            self.voxel_debris.push(spawn.clone());
        }
    }
}

/// Select the death cues one dying object contributes.
///
/// The building tail is `BuildingClass::DestructionEffects`: when the dying
/// object is a structure whose type carries **no** `DieSound=` entries,
/// gamemd plays the global `[AudioVisual] BuildingDieSound` at the building's
/// own coordinate — `0x0044173F MOV ECX,[type+0x520]` reads the `DieSound`
/// vector's count (`TechnoTypeClass+0x510..0x528`), `0x0044174A CMP ECX,EBX ;
/// JNZ` skips the global when it is non-zero (`EBX` is zeroed at
/// `0x00441606` and stays zero), and `0x00441773 MOV ECX,[Rules+0x6E8]` +
/// `0x00441779 CALL VocClass::PlayAtCoord @ 0x00750E20` is the play. It is
/// not owner-gated and draws no RNG — there is one id, not a list.
///
/// Where the global sits relative to the per-type `VoiceDie=`/`DieSound=`
/// draws is UNCHECKED: native emits it from the building-specific destruction
/// path, not from the shared Techno death transaction. It cannot matter on
/// retail data, because the branch that reaches it requires an empty
/// `DieSound=` list and no stock building pairs `VoiceDie=` with an empty
/// `DieSound=`.
fn append_selected_death_sounds(
    object_type: &ObjectType,
    category: EntityCategory,
    building_die_sound: Option<&str>,
    owner_is_human: bool,
    main_rng: &mut SimRng,
    rx: u16,
    ry: u16,
    death_sounds: &mut Vec<(String, u16, u16)>,
) {
    let mut append_choice = |choices: &[String]| {
        if choices.is_empty() {
            return;
        }
        let index = (main_rng.next_u32() % choices.len() as u32) as usize;
        death_sounds.push((choices[index].clone(), rx, ry));
    };

    if owner_is_human {
        append_choice(&object_type.voice_die);
    }
    append_choice(&object_type.die_sounds);

    if category == EntityCategory::Structure && object_type.die_sounds.is_empty() {
        if let Some(sound_id) = building_die_sound.filter(|id| !id.is_empty()) {
            death_sounds.push((sound_id.to_owned(), rx, ry));
        }
    }
}

/// Concrete-class death work that native runs only after the shared Techno
/// death-weapon transaction has returned. Keeping the plan data-only avoids
/// consuming smudge RNG (or interning the InfDeath AnimType) too early.
enum ConcreteDeathSmudgePlan {
    Infantry(crate::sim::world::InfantryDeathPostlude),
    Building,
}

/// Build the native ReceiveDamage value ABI for one ordered area or direct
/// receiver record and run the shared receiver exactly once. The returned
/// signed HP delta is the only health input consumed by `commit_damage_events`.
#[derive(Debug, Clone, Copy)]
struct ResolvedReceiveDamage {
    outcome: damage::DamageOutcome,
    invulnerability_impact: Option<CombatLightRequest>,
}

fn receiver_effect_coord(
    target: &GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> ProjectileCoord {
    let x = i32::from(target.position.rx)
        .wrapping_mul(256)
        .wrapping_add(target.position.sub_x.to_num::<i32>());
    let y = i32::from(target.position.ry)
        .wrapping_mul(256)
        .wrapping_add(target.position.sub_y.to_num::<i32>());
    let z = object_world_z_leptons(target, terrain);
    ProjectileCoord::new(x, y, z)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildingReceivePrelude {
    Continue,
    Respond,
    ReturnZero,
}

/// Execute the BuildingClass wrapper work which natively precedes the shared
/// Techno receiver.
///
/// gamemd-derived: `BuildingClass__ReceiveDamage @ 0x00442230` returns zero at
/// `0x00442262` for disallowed self damage. A non-null attacker then writes the
/// victim owner's `House+0x54D8` at `0x0044229C`, before Building immunity,
/// the already-dead gate, and `TechnoClass__ReceiveDamage @ 0x00442425`.
fn apply_building_receive_prelude(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    houses: &mut BTreeMap<InternedId, HouseState>,
    current_tick: u64,
) -> BuildingReceivePrelude {
    let Some(target) = entities.get(event.target_id) else {
        return BuildingReceivePrelude::Continue;
    };
    if target.category != EntityCategory::Structure {
        return BuildingReceivePrelude::Continue;
    }
    let Some(target_type) = rules.object(interner.resolve(target.type_ref())) else {
        return BuildingReceivePrelude::Continue;
    };

    if event.attacker_id == event.target_id && !target_type.damage_self {
        return BuildingReceivePrelude::ReturnZero;
    }
    if event.attacker_id != RAD_NO_ATTACKER && !target_type.is_1x1_with_undeploy() {
        if let Some(owner) = houses.get_mut(&target.owner()) {
            owner
                .strategy_emergency
                .note_building_attack(current_tick as u32 as i32);
        }
    }

    BuildingReceivePrelude::Respond
}

fn resolve_receive_damage(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    houses: &BTreeMap<InternedId, HouseState>,
    alliances: &HouseAllianceMap,
    scenario_no_damage: bool,
    current_tick: u64,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<ResolvedReceiveDamage> {
    let distance_leptons = event.distance_leptons;
    let receiver_flags = event.receiver_flags;
    let target = entities.get(event.target_id)?;
    // Building442230 returns before Techno for Health0, after its wrapper
    // response (already executed by commit_entities). Infantry/Unit instead
    // delegate and can re-enter Techno's fatal tail at zero Health.
    if target.category == EntityCategory::Structure && target.health.current == 0 {
        return None;
    }
    let warhead = rules.warhead(interner.resolve(event.warhead_ref))?;
    let target_type = rules.object(interner.resolve(target.type_ref()));
    let source = (event.attacker_id != RAD_NO_ATTACKER)
        .then(|| entities.get(event.attacker_id))
        .flatten();
    let source_house = event
        .source_house
        .or_else(|| source.map(|entity| entity.owner()));
    // InfantryClass mutates the positive raw i32 before forwarding to the
    // shared Techno receiver. Its sign is therefore Techno's original-sign
    // snapshot used by the IC/FS gate below.
    let receiver_input = if infantry_spared_mutation(target, warhead, terrain) {
        0
    } else {
        infantry_prone_raw_damage(
            target,
            warhead,
            event.damage,
            receiver_flags.ignore_defenses,
        )
    };

    let allied = |asker: InternedId, other: InternedId| {
        crate::map::houses::is_allied_with(
            alliances,
            interner.resolve(asker),
            interner.resolve(other),
        )
    };
    let attacker_is_allied = source_house.is_some_and(|owner| allied(owner, target.owner()));
    let source_house_is_allied = source_house.is_some_and(|owner| allied(target.owner(), owner));

    let target_is_building = target.category == EntityCategory::Structure;
    let target_view = damage::TargetDamageView {
        armor: damage::ArmorClass(
            target_type
                .map(|object| armor_index(&object.armor))
                .unwrap_or(0) as u8,
        ),
        current_hp: i32::from(target.health.current),
        object_immune: target_type.is_some_and(|object| object.immune),
    };
    let type_immune = target_type.is_some_and(|object| object.type_immune)
        && source.is_some_and(|source| {
            source.type_ref() == target.type_ref() && source.owner() == target.owner()
        });
    // 701900 jumps to701BF6 when ignoreDefenses is set, before either linked
    // bunker branch. BlowUpBridge's forced C4 receiver must bypass both arms.
    let bunker_blocked = !receiver_flags.ignore_defenses
        && if target_is_building && target.bunker_occupant.is_some() {
            // Linked Building branch is intentionally the inverse of the installed
            // non-Building branch in TechnoClass::ReceiveDamage.
            warhead.penetrates_bunker
        } else {
            target.bunker_link.installed_in().is_some() && !warhead.penetrates_bunker
        };
    let active_invulnerability = target.invulnerability.as_ref().filter(|_| {
        crate::sim::superweapon::invulnerability::is_invulnerable(
            target.invulnerability.as_ref(),
            current_tick as u32,
        )
    });
    let gates = damage::ImmunityInputs {
        ignore_defenses: receiver_flags.ignore_defenses,
        attacker_present: event.attacker_id != RAD_NO_ATTACKER,
        type_immune,
        // IC/FS checks original sign and precedes WarpingOut. Warping does not
        // share the negative/healing exemption; both honor ignoreDefenses.
        invulnerable: !receiver_flags.ignore_defenses
            && receiver_input >= 0
            && active_invulnerability.is_some(),
        // `+0x270` (vtable `+0x1D4`, `0x00701AB1`): a Chrono teleport's
        // warp-out or a Temporal warp.
        warping_out: !receiver_flags.ignore_defenses && target.is_warped_out(),
        bunker_blocked,
        radiation_immune: warhead.radiation
            && target_type.is_some_and(|object| object.immune_to_radiation),
        psychic_immune: warhead.psychic_damage
            && target_type.is_some_and(|object| object.immune_to_psionic_weapons),
        poison_immune: warhead.poison && target_type.is_some_and(|object| object.immune_to_poison),
        affects_allies: warhead.affects_allies,
        attacker_is_allied,
        source_house_is_allied,
        psychedelic: warhead.psychedelic,
        psionics_immune: target_type.is_some_and(|object| object.immune_to_psionics),
        target_is_building,
    };
    // `HouseClass::GetArmorMultForType @ 0x0050BD30` on the target's owner:
    // its HouseType's per-category float. The difficulty and country
    // `Armor=` product (`House+0x1A0`) is never read here.
    let house_type_armor = houses.get(&target.owner()).map_or(1.0, |house| {
        target_type.map_or(1.0, |object| {
            rules.country_armor_mult_for_type(interner.resolve(house.house_type_id()), object)
        })
    });
    let rank_armor = target_type
        .is_some_and(|object| {
            self::veterancy::has_weapon_ability(
                self::veterancy::rank_from_u16(target.veterancy()),
                object,
                crate::rules::object_type::Ability::Stronger,
            )
        })
        .then(|| {
            crate::util::native_x87::NativeF64Bits::from_bits(rules.general.veteran_armor.to_bits())
        });
    let divisors = damage::DefenceDivisors {
        house_type_armor: crate::util::native_x87::NativeF32Bits::from_bits(
            house_type_armor.to_bits(),
        ),
        unit_armor: target.armor_multiplier,
        rank_armor,
    };
    // `arg6` stays on the ordered call: its Unit-class consumer is the crew
    // block, which the concrete death reads from the killing event.
    let outcome = damage::receive::receive_damage(
        receiver_input,
        warhead.cell_spread_f64,
        warhead.percent_at_max_f64,
        &warhead.verses_f64,
        &target_view,
        &divisors,
        &gates,
        distance_leptons,
        scenario_no_damage,
        rules.combat_damage.max_damage,
    );
    let invulnerability_impact = outcome.invulnerability_impact_damage.map(|damage| {
        let flags = match active_invulnerability
            .expect("receiver gate retained active state")
            .kind
        {
            crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain => 1,
            crate::sim::superweapon::invulnerability::InvulnKind::ForceShield => 6,
        };
        CombatLightRequest {
            target_id: Some(target.stable_id()),
            damage,
            warhead_ref: event.warhead_ref,
            coord: receiver_effect_coord(target, terrain),
            force_create: true,
            flags,
        }
    });
    Some(ResolvedReceiveDamage {
        outcome,
        invulnerability_impact,
    })
}

/// Inspect the value left in WaveClass::DamageArea's shared per-cell damage
/// local by one concrete Techno receiver. The receiver commit immediately
/// following this call runs the same pure ABI resolver against the same live
/// state; exposing the pointer result here avoids fabricating immutable damage
/// inputs for later occupants.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wave_post_object_damage(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    houses: &BTreeMap<InternedId, HouseState>,
    alliances: &HouseAllianceMap,
    scenario_no_damage: bool,
    current_tick: u64,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<i32> {
    resolve_receive_damage(
        event,
        entities,
        rules,
        interner,
        houses,
        alliances,
        scenario_no_damage,
        current_tick,
        terrain,
    )
    .and_then(|resolved| resolved.outcome.post_object_damage)
}

/// TechnoClass::ReceiveDamage builds the anger-node increment from the final
/// ObjectClass damage packet, not from raw weapon damage. The x87 keeps the
/// division result live, multiplies by the type's virtual cost, then Math__ftol
/// returns an i64 whose low dword is passed to HouseClass.
fn receiver_anger_delta(final_damage: i32, strength: i32, cost: i32) -> i32 {
    if strength == 0 {
        return 0;
    }
    let ratio = X87Chop53::div(
        X87Chop53::load_i32(final_damage),
        X87Chop53::load_i32(strength),
    )
    .expect("live Techno strength is nonzero");
    X87Chop53::ftol_i64(X87Chop53::mul(ratio, X87Chop53::load_i32(cost)))
        .expect("i32 damage/cost ratio fits native ftol i64") as i32
}

/// TechnoClass::ReceiveDamage PostMortem interpolation at 0x00701ED0.
/// `CellSpread` and `DelayKillAtMax` are native binary32 inputs; every other
/// operand is signed i32 and the two ftol results use their low dword.
fn postmortem_delay_duration(warhead: &WarheadType, distance_leptons: i32) -> i32 {
    let base = X87Chop53::load_i32(warhead.delay_kill_frames);
    let at_max = X87Chop53::load_f32(NativeF32Bits::from_bits(
        (warhead.delay_kill_at_max_f64 as f32).to_bits(),
    ))
    .expect("DelayKillAtMax parser retains a finite native f32");
    let slope = X87Chop53::sub(X87Chop53::mul(at_max, base), base);
    let denominator = cell_spread::whole_cells(warhead.cell_spread_f64).wrapping_shl(8);
    let Ok(slope_per_lepton) = X87Chop53::div(slope, X87Chop53::load_i32(denominator)) else {
        // Masked x87 divide-by-zero/non-finite conversion yields the integer
        // indefinite qword; Math__ftol returns its low dword, which is zero.
        return 0;
    };
    let delay = X87Chop53::add(
        base,
        X87Chop53::mul(X87Chop53::load_i32(distance_leptons), slope_per_lepton),
    );
    X87Chop53::ftol_i32_low_masked(delay)
}

fn postmortem_duration_for_event(
    event: &EntityDamageEvent,
    target: &GameEntity,
    rules: &RuleSet,
    interner: &StringInterner,
    state: damage::DamageState,
) -> Option<i32> {
    if state != damage::DamageState::Dead || target.category != EntityCategory::Structure {
        return None;
    }
    let warhead = rules.warhead(interner.resolve(event.warhead_ref))?;
    let object = rules.object(interner.resolve(target.type_ref()))?;
    (warhead.causes_delay_kill && object.eligible_for_delay_kill)
        .then(|| postmortem_delay_duration(warhead, event.distance_leptons))
}

fn has_active_area_invulnerability(entity: &GameEntity, current_tick: u64) -> bool {
    // Every GameEntity is a TechnoClass-derived object, so the native
    // AbstractFlags +0x14 bit-0 identity test is inherent in this store. The
    // virtual +0x160 result is the existing passive IC/FS timer predicate.
    crate::sim::superweapon::invulnerability::is_invulnerable(
        entity.invulnerability.as_ref(),
        current_tick as u32,
    )
}

fn event_arms_ic_isolation(
    event: &EntityDamageEvent,
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    event.near_center_ic_isolation_eligible
        && event.distance_leptons < 85
        && entities.get(event.target_id).is_some_and(|target| {
            has_active_area_invulnerability(target, current_tick)
                && target.invulnerability.as_ref().is_some_and(|state| {
                    state.kind == crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain
                })
        })
}

fn near_center_ic_isolation_armed(
    damage_events: &[EntityDamageEvent],
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    damage_events
        .iter()
        .any(|event| event_arms_ic_isolation(event, entities, current_tick))
}

fn area_near_center_ic_isolation_armed(
    receivers: &[combat_aoe::AreaDamageReceiver],
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    receivers.iter().any(|receiver| {
        matches!(receiver, combat_aoe::AreaDamageReceiver::Entity(event)
            if event_arms_ic_isolation(event, entities, current_tick))
    })
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
    /// Recursive/shrapnel bullet emissions awaiting the caller's admission.
    /// FireAt admits its own ordinary Bullet directly into the live Logic walk.
    pub(crate) projectile_spawns: Vec<ProjectileSpawn>,
    /// Native-order ReceiveDamage calls, including raw area records.
    pub(crate) damage_events: Vec<combat_aoe::AreaDamageReceiver>,
    pub(crate) remove_attack: Vec<u64>,
    pub(crate) fire_events: Vec<SimFireEvent>,
    /// aircraft that fired this tick
    pub(crate) ammo_deduct: Vec<u64>,
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

pub(crate) fn projectile_impact_cell(
    impact: ProjectileCoord,
) -> (u16, u16, SimFixed, SimFixed, i32) {
    let rx = crate::util::lepton::lepton_to_cell(impact.x).clamp(0, i32::from(u16::MAX)) as u16;
    let ry = crate::util::lepton::lepton_to_cell(impact.y).clamp(0, i32::from(u16::MAX)) as u16;
    (
        rx,
        ry,
        SimFixed::from_num(impact.x.rem_euclid(256)),
        SimFixed::from_num(impact.y.rem_euclid(256)),
        impact.z,
    )
}

/// The cell whose object lists a Map lookup answered: a real cell, by its own
/// coordinate. The shared dummy cell never holds an object: `MapClass::
/// Place_Down @ 0x005683C0`, the one `CellClass::AddContent` caller, skips a
/// missing cell.
fn cell_list_position(
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    cell: crate::map::cell_index::NativeCellIdentity,
) -> Option<(u16, u16)> {
    match cell {
        crate::map::cell_index::NativeCellIdentity::Real(_) => {
            let (x, y) = cells.coord(cell);
            Some((x as u16, y as u16))
        }
        crate::map::cell_index::NativeCellIdentity::Dummy => None,
    }
}

/// `BulletClass::SpawnShrapnel @ 0x0046A310` for the bullet at `location`,
/// its Location (`+0x9C`), which the count, the admission, the ring centre and
/// every fragment's origin read; the impact coordinate never reaches it.
fn emit_projectile_shrapnel(
    detonation: &ProjectileDetonation,
    location: ProjectileCoord,
    entities: &EntityStore,
    occupancy: &OccupancyGrid,
    terrain_object_cells: &BTreeMap<(u16, u16), u64>,
    rules: &RuleSet,
    interner: &mut StringInterner,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    house_alliances: &HouseAllianceMap,
    scenario_rng: &mut SimRng,
    native_unique_ids: &mut Option<crate::sim::native_identity::NativeUniqueIdCursor>,
    out: &mut CombatEmit,
) {
    let Some(parent_weapon) = rules.weapon(interner.resolve(detonation.payload.weapon)) else {
        return;
    };
    let Some(parent_projectile) = parent_weapon
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name))
    else {
        return;
    };
    let Some(child_weapon_name) = parent_projectile.shrapnel_weapon.as_deref() else {
        return;
    };
    let Some(child_weapon) = rules.weapon(child_weapon_name) else {
        return;
    };
    let Some(child_projectile) = child_weapon
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name))
    else {
        log::debug!(
            "Projectile {} shrapnel skipped: child projectile constructor unavailable",
            detonation.projectile_id
        );
        return;
    };
    let Some(child_warhead_name) = child_weapon.warhead.as_deref() else {
        return;
    };

    // `0x0046A336..0x0046A3C2`: a negative ShrapnelCount is lessened by the
    // whole cells from the bullet's Location to its firer's GetCoords
    // (`[bullet+0xB0]`, vt+0x48 at `0x0046A370`), through `0x0041C380` and a
    // signed /256; a bullet without a firer gets 3.
    let firer = entities.get(detonation.source_id);
    let distance_cells = firer.map_or(0, |firer| {
        let coord = crate::sim::movement::ground_pose::object_get_coords(firer, terrain);
        crate::util::native_x87::distance_3d_leptons(
            [coord.x, coord.y, coord.z],
            [location.x, location.y, location.z],
        ) / 256
    });
    let Some(count) = projectile_shrapnel_count(
        parent_projectile.shrapnel_count,
        firer.is_some(),
        distance_cells,
    ) else {
        return;
    };

    // The bullet's cell (`vt+0x1B8`, `0x0041BEA0`) centres the rings.
    let center_rx = crate::util::lepton::lepton_to_cell(location.x);
    let center_ry = crate::util::lepton::lepton_to_cell(location.y);
    // `0x0046A3D6..0x0046A406`: shrapnel is thrown only when the bullet's cell
    // (GetCell, vt+0x1BC, `0x005F6960`) holds a first ground object (`+0xE4`)
    // that is not a building (What_Am_I 6). The second GetCell (`0x0046A3F2`)
    // answers the same cell. A missed lookup's dummy cell holds none.
    let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    let bullet_cell = match cells.as_ref() {
        Some(cells) => cell_list_position(
            cells,
            crate::sim::movement::ground_pose::query_object_cell(
                cells,
                crate::sim::components::DriveCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
            ),
        ),
        // Mapless fixtures read the coordinate's own cell.
        None => u16::try_from(center_rx)
            .ok()
            .zip(u16::try_from(center_ry).ok()),
    };
    let ground_head = |cell: Option<(u16, u16)>| {
        let (rx, ry) = cell?;
        occupancy
            .cell_objects(
                rx,
                ry,
                crate::sim::movement::locomotor::MovementLayer::Ground,
                terrain_object_cells.get(&(rx, ry)).copied(),
            )
            .next()
    };
    let admitted = match ground_head(bullet_cell) {
        Some(crate::sim::occupancy::CellObjectMember::Entity(id)) => entities
            .get(id)
            .is_some_and(|object| object.category != EntityCategory::Structure),
        Some(crate::sim::occupancy::CellObjectMember::Terrain(_)) => true,
        None => false,
    };
    if !admitted {
        return;
    }
    // Random CellClass selections must capture their target coordinate at the
    // lookup call point: every miss returns the same mutable process dummy, so
    // resolving a collected list afterward would give all missed children the
    // final lookup's coordinate. Entity entries deliberately remain live reads
    // until child construction, preserving their existing behavior.
    let mut targets: Vec<(ProjectileTarget, Option<ProjectileCoord>)> =
        Vec::with_capacity(count as usize);
    // `0x0046A436..0x0046A48B`: the child weapon's Range (`+0xB4`) divided by
    // 256 toward zero names the last ring; rings 1 through it are walked,
    // never the bullet's cell. `0x0046A4E5..0x0046A4F5`: a ring cell's first
    // object is a target only while the bullet has a firer, so with the firer
    // gone every fragment takes a random cell.
    let rings = (child_weapon.range_leptons / 256).max(0) as usize;
    let ring_cells = if firer.is_some() {
        self::cell_spread::sweep(rings)
    } else {
        &[]
    };
    let source_owner = firer.map(|source| source.owner());
    for &(dx, dy) in ring_cells.iter().skip(1) {
        // `0x0046A496..0x0046A4D2`: the ring cell, its packed words added to
        // the centre's, through `MapClass::operator[] @ 0x005657A0`.
        let rx = (center_rx as i16).wrapping_add(dx);
        let ry = (center_ry as i16).wrapping_add(dy);
        let cell = match cells.as_ref() {
            Some(cells) => cell_list_position(cells, cells.lookup((rx, ry))),
            None => u16::try_from(rx).ok().zip(u16::try_from(ry).ok()),
        };
        // `0x0046A4D7..0x0046A509`: the head of its ground list, unless it is
        // the firer or the firer's house is allied to it
        // (`Is_Ally_ByObject @ 0x004F9A90`).
        let target_id = match ground_head(cell) {
            Some(crate::sim::occupancy::CellObjectMember::Entity(id)) => id,
            // A tree heading the list is a native target (a houseless object
            // is no ally), but a VERA bullet cannot target a TerrainClass:
            // the cell yields no fragment, and nothing behind the tree is read.
            Some(crate::sim::occupancy::CellObjectMember::Terrain(_)) | None => continue,
        };
        if target_id == detonation.source_id {
            continue;
        }
        let Some(target) = entities.get(target_id) else {
            continue;
        };
        let allied = source_owner.is_some_and(|owner| {
            crate::map::houses::are_houses_friendly(
                house_alliances,
                interner.resolve(owner),
                interner.resolve(target.owner()),
            )
        });
        if allied {
            continue;
        }
        targets.push((ProjectileTarget::Entity(target_id), None));
        // `0x0046A8D1..0x0046A8E0`: the walk ends once the count is reached,
        // tested after each fragment, so a zero count walks every ring and
        // takes no random cell (`0x0046A92C..0x0046A936`).
        if targets.len() == count as usize {
            break;
        }
    }
    while targets.len() < count as usize {
        let (rx, ry) = projectile_random_shrapnel_cell(center_rx, center_ry, scenario_rng);
        let selected = crate::sim::cell_rect::get_cellclass_fallback(terrain, rx, ry);
        let (target, initial_target_position) = if terrain.is_none() {
            // Rules-less/terrain-less fixtures historically retain a stable
            // Cell target and flat fallback surface. Native always has a map;
            // do not invent a persistent process-dummy pointer for this Rust
            // compatibility path without separate evidence.
            let target = ProjectileTarget::Cell {
                rx: rx as u16,
                ry: ry as u16,
            };
            (
                target,
                crate::sim::projectile::cell_target_coord(terrain, rx as u16, ry as u16),
            )
        } else {
            match selected {
                crate::sim::cell_rect::CellRef::Real(cell) => {
                    let target = ProjectileTarget::Cell {
                        rx: cell.rx,
                        ry: cell.ry,
                    };
                    (
                        target,
                        crate::sim::projectile::cell_target_coord(terrain, cell.rx, cell.ry),
                    )
                }
                crate::sim::cell_rect::CellRef::Dummy { cell } => (
                    ProjectileTarget::DummyCell,
                    crate::sim::projectile::dummy_cell_target_coord(&cell),
                ),
            }
        };
        targets.push((target, Some(initial_target_position)));
    }

    for (target, captured_target_coord) in targets {
        // The random-cell children (captured) launch through the second branch.
        let random_cell = captured_target_coord.is_some();
        let target_coord = if let Some(captured) = captured_target_coord {
            captured
        } else {
            match target {
                ProjectileTarget::Entity(id) => {
                    let Some(entity) = entities.get(id) else {
                        continue;
                    };
                    // `0x0046A614`: the object's GetCoords (vt+0x48), a
                    // building's foundation center (`0x00447AC0`).
                    let coord =
                        crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
                    ProjectileCoord::new(coord.x, coord.y, coord.z)
                }
                ProjectileTarget::Cell { rx, ry } => {
                    crate::sim::projectile::cell_target_coord(terrain, rx, ry)
                }
                ProjectileTarget::None => ProjectileCoord::new(0, 0, 0),
                ProjectileTarget::DummyCell => terrain
                    .map(crate::map::resolved_terrain::ResolvedTerrainGrid::shared_cell_dummy)
                    .as_ref()
                    .map(crate::sim::projectile::dummy_cell_target_coord)
                    .unwrap_or(ProjectileCoord::new(0, 0, 0)),
            }
        };
        let native_unique_id =
            crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(native_unique_ids);
        out.projectile_spawns.push(ProjectileSpawn {
            native_unique_id,
            line_trail: crate::sim::projectile::ProjectileLineTrail::from_type(
                child_projectile,
                rules.general.line_trail_color_override,
            ),
            flat: child_projectile.flat,
            source_id: detonation.source_id,
            origin: location,
            target,
            initial_target_position: target_coord,
            payload: ProjectilePayload::new(
                child_weapon.damage,
                interner.intern(child_warhead_name),
                interner.intern(child_weapon_name),
            ),
            speed_leptons_per_frame: child_weapon.speed.clamp(1, i32::from(u16::MAX)) as u16,
            velocity: crate::sim::projectile::launch::shrapnel_launch_velocity(
                location,
                target_coord,
                child_weapon.speed,
                random_cell,
            ),
            // Child launch owner: BulletClass::Shrapnel @ 0x0046A310.
            trajectory: ProjectileTrajectory::Ballistic,
            guidance: None,
            visual: ProjectileVisualState::new(
                child_projectile.anim_low as u8,
                child_projectile.anim_high as u8,
                child_projectile.anim_rate as u8,
            ),
            arm_frames: projectile_arm_delay(child_projectile.arm, target, entities),
            fuse_frames: None,
            ranged_fuse: child_projectile.rot > 0 || child_projectile.ranged,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
            collision: ProjectileCollisionPolicy {
                level: child_projectile.level,
                subject_to_walls: child_projectile.subject_to_walls,
                native_cell_collision: child_projectile.rot <= 0,
                dropping: child_projectile.dropping,
                subject_to_cliffs: child_projectile.subject_to_cliffs,
                flak_scatter: child_projectile.flak_scatter,
                anti_air: child_projectile.aa,
                airburst: child_projectile.airburst,
                inaccurate: child_projectile.inaccurate,
                floater: child_projectile.floater,
                elasticity_bits: child_projectile.elasticity.to_bits(),
                arcing: child_projectile.arcing,
            },
        });
    }
}

/// Outputs produced by one Bullet Logic slot after its detonation receivers
/// have committed, but before the world retires the Bullet object itself.
pub(crate) struct LogicProjectileCommit {
    pub(crate) projectile_spawns: Vec<ProjectileSpawn>,
    pub(crate) effects: DeathEffects,
    pub(crate) under_attack_events: Vec<UnderAttackEvent>,
}

/// Build the per-attacker fire snapshot from current entity state. PURE READ —
/// the caller has already decremented cooldown/burst-delay for this tick and
/// read any garrison's fire state. Single source of the snapshot field-reads so
/// non-Unit class hosts and the Unit live Fire→Facing host use the same
/// field reads.
pub(crate) fn build_attacker_snapshot(
    entity: &GameEntity,
    target: TargetKind,
    garrison: Option<GarrisonSnapshot>,
) -> AttackerSnapshot {
    AttackerSnapshot {
        stable_id: entity.stable_id(),
        owner: entity.owner(),
        category: entity.category,
        target,
        pos_rx: entity.position.rx,
        pos_ry: entity.position.ry,
        pos_z: entity.position.z,
        pos_exact_z_leptons: entity.position.exact_z_leptons,
        sub_x: entity.position.sub_x,
        sub_y: entity.position.sub_y,
        type_id: entity.type_ref(),
        veterancy: entity.veterancy(),
        barrel_facing: entity.barrel_facing,
        hull_facing: entity.body_facing,
        garrison,
        scan_mission: threat_range::scan_mission_for(entity),
        building_shot: None,
    }
}

/// Pay one kill's veterancy award ([`record_the_kill`]'s `points`); the
/// recipient's own cost divides it inside `VeterancyClass::Add @ 0x0074FF50`.
///
/// Who receives it is the native redirection chain at
/// `0x00702E9D..0x00702FF0`, in this order (`EDI` is the killer):
/// 1. `killer+0x82` (`InOpenTransport`, set by
///    `TechnoClass::SetInOpenTransport @ 0x00710470`) AND `killer+0x11C`
///    (`Transporter`, written beside it in `InfantryClass::PerCellProcess` at
///    `0x0051A463`) non-null AND the transporter's type `Trainable=` → the
///    TRANSPORTER, with the transporter's cost (`0x00702EA7..0x00702EF0`).
/// 2. else the killer's own type `Trainable=` → the killer
///    (`0x00702EF5..0x00702F2C`).
/// 3. else the killer's type `MissileSpawn=` (`+0xD68`) AND `killer+0x2D4`
///    (spawn owner) non-null AND its type `Trainable=` → the spawn OWNER
///    (`0x00702F31..0x00702F96`); stock `V3ROCKET`/`DMISL`/`CMISL` are
///    `Trainable=no, MissileSpawn=yes`, which is how a V3 promotes.
/// 4. else an occupied Building (`vtable+0x400`, RTTI 6) → the occupant at
///    the building's fire index (`+0x688[+0x69C]`, `0x00702F98..0x00702FEA`),
///    with no `Trainable=` test on it. A BuildingType is untrainable by
///    default (constructor `0x0045E42E`) and no retail occupiable type sets
///    the key, so every garrison kill lands here: its occupants promote, the
///    building never does. FireAt has already advanced the index past the
///    shooter (`0x006FF031..0x006FF085`) when its bullet kills, so with
///    several occupants the NEXT one in line is paid. (VERA's line runs in
///    reverse entry order: see `passenger::PassengerCargo`.)
///    RESIDUAL: native reads that slot unchecked; VERA pays nobody when the
///    index is past the occupant count ([`combat_weapon::firing_occupant_id`]).
///    Trigger: occupants leaving, but not all of them, between the shot and
///    the kill. Effect: native reads a stale slot. Frequency: rare.
/// 5. else nobody.
///
/// Every cost on both sides is the type's Cost_Of for the VICTIM's house
/// (`victim_house`; `0x00702ED1`, `0x00702F13`, `0x00702F77`, `0x00702FD4`).
fn award_kill_experience(
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    killer_id: u64,
    victim_id: u64,
    points: i32,
    victim_house: Option<&HouseCostFactors>,
) {
    if killer_id == RAD_NO_ATTACKER || killer_id == victim_id {
        return;
    }
    let cost_of = |object: &ObjectType| rules.cost_of(object, victim_house);
    let Some(killer) = entities.get(killer_id) else {
        return;
    };
    let Some(killer_type) = rules.object(interner.resolve(killer.type_ref())) else {
        return;
    };
    let trainable_cost = |id: u64| -> Option<(u64, i32)> {
        let object = rules.object(interner.resolve(entities.get(id)?.type_ref()))?;
        object.trainable.then_some((id, cost_of(object)))
    };
    // Branch 1: a passenger firing from an OpenTopped transport pays its
    // transporter (the `+0x82`/`+0x11C` pair).
    let open_transporter = killer.passenger_role.open_transport_id();
    let recipient = if let Some(transporter) = open_transporter.and_then(trainable_cost) {
        Some(transporter)
    } else if killer_type.trainable {
        // Branch 2: the killer itself.
        Some((killer_id, cost_of(killer_type)))
    } else if killer_type.missile_spawn {
        // Branch 3: a spawned missile pays its launcher.
        killer.spawn_owner_id.and_then(trainable_cost)
    } else {
        // Branch 4: an occupied building pays the occupant at its fire index.
        combat_weapon::firing_occupant_id(killer, killer_type).and_then(|occupant| {
            let occupant_type =
                rules.object(interner.resolve(entities.get(occupant)?.type_ref()))?;
            Some((occupant, cost_of(occupant_type)))
        })
    };
    let Some((recipient_id, recipient_cost)) = recipient else {
        return;
    };
    if let Some(recipient) = entities.get_mut(recipient_id) {
        self::veterancy::award_kill(
            recipient,
            recipient_cost,
            points,
            true,
            rules.general.veteran_ratio,
            rules.general.veteran_cap,
        );
    }
}

/// The caller decides whether this callback leaves the victim alive. Native
/// RecordKill always books immediately; this controls only VERA's fallback for
/// teardown sites whose native callback is still unrepresented.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KillCallback {
    Terminal,
    OwnerChange,
}

/// `TechnoClass::Record_The_Kill @ 0x00702D40` for `victim_id`, destroyed by
/// `killer_id` (none for a death without an attacker) and credited to
/// `killer_owner`: the kill/score record and the experience award
/// ([`award_kill_experience`]). Every cost it reads is Cost_Of for the victim's
/// house, which it loads once (`0x00702D61`). One award feeds both the score
/// (`0x0070300F`) and the experience: the victim's cost, zero when the
/// killer's house is allied with the victim, else doubled for a veteran or
/// tripled for an elite victim, from the rank it died at. It is not
/// `Points=`, which gamemd parses but never reads back (TS legacy).
///
/// Every routed lethal path calls it at the instant of the kill, while the
/// victim's veterancy is still the value it died at. ChangeOwner7015A8 also
/// calls it with no attacker before the owner store: that call immediately
/// books the old House's loss while the object remains alive. Each actual
/// native callback books independently; there is no first-credit guard in
/// 702D40. The UnInit fallback guard prevents an additional deferred record,
/// rather than suppressing this callback. Spawner missiles
/// credit their launcher's house; if the launcher dies during the missile's
/// flight, the kill goes uncredited.
///
/// NOT routed yet, because each site would need a `&RuleSet` threaded into a
/// function that does not take one: the Iron Curtain and Genetic Mutator
/// infantry kills, aircraft self-destruct, passengers dying with their
/// transport (`world::lifecycle`) or with a collapsing bridge, and passengers
/// ejected by a sell. Those victims book a Loss with no matching Kill.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_the_kill(
    entities: &mut EntityStore,
    houses: &mut BTreeMap<InternedId, HouseState>,
    interner: &StringInterner,
    alliances: &HouseAllianceMap,
    victim_id: u64,
    killer_id: Option<u64>,
    killer_owner: Option<InternedId>,
    callback: KillCallback,
    rules: &RuleSet,
) {
    let Some(victim) = entities.get(victim_id) else {
        return;
    };
    // `DontScore=` (`+0xC9F`) returns before any bookkeeping. Stock sets
    // it on `SLAV`, `V3ROCKET`, `DMISL` and `CMISL`: without it every missile
    // shot down would promote its interceptor.
    if victim.dont_score {
        return;
    }
    let victim_owner = victim.owner();
    let victim_house = houses
        .get(&victim_owner)
        .map(crate::sim::house_state::HouseState::cost_factors);
    let victim_type = rules.object(interner.resolve(victim.type_ref()));
    let insignificant_building = victim.category == EntityCategory::Structure
        && victim_type.is_some_and(|object| object.insignificant);
    let victim_cost = victim_type.map_or(0, |object| rules.cost_of(object, victim_house.as_ref()));
    // `0x00702E64` asks the killer's house `HouseClass::IsAlly @ 0x004F9A90`,
    // which reads only the asker's own ally bits: a one-way test.
    let asker = killer_id
        .and_then(|id| entities.get(id))
        .map(|killer| killer.owner())
        .or(killer_owner);
    let allied = asker.is_some_and(|asker| {
        crate::map::houses::is_allied_with(
            alliances,
            interner.resolve(asker),
            interner.resolve(victim_owner),
        )
    });
    let points = self::veterancy::kill_award_points(
        victim_cost,
        self::veterancy::rank_of(victim.veterancy_raw),
        allied,
    );
    if let Some(killer_id) = killer_id {
        award_kill_experience(
            entities,
            rules,
            interner,
            killer_id,
            victim_id,
            points,
            victim_house.as_ref(),
        );
    }
    // 702FF0 awards experience before 703003..7031DC book the House fields.
    if let Some(victim) = entities.get_mut(victim_id) {
        record_kill_credit(
            victim,
            houses,
            killer_owner,
            points,
            insignificant_building,
            callback,
        );
    }
}

impl crate::sim::world::Simulation {
    /// [`record_the_kill`] in this world.
    pub(crate) fn record_the_kill(
        &mut self,
        victim_id: u64,
        killer_id: Option<u64>,
        killer_owner: Option<InternedId>,
        callback: KillCallback,
        rules: &RuleSet,
    ) {
        record_the_kill(
            &mut self.substrate.entities,
            &mut self.houses,
            &self.interner,
            &self.house_alliances,
            victim_id,
            killer_id,
            killer_owner,
            callback,
            rules,
        );
    }
}

/// The immediate House-accounting half of RecordKill70300F..7031DC.
/// MatchStatistics owns all mutations; retained fatal attribution is an
/// observation, never a second deferred score authority. A Temporal erase
/// calls this at full health (71AAC4), so its later UnInit must also skip the
/// fallback. FootCrash4DEC51 calls at full health before4DEC72 zeroes it, so
/// health/attacker alone cannot identify a terminal callback. An attacker-free
/// live ChangeOwner callback leaves the existing death guard untouched, since
/// the object remains alive afterwards.
///
/// RESIDUAL: sale completion44A1EF writes Building+53C=-1 before its NULL kill
/// callback44A1F9, suppressing only BuildingsLost703054. VERA's existing sale
/// route does not retain that field or call RecordKill; Selling alone is not
/// its substitute. Live Tag callbacks and the radar redraw remain with those
/// separate mechanisms. This accounting change does not certify either.
fn record_kill_credit(
    victim: &mut crate::sim::game_entity::GameEntity,
    houses: &mut BTreeMap<InternedId, HouseState>,
    killer_owner: Option<InternedId>,
    points: i32,
    insignificant_building: bool,
    callback: KillCallback,
) {
    victim.killed_by = killer_owner;
    if callback == KillCallback::Terminal
        || victim.health.current == 0
        || killer_owner.is_some()
        || victim.crashing
    {
        victim.destruction_recorded = true;
    }
    // Score is added before the Building Insignificant branch703045.
    if let Some(killer) = killer_owner
        && let Some(house) = houses.get_mut(&killer)
    {
        house.economy.add_score(points);
    }
    if !insignificant_building {
        if let Some(house) = houses.get_mut(&victim.owner()) {
            house.stats.record_loss(victim.category);
        }
        if let Some(killer) = killer_owner
            && let Some(house) = houses.get_mut(&killer)
        {
            house.stats.record_kill(victim.category);
        }
    }
}

/// Squared distance in leptons from raw coordinates.
///
/// Takes individual fields rather than a `&Position`, for use with snapshots
/// where positions are destructured.
pub(crate) fn lepton_distance_sq_raw(
    ax_cell: u16,
    ay_cell: u16,
    ax_sub: SimFixed,
    ay_sub: SimFixed,
    bx_cell: u16,
    by_cell: u16,
    bx_sub: SimFixed,
    by_sub: SimFixed,
) -> i64 {
    let ax: i64 = ax_cell as i64 * 256 + ax_sub.to_num::<i64>();
    let ay: i64 = ay_cell as i64 * 256 + ay_sub.to_num::<i64>();
    let bx: i64 = bx_cell as i64 * 256 + bx_sub.to_num::<i64>();
    let by: i64 = by_cell as i64 * 256 + by_sub.to_num::<i64>();
    let dx: i64 = ax - bx;
    let dy: i64 = ay - by;
    dx * dx + dy * dy
}

/// Check if a squared lepton distance is within weapon range.
///
/// Converts weapon range from cells to leptons (×256) before squaring.
/// Uses i64 to match `lepton_distance_sq_raw()` output.
///
/// The scale runs on the fixed-point bits, not through `to_num`, because
/// `CCINIClass::ReadRange` 0x00474620 multiplies BEFORE truncating: a
/// `Range=1.5` weapon reaches 384 leptons, and truncating to whole cells first
/// cost it a third of its reach.
///
/// RESIDUAL — this 2-D twin does NOT honour the `-512` always-in-range
/// sentinel that `in_range::compute_in_range` does. `TechnoClass::InRange`
/// 0x006F7220 tests it first of all (`CMP EDI,0xFFFFFE00` at 0x006F724E) and
/// returns true; here `Range=-2` scales to `-512` leptons, squares to a
/// positive `262144`, and reads as a two-cell reach.
///
/// - Trigger: any consumer of this function firing a `Range=-2` weapon —
///   `ASWLauncher` (`[DEST]`/`[CDEST]` Destroyer secondary), `MakeupKit`
///   (`[SPY]` primary), and the unreferenced `TankMakeupKit`/`CRMakeupKit`.
/// - Player effect: a Destroyer's anti-submarine weapon reads as in range only
///   within two cells, where gamemd is always in range.
/// - Frequency: only the no-resolved-terrain fallbacks listed under RESIDUAL 2
///   reach this predicate.
/// - Downstream risk: those fallbacks agree with each other, so the drift is
///   consistent rather than self-correcting.
///
/// RESIDUAL 2 — this twin also has no line-of-fire walk, no elevation bonus
/// and no occupied arm. `TechnoClass::InRange` ends in `CALL 0x004CC310` at
/// 0x006F7642 and refuses the shot when a wall or a cliff sits on the line, it
/// adds `0x006F6F60`'s bonus for a `SubjectToElevation=` projectile fired down
/// at a lower target, and it measures an occupied building with its reach
/// (`0x006F727E`); `compute_in_range` does all three (see
/// `sim::combat::line_of_fire`, `in_range::elevation_bonus_leptons` and
/// `combat_weapon::occupied_reach_cells`) and this function does none.
///
/// Every reader is a fallback for a world without resolved terrain (headless
/// fixtures, pre-map bring-up): the fire gate, the threat scan, pursuit and
/// the cursor. None of them can run a walk, so they agree with each other
/// rather than diverging.
pub(crate) fn is_within_range_leptons(dist_sq_leptons: i64, range_cells: SimFixed) -> bool {
    let range_leptons: i64 = (i64::from(range_cells.to_bits()) * 256) >> 16;
    let range_sq: i64 = range_leptons * range_leptons;
    dist_sq_leptons <= range_sq
}

pub(crate) use self::combat_targeting::acquire_best_target_for_entity;
/// The threat mask an acquisition callsite pushes into
/// `TechnoClass::Greatest_Threat @ 0x006F8DF0`, plus the passive block's own
/// derivation of it. Re-exported because the mask is chosen by the mission
/// handlers in `sim/world/`, not inside `combat/`.
pub use self::threat_range::ScanMission;
pub(crate) use self::threat_range::scan_mission_for;

/// Ordinary launch-height, resolved impact presentation and damage-effect
/// wiring. Bridge Cell+58 launch and +48/+58 impact comparisons use the native
/// oracle cases in `bridge_launch_tests` and `world::projectile_collision`.
#[cfg(test)]
mod impact_height_tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::sim::intern::test_interner;

    const TEST_GRID: u16 = 16;
    /// Terrain floor used by every raised-ground case here. Chosen because two
    /// levels is what the reported screenshot showed: one whole tile of
    /// vertical error.
    const RAISED_LEVEL: u8 = 2;

    #[test]
    fn gsi_04_11_refinery_survivor_cells_follow_native_sentinel_offsets() {
        let SmudgeSpawnRequest::BuildingCenter {
            foundation_w,
            foundation_h,
            ..
        } = building_center_smudge_request(10, 20, 3, "3x3Refinery")
        else {
            panic!("the destruction-center mark");
        };
        assert_eq!((foundation_w, foundation_h), (3, 3));
        let survivor_cells = crate::sim::crew_survival::foundation_cells(10, 20, "3x3Refinery");
        assert_eq!(
            survivor_cells,
            vec![
                (10, 20),
                (11, 20),
                (12, 20),
                (10, 21),
                (11, 21),
                (10, 22),
                (11, 22),
                (12, 22),
            ]
        );
        assert!(!survivor_cells.contains(&(12, 21)));
    }

    /// `TechnoClass::ReceiveDamage`'s metallic debris loop
    /// (`0x007024E0..0x0070256B`, `tools/spatial_oracle/bridge_debris_producer.py`,
    /// executed with the whole AnimClass constructor per piece) on retail
    /// rules and art: each piece's `MetallicDebris=` pick, then that piece's
    /// constructor draws, before the next pick. The budget is pinned (equal
    /// bounds take no draw) so the rows start at the loop.
    #[test]
    fn debris_pieces_construct_between_picks_as_the_original() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
            return;
        };
        let mut rules =
            crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bridge_debris_producer.json",
        ))
        .unwrap();
        // Use the exact retained image-header inputs supplied to the original
        // constructor corpus; the old draw-only test omitted asset binding.
        let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
        for native in golden["retail_anim_types"].as_array().unwrap() {
            if native["source"]["art_body_read"] == true {
                registry.bind_anim_frame_count_for_test(
                    native["name"].as_str().unwrap(),
                    native["frames"].as_i64().unwrap() as i32,
                );
            }
        }
        rules.install_art_data(registry);
        let rows = golden["death_loop"].as_array().unwrap();
        assert_eq!(rows.len(), 12);
        for row in rows {
            let input = &row["input"];
            let pieces = input["pieces"].as_i64().unwrap() as i32;
            let mut object_type = rules.object("MTNK").unwrap().clone();
            object_type.max_debris = pieces + 1;
            object_type.min_debris = pieces;
            object_type.debris_types.clear();
            object_type.debris_anims.clear();
            let coord = input["coord"].as_array().unwrap();
            let at = |i: usize| coord[i].as_i64().unwrap() as i32;
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_before"].as_str().unwrap()
            );
            let mut voxels = Vec::new();
            let mut effects = Vec::new();
            throw_debris_for_death(
                &mut sim,
                &object_type,
                &rules,
                owner,
                glam::IVec3::new(at(0), at(1), at(2)),
                &mut voxels,
                &mut effects,
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}"
            );
            let names: Vec<String> = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["call"] == "anim_ctor")
                .map(|event| event["type"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(
                sim.substrate
                    .anims
                    .iter()
                    .map(|(_, anim)| sim.interner.resolve(anim.type_id).to_string())
                    .collect::<Vec<_>>(),
                names,
                "{input}"
            );
            assert!(
                effects.is_empty() && voxels.is_empty(),
                "live constructors leave no deferred packets"
            );
            assert_eq!(sim.logic_order().len(), names.len());
            for ((_, anim), native) in sim
                .substrate
                .anims
                .iter()
                .zip(row["anims"].as_array().unwrap())
            {
                let location = native["location"].as_array().unwrap();
                assert_eq!(
                    [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                    std::array::from_fn(|i| location[i].as_i64().unwrap() as i32),
                    "{input}"
                );
                if native["is_bouncing"].as_u64().unwrap() == 0 {
                    assert!(
                        anim.bounce.is_none(),
                        "unread retail D is not a Bouncer: {input}"
                    );
                    assert_eq!(native["type"], "D");
                    continue;
                }
                let body = anim.bounce.expect("an ART-read bouncing chunk");
                let bits = |key: &str, i: usize| native["bounce"][key][i].as_u64().unwrap() as u32;
                for axis in 0..3 {
                    assert_eq!(body.position[axis].bits(), bits("position_bits", axis));
                    assert_eq!(body.velocity[axis].bits(), bits("velocity_bits", axis));
                }
            }
        }
    }

    fn terrain_cell(rx: u16, ry: u16, level: u8) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            level,
            filled_clear: true,
            terrain_class: Default::default(),
            accepts_smudge: true,
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        }
    }

    pub(super) fn terrain_at_level(level: u8) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(TEST_GRID, TEST_GRID, |rx, ry| {
            terrain_cell(rx, ry, level)
        })
    }

    /// Armed tank plus a warhead that emits an impact animation, so a
    /// force-fire produces an observable `ExplosionEffect`.
    fn impact_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "\
[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[Warheads]\n0=AP\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nAnimList=TWLT070\n",
        );
        RuleSet::from_ini(&ini).expect("impact rules should parse")
    }

    #[test]
    fn ordinary_cell_launch_uses_terrain_ground_leptons() {
        let entities = EntityStore::new();
        let flat = terrain_at_level(0);
        let raised = terrain_at_level(RAISED_LEVEL);

        assert_eq!(
            attack_world_z_leptons(TargetKind::Cell(7, 9), &entities, Some(&raised)),
            208,
            "a non-bridge Cell+58 aim is its ground surface in leptons"
        );
        assert_eq!(
            attack_world_z_leptons(TargetKind::Cell(7, 9), &entities, Some(&flat)),
            0,
            "level-0 ground is still zero — that is the value, not the fallback"
        );
        assert_eq!(
            attack_world_z_leptons(TargetKind::Cell(7, 9), &entities, None),
            0,
            "no loaded map means no cell to read"
        );
        assert_eq!(
            attack_world_z_leptons(
                TargetKind::Cell(TEST_GRID + 5, TEST_GRID + 5),
                &entities,
                Some(&raised)
            ),
            0,
            "mapless test boundary; production fallback targets retain the shared CellClass"
        );
    }

    /// The impact byte is a signed level count on both sides of the sim/app
    /// boundary, because the projection decodes it with `as i8`.
    ///
    /// Catches the two-narrowings shape error: clamping into `u8` range lets
    /// 200 through, which the projection reads back as -56 levels and draws
    /// 840 px away, while clamping into `i8` range saturates at the top of the
    /// domain the reader actually decodes.
    #[test]
    fn impact_z_byte_saturates_in_the_signed_domain_the_projection_decodes() {
        for level in [0_i32, 1, 2, 14, 127] {
            assert_eq!(
                i32::from(impact_z_byte(level) as i8),
                level,
                "every reachable map height must survive the round trip"
            );
        }
        assert_eq!(
            impact_z_byte(200) as i8,
            i8::MAX,
            "an over-range height saturates at the top of the signed domain, it \
             does not wrap to a large negative one"
        );
        assert_eq!(impact_z_byte(-40) as i8, -40, "below-ground z stays signed");
        assert_eq!(impact_z_byte(-9000) as i8, i8::MIN);
    }

    #[test]
    fn entity_launch_uses_the_object_coordinate() {
        let mut entities = EntityStore::new();
        let mut on_deck = GameEntity::test_default(1, "MTNK", "Americans", 7, 9);
        on_deck.position.z = 6;
        on_deck.on_bridge = true;
        entities.insert(on_deck);
        let terrain = terrain_at_level(RAISED_LEVEL);

        assert_eq!(
            attack_world_z_leptons(TargetKind::Entity(1), &entities, Some(&terrain)),
            624,
            "the object-owned bridge layer raises its ground coordinate"
        );
        assert_eq!(
            attack_world_z_leptons(TargetKind::Entity(1), &entities, None),
            624,
            "the mapless fallback already includes the object layer"
        );
        assert_eq!(
            attack_world_z_leptons(TargetKind::Entity(404), &entities, Some(&terrain)),
            0,
            "a vanished target contributes nothing"
        );
    }

    #[test]
    fn force_fire_on_raised_ground_places_the_explosion_at_the_terrain_height() {
        let mut rules = impact_rules();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
            &IniFile::from_str(""),
        ));
        let mut terrain = terrain_at_level(RAISED_LEVEL);
        let mut store = EntityStore::new();
        // `test_interner` snapshots the thread-local, so the entity's type and
        // owner strings must be interned before the snapshot is taken.
        let mut firer = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        // This hand-placed combat fixture represents a revealed actor. The
        // live Unit firing host correctly skips constructor-only limbo state.
        firer.lifecycle.in_limbo = false;
        // The fixture `MTNK` authors no `Turret=`, so its HULL is what the
        // native body gate compares (`UnitClass::GetFireError @ 0x00740FD0`
        // step 17). Face it south at the force-fire cell so this test measures
        // impact height, not turn-to-fire.
        firer.body_facing.snap(0x8000, 0);
        store.insert(firer);
        let mut interner = test_interner();
        assert!(
            install_cell_attack_target_for_test(&mut store, 1, 5, 6, Some(&rules), &interner),
            "armed tank should accept a force-fire order on an adjacent cell"
        );

        let mut scenario_rng = SimRng::new(1);
        let result = tick_combat_with_fog(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            None,
            &BTreeMap::<InternedId, PowerState>::new(),
            None,
            None,
            None,
            Some(&mut terrain),
            0,
            100,
            0,
            &[1],
            None,
            &mut scenario_rng,
        );

        let effect = result
            .fixture_anims
            .first()
            .expect("force-fire should construct the warhead's impact animation");
        let (rx, ry, _, _, _) = effect.world_coord.to_cell_sub_z();
        assert_eq!((rx, ry), (5, 6));
        assert_eq!(
            effect.world_coord.z,
            i32::from(RAISED_LEVEL) * 104,
            "the impact animation is placed at the impact height; a constant 0 \
             draws it 15 screen pixels per level below the ground it hit"
        );
    }
}
