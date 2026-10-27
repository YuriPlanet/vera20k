//! `CellClass::PickupCrate @ 0x00481A00` through the complete dispatcher.
//!
//! The original-code rows (`tools/spatial_oracle/crate_pickup.json`, whose
//! sidecar bounds them to the guards, selection, removal and the Speed arm)
//! run through [`pickup_crate`] itself; the other arms are Rust regressions
//! against the native reading cited at each arm.

use super::super::tests::{crate_cells, crate_registry, crate_ruleset, sim_with_grid};
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::Health;
use crate::sim::combat::inviso_scatter::{RandomDirectionSnap, random_direction_coord};
use crate::sim::crates::CrateSlot;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::world::{PlacementEvidence, RevealPosition, RevealRequest, SimSoundEvent};
use crate::util::lepton::lepton_to_cell;
use crate::util::native_x87::NativeF64Bits;

const CELL: (i16, i16) = (10, 10);
const STOCK_WEIGHTS: [i32; POWERUP_COUNT] = [
    20, 20, 10, 0, 0, 0, 0, 0, 10, 10, 10, 10, 0, 0, 20, 0, 0, 0, 0,
];

fn house(sim: &mut Simulation, name: &str, player_control: bool) -> InternedId {
    let id = sim.interner.intern(name);
    let mut house = HouseState::new(id, 0, None, false, 0, 10);
    house.set_player_control_for_test(player_control);
    sim.houses.register(house);
    id
}

/// A revealed Ground-layer object at the centre of `cell`.
fn place_actor(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    cell: (u16, u16),
    category: EntityCategory,
    type_name: &str,
) -> u64 {
    let type_id = sim.interner.intern(type_name);
    let id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        cell.0,
        cell.1,
        0,
        0,
        owner,
        Health::new(100),
        type_id,
        category,
        0,
        5,
        true,
    );
    entity.position.set_sub_x_for_test(SimFixed::from_num(128));
    entity.position.set_sub_y_for_test(SimFixed::from_num(128));
    sim.substrate.entities.insert(entity);
    sim.try_reveal_entity_with_context(
        id,
        RevealRequest {
            position: RevealPosition::new(
                cell.0,
                cell.1,
                0,
                None,
                SimFixed::from_num(128),
                SimFixed::from_num(128),
            ),
            placement: PlacementEvidence::MarkSucceeded,
            logic_eligible: true,
        },
        crate::sim::world::UninitContext::with_rules(rules),
    );
    id
}

fn place_crate(sim: &mut Simulation, registry: &OverlayTypeRegistry, cell: (i16, i16), slot: u8) {
    let wood = registry.id_for_name("WOOD").unwrap();
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(cell.0 as u16, cell.1 as u16, wood, slot);
}

/// A placed crate with its runtime slot, as the placer leaves one: multiplayer
/// removal (`0x0056C020`) clears only a cell some slot holds.
fn place_registered_crate(
    sim: &mut Simulation,
    registry: &OverlayTypeRegistry,
    cell: (i16, i16),
    slot: u8,
) {
    place_crate(sim, registry, cell, slot);
    let index = (0..crate::sim::crates::state::CRATE_SLOT_CAPACITY)
        .find(|&index| sim.crate_authority.slots()[index].is_empty())
        .expect("a free crate slot");
    *sim.crate_authority.slot_mut_for_test(index) = CrateSlot {
        start_frame: 50,
        aux: 777,
        duration: 100,
        cell_x: cell.0,
        cell_y: cell.1,
    };
}

fn overlay_at(sim: &Simulation, cell: (i16, i16)) -> Option<u8> {
    sim.overlay_grid
        .as_ref()
        .unwrap()
        .cell(cell.0 as u16, cell.1 as u16)
        .overlay_id
}

fn sounds(sim: &Simulation) -> Vec<(String, Option<[InternedId; 2]>)> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                ..
            } => Some((sound_id.clone(), *audible_to)),
            _ => None,
        })
        .collect()
}

fn eva_events(sim: &Simulation) -> Vec<(InternedId, &'static str)> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::HouseEva { owner, event } => Some((*owner, *event)),
            _ => None,
        })
        .collect()
}

/// A multiplayer pickup fixture on the crate test grid: one crate at `CELL`
/// holding `slot`, one Speed-style actor standing on it.
fn multiplayer_fixture(slot: u8) -> (Simulation, RuleSet, OverlayTypeRegistry, InternedId, u64) {
    let mut sim = sim_with_grid(31);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().crates = false;
    // Bases off: a house with no building and the credits of one Money crate
    // would otherwise be pre-empted onto the Unit slot (`0x00481BB8`).
    sim.session.game_options_mut_for_test().bases = false;
    let mut rules = crate_ruleset("");
    rules.powerups.weights = STOCK_WEIGHTS;
    rules.crate_rules.radius = 768;
    sim.session.set_binary_frame_for_test(100);
    sim.playfield_bounds_mut_for_test().as_mut().unwrap().base = 10;
    sim.set_playfield_size_height_for_test(Some(10));
    let registry = crate_registry();
    place_registered_crate(&mut sim, &registry, CELL, slot);
    let owner = house(&mut sim, "H0", false);
    let actor = place_actor(
        &mut sim,
        &rules,
        owner,
        (CELL.0 as u16, CELL.1 as u16),
        EntityCategory::Infantry,
        "E1",
    );
    (sim, rules, registry, owner, actor)
}

/// Every completed original row (`returned` recorded) through the dispatcher:
/// the guards, the stored or drawn selection, the slot and overlay removal,
/// the Speed recipient's factor and the entire Scenario RNG state.
#[test]
fn original_pickup_rows_match_through_the_dispatcher() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/crate_pickup.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in rows {
        let Some(returned) = row["returned"].as_u64() else {
            continue;
        };
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        assert!(
            input.get("land").is_none(),
            "{name}: land rows need terrain"
        );
        let cell = input["cell"].as_array().map_or(CELL, |v| {
            (v[0].as_i64().unwrap() as i16, v[1].as_i64().unwrap() as i16)
        });
        let seed = input["seed"].as_u64().unwrap_or(31);
        let mut sim = sim_with_grid(seed);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["mode"].as_i64() != Some(0),
            ),
        );
        // `0x00A8B261` is zero in the fixture: no replacement placer.
        sim.session.game_options_mut_for_test().crates = false;
        sim.session.set_binary_frame_for_test(100);
        sim.playfield_bounds_mut_for_test().as_mut().unwrap().base = 10;
        sim.set_playfield_size_height_for_test(Some(10));
        let mut rules = crate_ruleset("");
        rules.powerups.weights = STOCK_WEIGHTS;
        if let Some(weights) = input["weights"].as_array() {
            rules.powerups.weights = std::array::from_fn(|i| weights[i].as_i64().unwrap() as i32);
        }
        let choices = input["solo_choices"].as_array().map_or([2, 10, 0], |v| {
            std::array::from_fn(|i| v[i].as_u64().unwrap() as usize)
        });
        rules.crate_rules.silver_crate = choices[0];
        rules.crate_rules.wood_crate = choices[1];
        rules.crate_rules.water_crate = choices[2];
        rules.crate_rules.solo_crate_money = input["solo_money"].as_i64().unwrap_or(2000) as i32;
        rules.crate_rules.radius = 768;
        let multiplier = input["multiplier"].as_f64().unwrap_or(1.2);
        rules.powerups.magnitudes[POWERUP_SPEED] = NativeF64Bits::from_bits(multiplier.to_bits());
        if input["different_image"] == true {
            rules.crate_rules.wood_crate_img = Some("SILVER".into());
        }
        if input["same_images"] == true {
            rules.crate_rules.crate_img = Some("WOOD".into());
            rules.crate_rules.water_crate_img = Some("WOOD".into());
        }
        let registry = crate_registry();
        let wood = registry.id_for_name("WOOD").unwrap();
        if input["overlay"].as_i64() != Some(-1) {
            let image = if input["crate"].as_i64() == Some(0) {
                registry.id_for_name("TIB01").unwrap()
            } else {
                wood
            };
            sim.overlay_grid.as_mut().unwrap().place_overlay(
                cell.0 as u16,
                cell.1 as u16,
                image,
                input["selection"].as_u64().unwrap_or(10) as u8,
            );
        }
        for (index, start, aux, duration) in [(0, 50, 777, 100), (1, 60, 888, 200)] {
            let occupied = if index == 0 {
                input["registered"] != false
            } else {
                input["duplicate_slot"] == true
            };
            *sim.crate_authority.slot_mut_for_test(index) = if occupied {
                CrateSlot {
                    start_frame: start,
                    aux,
                    duration,
                    cell_x: cell.0,
                    cell_y: cell.1,
                }
            } else {
                CrateSlot {
                    start_frame: 0,
                    ..CrateSlot::default()
                }
            };
        }
        let owner = sim.interner.intern("H0");
        let mut owner_house = HouseState::new(owner, 0, None, false, 0, 10);
        owner_house.set_multiplay_passive_for_test(input["passive"].as_i64() == Some(1));
        sim.houses.register(owner_house);
        let actor = if input["null_actor"] == true {
            99
        } else {
            // The fixture's one actor always stands at (10, 10)'s centre,
            // whatever cell the crate occupies.
            place_actor(
                &mut sim,
                &rules,
                owner,
                (10, 10),
                EntityCategory::Infantry,
                "E1",
            )
        };
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_before"].as_str().unwrap(),
            "{name}"
        );

        let answer = pickup_crate(
            &mut sim,
            &rules,
            &registry,
            cell,
            actor,
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(u64::from(answer), returned, "{name}: AL");
        assert_eq!(
            overlay_at(&sim, cell),
            (row["overlay"] != -1).then_some(if input["crate"].as_i64() == Some(0) {
                registry.id_for_name("TIB01").unwrap()
            } else {
                wood
            }),
            "{name}: overlay"
        );
        let slot = sim.crate_authority.slots()[0];
        assert_eq!(
            serde_json::json!([
                slot.start_frame,
                slot.aux,
                slot.duration,
                slot.cell_x,
                slot.cell_y
            ]),
            row["slot"],
            "{name}: slot"
        );
        if input["null_actor"] != true {
            let factor = sim
                .substrate
                .entities
                .get(actor)
                .unwrap()
                .foot_speed
                .crate_multiplier();
            assert_eq!(
                serde_json::json!([format!("{:016x}", factor.bits())]),
                row["factors"],
                "{name}: Speed recipient"
            );
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{name}: RNG"
        );
        compared += 1;
    }
    assert_eq!(compared, 16);
}

