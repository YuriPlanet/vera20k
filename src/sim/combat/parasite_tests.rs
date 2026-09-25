//! ParasiteClass through the live frame: LimboLaunch, the Parasite detonation
//! (AttachTo), the bite in the victim's own turn (ParasiteClass AI), and the
//! releases (PointerExpired, ExitUnit). Rust regression coverage; the fixture
//! copies retail `rulesmd.ini` values for every key these chains read.

use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::{
    AttackTarget, EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind,
};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use std::collections::BTreeMap;

const RULES: &str = "\
[General]\nRepairPercent=15%\nRepairStep=8\n\
[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\nHover=50%\nAmphibious=80%\nFloatBeach=0%\n\
Buildable=yes\n\
[CombatDamage]\nC4Warhead=Super\n\
[InfantryTypes]\n0=DOG\n1=E1\n\
[VehicleTypes]\n0=DRON\n1=MTNK\n2=DLPH\n3=MCV\n4=TRAN\n\
[AircraftTypes]\n[BuildingTypes]\n0=YARD\n\
[DOG]\nPrimary=BadTeeth\nStrength=100\nArmor=none\nSpeed=8\nSight=9\n\
ReselectIfLimboed=yes\nRejoinTeamIfLimboed=yes\nMovementZone=Infantry\n\
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
[E1]\nPrimary=M60\nStrength=125\nArmor=none\nSpeed=4\nSight=5\n\
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
[DRON]\nPrimary=DroneJump\nStrength=100\nSuppressionThreshold=5\nArmor=special_1\nSight=4\n\
ReselectIfLimboed=yes\nSpeed=10\nROT=40\nTurret=no\nCrusher=no\nCrewed=no\n\
Parasiteable=no\nMovementZone=Destroyer\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[MTNK]\nPrimary=105mm\nStrength=300\nArmor=heavy\nSpeed=7\nROT=5\nTurret=yes\nSight=8\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[DLPH]\nStrength=200\nArmor=light\nSpeed=8\nSight=4\nOrganic=yes\nNaval=yes\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[BadTeeth]\nDamage=30\nROF=30\nRange=1.5\nProjectile=DOGJUMP\nSpeed=30\n\
Warhead=ParasiteDog\nLimboLaunch=yes\n\
[DroneJump]\nDamage=50\nROF=60\nRange=1.83\nProjectile=JUMP\nSpeed=30\n\
Warhead=Parasite\nLimboLaunch=yes\n\
[M60]\nDamage=15\nROF=20\nRange=4\nProjectile=Invisible\nSpeed=100\nWarhead=SA\n\
[105mm]\nDamage=65\nROF=50\nRange=5.75\nProjectile=Invisible\nSpeed=100\nWarhead=AP\n\
[DOGJUMP]\nAA=no\nArm=2\nROT=8\nProximity=yes\nRanged=yes\nSubjectToCliffs=no\n\
SubjectToElevation=no\nSubjectToWalls=yes\n\
[JUMP]\nAA=no\nArm=2\nROT=8\nProximity=yes\nRanged=yes\nSubjectToCliffs=no\n\
SubjectToElevation=no\nSubjectToWalls=yes\n\
[Invisible]\nInviso=yes\n\
[Warheads]\n0=ParasiteDog\n1=Parasite\n2=SA\n3=AP\n4=Super\n5=SonicWarhead\n\
[ParasiteDog]\nVerses=100%,100%,100%,0%,0%,0%,0%,0%,0%,0%,0%\nParasite=yes\nInfDeath=1\nRocker=yes\n\
[Parasite]\nVerses=100%,100%,100%,100%,100%,100%,0%,0%,0%,0%,0%\nParasite=yes\nInfDeath=1\nRocker=yes\n\
[SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n\
[AP]\nVerses=25%,25%,25%,75%,100%,100%,65%,65%,35%,100%,100%\n\
[Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
[SonicWarhead]\nVerses=100%,100%,100%,100%,80%,80%,100%,60%,60%,100%,100%\nSonic=yes\n\
[MCV]\nDeploysInto=YARD\nStrength=1000\nArmor=heavy\nSpeed=4\nSight=4\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[YARD]\nFoundation=4x4\nConstructionYard=yes\nStrength=1000\nArmor=wood\n\
[TRAN]\nPassengers=3\nSizeLimit=100\nStrength=300\nArmor=heavy\nSpeed=6\nSight=4\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n";

fn rules() -> RuleSet {
    // Retail `[DRON] PrimaryFireFLH=`: a jump from ground height would
    // detonate on its first AI (`0x00466DB1`, old height <= 0).
    let mut rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("parasite fixture rules");
    rules.art_registry = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[DRON]\nPrimaryFireFLH=0,0,30\n",
    ));
    rules
}

