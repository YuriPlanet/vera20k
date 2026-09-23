//! Tests for the Temporal owner. `native_update_corpus` compares against
//! `tools/spatial_oracle/temporal_update.json`, produced by running the
//! original `TemporalClass::Update` under Unicorn; the rest are Rust
//! regression tests of the chain, the releases and the production paths.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::SimSoundEvent;

const RULES: &str = "\
[General]
WarpAway=WARPAWAY
ChronoSparkle1=CHRONOSK
[CombatDamage]
OpenToppedWarpDistance=7
C4Warhead=Super
[InfantryTypes]
0=CLEG
1=CLEGU
2=CLEG1
3=E1
[VehicleTypes]
0=HTNK
1=BFRT
2=FV
[AircraftTypes]
[BuildingTypes]
0=GAPOWR
1=GACNST
[Warheads]
0=ChronoBeam
1=Super
2=AP
[CLEG]
Strength=125
Cost=1500
Speed=4
Sight=6
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=NeutronRifle
ElitePrimary=NeutronRifleE
IFVMode=10
[CLEGU]
Strength=125
Cost=1500
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=CRNeutronRifle
Trainable=no
[CLEG1]
Strength=125
Cost=1500
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=Beam1
[E1]
Strength=125
Cost=200
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=M60
[HTNK]
Strength=400
Cost=900
Speed=4
ROT=5
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
Primary=Gun
[BFRT]
Strength=300
Speed=4
Passengers=5
OpenTopped=yes
[FV]
Strength=200
Speed=4
Passengers=1
Gunner=yes
[GAPOWR]
Strength=750
Cost=800
Foundation=2x2
Power=100
[GACNST]
Strength=1000
Foundation=4x4
Power=-50
[NeutronRifle]
Damage=8
ROF=120
Range=5
Projectile=InvisibleMedium
Warhead=ChronoBeam
[NeutronRifleE]
Damage=16
ROF=120
Range=5
Projectile=InvisibleMedium
Warhead=ChronoBeam
[CRNeutronRifle]
Damage=5
ROF=120
Range=6
Projectile=InvisibleLow
Warhead=ChronoBeam
[Beam1]
Damage=1
ROF=120
Range=5
Projectile=InvisibleMedium
Warhead=ChronoBeam
[M60]
Damage=15
ROF=20
Range=4
Projectile=InvisibleLow
Warhead=AP
[Gun]
Damage=90
ROF=65
Range=5.75
Projectile=InvisibleLow
Warhead=AP
[InvisibleMedium]
Inviso=yes
[InvisibleLow]
Inviso=yes
[ChronoBeam]
Temporal=yes
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,0%
[Super]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[AP]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const ART: &str = "\
[WARPAWAY]
Rate=300
[CHRONOSK]
Rate=150
LoopStart=0
LoopEnd=2
LoopCount=1
";

fn rules() -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("temporal rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(ART));
    art.bind_anim_frame_count_for_test("WARPAWAY", 20);
    art.bind_anim_frame_count_for_test("CHRONOSK", 3);
    rules.art_registry = art;
    rules
}