/// `0x00482463..0x00482560`: `RandomRanged(ftol(magnitude), +900)` on the
/// Scenario RNG, `Add_Credits` on the actor's house, the money sound for it.
#[test]
fn money_crate_draws_once_pays_the_owner_and_sounds() {
    let (mut sim, mut rules, registry, owner, actor) = multiplayer_fixture(0);
    rules.powerups.magnitudes[POWERUP_MONEY] = NativeF64Bits::from_bits(2000.0_f64.to_bits());
    rules.general.crate_money_sound = Some("CrateMoney".into());
    let mut probe = sim.scenario_rng.clone();
    let expected = probe.next_range_i32_inclusive(2000, 2900);
    assert!((2000..=2900).contains(&expected));

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(sim.houses[&owner].economy().credits(), expected);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        probe.native_state_hex()
    );
    assert_eq!(overlay_at(&sim, CELL), None);
    assert_eq!(
        sounds(&sim),
        vec![("CrateMoney".to_string(), Some([owner, owner]))]
    );
}

/// `0x00481D86..0x00481DB3`: outside game mode 0 with the lobby Crates option
/// the pickup's removal is followed by one random replacement.
#[test]
fn multiplayer_pickup_places_one_replacement_crate() {
    let (mut sim, rules, registry, _owner, actor) = multiplayer_fixture(0);
    sim.session.game_options_mut_for_test().crates = true;
    *sim.crate_authority.slot_mut_for_test(0) = CrateSlot {
        start_frame: 0,
        aux: 0,
        duration: 100,
        cell_x: CELL.0,
        cell_y: CELL.1,
    };

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    let cells = crate_cells(&sim, &registry);
    assert_eq!(cells.len(), 1, "one replacement crate: {cells:?}");
    assert!(!sim.crate_authority.slots()[0].is_empty());
    let (x, y) = cells[0];
    assert_eq!(
        (
            sim.crate_authority.slots()[0].cell_x,
            sim.crate_authority.slots()[0].cell_y
        ),
        (x as i16, y as i16)
    );
}

fn effect_rules(extra: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nBaseUnit=AMCV\n[InfantryTypes]\n0=E1\n1=CIV\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\nTrainable=yes\nPrimary=M1Carbine\n\
         [CIV]\nStrength=100\nTrainable=no\n\
         [CrateRules]\nCrateImg=SILVER\nWoodCrateImg=WOOD\nWaterCrateImg=WATER\nCrateRadius=3.0\n\
         [Powerups]\nArmor=10,ARMOR,yes,1.5\nFirepower=10,FIREPOWR,yes,2.0\nVeteran=20,VETERAN,yes,1\n\
         Speed=10,SPEED,yes,1.2\nReveal=10,REVEAL,yes\nMoney=20,MONEY,yes,2000\n{extra}"
    )))
    .unwrap()
}

#[test]
fn cloak_crate_grants_foot_stealth_inside_the_strict_radius_and_then_falls_back() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_CLOAK as u8);
    let rules = effect_rules(
        "[General]\nCloakingStages=6\n[E1]\nCloakingSpeed=1\n\
         [VehicleTypes]\n0=TNK\n[TNK]\nStrength=100\nSpeed=4\nCloakingSpeed=1\n",
    );
    let enemy = house(&mut sim, "H1", false);
    let nearby = place_actor(
        &mut sim,
        &rules,
        enemy,
        (11, 10),
        EntityCategory::Unit,
        "TNK",
    );
    let boundary = place_actor(
        &mut sim,
        &rules,
        owner,
        (13, 10),
        EntityCategory::Infantry,
        "E1",
    );
    sim.resolve_type_handles(&rules);
    let rng = sim.scenario_rng.native_state_hex();
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        Default::default()
    ));
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        rng,
        "Cloak spends no RNG"
    );
    assert!(
        sim.entities().get(actor).unwrap().cloak.is_none(),
        "the grant starts no transition"
    );
    assert!(sounds(&sim).is_empty());
    assert!(eva_events(&sim).is_empty());
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(&[], Some(&rules), grid.as_deref(), Some(&registry), 22);
    assert_eq!(
        sim.entities().get(actor).unwrap().cloak.unwrap().state(),
        1,
        "infantry also runs Foot cloak AI"
    );
    assert_eq!(
        sim.entities().get(nearby).unwrap().cloak.unwrap().state(),
        1,
        "the gift ignores allegiance"
    );
    assert!(
        sim.entities().get(boundary).unwrap().cloak.is_none(),
        "exact radius is excluded"
    );
    place_registered_crate(&mut sim, &registry, CELL, POWERUP_CLOAK as u8);
    let credits = sim.houses[&owner].economy().credits();
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        Default::default()
    ));
    assert!(
        sim.houses[&owner].economy().credits() > credits,
        "already gifted actors receive Money"
    );
}

#[test]
fn cloak_crate_ability_survives_save_load_and_does_not_edit_the_type() {
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_CLOAK as u8);
    let rules = effect_rules("");
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        Default::default()
    ));
    let untouched = place_actor(
        &mut sim,
        &rules,
        owner,
        (12, 10),
        EntityCategory::Infantry,
        "E1",
    );
    let no_grant = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(untouched)
        .unwrap()
        .stealth_ability
        .grant();
    assert_ne!(
        sim.state_hash(),
        no_grant,
        "the ability participates in deterministic state"
    );
    // Match the snapshot loader's native Seed0 reset; no RNG comparison is
    // part of this persistence regression.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "cloak-crate", 0);
    let restored = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(restored.state_hash(), expected);
    assert!(
        restored
            .entities()
            .get(actor)
            .unwrap()
            .stealth_ability
            .active(false)
    );
    assert!(
        !rules.object("E1").unwrap().cloakable,
        "a permanent gift belongs to the instance"
    );
}