struct Arena {
    sim: Simulation,
    grid: PathGrid,
}

impl Arena {
    /// A flat 32x32 clear map with its playfield, zones and path grid.
    fn new(rules: &RuleSet) -> Self {
        const SIZE: u16 = 32;
        let mut sim = Simulation::with_seed(7);
        sim.input_delay_ticks = 0;
        sim.session.map_width = SIZE;
        sim.session.map_height = SIZE;
        // Retail [Clear]: Foot/Track/Wheel 100%, Hover 50%, Amphibious 80%.
        let clear = crate::rules::terrain_rules::SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: None,
            amphibious: Some(80),
            float_beach: None,
            hover: Some(50),
        };
        let cell = |x, y| {
            let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
            cell.speed_costs = clear;
            cell.base_speed_costs = clear;
            cell
        };
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
                SIZE,
                SIZE,
                (0..SIZE)
                    .flat_map(|y| (0..SIZE).map(move |x| cell(x, y)))
                    .collect(),
            ),
        );
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 20,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        });
        sim.playfield_size_height = Some(20);
        assert!(sim.rebuild_dynamic_navigation(rules));
        let grid = sim
            .path_grid_snapshot()
            .map(|grid| (*grid).clone())
            .expect("navigation grid");
        Self { sim, grid }
    }

    fn spawn(&mut self, rules: &RuleSet, kind: &str, owner: &str, cell: (u16, u16)) -> u64 {
        self.sim
            .spawn_object_at_height(kind, owner, cell.0, cell.1, 0, 0, rules)
            .unwrap_or_else(|| panic!("spawn {kind}"))
    }

    fn attack(&mut self, attacker: u64, target: u64) {
        let owner = self.sim.substrate.entities.get(attacker).unwrap().owner();
        let tick = self.sim.session.tick + 1;
        self.sim.queue_command(CommandEnvelope::new(
            owner,
            tick,
            Command::Attack {
                attacker_id: attacker,
                target_id: target,
            },
        ));
    }

    fn step(&mut self, rules: &RuleSet) {
        let commands = self.sim.take_due_commands();
        let _ = self.sim.advance_tick(
            &commands,
            Some(rules),
            &BTreeMap::new(),
            Some(&self.grid),
            None,
            33,
        );
    }

    fn frame(&self) -> u32 {
        self.sim.session.binary_frame
    }

    /// Step until `done` holds; panics after `limit` frames.
    fn until(&mut self, rules: &RuleSet, limit: u32, mut done: impl FnMut(&Simulation) -> bool) {
        for _ in 0..limit {
            if done(&self.sim) {
                return;
            }
            self.step(rules);
        }
        assert!(done(&self.sim), "condition not reached in {limit} frames");
    }

    fn eater_of(&self, victim: u64) -> Option<u64> {
        self.sim
            .substrate
            .entities
            .get(victim)
            .and_then(|v| v.parasite_eating_me)
    }

    fn in_limbo(&self, id: u64) -> bool {
        self.sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| e.lifecycle.in_limbo)
    }

    fn health(&self, id: u64) -> Option<i32> {
        self.sim
            .substrate
            .entities
            .get(id)
            .map(|e| e.health.current)
    }

    /// Retired by UnInit: pending delete, or already drained.
    fn gone(&self, id: u64) -> bool {
        self.sim
            .substrate
            .entities
            .get(id)
            .is_none_or(|e| !e.lifecycle.object_alive)
    }

    /// One direct ReceiveDamage call at distance zero.
    fn hit(
        &mut self,
        rules: &RuleSet,
        victim: u64,
        source: Option<u64>,
        damage: i32,
        warhead: &str,
    ) {
        let warhead = self.sim.interner.intern(warhead);
        let house = source
            .and_then(|id| self.sim.substrate.entities.get(id))
            .map(|e| e.owner());
        let event = EntityDamageEvent::direct_receiver(
            victim,
            damage,
            0,
            source.unwrap_or(RAD_NO_ATTACKER),
            house,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        self.sim.commit_direct_damage_receiver(rules, None, event);
    }

    fn cell(&self, id: u64) -> (u16, u16) {
        let e = self.sim.substrate.entities.get(id).unwrap();
        (e.position.rx, e.position.ry)
    }

    /// Infect `victim` through a real launch and detonation.
    fn infect(&mut self, rules: &RuleSet, owner: u64, victim: u64) {
        self.attack(owner, victim);
        self.until(rules, 200, |sim| {
            sim.substrate
                .entities
                .get(victim)
                .is_some_and(|v| v.parasite_eating_me == Some(owner))
        });
    }
}