/// Americans (human), Russians and Allies2 (computer).
fn sim(seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    for (name, side, human) in [
        ("Americans", 0, true),
        ("Russians", 1, false),
        ("Allies2", 0, false),
    ] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn entity(sim: &Simulation, id: u64) -> &GameEntity {
    sim.substrate.entities.get(id).expect("entity")
}

fn link(sim: &Simulation, id: u64) -> TemporalLink {
    entity(sim, id)
        .temporal
        .link
        .clone()
        .expect("a Temporal firer")
}

fn head_of(sim: &Simulation, id: u64) -> Option<u64> {
    entity(sim, id).temporal.head
}

fn erased(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_none_or(|entity| !entity.lifecycle.object_alive)
}

/// Link the attackers onto `target` in chain order (the first is the head),
/// as the oracle fixture does.
fn chain(sim: &mut Simulation, target: u64, attackers: &[u64], warp_remaining: i32) {
    for (n, &attacker) in attackers.iter().enumerate() {
        let link = TemporalLink {
            target: Some(target),
            prev: n.checked_sub(1).map(|prev| attackers[prev]),
            next: attackers.get(n + 1).copied(),
            warp_remaining: if n == 0 { warp_remaining } else { 0 },
        };
        sim.substrate
            .entities
            .get_mut(attacker)
            .unwrap()
            .temporal
            .link = Some(link);
    }
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .temporal
        .head = attackers.first().copied();
}

/// Place an object at an exact lepton coordinate (shifted by `ORIGIN` so the
/// oracle's negative coordinates fit VERA's unsigned cells).
fn place(sim: &mut Simulation, id: u64, coord: [i32; 3]) {
    const ORIGIN: i32 = 16 * 256;
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let (x, y) = (coord[0] + ORIGIN, coord[1] + ORIGIN);
    entity.position.rx = (x / 256) as u16;
    entity.position.ry = (y / 256) as u16;
    entity.position.sub_x = crate::util::fixed_math::SimFixed::from_num(x % 256);
    entity.position.sub_y = crate::util::fixed_math::SimFixed::from_num(y % 256);
    entity.position.exact_z_leptons = Some(coord[2]);
}

fn set_elite(sim: &mut Simulation, id: u64) {
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.veterancy_raw = crate::util::native_x87::NativeF32Bits::from_bits(2.0f32.to_bits());
    entity.veterancy = crate::sim::combat::veterancy::rank_u16(entity.veterancy_raw);
}

fn veterancy(sim: &Simulation, id: u64) -> f32 {
    f32::from_bits(entity(sim, id).veterancy_raw.bits())
}

fn put_on_attack(sim: &mut Simulation, id: u64) {
    let now = sim.session.binary_frame;
    let _ = sim.mission_assign_exact(id, MissionId::from_known(MissionType::Attack), now);
}

fn warp_away_anims(sim: &Simulation) -> Vec<AnimWorldCoord> {
    sim.substrate
        .anims
        .iter()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "WARPAWAY")
        .map(|(&id, anim)| {
            assert_eq!(anim.draw_flags, TEMPORAL_ANIM_DRAW_FLAGS);
            sim.anim_absolute_coord(id).unwrap()
        })
        .collect()
}

fn unit_lost_events(sim: &Simulation) -> usize {
    sim.sound_events
        .iter()
        .filter(|event| matches!(event, SimSoundEvent::UnitLost { .. }))
        .count()
}

fn house_stats(sim: &Simulation, house: &str) -> crate::sim::house_state::MatchStatistics {
    let id = sim.interner.get(house).unwrap();
    sim.houses.get(&id).unwrap().stats
}

/// Init_Managers reads weapon 0 only, at rookie rank.
#[test]
fn init_links_only_temporal_firers() {
    let rules = rules();
    let has_link = |kind: &str| init_temporal(rules.object(kind).unwrap(), &rules).has_link();
    assert!(has_link("CLEG"));
    assert!(has_link("CLEGU"));
    assert!(!has_link("E1"));
    assert!(!has_link("HTNK"));
    assert!(!has_link("FV"), "the IFV borrows its gunner's link");
    let mut sim = sim(1);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Americans", 10, 10);
    assert!(
        entity(&sim, cleg).temporal.has_link(),
        "construction installs it"
    );
    assert!(!entity(&sim, cleg).temporal.is_warping_someone());
}

#[derive(serde::Deserialize)]
struct NativeCase {
    input: NativeInput,
    attackers: Vec<NativeNode>,
    events: Vec<serde_json::Value>,
    target_head: String,
    target_warped: u8,
}

#[derive(serde::Deserialize)]
struct NativeInput {
    name: String,
    attackers: Vec<NativeAttacker>,
    warp_remaining: i32,
    target: NativeTarget,
    #[serde(default)]
    corrupt_head: bool,
}

#[derive(serde::Deserialize)]
struct NativeAttacker {
    damage: i32,
    #[serde(default = "trainable_default")]
    trainable: u8,
    #[serde(default)]
    open_topped: u8,
    coords: Option<[i32; 3]>,
}

fn trainable_default() -> u8 {
    1
}

