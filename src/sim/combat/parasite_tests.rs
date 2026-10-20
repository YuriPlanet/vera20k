//! ParasiteClass through the live frame: LimboLaunch, the Parasite detonation
//! (AttachTo), the bite in the victim's own turn (ParasiteClass AI), and the
//! releases (PointerExpired, ExitUnit). Rust regression coverage; the fixture
//! copies retail `rulesmd.ini` values for every key these chains read.

use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

const RULES: &str = "\
[General]\nRepairPercent=15%\nRepairStep=8\n\
[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\nHover=50%\nAmphibious=80%\nFloatBeach=0%\n\
Buildable=yes\n\
[Tiberium]\nFoot=90%\nTrack=70%\nWheel=50%\nFloat=0%\nFloatBeach=0%\nHover=50%\nAmphibious=50%\n\
Buildable=no\n\
[CombatDamage]\nC4Warhead=Super\nIronCurtainDuration=750\n\
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
    rules_with_ini(&IniFile::from_str(RULES))
}

fn rules_with_ini(ini: &IniFile) -> RuleSet {
    // Retail `[DRON] PrimaryFireFLH=`: a jump from ground height would
    // detonate on its first AI (`0x00466DB1`, old height <= 0).
    let mut rules = RuleSet::from_ini(ini).expect("parasite fixture rules");
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[DRON]\nPrimaryFireFLH=0,0,30\n"),
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
        sim.session.set_map_width_for_test(SIZE);
        sim.session.set_map_height_for_test(SIZE);
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
        let tick = self.sim.session.tick() + 1;
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
        let _ = self
            .sim
            .advance_tick(&commands, Some(rules), Some(&self.grid), None, 33);
    }

    fn frame(&self) -> u32 {
        self.sim.session.binary_frame()
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
            .and_then(|v| v.parasite.eater)
    }

    fn in_limbo(&self, id: u64) -> bool {
        self.sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| e.lifecycle.in_limbo())
    }

    fn health(&self, id: u64) -> Option<i32> {
        self.sim
            .substrate
            .entities
            .get(id)
            .map(|e| e.health.current())
    }

    /// Retired by UnInit: pending delete, or already drained.
    fn gone(&self, id: u64) -> bool {
        self.sim
            .substrate
            .entities
            .get(id)
            .is_none_or(|e| !e.lifecycle.object_alive())
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
        self.sim.commit_direct_damage_receiver(
            rules,
            None,
            event,
            crate::sim::world::FrameEffects::default(),
        );
    }

    fn cell(&self, id: u64) -> (u16, u16) {
        let e = self.sim.substrate.entities.get(id).unwrap();
        (e.position.rx(), e.position.ry())
    }

    /// Infect `victim` through a real launch and detonation.
    fn infect(&mut self, rules: &RuleSet, owner: u64, victim: u64) {
        self.attack(owner, victim);
        self.until(rules, 200, |sim| {
            sim.substrate
                .entities
                .get(victim)
                .is_some_and(|v| v.parasite.eater == Some(owner))
        });
    }
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
            .is_some_and(|d| d.lifecycle.in_limbo())
    });
    // TechnoClass::Fire LimboLaunch: the dog rides its bullet.
    assert!(arena.in_limbo(dog));

    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(gi)
            .is_none_or(|v| v.health.current() == 0)
    });
    let death_frame = arena.frame();

    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| !d.lifecycle.in_limbo())
    });
    let dog_entity = arena.sim.substrate.entities.get(dog).unwrap();
    assert!(dog_entity.health.current() > 0, "the dog survives its kill");
    assert_eq!(
        (dog_entity.position.rx(), dog_entity.position.ry()),
        (11, 10),
        "PointerExpired unlimboes the dog at the victim's cell"
    );
    assert!(
        dog_entity.parasite.class().unwrap().victim.is_none(),
        "the ParasiteClass forgets its victim"
    );
    assert!(
        !dog_entity.parasite.is_paralyzed(arena.frame()),
        "PointerExpired release does not paralyze"
    );
    // ObjectClass::ReceiveDamage's exact-zero Destroy (0x005F57AF) runs
    // Detach_All(1) inside the killing bite, so the dog is out on that frame.
    assert_eq!(arena.frame(), death_frame);
}