#[test]
fn constructor_allocates_a_parasite_only_for_weapon_zero_parasite_types() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let dog = arena.spawn(&rules, "DOG", "Russians", (5, 5));
    let drone = arena.spawn(&rules, "DRON", "Russians", (6, 5));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (7, 5));
    let entities = &arena.sim.substrate.entities;
    assert!(entities.get(dog).unwrap().parasite.is_some());
    assert!(entities.get(drone).unwrap().parasite.is_some());
    assert!(entities.get(tank).unwrap().parasite.is_none());
}

#[test]
fn dog_bite_kills_the_infantry_and_releases_the_dog_where_it_stood() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let dog = arena.spawn(&rules, "DOG", "Russians", (10, 10));
    let gi = arena.spawn(&rules, "E1", "Americans", (11, 10));
    arena.attack(dog, gi);

    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| d.lifecycle.in_limbo)
    });
    // TechnoClass::Fire LimboLaunch: the dog rides its bullet.
    assert!(arena.in_limbo(dog));

    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(gi)
            .is_none_or(|v| v.health.current == 0)
    });
    let death_frame = arena.frame();

    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| !d.lifecycle.in_limbo)
    });
    let dog_entity = arena.sim.substrate.entities.get(dog).unwrap();
    assert!(dog_entity.health.current > 0, "the dog survives its kill");
    assert_eq!(
        (dog_entity.position.rx, dog_entity.position.ry),
        (11, 10),
        "PointerExpired unlimboes the dog at the victim's cell"
    );
    assert!(
        dog_entity.parasite.as_deref().unwrap().victim().is_none(),
        "the ParasiteClass forgets its victim"
    );
    assert!(
        !dog_entity.is_paralyzed(arena.frame()),
        "PointerExpired release does not paralyze"
    );
    // ObjectClass::ReceiveDamage's exact-zero Destroy (0x005F57AF) runs
    // Detach_All(1) inside the killing bite, so the dog is out on that frame.
    assert_eq!(arena.frame(), death_frame);
}

#[test]
fn drone_bites_on_its_weapon_rate_until_the_host_dies_then_drops_off() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    arena.infect(&rules, drone, tank);
    assert!(arena.in_limbo(drone));

    let mut bites = Vec::new();
    let mut last = arena.health(tank).unwrap();
    arena.until(&rules, 1000, |sim| {
        let hp = sim
            .substrate
            .entities
            .get(tank)
            .map_or(0, |t| t.health.current);
        if hp != last {
            bites.push((sim.session.binary_frame, hp));
            last = hp;
        }
        hp == 0
    });
    // 50 per bite against Strength 300: six bites, one ROF (60) apart.
    assert_eq!(
        bites.iter().map(|&(_, hp)| hp).collect::<Vec<_>>(),
        [250, 200, 150, 100, 50, 0]
    );
    assert!(
        bites.windows(2).all(|pair| pair[1].0 - pair[0].0 == 60),
        "bite cadence {bites:?}"
    );

    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(drone)
            .is_some_and(|d| !d.lifecycle.in_limbo)
    });
    let drone_entity = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(drone_entity.health.current > 0);
    assert_eq!(
        (drone_entity.position.rx, drone_entity.position.ry),
        (11, 10)
    );
    assert_eq!(drone_entity.category, EntityCategory::Unit);
}