#[derive(serde::Deserialize)]
struct NativeTarget {
    coords: [i32; 3],
    rtti: u8,
}

#[derive(serde::Deserialize)]
struct NativeNode {
    target: String,
    prev: String,
    next: String,
    warp_remaining: i32,
}

/// The oracle's names for a node's pointer fields, over this fixture's ids.
fn native_name(value: Option<u64>, target: u64, attackers: &[u64]) -> String {
    match value {
        None => "0x0".to_string(),
        Some(id) if id == target => "target".to_string(),
        Some(id) => {
            let n = attackers
                .iter()
                .position(|&a| a == id)
                .expect("an attacker");
            format!("temporal{n}")
        }
    }
}

/// `TemporalClass::Update @ 0x0071A760` against the original, case by case:
/// the step (the head's weapon plus SumChainDamage to depth 0x32), the erase
/// at `<= 0` and its effects, the corrupt-head release, and the open-topped
/// release boundary through `Sqrt_Approx` and `ftol` (`distance_3d_leptons`).
///
/// The oracle stubs UnInit, so its chained attackers keep their links after
/// an erase; VERA's UnInit runs the pointer-expiry forward (`0x0071AB60`),
/// which clears them as the native forward does. Its `idle` events are the
/// Update's own; VERA queues the same Guard. `kill_occupants` (the building
/// erase's `0x004585C0`) and `mark` (presentation) are module residuals.
#[test]
fn native_update_corpus() {
    let cases: Vec<NativeCase> = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/temporal_update.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 28);
    let rules = rules();
    for case in cases {
        let name = case.input.name.as_str();
        let mut sim = sim(7);
        let target_kind = match case.input.target.rtti {
            1 => "HTNK",
            6 => "GAPOWR",
            other => panic!("rtti {other}"),
        };
        let target = spawn(&mut sim, &rules, target_kind, "Americans", 20, 20);
        if case.input.target.rtti == 1 {
            place(&mut sim, target, case.input.target.coords);
        }
        let transport = spawn(&mut sim, &rules, "BFRT", "Russians", 4, 4);
        let mut attackers = Vec::new();
        for (n, attacker) in case.input.attackers.iter().enumerate() {
            let kind = match (attacker.damage, attacker.trainable) {
                (8 | 16, 1) => "CLEG",
                (5, _) => "CLEGU",
                (1, 1) => "CLEG1",
                other => panic!("{name}: no fixture type for {other:?}"),
            };
            let id = spawn(&mut sim, &rules, kind, "Russians", 2 + n as u16 % 20, 2);
            if attacker.damage == 16 {
                set_elite(&mut sim, id);
            }
            if attacker.open_topped == 1 {
                sim.substrate.entities.get_mut(id).unwrap().passenger_role =
                    crate::sim::passenger::PassengerRole::Inside {
                        transport_id: transport,
                    };
            }
            if let Some(coords) = attacker.coords {
                place(&mut sim, id, coords);
            }
            put_on_attack(&mut sim, id);
            attackers.push(id);
        }
        chain(&mut sim, target, &attackers, case.input.warp_remaining);
        if case.input.corrupt_head {
            let last = *attackers.last().unwrap();
            let before_last = attackers[attackers.len() - 2];
            let head = attackers[0];
            let set = |sim: &mut Simulation, id: u64, f: &dyn Fn(&mut TemporalLink)| {
                f(sim
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .temporal
                    .link
                    .as_mut()
                    .unwrap());
            };
            set(&mut sim, head, &|link| link.prev = Some(last));
            set(&mut sim, last, &|link| {
                link.next = Some(head);
                link.prev = None;
            });
            set(&mut sim, before_last, &|link| link.next = None);
        }
        let kills_before = house_stats(&sim, "Russians");
        let lost_before = unit_lost_events(&sim);

        sim.temporal_update_head(attackers[0], &rules);

        let events: Vec<&str> = case
            .events
            .iter()
            .map(|event| event[0].as_str().unwrap())
            .collect();
        let native_erase = events.contains(&"uninit");
        assert_eq!(erased(&sim, target), native_erase, "{name}: erase");
        assert_eq!(
            native_name(head_of(&sim, target), target, &attackers),
            case.target_head,
            "{name}: the target's head"
        );
        assert_eq!(
            entity(&sim, target).temporal.is_warped(),
            case.target_warped == 1,
            "{name}: +0x270"
        );
        for (n, (&attacker, native)) in attackers.iter().zip(&case.attackers).enumerate() {
            let link = link(&sim, attacker);
            assert_eq!(
                link.warp_remaining, native.warp_remaining,
                "{name}: temporal{n} WarpRemaining"
            );
            if native_erase && n > 0 {
                // The UnInit's pointer-expiry forward cleared the chain.
                assert_eq!((link.target, link.prev, link.next), (None, None, None));
                continue;
            }
            assert_eq!(
                (
                    native_name(link.target, target, &attackers),
                    native_name(link.prev, target, &attackers),
                    native_name(link.next, target, &attackers),
                ),
                (
                    native.target.clone(),
                    native.prev.clone(),
                    native.next.clone()
                ),
                "{name}: temporal{n} links"
            );
        }
        // The erase's effects, in the corpus's terms.
        assert_eq!(
            warp_away_anims(&sim).len(),
            usize::from(events.contains(&"anim")),
            "{name}: WarpAway"
        );
        if native_erase {
            let at = warp_away_anims(&sim)[0];
            let location = location_coord(entity(&sim, target));
            assert_eq!((at.x, at.y, at.z), (location.x, location.y, location.z));
            assert_eq!(
                veterancy(&sim, attackers[0]) > 0.0,
                events.contains(&"veterancy_add"),
                "{name}: VeterancyStruct::Add"
            );
            let kills = house_stats(&sim, "Russians");
            let (unit_kills, building_kills) = (
                kills.units_killed - kills_before.units_killed,
                kills.buildings_killed - kills_before.buildings_killed,
            );
            assert_eq!(
                (unit_kills, building_kills),
                if target_kind == "HTNK" {
                    (1, 0)
                } else {
                    (0, 1)
                },
                "{name}: Record_The_Kill"
            );
            // Death_Announcement for the human unit owner; a building's
            // +0x3B8 only records a camera cell.
            assert_eq!(
                unit_lost_events(&sim) - lost_before,
                usize::from(target_kind == "HTNK"),
                "{name}: unit lost"
            );
        }
        let idles = events.iter().filter(|&&event| event == "idle").count();
        if idles > 0 {
            // Enter_Idle_Mode(0, 1) on an Attack-mission Foot queues Guard.
            let idled: Vec<u64> = if case.input.corrupt_head {
                attackers.clone()
            } else {
                vec![attackers[0]]
            };
            for id in idled {
                assert_eq!(
                    entity(&sim, id).mission.queued().known(),
                    Some(MissionType::Guard),
                    "{name}: idle"
                );
            }
        }
    }
}