/// `0x00482972..0x00482B19`: every marked, `Trainable=` Techno within the
/// radius climbs one rank for a magnitude of 1; a civilian and a distant
/// soldier do not.
#[test]
fn veteran_crate_promotes_trainable_technos_within_the_radius() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_VETERAN as u8);
    let rules = effect_rules("[AudioVisual]\nCratePromoteSound=CratePromoted\n");
    let civilian = place_actor(
        &mut sim,
        &rules,
        owner,
        (10, 10),
        EntityCategory::Infantry,
        "CIV",
    );
    let distant = place_actor(
        &mut sim,
        &rules,
        owner,
        (20, 20),
        EntityCategory::Infantry,
        "E1",
    );

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    let raw = |id: u64| {
        f32::from_bits(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .veterancy
                .raw()
                .bits(),
        )
    };
    assert_eq!(raw(actor), 1.0, "the actor on the crate is a veteran");
    assert_eq!(raw(civilian), 0.0, "not Trainable=");
    assert_eq!(raw(distant), 0.0, "outside CrateRadius");
    assert_eq!(
        sounds(&sim),
        vec![("CratePromoted".to_string(), Some([owner, owner]))]
    );
}

/// `0x00482D56..0x00482F31` and `0x00483125..0x004832F0`: the multiplier
/// applies once per object in the radius, the EVA line names every changed
/// house for the app's local test, and a second crate stacks nothing.
#[test]
fn armor_and_firepower_crates_multiply_once_and_announce() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_ARMOR as u8);
    let rules =
        effect_rules("[AudioVisual]\nCrateArmourSound=CrateArmor\nCrateFireSound=CrateFirePower\n");
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_player_control_for_test(true);
    let distant = place_actor(
        &mut sim,
        &rules,
        owner,
        (20, 20),
        EntityCategory::Infantry,
        "E1",
    );

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));
    let armor = |sim: &Simulation, id: u64| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .crate_multipliers
            .armor()
    };
    assert_eq!(armor(&sim, actor).bits(), 1.5_f64.to_bits());
    assert_eq!(armor(&sim, distant), NativeF64Bits::ONE);
    assert_eq!(eva_events(&sim), vec![(owner, "EVA_UnitArmorUpgraded")]);
    assert_eq!(
        sounds(&sim),
        vec![("CrateArmor".to_string(), Some([owner, owner]))]
    );

    // A second Armor crate: the armoured actor is ineligible (`0x00481C69`)
    // and falls back to Money, so nothing stacks.
    place_registered_crate(&mut sim, &registry, CELL, POWERUP_ARMOR as u8);
    let credits = sim.houses[&owner].economy().credits();
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));
    assert_eq!(armor(&sim, actor).bits(), 1.5_f64.to_bits());
    assert!(
        sim.houses[&owner].economy().credits() > credits,
        "Money fallback"
    );

    place_registered_crate(&mut sim, &registry, CELL, POWERUP_FIREPOWER as u8);
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));
    let firepower = sim
        .substrate
        .entities
        .get(actor)
        .unwrap()
        .crate_multipliers
        .firepower();
    assert_eq!(firepower.bits(), 2.0_f64.to_bits());
    assert_eq!(
        sim.substrate
            .entities
            .get(distant)
            .unwrap()
            .crate_multipliers
            .firepower(),
        NativeF64Bits::ONE
    );
    assert!(eva_events(&sim).contains(&(owner, "EVA_UnitFirePowerUpgraded")));
}

/// `0x00481F9D..0x0048203C`: `MapClass::Reveal` for the actor's house,
/// which also sets its map-clear byte (`+0x241`, `0x00577DB0`).
#[test]
fn reveal_crate_reveals_the_whole_map_for_the_owner() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_REVEAL as u8);
    let rules = effect_rules("[AudioVisual]\nCrateRevealSound=CrateReveal\n");
    sim.fog.set_width_for_test(sim.session.map_width());
    sim.fog.set_height_for_test(sim.session.map_height());
    assert!(!sim.fog.whole_map_revealed_for_owner(owner));
    assert!(!sim.houses[&owner].map_is_clear());

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert!(sim.fog.whole_map_revealed_for_owner(owner));
    assert!(sim.houses[&owner].map_is_clear());
    // `0x0048202D..0x0048203C`: the one arm without the local-player test.
    assert_eq!(sounds(&sim), vec![("CrateReveal".to_string(), None)]);
}

/// The guards: a passive multiplayer owner, a non-crate overlay and a missing
/// actor consume nothing and spend no draw.
#[test]
fn guards_leave_the_crate_and_the_rng_alone() {
    let (mut sim, rules, registry, owner, actor) = multiplayer_fixture(10);
    let before = sim.scenario_rng.native_state_hex();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_multiplay_passive_for_test(true);
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(overlay_at(&sim, CELL).is_some());
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_multiplay_passive_for_test(false);
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        99,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(overlay_at(&sim, CELL).is_some());
    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        (11, 11),
        actor,
        crate::sim::world::FrameEffects::default()
    ));
    assert_eq!(sim.scenario_rng.native_state_hex(), before);
    assert!(sim.sound_events.is_empty());
}

/// A spawnable arena with a Drive tank, a `CrateGoodie=` vehicle type, a
/// construction yard type and the C4 warhead the HealBase arm names.
const ARENA_RULES: &str = "[Countries]\n0=Americans\n1=Russians\n[Sides]\nAllied=Americans\nSoviet=Russians\n\
             [Americans]\nSide=Allied\n[Russians]\nSide=Soviet\n\
             [General]\nBaseUnit=AMCV\nHarvesterUnit=HARV\n[AI]\nBuildConst=YARD\n\
             [CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
             [Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [InfantryTypes]\n0=E1\n[AircraftTypes]\n0=PLANE\n[VehicleTypes]\n0=TNK\n1=AMCV\n2=HARV\n[BuildingTypes]\n0=YARD\n\
             [E1]\nStrength=100\nSpeed=4\nSpeedType=Foot\nMovementZone=Normal\nOwner=Americans,Russians\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [PLANE]\nStrength=150\n\
             [TNK]\nStrength=300\nCloakingSpeed=4\nSpeed=5\nROT=5\nOwner=Americans,Russians\nCrateGoodie=yes\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [AMCV]\nStrength=1000\nSpeed=4\nROT=5\nOwner=Americans,Russians\nDeploysInto=YARD\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [HARV]\nStrength=1000\nSpeed=4\nROT=5\nOwner=Americans,Russians\nHarvester=yes\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [YARD]\nStrength=1000\nConstructionYard=yes\n[Unload]\nRate=0.016\n\
             [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\nAmphibious=80%\nHover=50%\nBuildable=yes\n\
             [CrateRules]\nCrateImg=WOOD\nWoodCrateImg=WOOD\nWaterCrateImg=WOOD\nCrateRadius=3.0\n\
             HealCrateSound=HealCrate\n\
             [AudioVisual]\nCrateMoneySound=CrateMoney\nCrateUnitSound=CrateFreeUnit\n\
             [Powerups]\nMoney=20,MONEY,yes,2000\nUnit=20,<none>,no\nHealBase=10,HEALALL,yes\n";

fn arena() -> (Simulation, RuleSet, OverlayTypeRegistry, InternedId, u64) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(ARENA_RULES),
        &IniFile::from_str("[YARD]\nFoundation=4x3\n"),
    )
    .unwrap();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().crates = false;
    // Bases off: no MCV pre-empt, and the random Unit draw refuses `BaseUnit=`.
    sim.session.game_options_mut_for_test().bases = false;
    // A normal game: its unit totals keep the buildingless House undefeated.
    sim.session.game_options_mut_for_test().short_game = false;
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, Some(owner), true, 5000, 10));
    let actor = sim
        .spawn_object("TNK", "Americans", 20, 22, 0, &rules)
        .expect("the tank spawns");
    // The OverlayType table reads its land rows from the same rules, as the
    // match's table does, so the crate cell keeps `[Clear]`'s speed profile.
    let registry = OverlayTypeRegistry::from_ini(
        &IniFile::from_str(&format!(
            "{ARENA_RULES}[OverlayTypes]\n0=WOOD\n[WOOD]\nCrate=yes\nCrateTrigger=yes\nLand=Clear\n"
        )),
        None,
    );
    (sim, rules, registry, owner, actor)
}

