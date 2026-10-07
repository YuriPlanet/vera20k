//! A nuclear silo's launch: `BuildingClass::Mission_Missile @ 0x0044C980`,
//! the mission `SuperClass::Launch` queues on a `NukeSilo=` building
//! (`superweapon::nuke`).
//!
//! The warning anim's visibility (`AnimClass::AI 0x00423B29` rewrites
//! `+0x19D` from `0x0043B4C0`, a test of the local player) is the
//! presentation's, through [`Simulation::psi_warning_detected_by`].
//!
//! RESIDUALS:
//! - The handler's other arm (`0x0044CCDE..0x0044D584`), for a building that
//!   is not a silo: only a `NukeSilo=` building is given Missile (retail's
//!   one other reader, `EMPulseCannon=`, is set on no type). Such a building
//!   only waits its mission rate here.
//! - A refused missile (`BulletClass::Fire` false, `0x0044CC40`: the bullet
//!   is deleted, which also removes the warning anim, and no take-off anim
//!   plays): VERA admits every bullet, and the silo's coordinate is on the
//!   map.
//! - The missile's `Trailer=` smoke (`[NKMSLUP] Trailer=NUKEPUFF`): VERA's
//!   bullets spawn no trailer anims.
//!
//! EVIDENCE BOUNDS: `tools.superweapon_oracle` supplies GetFLH (`vt+0xB0`)
//! and the silo's coordinate, so the missile's origin rests on the shared
//! FLH owner (`combat::fire_coord`), and its one fixture cell stands 104
//! leptons a level with no ramp. No native run times the flight from launch
//! to impact; the bullets move on the shared projectile owner's cadence.

use crate::rules::flh::Flh;
use crate::rules::ruleset::RuleSet;
use crate::sim::building_construction::BuildingBodyMode;
use crate::sim::combat::fire_coord::{FireSource, fire_coordinate};
use crate::sim::combat::{FiredBullet, admit_fired_bullet};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::projectile::{
    ProjectileCoord, ProjectilePayload, ProjectileTarget, cell_ground_coord,
};
use crate::sim::world::Simulation;
use crate::util::native_x87::NativeF64Bits;

/// `PSIWARN`, the warning anim's type (`0x0081907C`, found by name at
/// `0x0044C9FA`).
const WARNING_ANIM: &str = "PSIWARN";
/// The missile's Construct speed, pushed as an immediate (`0x0044CAC8`).
const MISSILE_SPEED: i32 = 0xFF;
/// `[0x007E44A8]` = 10.0, the missile's launch velocity scale.
const MISSILE_VELOCITY_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x4024_0000_0000_0000);
/// The take-off anim's ZAdjust, written after its constructor
/// (`0x0044CC9D`).
const TAKE_OFF_Z_ADJUST: i32 = -100;
/// Draw flags of both anims (`0x0044CA40`, `0x0044CC7D`).
const ANIM_DRAW_FLAGS: u32 = 0x600;

/// Mission_Missile for a `NukeSilo=` type (`+0x16BA`) by its status
/// (`+0xBC`, jump table `0x0044D5A4`):
/// - 0 (`0x0044C9BC`): the ready byte (`+0x6DD`) clears, Begin_Mode(Active),
///   status 1, and the warning anim stands on the owner's NukeTarget
///   ([`spawn_warning`]); falls into 1.
/// - 1 (`0x0044CA7A`): with the ready byte set, Begin_Mode(Aux1) and status
///   2; falls into 2 either way.
/// - 2 (`0x0044CA97`): the missile ([`launch_missile`]); returns 1.
/// - 3 (`0x0044CCBE`): Begin_Mode(Aux2), status 4; returns 6.
/// - 4 (`0x0044D53C`): Begin_Mode(Idle) and Guard queued, not commenced;
///   returns 60.
///
/// So the missile leaves on the first visit and the silo is back on Guard
/// 67 frames later.
pub(super) fn mission_missile(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let silo = sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.nuke_silo);
    let status = entity.mission.handler_state();
    if !silo || status > 4 {
        // `0x0044D584`: the mission rate.
        return rules.mission_control.rate_frames(MissionType::Missile);
    }
    let now = sim.session.binary_frame as i32;
    let mut warning = None;
    if status == 0 {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.mission_leaf.set_building_ready_latch(0);
            entity.begin_building_body(BuildingBodyMode::Active, now);
            entity.mission.set_handler_state(1);
        }
        warning = spawn_warning(sim, rules, id);
    }
    match status {
        0..=2 => {
            if status <= 1
                && let Some(entity) = sim.substrate.entities.get_mut(id)
                && entity.building_ready_latch() != 0
            {
                entity.begin_building_body(BuildingBodyMode::Aux1, now);
                entity.mission.set_handler_state(2);
            }
            launch_missile(sim, rules, id, warning)
        }
        3 => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.begin_building_body(BuildingBodyMode::Aux2, now);
                entity.mission.set_handler_state(4);
            }
            6
        }
        _ => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.begin_building_body(BuildingBodyMode::Idle, now);
            }
            let frame = sim.session.binary_frame;
            let _ = sim.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Guard),
                0,
                frame,
                &LiveReadyInputProvider { rules },
            );
            60
        }
    }
}