/// The corpus's open-topped cases through the distance helper alone: native
/// holds at 1793 leptons (`Sqrt_Approx(1793^2)` truncates to 1792) and lets
/// go from 1794.
#[test]
fn native_open_topped_boundary_is_distance_3d() {
    let cases: Vec<NativeCase> = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/temporal_update.json"
    ))
    .unwrap();
    let limit = 7 * 256;
    let mut checked = 0;
    for case in cases
        .iter()
        .filter(|case| case.input.name.starts_with("open_topped_"))
    {
        let owner = case.input.attackers[0].coords.unwrap();
        let distance =
            crate::util::native_x87::distance_3d_leptons(case.input.target.coords, owner);
        let released = case.target_head != "temporal0";
        assert_eq!(distance > limit, released, "{}", case.input.name);
        checked += 1;
    }
    assert_eq!(checked, 16);
}

/// InitiateWarp: the first attacker heads the chain with Strength * 10; each
/// later one inserts right after the head. The target is deselected.
#[test]
fn initiate_warp_heads_the_chain_and_inserts_after_the_head() {
    let rules = rules();
    let mut sim = sim(2);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let first = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let second = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 11);
    let third = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 12);
    sim.substrate.entities.get_mut(tank).unwrap().selected = true;

    sim.temporal_initiate_warp(first, Some(tank), &rules);
    assert_eq!(head_of(&sim, tank), Some(first));
    assert_eq!(link(&sim, first).warp_remaining, 4000, "Strength=400 * 10");
    assert!(!entity(&sim, tank).selected, "Deselect");
    assert!(entity(&sim, tank).is_warped_out());

    sim.temporal_initiate_warp(second, Some(tank), &rules);
    sim.temporal_initiate_warp(third, Some(tank), &rules);
    assert_eq!(head_of(&sim, tank), Some(first));
    let (head, a, b) = (link(&sim, first), link(&sim, third), link(&sim, second));
    assert_eq!((head.prev, head.next), (None, Some(third)));
    assert_eq!((a.prev, a.next), (Some(first), Some(second)));
    assert_eq!((b.prev, b.next), (Some(third), None));
    assert_eq!(head.warp_remaining, 4000, "later attackers leave it alone");
}