/// A `RejoinTeamIfLimboed=` dog in a team of two jumps a GI eight cells
/// away; the order is given before the team forms, since a player order
/// takes a Foot off its team (`0x004C735D`). Returns the dog, its partner
/// and the team.
fn team_dog_jumps(rules: &RuleSet, arena: &mut Arena) -> (u64, u64, u64) {
    let dog = arena.spawn(rules, "DOG", "Russians", (4, 10));
    let partner = arena.spawn(rules, "DOG", "Russians", (4, 12));
    let gi = arena.spawn(rules, "E1", "Americans", (12, 10));
    arena.attack(dog, gi);
    arena.step(rules);
    arena.step(rules);
    assert!(!arena.in_limbo(dog), "the dog has not jumped yet");
    let team = crate::sim::team_script_vm::join_team_for_test(
        &mut arena.sim,
        &[dog, partner],
        false,
        false,
    );
    arena.until(rules, 300, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| d.lifecycle.in_limbo())
    });
    // TechnoClass::Fire 0x006FF7A3..0x006FF7E9 keeps the team (+0x434); the
    // jump's Limbo takes the dog off it (0x004D9744).
    assert_eq!(arena.sim.team_script_vm.team_for_member(dog), None);
    assert_eq!(arena.sim.team_script_vm.team_to_rejoin(dog), Some(team));
    (dog, partner, team)
}

#[test]
fn a_team_dog_rejoins_its_team_when_its_kill_releases_it() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let (dog, _, team) = team_dog_jumps(&rules, &mut arena);
    arena.until(&rules, 200, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| !d.lifecycle.in_limbo())
    });
    // PointerExpired's release re-adds it (0x0062A3AD..0x0062A3BC).
    assert_eq!(
        arena
            .sim
            .team_script_vm
            .team_for_member(dog)
            .map(|(id, _)| id),
        Some(team)
    );
}

#[test]
fn a_dog_whose_team_died_while_it_was_away_stays_teamless() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let (dog, partner, team) = team_dog_jumps(&rules, &mut arena);
    // The last member dies: the has-been-full team is destroyed, and
    // TechnoClass::PointerExpired (0x00707BB2) forgets it.
    arena.hit(&rules, partner, None, 1000, "Super");
    arena.until(&rules, 200, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| !d.lifecycle.in_limbo())
    });
    assert!(arena.sim.team_script_vm.team(team).is_none());
    assert_eq!(arena.sim.team_script_vm.team_to_rejoin(dog), None);
    assert_eq!(arena.sim.team_script_vm.team_for_member(dog), None);
}