#[test]
fn third_party_fire_above_the_threshold_makes_the_drone_die_with_its_host() {
    // FootClass::ReceiveDamage 0x004D7374..0x004D73D1 arms `2*damage - 5`
    // (DRON SuppressionThreshold) of suppression; the host's PointerExpired
    // 0x0062A2A3..0x0062A2E2 then UnInits the owner instead of releasing it.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let rhino = arena.spawn(&rules, "MTNK", "Russians", (14, 10));
    arena.infect(&rules, drone, tank);
    arena.hit(&rules, tank, Some(rhino), 400, "AP");
    let suppression_end = arena.frame() + 2 * 400 - 5;
    arena.until(&rules, 60, |sim| sim.substrate.entities.get(tank).is_none());
    assert!(arena.frame() < suppression_end);
    assert!(
        arena.gone(drone),
        "a suppressed drone is deleted with its host"
    );
}

#[test]
fn a_heal_forces_the_drone_off_and_its_running_suppression_kills_it() {
    // 0x004D73D4..0x004D740E: negative damage arms 50 frames of suppression and
    // calls ExitUnit, whose suppressed non-Naval arm (0x0062A89B) kills the owner.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    arena.infect(&rules, drone, tank);
    arena.hit(&rules, tank, None, -50, "SA");
    assert!(arena.gone(drone));
    assert_eq!(arena.eater_of(tank), None);
    assert!(arena.health(tank).is_some_and(|hp| hp > 0));
}

#[test]
fn a_sonic_hit_ejects_the_drone_alive_beside_its_host_and_drops_the_shooter_target() {
    // 0x004D734F..0x004D7371: ExitUnit with no suppression, then the source's
    // Assign_Target(NULL). A successful exit unlimboes the owner facing 90
    // degrees off the host and paralyzes it for 3 x ROF (0x0062A7AE..0x0062A7DD).
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let dolphin = arena.spawn(&rules, "DLPH", "Americans", (14, 10));
    arena.infect(&rules, drone, tank);
    arena
        .sim
        .substrate
        .entities
        .get_mut(dolphin)
        .unwrap()
        .attack_target = Some(AttackTarget::new(tank));
    arena
        .sim
        .substrate
        .entities
        .get_mut(drone)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(3, 3)));
    let host_facing = arena.sim.substrate.entities.get(tank).unwrap().facing;
    arena.hit(&rules, tank, Some(dolphin), 4, "SonicWarhead");

    let frame = arena.frame();
    let released = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(!released.lifecycle.in_limbo && released.lifecycle.object_alive);
    assert_eq!(released.health.current, 100);
    assert_eq!(host_facing, 0);
    assert_eq!(released.facing, 64, "north host: released facing east");
    assert_eq!(released.paralysis_timer.remaining(frame as i32), 3 * 60);
    // ExitUnit 0x0062A771: Set_ArchiveTarget(NULL).
    assert_eq!(released.archive_target(), None);
    let (rx, ry) = arena.cell(drone);
    assert!(rx.abs_diff(11) <= 1 && ry.abs_diff(10) <= 1 && (rx, ry) != (11, 10));
    assert_eq!(arena.eater_of(tank), None);
    let host = arena.sim.substrate.entities.get(tank).unwrap();
    assert!(!host.is_paralyzed(frame));
    assert!(
        arena
            .sim
            .substrate
            .entities
            .get(dolphin)
            .unwrap()
            .attack_target
            .is_none()
    );
}