/// CanWarpTarget refuses an Iron-Curtained target and a `Warpable=no` type,
/// and an attacker being warped cannot start a warp of its own.
#[test]
fn warp_start_refusals() {
    let text = RULES.replace("[GACNST]\n", "[GACNST]\nWarpable=no\n");
    let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
    let mut sim = sim(3);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let yard = spawn(&mut sim, &rules, "GACNST", "Americans", 20, 20);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let other = spawn(&mut sim, &rules, "CLEG", "Allies2", 8, 10);

    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(tank).unwrap(),
        sim.session.binary_frame,
        100,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    sim.temporal_initiate_warp(cleg, Some(tank), &rules);
    assert_eq!(head_of(&sim, tank), None, "Iron Curtain");
    assert!(!entity(&sim, cleg).temporal.is_warping_someone());

    sim.temporal_initiate_warp(cleg, Some(yard), &rules);
    assert_eq!(head_of(&sim, yard), None, "Warpable=no");

    // `other` warps the legionnaire, which then cannot start on the yard's
    // neighbour.
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 26, 10);
    sim.temporal_initiate_warp(other, Some(cleg), &rules);
    assert_eq!(head_of(&sim, cleg), Some(other));
    sim.temporal_initiate_warp(cleg, Some(plant), &rules);
    assert_eq!(head_of(&sim, plant), None, "an attacker being warped");
}

/// A warp's start makes a victim that was itself warping let go
/// (`0x0071B162`), and retargeting drops the previous victim with its
/// progress: nothing heals and the next warp restarts at Strength * 10.
#[test]
fn retarget_and_counter_warp_release() {
    let rules = rules();
    let mut sim = sim(4);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let other_tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 14);
    let red = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let blue = spawn(&mut sim, &rules, "CLEG", "Americans", 8, 10);

    sim.temporal_initiate_warp(red, Some(tank), &rules);
    sim.temporal_update_head(red, &rules);
    assert_eq!(link(&sim, red).warp_remaining, 3992);
    sim.temporal_initiate_warp(red, Some(other_tank), &rules);
    assert_eq!(head_of(&sim, tank), None, "released");
    assert_eq!(head_of(&sim, other_tank), Some(red));
    assert_eq!(link(&sim, red).warp_remaining, 4000, "a fresh start");
    assert_eq!(
        entity(&sim, tank).health.current,
        400,
        "nothing was lost or healed"
    );

    // Blue warps red, which lets its own victim go.
    sim.temporal_initiate_warp(blue, Some(red), &rules);
    assert_eq!(head_of(&sim, red), Some(blue));
    assert_eq!(head_of(&sim, other_tank), None);
    assert!(!entity(&sim, red).temporal.is_warping_someone());
}