#[test]
fn drone_bites_on_its_weapon_rate_until_the_host_dies_then_drops_off() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    // A player's tank: ReceiveDamage's scatter tail moves a computer's host
    // off its cell at a bite (`0x00702C8A`).
    let owner = arena.sim.entities().get(tank).unwrap().owner();
    arena
        .sim
        .houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    arena.infect(&rules, drone, tank);
    assert!(arena.in_limbo(drone));

    let mut bites = Vec::new();
    let mut last = arena.health(tank).unwrap();
    arena.until(&rules, 1000, |sim| {
        let hp = sim
            .substrate
            .entities
            .get(tank)
            .map_or(0, |t| t.health.current());
        if hp != last {
            bites.push((sim.session.binary_frame(), hp));
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
            .is_some_and(|d| !d.lifecycle.in_limbo())
    });
    let drone_entity = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(drone_entity.health.current() > 0);
    assert_eq!(
        (drone_entity.position.rx(), drone_entity.position.ry()),
        (11, 10)
    );
    assert_eq!(drone_entity.category(), EntityCategory::Unit);
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
fn a_paid_repair_radio_request_releases_the_drone_after_the_heal() {
    // Techno radio6F4D70 arms suppression50 and calls the existing ExitUnit
    // owner after payment and heal. A failed charge must leave it attached.
    // This is a Rust integration check of that caller, not a native infection
    // or depot-arrival comparison. MTNK's supplied cost0 has native floor1.
    use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse};
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    // A player's tank, before the sourceless hit: ReceiveDamage's scatter tail
    // moves a computer's (`0x00702C8A`).
    let owner = arena.sim.entities().get(tank).unwrap().owner();
    arena
        .sim
        .houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    arena.hit(&rules, tank, None, 10, "AP");
    arena.infect(&rules, drone, tank);
    let hp = arena.health(tank).unwrap();
    assert!(hp <= 290);
    assert_eq!(
        crate::sim::radio::receive_radio(
            &mut arena.sim,
            tank,
            None,
            RadioMessage::RepairTick,
            RadioPayload::default(),
            Some(&rules),
            crate::sim::world::FrameEffects::default(),
        ),
        RadioResponse::InsufficientFunds
    );
    assert_eq!(arena.eater_of(tank), Some(drone));
    assert_eq!(arena.health(tank), Some(hp));
    arena
        .sim
        .houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(1);
    assert_eq!(
        crate::sim::radio::receive_radio(
            &mut arena.sim,
            tank,
            None,
            RadioMessage::RepairTick,
            RadioPayload::default(),
            Some(&rules),
            crate::sim::world::FrameEffects::default(),
        ),
        RadioResponse::Roger
    );
    assert_eq!(arena.health(tank), Some(hp + 8));
    assert!(arena.gone(drone));
    assert_eq!(arena.eater_of(tank), None);
    assert_eq!(arena.sim.houses[&owner].economy().credits(), 0);
    assert_eq!(arena.sim.houses[&owner].economy().spent_credits(), 1);
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
fn retail_drone_fire_frames_survive_launch_parasitism_and_release() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    // The ordered reader above retains UnitType's native frame layout. Install
    // its immutable ART metadata too, as the normal asset binding does.
    rules.install_art_fixture(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (8, 8));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (9, 8));
    arena.infect(&rules, drone, tank);
    assert_eq!(
        arena
            .sim
            .entities()
            .get(drone)
            .unwrap()
            .unit_firing_counter
            .get(),
        7
    );
    for _ in 0..12 {
        arena.step(&rules);
    }
    assert!(arena.in_limbo(drone));
    assert_eq!(
        arena
            .sim
            .entities()
            .get(drone)
            .unwrap()
            .unit_firing_counter
            .get(),
        7
    );
    arena.hit(&rules, tank, None, 4, "SonicWarhead");
    let actor = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(actor.lifecycle.object_alive() && !actor.lifecycle.in_limbo());
    let facing = actor.body_facing_byte(arena.frame());
    let native = crate::test_fixture::json("tools/spatial_oracle/unit_shp_fire_frames.json");
    let expected = u16::try_from(
        native["first_by_facing"][usize::from(facing)]
            .as_u64()
            .unwrap(),
    )
    .unwrap();
    let frame = crate::sim::animation::resolve_shp_vehicle_body_frame(
        rules.animation_sequence("DRON").unwrap(),
        facing,
        actor.body_frame_counter.get(),
        actor.unit_firing_counter.get(),
        crate::sim::movement::motion_query::is_moving(actor) == Some(true),
        false,
    );
    // Original Unit741340 -> Bullet4666E0 -> Attach62A980 -> Exit62A4A0
    // retains +6C0=7 through limbo. Draw73C62B..73C725 selects the fire
    // block for the actual facing, not the standing block. The original
    // counter resumes in Unit7363C9.
    assert_eq!(frame, Some(expected), "post-release DRON facing {facing}");

    // Stop interrupts the attack without restarting/consuming a separate
    // presentation clock. Match the actual original Unit AI countdown.
    let owner = actor.owner();
    arena.sim.queue_command(CommandEnvelope::new(
        owner,
        arena.sim.session.tick() + 1,
        Command::Stop { entity_id: drone },
    ));
    let history = crate::test_fixture::json("tools/spatial_oracle/unit_shp_fire_history.json");
    for row in history["history"]["recovery"].as_array().unwrap() {
        arena.step(&rules);
        let actor = arena.sim.entities().get(drone).unwrap();
        let counter = actor.unit_firing_counter.get();
        assert_eq!(
            i64::from(counter),
            row["source"]["counter"].as_i64().unwrap()
        );
        let image = crate::sim::animation::resolve_shp_vehicle_body_frame(
            rules.animation_sequence("DRON").unwrap(),
            actor.body_facing_byte(arena.frame()),
            actor.body_frame_counter.get(),
            counter,
            crate::sim::movement::motion_query::is_moving(actor) == Some(true),
            false,
        )
        .unwrap();
        assert_eq!((56..88).contains(&image), counter >= 0);
    }
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
        .attack_target
        .set_for_test(Some(TargetKind::Entity(tank)));
    arena
        .sim
        .substrate
        .entities
        .get_mut(drone)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(3, 3)));
    let host_facing = arena
        .sim
        .substrate
        .entities
        .get(tank)
        .unwrap()
        .body_facing
        .destination();
    arena.hit(&rules, tank, Some(dolphin), 4, "SonicWarhead");

    let frame = arena.frame();
    let released = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(!released.lifecycle.in_limbo() && released.lifecycle.object_alive());
    assert_eq!(released.health.current(), 100);
    assert_eq!(host_facing, 0);
    assert_eq!(
        released.body_facing.destination(),
        0x4000,
        "north host: released facing east"
    );
    assert_eq!(released.parasite.paralysis.remaining(frame as i32), 3 * 60);
    // ExitUnit 0x0062A771: Set_ArchiveTarget(NULL).
    assert_eq!(released.archive_target(), None);
    let (rx, ry) = arena.cell(drone);
    assert!(rx.abs_diff(11) <= 1 && ry.abs_diff(10) <= 1 && (rx, ry) != (11, 10));
    assert_eq!(arena.eater_of(tank), None);
    let host = arena.sim.substrate.entities.get(tank).unwrap();
    assert!(!host.parasite.is_paralyzed(frame));
    assert!(
        arena
            .sim
            .substrate
            .entities
            .get(dolphin)
            .unwrap()
            .attack_target
            .target()
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
    let sw = crate::sim::superweapon::chronosphere_tests::charge_super(
        &mut arena.sim,
        owner,
        "IronCurtainSpecial",
    );
    crate::sim::superweapon::iron_curtain::launch(
        &mut arena.sim,
        &rules,
        owner,
        11,
        10,
        sw,
        None,
        crate::sim::world::FrameEffects::default(),
    );

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
    let host = arena.sim.substrate.entities.get_mut(tank).unwrap();
    // A warping host runs Teleport as its active locomotor (the Chrono Miner).
    host.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Teleport,
        ),
    );
    host.install_teleport_state_for_test(Some(
        crate::sim::movement::teleport_movement::TeleportState::for_test(
            crate::sim::movement::teleport_movement::TeleportPhase::Relocate,
            20,
            20,
            0,
        ),
    ));
    arena.step(&rules);

    assert_eq!(arena.cell(tank), (20, 20));
    assert_eq!(arena.eater_of(tank), None);
    let released = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(!released.lifecycle.in_limbo() && released.lifecycle.object_alive());
    assert!(released.parasite.is_paralyzed(arena.frame()));
    let (rx, ry) = arena.cell(drone);
    assert!(rx.abs_diff(11) <= 1 && ry.abs_diff(10) <= 1);
}