#[test]
fn iron_curtain_strips_the_drone_and_kills_an_organic_vehicle() {
    // FootClass::IronCurtain 0x004DEAE0: Organic types take C4Warhead damage of
    // their Strength instead of the curtain; others force the parasite off
    // (suppression 50, ExitUnit) before TechnoClass::IronCurtain.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let dolphin = arena.spawn(&rules, "DLPH", "Americans", (12, 10));
    arena.infect(&rules, drone, tank);
    let owner = arena.sim.interner.intern("Americans");
    let sw = arena.sim.interner.intern("IronCurtainSpecial");
    crate::sim::superweapon::iron_curtain::launch(&mut arena.sim, &rules, owner, 11, 10, sw, None);

    let frame = arena.frame();
    assert!(arena.gone(drone));
    assert_eq!(arena.eater_of(tank), None);
    assert!(crate::sim::superweapon::invulnerability::is_invulnerable(
        arena
            .sim
            .substrate
            .entities
            .get(tank)
            .unwrap()
            .invulnerability
            .as_ref(),
        frame
    ));
    assert_eq!(arena.health(dolphin), Some(0));
    assert!(
        arena
            .sim
            .substrate
            .entities
            .get(dolphin)
            .unwrap()
            .invulnerability
            .is_none()
    );
}

#[test]
fn a_teleport_warp_ejects_the_drone_before_the_host_relocates() {
    // TeleportLocomotion 0x007195BF..0x007195CF: ExitUnit (no suppression)
    // precedes SetCoords, so the drone lands at the pre-warp spot, paralyzed.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    arena.infect(&rules, drone, tank);
    arena
        .sim
        .substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .teleport_state = Some(crate::sim::movement::teleport_movement::TeleportState {
        phase: crate::sim::movement::teleport_movement::TeleportPhase::Relocate,
        target_rx: 20,
        target_ry: 20,
        being_warped_ticks: 0,
    });
    arena.step(&rules);

    assert_eq!(arena.cell(tank), (20, 20));
    assert_eq!(arena.eater_of(tank), None);
    let released = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(!released.lifecycle.in_limbo && released.lifecycle.object_alive);
    assert!(released.is_paralyzed(arena.frame()));
    let (rx, ry) = arena.cell(drone);
    assert!(rx.abs_diff(11) <= 1 && ry.abs_diff(10) <= 1);
}

#[test]
fn a_host_lost_in_flight_returns_the_drone_to_its_launch_cell() {
    // AttachTo 0x0062AA02..0x0062AAD6: CanInfect fails, so the owner unlimboes
    // at the centre of its last cell (Foot+55C), facing 0, unparalyzed.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let rhino = arena.spawn(&rules, "MTNK", "Russians", (14, 10));
    let anchor = Some(TargetKind::Cell(3, 3));
    arena
        .sim
        .substrate
        .entities
        .get_mut(drone)
        .unwrap()
        .set_archive_target(anchor);
    arena.attack(drone, tank);
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(drone)
            .is_some_and(|d| d.lifecycle.in_limbo)
    });
    arena.hit(&rules, tank, Some(rhino), 400, "AP");
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(drone)
            .is_some_and(|d| !d.lifecycle.in_limbo)
    });
    let returned = arena.sim.substrate.entities.get(drone).unwrap();
    assert_eq!(arena.cell(drone), (10, 10));
    assert_eq!(returned.facing, 0);
    assert!(!returned.is_paralyzed(arena.frame()));
    assert!(returned.parasite.as_deref().unwrap().victim().is_none());
    // The refusal (0x0062AA96..0x0062AAC9) never calls Set_ArchiveTarget.
    assert_eq!(returned.archive_target(), anchor);
}