/// LetGo: a departing head hands the chain and its progress to the next
/// attacker; a middle attacker unlinks; a lone head frees the target.
#[test]
fn let_go_hands_over_progress_or_frees_the_target() {
    let rules = rules();
    let mut sim = sim(5);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let a = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let b = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 11);
    let c = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 12);
    chain(&mut sim, tank, &[a, b, c], 1234);

    sim.temporal_let_go(b);
    assert_eq!((link(&sim, a).next, link(&sim, c).prev), (Some(c), Some(a)));
    assert_eq!(link(&sim, b), TemporalLink::default().with_remaining(0));

    sim.temporal_let_go(a);
    assert_eq!(head_of(&sim, tank), Some(c));
    assert_eq!(link(&sim, c).warp_remaining, 1234, "progress inherited");
    assert_eq!(link(&sim, c).prev, None);

    sim.temporal_let_go(c);
    assert_eq!(head_of(&sim, tank), None);
    assert!(!entity(&sim, tank).is_warped_out());
}

impl TemporalLink {
    fn with_remaining(mut self, warp_remaining: i32) -> Self {
        self.warp_remaining = warp_remaining;
        self
    }
}

/// The pointer-expiry forward: an attacker's own death lets go (the chain
/// passes to the next attacker); a target's removal clears every link.
#[test]
fn pointer_expiry_passes_the_chain_on() {
    let rules = rules();
    let mut sim = sim(6);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let a = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let b = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 11);
    chain(&mut sim, tank, &[a, b], 777);

    sim.uninit_with_rules(a, &rules);
    assert_eq!(head_of(&sim, tank), Some(b));
    assert_eq!(link(&sim, b).warp_remaining, 777);

    put_on_attack(&mut sim, b);
    sim.uninit_with_rules(tank, &rules);
    assert_eq!(link(&sim, b).target, None);
    assert_eq!(
        entity(&sim, b).mission.queued().known(),
        Some(MissionType::Guard),
        "the target arm idles its attacker"
    );
}

/// ReceiveGunner and RemoveGunner move the TemporalClass between the
/// Chrono Legionnaire and the IFV; the IFV's victim is let go when the
/// gunner leaves.
#[test]
fn the_ifv_borrows_its_gunners_beam() {
    let rules = rules();
    let mut sim = sim(8);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let partner = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 12);
    let ifv = spawn(&mut sim, &rules, "FV", "Russians", 9, 10);

    sim.temporal_receive_gunner(ifv, cleg);
    assert!(!entity(&sim, cleg).temporal.has_link());
    assert!(entity(&sim, ifv).temporal.has_link());

    // The IFV heads a chain; its partner follows.
    sim.temporal_initiate_warp(ifv, Some(tank), &rules);
    sim.temporal_initiate_warp(partner, Some(tank), &rules);
    assert_eq!(head_of(&sim, tank), Some(ifv));
    assert_eq!(link(&sim, partner).prev, Some(ifv));

    sim.temporal_remove_gunner(ifv, cleg);
    assert!(!entity(&sim, ifv).temporal.has_link());
    assert!(entity(&sim, cleg).temporal.has_link());
    assert!(
        !entity(&sim, cleg).temporal.is_warping_someone(),
        "RemoveGunner lets go"
    );
    assert_eq!(head_of(&sim, tank), Some(partner), "the chain continues");
}

/// A warped building is offline: it gives no power, is not operational, and
/// admits no garrison; its release restores all three.
#[test]
fn a_warped_building_goes_offline() {
    let rules = rules();
    let mut sim = sim(9);
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 12, 10);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let now = sim.session.binary_frame;
    let _ = sim.mission_assign_exact(plant, MissionId::from_known(MissionType::Guard), now);
    let americans = sim.interner.get("Americans").unwrap();
    let output = |sim: &mut Simulation| {
        let _ = crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
        );
        sim.power_states
            .get(&americans)
            .map_or(0, |state| state.total_output)
    };
    assert_eq!(output(&mut sim), 100);
    assert_eq!(sim.building_operational_state(plant, &rules), Some(true));

    sim.temporal_initiate_warp(cleg, Some(plant), &rules);
    assert!(!entity(&sim, plant).building_online());
    assert_eq!(output(&mut sim), 0, "GetPowerOutput 0x0044E7C7");
    assert_eq!(sim.building_operational_state(plant, &rules), Some(false));

    sim.temporal_let_go(cleg);
    assert!(entity(&sim, plant).building_online());
    assert_eq!(output(&mut sim), 100);
    assert_eq!(sim.building_operational_state(plant, &rules), Some(true));
}

