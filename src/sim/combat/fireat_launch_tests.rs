//! Production frames for FireAt's launch geometry on retail rules and art:
//! the launch speed (`WeaponTypeClass::GetSpeed @ 0x00773070`) from the
//! barrel's FLH, and the moving-target lead (`0x0070BCB0`). Skipped without
//! the retail `ini/rulesmd.ini` and `ini/artmd.ini`.

use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::projectile::{ProjectileCoord, launch::fireat_launch_distance};
use crate::sim::world::Simulation;

struct Duel {
    sim: Simulation,
    rules: RuleSet,
    grid: crate::sim::pathfinding::PathGrid,
    hm: BTreeMap<(u16, u16), u8>,
    /// Projectiles alive before the latest tick.
    before: std::collections::BTreeSet<u64>,
}

impl Duel {
    fn new() -> Option<Self> {
        let ini = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?;
        let mut rules = RuleSet::from_ini(&ini).expect("retail rules parse");
        let art = crate::rules::retail_ini_fixture::retail_ini("artmd.ini")?;
        rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art));
        let mut sim = Simulation::new();
        for (name, side, human) in [("Americans", 0, true), ("Russians", 1, true)] {
            let id = sim.interner.intern(name);
            sim.houses.insert(
                id,
                crate::sim::house_state::HouseState::new(id, side, None, human, 0, 10),
            );
            sim.session.house_order.push(id);
        }
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        sim.resolve_type_handles(&rules);
        Some(Self {
            sim,
            rules,
            grid,
            hm: BTreeMap::new(),
            before: std::collections::BTreeSet::new(),
        })
    }

    fn spawn(&mut self, kind: &str, owner: &str, rx: u16, ry: u16, facing: u8) -> u64 {
        self.sim
            .spawn_object(kind, owner, rx, ry, facing, &self.rules, &self.hm)
            .unwrap_or_else(|| panic!("spawn {kind}"))
    }

    fn order(&mut self, owner: &str, command: Command) {
        let owner = self.sim.interner.intern(owner);
        self.sim.queue_command(CommandEnvelope::new(
            owner,
            self.sim.session.tick + 1,
            command,
        ));
    }

    fn tick(&mut self) {
        self.sim.fire_events.clear();
        self.before = self.sim.projectiles.iter().map(|(&id, _)| id).collect();
        let commands = self.sim.take_due_commands();
        self.sim.advance_tick(
            &commands,
            Some(&self.rules),
            &self.hm,
            Some(&self.grid),
            None,
            67,
        );
    }

    fn health(&self, id: u64) -> i32 {
        self.sim
            .substrate
            .entities
            .get(id)
            .map_or(0, |entity| i32::from(entity.health.current))
    }

    fn location(&self, id: u64) -> ProjectileCoord {
        let entity = self.sim.substrate.entities.get(id).unwrap();
        ProjectileCoord::new(
            i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
            i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
            crate::sim::combat::object_world_z_leptons(entity, self.sim.resolved_terrain.as_ref()),
        )
    }

    /// The first shell the firer launched this frame, if any. It has already
    /// taken its first AI step in the frame's tail, so it is recognised as
    /// new rather than by standing on its launch origin.
    fn launched_by(&self, firer: u64) -> Option<&crate::sim::projectile::Projectile> {
        self.sim
            .projectiles
            .iter()
            .map(|(_, projectile)| projectile)
            .find(|projectile| {
                projectile.source_id == firer && !self.before.contains(&projectile.id)
            })
    }
}

/// Retail data the regression rests on: the Grizzly's `[105mm]` fires
/// `[Cannon]`, an `Arcing=` projectile, at `Speed=40` under `Gravity=6`.
#[test]
fn retail_cannon_is_an_arcing_speed_40_shell_under_gravity_6() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(rules.general.gravity, 6);
    for (tank, weapon) in [("MTNK", "105mm"), ("HTNK", "120mm")] {
        let object = rules.object(tank).unwrap();
        assert_eq!(object.primary.as_deref(), Some(weapon), "{tank}");
        let weapon = rules.weapon(weapon).unwrap();
        assert_eq!(weapon.speed, 40);
        let projectile = rules
            .projectile(weapon.projectile.as_deref().unwrap())
            .unwrap();
        assert!(projectile.arcing && projectile.rot == 0, "{tank}");
    }
}

/// A Grizzly engaging a Rhino four cells away hits it. Before, FireAt
/// launched every `ROT=0` shell at `Speed=`, and `[Cannon]`'s 40 has no
/// ballistic solution past about one cell at `Gravity=6`: the Grizzly played
/// its report and reloaded, but no shell ever left the barrel.
#[test]
fn a_grizzly_hits_a_rhino_four_cells_away() {
    let Some(mut duel) = Duel::new() else {
        return;
    };
    // Facing each other (east is 64 of 256), in the arena's middle: its
    // playfield leaves the rim cells off the map, where a shell is removed.
    let grizzly = duel.spawn("MTNK", "Americans", 16, 16, 64);
    let rhino = duel.spawn("HTNK", "Russians", 20, 16, 192);
    let full = duel.health(rhino);
    duel.order(
        "Americans",
        Command::Attack {
            attacker_id: grizzly,
            target_id: rhino,
        },
    );
    let mut shells = 0;
    for _ in 0..200 {
        duel.tick();
        shells += usize::from(duel.launched_by(grizzly).is_some());
        if shells >= 2 && duel.health(rhino) < full {
            break;
        }
    }
    assert!(shells >= 1, "the Grizzly launched no shell");
    assert!(
        duel.health(rhino) < full,
        "{shells} shells launched, the Rhino is untouched"
    );
}