#[test]
fn one_parasite_per_host_the_second_drone_never_jumps() {
    // TechnoClass::Fire 0x006FF81F locks the host for 0x14 frames; GetFireError
    // 0x006FCAE1 holds the second shot, then CanInfect's one-parasite rule
    // (0x0062A90B) makes it illegal once the first has attached.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let first = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let second = arena.spawn(&rules, "DRON", "Russians", (12, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    arena.attack(first, tank);
    arena.attack(second, tank);
    let mut launched = std::collections::BTreeSet::new();
    for _ in 0..150 {
        arena.step(&rules);
        for drone in [first, second] {
            if arena.in_limbo(drone) {
                launched.insert(drone);
            }
        }
    }
    let eater = arena.eater_of(tank).expect("one drone attached");
    assert_eq!(launched.into_iter().collect::<Vec<_>>(), [eater]);
}

#[test]
fn an_infected_unit_cannot_load_bunker_or_deploy() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let transport = arena.spawn(&rules, "TRAN", "Americans", (13, 10));
    let mcv = arena.spawn(&rules, "MCV", "Americans", (11, 14));
    let can_load = |sim: &Simulation| {
        let (passenger, carrier) = (
            sim.substrate.entities.get(tank).unwrap(),
            sim.substrate.entities.get(transport).unwrap(),
        );
        crate::sim::passenger::can_enter_transport(
            passenger,
            carrier,
            rules.object("MTNK").unwrap(),
            rules.object("TRAN").unwrap(),
            &crate::sim::passenger::PassengerCargo::new(3, 100),
            &rules,
            &sim.houses,
            None,
        )
    };
    assert!(can_load(&arena.sim));
    assert!(crate::sim::docking::bunker_link::can_auto_deploy_here(
        &arena.sim, tank, &rules
    ));
    arena.infect(&rules, drone, tank);
    // UnitClass::Receive_Radio 0x0F 0x007375FC; CanEnterBunker 0x0070FBB9.
    assert!(!can_load(&arena.sim));
    assert!(!crate::sim::docking::bunker_link::can_auto_deploy_here(
        &arena.sim, tank, &rules
    ));

    // CanDeploySlashUnload 0x00700EB4 refuses a DeploysInto unit; UnitClass::
    // Deploy then clears Unit+0x68C (0x00739AA7).
    let second = arena.spawn(&rules, "DRON", "Russians", (12, 14));
    arena.infect(&rules, second, mcv);
    arena
        .sim
        .substrate
        .entities
        .get_mut(mcv)
        .unwrap()
        .mcv_deploy_pending = true;
    assert!(!arena.sim.deploy_mcv(mcv, &rules, &BTreeMap::new()));
    assert!(
        !arena
            .sim
            .substrate
            .entities
            .get(mcv)
            .unwrap()
            .mcv_deploy_pending
    );
}

#[test]
fn a_load_keeps_the_infection_and_restarts_both_parasite_timers() {
    // ParasiteClass::Load 0x006295DB..0x006295F3 restarts the suppression and
    // bite timers at the load frame with zero duration: the restored drone
    // bites on the host's next turn and no longer dies with it.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let rhino = arena.spawn(&rules, "MTNK", "Russians", (14, 10));
    arena.infect(&rules, drone, tank);
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(tank)
            .is_some_and(|t| t.health.current == 250)
    });
    // Third-party AP (100% against heavy): 20 raw arms 35 frames of suppression.
    arena.hit(&rules, tank, Some(rhino), 20, "AP");
    assert_eq!(arena.health(tank), Some(230));

    let saved = crate::sim::snapshot::GameSnapshot::save(&arena.sim, 0, 0, "parasite", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    let mut copy = Arena {
        sim: restored,
        grid: arena.grid.clone(),
    };
    for id in [drone, tank] {
        let (left, right) = (
            arena.sim.substrate.entities.get(id).unwrap(),
            copy.sim.substrate.entities.get(id).unwrap(),
        );
        assert_eq!(left.parasite_eating_me, right.parasite_eating_me);
        assert_eq!(left.parasite_launch_lock, right.parasite_launch_lock);
        assert_eq!(left.paralysis_timer, right.paralysis_timer);
        assert_eq!(left.lifecycle.in_limbo, right.lifecycle.in_limbo);
    }

    arena.step(&rules);
    copy.step(&rules);
    assert_eq!(
        arena.health(tank),
        Some(230),
        "the saved bite timer still runs"
    );
    assert_eq!(
        copy.health(tank),
        Some(180),
        "the loaded bite is due at once"
    );

    // Killing the host while the original is still suppressed deletes its
    // drone; the loaded one has no suppression left and drops off alive. The
    // killing hit stays at the threshold (5 raw) so it arms nothing itself.
    for world in [&mut arena, &mut copy] {
        world
            .sim
            .substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .health
            .current = 5;
        world.hit(&rules, tank, Some(rhino), 5, "AP");
        world.until(&rules, 60, |sim| sim.substrate.entities.get(tank).is_none());
    }
    assert!(arena.gone(drone));
    assert!(!copy.gone(drone) && !copy.in_limbo(drone));
}

