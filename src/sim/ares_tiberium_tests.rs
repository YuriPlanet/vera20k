//! Ares tiberium damage (10049280, hook 0x4D85E4) with its visceroid
//! (100468B0) and heal (10014BA0, hook 0x4DA53E) through their production
//! owners.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::game_entity::GameEntity;
use crate::sim::miner::{CargoBale, ResourceType};
use crate::sim::movement::PerCellReason;
use crate::sim::tiberium::test_support::{
    TEST_GRID_SIZE, flat_terrain, place_tiberium_on_map, tiberium_rules_text,
};

/// Ore on (5, 5) whose Riparius carries `riparius` keys, and `objects`
/// (`(type, category)`) all standing on it.
fn scene(riparius: &str, objects: &[(&str, EntityCategory)]) -> (Simulation, RuleSet, Vec<u64>) {
    scene_from(riparius, "", objects)
}

/// [`scene`] with `harvester` keys (and sections after them) on the `HARV`
/// vehicle and Riparius `Power=4`.
fn scene_with_harvester(harvester: &str) -> (Simulation, RuleSet, Vec<u64>) {
    scene_from("Power=4\n", harvester, &[])
}

fn scene_from(
    riparius: &str,
    harvester: &str,
    objects: &[(&str, EntityCategory)],
) -> (Simulation, RuleSet, Vec<u64>) {
    let tiberium =
        tiberium_rules_text().replacen("[Riparius]\n", &format!("[Riparius]\n{riparius}"), 1);
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nTiberiumDamageEnabled=yes\nTiberiumHealEnabled=yes\nRepairStep=8\n\
         [InfantryTypes]\n0=E1\n1=PROOF\n[VehicleTypes]\n0=HEALER\n1=HARV\n\
         [E1]\nStrength=100\n[PROOF]\nStrength=100\nTiberiumProof=yes\n\
         [HEALER]\nStrength=300\nTiberiumHeal=yes\n\
         [Warheads]\n0=TIBWH\n[TIBWH]\nCellSpread=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         {tiberium}\n[HARV]\nStrength=300\nStorage=20\n{harvester}"
    )))
    .unwrap();
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(flat_terrain(16, 16));
    place_tiberium_on_map(&mut sim, (5, 5), ResourceType::Ore, 6);
    let ids = objects
        .iter()
        .map(|&(name, category)| {
            let id = sim.substrate.allocate_stable_id();
            let mut entity = GameEntity::test_default_of_category(id, name, "A", 5, 5, category);
            entity.set_type_ref_for_test(sim.interner.intern(name));
            entity.set_owner_for_test(sim.interner.intern("A"));
            entity.lifecycle.set_object_alive_for_test(true);
            sim.substrate.entities.insert(entity);
            id
        })
        .collect();
    (sim, rules, ids)
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.substrate.entities.get(id).unwrap().health.current()
}

/// A cell arrival on ore hurts infantry by the mineral's `Damage` with its
/// `Warhead`; `TiberiumProof=yes` (and the default for vehicles) spares it.
#[test]
fn per_cell_arrival_on_tiberium_applies_the_mineral_damage() {
    let (mut sim, rules, ids) = scene(
        "Damage=7\nWarhead=TIBWH\n",
        &[
            ("E1", EntityCategory::Infantry),
            ("PROOF", EntityCategory::Infantry),
            ("HEALER", EntityCategory::Unit),
        ],
    );
    for &id in &ids {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(100);
        sim.per_cell_process(
            id,
            PerCellReason::Arrival,
            &rules,
            Some(rules.overlay_registry()),
            FrameEffects::default(),
        )
        .unwrap();
    }
    assert_eq!(health(&sim, ids[0]), 93);
    assert_eq!(health(&sim, ids[1]), 100, "TiberiumProof=yes");
    assert_eq!(health(&sim, ids[2]), 100, "vehicles default to proof");
}