#[test]
fn a_teleport_warp_drops_the_drone_onto_ore_through_the_frames_overlay_table() {
    // ExitUnit's Unlimbo (0x0062A5E8..0x0062A61F) asks the drone's own
    // Can_Enter_Cell, whose overlay arm reads the OverlayTypeClass table the
    // frame binds: a Chrono Miner warping off an ore field drops its drone on
    // ore. Without the table the query fails and the drone is deleted.
    use crate::sim::tiberium::test_support::{bales_at, overlay_registry, place_tiberium_on_map};
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    arena.infect(&rules, drone, tank);
    // The overlay grid matches the arena's 32x32 map.
    arena.sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    for y in 9..=11 {
        for x in 10..=12 {
            place_tiberium_on_map(
                &mut arena.sim,
                (x, y),
                crate::sim::miner::ResourceType::Ore,
                5,
            );
        }
    }
    assert!(arena.sim.rebuild_dynamic_navigation(&rules));
    arena.grid = (*arena.sim.path_grid_snapshot().unwrap()).clone();
    let host = arena.sim.substrate.entities.get_mut(tank).unwrap();
    host.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Teleport,
        ),
    );
    host.install_teleport_state_for_test(Some(
        crate::sim::movement::teleport_movement::TeleportState::for_test(
            crate::sim::movement::teleport_movement::TeleportPhase::Relocate,
            20,
            20,
            0,
        ),
    ));
    let commands = arena.sim.take_due_commands();
    let _ = arena.sim.advance_tick(
        &commands,
        Some(&rules),
        Some(&arena.grid),
        Some(overlay_registry()),
        33,
    );

    assert_eq!(arena.cell(tank), (20, 20));
    let released = arena.sim.substrate.entities.get(drone).unwrap();
    assert!(!released.lifecycle.in_limbo() && released.lifecycle.object_alive());
    let (rx, ry) = arena.cell(drone);
    assert!(
        bales_at(&arena.sim, rx, ry) > 0,
        "released onto ore at ({rx},{ry})"
    );
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
            .is_some_and(|d| d.lifecycle.in_limbo())
    });
    // Attack's ordinary MegaMission first clears Archive through70C610
    // (4C73EF's mission!=11 branch,4C7448..4C7451). Supply this component
    // prior after that event/launch so the comparison isolates AttachTo's
    // refusal tail62AA96..62AAD9, which has no Archive assignment.
    let launched = arena.sim.substrate.entities.get_mut(drone).unwrap();
    assert_eq!(launched.archive_target(), None);
    launched.set_archive_target(anchor);
    arena.hit(&rules, tank, Some(rhino), 400, "AP");
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(drone)
            .is_some_and(|d| !d.lifecycle.in_limbo())
    });
    let returned = arena.sim.substrate.entities.get(drone).unwrap();
    assert_eq!(arena.cell(drone), (10, 10));
    assert_eq!(returned.body_facing.destination(), 0);
    assert!(!returned.parasite.is_paralyzed(arena.frame()));
    assert!(returned.parasite.class().unwrap().victim.is_none());
    // The refusal (0x0062AA96..0x0062AAC9) never calls Set_ArchiveTarget.
    assert_eq!(returned.archive_target(), anchor);
    // Its ReReveal (0x0062AA51) takes back the sight the Unlimbo opened, and
    // no frame gives it back before the drone's next reveal event.
    let russians = returned.owner();
    assert!(!arena.sim.fog.sight_is_latched(drone, russians));
    arena.step(&rules);
    assert!(!arena.sim.fog.sight_is_latched(drone, russians));
}

