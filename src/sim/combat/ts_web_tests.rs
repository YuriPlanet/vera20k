//! Firestorm 2.0's webs (docs/ts-webs.md) through the production detonation,
//! receiver, Doing and weapon-choice owners: `BulletClass::Detonate`'s
//! Webby branch (0x004465B0), `InfantryClass::Take_Damage`'s web arm
//! (0x004D2380), `Doing_AI` (0x004D8A20) and `What_Weapon_Should_I_Use`
//! (0x00629F20). Expected values come from instruction reading.

use super::TargetKind;
use super::combat_weapon::{attacker_facts, select_weapon_against};
use super::world_receiver::{ReceiverRun, commit_projectiles};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::particle_system_type::ParticleSystemBehavesLike;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::movement::infantry_action::{DO_PRONE, DO_STRUGGLE};
use crate::sim::pathfinding::PathGrid;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

/// Firestorm's web gun on a `SPIDER`, an `E1` and a web-immune `CYBORG`;
/// the web lasts 60 frames with no variation and reaches one cell.
const RULES: &str = "\
[InfantryTypes]
0=E1
1=CYBORG
[VehicleTypes]
0=SPIDER
[E1]
Strength=125
Speed=4
Primary=Pistols
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[CYBORG]
Strength=300
Speed=4
Primary=Pistols
IsWebImmune=yes
Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}
[SPIDER]
Strength=200
Speed=6
Primary=Pistols
Secondary=WebLauncher
[Pistols]
Damage=10
ROF=20
Range=5
Warhead=SA
[WebLauncher]
Damage=10
ROF=100
Range=6
Warhead=WebMass
[Warheads]
0=SA
1=WebMass
[SA]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[WebMass]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
Webby=yes
WebDuration=60
WebDurationVariation=0
WebRadius=1
Particle=WebSys
[ParticleSystems]
0=WebSys
1=SAWebSys
[WebSys]
HoldsWhat=Web
BehavesLike=Web
Lifetime=30
[SAWebSys]
HoldsWhat=SAWeb
BehavesLike=Web
Lifetime=30
[Particles]
0=Web
1=SAWeb
[Web]
BehavesLike=Web
MaxEC=80
EndStateAI=10
StateAIAdvance=2
DeleteOnStateLimit=yes
[SAWeb]
BehavesLike=Web
Warhead=SA
MaxEC=80
EndStateAI=10
StateAIAdvance=2
DeleteOnStateLimit=yes
";

const ART: &str = "\
[E1]
Sequence=E1Sequence
[CYBORG]
Sequence=E1Sequence
[E1Sequence]
Ready=0,1,1
Guard=0,1,1
Prone=86,1,6
Walk=8,6,6
Struggle=0,6,0
";

fn rules() -> RuleSet {
    rules_from(RULES)
}

fn rules_from(text: &str) -> RuleSet {
    let art = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(text),
        &art,
    )
    .expect("web rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules
}

/// The spider at (20,20), the E1 at (23,20) and the cyborg beside him.
fn scene(rules: &RuleSet) -> (Simulation, PathGrid, [u64; 3]) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4465B0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.set_input_delay_ticks(0);
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    let mut spawn = |kind, owner, rx| {
        sim.spawn_object_at_height(kind, owner, rx, 20, 0, 0, rules)
            .unwrap_or_else(|| panic!("{kind} spawns"))
    };
    let ids = [
        spawn("SPIDER", "Americans", 20),
        spawn("E1", "Russians", 23),
        spawn("CYBORG", "Russians", 24),
    ];
    (sim, grid, ids)
}