/// A hurt `TiberiumHeal=yes` unit on a Tiberium cell heals `Heal.UStep` on
/// frames divisible by `ftol(TiberiumHeal * 900)` (1/60 min: 14 under the
/// process's chop control word), up to Strength; infantry without the flag
/// does not.
#[test]
fn tiberium_heal_steps_on_the_delay_cadence_up_to_strength() {
    let (mut sim, rules, ids) = scene(
        "Heal.UStep=11\n",
        &[
            ("HEALER", EntityCategory::Unit),
            ("E1", EntityCategory::Infantry),
        ],
    );
    let (unit, infantry) = (ids[0], ids[1]);
    sim.substrate
        .entities
        .get_mut(unit)
        .unwrap()
        .health
        .set_for_test(100);
    sim.substrate
        .entities
        .get_mut(infantry)
        .unwrap()
        .health
        .set_for_test(50);
    sim.session.set_binary_frame_for_test(30);
    sim.ares_foot_tiberium_heal(unit, &rules);
    assert_eq!(health(&sim, unit), 100, "off the 14-frame cadence");
    sim.session.set_binary_frame_for_test(28);
    sim.ares_foot_tiberium_heal(unit, &rules);
    sim.ares_foot_tiberium_heal(infantry, &rules);
    assert_eq!(health(&sim, unit), 111);
    assert_eq!(health(&sim, infantry), 50, "no TiberiumHeal=");
    sim.substrate
        .entities
        .get_mut(unit)
        .unwrap()
        .health
        .set_for_test(295);
    sim.ares_foot_tiberium_heal(unit, &rules);
    assert_eq!(health(&sim, unit), 300);
}

/// Kill `id` with one direct `TIBWH` hit.
fn kill(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let warhead = sim.interner.intern("TIBWH");
    let event = EntityDamageEvent::direct_receiver(
        id,
        10_000,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_direct_damage_receiver(
        rules,
        Some(rules.overlay_registry()),
        event,
        FrameEffects::default(),
    );
    assert!(
        sim.substrate
            .entities
            .get(id)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    );
}

/// The ore density over the 3x3 block around (8, 8), clear of the scene's
/// ore on (5, 5).
fn ore_around(sim: &Simulation) -> u32 {
    (7..=9)
        .flat_map(|x| (7..=9).map(move |y| (x, y)))
        .map(|(x, y)| u32::from(crate::sim::tiberium::test_support::bales_at(sim, x, y)))
        .sum()
}

/// A unit standing on (8, 8) carrying `bales` of Riparius.
fn harvester(sim: &mut Simulation, rules: &RuleSet, name: &str, bales: u16) -> u64 {
    let id = sim.substrate.allocate_stable_id();
    let mut entity =
        GameEntity::test_default_of_category(id, name, "A", 8, 8, EntityCategory::Unit);
    entity.set_type_ref_for_test(sim.interner.intern(name));
    entity.set_owner_for_test(sim.interner.intern("A"));
    entity.lifecycle.set_object_alive_for_test(true);
    let bale = CargoBale::of_type(TiberiumTypeId(0), ResourceType::Ore, rules).unwrap();
    let mut miner =
        crate::sim::miner::Miner::new(crate::sim::miner::MinerKind::War, &Default::default(), 20);
    miner.add_bales(bale, bales);
    entity.miner = Some(Box::new(miner));
    sim.substrate.entities.insert(entity);
    id
}

/// Ares 1004AEC0 (hook 0x702200) and 1004B050 (hook 0x702216): a dying
/// `TiberiumSpill=yes` harvester spills ore around its cell; a
/// `TiberiumRemains=yes` one leaves ore there even when empty; a plain one
/// leaves nothing.
#[test]
fn dying_spill_and_remains_types_leave_ore_around_their_cell() {
    for (keys, bales, leaves_ore) in [
        ("TiberiumSpill=yes\n", 10, true),
        ("TiberiumRemains=yes\n", 0, true),
        ("", 10, false),
    ] {
        let (mut sim, rules, _) = scene_with_harvester(keys);
        let id = harvester(&mut sim, &rules, "HARV", bales);
        let before = sim.rng_state();
        kill(&mut sim, &rules, id);
        assert_eq!(ore_around(&sim) > 0, leaves_ore, "{keys:?}");
        if leaves_ore {
            assert_ne!(sim.rng_state(), before, "{keys:?} draws RandomRanged(0, 2)");
        }
    }
}

/// Ares 1004BD90 (hook 0x738749): with `TiberiumExplosive=yes` a dying
/// harvester's bales times Power go off as one Apply_area_damage with
/// `TiberiumExplosiveWarhead` that leaves tiberium alone.
#[test]
fn tiberium_explosive_harvester_blasts_bales_times_power() {
    let (mut sim, rules, _) = scene_with_harvester(
        "\n[CombatDamage]\nTiberiumExplosive=yes\nTiberiumExplosiveWarhead=TIBWH\n",
    );
    let id = harvester(&mut sim, &rules, "HARV", 10);
    let (_, visits) =
        crate::sim::combat::receiver_fixture::trace_area_visits(|| kill(&mut sim, &rules, id));
    // 10 bales of Power=4 Riparius.
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0]["damage"], 40);
    assert_eq!(visits[0]["warhead"], "TIBWH");
    assert_eq!(visits[0]["affects_tiberium"], false);
}