/// `0x00482041..0x00482449`: a random `CrateGoodie=` vehicle is built for the
/// actor's house and unlimboed at the crate cell; the pickup returns false
/// and spawns no anim.
#[test]
fn unit_crate_places_a_goodie_vehicle_and_returns_false() {
    let (mut sim, rules, registry, owner, actor) = arena();
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_UNIT as u8);
    let tanks = |sim: &Simulation| {
        sim.substrate
            .entities
            .values()
            .filter(|entity| sim.interner.resolve(entity.type_ref()) == "TNK")
            .count()
    };
    assert_eq!(tanks(&sim), 1);

    assert!(!pickup_crate(
        &mut sim,
        &rules,
        &registry,
        (21, 22),
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(tanks(&sim), 2);
    let free = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.stable_id() != actor && sim.interner.resolve(entity.type_ref()) == "TNK"
        })
        .expect("the free tank");
    assert_eq!((free.position.rx(), free.position.ry()), (21, 22));
    assert!(free.lifecycle.cell_marked() && !free.lifecycle.in_limbo());
    assert_eq!(free.owner(), owner);
    assert_eq!(overlay_at(&sim, (21, 22)), None);
    assert_eq!(
        sounds(&sim),
        vec![("CrateFreeUnit".to_string(), Some([owner, owner]))]
    );
}

/// `0x0048231E..0x00482366`: the picker stands on the crate, so the goodie's
/// Unlimbo there is refused and Find_Nearby_Passable_Cell places it with no
/// height gate and bridge cells allowed. Ground two levels above the crate
/// cell therefore counts, and the first ring takes the goodie.
#[test]
fn unit_crate_under_its_picker_lands_the_goodie_on_the_first_ring_at_any_level() {
    let (mut sim, rules, registry, owner, actor) = arena();
    let at = (20u16, 22u16);
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for y in 0..32 {
        for x in 0..32 {
            if (x, y) != at {
                terrain.cell_mut_for_test(x, y).unwrap().level = 2;
            }
        }
    }
    place_registered_crate(&mut sim, &registry, (20, 22), POWERUP_UNIT as u8);

    assert!(!pickup_crate(
        &mut sim,
        &rules,
        &registry,
        (20, 22),
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    let free = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.stable_id() != actor && sim.interner.resolve(entity.type_ref()) == "TNK"
        })
        .expect("the goodie was placed");
    let cell = (free.position.rx(), free.position.ry());
    assert_eq!(
        cell.0.abs_diff(at.0).max(cell.1.abs_diff(at.1)),
        1,
        "the first ring, two levels up: {cell:?}"
    );
    assert!(free.lifecycle.cell_marked() && !free.lifecycle.in_limbo());
    assert_eq!(free.owner(), owner);
}

/// `0x00481BB8..0x00481BFF`: with Bases on, a house with no tracked building,
/// more than 1500 credits and no tracked `BaseUnit=` is forced onto the Unit
/// slot and receives its first ownable `BaseUnit=` (`0x00482062..0x0048207F`),
/// whatever the crate held.
#[test]
fn mcv_preempt_grants_the_base_unit_with_bases_on() {
    let (mut sim, rules, registry, owner, actor) = arena();
    sim.session.game_options_mut_for_test().bases = true;
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_MONEY as u8);

    assert!(!pickup_crate(
        &mut sim,
        &rules,
        &registry,
        (21, 22),
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    let mcv = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "AMCV")
        .expect("the free MCV");
    assert_eq!((mcv.position.rx(), mcv.position.ry()), (21, 22));
    assert_eq!(mcv.owner(), owner);
}

/// `0x00482B8F..0x00482C9C`: the HealBase sound, then `Health - Strength`
/// through `ReceiveDamage(.., C4Warhead=, .., 1, 1, ..)` on the actor
/// house's Logic members, including buildings.
#[test]
fn heal_base_crate_restores_the_owners_buildings() {
    let (mut sim, rules, registry, owner, actor) = arena();
    let rival = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(rival, 1, Some(rival), false, 5000, 10));
    let own = sim
        .spawn_object("YARD", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let theirs = sim
        .spawn_object("YARD", "Russians", 8, 20, 0, &rules)
        .unwrap();
    for id in [own, theirs] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(400);
    }
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_HEAL_BASE as u8);

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        (21, 22),
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(
        sim.substrate.entities.get(own).unwrap().health.current(),
        1000
    );
    assert_eq!(
        sim.substrate.entities.get(theirs).unwrap().health.current(),
        400
    );
    // `[CrateRules]` retains the upper-cased name at its data boundary.
    assert_eq!(
        sounds(&sim),
        vec![("HEALCRATE".to_string(), Some([owner, owner]))]
    );
}

/// HealBase482C18 walks Logic, so it reaches owned mobile classes and skips
/// an Aircraft absent from Logic. Zero damage still reaches the full-health
/// collector's surviving Techno receiver and uncloaks it.
#[test]
fn heal_base_reaches_mobile_logic_members_and_the_zero_damage_receiver() {
    let (mut sim, mut rules, registry, owner, tank) = arena();
    rules.general.cloaking_stages = 10;
    sim.session.set_binary_frame_for_test(100);
    let rival = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(rival, 1, Some(rival), false, 5000, 10));
    let infantry = sim
        .spawn_object("E1", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let enemy = sim
        .spawn_object("TNK", "Russians", 8, 20, 0, &rules)
        .unwrap();
    let aircraft = place_actor(
        &mut sim,
        &rules,
        owner,
        (10, 10),
        EntityCategory::Aircraft,
        "PLANE",
    );
    let outside_logic = place_actor(
        &mut sim,
        &rules,
        owner,
        (12, 10),
        EntityCategory::Aircraft,
        "PLANE",
    );
    for (id, health) in [
        (infantry, 50),
        (enemy, 100),
        (aircraft, 75),
        (outside_logic, 75),
    ] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(health);
    }
    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
    cloak.set_state_for_test(2);
    sim.substrate.entities.get_mut(tank).unwrap().cloak = Some(cloak);
    let logic = vec![aircraft, 0, enemy, infantry, tank];
    sim.set_logic_order_for_test(logic.clone());
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_HEAL_BASE as u8);

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        (21, 22),
        tank,
        crate::sim::world::FrameEffects::default()
    ));

    let health = |id| sim.substrate.entities.get(id).unwrap().health.current();
    assert_eq!(
        [
            health(tank),
            health(infantry),
            health(enemy),
            health(aircraft),
            health(outside_logic)
        ],
        [300, 100, 100, 150, 75]
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .cloak
            .unwrap()
            .state(),
        3
    );
    assert_eq!(sim.logic_order(), logic);
    assert_eq!(overlay_at(&sim, (21, 22)), None);
}

/// Drive `Process_Movement` (`0x004B405D`/`0x004B46E6`) through the master
/// frame: a tank ordered across a crate cell picks the crate up.
#[test]
fn a_moving_tank_picks_up_the_crate_on_its_path() {
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules, registry, owner, actor) = arena();
    place_registered_crate(&mut sim, &registry, (22, 22), POWERUP_MONEY as u8);
    let credits = sim.houses[&owner].economy().credits();
    let grid = sim.path_grid_snapshot();
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: actor,
            target_rx: 25,
            target_ry: 22,
        },
    );
    sim.advance_tick(&[order], Some(&rules), grid.as_deref(), Some(&registry), 22);
    for _ in 0..400 {
        if overlay_at(&sim, (22, 22)).is_none() {
            break;
        }
        sim.advance_tick(&[], Some(&rules), grid.as_deref(), Some(&registry), 22);
    }
    assert_eq!(
        overlay_at(&sim, (22, 22)),
        None,
        "the tank crossed the crate"
    );
    assert!(sim.houses[&owner].economy().credits() > credits);
}

/// The ordinary Drive movement host reaches HealBase without a direct call to
/// the pickup helper. Healing also reaches a distant same-house Infantry.
#[test]
fn a_moving_tank_collects_heal_base_for_its_house() {
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules, registry, owner, tank) = arena();
    let rival = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(rival, 1, Some(rival), false, 5000, 10));
    let own = sim
        .spawn_object("E1", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let theirs = sim
        .spawn_object("E1", "Russians", 8, 20, 0, &rules)
        .unwrap();
    for id in [tank, own, theirs] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(25);
    }
    place_registered_crate(&mut sim, &registry, (22, 22), POWERUP_HEAL_BASE as u8);
    let grid = sim.path_grid_snapshot();
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: tank,
            target_rx: 25,
            target_ry: 22,
        },
    );
    sim.advance_tick(&[order], Some(&rules), grid.as_deref(), Some(&registry), 22);
    for _ in 0..400 {
        if overlay_at(&sim, (22, 22)).is_none() {
            break;
        }
        sim.advance_tick(&[], Some(&rules), grid.as_deref(), Some(&registry), 22);
    }
    assert_eq!(
        overlay_at(&sim, (22, 22)),
        None,
        "the tank crossed the crate"
    );
    let health = |id| sim.substrate.entities.get(id).unwrap().health.current();
    assert_eq!([health(tank), health(own), health(theirs)], [300, 100, 25]);
}

