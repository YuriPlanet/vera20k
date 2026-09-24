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
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

pub(crate) mod base_defense_response;
pub mod burst;
pub(crate) mod cell_spread;
pub(crate) mod combat_aoe;
pub(crate) mod combat_fire_gate;
pub(crate) mod combat_targeting;
pub(crate) mod combat_weapon;
pub(crate) mod damage;
pub(crate) mod destruction_effects;
pub(crate) mod fire_coord;
pub(crate) mod fire_error;
pub(crate) mod fire_error_world;
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
    tick_combat_with_fog_and_main_rng_with_terrain_area,
};
pub(crate) mod line_of_fire;
pub(crate) mod parasite;
pub mod smudge_dispatch;
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
#[path = "delayed_building_fire_tests.rs"]
mod delayed_building_fire_tests;

use std::collections::{BTreeMap, BTreeSet};

use self::combat_weapon::{WeaponSlot, select_weapon_against, select_weapon_slot};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
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
use crate::sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS;
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
    ProjectilePayload, ProjectileSpawn, ProjectileTarget, ProjectileTrajectory, ProjectileVelocity,
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
use crate::sim::wave::WaveDamageEvent;
use crate::sim::world::{FireOriginSnapshot, SimFireEvent, SimSoundEvent};
use crate::util::fixed_math::{SIM_ZERO, SimFixed, sim_to_i32};
use crate::util::lepton::{LEPTONS_PER_LEVEL, ground_height_leptons};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

use super::animation::SequenceKind;
use super::game_entity::{GameEntity, PendingBuildingFire};
use super::occupancy::OccupancyGrid;
use super::production::foundation_dimensions;

/// RA2 runs at 15 logical frames per second. ROF values are in frames.
/// Radius in cells that RevealOnFire clears shroud around the fire location.
const REVEAL_ON_FIRE_RADIUS: u16 = 3;
/// Step size for selecting explosion anim from a warhead's AnimList: idx = damage / 25.
const ANIM_LIST_DAMAGE_STEP: u16 = 25;

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

/// Explicitly classified delivery decision at weapon fire.
///
/// Unsupported projectile behaviors intentionally remain on the established
/// immediate path until their own native trajectory contracts are ported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectileDelivery {
    Persistent {
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
    },
    Immediate(ImmediateProjectileReason),
}

/// The bounded lifecycle never silently treats an unsupported bullet as a
/// straight ordinary shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImmediateProjectileReason {
    NoProjectile,
    MissingProjectileType,
    Invisible,
}

/// Which delivery path a weapon's shot takes.
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
/// RESIDUAL (GSI-08.07) — the second scatter site is not modelled, because one
/// of its operands is unidentified. `BulletClass::Fire @ 0x004687B4` offsets an
/// `Inviso && FlakScatter` bullet's already-resolved target coordinate, drawing
/// `Random__RandomRanged(0, RulesClass+0x1734 << 1)` for the magnitude and
/// `Random__RandomRanged(0, 0x7FFFFFFE)` for the angle. **The blocker is the
/// divisor.** The magnitude is `(roll * ftol(dist)) / *(*(Bullet+0x130) + 0xB4)`
/// — `0x004687D9 IMUL ESI,EAX`, `0x004687DC MOV ECX,[EBX+0x130]`,
/// `0x004687E5 IDIV dword ptr [ECX+0xB4]` — and neither `Bullet+0x130`'s
/// referent nor its `+0xB4` field has been identified, so the offset cannot be
/// computed at all today. It is NOT the weapon `Range=`; an earlier note here
/// said so and was wrong.
///
/// Two facts for whoever implements it. First, the site OFFSETS, walked in
/// assembly this session: `0x00468884 CALL Math__CosFromTable / 0x00468889 FMUL
/// <mag> / 0x00468890 FIADD dword ptr [ESP+0x44]` and `0x00468864 CALL
/// Math__SinFromTable / 0x00468869 FMUL <mag> / 0x0046886D FSUBR double ptr
/// [ESP+0x38]` — `x += cos(theta)*mag`, `y -= sin(theta)*mag`, the same shape as
/// the verified launch site at `0x006FE7E5`/`0x006FE7C0`. The decompiler renders
/// it as a plain assignment (the dropped-`FIADD` artifact that made the mapping
/// ledger wrong at the launch site); do not follow that rendering. Second, VERA
/// resolves an `Inviso` shot on the immediate path, where the impact coordinate
/// feeds area damage, wall routing, bridge damage, radiation and animation
/// placement, so wiring the offset in touches all of those consumers.
///
/// Trigger: every Flak Cannon / Flak Track shot at an aircraft (`[FlakProj]`,
/// 6 weapons). Player effect: flak never misses — the miss distance itself is
/// not yet derivable. Frequency: any skirmish with air units. Downstream risk:
/// two Scenario RNG draws are missing from that path, so the draw sequence
/// differs from native for those six weapons.
///
/// Ordinary `ROT < 1, Vertical = no` AI subtracts gravity every visit
/// (467402..467429), independently of `Arcing`. Production gives all such
/// persistent shots the ordinary gravity/collision arm. Scalar FireAt math
/// retains binary64 velocity; upstream FLH/pivot and homing producers remain
/// explicitly bounded in their owners.
fn classify_projectile_delivery(
    weapon: &crate::rules::weapon_type::WeaponType,
    rules: &RuleSet,
) -> ProjectileDelivery {
    let Some(projectile_id) = weapon.projectile.as_deref() else {
        return ProjectileDelivery::Immediate(ImmediateProjectileReason::NoProjectile);
    };
    let Some(projectile) = rules.projectile(projectile_id) else {
        return ProjectileDelivery::Immediate(ImmediateProjectileReason::MissingProjectileType);
    };
    if projectile.inviso {
        return ProjectileDelivery::Immediate(ImmediateProjectileReason::Invisible);
    }
    // `BulletClass::AI @ 0x004666E0` selects an arm exactly twice: `ROT < 1` at
    // `0x004668D1`, then `Vertical` (`+0x2C0`) at `0x004671D0`. Nothing else
    // participates.
    let ballistic = projectile.arcing;
    ProjectileDelivery::Persistent {
        arm_frames: projectile.arm,
        tracks_target: projectile.rot > 0,
        collision: ProjectileCollisionPolicy {
            level_non_water: projectile.level,
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
            missile_rot_var: rules.general.missile_rot_var,
            course_lock_duration: projectile
                .course_lock_duration
                .clamp(0, i32::from(u16::MAX)) as u16,
            // The RE contract proves this is BulletClass-identity-derived but
            // not its formula. Keep the raw phase as an explicit live seam.
            sidewinder_phase: 0,
            airburst: projectile.airburst,
            inaccurate: projectile.inaccurate,
            very_high: projectile.very_high,
            level: projectile.level,
            // Replaced at construction with the launch facing.
            heading_bam: 0,
            frames_elapsed: 0,
            max_speed: weapon.speed.clamp(0, i32::from(u16::MAX)) as u16,
            acceleration: projectile.acceleration,
            // Replaced at construction with the launch-time target coord.
            fuse_reference: ProjectileCoord::new(0, 0, 0),
            closing_frames: 0,
            closing_accumulator_bits: 0,
        }),
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

        for weapon_name in ["ProxGun", "CliffGun"] {
            let weapon = rules.weapon(weapon_name).expect("weapon");
            assert!(
                matches!(
                    classify_projectile_delivery(weapon, &rules),
                    ProjectileDelivery::Persistent { .. }
                ),
                "{weapon_name} must stay on the tracked path"
            );
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
            ProjectileDelivery::Persistent {
                arm_frames: 0,
                tracks_target: false,
                collision: ProjectileCollisionPolicy {
                    level_non_water: false,
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
        for weapon_name in ["W0", "W1", "W2", "W3"] {
            let weapon = rules.weapon(weapon_name).expect("weapon");
            assert!(
                matches!(
                    classify_projectile_delivery(weapon, &rules),
                    ProjectileDelivery::Persistent { .. }
                ),
                "{weapon_name} must stay on the tracked path"
            );
        }
        // The flak arm is only selected when `FlakScatter && !Inviso`.
        let flak = rules.weapon("W2").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(flak, &rules),
            ProjectileDelivery::Persistent {
                launch_scatter_is_flak: Some(true),
                ..
            }
        ));
        // `Bouncy` + `Arcing` is an ordinary ballistic shell, with no scatter.
        let bouncy = rules.weapon("W0").expect("weapon");
        assert!(matches!(
            classify_projectile_delivery(bouncy, &rules),
            ProjectileDelivery::Persistent {
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
            ProjectileDelivery::Persistent {
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

/// A cell area to reveal due to a RevealOnFire weapon firing.
pub struct RevealEvent {
    pub owner: InternedId,
    pub rx: u16,
    pub ry: u16,
    pub radius: u16,
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

/// Combat-only target category used for projectile AA/AG legality and weapon
/// selection.
///
/// `ConsideredAircraft=yes` infantry, such as Rocketeers/JumpJets, remain
/// infantry entities for movement, selection, crush, and animation, but weapon
/// selection must treat them as air targets.
pub(crate) fn combat_target_category(
    entity: &GameEntity,
    rules: &RuleSet,
    interner: &StringInterner,
) -> EntityCategory {
    if rules
        .object(interner.resolve(entity.type_ref()))
        .is_some_and(|obj| obj.considered_aircraft)
    {
        EntityCategory::Aircraft
    } else {
        entity.category
    }
}

/// Return the active wall-overlay flags at a cell, if available.
fn wall_overlay_flags_at<'a>(
    overlay_grid: Option<&OverlayGrid>,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    rx: u16,
    ry: u16,
) -> Option<&'a crate::map::overlay_types::OverlayTypeFlags> {
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
    wall_flags: &crate::map::overlay_types::OverlayTypeFlags,
) -> bool {
    warhead.wall || warhead.wall_absolute_destroyer || (warhead.wood && wall_flags.armor_is_wood)
}

fn infantry_prone_area_raw_damage(
    target: &GameEntity,
    warhead: &WarheadType,
    damage: i32,
    ignore_defenses: bool,
) -> i32 {
    if target.category != EntityCategory::Infantry
        || !infantry::is_prone_for_damage(target)
        || damage <= 0
        || ignore_defenses
    {
        return damage;
    }

    let Ok(multiplier) =
        X87Chop53::load_f64(NativeF64Bits::from_bits(warhead.prone_damage_f64.to_bits()))
    else {
        // FISTP's native indefinite result is negative and the wrapper's
        // immediate minimum-one clamp therefore converts it to one.
        return 1;
    };
    let product = X87Chop53::mul(X87Chop53::load_i32(damage), multiplier);
    let scaled = X87Chop53::ftol_i64(product)
        .ok()
        .and_then(|value| i32::try_from(value).ok())
        // A native out-of-range FISTP yields the signed indefinite value,
        // which the immediately following minimum-one clamp replaces with 1.
        .unwrap_or(i32::MIN);
    scaled.max(1)
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

/// Sentinel attacker id for sourceless damage (the radiation field). Stable
/// entity ids start at 1, so 0 is never a live attacker; retaliation treats
/// it as "attacker gone" and the last-attacker bookkeeping skips it.
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
/// dispatch; legacy direct callers retain their already-resolved amount.
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
    pub(crate) distance_leptons: Option<i32>,
    pub(crate) receiver_flags: Option<ReceiverCallFlags>,
    /// This record belongs to an Apply_area_damage transaction whose captured
    /// CellSpread is at most 0.5. The receiver commit uses this transient fact
    /// to reproduce the native near-center Iron Curtain isolation scan; it is
    /// deliberately false for direct-receiver and legacy precomputed calls.
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
            distance_leptons: Some(distance_leptons),
            receiver_flags: Some(ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            }),
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
            distance_leptons: Some(distance_leptons),
            receiver_flags: Some(receiver_flags),
            near_center_ic_isolation_eligible: false,
        }
    }

    /// `WaveClass::DamageArea` calls the concrete occupant receiver directly,
    /// at distance zero, while both the wave and firer are still represented.
    pub(crate) fn from_wave(event: WaveDamageEvent, entities: &EntityStore) -> Self {
        Self::direct_receiver(
            event.target_id,
            event.payload.base_damage,
            0,
            event.payload.firer_id,
            entities
                .get(event.payload.firer_id)
                .map(|firer| firer.owner()),
            event.payload.warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        )
    }
}

/// Infantry shot waiting for its current fire animation to reach the discharge frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PendingInfantryFire {
    /// Fire sequence started when the shot was accepted.
    pub sequence: SequenceKind,
    /// Animation frame index that spawns the projectile/damage.
    pub fire_frame: u16,
}

/// Component: this entity is attacking a specific target.
///
/// Attached by `issue_attack_command()` (entity targets) or
/// `issue_attack_cell_command()` (cell targets). The combat system fires the
/// attacker's weapon at the resolved target each tick. Supports burst firing:
/// multiple rapid shots per attack cycle, with ROF cooldown only after
/// the full burst completes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttackTarget {
    /// What this attacker is firing at: an entity or a ground cell (force-fire).
    pub target: TargetKind,
    /// Simulation ticks remaining before the next shot (ROF cooldown).
    pub cooldown_ticks: u16,
    /// Ticks between individual burst shots (short inter-shot delay).
    pub burst_delay_ticks: u8,
    /// Infantry-only delayed shot latch. `None` for vehicles/buildings/aircraft.
    #[serde(default)]
    pub pending_infantry_fire: Option<PendingInfantryFire>,
}