/// Ore on (5, 5) that kills a Strength-100 `E1` standing on it, with
/// `[General] SmallVisceroid=VISC`, a 100% `TiberiumTransmogrify` and a
/// registered Neutral house.
fn visceroid_scene() -> (Simulation, RuleSet, u64) {
    let tiberium = tiberium_rules_text().replacen(
        "[Riparius]\n",
        "[Riparius]\nDamage=150\nWarhead=TIBWH\n",
        1,
    );
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nTiberiumDamageEnabled=yes\nTiberiumTransmogrify=100\n\
         SmallVisceroid=VISC\n[Countries]\n0=Americans\n1=Neutral\n\
         [Americans]\n[Neutral]\n\
         [InfantryTypes]\n0=E1\n[VehicleTypes]\n0=VISC\n\
         [E1]\nStrength=100\n[VISC]\nStrength=200\nSmallVisceroid=yes\n\
         [Warheads]\n0=TIBWH\n[TIBWH]\nCellSpread=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         {tiberium}\n"
    )))
    .unwrap();
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(flat_terrain(TEST_GRID_SIZE, TEST_GRID_SIZE));
    place_tiberium_on_map(&mut sim, (5, 5), ResourceType::Ore, 6);
    for (house, country) in [("A", "Americans"), ("Neutral", "Neutral")] {
        let name = sim.interner.intern(house);
        let country = sim.interner.intern(country);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                name,
                0,
                Some(country),
                false,
                0,
                10,
            ));
    }
    let id = sim.substrate.allocate_stable_id();
    let mut entity =
        GameEntity::test_default_of_category(id, "E1", "A", 5, 5, EntityCategory::Infantry);
    entity.set_type_ref_for_test(sim.interner.intern("E1"));
    entity.set_owner_for_test(sim.interner.intern("A"));
    entity.lifecycle.set_object_alive_for_test(true);
    sim.substrate.entities.insert(entity);
    (sim, rules, id)
}

fn visceroids(sim: &Simulation) -> Vec<(String, u16, u16)> {
    let visc = sim.interner.get("VISC");
    sim.substrate
        .entities
        .values()
        .filter(|entity| Some(entity.type_ref()) == visc)
        .map(|entity| {
            (
                sim.interner.resolve(entity.owner()).to_owned(),
                entity.position.rx(),
                entity.position.ry(),
            )
        })
        .collect()
}

/// Ares 10049280 -> SpawnVisceroid 100468B0: infantry the ore kills
/// becomes the Neutral house's SmallVisceroid in its cell on a map that
/// keeps the fresh `TiberiumDeathToVisceroid=yes`.
#[test]
fn tiberium_death_spawns_a_neutral_small_visceroid() {
    let (mut sim, rules, id) = visceroid_scene();
    let killed = sim.ares_foot_tiberium_damage(
        id,
        &rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    );
    assert!(killed);
    assert_eq!(visceroids(&sim), [("Neutral".to_owned(), 5, 5)]);
}