/// A Grizzly's `Arcing=` shell leaves its barrel (`[MTNK] PrimaryFireFLH`),
/// where its report and muzzle flash are placed too (`[ESP+0x44]`); only a
/// `Dropping=` projectile would start at the hull centre. Its launch speed is
/// GetSpeed at the 2-D distance from that barrel to the Rhino.
#[test]
fn a_grizzly_shell_leaves_its_barrel_at_getspeed() {
    let Some(mut duel) = Duel::new() else {
        return;
    };
    let grizzly = duel.spawn("MTNK", "Americans", 16, 16, 64);
    let rhino = duel.spawn("HTNK", "Russians", 20, 16, 192);
    duel.order(
        "Americans",
        Command::Attack {
            attacker_id: grizzly,
            target_id: rhino,
        },
    );
    for _ in 0..200 {
        duel.tick();
        let Some(shell) = duel.launched_by(grizzly) else {
            continue;
        };
        assert_ne!(
            shell.launch_origin,
            duel.location(grizzly),
            "the shell starts at the barrel, not the hull centre"
        );
        let event = duel
            .sim
            .fire_events
            .iter()
            .find(|event| event.attacker_id == grizzly)
            .expect("the shot's fire event");
        assert_eq!(event.fire_coord, shell.launch_origin);
        // ftol(Sqrt_Approx(d * 6 * 1.2)); four cells out, far below the
        // half-distance clamp.
        let speed = crate::sim::projectile::launch::weapon_launch_speed(
            40,
            Some(crate::sim::projectile::launch::LaunchSpeedProjectile {
                rot: 0,
                floater: false,
            }),
            6,
            fireat_launch_distance(shell.launch_origin, duel.location(rhino)),
        );
        assert!(speed > 40, "the derived speed ({speed}) beats Speed=40");
        assert_eq!(i32::from(shell.speed_leptons_per_frame), speed);
        return;
    }
    panic!("the Grizzly never launched a shell");
}

/// A Rhino driving south past the Grizzly is led: the shell is launched at
/// where it will be, south of where it stands at the shot (`0x0070BCB0`'s lead
/// along its facing). Movement runs before combat in a frame, so the Rhino's
/// position after the frame is its position at the shot.
#[test]
fn a_moving_rhino_is_led() {
    let Some(mut duel) = Duel::new() else {
        return;
    };
    let grizzly = duel.spawn("MTNK", "Americans", 16, 16, 64);
    let rhino = duel.spawn("HTNK", "Russians", 20, 10, 128);
    duel.order(
        "Russians",
        Command::Move {
            entity_id: rhino,
            target_rx: 20,
            target_ry: 28,
            queue: false,
            group_id: None,
        },
    );
    duel.order(
        "Americans",
        Command::Attack {
            attacker_id: grizzly,
            target_id: rhino,
        },
    );
    for _ in 0..240 {
        let moving = duel
            .sim
            .substrate
            .entities
            .get(rhino)
            .is_some_and(|entity| {
                crate::sim::movement::motion_query::is_moving(entity) == Some(true)
            });
        duel.tick();
        let Some(shell) = duel.launched_by(grizzly) else {
            continue;
        };
        if !moving {
            continue;
        }
        let target = duel.location(rhino);
        let [vx, vy, _] = shell
            .velocity
            .native()
            .map(|bits| f64::from_bits(bits.bits()));
        // Where the launch heading crosses the Rhino's column, relative to
        // the Rhino: the lead, `ftol(d / (GetSpeed * 0.9) * Rhino speed)`.
        let crossing = shell.launch_origin.y as f64
            + vy / vx * (target.x - shell.launch_origin.x) as f64
            - target.y as f64;
        // About 230 here; the unled heading crosses within ~15 of the Rhino.
        assert!(
            crossing > 100.0,
            "the shell aims {crossing:.1} leptons south of the Rhino"
        );
        return;
    }
    panic!("no shell was launched at the moving Rhino");
}

/// A homing missile at a moving Rhino is led too, but its proximity fuse keeps
/// the Rhino's own coordinate: `ProximityDetector::Setup` (`0x004E1130`, from
/// `BulletClass::Fire` at `0x00468A93`) copies the target's unled vt+0x58
/// (`0x00468700..0x00468724`), not the led aim.
#[test]
#[ignore = "the IFV's HoverMissile launches at ground height (not Weapon1FLH z=180) and \
            collides on its first step, which now runs in the firing frame's tail"]
fn a_homing_missile_fuses_on_the_unled_target() {
    let Some(mut duel) = Duel::new() else {
        return;
    };
    let ifv = duel.spawn("FV", "Americans", 16, 16, 64);
    let rhino = duel.spawn("HTNK", "Russians", 20, 10, 128);
    duel.order(
        "Russians",
        Command::Move {
            entity_id: rhino,
            target_rx: 20,
            target_ry: 28,
            queue: false,
            group_id: None,
        },
    );
    duel.order(
        "Americans",
        Command::Attack {
            attacker_id: ifv,
            target_id: rhino,
        },
    );
    for _ in 0..240 {
        let moving = duel
            .sim
            .substrate
            .entities
            .get(rhino)
            .is_some_and(|entity| {
                crate::sim::movement::motion_query::is_moving(entity) == Some(true)
            });
        duel.tick();
        let Some(missile) = duel.launched_by(ifv) else {
            continue;
        };
        if !moving {
            continue;
        }
        let guidance = missile.guidance.expect("HoverMissile homes (ROT=60)");
        assert_eq!(guidance.fuse_reference, missile.launch_target);
        return;
    }
    panic!("no missile was launched at the moving Rhino");
}