/// Drive `Force_Track`'s false answer (`0x004B0D22`): a Unit crate on the
/// supplied cell places the free vehicle, and the host drops the head it was
/// installing instead of claiming the cell.
#[test]
fn force_track_onto_a_unit_crate_drops_the_head() {
    let (mut sim, rules, registry, owner, actor) = arena();
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_UNIT as u8);

    assert!(!sim.force_track(
        actor,
        -1,
        DriveCoord::cell(21, 22, 0),
        &rules,
        Some(&registry),
        crate::sim::world::FrameEffects::default()
    ));

    let tank = sim.substrate.entities.get(actor).unwrap();
    let drive = tank
        .locomotor
        .as_ref()
        .and_then(|loco| loco.selected_drive_runtime())
        .and_then(|runtime| runtime.retained())
        .expect("Drive runtime");
    assert_eq!(drive.head_to(), None, "the head is dropped again");
    assert!(!drive.track_valid());
    let free = sim
        .substrate
        .entities
        .values()
        .filter(|entity| entity.owner() == owner && entity.stable_id() != actor)
        .count();
    assert_eq!(free, 1, "the free vehicle stands on the crate cell");
    assert_eq!(overlay_at(&sim, (21, 22)), None);
}

/// Walk's head chooser (`0x0075C56C`) through the master frame: a soldier
/// ordered across a crate cell picks the crate up when his head commits to it.
#[test]
fn a_walking_soldier_picks_up_the_crate_on_his_path() {
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules, registry, owner, _tank) = arena();
    let soldier = sim
        .spawn_object("E1", "Americans", 10, 10, 0, &rules)
        .expect("the soldier spawns");
    place_registered_crate(&mut sim, &registry, (12, 10), POWERUP_MONEY as u8);
    let credits = sim.houses[&owner].economy().credits();
    let grid = sim.path_grid_snapshot();
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: soldier,
            target_rx: 15,
            target_ry: 10,
        },
    );
    sim.advance_tick(&[order], Some(&rules), grid.as_deref(), Some(&registry), 22);
    for _ in 0..600 {
        if overlay_at(&sim, (12, 10)).is_none() {
            break;
        }
        sim.advance_tick(&[], Some(&rules), grid.as_deref(), Some(&registry), 22);
    }
    assert_eq!(
        overlay_at(&sim, (12, 10)),
        None,
        "the soldier crossed the crate"
    );
    assert!(sim.houses[&owner].economy().credits() > credits);
}

/// A nonempty CrateGoodie pool can still have no acceptable candidate: an
/// MCV is forbidden with Bases off, and for a non-preempted AI with Bases on.
#[test]
fn random_unit_crate_with_only_forbidden_mcvs_returns_without_rng() {
    for (bases, human) in [(false, true), (true, false)] {
        let (mut sim, _, _, owner, _) = arena();
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nBaseUnit=AMCV\n[VehicleTypes]\n0=AMCV\n\
             [AMCV]\nStrength=1000\nCrateGoodie=yes\n",
        ))
        .unwrap();
        sim.session.game_options_mut_for_test().bases = bases;
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_is_human_for_test(human);
        let before = sim.scenario_rng.native_state_hex();
        assert!(
            effects::choose_unit_crate_type(
                &sim.interner,
                &sim.session,
                &mut sim.scenario_rng,
                &sim.houses,
                &rules,
                owner,
                false
            )
            .is_none(),
            "no vehicle is eligible with bases={bases}, human={human}"
        );
        assert_eq!(sim.scenario_rng.native_state_hex(), before);
    }
}

/// The guard must not reject an MCV that the existing human/Bases rule admits.
#[test]
fn random_unit_crate_still_allows_a_human_mcv_with_bases() {
    let (mut sim, _, _, owner, _) = arena();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nBaseUnit=AMCV\n[VehicleTypes]\n0=AMCV\n\
         [AMCV]\nStrength=1000\nCrateGoodie=yes\n",
    ))
    .unwrap();
    sim.session.game_options_mut_for_test().bases = true;
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_is_human_for_test(true);
    assert_eq!(
        effects::choose_unit_crate_type(
            &sim.interner,
            &sim.session,
            &mut sim.scenario_rng,
            &sim.houses,
            &rules,
            owner,
            false
        )
        .map(|object| object.id.as_str()),
        Some("AMCV")
    );
}

/// A rejected MCV draw must still be spent when another type is eligible.
/// Filtering the draw pool would change both the result and the RNG stream.
#[test]
fn random_unit_crate_keeps_rejected_draws_before_an_eligible_vehicle() {
    let (mut sim, _, _, owner, _) = arena();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nBaseUnit=AMCV\n[VehicleTypes]\n0=AMCV\n1=TNK\n\
         [AMCV]\nStrength=1000\nCrateGoodie=yes\n\
         [TNK]\nStrength=300\nCrateGoodie=yes\n",
    ))
    .unwrap();
    sim.session.game_options_mut_for_test().bases = false;
    // Select a deterministic starting state whose first two draws are 0, 1.
    let expected = (0..100)
        .find_map(|_| {
            let mut probe = sim.scenario_rng.clone();
            if probe.next_range_i32_inclusive(0, 1) == 0
                && probe.next_range_i32_inclusive(0, 1) == 1
            {
                Some(probe)
            } else {
                sim.scenario_rng.next_range_i32_inclusive(0, 1);
                None
            }
        })
        .expect("a deterministic rejected-MCV then accepted-tank draw pair");
    assert_eq!(
        effects::choose_unit_crate_type(
            &sim.interner,
            &sim.session,
            &mut sim.scenario_rng,
            &sim.houses,
            &rules,
            owner,
            false
        )
        .map(|object| object.id.as_str()),
        Some("TNK")
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected.native_state_hex()
    );
}