/// One `WebLauncher` bullet from `firer` bursting on `target`.
fn web(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64) {
    let coords = object_get_coords(sim.entities().get(target).unwrap(), sim.resolved_terrain());
    let payload = ProjectilePayload::new(10, sim.interner.intern("WebMass"), None, false)
        .with_installed_weapon(Some(sim.interner.intern("WebLauncher")));
    let detonation = ProjectileDetonation {
        projectile_id: 9_999,
        source_id: firer,
        target: ProjectileTarget::Entity(target),
        impact: ProjectileCoord::new(coords.x, coords.y, coords.z),
        payload,
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = ReceiverRun::default();
    let _ = commit_projectiles(sim, &mut run, &[detonation], rules, None);
    run.finish();
}

fn step(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
}

fn web_left(sim: &Simulation, id: u64) -> i32 {
    let frame = sim.session.binary_frame() as i32;
    sim.entities().get(id).unwrap().web_remaining(frame)
}

fn doing(sim: &Simulation, id: u64) -> i32 {
    sim.entities()
        .get(id)
        .unwrap()
        .infantry_sprite_pose()
        .unwrap()
        .0
}

/// The burst webs the man it hits without harming him and leaves one web
/// system in each cell within `WebRadius` (0x00446640..0x00446838); the
/// `IsWebImmune` cyborg beside him is untouched (0x004D23E5).
#[test]
fn a_web_holds_a_man_unharmed_and_spares_the_web_immune() {
    let rules = rules();
    let (mut sim, _, [spider, man, cyborg]) = scene(&rules);
    web(&mut sim, &rules, spider, man);

    let frame = sim.session.binary_frame() as i32;
    let man_entity = sim.entities().get(man).unwrap();
    assert_eq!(man_entity.health.current(), 125, "a web does no damage");
    assert_eq!(web_left(&sim, man), 60);
    assert_eq!(doing(&sim, man), DO_STRUGGLE);
    assert!(man_entity.is_immobilized(frame));
    assert_eq!(sim.entities().get(cyborg).unwrap().health.current(), 300);
    assert_eq!(web_left(&sim, cyborg), 0, "IsWebImmune");

    let systems: Vec<_> = sim.particle_systems().iter().map(|(_, sys)| sys).collect();
    assert_eq!(systems.len(), 5, "the centre and its four neighbours");
    assert!(systems.iter().all(|sys| {
        rules.particle_system_type(sys.type_id()).behaves_like == ParticleSystemBehavesLike::Web
            && sys.particles().len() == 1
    }));
}

/// The webbed man struggles for the whole web, refuses a move order, and
/// lies down in Prone once it is over (0x004D8A6B..0x004D8A92).
#[test]
fn a_webbed_man_struggles_in_place_until_the_web_ends_then_lies_prone() {
    let rules = rules();
    let (mut sim, grid, [spider, man, _]) = scene(&rules);
    web(&mut sim, &rules, spider, man);
    let owner = sim.interner.intern("Russians");
    let tick = sim.session.tick() + 1;
    sim.queue_command(crate::sim::command::CommandEnvelope::new(
        owner,
        tick,
        crate::sim::command::Command::Move {
            entity_id: man,
            target_rx: 28,
            target_ry: 20,
        },
    ));
    let cell = |sim: &Simulation| {
        let position = &sim.entities().get(man).unwrap().position;
        (position.rx(), position.ry())
    };
    while web_left(&sim, man) > 0 {
        assert_eq!(
            doing(&sim, man),
            DO_STRUGGLE,
            "{} left",
            web_left(&sim, man)
        );
        assert_eq!(cell(&sim), (23, 20));
        step(&mut sim, &rules, &grid);
    }
    for _ in 0..30 {
        if doing(&sim, man) != DO_STRUGGLE {
            break;
        }
        step(&mut sim, &rules, &grid);
    }
    assert_eq!(doing(&sim, man), DO_PRONE);
    assert_eq!(cell(&sim), (23, 20));
}

/// A web gun webs what it can and the other gun takes the rest
/// (0x0062A13A..0x0062A145): the secondary for a free man, the primary for
/// a webbed one and for a web-immune one.
#[test]
fn a_web_gun_is_chosen_only_against_a_man_it_can_web() {
    let rules = rules();
    let (mut sim, _, [spider, man, cyborg]) = scene(&rules);
    let choose = |sim: &Simulation, target| {
        let firer = sim.entities().get(spider).unwrap();
        let object = rules.object("SPIDER").unwrap();
        select_weapon_against(
            &rules,
            firer,
            object,
            &attacker_facts(firer, object),
            Some(&TargetKind::Entity(target)),
            sim,
            &sim.interner,
            sim.resolved_terrain.as_ref(),
            None,
        )
        .unwrap()
        .0
    };
    assert_eq!(choose(&sim, man), 1, "a free man");
    assert_eq!(choose(&sim, cyborg), 0, "IsWebImmune");
    web(&mut sim, &rules, spider, man);
    assert_eq!(choose(&sim, man), 0, "already webbed");
}

/// Each cell's men are hit right after that cell's web particle is made
/// (0x004467BC, then 0x00446803), so the man in the centre, the third cell,
/// takes the fourth Scenario draw: three particle lifetimes come first.
#[test]
fn each_cell_is_webbed_right_after_its_particle_is_made() {
    use crate::sim::rng::SimRng;
    let rules = rules_from(&RULES.replace("WebDurationVariation=0", "WebDurationVariation=7"));
    let (mut sim, _, [spider, man, _]) = scene(&rules);
    *sim.scenario_rng = SimRng::with_next_draws_for_test(&[0, 0, 0, 3, 0, 0, 0, 0]);
    web(&mut sim, &rules, spider, man);
    let variation = SimRng::with_next_draws_for_test(&[3]).next_range_i32_inclusive(-7, 7);
    assert_eq!(web_left(&sim, man), 60 + variation);
}

/// A web particle whose warhead does not web hurts no one: Firestorm drops
/// its zero damage (0x00585EE4), where gamemd's receiver would make it 1.
#[test]
fn a_web_particle_without_a_webby_warhead_leaves_its_cell_unharmed() {
    let rules = rules();
    let (mut sim, grid, [_, man, _]) = scene(&rules);
    let coords = object_get_coords(sim.entities().get(man).unwrap(), sim.resolved_terrain());
    let point = glam::IVec3::new(coords.x, coords.y, coords.z);
    let system = sim
        .spawn_particle_system(
            rules.ps_type_id_by_name("SAWebSys").unwrap(),
            point,
            None,
            None,
            glam::IVec3::ZERO,
            None,
            &rules,
        )
        .unwrap();
    sim.spawn_particle_in_system(system, point, point, &rules);
    for _ in 0..5 {
        step(&mut sim, &rules, &grid);
    }
    assert_eq!(
        sim.particle_systems()
            .get(system)
            .unwrap()
            .particles()
            .len(),
        1
    );
    assert_eq!(sim.entities().get(man).unwrap().health.current(), 125);
}