#[test]
fn the_reselect_memo_stays_out_of_the_peer_hash() {
    // TechnoClass::Fire 0x006FF763..0x006FF79C writes Techno+432 only for the
    // local player's selected firer; two peers must still hash alike.
    let rules = rules();
    let run = |local: &str, selected: bool| {
        let mut arena = Arena::new(&rules);
        let dog = arena.spawn(&rules, "DOG", "Russians", (10, 10));
        let gi = arena.spawn(&rules, "E1", "Americans", (11, 10));
        arena.sim.session.current_house = Some(arena.sim.interner.intern(local));
        arena.sim.substrate.entities.get_mut(dog).unwrap().selected = selected;
        arena.attack(dog, gi);
        arena.until(&rules, 100, |sim| {
            sim.substrate
                .entities
                .get(dog)
                .is_some_and(|d| d.lifecycle.in_limbo)
        });
        let memo = arena
            .sim
            .substrate
            .entities
            .get(dog)
            .unwrap()
            .limbo_reselect;
        (memo, arena.sim.state_hash())
    };
    let (local_memo, local_hash) = run("Russians", true);
    let (peer_memo, peer_hash) = run("Americans", false);
    assert!(local_memo && !peer_memo);
    assert_eq!(local_hash, peer_hash);
}

#[test]
fn a_dog_released_on_a_bridge_deck_stays_on_the_deck() {
    // GetReleaseCoords 0x0062AC60..0x0062ACF6: the victim's own location, deck
    // height included, and Owner OnBridge = Victim OnBridge; Unlimbo keeps it.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let dog = arena.spawn(&rules, "DOG", "Russians", (10, 10));
    let gi = arena.spawn(&rules, "E1", "Americans", (11, 10));
    // Both stand on a deck four levels up.
    let deck = 4 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
    for id in [dog, gi] {
        let entity = arena.sim.substrate.entities.get_mut(id).unwrap();
        entity.on_bridge = true;
        entity.position.z = 4;
        entity.position.exact_z_leptons = Some(deck);
    }
    arena.attack(dog, gi);
    arena.until(&rules, 60, |sim| {
        sim.substrate
            .entities
            .get(gi)
            .is_none_or(|v| v.health.current == 0)
    });
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| !d.lifecycle.in_limbo)
    });
    let released = arena.sim.substrate.entities.get(dog).unwrap();
    assert!(released.on_bridge);
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&released.position).z,
        deck
    );
}

#[test]
fn a_parasite_order_on_an_iron_curtained_host_is_dropped() {
    // GetFireError 0x006FCAFA..0x006FCB21: an Iron-Curtained Foot is FIRE_ILLEGAL
    // for a Parasite warhead, so the drone gives the order up rather than
    // waiting beside the host for the curtain to fall.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let frame = arena.frame();
    crate::sim::superweapon::invulnerability::apply_invulnerability(
        arena.sim.substrate.entities.get_mut(tank).unwrap(),
        frame,
        750,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    arena.attack(drone, tank);
    arena.until(&rules, 40, |sim| {
        sim.substrate
            .entities
            .get(drone)
            .is_some_and(|d| d.attack_target.is_none() && sim.session.tick > 2)
    });
    assert!(!arena.in_limbo(drone));
    assert_eq!(arena.eater_of(tank), None);
}
