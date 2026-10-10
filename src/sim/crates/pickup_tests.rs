//! `CellClass::PickupCrate @ 0x00481A00` through the complete dispatcher.
//!
//! The original-code rows (`tools/spatial_oracle/crate_pickup.json`, whose
//! sidecar bounds them to the guards, selection, removal and the Speed arm)
//! run through [`pickup_crate`] itself; the other arms are Rust regression
//! checks against the native reading cited at each arm.

use super::super::tests::{crate_cells, crate_registry, crate_ruleset, sim_with_grid};
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::components::Health;
use crate::sim::crates::CrateSlot;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::world::{PlacementEvidence, RevealPosition, RevealRequest, SimSoundEvent};
use crate::util::native_x87::NativeF64Bits;

const CELL: (i16, i16) = (10, 10);
const STOCK_WEIGHTS: [i32; POWERUP_COUNT] = [
    20, 20, 10, 0, 0, 0, 0, 0, 10, 10, 10, 10, 0, 0, 20, 0, 0, 0, 0,
];

fn house(sim: &mut Simulation, name: &str, player_control: bool) -> InternedId {
    let id = sim.interner.intern(name);
    let mut house = HouseState::new(id, 0, None, false, 0, 10);
    house.player_control = player_control;
    sim.houses.insert(id, house);
    id
}

/// A revealed Ground-layer object at the centre of `cell`.
fn place_actor(
    sim: &mut Simulation,
    owner: InternedId,
    cell: (u16, u16),
    category: EntityCategory,
    type_name: &str,
) -> u64 {
    let type_id = sim.interner.intern(type_name);
    let id = sim.allocate_stable_id();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        cell.0,
        cell.1,
        0,
        0,
        owner,
        Health { current: 100 },
        type_id,
        category,
        0,
        5,
        true,
    );
    entity.position.sub_x = SimFixed::from_num(128);
    entity.position.sub_y = SimFixed::from_num(128);
    sim.substrate.entities.insert(entity);
    sim.try_reveal_entity(
        id,
        RevealRequest {
            position: RevealPosition {
                exact_z_leptons: None,
                rx: cell.0,
                ry: cell.1,
                z: 0,
                sub_x: SimFixed::from_num(128),
                sub_y: SimFixed::from_num(128),
            },
            placement: PlacementEvidence::MarkSucceeded,
            logic_eligible: true,
        },
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
    *sim.crate_authority.slot_mut(index) = CrateSlot {
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
    sim.session.game_mode_nonzero = true;
    sim.session.game_options.crates = false;
    // Bases off: a house with no building and the credits of one Money crate
    // would otherwise be pre-empted onto the Unit slot (`0x00481BB8`).
    sim.session.game_options.bases = false;
    let mut rules = crate_ruleset("");
    rules.powerups.weights = STOCK_WEIGHTS;
    rules.crate_rules.radius = 768;
    sim.session.binary_frame = 100;
    sim.playfield_bounds.as_mut().unwrap().base = 10;
    sim.playfield_size_height = Some(10);
    let registry = crate_registry();
    place_registered_crate(&mut sim, &registry, CELL, slot);
    let owner = house(&mut sim, "H0", false);
    let actor = place_actor(
        &mut sim,
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
        sim.session.game_mode_nonzero = input["mode"].as_i64() != Some(0);
        // `0x00A8B261` is zero in the fixture: no replacement placer.
        sim.session.game_options.crates = false;
        sim.session.binary_frame = 100;
        sim.playfield_bounds.as_mut().unwrap().base = 10;
        sim.playfield_size_height = Some(10);
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
            *sim.crate_authority.slot_mut(index) = if occupied {
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
        owner_house.multiplay_passive = input["passive"].as_i64() == Some(1);
        sim.houses.insert(owner, owner_house);
        let actor = if input["null_actor"] == true {
            99
        } else {
            // The fixture's one actor always stands at (10, 10)'s centre,
            // whatever cell the crate occupies.
            place_actor(&mut sim, owner, (10, 10), EntityCategory::Infantry, "E1")
        };
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_before"].as_str().unwrap(),
            "{name}"
        );

        let answer = pickup_crate(&mut sim, &rules, &registry, cell, actor);

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

    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));

    assert_eq!(sim.houses[&owner].economy.credits(), expected);
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
    sim.session.game_options.crates = true;
    *sim.crate_authority.slot_mut(0) = CrateSlot {
        start_frame: 0,
        aux: 0,
        duration: 100,
        cell_x: CELL.0,
        cell_y: CELL.1,
    };

    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));

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