/// The inclusive bounds of the mid-burst delay draw.
///
/// gamemd-derived: `TechnoClass::GetROF @ 0x006FCFA0` — the mid-burst branch
/// (`burst index < Burst=`) returns `Random::RandomRanged(3, 5)`.
const BURST_INTER_SHOT_DELAY_MIN: i32 = 3;
const BURST_INTER_SHOT_DELAY_MAX: i32 = 5;

fn infantry_fire_sequence(
    obj: &ObjectType,
    weapon_slot: WeaponSlot,
    is_prone: bool,
    is_fully_deployed: bool,
) -> SequenceKind {
    if is_fully_deployed {
        return SequenceKind::DeployedFire;
    }
    match (weapon_slot, is_prone) {
        (WeaponSlot::Primary, true) => SequenceKind::FireProne,
        (WeaponSlot::Primary, false) => SequenceKind::Attack,
        (WeaponSlot::Secondary, true) if obj.secondary_prone_frame != obj.fire_up_frame => {
            SequenceKind::SecondaryProne
        }
        (WeaponSlot::Secondary, false) if obj.secondary_fire_frame != obj.fire_up_frame => {
            SequenceKind::SecondaryFire
        }
        _ => SequenceKind::Attack,
    }
}

fn infantry_fire_frame(
    obj: &ObjectType,
    weapon_slot: WeaponSlot,
    is_prone: bool,
    is_fully_deployed: bool,
) -> u16 {
    let frame = match (weapon_slot, is_prone || is_fully_deployed) {
        (WeaponSlot::Primary, false) => obj.fire_up_frame,
        (WeaponSlot::Primary, true) => obj.fire_prone_frame,
        (WeaponSlot::Secondary, false) => obj.secondary_fire_frame,
        (WeaponSlot::Secondary, true) => obj.secondary_prone_frame,
    };
    frame as u16
}

fn infantry_idle_sequence(is_prone: bool, is_fully_deployed: bool) -> SequenceKind {
    if is_fully_deployed {
        SequenceKind::Deployed
    } else if is_prone {
        SequenceKind::Prone
    } else {
        SequenceKind::Stand
    }
}

impl AttackTarget {
    /// Entity-targeted attack: fire at a specific entity by stable ID.
    pub fn new(target_stable_id: u64) -> Self {
        Self {
            target: TargetKind::Entity(target_stable_id),
            cooldown_ticks: 0,
            burst_delay_ticks: 0,
            pending_infantry_fire: None,
        }
    }

    /// Ground-targeted attack: fire at a specific cell coord (force-fire on terrain).
    pub fn for_cell(rx: u16, ry: u16) -> Self {
        Self {
            target: TargetKind::Cell(rx, ry),
            cooldown_ticks: 0,
            burst_delay_ticks: 0,
            pending_infantry_fire: None,
        }
    }
}