/// Original481A00→56BD40→4824A0→421EA0/423AC0/725C70. The native
/// corpus independently seeds each stream at the pickup boundary, after an
/// original placement; geometry/House priors are explicit, not a native map
/// bootstrap claim. The ordinary app Move capture covers that integration.
#[test]
fn retail_cash_crate_matches_native_replacement_reward_anim_and_cleanup() {
    use crate::sim::rng::{MainRng, SimRng};
    use serde_json::json;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
    else {
        return;
    };
    let rows = crate::test_fixture::json("tools/spatial_oracle/cash_crate.json");
    let raw_shp = crate::rules::retail_ini_fixture::retail_archive_bytes("MONEY.SHP")
        .expect("physical MONEY artwork");
    let mut rules = retail.rules;
    let count = crate::assets::shp_file::ShpFile::frame_count_from_bytes(&raw_shp).unwrap();
    rules.bind_anim_frame_count_for_test("MONEY", i32::from(count));
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let crate_id = registry.id_for_name("CRATE").unwrap();
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let native_art = &row["retail"]["art"];
        assert_eq!(json!(count), native_art["raw_shp_frame_count"], "{name}");
        assert_eq!(
            crate::util::sha256::sha256_hex(&raw_shp),
            native_art["asset_loads"][0]["sha256"].as_str().unwrap(),
            "{name} actual SHP input"
        );
        assert_eq!(json!(rules.powerups.weights), row["retail"]["weights"]);
        assert_eq!(
            rules.powerups.anims[POWERUP_MONEY].as_deref(),
            Some("MONEY")
        );
        assert_eq!(
            rules.powerups.magnitudes[POWERUP_MONEY].bits(),
            row["retail"]["money_magnitude"].as_f64().unwrap().to_bits()
        );
        assert_eq!(
            rules.crate_rules.regen.bits(),
            row["retail"]["regen_minutes"].as_f64().unwrap().to_bits()
        );
        let mut sim = Simulation::with_seed(input["placement_seed"].as_u64().unwrap());
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.overlay_grid = Some(OverlayGrid::new(32, 32));
        let size = &input["geometry"]["size"];
        let local = &input["geometry"]["raw_local_size"];
        // This is the corpus's supplied geometry, not a full Pack5 map load.
        let header = crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: size[2].as_u64().unwrap() as u32,
            height: size[3].as_u64().unwrap() as u32,
            local_left: local[0].as_u64().unwrap() as u32,
            local_top: local[1].as_u64().unwrap() as u32,
            local_width: local[2].as_u64().unwrap() as u32,
            local_height: local[3].as_u64().unwrap() as u32,
        };
        sim.install_playfield_from_map_header(&header);
        let bounds = sim.playfield_bounds().unwrap();
        assert_eq!(
            json!([
                bounds.off_fc,
                bounds.off_100,
                bounds.off_104,
                bounds.off_108
            ]),
            input["geometry"]["local_size"],
            "{name} original clipped LocalSize"
        );
        assert_eq!(
            json!(sim.map_rect().unwrap()),
            input["geometry"]["map_rect"],
            "{name} initialized MapRect"
        );
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["mode"].as_i64().unwrap() != 0,
            ),
        );
        // The joined native Anim fixture supplies its stored speed explicitly;
        // normalized MONEY timing must use the same input through its owner.
        sim.session.game_options_mut_for_test().game_speed =
            input["game_speed"].as_i64().unwrap() as i32;
        sim.session.game_options_mut_for_test().crates =
            input["regenerate"].as_bool().unwrap_or(true);
        sim.session
            .set_binary_frame_for_test(input["frame"].as_u64().unwrap() as u32);
        let owner = house(&mut sim, "Americans", true);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut()
            .set_credits_for_test(input["balance"].as_i64().unwrap() as i32);
        // Native supplies one tracked building without a resident Building
        // body. Install that prior through the existing tracking owner.
        let yard = place_actor(
            &mut sim,
            &rules,
            owner,
            (24, 24),
            EntityCategory::Structure,
            "GACNST",
        );
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .tracking_mut()
            .add_tracking(sim.substrate.entities.get_mut(yard).unwrap());
        let actor = sim
            .spawn_object("MTNK", "Americans", 10, 11, 0, &rules)
            .unwrap();
        let cell = (
            input["pickup_cell"][0].as_i64().unwrap() as i16,
            input["pickup_cell"][1].as_i64().unwrap() as i16,
        );
        let placement_seed = input["placement_seed"].as_u64().unwrap();
        sim.scenario_rng = SimRng::new(placement_seed);
        sim.main_rng = MainRng::new(placement_seed);
        sim.mapgen_rng = SimRng::new(placement_seed);
        assert!(super::super::place_crate(
            &mut sim,
            &rules,
            &registry,
            cell,
            input
                .get("scalar")
                .map(|value| value.as_u64().unwrap() as u8)
        ));
        let slot_values = |sim: &Simulation| {
            json!(
                sim.crate_authority
                    .slots()
                    .iter()
                    .filter(|slot| !slot.is_empty())
                    .map(|slot| (
                        slot.start_frame,
                        slot.aux,
                        slot.duration,
                        slot.cell_x,
                        slot.cell_y
                    ))
                    .collect::<Vec<_>>()
            )
        };
        // Zero coordinates alone define an empty native slot. The supplied
        // native arena zeroes unused timers; Rust constructs them paused (-1).
        // Compare all live slots, including their timer words, without treating
        // irrelevant unused timer storage as a gameplay difference.
        let native_live_slots = |slots: &serde_json::Value| {
            json!(
                slots
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|slot| {
                        slot[3].as_i64().unwrap() != 0 || slot[4].as_i64().unwrap() != 0
                    })
                    .collect::<Vec<_>>()
            )
        };
        assert_eq!(
            slot_values(&sim),
            native_live_slots(&row["initial_slots"]),
            "{name} original placement"
        );
        assert_eq!(overlay_at(&sim, cell), Some(crate_id));
        while sim.native_identity_cursor().unwrap_or(0)
            < row["id_cursor_before"].as_u64().unwrap() as u32
        {
            sim.next_native_runtime_id();
        }
        assert_eq!(
            json!(sim.native_identity_cursor().unwrap()),
            row["id_cursor_before"]
        );
        let seed = input["seed"].as_u64().unwrap();
        sim.scenario_rng = SimRng::new(seed);
        sim.main_rng = MainRng::new(seed);
        sim.mapgen_rng = SimRng::new(seed);
        let rng = |sim: &Simulation| {
            json!({"scenario": sim.scenario_rng.native_state_hex(),
            "main": sim.main_rng.native_state_hex(), "mapgen": sim.mapgen_rng.native_state_hex()})
        };
        assert_eq!(
            rng(&sim),
            row["rng_before"],
            "{name} independent native seed input"
        );
        sim.sound_events.clear_for_test();
        assert_eq!(
            pickup_crate(
                &mut sim,
                &rules,
                &registry,
                cell,
                actor,
                crate::sim::world::FrameEffects::default()
            ),
            row["returned"].as_u64().unwrap() != 0
        );
        assert_eq!(
            json!(sim.houses[&owner].economy().credits()),
            row["balance"],
            "{name}"
        );
        assert_eq!(
            slot_values(&sim),
            native_live_slots(&row["final_slots"]),
            "{name} replacement ordering"
        );
        assert_eq!(overlay_at(&sim, cell), None, "{name} old image removed");
        assert_eq!(rng(&sim), row["rng_after"], "{name} all three streams");
        assert_eq!(
            json!(sim.native_identity_cursor().unwrap()),
            row["id_cursor_after"]
        );
        assert_eq!(
            sounds(&sim),
            vec![("CrateMoney".into(), Some([owner, owner]))]
        );
        let sound_coord = sim
            .sound_events
            .iter()
            .find_map(|event| match event {
                SimSoundEvent::VocAt {
                    sound_id,
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    world_z_leptons,
                    ..
                } if sound_id == "CrateMoney" => {
                    let position = crate::sim::components::Position::new(
                        *rx,
                        *ry,
                        0,
                        Some(*world_z_leptons),
                        *sub_x,
                        *sub_y,
                    );
                    let point = crate::sim::movement::ground_pose::position_world_coord(&position);
                    Some(json!([point.x, point.y, point.z]))
                }
                _ => None,
            })
            .unwrap();
        let native_sound = row["pickup_events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["call"] == "play_sound")
            .unwrap();
        assert_eq!(
            sound_coord, native_sound["coord"],
            "{name} retained sound center"
        );
        let anim = *sim
            .anims()
            .find(|(_, anim)| sim.resolve(anim.type_id()) == "MONEY")
            .unwrap()
            .0;
        crate::sim::anim_class::trigger_tests::assert_state(&sim, anim, &row["initial_anim"], name);
        let history = row["history"].as_array().unwrap();
        let last = history.last().unwrap()["frame"].as_u64().unwrap() as u32;
        for frame in input["frame"].as_u64().unwrap() as u32..=last {
            sim.session.set_binary_frame_for_test(frame);
            sim.visit_anim(
                anim,
                &rules,
                Some(&registry),
                crate::sim::world::FrameEffects::default(),
            );
            if let Some(control) = history
                .iter()
                .find(|control| control["frame"].as_u64() == Some(u64::from(frame)))
            {
                crate::sim::anim_class::trigger_tests::assert_state(
                    &sim,
                    anim,
                    &control["state"],
                    name,
                );
            }
        }
        sim.process_pending_delete();
        assert!(sim.anim(anim).is_none());
        assert_eq!(json!(sim.anims().count()), row["cleanup"]["anim_count"]);
        assert_eq!(
            json!(sim.substrate.pending_delete.len()),
            row["cleanup"]["pending_delete_count"]
        );
        assert_eq!(
            json!(sim.native_identity_cursor().unwrap()),
            row["id_cursor_after_cleanup"]
        );
        assert_eq!(
            rng(&sim),
            row["rng_after_cleanup"],
            "{name} lifetime adds no draws"
        );
    }
}

// --- The five residual jump-table arms -------------------------------------