/// Status 0's warning (`0x0044C9DC..0x0044CA74`): a PSIWARN anim at the
/// owner's NukeTarget cell (its GetCoords, vt+0x48), constructed with
/// (delay 0, loop 1, flags 0x600, ZAdjust 0, not reversed), its bullet
/// (`+0x17C`, `0x00424C90`) none, its house (`+0x180`, `0x00424CA0`) the
/// silo's owner and hidden (`+0x19D`). The silo holds it at `+0x54C`
/// (`0x0044CA74`) until status 2 hands it to the missile and clears the field
/// (`0x0044CB04..0x0044CB14`); both run in one visit, so VERA passes it on.
fn spawn_warning(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
) -> Option<crate::sim::anim_class::AnimId> {
    let owner = sim.substrate.entities.get(id)?.owner();
    let (rx, ry) = sim.houses.get(&owner)?.nuke_target();
    let coord = cell_ground_coord(sim.resolved_terrain.as_ref(), rx, ry);
    let anim = spawn_anim(sim, rules, WARNING_ANIM, coord, true)?;
    sim.set_anim_owner_house(anim, owner);
    Some(anim)
}

/// Status 2 (`0x0044CA97..0x0044CCB6`): the missile, a bullet of the firing
/// type's `WeaponType=` (SuperWeaponType `+0x9C`, the type at the silo's
/// `+0x5F8`) built by `CreateBullet @ 0x0046B050` with the weapon's
/// BulletType, damage and warhead, the NukeTarget cell as target, the silo as
/// owner and speed 255, given the weapon (`SetWeaponType @ 0x0046B260`) and
/// handed to the warning anim (`0x0044CB0F`). It leaves the silo's weapon-0
/// fire coordinate with no offset (GetFLH, vt+0xB0) straight up
/// ([`crate::sim::projectile::launch::missile_launch_velocity`] at 10.0), and
/// the take-off anim (`[General] NukeTakeOff=`, `Rules+0x98`) plays there
/// with ZAdjust -100. Status 3; returns 1.
fn launch_missile(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    warning: Option<crate::sim::anim_class::AnimId>,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let owner = entity.owner();
    let firing = entity
        .mission_leaf
        .as_building()
        .map_or(-1, |leaf| leaf.firing_super_weapon());
    let weapon = usize::try_from(firing)
        .ok()
        .and_then(|index| rules.super_weapon_order.get(index))
        .and_then(|name| rules.super_weapon(name))
        .and_then(|sw| sw.weapon_type.as_deref())
        .and_then(|name| rules.weapon(name));
    let warhead = weapon
        .and_then(|weapon| weapon.warhead.as_deref())
        .and_then(|name| rules.warhead(name));
    let (Some(weapon), Some(warhead), Some(object)) =
        (weapon, warhead, sim.object_type(entity.type_ref(), rules))
    else {
        // Native dereferences the missing WeaponType or warhead.
        return 1;
    };
    let Some((rx, ry)) = sim.houses.get(&owner).map(|house| house.nuke_target()) else {
        return 1;
    };
    let origin = fire_coordinate(
        sim,
        rules,
        &FireSource::of_entity(entity),
        object,
        0,
        0,
        Flh::default(),
    )
    .coord;
    let bullet_id = sim.allocate_stable_id();
    let native_unique_id = sim.next_native_runtime_id();
    if let Some(warning) = warning {
        sim.set_anim_attached_bullet(warning, Some(bullet_id));
    }
    let velocity = crate::sim::projectile::launch::missile_launch_velocity(MISSILE_VELOCITY_SCALE);
    let payload = ProjectilePayload::new(
        weapon.damage,
        sim.interner.intern(&warhead.id),
        sim.interner.intern(&weapon.id),
    );
    if let Some(bullet) = FiredBullet::launched(
        sim,
        native_unique_id,
        id,
        ProjectileTarget::Cell { rx, ry },
        payload,
        MISSILE_SPEED,
        origin,
        velocity,
    ) {
        admit_fired_bullet(sim, rules, weapon, bullet_id, bullet);
    }
    let take_off = rules.general.nuke_take_off.clone();
    if let Some(anim) = spawn_anim(sim, rules, &take_off, origin, false) {
        sim.set_anim_z_adjust(anim, TAKE_OFF_Z_ADJUST);
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.set_handler_state(3);
    }
    1
}