/// `0x00482972..0x00482B19`: every marked, `Trainable=` Techno within the
/// radius climbs one rank for a magnitude of 1; a civilian and a distant
/// soldier do not.
#[test]
fn veteran_crate_promotes_trainable_technos_within_the_radius() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_VETERAN as u8);
    let rules = effect_rules("[AudioVisual]\nCratePromoteSound=CratePromoted\n");
    let civilian = place_actor(&mut sim, owner, (10, 10), EntityCategory::Infantry, "CIV");
    let distant = place_actor(&mut sim, owner, (20, 20), EntityCategory::Infantry, "E1");

    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));

    let raw =
        |id: u64| f32::from_bits(sim.substrate.entities.get(id).unwrap().veterancy_raw.bits());
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
    sim.houses.get_mut(&owner).unwrap().player_control = true;
    let distant = place_actor(&mut sim, owner, (20, 20), EntityCategory::Infantry, "E1");

    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));
    let armor =
        |sim: &Simulation, id: u64| sim.substrate.entities.get(id).unwrap().armor_multiplier;
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
    let credits = sim.houses[&owner].economy.credits();
    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));
    assert_eq!(armor(&sim, actor).bits(), 1.5_f64.to_bits());
    assert!(
        sim.houses[&owner].economy.credits() > credits,
        "Money fallback"
    );

    place_registered_crate(&mut sim, &registry, CELL, POWERUP_FIREPOWER as u8);
    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));
    let firepower = sim
        .substrate
        .entities
        .get(actor)
        .unwrap()
        .firepower_multiplier;
    assert_eq!(firepower.bits(), 2.0_f64.to_bits());
    assert_eq!(
        sim.substrate
            .entities
            .get(distant)
            .unwrap()
            .firepower_multiplier,
        NativeF64Bits::ONE
    );
    assert!(eva_events(&sim).contains(&(owner, "EVA_UnitFirePowerUpgraded")));
}

/// `0x00481F9D..0x0048203C`: `MapClass::Reveal` for the actor's house.
#[test]
fn reveal_crate_reveals_the_whole_map_for_the_owner() {
    let (mut sim, _, registry, owner, actor) = multiplayer_fixture(POWERUP_REVEAL as u8);
    let rules = effect_rules("[AudioVisual]\nCrateRevealSound=CrateReveal\n");
    sim.fog.width = sim.session.map_width;
    sim.fog.height = sim.session.map_height;
    assert!(!sim.fog.whole_map_revealed_owners.contains(&owner));

    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));

    assert!(sim.fog.whole_map_revealed_owners.contains(&owner));
    // `0x0048202D..0x0048203C`: the one arm without the local-player test.
    assert_eq!(sounds(&sim), vec![("CrateReveal".to_string(), None)]);
}

/// The guards: a passive multiplayer owner, a non-crate overlay and a missing
/// actor consume nothing and spend no draw.
#[test]
fn guards_leave_the_crate_and_the_rng_alone() {
    let (mut sim, rules, registry, owner, actor) = multiplayer_fixture(10);
    let before = sim.scenario_rng.native_state_hex();
    sim.houses.get_mut(&owner).unwrap().multiplay_passive = true;
    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, actor));
    assert!(overlay_at(&sim, CELL).is_some());
    sim.houses.get_mut(&owner).unwrap().multiplay_passive = false;
    assert!(pickup_crate(&mut sim, &rules, &registry, CELL, 99));
    assert!(overlay_at(&sim, CELL).is_some());
    assert!(pickup_crate(&mut sim, &rules, &registry, (11, 11), actor));
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
             [InfantryTypes]\n0=E1\n[AircraftTypes]\n[VehicleTypes]\n0=TNK\n1=AMCV\n2=HARV\n[BuildingTypes]\n0=YARD\n\
             [E1]\nStrength=100\nSpeed=4\nSpeedType=Foot\nMovementZone=Normal\nOwner=Americans,Russians\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [TNK]\nStrength=300\nSpeed=5\nROT=5\nOwner=Americans,Russians\nCrateGoodie=yes\n\
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
    sim.session.game_mode_nonzero = true;
    sim.session.game_options.crates = false;
    // Bases off: no MCV pre-empt, and the random Unit draw refuses `BaseUnit=`.
    sim.session.game_options.bases = false;
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        HouseState::new(owner, 0, Some(owner), true, 5000, 10),
    );
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

/// The reads this chain adds: the seven `[AudioVisual]` crate sounds and
/// `CrateGoodie=`.
#[test]
fn crate_sound_and_goodie_keys_parse() {
    let (_, rules, _, _, _) = arena();
    assert_eq!(
        rules.general.crate_money_sound.as_deref(),
        Some("CrateMoney")
    );
    assert_eq!(
        rules.general.crate_unit_sound.as_deref(),
        Some("CrateFreeUnit")
    );
    assert_eq!(rules.general.crate_reveal_sound, None);
    assert!(rules.object("TNK").unwrap().crate_goodie);
    assert!(!rules.object("AMCV").unwrap().crate_goodie);
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

    assert!(!pickup_crate(&mut sim, &rules, &registry, (21, 22), actor));

    assert_eq!(tanks(&sim), 2);
    let free = sim
        .substrate
        .entities
        .values()
        .find(|entity| {
            entity.stable_id() != actor && sim.interner.resolve(entity.type_ref()) == "TNK"
        })
        .expect("the free tank");
    assert_eq!((free.position.rx, free.position.ry), (21, 22));
    assert!(free.lifecycle.cell_marked && !free.lifecycle.in_limbo);
    assert_eq!(free.owner(), owner);
    assert_eq!(overlay_at(&sim, (21, 22)), None);
    assert_eq!(
        sounds(&sim),
        vec![("CrateFreeUnit".to_string(), Some([owner, owner]))]
    );
}