#[test]
fn a_drone_whose_launch_cell_was_taken_is_deleted_when_its_host_is_lost() {
    // AttachTo's refusal Unlimbo (0x0062AA2C) raises no 0x00A8E7AC, so the
    // drone's own Can_Enter_Cell (Object 0x005F4F1B..0x005F4F44) decides: a
    // tank that took the launch cell during the jump refuses it, and the
    // refused drone is deleted (0x0062AA39).
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    let rhino = arena.spawn(&rules, "MTNK", "Russians", (14, 10));
    arena.attack(drone, tank);
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(drone)
            .is_some_and(|d| d.lifecycle.in_limbo())
    });
    arena.spawn(&rules, "MTNK", "Russians", (10, 10));
    arena.hit(&rules, tank, Some(rhino), 400, "AP");
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(drone)
            .is_none_or(|d| !d.lifecycle.in_limbo() || !d.lifecycle.object_alive())
    });
    assert!(arena.gone(drone));
}

#[test]
fn an_attached_parasite_may_enter_its_victims_cell_and_no_other_held_cell() {
    // Unit Can_Enter_Cell 0x0073F530..0x0073F56F: an owner holding a victim
    // answers 0 at the first other object whose map cell is the victim's,
    // before any blocker rule. An unarmed friendly vehicle's cell still
    // refuses it.
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    arena.spawn(&rules, "MCV", "Russians", (10, 12));
    let can_enter = |arena: &Arena, cell: (i16, i16)| {
        let terrain = arena.sim.resolved_terrain.as_ref().unwrap();
        let cell = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(cell);
        arena
            .sim
            .foot_can_enter(
                drone,
                cell,
                crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
                &rules,
                None,
            )
            .unwrap()
    };
    assert_ne!(can_enter(&arena, (11, 10)), 0, "a free drone is refused");
    arena.infect(&rules, drone, tank);
    assert_eq!(can_enter(&arena, (11, 10)), 0);
    assert_ne!(can_enter(&arena, (10, 12)), 0);
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
    let can_load =
        |sim: &Simulation| crate::sim::passenger::can_enter_transport(sim, &rules, tank, transport);
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
        .mcv_deploy_pending
        .set_for_test(true);
    assert!(!arena.sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(
        !arena
            .sim
            .substrate
            .entities
            .get(mcv)
            .unwrap()
            .mcv_deploy_pending
            .is_pending()
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
            .is_some_and(|t| t.health.current() == 250)
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
        assert_eq!(left.parasite.eater, right.parasite.eater);
        assert_eq!(left.parasite.launch_lock, right.parasite.launch_lock);
        assert_eq!(left.parasite.paralysis, right.parasite.paralysis);
        assert_eq!(left.lifecycle.in_limbo(), right.lifecycle.in_limbo());
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
            .set_for_test(5);
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
        // Skirmish: IsHumanPlayer (`0x0050B6F0`) is the local player.
        arena.sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        arena
            .sim
            .session
            .set_current_house_for_test(Some(arena.sim.interner.intern(local)));
        arena
            .sim
            .substrate
            .entities
            .get_mut(dog)
            .unwrap()
            .selection
            .set_selected_for_test(selected);
        arena.attack(dog, gi);
        arena.until(&rules, 100, |sim| {
            sim.substrate
                .entities
                .get(dog)
                .is_some_and(|d| d.lifecycle.in_limbo())
        });
        let memo = arena
            .sim
            .substrate
            .entities
            .get(dog)
            .unwrap()
            .parasite
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
    // The release boundary supplies a real structural deck beneath both
    // objects. OnBridge alone on the old clear cells made the low target's
    // native InRange ground snap fall to Z0, outside BadTeeth's range.
    for (x, y) in [(10, 10), (11, 10)] {
        let cell = arena
            .sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(x, y)
            .unwrap();
        cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_deck_level = 4;
    }
    assert!(arena.sim.rebuild_dynamic_navigation(&rules));
    arena.grid = (*arena.sim.path_grid_snapshot().unwrap()).clone();
    let dog = arena.spawn(&rules, "DOG", "Russians", (10, 10));
    let gi = arena.spawn(&rules, "E1", "Americans", (11, 10));
    // Both stand on a deck four levels up.
    let deck = 4 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
    for id in [dog, gi] {
        arena.sim.remove_entity_occupancy(id);
        let entity = arena.sim.substrate.entities.get_mut(id).unwrap();
        entity.set_on_bridge(true);
        entity.position.set_z_for_test(4);
        entity.position.set_exact_z_leptons_for_test(Some(deck));
        arena.sim.add_entity_occupancy(id);
    }
    arena.attack(dog, gi);
    arena.until(&rules, 60, |sim| {
        sim.substrate
            .entities
            .get(gi)
            .is_none_or(|v| v.health.current() == 0)
    });
    arena.until(&rules, 100, |sim| {
        sim.substrate
            .entities
            .get(dog)
            .is_some_and(|d| !d.lifecycle.in_limbo())
    });
    let released = arena.sim.substrate.entities.get(dog).unwrap();
    assert!(released.on_bridge());
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
            .is_some_and(|d| d.attack_target.target().is_none() && sim.session.tick() > 2)
    });
    assert!(!arena.in_limbo(drone));
    assert_eq!(arena.eater_of(tank), None);
}