/// The residual arms' synthetic rules: the three named warheads they resolve
/// (Gas by its table name, `[CombatDamage] C4Warhead=` and `FlameDamage=`),
/// the retail zero-weight `[Powerups]` rows with their magnitudes, and a
/// registered animation table (registration order, so `AnimTypeArray8B4154`
/// index 0 is FIRE1) with fixed ART sections, so the explosion and fire
/// animations construct.
fn residual_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[AircraftTypes]\n[VehicleTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\n\
         [Warheads]\n0=Super\n1=Gas\n2=Flame\n\
         [Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nAnimList=BOOM\n\
         [Gas]\nVerses=50%,50%,50%,50%,50%,50%,50%,50%,50%,50%,50%\n\
         [Flame]\nVerses=50%,50%,50%,50%,50%,50%,50%,50%,50%,50%,50%\n\
         [CombatDamage]\nC4Warhead=Super\nFlameDamage=Flame\n\
         [Animations]\n0=FIRE1\n1=BOOM\n\
         [CrateRules]\nCrateImg=SILVER\nWoodCrateImg=WOOD\nWaterCrateImg=WATER\nCrateRadius=3.0\n\
         [Powerups]\nExplosion=0,<none>,yes,500\nNapalm=0,<none>,no,600\n\
         ICBM=0,CHEMISLE,yes\nGas=0,<none>,yes,100\nTiberium=0,<none>,no\n",
    );
    let art = IniFile::from_str("[FIRE1]\nStart=0\n[BOOM]\nStart=0\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut art_registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    art_registry.bind_anim_frame_count_for_test("FIRE1", 2);
    art_registry.bind_anim_frame_count_for_test("BOOM", 2);
    rules.install_art_fixture(art_registry);
    rules
}

/// Every live anim as (type, coordinate, flags, zAdjust) in creation order.
fn anim_briefs(
    sim: &Simulation,
) -> Vec<(String, crate::sim::anim_class::AnimWorldCoord, u32, i32)> {
    sim.substrate
        .anims
        .iter()
        .map(|(_, anim)| {
            (
                sim.interner.resolve(anim.type_id()).to_ascii_uppercase(),
                anim.world_coord(),
                anim.draw_flags(),
                anim.z_adjust(),
            )
        })
        .collect()
}

fn health_of(sim: &Simulation, id: u64) -> i32 {
    sim.substrate.entities.get(id).unwrap().health.current()
}

/// `0x00481DE7..0x00481E99`: the "GAS" warhead lands on the crate cell and
/// each 0x89F688 neighbour at `ftol(magnitude)`, all through the Gas warhead's
/// 50% verses quotient, with no RNG draw and no crate sound.
#[test]
fn gas_crate_hits_the_nine_cells_with_the_gas_warhead_without_rng() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_GAS as u8);
    let rules = residual_rules();
    let victim = place_actor(
        &mut sim,
        &rules,
        owner,
        (11, 10),
        EntityCategory::Infantry,
        "E1",
    );
    let corner = place_actor(
        &mut sim,
        &rules,
        owner,
        (9, 9),
        EntityCategory::Infantry,
        "E1",
    );
    let far = place_actor(
        &mut sim,
        &rules,
        owner,
        (20, 20),
        EntityCategory::Infantry,
        "E1",
    );
    let rng = sim.scenario_rng.native_state_hex();

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        rng,
        "Gas spends no draw"
    );
    // `ftol(100) * 50%` per hit; every unit in the nine cells takes exactly
    // one (the [Gas] warhead carries no CellSpread).
    assert_eq!(health_of(&sim, actor), 50, "the centre cell");
    assert_eq!(health_of(&sim, victim), 50, "the E neighbour");
    assert_eq!(health_of(&sim, corner), 50, "the NW neighbour");
    assert_eq!(health_of(&sim, far), 100, "outside the nine cells");
    assert!(sounds(&sim).is_empty(), "the arm skips the pickup sound");
}

/// The Gas arm's defensive branch: a ruleset with no "GAS"-named warhead
/// applies no damage anywhere in the nine cells and spends no draw, while the
/// shared anim tail still runs (the slot's `Gas=0,BOOM,…` anim).
#[test]
fn gas_crate_without_a_gas_warhead_leaves_the_nine_cells_unharmed() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_GAS as u8);
    let ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[AircraftTypes]\n[VehicleTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\n\
         [Warheads]\n0=Super\n\
         [Animations]\n0=FIRE1\n1=BOOM\n\
         [CrateRules]\nCrateImg=SILVER\nWoodCrateImg=WOOD\nWaterCrateImg=WATER\nCrateRadius=3.0\n\
         [Powerups]\nGas=0,BOOM,yes,100\n",
    );
    let art = IniFile::from_str("[FIRE1]\nStart=0\n[BOOM]\nStart=0\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut art_registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    art_registry.bind_anim_frame_count_for_test("BOOM", 2);
    rules.install_art_fixture(art_registry);
    let victim = place_actor(
        &mut sim,
        &rules,
        owner,
        (11, 10),
        EntityCategory::Infantry,
        "E1",
    );
    let corner = place_actor(
        &mut sim,
        &rules,
        owner,
        (9, 9),
        EntityCategory::Infantry,
        "E1",
    );
    let rng = sim.scenario_rng.native_state_hex();

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        rng,
        "the arm still spends no draw"
    );
    assert_eq!(health_of(&sim, actor), 100, "the centre cell");
    assert_eq!(health_of(&sim, victim), 100, "the E neighbour");
    assert_eq!(health_of(&sim, corner), 100, "the NW neighbour");
    let briefs = anim_briefs(&sim);
    assert_eq!(briefs.len(), 1, "the anim tail still runs");
    assert_eq!(briefs[0].0, "BOOM");
    assert!(sounds(&sim).is_empty(), "the arm skips the pickup sound");
}

/// `0x00481E99..0x00481F68` with the drawn type 1: the clamp at
/// `0x00481ECC..0x00481ED1` (`cmp edi,1`/`jne`/`xor edi,edi`) forces the ore
/// type to 0 and both paths fall into the same `IncreaseTiberium(type, 1)` —
/// the arm runs on with the full `2 + 2N` draw order and Riparius ore.
#[test]
fn tiberium_crate_with_the_drawn_type_one_clamps_to_type_zero_and_places_anyway() {
    let (mut sim, rules, registry, _owner, actor) = tiberium_fixture();
    // A seed whose first `RandomRanged(0, 1)` draw answers 1.
    let seed = (0..1000)
        .find(|&seed| {
            let mut probe = crate::sim::rng::SimRng::new(seed);
            probe.next_range_i32_inclusive(0, 1) == 1
        })
        .expect("a seed whose first draw is 1");
    sim.scenario_rng = crate::sim::rng::SimRng::new(seed);
    let receiver = sim.resolved_terrain.as_ref().map(|terrain| {
        crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(CELL)
    });
    let base = crate_center(&sim, CELL, receiver);

    // The clamp spends nothing: exactly the drawn-0 arm's replay.
    let mut probe = sim.scenario_rng.clone();
    assert_eq!(probe.next_range_i32_inclusive(0, 1), 1);
    probe.next_range_u32(12);
    probe.next_u32();
    let count = probe.next_range_i32_inclusive(10, 20);
    let mut expected_cells = std::collections::BTreeSet::from([CELL]);
    for _ in 0..count {
        let distance = probe.next_range_i32_inclusive(0, 0x300);
        let (x, y) = random_direction_coord(
            &mut probe,
            base.x,
            base.y,
            distance,
            RandomDirectionSnap::CellCenter,
        );
        let cell = (
            u16::try_from(lepton_to_cell(x)).unwrap_or(u16::MAX),
            u16::try_from(lepton_to_cell(y)).unwrap_or(u16::MAX),
        );
        if expected_cells.insert((cell.0 as i16, cell.1 as i16)) {
            probe.next_range_u32(12);
            probe.next_u32();
        }
    }

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        probe.native_state_hex(),
        "the clamp spends nothing; the arm runs its full draw order"
    );
    assert_eq!(
        total_bales(&sim, &registry),
        u32::try_from(expected_cells.len()).unwrap(),
        "{count} jitter passes over {expected_cells:?}"
    );
    let pickup = sim
        .overlay_grid
        .as_ref()
        .unwrap()
        .cell(CELL.0 as u16, CELL.1 as u16);
    assert_eq!(
        pickup
            .overlay_id
            .and_then(|id| registry.tiberium_type_for_overlay(&rules.tiberium_types, id))
            .map(|type_id| type_id.0),
        Some(0),
        "the drawn 1 plants the first registered type"
    );
}