/// Compute the effective target coordinates for an entity.
///
/// For structures, returns the **foundation center** instead of the raw
/// position (NW corner cell center):
///   X = Location.X + (foundationWidth  - 1) * 128
///   Y = Location.Y + (foundationHeight - 1) * 128
///
/// Native virtual GetCoords: Object5F65A0 and Building447AC0. Callers which
/// consume stored Location rather than this virtual point keep that distinction.
fn target_coords(
    entity: &GameEntity,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> (u16, u16, SimFixed, SimFixed) {
    let mut rx = entity.position.rx;
    let mut ry = entity.position.ry;
    let mut sub_x = entity.position.sub_x;
    let mut sub_y = entity.position.sub_y;

    if entity.category == EntityCategory::Structure {
        if let Some(obj) = rules.and_then(|r| r.object(interner.resolve(entity.type_ref()))) {
            let (fw, fh) = foundation_dimensions(&obj.foundation);
            // Shift from NW corner cell center to foundation geometric center.
            // (fw-1)*128 leptons in X, (fh-1)*128 leptons in Y.
            // sub_x/sub_y may exceed 256 — lepton_distance_sq_raw handles
            // this correctly since it computes cell*256+sub as a flat value.
            //447AC0 subtracts1 as a signed integer, including the native0x0
            //foundation: its GetCoords lies128 leptons before the raw anchor.
            let offset_x = (i32::from(fw) - 1) * 128;
            let offset_y = (i32::from(fh) - 1) * 128;
            let full_x: i32 = rx as i32 * 256 + sub_x.to_num::<i32>() + offset_x;
            let full_y: i32 = ry as i32 * 256 + sub_y.to_num::<i32>() + offset_y;
            rx = (full_x / 256) as u16;
            ry = (full_y / 256) as u16;
            sub_x = SimFixed::from_num(full_x % 256);
            sub_y = SimFixed::from_num(full_y % 256);
        }
    }

    (rx, ry, sub_x, sub_y)
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
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> Option<(u16, u16, SimFixed, SimFixed)> {
    match *target {
        TargetKind::Entity(id) => entities.get(id).map(|t| target_coords(t, rules, interner)),
        TargetKind::Cell(rx, ry) => Some(cell_center_coords(rx, ry)),
    }
}

/// ObjectClass::Distance_To5F6440: planar GetCoords distance, then the target
/// building's (foundation width + height)*64 discount, clamped to zero. This
/// differs from the weapon CanFireAt/InRange gate and has no altitude bonus.
/// Reuse the coordinate projection and deterministic native sqrt owner: exact
/// integer sqrt changes observable lepton ties (1281 becomes1280 natively).
/// Native comparisons: tools/spatial_oracle/aircraft_approach_range.{py,json}.
pub(crate) fn object_distance_to(
    source: &GameEntity,
    target: &TargetKind,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> Option<i32> {
    let planar = |(rx, ry, sx, sy): (u16, u16, SimFixed, SimFixed)| {
        [
            i32::from(rx) * 256 + sx.to_num::<i32>(),
            i32::from(ry) * 256 + sy.to_num::<i32>(),
            0,
        ]
    };
    let from = planar(target_coords(source, Some(rules), interner));
    let to = planar(resolve_target_coords(
        target,
        entities,
        Some(rules),
        interner,
    )?);
    let distance = crate::util::native_x87::distance_3d_leptons(from, to);
    if let TargetKind::Entity(id) = *target
        && let Some(building) = entities.get(id)
        && building.category == EntityCategory::Structure
    {
        let object = rules.object(interner.resolve(building.type_ref()))?;
        let (width, height) = foundation_dimensions(&object.foundation);
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
/// `0x006B7230` calls the Unit owner's `TechnoClass::CanFireAtTarget` vslot,
/// which dispatches through weapon selection @ `0x006F7780`, `CanFireAt` @
/// `0x006F77B0`, and ordinary `TechnoClass::InRange` @ `0x006F7220`.
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
    let Some(selected) = select_weapon_against(
        rules,
        attacker_obj,
        &combat_weapon::attacker_facts(attacker, attacker_obj),
        attacker.owner(),
        target,
        entities,
        interner,
        Some(terrain),
        alliances,
    ) else {
        return false;
    };
    let Some(source) =
        in_range::fire_source_coords(attacker, target, selected.weapon, entities, terrain)
    else {
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
/// Returns `None` if the selected weapon cannot legally fire at the target
/// (the selection's GetFireError subset). Pursuit treats `None` as "skip":
/// the fire routine asks GetFireError itself, and `TechnoClass::AI`'s
/// 16-frame check drops an ILLEGAL or CANT target (a building drops it at
/// once).
pub(crate) fn pursuit_selected_weapon<'a>(
    entity: &GameEntity,
    target: &TargetKind,
    entities: &EntityStore,
    rules: &'a RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<&'a WeaponType> {
    let attacker_obj = rules.object(interner.resolve(entity.type_ref()))?;
    let selected = select_weapon_against(
        rules,
        attacker_obj,
        &combat_weapon::attacker_facts(entity, attacker_obj),
        entity.owner(),
        target,
        entities,
        interner,
        terrain,
        alliances,
    )?;
    Some(selected.weapon)
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
        let Some((trx, try_, tsx, tsy)) =
            resolve_target_coords(target, entities, Some(rules), interner)
        else {
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

    let Some(src) = in_range::fire_source_coords(entity, target, weapon, entities, terrain) else {
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

/// Issue an attack command: make `attacker` fire at `target`.
///
/// Replaces any existing AttackTarget. Infantry and vehicles turn through their
/// firing/movement owners, not through target assignment.
pub fn issue_attack_command(
    entities: &mut EntityStore,
    attacker_id: u64,
    target_id: u64,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> bool {
    // Read target position first (immutable borrow, lepton-precise).
    // Use foundation center for buildings (see target_coords doc comment).
    let target_pos = entities
        .get(target_id)
        .map(|t| target_coords(t, rules, interner));
    let (trx, try_, _tsx, _tsy) = match target_pos {
        Some(p) => p,
        None => return false,
    };

    // Read attacker position before mutable borrow (needed for body-facing delta).
    let attacker_pos = entities.get(attacker_id).map(|a| {
        (
            a.position.rx,
            a.position.ry,
            a.barrel_facing.is_some(),
            a.category,
        )
    });
    let (arx, ary, has_turret, category) = match attacker_pos {
        Some(p) => p,
        None => return false,
    };

    // Mutate attacker.
    let attacker = match entities.get_mut(attacker_id) {
        Some(a) => a,
        None => return false,
    };

    // gamemd-derived: a target assignment writes the target pointer and nothing
    // else — no facing. A TURRETLESS VEHICLE therefore gets no instant snap
    // here: `UnitClass::Fire_At_Target @ 0x00736DF0` case 2 turns its hull at
    // `ROT=` (`FacingClass::Set(+0x388)` at `0x00737004`) only once the fire
    // gate refuses the shot for facing, and only while it is stationary.
    //
    // Infantry likewise snap only when their fire action starts (00520925),
    // now owned by world_receiver::resolve_attacker_fire. The legacy body-only
    // aircraft/structure order behavior remains for its class-specific audit.
    if !has_turret && !matches!(category, EntityCategory::Unit | EntityCategory::Infantry) {
        let dx: i32 = trx as i32 - arx as i32;
        let dy: i32 = try_ as i32 - ary as i32;
        attacker.facing = crate::sim::movement::facing_from_delta(dx, dy);
    }

    // Walk's physical head survives a null destination. The synchronized
    // command owner applies that setter after TarCom assignment; the shared
    // target helper must not destroy the adapter needed to finish the head.
    if !attacker
        .locomotor
        .as_ref()
        .is_some_and(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
    {
        attacker.movement_target = None;
    }

    // Attach the attack target using stable ID (fire immediately).
    attacker.attack_target = Some(AttackTarget::new(target_id));
    // An ordered target was not picked up by the passive scanner, so it is not
    // subject to the scanner's stale-target drop or the off-mission clear.
    attacker.passively_acquired_target = false;

    true
}

/// Swing an existing attack onto a different entity WITHOUT restarting the
/// weapon.
///
/// The rearm countdown and inter-shot delay still live on
/// [`AttackTarget`] here, so replacing the whole record — which is what building
/// a fresh `AttackTarget` does — zeroes them and hands the attacker a free shot
/// on the spot. The original keeps its rearm timer on the OBJECT and its target
/// assignment writes nothing but the target pointer and two adjacent fields, so
/// swinging onto a new victim never shortens the reload. Mutating in place is
/// how that contract is honoured here.
///
/// This is the one owner of that operation: combat's own auto-retarget and the
/// passive scanner's re-pick both go through it. Only the pending infantry shot
/// is dropped, because it was latched against the old victim.
///
/// RESIDUAL: this does NOT perform the infantry firing-sequence and animation
/// reset that the full target setter does. That was unreachable in practice
/// before the passive scanner existed; it is now reachable on every infantry
/// re-pick, roughly every 28 frames. Deterministic and visual only — the fire
/// decision does not read the sequence — so it is recorded rather than fixed
/// here.
///
/// **Target provenance is preserved on purpose.** Swinging onto a new victim
/// continues whatever acquisition installed the target in the first place — an
/// auto-retarget after the old victim died is not a new order — so
/// `passively_acquired_target` carries over. Clearing it here would leave the
/// object holding a live target with the flag false, and that state is exactly
/// what the passive block, the pursuit skip and the release-on-range-loss path
/// all key off: the object would stop re-evaluating, start being chased across
/// the map, and never let go of a target that walked out of range. The
/// flag-clearing that the original's target ASSIGNMENT performs lives in the
/// target setter, which is the assignment's counterpart; this in-place swing has
/// no counterpart there.
pub(crate) fn retarget_preserving_rearm(entity: &mut GameEntity, new_target_sid: u64) {
    if let Some(ref mut attack) = entity.attack_target {
        attack.target = TargetKind::Entity(new_target_sid);
        attack.pending_infantry_fire = None;
    }
}

/// Issue a force-fire-on-cell command: make `attacker` fire at a ground cell.
///
/// Used by `Command::ForceAttackCell` (Ctrl + left-click on empty terrain).
/// Aborts (returns `false`) if the attacker has no weapon — caller filters
/// unarmed units client-side, but this defensive check keeps a stray command
/// from corrupting state.
///
/// gamemd-derived: `TechnoClass::What_Action_OnCell @ 0x00700600` inlines
/// `TechnoClass::Is_Armed` at `0x007008BD..0x007008CE` —
/// `CALL [EDX+0x3F4]` (`GetCurrentWeapon`), `TEST EAX,EAX`,
/// `CMP dword [EAX],0x0`, both misses jumping to `0x00700AB7` past every arm
/// that can return 5 (ACTION_ATTACK). That is the gate, and it is a single
/// weapon slot — not `Primary=` plus `Secondary=`. Reading the INI keys
/// refused force-fire for `[SREF]` and `[YAGGUN]`, whose weapons live only in
/// `Weapon1..N`.
pub fn issue_attack_cell_command(
    entities: &mut EntityStore,
    attacker_id: u64,
    target_rx: u16,
    target_ry: u16,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> bool {
    // Read attacker position + weapon presence before mutable borrow.
    let attacker_info = entities.get(attacker_id).map(|a| {
        let type_str = interner.resolve(a.type_ref());
        let has_weapon = rules
            .and_then(|r| r.object(type_str))
            .is_some_and(|obj| combat_weapon::is_armed(a, obj));
        (
            a.position.rx,
            a.position.ry,
            a.barrel_facing.is_some(),
            has_weapon,
            a.category,
        )
    });
    let (arx, ary, has_turret, has_weapon, category) = match attacker_info {
        Some(info) => info,
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

    let (trx, try_, _tsx, _tsy) = cell_center_coords(target_rx, target_ry);

    let attacker = match entities.get_mut(attacker_id) {
        Some(a) => a,
        None => return false,
    };

    // As with entity targets, Infantry/Unit facing belongs to Fire_At_Target.
    if !has_turret && !matches!(category, EntityCategory::Unit | EntityCategory::Infantry) {
        let dx: i32 = trx as i32 - arx as i32;
        let dy: i32 = try_ as i32 - ary as i32;
        attacker.facing = crate::sim::movement::facing_from_delta(dx, dy);
    }

    if !attacker
        .locomotor
        .as_ref()
        .is_some_and(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
    {
        attacker.movement_target = None;
    }
    attacker.attack_target = Some(AttackTarget::for_cell(target_rx, target_ry));
    attacker.passively_acquired_target = false;
    true
}

/// Compute distance in cells between two entities' grid positions.
#[cfg(test)]
pub(crate) fn cell_distance(ax: u16, ay: u16, bx: u16, by: u16) -> f32 {
    let dx: f32 = ax as f32 - bx as f32;
    let dy: f32 = ay as f32 - by as f32;
    (dx * dx + dy * dy).sqrt()
}

use self::combat_targeting::{AttackerSnapshot, GarrisonSnapshot, acquire_best_target};

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
    /// A death producer's own constructor call (`Death_Explosion`, the
    /// Aircraft death arm, `DestructionEffects`): `AnimClass(type, coord,
    /// delay, 1, 0x600, 0, 0)` at an exact coordinate. `None` rows construct
    /// with the warhead impact's `(0, 1, 0x2600, -15)` at a level-rounded
    /// coordinate: the impact anim, the InfDeath anims and the TechnoClass
    /// debris anims. Natively the debris anims take `(center + 0x14 Z, 0, 1,
    /// 0x600, 0, 0)` (`0x007024AA`, `0x00702566`); on stock their rows are
    /// dropped as unbound art (GSI-05.14).
    pub death: Option<destruction_effects::DeathAnimSpawn>,
}

/// One transient combat-light request emitted when active IronCurtain or
/// ForceShield rejects a positive receiver call. `FUN_0048A620` creates an
/// unowned screen-space light, not an AnimClass/ParticleSystem, so this record
/// retains the exact call inputs without inventing an attachment or house.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvulnerabilityImpactEffect {
    /// Receiver provenance only; this is not native effect ownership.
    pub target_id: u64,
    /// Post-defender-transform damage shifted left once before the helper call.
    pub doubled_damage: i32,
    pub warhead_ref: InternedId,
    pub coord: ProjectileCoord,
    /// Native helper force/create argument (literal true on this callsite).
    pub force_create: bool,
    /// Native raw draw flags: IC=1, ForceShield=6.
    pub flags: u32,
}

/// One smudge producer payload. Production commits it synchronously through
/// the world receiver; phase-level combat fixtures retain it in their result as
/// a test adapter.
#[derive(Debug, Clone)]
pub enum SmudgeSpawnRequest {
    /// Emitted alongside ExplosionEffect when a warhead's AnimList anim spawns.
    /// Carries the anim's interned SHP name for AnimType flag lookup.
    Anim {
        anim_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        /// Exact absolute CoordStruct Z. ExplosionEffect keeps its separate
        /// coarse presentation byte; the native smudge altitude gate is in
        /// leptons and must never reconstruct this value from that byte.
        world_z_leptons: i32,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_infantry_death_anim(
    general: &crate::rules::ruleset::GeneralRules,
    inf_death: u8,
    rx: u16,
    ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
    z: u8,
    world_z_leptons: i32,
    interner: &mut StringInterner,
    explosion_effects: &mut Vec<ExplosionEffect>,
    smudge_spawn_requests: &mut Vec<SmudgeSpawnRequest>,
) {
    let Some(anim_name) = general.infantry_death_anim(inf_death) else {
        return;
    };
    let anim_name = interner.intern(anim_name);
    explosion_effects.push(ExplosionEffect {
        shp_name: anim_name,
        rx,
        ry,
        sub_x,
        sub_y,
        z,
        death: None,
    });
    smudge_spawn_requests.push(SmudgeSpawnRequest::Anim {
        anim_name,
        rx,
        ry,
        sub_x,
        sub_y,
        world_z_leptons,
    });
}

/// Emit the warhead's AnimList animation and a paired smudge spawn request
/// for one detonation at (rx, ry, z). Mirrors gamemd's WarheadType::Detonate
/// dispatch into AnimClass::Start: every detonation that spawns an anim
/// also runs the anim's first-frame smudge logic.
///
/// Pushes nothing if `warhead.anim_list` is empty.
///
/// `base_damage` is the post-modifier damage at the impact center; it
/// drives AnimList selection via `damage / 25`, clamped to `len - 1`.
/// The dying object's OWN explosion is emitted separately, in the death loop —
/// see `UnitClass::Death_Explosion @ 0x00738680` there. This function is only
/// the warhead's half.
pub(crate) fn emit_warhead_detonation_effects(
    warhead: &WarheadType,
    base_damage: i32,
    rx: u16,
    ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
    z: u8,
    world_z_leptons: i32,
    interner: &mut StringInterner,
    explosion_effects: &mut Vec<ExplosionEffect>,
    smudge_spawn_requests: &mut Vec<SmudgeSpawnRequest>,
) {
    if warhead.anim_list.is_empty() {
        return;
    }
    let idx = (base_damage / ANIM_LIST_DAMAGE_STEP as i32).max(0) as usize;
    let idx = idx.min(warhead.anim_list.len() - 1);
    let interned_name = interner.intern(&warhead.anim_list[idx]);
    explosion_effects.push(ExplosionEffect {
        shp_name: interned_name,
        rx,
        ry,
        sub_x,
        sub_y,
        z,
        death: None,
    });
    smudge_spawn_requests.push(SmudgeSpawnRequest::Anim {
        anim_name: interned_name,
        rx,
        ry,
        sub_x,
        sub_y,
        world_z_leptons,
    });
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

/// Ordinary fire prelude plus one consuming deferred-consequence packet.
/// The frame admits bullets and applies facing before committing the packet at
/// its existing post-SpawnManager boundary.
pub struct CombatTickResult {
    /// Bullets admitted after the current BulletClass pass; no recursive advance.
    pub projectile_spawns: Vec<ProjectileSpawn>,
    /// Phase-2 entry facing slots, amended by explicit retarget/removal and
    /// Fire_At_Target hull turns, applied by unit_post before SpawnManager.
    pub unit_facing: Vec<UnitFacingUpdate>,
    pub(crate) consequences: crate::sim::world::damage_consequences::DamageConsequences,
}

/// A "your asset is being shot" ping: a Structure or harvester took damage
/// this tick.
///
/// Native has two producers and neither tests the attacker's house:
/// `BuildingClass::ReceiveDamage @ 0x00442230` calls
/// `HouseClass::NotifyUnderAttack` when the source is non-null, the damage
/// result is non-zero and `BuildingType+0x232 Insignificant` is clear (after
/// a victim `vtbl+0x80` pre-check whose identity is not pinned and is not
/// modelled); own-fire on an own building announces. `UnitClass::ReceiveDamage 0x007384B9..0x00738530` pings a
/// `Harvester=` unit on any non-zero, non-fatal result with or without a
/// source.
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

/// Exact ObjectClass-style world Z for effect and projectile coordinates.
///
/// This is deliberately distinct from [`in_range::effective_z_leptons`]: the
/// range helper applies low-flight targeting rules, while native animation and
/// bullet coordinates retain the object's actual airborne height. An explicit
/// exact coordinate is already absolute. Otherwise the base is exact sloped
/// terrain plus the object-owned bridge deck, followed by the one active
/// object/locomotor altitude source in presentation precedence order.
pub(crate) fn object_world_z_leptons(
    entity: &GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    if let Some(exact_z_leptons) = entity.position.exact_z_leptons {
        return exact_z_leptons;
    }

    let world_x = i32::from(entity.position.rx)
        .wrapping_mul(256)
        .wrapping_add(entity.position.sub_x.to_num::<i32>());
    let world_y = i32::from(entity.position.ry)
        .wrapping_mul(256)
        .wrapping_add(entity.position.sub_y.to_num::<i32>());
    let base_z = terrain
        .and_then(|terrain| terrain.cell(entity.position.rx, entity.position.ry))
        .and_then(|cell| ground_height_leptons(cell.level, cell.slope_type, world_x, world_y).ok())
        .map(|ground_z| {
            ground_z.wrapping_add(if entity.on_bridge {
                BRIDGE_DECK_HEIGHT_LEPTONS
            } else {
                0
            })
        })
        // Position.z is already the effective layer level (including a bridge
        // deck), so the mapless fallback must not add OnBridge a second time.
        .unwrap_or_else(|| i32::from(entity.position.z).wrapping_mul(LEPTONS_PER_LEVEL as i32));

    let altitude = entity
        .parachute_state
        .as_ref()
        .map(|state| state.altitude.to_num::<i32>())
        .or_else(|| {
            entity
                .rocket_state
                .as_ref()
                .map(|state| state.altitude.to_num::<i32>())
        })
        .or_else(|| {
            entity
                .drop_pod_state
                .as_ref()
                .filter(|state| {
                    state.phase == crate::sim::movement::drop_pod_movement::DropPodPhase::Descending
                })
                .map(|state| state.altitude.to_num::<i32>())
        })
        .or_else(|| {
            entity
                .locomotor
                .as_ref()
                .filter(|locomotor| {
                    locomotor.layer == crate::sim::movement::locomotor::MovementLayer::Air
                        && locomotor.kind != crate::rules::locomotor_type::LocomotorKind::Rocket
                })
                .map(|locomotor| locomotor.altitude.to_num::<i32>())
        })
        .unwrap_or(0);

    base_z.wrapping_add(altitude)
}

/// The **one** impact height for an attack, in tile-step level units (signed).
///
/// The original engine forms a single impact coordinate per detonation and
/// hands that same coordinate to area damage and to the animation placement —
/// there is no second Z anywhere on the path. This function is VERA's
/// equivalent single value, and every consumer reads it rather than deriving
/// its own: the AoE object-layer selector, the bridge-damage Z gate, the
/// persistent-projectile impact coordinate, the impact-animation height, and
/// (through `app::presentation::fire_effects`) the pixel the tracer ends on. A second
/// derivation could only agree with this one by coincidence.
///
/// Three native quantities sit close together here and are not the same
/// thing:
/// * a cell's **own** coordinate — cell centre on both axes, terrain floor
///   height for Z;
/// * the **aim point** for a cell target — that, plus a four-level structural
///   bridge deck offset when a span crosses the cell;
/// * the **impact** coordinate — the projectile's own location, whose Z the
///   flight step clamps to the plain cell ground-height lookup at the moment
///   of ground contact. The resolution ladder that can substitute a target's
///   bridge-aware aim point runs only when there is a live *object* target;
///   for a shot at bare ground it is skipped entirely.
///
/// VERA models the impact, so a ground cell contributes its terrain floor
/// level and nothing else. There is no branch yielding zero: zero is what a
/// level-0 cell is worth, never a stand-in for a height we failed to look up.
/// VERA carries this quantity in whole tile-step levels along the entire
/// impact path; native carries the same step count scaled into leptons.
///
/// **Residual DRIFT — the structural-bridge deck term is unmodelled here.**
/// A force-fire at a bridge cell therefore damages the ground occupant list
/// and draws its explosion at ground height rather than four levels up on the
/// deck. It is not a one-line addition, because two VERA consumers want
/// opposite values: `combat_aoe::select_object_damage_layer` picks the bridge
/// occupant list only for an impact well above the cell's ground level (it
/// wants the deck term), while `bridge_state`'s path Z gate accepts only an
/// impact within one level of the cell's *ground* level (it rejects the deck
/// term outright). With ground-only Z that gate now admits every cell target:
/// it is **disabled, not widened** — a gate that can no longer reject
/// anything is not a modelled gate. That is RNG-visible: a path that newly
/// matches consumes a bridge-strength draw from the scenario stream, so a
/// replay containing a `Wall=yes` force-fire at a bridge over ground level ≥ 2
/// diverges from one recorded before this change. Trigger frequency: needs
/// deliberate bridge-cutting over raised ground, uncommon per match but a real
/// tactic on bridge maps.
/// *Settling step:* walk the bridge block of the native area-damage routine
/// and establish which reference **its** Z comparisons use — cell ground
/// height or deck plane — before either the gate or the deck term moves. The
/// VERA gate's native equivalent is UNCHECKED and is not authority for
/// dropping a verified native term.
///
/// `terrain` is `None` only where combat runs without a loaded map (headless
/// fixtures). With no map there is no cell to read — a VERA API boundary, not
/// a game rule.
pub(crate) fn attack_impact_z(
    target: TargetKind,
    entities: &EntityStore,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    match target {
        TargetKind::Entity(eid) => entities
            .get(eid)
            .map(|entity| i32::from(entity.position.z))
            .unwrap_or(0),
        TargetKind::Cell(rx, ry) => terrain
            .and_then(|grid| grid.cell(rx, ry))
            .map(|cell| i32::from(cell.level))
            .unwrap_or(0),
    }
}

fn attack_air_impact(
    target: TargetKind,
    impact_rx: u16,
    impact_ry: u16,
    impact_sub_x: SimFixed,
    impact_sub_y: SimFixed,
    entities: &EntityStore,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<combat_aoe::AoEAirImpact> {
    match target {
        TargetKind::Entity(entity_id) => {
            combat_aoe::air_impact_from_entity(entities.get(entity_id)?, terrain)
        }
        TargetKind::Cell(_, _) => combat_aoe::air_impact_from_layer_z(
            terrain,
            impact_rx,
            impact_ry,
            impact_sub_x,
            impact_sub_y,
            attack_impact_z(target, entities, terrain),
        ),
    }
}

fn attack_world_z_leptons(
    target: TargetKind,
    impact_rx: u16,
    impact_ry: u16,
    impact_sub_x: SimFixed,
    impact_sub_y: SimFixed,
    entities: &EntityStore,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    match target {
        TargetKind::Entity(entity_id) => entities
            .get(entity_id)
            .map(|entity| object_world_z_leptons(entity, terrain))
            .unwrap_or_else(|| {
                attack_impact_z(target, entities, terrain).wrapping_mul(LEPTONS_PER_LEVEL as i32)
            }),
        TargetKind::Cell(_, _) => combat_aoe::air_impact_from_layer_z(
            terrain,
            impact_rx,
            impact_ry,
            impact_sub_x,
            impact_sub_y,
            attack_impact_z(target, entities, terrain),
        )
        .map(|impact| impact.z_leptons)
        .unwrap_or_else(|| {
            attack_impact_z(target, entities, terrain).wrapping_mul(LEPTONS_PER_LEVEL as i32)
        }),
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

/// Resolve the active fatal-receiver death producer. The reachability gate is
/// independent of `DeathWeapon=`: native consults effective Explodes abilities
/// and the receiver's live `CurrentWeaponNumber` Suicide flag first. Once
/// admitted, selection is explicit type weapon, current weapon, then the Rules
/// default, with distinct native damage formulas for the first two vs default.
fn death_weapon_aoe(
    rules: &RuleSet,
    obj: &ObjectType,
    veterancy: u16,
    current_weapon_index: u8,
    current_weapon_ref: Option<InternedId>,
    interner: &mut StringInterner,
) -> Option<(i32, InternedId, InternedId)> {
    let selected_current_weapon =
        current_weapon_ref.and_then(|weapon_id| rules.weapon(interner.resolve(weapon_id)));
    let slot_current_weapon =
        combat_weapon::weapon_for_slot_index(obj, veterancy, i32::from(current_weapon_index))
            .and_then(|(weapon_id, _)| rules.weapon(weapon_id));
    let current_weapon = selected_current_weapon.or(slot_current_weapon);
    let effective_explodes = obj.explodes
        || (veterancy >= 100 && obj.veteran_explodes)
        || (veterancy >= 200 && obj.elite_explodes);
    if !effective_explodes && !current_weapon.is_some_and(|weapon| weapon.suicide) {
        return None;
    }

    if let Some(explicit) = obj
        .death_weapon
        .as_deref()
        .and_then(|weapon_id| rules.weapon(weapon_id))
    {
        let damage =
            death_weapon_ftol_product_i32_f32(explicit.damage, obj.death_weapon_damage_modifier)?;
        let warhead_ref = interner.intern(explicit.warhead.as_ref()?);
        let weapon_ref = interner.intern(&explicit.id);
        return Some((damage, warhead_ref, weapon_ref));
    }
    if let Some(current) = current_weapon {
        let damage =
            death_weapon_ftol_product_i32_f32(current.damage, obj.death_weapon_damage_modifier)?;
        let warhead_ref = interner.intern(current.warhead.as_ref()?);
        let weapon_ref = interner.intern(&current.id);
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
    pub(crate) invulnerability_impact_effects: Vec<InvulnerabilityImpactEffect>,
    pub(crate) bridge_damage_events: Vec<BridgeDamageEvent>,
    #[cfg(test)]
    pub(crate) wall_mutations: Vec<WallMutation>,
    #[cfg(test)]
    pub(crate) cell_target_detaches: Vec<combat_aoe::CellTargetDetach>,
    pub(crate) tiberium_reduction_requests: Vec<TiberiumReductionRequest>,
    pub(crate) death_sounds: Vec<(InternedId, u16, u16)>,
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
        self.invulnerability_impact_effects
            .append(&mut other.invulnerability_impact_effects);
        self.bridge_damage_events
            .append(&mut other.bridge_damage_events);
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
/// RESIDUAL (GSI-05.14) — the SHP half pushes `ExplosionEffect` rows, and with
/// stock data nothing comes of them: `spawn_combat_explosion_anim` constructs
/// only art types the loader bound, `anim_class_roots` lists neither
/// `DebrisAnims=` nor `MetallicDebris=`, and no stock warhead, `Explosion=` or
/// `DestroyAnim=` names a debris type, so every row is dropped. The draws are
/// taken; no chunk is drawn. Binding them without the bouncer arm would be
/// worse, because `LoopCount=-1` chunks would play in place forever. Native
/// builds a bouncing `AnimClass` (`0x00421EA0`): every stock
/// debris AnimType is `Bouncer=yes` — all 26 named by `[General]
/// MetallicDebris=` or by any `DebrisAnims=` line carry it, authored in
/// `artmd.ini` rather than `rulesmd.ini` (`AnimTypeClass+0x35A`, read at
/// `0x004286A7`) — so native's constructor enters its own `BounceClass::Init`
/// arm and the chunk flies an arc before landing.
/// - Trigger: every death that reaches either SHP arm — in gamemd, 324 of the
///   356 stock sections that throw (of 439 authoring `MaxDebris=` in gamemd's
///   own spelling, 83 author 0).
/// - Player effect: no debris chunk appears at all, and its
///   `Damage=`/`Warhead=` on landing is not applied.
/// - Frequency: continuous — every building death (no `[BuildingTypes]` section
///   authors `DebrisTypes=`, so all 292 that throw land here) plus 18 of the 50
///   registered `[VehicleTypes]` that throw and 11 of the 12 `[AircraftTypes]`.
///   The other 32 vehicle types take the voxel arm instead. Those counts are on
///   gamemd's case-exact key read, which `ObjectType::from_ini_section` matches
///   (`CCINIClass::ReadInt @ 0x005276D0` CRCs the raw key bytes): the 17
///   `[VehicleTypes]` spelling `Maxdebris=` take the constructor default 0 here
///   as they do in retail and never reach this arm.
/// - Downstream risk: the constructor's own draws are not consumed either —
///   one `RandomRanged` for `RandomRate=`, three `Random__Next()` for the
///   launch velocity and three `RandomRanged(-0xFFFF, 0xFFFF)` inside
///   `BounceClass::Init`, so seven per anim. Every debris producer in the
///   engine shares that gap today; closing it belongs with the AnimClass
///   bouncer owner, not here, because the same seven draws are missing from
///   the `Explosion=`/`DestroyAnim=` producer beside this one.
#[allow(clippy::too_many_arguments)]
fn throw_debris_for_death(
    object_type: &ObjectType,
    rules: &RuleSet,
    interner: &mut StringInterner,
    owner: InternedId,
    rx: u16,
    ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
    z: u8,
    world_z_leptons: i32,
    scenario_rng: &mut SimRng,
    voxel_debris: &mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    explosion_effects: &mut Vec<ExplosionEffect>,
) {
    use crate::sim::voxel_anim::{DebrisTypeData, ShpDebrisSource, throw_death_debris};

    if object_type.max_debris <= 0 {
        return;
    }
    let world_x = i32::from(rx)
        .wrapping_mul(256)
        .wrapping_add(sub_x.to_num::<i32>());
    let world_y = i32::from(ry)
        .wrapping_mul(256)
        .wrapping_add(sub_y.to_num::<i32>());

    let debris_types: Vec<Option<(crate::rules::voxel_anim_type::VoxelAnimTypeId, _)>> =
        object_type
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
    let Ok(thrown) = throw_death_debris(
        &data,
        Some(owner),
        glam::IVec3::new(world_x, world_y, world_z_leptons),
        scenario_rng,
    ) else {
        // A launch velocity outside the verified x87 domain needs a modded
        // `[VoxelAnims]` value far past any stock one; the draws are already
        // consumed, so the death simply throws nothing.
        return;
    };
    voxel_debris.extend(thrown.voxels);
    for row in thrown.anims {
        let name = match row.source {
            ShpDebrisSource::TypeDebrisAnims => object_type.debris_anims.get(row.index),
            ShpDebrisSource::RulesMetallicDebris => rules.general.metallic_debris.get(row.index),
        };
        // Native lifts the anim coordinate by 20 leptons (`ADD EAX, 0x14` at
        // `0x00702443`). `ExplosionEffect` carries Z as a height LEVEL, and 20
        // leptons is under a sixth of one, so the lift is below this row's
        // resolution and is not represented.
        if let Some(name) = name {
            let shp_name = interner.intern(name);
            explosion_effects.push(ExplosionEffect {
                shp_name,
                rx,
                ry,
                sub_x,
                sub_y,
                z,
                death: None,
            });
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
    interner: &mut StringInterner,
    rx: u16,
    ry: u16,
    death_sounds: &mut Vec<(InternedId, u16, u16)>,
) {
    let mut append_choice = |choices: &[String]| {
        if choices.is_empty() {
            return;
        }
        let index = (main_rng.next_u32() % choices.len() as u32) as usize;
        death_sounds.push((interner.intern(&choices[index]), rx, ry));
    };

    if owner_is_human {
        append_choice(&object_type.voice_die);
    }
    append_choice(&object_type.die_sounds);

    if category == EntityCategory::Structure && object_type.die_sounds.is_empty() {
        if let Some(sound_id) = building_die_sound.filter(|id| !id.is_empty()) {
            death_sounds.push((interner.intern(sound_id), rx, ry));
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
    invulnerability_impact: Option<InvulnerabilityImpactEffect>,
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
    let distance_leptons = event.distance_leptons?;
    let receiver_flags = event.receiver_flags?;
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
    let receiver_input = infantry_prone_area_raw_damage(
        target,
        warhead,
        event.damage,
        receiver_flags.ignore_defenses,
    );

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
    let defender_country_armor = houses.get(&target.owner()).map_or(1.0, |house| {
        let difficulty_armor = rules.general.difficulty_armor[house.difficulty.table_index()];
        let country_name = house
            .country
            .map(|country| interner.resolve(country))
            .unwrap_or_else(|| interner.resolve(target.owner()));
        let (country_armor, category_armor) = target_type
            .map(|object| rules.country_armor_factors(country_name, object))
            .unwrap_or((1.0, 1.0));
        let house_armor = difficulty_armor * country_armor;
        house_armor * category_armor
    });
    let defender_vet_armor = target_type
        .is_some_and(|object| {
            if target.veterancy >= ELITE_VETERANCY {
                object.veteran_stronger || object.elite_stronger
            } else {
                target.veterancy >= VETERAN_VETERANCY && object.veteran_stronger
            }
        })
        .then_some(rules.general.veteran_armor)
        .unwrap_or(1.0);
    let combat_mods = damage::CombatMods {
        defender_country_armor,
        defender_unit_armor: f64::from_bits(target.armor_multiplier.bits()),
        defender_vet_armor,
        ..damage::CombatMods::default()
    };
    // `arg6` stays on the ordered call: its Unit-class consumer is the crew
    // block, which the concrete death reads from the killing event.
    let outcome = damage::receive::receive_damage(
        receiver_input,
        warhead.cell_spread_f64,
        warhead.percent_at_max_f64,
        &warhead.verses_f64,
        &target_view,
        &combat_mods,
        &gates,
        distance_leptons,
        scenario_no_damage,
        rules.combat_damage.max_damage,
    );
    let invulnerability_impact = outcome.invulnerability_impact_damage.map(|doubled_damage| {
        let flags = match active_invulnerability
            .expect("receiver gate retained active state")
            .kind
        {
            crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain => 1,
            crate::sim::superweapon::invulnerability::InvulnKind::ForceShield => 6,
        };
        InvulnerabilityImpactEffect {
            target_id: target.stable_id(),
            doubled_damage,
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
    let spread = X87Chop53::load_f32(NativeF32Bits::from_bits(
        (warhead.cell_spread_f64 as f32).to_bits(),
    ))
    .expect("CellSpread parser retains a finite native f32");
    let spread_i32 =
        X87Chop53::ftol_i64(spread).expect("finite CellSpread converts through native ftol") as i32;
    let denominator = spread_i32.wrapping_shl(8);
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
        .then(|| postmortem_delay_duration(warhead, event.distance_leptons.unwrap_or(0)))
}

/// Type vtable `+0xAC` value used by receiver anger feedback. Unit, Infantry,
/// and Aircraft types return `Cost=` directly; BuildingType applies its active
/// bundled-pad and `FreeUnit=` deductions.
fn receiver_type_value(target: &GameEntity, object: &ObjectType, rules: &RuleSet) -> i32 {
    if target.category != EntityCategory::Structure {
        return object.cost;
    }
    rules.building_actual_cost(object)
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

fn near_center_ic_isolation_armed(
    damage_events: &[EntityDamageEvent],
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    damage_events.iter().any(|event| {
        event.near_center_ic_isolation_eligible
            && event.distance_leptons.is_some_and(|distance| distance < 85)
            && entities.get(event.target_id).is_some_and(|target| {
                has_active_area_invulnerability(target, current_tick)
                    && target.invulnerability.as_ref().is_some_and(|state| {
                        state.kind
                            == crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain
                    })
            })
    })
}

fn area_near_center_ic_isolation_armed(
    receivers: &[combat_aoe::AreaDamageReceiver],
    entities: &EntityStore,
    current_tick: u64,
) -> bool {
    receivers.iter().any(|receiver| {
        let combat_aoe::AreaDamageReceiver::Entity(event) = receiver else {
            return false;
        };
        event.near_center_ic_isolation_eligible
            && event.distance_leptons.is_some_and(|distance| distance < 85)
            && entities.get(event.target_id).is_some_and(|target| {
                has_active_area_invulnerability(target, current_tick)
                    && target.invulnerability.as_ref().is_some_and(|state| {
                        state.kind
                            == crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain
                    })
            })
    })
}

/// Transient per-tick bag of the Phase-2 fire-emission outputs. Bundles the
/// emit vectors so the per-attacker fire body (`resolve_attacker_fire`) can push
/// through one `&mut` handle. Never stored on `Simulation`, never serialized,
/// never hashed — destructured back into the named locals after the Phase-2 loop.
#[derive(Default)]
pub(crate) struct CombatEmit {
    /// One receiver-ordered consequence accumulator shared by weapon emission
    /// and fatal damage. Radiation is drained at its earlier ordinary phase.
    pub(crate) effects: DeathEffects,
    /// Persistent ordinary bullets admitted by accepted weapon fire. The world
    /// inserts them only after this frame's BulletClass pass has completed.
    pub(crate) projectile_spawns: Vec<ProjectileSpawn>,
    /// Native-order ReceiveDamage calls, including raw area records.
    pub(crate) damage_events: Vec<combat_aoe::AreaDamageReceiver>,
    pub(crate) remove_attack: Vec<u64>,
    /// (attacker_id, new_target_id)
    pub(crate) retarget_events: Vec<(u64, u64)>,
    pub(crate) fire_events: Vec<SimFireEvent>,
    pub(crate) reveal_events: Vec<RevealEvent>,
    /// (id, burst_delay, rof_cd)
    pub(crate) burst_updates: Vec<(u64, u8, u16)>,
    /// aircraft that fired this tick
    pub(crate) ammo_deduct: Vec<u64>,
    /// building IDs to advance fire index
    pub(crate) garrison_advance: Vec<u64>,
    pub(crate) pending_infantry_updates: Vec<(u64, Option<PendingInfantryFire>)>,
    pub(crate) animation_switches: Vec<(u64, SequenceKind)>,
    /// Native `CurrentWeaponNumber` writes emitted by live weapon selection.
    /// The per-attacker host commits these before that attack's receivers run.
    pub(crate) current_weapon_updates: Vec<(u64, u8, InternedId)>,
    /// Per-Unit post-Foot Facing slot output — captured at Phase-2 entry before
    /// current-frame attacker damage; that Unit's own explicit retarget/remove
    /// may replace it. Applied post-batch by `unit_post::apply_unit_facing`.
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

fn projectile_impact_cell(impact: ProjectileCoord) -> (u16, u16, SimFixed, SimFixed, i32) {
    let rx = impact.x.div_euclid(256).clamp(0, i32::from(u16::MAX)) as u16;
    let ry = impact.y.div_euclid(256).clamp(0, i32::from(u16::MAX)) as u16;
    (
        rx,
        ry,
        SimFixed::from_num(impact.x.rem_euclid(256)),
        SimFixed::from_num(impact.y.rem_euclid(256)),
        impact.z,
    )
}

fn emit_projectile_shrapnel(
    detonation: &ProjectileDetonation,
    entities: &EntityStore,
    occupancy: &OccupancyGrid,
    rules: &RuleSet,
    interner: &mut StringInterner,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    house_alliances: &HouseAllianceMap,
    scenario_rng: &mut SimRng,
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

    let target_position = match detonation.target {
        ProjectileTarget::Entity(id) => entities.get(id).map(|entity| {
            ProjectileCoord::new(
                i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
                i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
                i32::from(entity.position.z) * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
            )
        }),
        ProjectileTarget::Cell { rx, ry } => {
            Some(crate::sim::projectile::cell_target_coord(terrain, rx, ry))
        }
        ProjectileTarget::None => Some(ProjectileCoord::new(0, 0, 0)),
        ProjectileTarget::DummyCell => Some(
            terrain
                .map(crate::map::resolved_terrain::ResolvedTerrainGrid::shared_cell_dummy)
                .as_ref()
                .map(crate::sim::projectile::dummy_cell_target_coord)
                .unwrap_or(ProjectileCoord::new(0, 0, 0)),
        ),
    };
    let distance_cells = target_position.map_or(0, |target| {
        let dx = i64::from(target.x - detonation.impact.x);
        let dy = i64::from(target.y - detonation.impact.y);
        let dz = i64::from(target.z - detonation.impact.z);
        (dx.saturating_mul(dx)
            .saturating_add(dy.saturating_mul(dy))
            .saturating_add(dz.saturating_mul(dz)))
        .isqrt()
        .saturating_div(256) as i32
    });
    let count = projectile_shrapnel_count(
        parent_projectile.shrapnel_count,
        entities.get(detonation.source_id).is_some(),
        distance_cells,
    );
    if count == 0 {
        return;
    }

    let center_rx = detonation.impact.x / 256;
    let center_ry = detonation.impact.y / 256;
    let source_owner = entities
        .get(detonation.source_id)
        .map(|source| source.owner());
    // Random CellClass selections must capture their target coordinate at the
    // lookup call point: every miss returns the same mutable process dummy, so
    // resolving a collected list afterward would give all missed children the
    // final lookup's coordinate. Entity entries deliberately remain live reads
    // until child construction, preserving their existing behavior.
    let mut targets: Vec<(ProjectileTarget, Option<ProjectileCoord>)> =
        Vec::with_capacity(count as usize);
    let scan_radius = child_weapon.range.to_num::<i32>().max(0);
    for &(dx, dy) in self::cell_spread::splash_cells(SimFixed::from_num(scan_radius))
        .iter()
        .skip(1)
    {
        if targets.len() == count as usize {
            break;
        }
        let rx = center_rx + i32::from(dx);
        let ry = center_ry + i32::from(dy);
        let (Ok(rx), Ok(ry)) = (u16::try_from(rx), u16::try_from(ry)) else {
            continue;
        };
        let Some(target_id) = occupancy
            .get(rx, ry)
            .and_then(|cell| {
                cell.iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground)
                    .next()
            })
            .map(|occupant| occupant.entity_id)
        else {
            continue;
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
                    let (rx, ry, sub_x, sub_y) = target_coords(entity, Some(rules), interner);
                    ProjectileCoord::new(
                        i32::from(rx) * 256 + sub_x.to_num::<i32>(),
                        i32::from(ry) * 256 + sub_y.to_num::<i32>(),
                        i32::from(entity.position.z)
                            * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
                    )
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
        out.projectile_spawns.push(ProjectileSpawn {
            flat: child_projectile.flat,
            source_id: detonation.source_id,
            origin: detonation.impact,
            target,
            initial_target_position: target_coord,
            payload: ProjectilePayload {
                base_damage: child_weapon.damage,
                warhead: interner.intern(child_warhead_name),
                weapon: interner.intern(child_weapon_name),
                owner: detonation.payload.owner,
            },
            speed_leptons_per_frame: child_weapon.speed.clamp(1, i32::from(u16::MAX)) as u16,
            velocity: crate::sim::projectile::launch::shrapnel_launch_velocity(
                detonation.impact,
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
                level_non_water: child_projectile.level,
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
/// resolved any garrison occupant. Single source of the snapshot field-reads so
/// the legacy Phase-1 loop and the per-object Fire→Facing host build byte-identical
/// snapshots (no field-read drift between the two call sites).
pub(crate) fn build_attacker_snapshot(
    entity: &GameEntity,
    target: TargetKind,
    cooldown_ticks: u16,
    burst_delay_ticks: u8,
    pending_infantry_fire: Option<PendingInfantryFire>,
    pending_building_fire: Option<PendingBuildingFire>,
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
        facing: entity.facing,
        veterancy: entity.veterancy,
        cooldown_ticks,
        animation_sequence: entity.animation.as_ref().map(|a| a.sequence),
        animation_frame: entity.animation.as_ref().map(|a| a.frame_index),
        is_prone: entity
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone),
        is_fully_deployed: entity.is_fully_deployed(),
        has_movement: entity.movement_target.is_some(),
        pending_infantry_fire,
        pending_building_fire,
        barrel_facing: entity.barrel_facing,
        hull_facing: entity.body_facing,
        burst_delay_ticks,
        weapon_override: entity.weapon_override,
        garrison,
        scan_mission: threat_range::scan_mission_for(entity),
    }
}

/// Veterancy at or above this counts as veteran for the score award.
const VETERAN_VETERANCY: u16 = 100;
/// Veterancy at or above this counts as elite for the score award.
const ELITE_VETERANCY: u16 = 200;

/// Score value destroying `victim` is worth, before the allied-victim zeroing the
/// caller applies.
///
/// gamemd's kill-record step asks the victim's type for its value and that
/// accessor returns the type's **`Cost=`** — the same field production charges —
/// doubled at veteran and tripled at elite.
///
/// It is emphatically NOT `Points=`. `Points=` parses into its own type field
/// that nothing in the binary ever reads back: the only references are the
/// constructor zeroing it, the INI store, and the type-checksum walk. It is
/// dormant Tiberian Sun legacy in YR, so this engine does not parse it at all.
/// The two are not even proportional — a Rhino is `Cost=900 / Points=25` while a
/// GI is `200 / 10` — so using `Points=` both shrinks the column and reorders the
/// table.
///
/// gamemd passes the victim's house so its cost bonuses apply on top. This engine
/// models no per-house cost modifier anywhere — production charges the raw
/// `Cost=` too — so the award is the raw cost, consistent with what the player
/// was actually charged. UNCHECKED against the native bonus set.
pub(crate) fn score_award_for_victim(victim: Option<&ObjectType>, veterancy: u16) -> i32 {
    let cost = victim.map_or(0, |obj| obj.cost);
    if cost <= 0 {
        return 0;
    }
    let multiplier = if veterancy >= ELITE_VETERANCY {
        3
    } else if veterancy >= VETERAN_VETERANCY {
        2
    } else {
        1
    };
    cost.saturating_mul(multiplier)
}

/// Resolve and pay one kill's veterancy award.
///
/// gamemd-derived: `TechnoClass::Record_The_Kill @ 0x00702D40`. The award is the
/// victim's cost, zeroed when the two houses are allied and otherwise doubled
/// for a veteran victim or tripled for an elite one; the recipient's own cost
/// then divides it inside `VeterancyClass::Add @ 0x0074FF50`.
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
///    the building's fire index (`+0x688[+0x69C]`, `0x00702F98..0x00702FEA`).
///    NOT MODELLED: it needs a `Trainable=no` occupiable type, and every stock
///    `CanBeOccupied=yes` section leaves `Trainable=` at its default of yes,
///    so branch 2 pays the BUILDING in stock and the occupant never promotes.
/// 5. else nobody.
///
/// RESIDUAL — the costs on both sides are `TechnoTypeClass::GetActualCost`
/// (vtable `+0x84`) evaluated with the VICTIM's house (`0x00702ED1`,
/// `0x00702F13`, `0x00702F77`, `0x00702FD4`), not the bare `Cost=`. Stock
/// countries author no cost multipliers, so the two agree in every unmodded
/// match; a mod with per-country cost mults diverges. Frequency: zero in
/// stock.
#[allow(clippy::too_many_arguments)]
pub(crate) fn award_kill_experience(
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    alliances: &HouseAllianceMap,
    killer_id: u64,
    victim_id: u64,
) {
    if killer_id == RAD_NO_ATTACKER || killer_id == victim_id {
        return;
    }
    // `Record_The_Kill` reads the VICTIM type's `DontScore=` byte (`+0xC9F`) at
    // 0x00702E4E and returns before the rank multiplier, before the accumulator
    // and before the score add. Stock authors it on `SLAV`, `V3ROCKET`, `DMISL`
    // and `CMISL` — the last three are the V3, Dreadnought and Boomer missiles,
    // shot down by AA in most matches, so without this gate every intercepted
    // missile promotes the interceptor.
    let Some((victim_cost, victim_rank, victim_owner)) = entities
        .get(victim_id)
        .filter(|victim| !victim.dont_score)
        .map(|victim| {
            (
                rules
                    .object(interner.resolve(victim.type_ref()))
                    .map_or(0, |obj| obj.cost),
                self::veterancy::rank_of(victim.veterancy_raw),
                victim.owner(),
            )
        })
    else {
        return;
    };
    let Some(killer) = entities.get(killer_id) else {
        return;
    };
    let Some(killer_type) = rules.object(interner.resolve(killer.type_ref())) else {
        return;
    };
    let killer_owner = killer.owner();
    let trainable_cost = |id: u64| -> Option<(u64, i32)> {
        let object = rules.object(interner.resolve(entities.get(id)?.type_ref()))?;
        object.trainable.then_some((id, object.cost))
    };
    // Branch 1: a passenger firing from an OpenTopped transport pays its
    // transporter. `passenger_role.Inside` plus the transport's `OpenTopped=`
    // is the `+0x82`/`+0x11C` pair.
    let open_transporter = match killer.passenger_role {
        crate::sim::passenger::PassengerRole::Inside { transport_id } => entities
            .get(transport_id)
            .and_then(|transport| rules.object(interner.resolve(transport.type_ref())))
            .is_some_and(|transport_type| transport_type.open_topped)
            .then_some(transport_id),
        _ => None,
    };
    let recipient = if let Some(transporter) = open_transporter.and_then(trainable_cost) {
        Some(transporter)
    } else if killer_type.trainable {
        // Branch 2: the killer itself.
        Some((killer_id, killer_type.cost))
    } else if killer_type.missile_spawn {
        // Branch 3: a spawned missile pays its launcher.
        killer.spawn_owner_id.and_then(trainable_cost)
    } else {
        // Branch 4 (garrison occupant) is stock-unreachable — see above.
        None
    };
    // `0x00702E64` loads the KILLER's house and calls `HouseClass::IsAlly @
    // 0x004F9A90`, which reads only the asker's own ally bitfield — a one-way
    // test, not the symmetric OR most "don't shoot me" call sites want.
    let allied = crate::map::houses::is_allied_with(
        alliances,
        interner.resolve(killer_owner),
        interner.resolve(victim_owner),
    );
    let points = self::veterancy::kill_award_points(victim_cost, victim_rank, allied);
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

/// Record who destroyed `victim`, at the instant it happened.
///
/// Every lethal path funnels through here so there is one capture rather than a
/// recording mechanism per death cause. Call it immediately after zeroing a
/// victim's health, with the house that should be credited.
///
/// No-ops unless the victim is actually at zero health, and the first writer
/// wins within one fatal transaction. A qualifying PostMortem callback consumes
/// and clears this deferred-UnInit latch before restoring the object, so a later
/// independent lethal transaction can attribute freshly. The award is resolved
/// here because the rules are in hand and the veterancy is still the value the
/// object died at.
///
/// Routed today: the projectile/damage-event loop, death-explosion area damage,
/// and crushing. Spawner missiles arrive through the damage loop with the firer
/// set to the launching object, so they credit the launcher's house; if the
/// launcher itself dies during the missile's flight the firer no longer resolves
/// and that kill goes uncredited.
///
/// NOT routed yet, because each site would need a `&RuleSet` threaded into a
/// function that does not take one: the Iron Curtain and Genetic Mutator infantry
/// kills, aircraft self-destruct, passengers dying with their transport
/// (`world::lifecycle`) or with a collapsing bridge, and passengers ejected by a
/// sell. Those victims book a Loss with no matching Kill.
pub(crate) fn capture_kill_credit(
    victim: &mut crate::sim::game_entity::GameEntity,
    killer_owner: Option<InternedId>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) {
    if victim.health.current != 0 {
        return;
    }
    record_kill_credit(victim, killer_owner, rules, interner);
}

/// `Record_The_Kill`'s kill and score half for a victim that may still have
/// health: a Chrono Legionnaire's erase calls vtable `+0xE0` on a target it
/// removes at full health (`TemporalClass::Update @ 0x0071AAC4`, then UnInit).
/// The destruction record (`record_destruction_once`) books the loss for a
/// victim credited here, as for one at zero health. [`capture_kill_credit`]
/// adds the zero-health gate the damage paths need.
pub(crate) fn record_kill_credit(
    victim: &mut crate::sim::game_entity::GameEntity,
    killer_owner: Option<InternedId>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) {
    // `DontScore=` victims are invisible to the score screen entirely. gamemd
    // returns on this byte before any of its bookkeeping, so the kill and the
    // points are both suppressed here and the loss is suppressed at the
    // lifecycle recorder — the two halves of that one native early return.
    if victim.dont_score {
        return;
    }
    if victim.killed_by.is_some() {
        return;
    }
    let Some(killer_owner) = killer_owner else {
        return;
    };
    victim.killed_by = Some(killer_owner);
    victim.kill_award_points = score_award_for_victim(
        rules.object(interner.resolve(victim.type_ref())),
        victim.veterancy,
    );
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
/// - Frequency: every Destroyer ASW acquisition that reaches this predicate —
///   pursuit (`world_orders.rs`), the greatest-threat scan, the attack cursor —
///   so ordinary naval play, not an edge case.
/// - Downstream risk: pursuit walks the Destroyer to two cells before it will
///   fire, and target selection agrees with it, so the drift is consistent
///   rather than self-correcting.
///
/// Pre-existing, not introduced here; recorded because the lepton scaling on
/// the line above rewrote this function while leaving the sentinel unhandled.
/// The fix belongs with the remaining `is_within_range_leptons` call sites'
/// migration onto `compute_in_range`, not with a second sentinel test bolted
/// on here.
///
/// RESIDUAL 2 — this twin also has no line-of-fire walk. `TechnoClass::InRange`
/// ends in `CALL 0x004CC310` at 0x006F7642 and refuses the shot when a wall or
/// a cliff sits on the line; `compute_in_range` runs that walk (see
/// `sim::combat::line_of_fire`) and this function does not.
///
/// Pursuit no longer reaches it: `World::tick_attack_pursuit` measures through
/// `pursuit_in_range` → `compute_in_range`, matching the approach search
/// `FootClass::Greatest_Threat_Scan @ 0x004D5690`, which decides with `InRange`
/// 0x006F7220 itself. Two production readers still take the plain radius, each
/// recorded on its own call site:
///
/// - the fire gate's GARRISON branch (`resolve_attacker_fire`, the
///   `is_garrison || effective_range != weapon.range` arm);
/// - `ScanRange::Hard` in `greatest_threat::evaluate_candidate`, the
///   garrison passive scan's override.
///
/// The remaining readers are the no-resolved-terrain fallbacks in the fire
/// gate, the cursor and pursuit, which cannot run a walk at all and therefore
/// agree with each other rather than diverging.
///
/// - Trigger: a garrisoned occupant firing, or a garrison passive scan
///   choosing a candidate, across a wall or a ≥4-Level step.
/// - Player effect: garrisoned infantry shoot through a wall the identical
///   infantry standing in the open is refused; the passive scan can pick a
///   candidate behind one.
/// - Frequency: routine on urban maps, where garrisoning is a normal opening.
/// - Downstream risk: none to deterministic state; both stages agree with each
///   other, so it is a uniformly wrong answer, not a stall. The cure is
///   threading the override-aware range into `compute_in_range` so the garrison
///   branch can use the 3-D gate — the range VALUE chain M8 already records,
///   not a second walk bolted onto this function.
pub(crate) fn is_within_range_leptons(dist_sq_leptons: i64, range_cells: SimFixed) -> bool {
    let range_leptons: i64 = (i64::from(range_cells.to_bits()) * 256) >> 16;
    let range_sq: i64 = range_leptons * range_leptons;
    dist_sq_leptons <= range_sq
}

/// The end-of-burst reload, from `TechnoClass::GetROF @ 0x006FCFA0`.
///
/// gamemd-derived: the full-ROF branch computes
/// `ftol(ROF * house difficulty ROF + Random::RandomRanged(0, 2))`. `ROF=` is
/// already a native frame count, and the jitter is an ADDED integer, not a
/// scale — a shot's reload is `ROF`, `ROF + 1` or `ROF + 2`. The draw is
/// unconditional on this branch, so it must stay in the same slice as the
/// mid-burst draw or the scenario stream shifts twice.
///
/// The `VeteranROF=` arm follows in `veteran_rof_frames`.
///
/// RESIDUAL (GSI-08.05) — two arms of the native function are still absent.
/// - The per-house difficulty multiplier. Native scales `ROF` by the owning
///   house's difficulty ROF before the truncation; VERA parses no
///   `[Easy]/[Normal]/[Difficult] ROF=` and plumbs no per-house difficulty to
///   this site. Frequency today: zero — every house in VERA is Normal, whose
///   stock value is `1.0`, and there is no AI opponent to carry another.
///   It becomes ±20% on every weapon in the game the moment a difficulty other
///   than Normal can reach a house.
/// - `RadialFireSegments=` (`TechnoTypeClass+0x6A4`) is not parsed. One stock
///   author, `[AEGIS]`, which is buildable in an ordinary skirmish: native
///   replaces the launch direction with
///   `body facing + (PI * counter / segments - PI / 2)`, cycling a counter at
///   `TechnoClass+0x43C`. Player effect: the Aegis Cruiser fires straight at
///   one target instead of sweeping its flak arc. Frequency: every Aegis
///   engagement in an Allied naval match.
/// - Downstream risk: both change firing cadence or direction, so each moves
///   combat-timing fixtures and the pinned replay hash.
fn rof_to_cooldown_frames(rof_frames: i32, scenario_rng: &mut SimRng) -> u16 {
    let jitter = scenario_rng.next_range_u32_inclusive(0, 2) as i32;
    rof_frames.saturating_add(jitter).clamp(1, u16::MAX as i32) as u16
}

/// The `VeteranROF=` arm of `TechnoClass::GetROF @ 0x006FCFA0`.
///
/// gamemd-derived: `0x006FD0E2..0x006FD14C` — the inline `HasWeaponAbility(4)`
/// (veteran byte `+0x2A0`, elite byte `+0x2B2`) selects
/// `ftol(rof * Rules.VeteranROF)` (`FILD; FMUL [Rules+0x690]; ftol`) on the
/// already-jittered integer. Applied ONCE — there is no `EliteROF` key in the
/// binary. Stock `0.6` turns the elite Grizzly's 50..=52 into 30, 30, 31.
///
/// The `.max(1)` is VERA-internal: native stores whatever `ftol` yields, and
/// a zero reload is only reachable with `ROF=1`/`ROF=0` weapons, which no
/// stock type authors; it keeps the existing floor `rof_to_cooldown_frames`
/// applies (gamemd equivalent UNCHECKED).
fn veteran_rof_frames(
    rof_ticks: u16,
    rank: self::veterancy::VeterancyRank,
    object: &ObjectType,
    veteran_rof: f64,
) -> u16 {
    self::veterancy::scale_if_ability(
        i32::from(rof_ticks),
        rank,
        object,
        crate::rules::object_type::Ability::Rof,
        veteran_rof,
    )
    .clamp(1, i32::from(u16::MAX)) as u16
}

pub(crate) use self::combat_targeting::acquire_best_target_for_entity;
pub use self::combat_targeting::tick_retaliation;
/// The threat mask an acquisition callsite pushes into
/// `TechnoClass::Greatest_Threat @ 0x006F8DF0`, plus the passive block's own
/// derivation of it. Re-exported because the mask is chosen by the mission
/// handlers in `sim/world/`, not inside `combat/`.
pub use self::threat_range::ScanMission;
pub(crate) use self::threat_range::scan_mission_for;

/// Impact-height tests: a shot that lands on a ground cell must take that
/// cell's terrain floor height, not a constant. Kept inline because they pin
/// `attack_impact_z` and the single-impact-coordinate wiring that lives in
/// this file.
#[cfg(test)]
mod impact_height_tests {
    use super::*;
    use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
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

    #[test]
    fn gsi_04_11_fatal_infantry_special_anim_emits_effect_and_smudge_request() {
        let mut interner = test_interner();
        let general = crate::rules::ruleset::GeneralRules::default();
        let cases = [
            (1, None),
            (2, None),
            (3, Some("S_BANG34")),
            (4, Some("FLAMEGUY")),
            (5, Some("ELECTRO")),
            (6, Some("YURIDIE")),
            (7, Some("NUKEDIE")),
            (8, Some("VIRUSD")),
            (9, Some("GENDEATH")),
            (10, Some("BRUTDIE")),
        ];
        for (inf_death, expected_name) in cases {
            let mut effects = Vec::new();
            let mut smudges = Vec::new();
            emit_infantry_death_anim(
                &general,
                inf_death,
                7,
                8,
                SimFixed::from_num(64),
                SimFixed::from_num(192),
                2,
                208,
                &mut interner,
                &mut effects,
                &mut smudges,
            );
            let Some(expected_name) = expected_name else {
                assert!(effects.is_empty(), "InfDeath {inf_death}");
                assert!(smudges.is_empty(), "InfDeath {inf_death}");
                continue;
            };
            assert_eq!(effects.len(), 1, "InfDeath {inf_death}");
            assert_eq!(smudges.len(), 1, "InfDeath {inf_death}");
            assert_eq!(interner.resolve(effects[0].shp_name), expected_name);
            assert_eq!((effects[0].rx, effects[0].ry, effects[0].z), (7, 8, 2));
            let SmudgeSpawnRequest::Anim {
                anim_name,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } = &smudges[0]
            else {
                panic!("special death effect must run the Anim smudge start path");
            };
            assert_eq!(interner.resolve(*anim_name), expected_name);
            assert_eq!(
                (
                    *rx,
                    *ry,
                    sub_x.to_num::<i32>(),
                    sub_y.to_num::<i32>(),
                    *world_z_leptons,
                ),
                (7, 8, 64, 192, 208)
            );
        }
    }

    fn terrain_cell(rx: u16, ry: u16, level: u8) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level,
            filled_clear: true,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: Default::default(),
            speed_costs: Default::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            accepts_smudge: true,
            allows_tiberium: false,
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    pub(super) fn terrain_at_level(level: u8) -> ResolvedTerrainGrid {
        let cells: Vec<ResolvedTerrainCell> = (0..TEST_GRID)
            .flat_map(|ry| (0..TEST_GRID).map(move |rx| terrain_cell(rx, ry, level)))
            .collect();
        ResolvedTerrainGrid::from_cells(TEST_GRID, TEST_GRID, cells)
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
    fn cell_target_impact_z_is_the_cells_terrain_floor() {
        let entities = EntityStore::new();
        let flat = terrain_at_level(0);
        let raised = terrain_at_level(RAISED_LEVEL);

        assert_eq!(
            attack_impact_z(TargetKind::Cell(7, 9), &entities, Some(&raised)),
            i32::from(RAISED_LEVEL),
            "a ground-cell impact takes the cell's terrain floor height; a \
             constant 0 here renders the impact one whole tile below the ground \
             it landed on"
        );
        assert_eq!(
            attack_impact_z(TargetKind::Cell(7, 9), &entities, Some(&flat)),
            0,
            "level-0 ground is still zero — that is the value, not the fallback"
        );
        assert_eq!(
            attack_impact_z(TargetKind::Cell(7, 9), &entities, None),
            0,
            "no loaded map means no cell to read"
        );
        assert_eq!(
            attack_impact_z(
                TargetKind::Cell(TEST_GRID + 5, TEST_GRID + 5),
                &entities,
                Some(&raised)
            ),
            0,
            "VERA API boundary, no native equivalent: an off-map cell is not a \
             targetable cell in the first place, so this pins only that the \
             helper stays total, not a game rule about off-map heights"
        );
    }

    #[test]
    fn cell_target_impact_z_is_the_ground_floor_not_the_bridge_aim_point() {
        // A structural bridge cell whose ground floor is RAISED_LEVEL and whose
        // deck sits a full deck height above it.
        let mut cells: Vec<ResolvedTerrainCell> = (0..TEST_GRID)
            .flat_map(|ry| (0..TEST_GRID).map(move |rx| terrain_cell(rx, ry, RAISED_LEVEL)))
            .collect();
        let idx = 9 * TEST_GRID as usize + 7;
        cells[idx].bridge_facts = BridgeCellFacts {
            raw_flags: BRIDGE_FLAG_STRUCTURAL,
            ..BridgeCellFacts::default()
        };
        cells[idx].has_bridge_deck = true;
        cells[idx].bridge_walkable = true;
        cells[idx].bridge_deck_level = RAISED_LEVEL + 4;
        let terrain = ResolvedTerrainGrid::from_cells(TEST_GRID, TEST_GRID, cells);

        let entities = EntityStore::new();
        assert_eq!(
            attack_impact_z(TargetKind::Cell(7, 9), &entities, Some(&terrain)),
            i32::from(RAISED_LEVEL),
            "the impact coordinate is the projectile's own location clamped to \
             the cell's ground height, not the bridge-aware aim point — the \
             deck-adding accessor is reached only for a live object target. The \
             deck term is a recorded residual on `attack_impact_z`, not an \
             oversight"
        );
        assert_ne!(
            attack_impact_z(TargetKind::Cell(7, 9), &entities, Some(&terrain)),
            combat_aoe::bridge_adjusted_impact_z(Some(&terrain), 7, 9),
            "the aim-point helper is a different quantity; if these two ever \
             agree, the deck residual was closed and the bridge-damage Z gate \
             has to be settled in the same change"
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
    fn entity_target_impact_z_still_reads_the_entity_height() {
        let mut entities = EntityStore::new();
        let mut on_deck = GameEntity::test_default(1, "MTNK", "Americans", 7, 9);
        on_deck.position.z = 6;
        entities.insert(on_deck);
        let terrain = terrain_at_level(RAISED_LEVEL);

        assert_eq!(
            attack_impact_z(TargetKind::Entity(1), &entities, Some(&terrain)),
            6,
            "an object target still contributes its own height, terrain or not"
        );
        assert_eq!(
            attack_impact_z(TargetKind::Entity(1), &entities, None),
            6,
            "entity height does not depend on the terrain grid"
        );
        assert_eq!(
            attack_impact_z(TargetKind::Entity(404), &entities, Some(&terrain)),
            0,
            "a vanished target contributes nothing"
        );
    }

    #[test]
    fn force_fire_on_raised_ground_places_the_explosion_at_the_terrain_height() {
        let rules = impact_rules();
        let mut terrain = terrain_at_level(RAISED_LEVEL);
        let mut store = EntityStore::new();
        // `test_interner` snapshots the thread-local, so the entity's type and
        // owner strings must be interned before the snapshot is taken.
        let mut firer = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        // The fixture `MTNK` authors no `Turret=`, so its HULL is what the
        // native body gate compares (`UnitClass::GetFireError @ 0x00740FD0`
        // step 17). Face it south at the force-fire cell so this test measures
        // impact height, not turn-to-fire.
        firer.facing = 128;
        store.insert(firer);
        let mut interner = test_interner();
        assert!(
            issue_attack_cell_command(&mut store, 1, 5, 6, Some(&rules), &interner),
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
            .consequences
            .effects()
            .explosion_effects
            .first()
            .expect("force-fire should emit the warhead's impact animation");
        assert_eq!((effect.rx, effect.ry), (5, 6));
        assert_eq!(
            effect.z, RAISED_LEVEL,
            "the impact animation is placed at the impact height; a constant 0 \
             draws it 15 screen pixels per level below the ground it hit"
        );
    }
}
