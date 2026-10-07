//! Native comparisons for `BulletClass::NukeMaker @ 0x0046B310`
//! (`tools/superweapon_oracle.py` section `nuke_maker`; `--check`
//! regenerates it).

use super::nuke_maker;
use crate::map::resolved_terrain::{ResolvedTerrainCell, test_flat_cell, test_grid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::house_state::HouseState;
use crate::sim::projectile::{
    ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload, ProjectileTarget,
    ProjectileTrajectory,
};
use crate::sim::world::Simulation;
use serde_json::Value;

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

/// The carrier's BulletType rises to `altitude`; NukePayload deals
/// `damage` and is stored at `speed` (the fixture writes the field, as
/// native's does, past Process's speed postpass).
fn rules(altitude: i32, damage: i32, speed: i32) -> RuleSet {
    let text = format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=CARRIER\n[AircraftTypes]\n[BuildingTypes]\n\
         [CARRIER]\nStrength=100\nSpeed=4\nPrimary=NukeCarrier\nSecondary=NukePayload\n\
         [NukeCarrier]\nProjectile=GiantNukeUp\nSpeed=100\nWarhead=NukeMaker\n\
         [NukePayload]\nDamage={damage}\nProjectile=GiantNukeDown\nWarhead=NUKE\n\
         [GiantNukeUp]\nArm=2\nAcceleration=1\nVertical=yes\nDetonationAltitude={altitude}\n\
         [GiantNukeDown]\nArm=2\nAcceleration=1\nVertical=yes\nDetonationAltitude=30000\n\
         [Warheads]\n0=NukeMaker\n1=NUKE\n[NukeMaker]\nNukeMaker=yes\n[NUKE]\nCellSpread=1\n"
    );
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
    rules.set_weapon_speed_for_test("NukePayload", speed);
    rules
}

/// The carrier's detonation over the row's target: a cell target when the
/// target coordinate is a cell's, else a vehicle standing there. The
/// falling bullet matches the original's construction and Fire call.
#[test]
fn nuke_maker_matches_native() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let rows = oracle["nuke_maker"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    for row in rows {
        let rules = rules(
            int(&row["altitude"]),
            int(&row["payload_damage"]),
            int(&row["payload_speed"]),
        );
        let coords: Vec<i32> = row["target_coords"]
            .as_array()
            .unwrap()
            .iter()
            .map(int)
            .collect();
        let cell = (
            u16::try_from(coords[0] / 256).unwrap(),
            u16::try_from(coords[1] / 256).unwrap(),
        );
        let level = u8::try_from(int(&row["cell_level"])).unwrap();
        let mut sim = Simulation::with_seed(3);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Russians");
        sim.houses
            .insert(owner, HouseState::new(owner, 1, None, true, 0, 10));
        sim.session.house_order.push(owner);
        sim.resolved_terrain = Some(test_grid(80, 80, |x, y| ResolvedTerrainCell {
            level: if (x, y) == cell { level } else { 0 },
            ..test_flat_cell(x, y)
        }));
        let centre = coords[0] % 256 == 128 && coords[1] % 256 == 128;
        let target = if centre {
            ProjectileTarget::Cell {
                rx: cell.0,
                ry: cell.1,
            }
        } else {
            let id = 40;
            let mut entity = crate::sim::game_entity::GameEntity::test_default(
                id, "CARRIER", "Russians", cell.0, cell.1,
            );
            entity.owner = owner;
            entity.type_ref = sim.interner.intern("CARRIER");
            sim.substrate.entities.insert(entity);
            crate::sim::movement::ground_pose::put_location(
                &mut sim.substrate.entities.get_mut(id).unwrap().position,
                DriveCoord {
                    x: coords[0],
                    y: coords[1],
                    z: coords[2],
                },
            );
            ProjectileTarget::Entity(id)
        };
        let source_id = 77;
        let detonation = ProjectileDetonation {
            projectile_id: 76,
            source_id,
            target,
            impact: crate::sim::projectile::ProjectileCoord::new(coords[0], coords[1], coords[2]),
            payload: ProjectilePayload::new(
                0,
                sim.interner.intern("NukeMaker"),
                sim.interner.intern("NukeCarrier"),
            ),
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        nuke_maker(&mut sim, &rules, &detonation);
        let events = row["events"].as_array().unwrap();
        let construct = events.iter().find(|event| event[0] == "construct").unwrap();
        let fire = events.iter().find(|event| event[0] == "fire").unwrap();
        assert_eq!(
            construct[2], row["payload_speed"],
            "Construct takes the weapon's speed"
        );
        let bullets: Vec<_> = sim.projectiles.iter().collect();
        assert_eq!(bullets.len(), 1, "{row}");
        let (_, bullet) = bullets[0];
        let origin: Vec<i32> = fire[1].as_array().unwrap().iter().map(int).collect();
        assert_eq!(
            vec![
                bullet.launch_origin.x,
                bullet.launch_origin.y,
                bullet.launch_origin.z
            ],
            origin,
            "{row}"
        );
        let velocity: Vec<u64> = fire[2]
            .as_array()
            .unwrap()
            .iter()
            .map(|bits| bits.as_u64().unwrap())
            .collect();
        assert_eq!(
            bullet
                .velocity
                .native()
                .map(|component| component.bits())
                .to_vec(),
            velocity,
            "{row}"
        );
        assert_eq!(bullet.payload.base_damage, int(&construct[1]), "{row}");
        assert_eq!(sim.interner.resolve(bullet.payload.weapon), "NukePayload");
        assert_eq!(sim.interner.resolve(bullet.payload.warhead), "NUKE");
        assert_eq!(
            bullet.trajectory,
            ProjectileTrajectory::Vertical {
                detonation_altitude: 30000,
                acceleration: 1,
                max_speed: int(&construct[2]),
            },
            "{row}"
        );
        assert_eq!((bullet.target, bullet.source_id), (target, source_id));
    }
}