/// CanPlaceAtVictim (`0x0062AB40`) refuses only a victim in the air: it asks
/// the victim's vt+0x54 (`0x0062AB52`), IsInAir (`0x005F6B90`), which needs a
/// height of at least 208. A paratrooper still falling below that takes the
/// drone at its own cell.
#[test]
fn a_parasite_is_placed_at_a_falling_victim_below_the_air_line() {
    let rules = rules();
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let victim = arena.spawn(&rules, "E1", "Americans", (11, 10));
    let mut place_at_height = |height: i32| {
        arena
            .sim
            .substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(height));
        arena
            .sim
            .parasite_can_place_at_victim(drone, victim, &rules)
    };
    assert!(
        place_at_height(207),
        "below 208 the victim is not in the air"
    );
    assert!(!place_at_height(208), "at 208 the victim is in the air");
}

/// `tools/spatial_oracle/parasite_water_gate.json`: the original CanInfect
/// with a Naval, a land or no owner, and the victim on a tile around the
/// theater's WaterSet base or on the shared dummy. VERA's grid has no NULL
/// slot inside its bounds, so both dummy rows put the victim past the grid.
#[test]
fn naval_owner_water_gate_matches_original_can_infect() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/parasite_water_gate.json",
    ))
    .unwrap();
    let rules = rules();
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 14);
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let mut arena = Arena::new(&rules);
        let victim = arena.spawn(&rules, "MTNK", "Americans", (5, 2));
        // The owner's type supplies only `Naval=`; an id with no object is
        // the parasite without an owner.
        let owner = match (
            row["owner"].as_bool().unwrap(),
            row["naval"].as_bool().unwrap(),
        ) {
            (true, true) => arena.spawn(&rules, "DLPH", "Russians", (9, 9)),
            (true, false) => arena.spawn(&rules, "DRON", "Russians", (9, 9)),
            (false, _) => u64::MAX,
        };
        let terrain = arena.sim.resolved_terrain.as_mut().unwrap();
        terrain.set_projectile_water_set_base(row["water_set"].as_i64().unwrap() as i32);
        terrain.cell_mut(5, 2).unwrap().final_tile_index = row["tile"].as_i64().unwrap() as i32;
        if row["victim_cell"] != serde_json::json!([5, 2]) {
            arena
                .sim
                .substrate
                .entities
                .get_mut(victim)
                .unwrap()
                .position
                .set_rx_for_test(200);
        }
        assert_eq!(
            arena.sim.parasite_can_infect(owner, Some(victim), &rules),
            row["admits"].as_bool().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn grinder_arrival_refunds_the_infesting_drone_before_forced_release() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::movement::PerCellReason;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::world::FrameEffects;

    let input = format!(
        "[Countries]\n0=Russians\n1=Americans\n{}",
        RULES
            .replace("[DRON]\n", "[DRON]\nSoylent=200\n")
            .replace("[MTNK]\n", "[MTNK]\nSoylent=300\n")
    );
    let rules = rules_with_ini(&IniFile::from_str(&input));
    let mut arena = Arena::new(&rules);
    let drone = arena.spawn(&rules, "DRON", "Russians", (10, 10));
    let tank = arena.spawn(&rules, "MTNK", "Americans", (11, 10));
    arena.infect(&rules, drone, tank);
    let building = arena.spawn(&rules, "YARD", "Americans", (18, 10));
    let (x, y, owner) = {
        let actor = arena.sim.entities().get(tank).unwrap();
        (actor.position.rx(), actor.position.ry(), actor.owner())
    };
    arena.sim.substrate.occupancy.add(
        x,
        y,
        building,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::AppendBuilding,
    );
    let actor = arena.sim.substrate.entities.get_mut(tank).unwrap();
    actor
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Eaten), 0);
    actor
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::object(building)));
    arena
        .sim
        .houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let russian = arena.sim.interner.get("Russians").unwrap();
    arena
        .sim
        .houses
        .register(crate::sim::house_state::HouseState::new(
            russian, 1, None, false, 0, 10,
        ));
    let before = arena.sim.houses[&owner].economy().credits();
    arena
        .sim
        .per_cell_process(
            tank,
            PerCellReason::Arrival,
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
    // Unit73A0A5 refunds the tank300, then73A13E refunds the drone200.
    assert_eq!(arena.sim.houses[&owner].economy().credits(), before + 500);
    assert!(arena.gone(tank));
    assert!(
        arena.gone(drone),
        "suppression50 precedes ExitUnit's nonnaval arm"
    );
    assert_eq!(arena.eater_of(tank), None);
}