/// A warped object is frozen: its head's Update runs from its own AI turn,
/// it drops its target and destination, takes no damage, and sparkles every
/// 24th frame.
#[test]
fn a_warped_object_is_frozen_and_immune() {
    let rules = rules();
    let (mut sim, _grid) = arena(10, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Russians", 12, 12);
    sim.substrate.entities.get_mut(tank).unwrap().attack_target =
        Some(crate::sim::combat::AttackTarget::new(gi));
    sim.temporal_initiate_warp(cleg, Some(tank), &rules);

    sim.session.binary_frame = 48;
    assert!(sim.temporal_ai_prologue(tank, &rules), "frozen");
    assert_eq!(link(&sim, cleg).warp_remaining, 3992, "one step");
    assert!(entity(&sim, tank).attack_target.is_none(), "TarCom dropped");
    let sparkles: Vec<AnimWorldCoord> = sim
        .substrate
        .anims
        .iter()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "CHRONOSK")
        .map(|(&id, _)| sim.anim_absolute_coord(id).unwrap())
        .collect();
    let location = location_coord(entity(&sim, tank));
    assert_eq!(sparkles.len(), 1, "frame 48 sparkles");
    assert_eq!(
        (sparkles[0].x, sparkles[0].y, sparkles[0].z),
        (location.x + 0x78, location.y + 0x78, location.z)
    );
    sim.session.binary_frame = 49;
    assert!(sim.temporal_ai_prologue(tank, &rules));
    assert_eq!(
        sim.substrate
            .anims
            .iter()
            .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "CHRONOSK")
            .count(),
        1,
        "frame 49 does not"
    );

    // TechnoClass::ReceiveDamage 0x00701AB1: a warped receiver takes nothing
    // unless defenses are ignored.
    let warhead = sim.interner.intern("AP");
    let hit = |ignore_defenses| {
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            tank,
            100,
            0,
            gi,
            None,
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses,
                arg6: false,
            },
        )
    };
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit(false)]);
    assert_eq!(entity(&sim, tank).health.current, 400, "immune");
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit(true)]);
    assert_eq!(entity(&sim, tank).health.current, 300, "ignoreDefenses");
}

/// Through the production fire path: a Chrono Legionnaire ordered to attack a
/// Rhino warps it with no damage and erases it after Strength * 10 / Damage
/// = 500 of the tank's AI turns. Nothing survives, WarpAway plays at the
/// tank's Location, the kill is scored and the legionnaire gains experience
/// twice (the Update's Add and Record_The_Kill's).
#[test]
fn a_chrono_legionnaire_erases_a_tank() {
    use crate::sim::command::{Command, CommandEnvelope};
    let rules = rules();
    let (mut sim, grid) = arena(11, &rules);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 13, 10);
    let owner = sim.interner.intern("Russians");
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: cleg,
            target_id: tank,
        },
    ));
    let mut started = None;
    let mut erased_after = None;
    for frame in 0..700 {
        let commands = sim.take_due_commands();
        sim.advance_tick(
            &commands,
            Some(&rules),
            &std::collections::BTreeMap::new(),
            Some(&grid),
            None,
            33,
        );
        if started.is_none() && head_of(&sim, tank) == Some(cleg) {
            started = Some(frame);
            assert_eq!(entity(&sim, tank).health.current, 400, "no damage");
        }
        if erased(&sim, tank) {
            erased_after = Some(frame);
            break;
        }
    }
    let started = started.expect("the warp starts");
    let erased_after = erased_after.expect("the tank is erased");
    assert_eq!(
        erased_after - started,
        500,
        "one step per tank AI turn, the first on the turn after the shot"
    );
    assert_eq!(warp_away_anims(&sim).len(), 1);
    let stats = house_stats(&sim, "Russians");
    assert_eq!(stats.units_killed, 1);
    assert_eq!(house_stats(&sim, "Americans").units_lost, 1);
    assert!(veterancy(&sim, cleg) > 0.0);
    assert!(!entity(&sim, cleg).temporal.is_warping_someone());
    assert!(
        !sim.substrate.entities.values().any(|entity| entity.category
            == crate::map::entities::EntityCategory::Infantry
            && entity.owner() == sim.interner.get("Americans").unwrap()),
        "no survivors"
    );
}