/// `0x00481BB8..0x00481BFF`: with Bases on, a house with no tracked building,
/// more than 1500 credits and no tracked `BaseUnit=` is forced onto the Unit
/// slot and receives its first ownable `BaseUnit=` (`0x00482062..0x0048207F`),
/// whatever the crate held.
#[test]
fn mcv_preempt_grants_the_base_unit_with_bases_on() {
    let (mut sim, rules, registry, owner, actor) = arena();
    sim.session.game_options.bases = true;
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_MONEY as u8);

    assert!(!pickup_crate(&mut sim, &rules, &registry, (21, 22), actor));

    let mcv = sim
        .substrate
        .entities
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "AMCV")
        .expect("the free MCV");
    assert_eq!((mcv.position.rx, mcv.position.ry), (21, 22));
    assert_eq!(mcv.owner(), owner);
}

/// `0x00482B8F..0x00482C9C`: the HealBase sound, then `Health - Strength`
/// through `ReceiveDamage(.., C4Warhead=, .., 1, 1, ..)` on the actor
/// house's buildings only.
#[test]
fn heal_base_crate_restores_the_owners_buildings() {
    let (mut sim, rules, registry, owner, actor) = arena();
    let rival = sim.interner.intern("Russians");
    sim.houses.insert(
        rival,
        HouseState::new(rival, 1, Some(rival), false, 5000, 10),
    );
    let own = sim
        .spawn_object("YARD", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let theirs = sim
        .spawn_object("YARD", "Russians", 8, 20, 0, &rules)
        .unwrap();
    for id in [own, theirs] {
        sim.substrate.entities.get_mut(id).unwrap().health.current = 400;
    }
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_HEAL_BASE as u8);

    assert!(pickup_crate(&mut sim, &rules, &registry, (21, 22), actor));

    assert_eq!(
        sim.substrate.entities.get(own).unwrap().health.current,
        1000
    );
    assert_eq!(
        sim.substrate.entities.get(theirs).unwrap().health.current,
        400
    );
    // `[CrateRules]` retains the upper-cased name at its data boundary.
    assert_eq!(
        sounds(&sim),
        vec![("HEALCRATE".to_string(), Some([owner, owner]))]
    );
}

/// Drive `Force_Track` (`0x004B0D1B`): the supplied cell's crate is picked up
/// as the head is installed.
#[test]
fn force_track_onto_a_crate_cell_picks_it_up() {
    let (mut sim, rules, registry, owner, actor) = arena();
    place_registered_crate(&mut sim, &registry, (21, 22), POWERUP_MONEY as u8);
    let credits = sim.houses[&owner].economy.credits();

    assert!(sim.force_track(
        actor,
        -1,
        DriveCoord::cell(21, 22, 0),
        Some(&rules),
        Some(&registry)
    ));

    assert_eq!(overlay_at(&sim, (21, 22)), None);
    assert!(sim.houses[&owner].economy.credits() > credits);
}

/// Drive `Process_Movement` (`0x004B405D`/`0x004B46E6`) through the master
/// frame: a tank ordered across a crate cell picks the crate up.
#[test]
fn a_moving_tank_picks_up_the_crate_on_its_path() {
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules, registry, owner, actor) = arena();
    place_registered_crate(&mut sim, &registry, (22, 22), POWERUP_MONEY as u8);
    let credits = sim.houses[&owner].economy.credits();
    let grid = sim.path_grid_snapshot();
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Move {
            entity_id: actor,
            target_rx: 25,
            target_ry: 22,
            queue: false,
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
    assert!(sim.houses[&owner].economy.credits() > credits);
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
        Some(&rules),
        Some(&registry)
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
    let credits = sim.houses[&owner].economy.credits();
    let grid = sim.path_grid_snapshot();
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Move {
            entity_id: soldier,
            target_rx: 15,
            target_ry: 10,
            queue: false,
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
    assert!(sim.houses[&owner].economy.credits() > credits);
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
        sim.session.game_options.bases = bases;
        sim.houses.get_mut(&owner).unwrap().is_human = human;
        let before = sim.scenario_rng.native_state_hex();
        assert!(
            effects::choose_unit_crate_type(&mut sim, &rules, owner, false).is_none(),
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
    sim.session.game_options.bases = true;
    sim.houses.get_mut(&owner).unwrap().is_human = true;
    assert_eq!(
        effects::choose_unit_crate_type(&mut sim, &rules, owner, false)
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
    sim.session.game_options.bases = false;
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
        effects::choose_unit_crate_type(&mut sim, &rules, owner, false)
            .map(|object| object.id.as_str()),
        Some("TNK")
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected.native_state_hex()
    );
}