/// SpawnVisceroid 100468B0: a map with `TiberiumDeathToVisceroid=no`
/// draws nothing and creates nothing; with the flag, a unit in the cell
/// (occupation bit 0x20) still costs the `RandomRanged(0, 99)` draw but
/// blocks the visceroid.
#[test]
fn visceroid_needs_the_map_flag_and_a_cell_without_a_unit() {
    let (mut sim, rules, _) = visceroid_scene();
    let coord = DriveCoord {
        x: 5 * 256 + 128,
        y: 5 * 256 + 128,
        z: 0,
    };
    let registry = Some(rules.overlay_registry());
    sim.session
        .read_map_tiberium_death_to_visceroid(Some(false));
    let before = sim.rng_state();
    assert!(!sim.ares_spawn_visceroid(
        coord,
        "VISC",
        100,
        false,
        &rules,
        registry,
        FrameEffects::default()
    ));
    assert_eq!(sim.rng_state(), before, "no draw without the map flag");

    sim.session.read_map_tiberium_death_to_visceroid(None);
    sim.substrate.raw_cell_occupation.mark_ground(
        5,
        5,
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
    );
    assert!(!sim.ares_spawn_visceroid(
        coord,
        "VISC",
        100,
        false,
        &rules,
        registry,
        FrameEffects::default()
    ));
    assert_ne!(sim.rng_state(), before, "RandomRanged(0, 99)");
    assert!(visceroids(&sim).is_empty());
}

/// Ares CellChainReact (10050DD0) from Apply_area_damage's spread walk
/// (hook 0x48964F): a `Tiberium=yes` blast over a `ChainReaction=yes` ore
/// cell at density 20 always reacts (`RandomRanged(0, 99) < 100`). The cell
/// loses half its density, and `Power * 10` goes off as a nested blast with
/// the mineral's ExplosionWarhead that leaves tiberium alone. A cell without
/// `ChainReaction=yes` keeps its ore.
#[test]
fn chain_reacting_ore_halves_and_blasts_power_times_the_loss() {
    for chain_reaction in [true, false] {
        let mut tiberium = tiberium_rules_text().replacen(
            "[Riparius]\n",
            "[Riparius]\nPower=4\nExplosionWarhead=TIBWH\n",
            1,
        );
        if chain_reaction {
            tiberium = tiberium.replace(
                "Tiberium=yes\nLand=Tiberium\n",
                "Tiberium=yes\nLand=Tiberium\nChainReaction=yes\n",
            );
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Warheads]\n0=TIBWH\n[TIBWH]\nCellSpread=1\nTiberium=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{tiberium}"
        )))
        .unwrap();
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(flat_terrain(TEST_GRID_SIZE, TEST_GRID_SIZE));
        place_tiberium_on_map(&mut sim, (5, 5), ResourceType::Ore, 6);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .set_overlay_data(5, 5, 20);
        let warhead = rules.warhead("TIBWH").unwrap();
        let warhead_ref = sim.interner.intern("TIBWH");
        let (_, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
            crate::sim::combat::world_receiver::apply_area_damage(
                &mut sim,
                &rules,
                Some(rules.overlay_registry()),
                crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
                5,
                warhead,
                true,
                (RAD_NO_ATTACKER, None, warhead_ref),
                FrameEffects::default(),
            )
        });
        let density = sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_data;
        if chain_reaction {
            assert_eq!(density, 10);
            assert_eq!(visits.len(), 2);
            assert_eq!(visits[1]["damage"], 40);
            assert_eq!(visits[1]["warhead"], "TIBWH");
            assert_eq!(visits[1]["affects_tiberium"], false);
        } else {
            assert_eq!(density, 20);
            assert_eq!(visits.len(), 1);
        }
    }
}