/// GetFireError `0x006FC5D5`: only Temporal shots may hit a warped target.
/// A GI ordered onto the warped tank fires nothing and loses the target; the
/// tank takes no damage.
#[test]
fn only_temporal_fire_reaches_a_warped_target() {
    use crate::sim::command::{Command, CommandEnvelope};
    let rules = rules();
    let (mut sim, grid) = arena(14, &rules);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 13, 10);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Russians", 13, 13);
    sim.temporal_initiate_warp(cleg, Some(tank), &rules);
    let owner = sim.interner.intern("Russians");
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: gi,
            target_id: tank,
        },
    ));
    for _ in 0..40 {
        let commands = sim.take_due_commands();
        sim.advance_tick(
            &commands,
            Some(&rules),
            &std::collections::BTreeMap::new(),
            Some(&grid),
            None,
            33,
        );
    }
    assert_eq!(head_of(&sim, tank), Some(cleg), "still warped");
    assert_eq!(entity(&sim, tank).health.current, 400);
    assert!(
        entity(&sim, gi)
            .attack_target
            .as_ref()
            .is_none_or(|attack| attack.target != crate::sim::combat::TargetKind::Entity(tank)),
        "the illegal target is dropped"
    );
}

/// Moving away lets the target go at the first cell entered; the tank keeps
/// its health and its AI resumes.
#[test]
fn moving_releases_the_target() {
    use crate::sim::command::{Command, CommandEnvelope};
    let rules = rules();
    let (mut sim, grid) = arena(12, &rules);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 13, 10);
    let owner = sim.interner.intern("Russians");
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: cleg,
            target_id: tank,
        },
    ));
    let step = |sim: &mut Simulation, commands: &[CommandEnvelope]| {
        sim.advance_tick(
            commands,
            Some(&rules),
            &std::collections::BTreeMap::new(),
            Some(&grid),
            None,
            33,
        );
    };
    for _ in 0..200 {
        let commands = sim.take_due_commands();
        step(&mut sim, &commands);
        if head_of(&sim, tank).is_some() {
            break;
        }
    }
    assert_eq!(head_of(&sim, tank), Some(cleg));
    for _ in 0..30 {
        let commands = sim.take_due_commands();
        step(&mut sim, &commands);
    }
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Move {
            entity_id: cleg,
            target_rx: 4,
            target_ry: 10,
            queue: false,
            group_id: None,
        },
    ));
    for _ in 0..120 {
        let commands = sim.take_due_commands();
        step(&mut sim, &commands);
        if head_of(&sim, tank).is_none() {
            break;
        }
    }
    assert_eq!(head_of(&sim, tank), None, "released on the move");
    assert!(!erased(&sim, tank));
    assert_eq!(entity(&sim, tank).health.current, 400);
}

/// A snapshot taken mid-warp restores the chain and the head.
#[test]
fn a_warp_in_progress_survives_a_snapshot() {
    let rules = rules();
    let mut sim = sim(13);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
    let a = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 10);
    let b = spawn(&mut sim, &rules, "CLEG", "Russians", 10, 11);
    sim.temporal_initiate_warp(a, Some(tank), &rules);
    sim.temporal_initiate_warp(b, Some(tank), &rules);
    sim.temporal_update_head(a, &rules);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "temporal", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("temporal links resolve");
    let hash = restored.state_hash();
    assert_eq!(
        restored.substrate.entities.get(tank).unwrap().temporal,
        entity(&sim, tank).temporal
    );
    assert_eq!(
        restored.substrate.entities.get(a).unwrap().temporal,
        entity(&sim, a).temporal
    );
    assert_ne!(
        {
            let mut other = restored;
            other.temporal_let_go(b);
            other.state_hash()
        },
        hash,
        "the chain is hashed"
    );
}

/// A flat 32x32 clear map with its playfield, zones and path grid.
fn arena(seed: u64, rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = sim(seed);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    (sim, grid)
}