/// `AnimClass::AnimClass @ 0x00421EA0` with (delay 0, loop 1, flags 0x600,
/// ZAdjust 0, not reversed) at `coord`; `hidden` is the producer's `+0x19D`.
fn spawn_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    name: &str,
    coord: ProjectileCoord,
    hidden: bool,
) -> Option<crate::sim::anim_class::AnimId> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let world = crate::sim::anim_class::AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    };
    let type_name = sim.interner.intern(&name.to_ascii_uppercase());
    let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
    let descriptor = AnimClassSpawnDescriptor {
        delay: 0,
        loop_count: 1,
        draw_flags: ANIM_DRAW_FLAGS,
        z_adjust: 0,
        reverse: false,
        draw_runtime: crate::sim::anim_class::AnimDrawRuntime {
            hidden,
            ..Default::default()
        },
        ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
    };
    match sim.spawn_anim_at_world(rules, descriptor, world) {
        Ok(anim) => Some(anim),
        Err(error) => {
            log::debug!("nuclear silo anim [{name}] did not construct: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    //! Native comparisons (`tools/superweapon_oracle.py` section
    //! `mission_missile`; `--check` regenerates it).

    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, test_flat_cell, test_grid};
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;
    use crate::sim::projectile::ProjectileTrajectory;
    use serde_json::Value;

    /// Only the superweapon names `NukeCarrier`, as in a rules file
    /// without retail's `[WEEDGUY] Primary=NukeCarrier`.
    const RULES: &str = "[General]\nNukeTakeOff=NUKETO\n\
        [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
        [BuildingTypes]\n0=NAMISL\n1=GAPILE\n\
        [SuperWeaponTypes]\n0=NukeSpecial\n\
        [NukeSpecial]\nType=MultiMissile\nWeaponType=NukeCarrier\n\
        [NAMISL]\nStrength=1000\nNukeSilo=yes\nSuperWeapon=NukeSpecial\n\
        [GAPILE]\nStrength=1000\n\
        [NukeCarrier]\nDamage=1000\nProjectile=GiantNukeUp\nSpeed=100\nWarhead=NukeMaker\n\
        [GiantNukeUp]\nArm=2\nAcceleration=1\nVertical=yes\nDetonationAltitude=20000\n\
        [Warheads]\n0=NukeMaker\n\
        [NukeMaker]\nNukeMaker=yes\n\
        [Animations]\n0=PSIWARN\n1=NUKETO\n\
        [Missile]\nRate=1\n";
    const ART: &str = "[NAMISL]\nFoundation=1x1\n[GAPILE]\nFoundation=1x1\n\
        [PSIWARN]\n[NUKETO]\n";

    fn oracle() -> Value {
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
    }

    fn int(value: &Value) -> i32 {
        i32::try_from(value.as_i64().unwrap()).unwrap()
    }

    fn rules() -> RuleSet {
        let art = IniFile::from_str(ART);
        let mut rules =
            RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(RULES), &art).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules
    }

    /// A silo (or a building that is not one) at (20, 30) whose owner's
    /// NukeTarget is `target`, on ground raised `target_level` there.
    fn world(
        rules: &RuleSet,
        kind: &str,
        target: (u16, u16),
        target_level: u8,
    ) -> (Simulation, u64) {
        let mut sim = Simulation::with_seed(5);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let owner = sim.interner.intern("Russians");
        sim.houses
            .insert(owner, HouseState::new(owner, 1, None, true, 0, 10));
        sim.session.house_order.push(owner);
        sim.resolved_terrain = Some(test_grid(80, 220, |x, y| ResolvedTerrainCell {
            level: if (x, y) == target { target_level } else { 0 },
            ..test_flat_cell(x, y)
        }));
        let silo = sim
            .spawn_object_at_height(kind, "Russians", 20, 30, 0, 0, rules)
            .unwrap();
        sim.houses.get_mut(&owner).unwrap().set_nuke_target(target);
        sim.substrate
            .entities
            .get_mut(silo)
            .unwrap()
            .mission_leaf
            .set_building_firing_super_weapon(0);
        (sim, silo)
    }

    fn velocity_bits(velocity: crate::sim::projectile::ProjectileVelocity) -> Vec<u64> {
        velocity
            .native()
            .iter()
            .map(|component| component.bits())
            .collect()
    }

    /// Each status of a silo's Missile against the original, for a missile
    /// that constructs and fires; the anims, bullet and stores follow it.
    #[test]
    fn mission_missile_matches_native() {
        let oracle = oracle();
        let rows = oracle["mission_missile"].as_array().unwrap();
        // A refused Fire or a failed construction (three rows) and the other
        // arm (one row) are not replayed: VERA admits every bullet, and the
        // other arm only waits its mission rate here.
        let fired: Vec<&Value> = rows
            .iter()
            .filter(|row| {
                row["silo"] == true && row["fire_accepts"] == true && row["bullet"] == true
            })
            .collect();
        assert_eq!((rows.len(), fired.len()), (15, 11));
        let rules = rules();
        for row in fired {
            let target = (
                u16::try_from(int(&row["target"][0])).unwrap(),
                u16::try_from(int(&row["target"][1])).unwrap(),
            );
            let level = u8::try_from(int(&row["target_level"])).unwrap();
            let (mut sim, silo) = world(&rules, "NAMISL", target, level);
            let owner = sim.substrate.entities.get(silo).unwrap().owner();
            {
                let entity = sim.substrate.entities.get_mut(silo).unwrap();
                entity
                    .mission
                    .set_handler_state(u32::try_from(int(&row["status"])).unwrap());
                entity
                    .mission_leaf
                    .set_building_ready_latch(u8::from(row["ready"] == true));
            }
            let mode_before = sim
                .substrate
                .entities
                .get(silo)
                .unwrap()
                .queued_building_body_state();
            let anims_before: Vec<_> = sim.substrate.anims.iter().map(|(&id, _)| id).collect();
            let delay = mission_missile(&mut sim, silo, &rules);
            assert_eq!(delay, int(&row["delay"]), "{row}");
            let entity = sim.substrate.entities.get(silo).unwrap();
            assert_eq!(
                (
                    entity.mission.handler_state(),
                    entity.building_ready_latch()
                ),
                (
                    u32::try_from(int(&row["status_after"])).unwrap(),
                    u8::try_from(int(&row["ready_after"])).unwrap()
                ),
                "{row}"
            );
            let events = row["events"].as_array().unwrap();
            let modes: Vec<i32> = events
                .iter()
                .filter(|event| event[0] == "begin_mode")
                .map(|event| int(&event[1]))
                .collect();
            assert_eq!(
                entity.queued_building_body_state(),
                modes.last().copied().or(mode_before),
                "{row}"
            );
            let new_anims: Vec<_> = sim
                .substrate
                .anims
                .iter()
                .filter(|(id, _)| !anims_before.contains(id))
                .collect();
            let native_anims: Vec<&Value> =
                events.iter().filter(|event| event[0] == "anim").collect();
            assert_eq!(new_anims.len(), native_anims.len(), "{row}");
            let bullets: Vec<_> = sim
                .projectiles
                .iter()
                .filter(|(_, bullet)| bullet.source_id == silo)
                .collect();
            let fire = events.iter().find(|event| event[0] == "fire");
            assert_eq!(bullets.len(), usize::from(fire.is_some()), "{row}");
            let Some(fire) = fire else { continue };
            let (&bullet_id, bullet) = bullets[0];
            let native_velocity: Vec<u64> = fire[2]
                .as_array()
                .unwrap()
                .iter()
                .map(|bits| bits.as_u64().unwrap())
                .collect();
            assert_eq!(velocity_bits(bullet.velocity), native_velocity, "{row}");
            // Construct's damage and speed; the carrier's BulletType.
            let construct = events
                .iter()
                .find(|event| event[0] == "create_bullet")
                .unwrap();
            assert_eq!(bullet.payload.base_damage, int(&construct[2]), "{row}");
            assert_eq!(
                bullet.trajectory,
                ProjectileTrajectory::Vertical {
                    detonation_altitude: 20000,
                    acceleration: 1,
                    max_speed: int(&construct[3]),
                },
                "{row}"
            );
            assert_eq!(
                bullet.target,
                crate::sim::projectile::ProjectileTarget::Cell {
                    rx: target.0,
                    ry: target.1
                }
            );
            for (&(&id, anim), native) in new_anims.iter().zip(&native_anims) {
                let name = sim.interner.resolve(anim.type_id);
                let coord = sim.anim_absolute_coord(id).unwrap();
                match native[1].as_str().unwrap() {
                    "PSIWARN" => {
                        assert_eq!(name, "PSIWARN");
                        let native_coord: Vec<i32> =
                            native[2].as_array().unwrap().iter().map(int).collect();
                        assert_eq!(vec![coord.x, coord.y, coord.z], native_coord, "{row}");
                        assert!(anim.draw_runtime.hidden, "{row}");
                        assert_eq!(anim.owner_house(), Some(owner));
                        assert_eq!(anim.attached_bullet(), Some(bullet_id));
                    }
                    "take_off" => {
                        assert_eq!(name, "NUKETO");
                        // At the bullet's launch point, as native's FLH result
                        // feeds both.
                        assert_eq!(native[2], fire[1], "{row}");
                        assert_eq!(
                            [coord.x, coord.y, coord.z],
                            [
                                bullet.launch_origin.x,
                                bullet.launch_origin.y,
                                bullet.launch_origin.z
                            ],
                            "{row}"
                        );
                        assert_eq!(anim.z_adjust, int(&row["take_off_z_adjust"]));
                    }
                    other => panic!("anim {other}"),
                }
                assert_eq!(anim.draw_flags, u32::try_from(int(&native[5])).unwrap());
            }
        }
    }

    /// A building that is not a silo only waits its mission rate: the
    /// oracle's row and the fixture's `[Missile] Rate=1` are one minute.
    #[test]
    fn a_building_that_is_not_a_silo_waits_the_mission_rate() {
        let oracle = oracle();
        let row = oracle["mission_missile"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["silo"] == false)
            .unwrap();
        assert!(row["events"].as_array().unwrap().is_empty());
        let rules = rules();
        let (mut sim, building) = world(&rules, "GAPILE", (50, 60), 0);
        sim.substrate
            .entities
            .get_mut(building)
            .unwrap()
            .mission
            .set_handler_state(0);
        assert_eq!(
            mission_missile(&mut sim, building, &rules),
            int(&row["delay"])
        );
        assert!(sim.projectiles.iter().next().is_none());
    }

    /// `BulletClass::Fire` keeps the target cell's vt+0x58 (`0x00468707`):
    /// over a structural bridge deck, 416 leptons above its ground.
    #[test]
    fn the_missile_aims_at_the_bridge_deck_of_its_target_cell() {
        let rules = rules();
        let target = (50, 60);
        let (mut sim, silo) = world(&rules, "NAMISL", target, 0);
        let grid = sim.resolved_terrain.as_mut().unwrap();
        grid.cell_mut(target.0, target.1)
            .unwrap()
            .bridge_facts
            .raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        sim.substrate
            .entities
            .get_mut(silo)
            .unwrap()
            .mission
            .set_handler_state(2);
        mission_missile(&mut sim, silo, &rules);
        let (_, bullet) = sim.projectiles.iter().next().unwrap();
        let ground = cell_ground_coord(sim.resolved_terrain.as_ref(), target.0, target.1);
        assert_eq!(
            [
                bullet.launch_target.x,
                bullet.launch_target.y,
                bullet.launch_target.z
            ],
            [ground.x, ground.y, ground.z + 416]
        );
    }
}