/// `0x00482565..0x0048271E`: the picker takes the C4Warhead's 500 first, then
/// five blasts — exactly ten draws (distance plus raw direction each), each
/// with a 0x2600/-15 combat animation and a flash at the scattered coordinate.
#[test]
fn explosion_crate_hurts_the_picker_then_blasts_five_scattered_cells() {
    let (mut sim, _, registry, _, actor) = multiplayer_fixture(POWERUP_EXPLOSION as u8);
    let rules = residual_rules();
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .health
        .set_for_test(10000);
    let mut probe = sim.scenario_rng.clone();
    for _ in 0..5 {
        probe.next_range_i32_inclusive(0, 0x200);
        probe.next_u32();
    }

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        probe.native_state_hex(),
        "exactly two draws per blast"
    );
    // The direct 500, then up to five 500s whose scatter can reach the
    // picker's own cell ([Super] carries no CellSpread).
    let health = health_of(&sim, actor);
    assert!(
        (7000..=9500).contains(&health),
        "9500 less the blasts: {health}"
    );
    let blasts = anim_briefs(&sim);
    assert_eq!(blasts.len(), 5, "one SelectAnim construction per blast");
    for (name, _, flags, z_adjust) in &blasts {
        assert_eq!(name, "BOOM");
        assert_eq!(*flags, 0x2600);
        assert_eq!(*z_adjust, -15);
    }
    let flashes: Vec<i32> = sim
        .combat_light_requests
        .iter()
        .filter_map(|event| match event {
            crate::sim::combat::CombatLightRequest::Impact { damage, .. } => Some(*damage),
            _ => None,
        })
        .collect();
    assert_eq!(flashes, vec![500; 5], "one Flash per blast");
    assert!(sounds(&sim).is_empty(), "the arm skips the pickup sound");
}

/// `0x0048271E..0x00482840`: AnimTypeArray8B4154[0] at the componentwise
/// midpoint of the crate centre and the picker coordinate, then both damage
/// applications through the FlameDamage warhead — 50% verses on 600 twice,
/// 300 each — with no RNG draw. A bystander on the crate cell stays intact
/// because the area damage lands at the midpoint, not the crate cell.
#[test]
fn napalm_crate_burns_at_the_midpoint_with_the_flame_warhead_without_rng() {
    let (mut sim, _, registry, _, bystander) = multiplayer_fixture(POWERUP_NAPALM as u8);
    let rules = residual_rules();
    let owner = sim.interner.intern("H0");
    let picker = place_actor(
        &mut sim,
        &rules,
        owner,
        (11, 10),
        EntityCategory::Infantry,
        "E1",
    );
    for id in [bystander, picker] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(10000);
    }
    let rng = sim.scenario_rng.native_state_hex();

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        picker,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        rng,
        "Napalm spends no draw"
    );
    let centre = crate_center(
        &sim,
        CELL,
        sim.resolved_terrain.as_ref().map(|terrain| {
            crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(CELL)
        }),
    );
    let picker_coord = crate::sim::movement::ground_pose::position_world_coord(
        &sim.substrate.entities.get(picker).unwrap().position,
    );
    let expected_mid = crate::sim::anim_class::AnimWorldCoord {
        x: (centre.x + picker_coord.x) / 2,
        y: (centre.y + picker_coord.y) / 2,
        z: (centre.z + picker_coord.z) / 2,
    };
    assert_eq!(
        anim_briefs(&sim),
        vec![("FIRE1".to_owned(), expected_mid, 0x600, 0)],
        "one fire animation at the midpoint"
    );
    assert_eq!(health_of(&sim, picker), 9400, "two 300 Flame hits");
    assert_eq!(
        health_of(&sim, bystander),
        10000,
        "the area misses the crate cell"
    );
    assert!(sounds(&sim).is_empty(), "the arm skips the pickup sound");
}

/// `0x00482CA1..0x00482D56` on retail rules: FindFirstOfAction(Nuke) names
/// `NukeSpecial`, the picker House holds a MultiMissile Super, so the crate
/// grants `NukeSpecial` once (`Grant(1, 0, 0)`), charged, without a draw.
#[test]
fn retail_icbm_crate_grants_the_owner_a_one_time_nuke() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_ICBM as u8);
    crate::sim::superweapon::construct_house_supers(&mut sim, &rules, owner, false);
    let rng = sim.scenario_rng.native_state_hex();

    assert!(pickup_crate(
        &mut sim,
        &rules,
        &registry,
        CELL,
        actor,
        crate::sim::world::FrameEffects::default()
    ));

    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        rng,
        "ICBM spends no draw"
    );
    let nuke = sim.interner.get("NukeSpecial").unwrap();
    let granted: Vec<_> = sim.super_weapons[&owner]
        .iter()
        .filter(|(_, instance)| instance.is_active())
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(granted, vec![nuke], "only the first Action=Nuke type");
    let instance = &sim.super_weapons[&owner][&nuke];
    assert!(instance.is_ready(), "a one-time grant is charged at once");
    assert_eq!(
        serde_json::to_value(instance).unwrap()["one_time"],
        serde_json::json!(true)
    );
}

/// The Tiberium arm's fixture: ore-admitting terrain, a two-type `[Tiberiums]`
/// table with zero growth/spread percentages (an existing-cell re-hit refuses
/// without a draw) and the crate overlay beside the twelve flat ore variants.
fn tiberium_fixture() -> (Simulation, RuleSet, OverlayTypeRegistry, InternedId, u64) {
    let mut sim = sim_with_grid(31);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().crates = false;
    sim.session.game_options_mut_for_test().bases = false;
    // Mark's ordinary admission (0x004834A0, Track, both ignores) must admit
    // each cell, or mark_overlay answers Marked without writing the identity
    // and every placement silently no-ops; the retail Clear row carries
    // Track=100%.
    let mut terrain = crate::map::resolved_terrain::test_grid(40, 40, |rx, ry| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            allows_tiberium: true,
            ..crate::map::resolved_terrain::test_retail_clear_cell(rx, ry)
        }
    });
    terrain.set_land_speed_rules_for_test(&crate::map::resolved_terrain::test_retail_clear_rules());
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.session.set_binary_frame_for_test(100);
    sim.playfield_bounds_mut_for_test().as_mut().unwrap().base = 10;
    sim.set_playfield_size_height_for_test(Some(10));
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[AircraftTypes]\n[VehicleTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\n\
         [Tiberiums]\n0=Riparius\n1=Cruentus\n\
         [Riparius]\nImage=1\nValue=25\nGrowth=2200\nGrowthPercentage=0\nSpread=2200\nSpreadPercentage=0\n\
         [Cruentus]\nImage=2\nValue=50\nGrowth=10000\nGrowthPercentage=0\nSpread=10000\nSpreadPercentage=0\n\
         [CrateRules]\nCrateImg=SILVER\nWoodCrateImg=WOOD\nWaterCrateImg=WATER\nCrateRadius=3.0\n\
         [Powerups]\nTiberium=0,<none>,no\n",
    ))
    .unwrap();
    // The ore drivers size their per-type queues from the same tables.
    sim.ore_growth_state.reset_native_tiberium_classes_for_rect(
        (8, 8),
        rules.tiberium_types.len(),
        100,
    );
    let mut registry_text = String::from("[OverlayTypes]\n0=WOOD\n");
    for index in 1..=12 {
        registry_text.push_str(&format!("{index}=TIB{index:02}\n"));
    }
    registry_text.push_str("[WOOD]\nCrate=yes\nLand=Clear\n");
    for index in 1..=12 {
        registry_text.push_str(&format!("[TIB{index:02}]\nTiberium=yes\nLand=Tiberium\n"));
    }
    let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(&registry_text), None);
    place_registered_crate(&mut sim, &registry, CELL, POWERUP_TIBERIUM as u8);
    let owner = house(&mut sim, "H0", false);
    let actor = place_actor(
        &mut sim,
        &rules,
        owner,
        (CELL.0 as u16, CELL.1 as u16),
        EntityCategory::Infantry,
        "E1",
    );
    (sim, rules, registry, owner, actor)
}

/// Ore bales (density bytes) over every tiberium overlay on the grid.
fn total_bales(sim: &Simulation, registry: &OverlayTypeRegistry) -> u32 {
    let grid = sim.overlay_grid.as_ref().unwrap();
    let mut total = 0;
    for ry in 0..grid.height() {
        for rx in 0..grid.width() {
            let cell = grid.cell(rx, ry);
            if cell
                .overlay_id
                .is_some_and(|id| registry.flags(id).is_some_and(|flags| flags.tiberium))
            {
                total += u32::from(cell.overlay_data);
            }
        }
    }
    total
}
