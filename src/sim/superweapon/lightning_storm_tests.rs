//! The Lightning Storm (`lightning_storm`): its native comparisons against
//! `tools/superweapon_oracle.json` (`storm_start`, `storm_cloud`,
//! `storm_pixel_heights`, `storm_strike`, `storm_process`; `--check`
//! regenerates them), its strike's effects on the world, and a storm on
//! retail rules through production frames.
//!
//! The oracle stubs AnimClass's constructor, SelectAnim, the flash and
//! Apply_area_damage, recording each call, so a row's Scenario stream holds
//! the storm's own draws. VERA runs its ports of those calls; on the
//! fixture's anim types and empty cells they draw nothing either.

use super::chronosphere_tests::{charge_super, click, retail_rules_binding, step, world_with};
use super::lightning_storm::{self, LightningStorm, StrikeCell, debris_due};
use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::{ResolvedTerrainCell, test_grid};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::anim_class::AnimId;
use crate::sim::components::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::light_sources::LightingEvent;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::power_system::PowerState;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioLightingProfile;
use crate::sim::superweapon::cell_receiver_tests::test_terrain_cell;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::Value;

const STORM: &str = "LightningStormSpecial";
/// The oracle's anim types (`STORM_CLOUDS`, `STORM_BOLTS`, `STORM_DEBRIS`
/// and the explosion): name, SHP height, SHP frames.
const FIXTURE_ANIMS: [(&str, i32, i32); 9] = [
    ("WCCLOUD1", 80, 20),
    ("WCCLOUD2", 80, 21),
    ("WCLBOLT1", 381, 10),
    ("WCLBOLT2", 381, 11),
    ("WCLBOLT3", 381, 12),
    ("DBRIS1SM", 0, 9),
    ("DBRIS2SM", 0, 9),
    ("DBRIS3SM", 0, 9),
    ("WCBOLTX", 0, 9),
];
/// The oracle's map: cells beyond its 60-cell Size stay real cells.
const FIXTURE_MAP: u16 = 128;

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

fn triple(value: &Value) -> [i32; 3] {
    [int(&value[0]), int(&value[1]), int(&value[2])]
}

fn cell_of(value: &Value) -> (i16, i16) {
    (int(&value[0]) as i16, int(&value[1]) as i16)
}

/// The oracle's `LightningSounds=` hold sound indices; a name stands for
/// each.
fn sound_name(index: &Value) -> String {
    format!("Sound{}", int(index))
}

/// The oracle's Rules: retail's storm keys (`STORM_RETAIL`), its anim types
/// with their SHP heights and frames bound, `Sound17` for its one sound, and
/// a warhead for `LightningWarhead=`.
fn fixture_rules() -> RuleSet {
    let rules_ini = IniFile::from_str(
        "[General]\n\
         LightningDeferment=250\nLightningDamage=250\nLightningStormDuration=180\n\
         LightningHitDelay=10\nLightningScatterDelay=5\nLightningCellSpread=10\n\
         LightningSeparation=3\nLightningWarhead=IonWH\nWeatherConBoltExplosion=WCBOLTX\n\
         WeatherConClouds=WCCLOUD1,WCCLOUD2\nWeatherConBolts=WCLBOLT1,WCLBOLT2,WCLBOLT3\n\
         MetallicDebris=DBRIS1SM,DBRIS2SM,DBRIS3SM\n\
         [AudioVisual]\nLightningSounds=Sound17\nStormSound=WeatherIntro\n\
         [Warheads]\n0=IonWH\n\
         [IonWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
    );
    let art_ini = IniFile::from_str(
        &FIXTURE_ANIMS
            .iter()
            .map(|(name, ..)| format!("[{name}]\nRate=900\n"))
            .collect::<String>(),
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    for (name, height, frames) in FIXTURE_ANIMS {
        rules.bind_anim_frame_count_for_test(name, frames);
        rules.bind_anim_shp_height_for_test(name, height);
    }
    rules
}

/// A fixture cell: the row's Level (signed), bridge bit and land over a
/// flat passable cell.
fn fixture_cell(x: u16, y: u16, spec: Option<&Value>) -> ResolvedTerrainCell {
    let mut cell = test_terrain_cell(x, y);
    let Some(spec) = spec else {
        return cell;
    };
    if let Some(level) = spec.get("level").and_then(Value::as_i64) {
        cell.level = level as i8 as u8;
    }
    if spec.get("bridge").and_then(Value::as_bool) == Some(true) {
        cell.bridge_facts = BridgeCellFacts {
            raw_flags: BRIDGE_FLAG_STRUCTURAL,
            ..BridgeCellFacts::default()
        };
    }
    if let Some(land) = spec.get("land").and_then(Value::as_i64) {
        cell.yr_cell_land_type = land as u8;
    }
    cell
}

/// The oracle's world (`StormFixture`): a 128-cell square of `specs` cells
/// whose Size (`MapClass+0xF4/+0xF8`) is `size`; houses `H0..` from `houses`
/// (ally bits, defeated) with settled power states, `player` the PlayerPtr;
/// the Scenario stream seeded with `seed` at `frame`.
fn fixture_world(
    rules: &RuleSet,
    seed: i32,
    frame: i32,
    size: (i32, i32),
    specs: &[((u16, u16), &Value)],
    houses: &[Value],
    player: Option<usize>,
) -> (Simulation, Vec<InternedId>) {
    let mut sim = Simulation::with_seed(17);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.resolve_rule_animation_lists(rules);
    sim.scenario_rng = SimRng::new(seed as u64);
    sim.session.binary_frame = frame as u32;
    sim.session.map_width = FIXTURE_MAP;
    sim.session.map_height = FIXTURE_MAP;
    sim.resolved_terrain = Some(test_grid(FIXTURE_MAP, FIXTURE_MAP, |x, y| {
        fixture_cell(
            x,
            y,
            specs
                .iter()
                .find(|(at, _)| *at == (x, y))
                .map(|(_, spec)| *spec),
        )
    }));
    let span = i32::from(FIXTURE_MAP);
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: size.0,
        off_fc: -span,
        off_100: -span,
        off_104: span * 2,
        off_108: span * 2,
    });
    sim.playfield_size_height = Some(size.1);
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let mut ids = Vec::new();
    for (index, house) in houses.iter().enumerate() {
        let name = format!("H{index}");
        let id = sim.interner.intern(&name);
        let mut state = HouseState::new(id, 0, None, Some(index) == player, 0, 10);
        state.is_defeated = flag(&house[1]);
        sim.houses.insert(id, state);
        sim.session.house_order.push(id);
        sim.power_states.insert(id, PowerState::settled_for_test());
        let bits = int(&house[0]);
        let allies = sim.house_alliances.entry(name).or_default();
        for other in (0..31).filter(|other| bits & (1 << other) != 0) {
            allies.insert(format!("H{other}"));
        }
        ids.push(id);
    }
    sim.session.current_house = player.map(|index| ids[index]);
    (sim, ids)
}

fn house_index(houses: &[InternedId], owner: Option<InternedId>) -> Option<usize> {
    owner.map(|owner| houses.iter().position(|&house| house == owner).unwrap())
}

fn house_ref(houses: &[InternedId], index: &Value) -> Option<InternedId> {
    index.as_u64().map(|index| houses[index as usize])
}

/// Each house's radar outage (`+0x2B0`, `+0x2B8`) and recheck (`+0x5779`).
fn house_states(sim: &Simulation, houses: &[InternedId]) -> Vec<[i32; 3]> {
    houses
        .iter()
        .map(|house| {
            let state = &sim.power_states[house];
            let outage = state.radar_outage_for_test();
            [
                outage.start_frame(),
                outage.duration(),
                i32::from(state.radar_recheck_for_test()),
            ]
        })
        .collect()
}

fn expected_house_states(row: &Value) -> Vec<[i32; 3]> {
    row["house_states"]
        .as_array()
        .unwrap()
        .iter()
        .map(triple)
        .collect()
}

/// The storm's globals as the oracle's `storm()` reads them.
fn assert_globals(sim: &Simulation, houses: &[InternedId], storm: &Value, row: &Value) {
    let (active, time_to_end, deferment, duration, start, cell, owner) =
        sim.lightning_storm.globals_for_test();
    assert_eq!(
        (
            active,
            time_to_end,
            deferment,
            duration,
            start,
            cell,
            house_index(houses, owner)
        ),
        (
            flag(&storm["active"]),
            flag(&storm["time_to_end"]),
            int(&storm["deferment"]),
            int(&storm["duration"]),
            int(&storm["start"]),
            cell_of(&storm["coords"]),
            storm["owner"].as_u64().map(|owner| owner as usize),
        ),
        "{row}"
    );
}

/// The two lists as indices into `anims`.
fn lists(sim: &Simulation, anims: &[AnimId]) -> (Vec<usize>, Vec<usize>) {
    let index = |id: &AnimId| anims.iter().position(|anim| anim == id).unwrap();
    let (present, manifesting) = sim.lightning_storm.clouds_for_test();
    (
        present.iter().map(index).collect(),
        manifesting.iter().map(index).collect(),
    )
}

fn expected_lists(storm: &Value) -> (Vec<usize>, Vec<usize>) {
    let ids = |key: &str| {
        storm["lists"][key]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| int(index) as usize)
            .collect()
    };
    (ids("present"), ids("manifesting"))
}

/// What the storm reports per channel, from VERA and from a row's calls.
#[derive(Debug, Default, PartialEq)]
struct Calls {
    /// Radar events, the begun and approaching lines and cues, in order.
    sounds: Vec<Sound>,
    /// UpdateLighting calls.
    lighting: usize,
    /// The flashes' damage and coordinate.
    flashes: Vec<(i32, [i32; 3])>,
    /// The constructed anims' type, coordinate, draw flags and z adjust.
    anims: Vec<(String, [i32; 3], u32, i32)>,
}

#[derive(Debug, PartialEq)]
enum Sound {
    /// `CreateRadarEvent`'s native type and cell.
    Radar(i32, (i16, i16)),
    Began,
    Approaching,
    At(String, [i32; 3]),
}

impl Calls {
    fn of(sim: &Simulation, anims_before: usize) -> Self {
        let sounds = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::SuperWeaponRadarEvent { radar } => Some(Sound::Radar(
                    radar.event_type as i32,
                    (radar.rx as i16, radar.ry as i16),
                )),
                SimSoundEvent::LightningStormBegan => Some(Sound::Began),
                SimSoundEvent::LightningStormApproaching => Some(Sound::Approaching),
                SimSoundEvent::VocAt {
                    sound_id,
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    world_z_leptons,
                    ..
                } => Some(Sound::At(
                    sound_id.clone(),
                    [
                        i32::from(*rx) * 256 + sub_x.to_num::<i32>(),
                        i32::from(*ry) * 256 + sub_y.to_num::<i32>(),
                        *world_z_leptons,
                    ],
                )),
                _ => None,
            })
            .collect();
        let lighting = sim
            .lighting_sources
            .pending
            .iter()
            .filter(|event| matches!(event, LightingEvent::Global { .. }))
            .count();
        let flashes = sim
            .combat_light_requests
            .iter()
            .map(|request| {
                assert!(!request.force_create && request.flags == 0);
                (
                    request.damage,
                    [request.coord.x, request.coord.y, request.coord.z],
                )
            })
            .collect();
        let anims = sim
            .anims()
            .skip(anims_before)
            .map(|(_, anim)| {
                (
                    sim.interner.resolve(anim.type_id).to_string(),
                    [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                    anim.draw_flags,
                    anim.z_adjust,
                )
            })
            .collect();
        Self {
            sounds,
            lighting,
            flashes,
            anims,
        }
    }

    /// A row's calls: Start's PrintText block (StormSound, its line) is the
    /// begun line, Process's (the EVA line, its text) the approaching one.
    fn expected(row: &Value) -> Self {
        let mut calls = Self::default();
        for event in row["events"].as_array().unwrap() {
            match event[0].as_str().unwrap() {
                "radar_event" => calls
                    .sounds
                    .push(Sound::Radar(int(&event[1]), cell_of(&event[2]))),
                "text" if event[1] == "TXT_LIGHTNING_STORM" => calls.sounds.push(Sound::Began),
                "eva" => calls.sounds.push(Sound::Approaching),
                "play_at" => calls
                    .sounds
                    .push(Sound::At(sound_name(&event[1]), triple(&event[2]))),
                "update_lighting" => calls.lighting += 1,
                "flash" => calls.flashes.push((int(&event[1]), triple(&event[3]))),
                "anim" => {
                    let row = &event[4];
                    assert_eq!((int(&row[0]), int(&row[1]), int(&row[4])), (0, 1, 0));
                    calls.anims.push((
                        event[2].as_str().unwrap_or_default().to_string(),
                        triple(&event[3]),
                        int(&row[2]) as u32,
                        int(&row[3]),
                    ));
                }
                _ => {}
            }
        }
        calls
    }
}

fn assert_rng(sim: &Simulation, row: &Value) {
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_after"].as_str().unwrap(),
        "{row}"
    );
}

/// `LightningStorm::Start @ 0x00539EB0` against the native rows: the empty
/// cell's redraws over MapRect, the deferred branch's countdown minimum and
/// duration, a raging storm's retarget, and the start's radar event,
/// outages (allies and the defeated skipped), the player's recheck,
/// UpdateLighting and its PrintText block.
#[test]
fn storm_start_matches_native_rows() {
    let oracle = oracle();
    let rows = rows(&oracle, "storm_start");
    assert_eq!(rows.len(), 17);
    for row in rows {
        let mut rules = fixture_rules();
        rules.general.lightning_print_text = flag(&row["print_text"]);
        let frame = int(&row["frame"]);
        let (mut sim, houses) = fixture_world(
            &rules,
            int(&row["seed"]),
            frame,
            (int(&row["size"][0]), int(&row["size"][1])),
            &[],
            row["houses"].as_array().unwrap(),
            row["player"].as_u64().map(|player| player as usize),
        );
        sim.lightning_storm = LightningStorm::for_test(
            flag(&row["active"]),
            false,
            int(&row["current_deferment"]),
            int(&row["current_duration"]),
            frame - 500,
            (7, 8),
            None,
        );
        lightning_storm::start(
            &mut sim,
            &rules,
            int(&row["duration"]),
            int(&row["deferment"]),
            cell_of(&row["cell"]),
            house_ref(&houses, &row["owner"]),
        );
        assert_globals(&sim, &houses, &row["storm"], row);
        assert_eq!(Calls::of(&sim, 0), Calls::expected(row), "{row}");
        if int(&row["mute"]) == 0 {
            assert_eq!(
                house_states(&sim, &houses),
                expected_house_states(row),
                "{row}"
            );
        }
        assert_rng(&sim, row);
    }
}

/// `LightningStorm::CreateCloudBolt @ 0x0053A140` against the native rows:
/// the cloud's height from the cell's Level, its bridge bit and the first
/// bolt image's half height (0x006D2120), the type draw, the row and both
/// lists.
#[test]
fn storm_cloud_matches_native_rows() {
    let oracle = oracle();
    let rows = rows(&oracle, "storm_cloud");
    assert_eq!(rows.len(), 20);
    for row in rows {
        let mut rules = fixture_rules();
        rules.bind_anim_shp_height_for_test("WCLBOLT1", int(&row["bolt_height"]));
        let cell = cell_of(&row["cell"]);
        let at = (cell.0 as u16, cell.1 as u16);
        let (mut sim, houses) = fixture_world(
            &rules,
            int(&row["seed"]),
            1000,
            (60, 60),
            &[(at, row)],
            &[serde_json::json!([1, false])],
            Some(0),
        );
        let existing: Vec<AnimId> = (0..int(&row["present"]))
            .map(|_| {
                super::spawn_super_anim(&mut sim, &rules, "WCCLOUD1", [1000, 1000, 0]).unwrap()
            })
            .collect();
        sim.lightning_storm
            .set_clouds_for_test(existing.clone(), existing.clone());
        lightning_storm::create_cloud_bolt(&mut sim, &rules, cell);
        assert_globals(&sim, &houses, &row["storm"], row);
        let anims: Vec<AnimId> = sim.anims().map(|(&id, _)| id).collect();
        assert_eq!(lists(&sim, &anims), expected_lists(&row["storm"]), "{row}");
        assert_eq!(
            Calls::of(&sim, existing.len()),
            Calls::expected(row),
            "{row}"
        );
        assert_rng(&sim, row);
    }
}

/// `0x006D2120` against native execution: the sampled pixel counts, and an
/// FNV-1a digest of every half SHP height's result, which the oracle found
/// equal under the control words `0x0E7F`, `0x027F` and `0x037F`.
#[test]
fn pixel_heights_match_native_rows() {
    let oracle = oracle();
    let rows = &oracle["storm_pixel_heights"];
    for sample in rows["heights"].as_array().unwrap() {
        assert_eq!(
            crate::util::lepton::native_pixel_height_leptons(int(&sample[0])),
            int(&sample[1]),
            "{sample}"
        );
    }
    let (first, last) = (int(&rows["domain"][0]), int(&rows["domain"][1]));
    let digest = (first..=last).fold(crate::util::fnv::FNV1A64_OFFSET_BASIS, |hash, pixels| {
        crate::util::fnv::fnv1a64_fold_bytes(
            hash,
            &crate::util::lepton::native_pixel_height_leptons(pixels).to_le_bytes(),
        )
    });
    assert_eq!(format!("{digest:#018x}"), rows["fnv1a64"].as_str().unwrap());
}

/// The struck cell's occupants as GroundStrike reads them: a fixture object
/// per kind and slot (`set_occupants`), `after` replacing them and the
/// Level after the damage.
fn strike_cells(spec: &Value) -> (StrikeCell, StrikeCell) {
    let object = |kind: &Value, slot: u64| -> (Option<u64>, bool) {
        match kind {
            Value::Null => (None, false),
            Value::String(kind) => (Some(slot * 10), kind == "infantry"),
            Value::Array(pair) => (
                Some(slot * 10 + pair[1].as_u64().unwrap()),
                pair[0] == "infantry",
            ),
            other => panic!("{other}"),
        }
    };
    let read = |spec: &Value, base: &Value| {
        let field = |key: &str| {
            spec.get(key)
                .or_else(|| base.get(key))
                .unwrap_or(&Value::Null)
        };
        let (building, _) = object(field("building"), 1);
        let (nearest, nearest_is_infantry) = object(field("nearest"), 2);
        StrikeCell {
            building,
            nearest,
            nearest_is_infantry,
            level: field("level").as_i64().unwrap_or(0) as i32,
            land: field("land").as_i64().unwrap_or(0) as i32,
        }
    };
    let before = read(spec, spec);
    let after = read(spec.get("after").unwrap_or(&Value::Null), spec);
    (before, after)
}

/// `LightningStorm::GroundStrike @ 0x0053A300` against the native rows. The
/// debris test (`0x0053A513..0x0053A61B`) runs over every row's occupants;
/// a row with an empty cell replays whole: the bolt (on the cell's floor,
/// flat here, so the row whose stub answers 150 is checked on x and y), the
/// cue, the explosion's row, the flash, the debris and the Scenario stream.
#[test]
fn storm_strike_matches_native_rows() {
    let oracle = oracle();
    let rows = rows(&oracle, "storm_strike");
    assert_eq!(rows.len(), 38);
    let mut replayed = 0;
    for row in rows {
        let spec = &row["cell"];
        let debris = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event[0] == "draw" && int(&event[2]) == 2 && int(&event[3]) == 4);
        let (before, after) = strike_cells(spec);
        assert_eq!(debris_due(&before, &after), debris, "{row}");
        if ["building", "nearest", "after"]
            .iter()
            .any(|key| spec.get(key).is_some())
        {
            continue;
        }

        let mut rules = fixture_rules();
        rules.general.lightning_sounds = row["sounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(sound_name)
            .collect();
        if row["explosion"].is_null() {
            rules.general.weather_con_bolt_explosion = String::new();
        }
        let coords = triple(&row["coords"]);
        let at = ((coords[0] >> 8) as u16, (coords[1] >> 8) as u16);
        let (mut sim, houses) = fixture_world(
            &rules,
            int(&row["seed"]),
            1000,
            (60, 60),
            &[(at, spec)],
            &[serde_json::json!([1, false]), serde_json::json!([2, false])],
            Some(0),
        );
        sim.metallic_debris = row["debris"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| sim.interner.intern(name.as_str().unwrap()))
            .collect();
        let owner = house_ref(&houses, &row["owner"]);
        sim.lightning_storm =
            LightningStorm::for_test(true, false, 0, 180, 0, (at.0 as i16, at.1 as i16), owner);
        let _ = lightning_storm::ground_strike(&mut sim, &rules, None, coords);

        let mut expected = Calls::expected(row);
        let mut seen = Calls::of(&sim, 0);
        if row["explosion"].is_null() {
            // RESIDUAL (module doc): VERA constructs nothing for a null type.
            expected.anims.retain(|(name, ..)| !name.is_empty());
        }
        if !spec["floor"].is_null() {
            // The stub's floor stands for a slope; the fixture cell is flat.
            seen.anims[0].1[2] = expected.anims[0].1[2];
        }
        assert_eq!(seen, expected, "{row}");
        assert_rng(&sim, row);
        replayed += 1;
    }
    assert_eq!(replayed, 29);
}

/// `LightningStorm::Process @ 0x0053A6C0` against the native rows: the
/// countdown, its warnings and its end calling Start; the cadences and
/// their scatter (separation, In_Bounds, three tries); the duration; the
/// lists, strikes past half a cloud's frames (last first) and present
/// clouds leaving at their last frame; and the end once no cloud is
/// present. BoltsPresent is not kept (module doc).
#[test]
fn storm_process_matches_native_rows() {
    let oracle = oracle();
    let rows = rows(&oracle, "storm_process");
    assert_eq!(rows.len(), 32);
    for row in rows {
        let mut rules = fixture_rules();
        let keys = &row["rules"];
        let general = &mut rules.general;
        general.lightning_damage = int(&keys["damage"]);
        general.lightning_deferment = int(&keys["deferment"]);
        general.lightning_storm_duration = int(&keys["duration"]);
        general.lightning_hit_delay = int(&keys["hit_delay"]);
        general.lightning_scatter_delay = int(&keys["scatter_delay"]);
        general.lightning_cell_spread = int(&keys["spread"]);
        general.lightning_separation = int(&keys["separation"]);
        general.lightning_print_text = flag(&row["print_text"]);
        let specs: Vec<((u16, u16), &Value)> = row["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    (int(&entry[0][0]) as u16, int(&entry[0][1]) as u16),
                    &entry[1],
                )
            })
            .collect();
        let (mut sim, houses) = fixture_world(
            &rules,
            int(&row["seed"]),
            int(&row["frame"]),
            (int(&row["size"][0]), int(&row["size"][1])),
            &specs,
            row["houses"].as_array().unwrap(),
            Some(0),
        );
        let before = &row["storm_before"];
        let field = |key: &str, default: i32| before.get(key).map_or(default, int);
        sim.lightning_storm = LightningStorm::for_test(
            before.get("active").is_some_and(flag),
            before.get("time_to_end").is_some_and(flag),
            field("deferment", 0),
            field("duration", 180),
            field("start", 0),
            before.get("coords").map_or((40, 40), cell_of),
            match before.get("owner") {
                Some(owner) => house_ref(&houses, owner),
                None => Some(houses[0]),
            },
        );
        let mut anims = Vec::new();
        for anim in row["anims_before"].as_array().unwrap() {
            let id = super::spawn_super_anim(
                &mut sim,
                &rules,
                anim[0].as_str().unwrap(),
                triple(&anim[1]),
            )
            .unwrap();
            sim.substrate
                .anims
                .get_mut(id)
                .unwrap()
                .runtime
                .current_frame = int(&anim[2]);
            anims.push(id);
        }
        let pick = |key: &str| -> Vec<AnimId> {
            row[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| anims[int(index) as usize])
                .collect()
        };
        sim.lightning_storm
            .set_clouds_for_test(pick("present"), pick("manifesting"));
        let anims_before = anims.len();

        let _ = lightning_storm::process(&mut sim, &rules, None);

        assert_globals(&sim, &houses, &row["storm"], row);
        let all: Vec<AnimId> = sim.anims().map(|(&id, _)| id).collect();
        assert_eq!(lists(&sim, &all), expected_lists(&row["storm"]), "{row}");
        let table: Vec<(usize, String, [i32; 3], i32)> = sim
            .anims()
            .enumerate()
            .map(|(index, (_, anim))| {
                (
                    index,
                    sim.interner.resolve(anim.type_id).to_string(),
                    [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                    anim.runtime.current_frame,
                )
            })
            .collect();
        let expected_table: Vec<(usize, String, [i32; 3], i32)> = row["anims"]
            .as_array()
            .unwrap()
            .iter()
            .map(|anim| {
                (
                    int(&anim[0]) as usize,
                    anim[1].as_str().unwrap().to_string(),
                    triple(&anim[2]),
                    int(&anim[3]),
                )
            })
            .collect();
        assert_eq!(table, expected_table, "{row}");
        assert_eq!(Calls::of(&sim, anims_before), Calls::expected(row), "{row}");
        assert_eq!(
            house_states(&sim, &houses),
            expected_house_states(row),
            "{row}"
        );
        assert_rng(&sim, row);
    }
}

fn lighting_timing_rules(deferment: i32, duration: i32, rate: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\n\
         LightningDeferment={deferment}\n\
         LightningStormDuration={duration}\n\
         LightningHitDelay=1000\n\
         LightningScatterDelay=1000\n\
         AmbientChangeRate={rate}\n\
         AmbientChangeStep=.2\n"
    )))
    .expect("lighting timing rules should parse")
}

fn count(sim: &Simulation, wanted: fn(&SimSoundEvent) -> bool) -> usize {
    sim.sound_events
        .iter()
        .filter(|event| wanted(event))
        .count()
}

fn began(event: &SimSoundEvent) -> bool {
    matches!(event, SimSoundEvent::LightningStormBegan)
}

fn launched(event: &SimSoundEvent) -> bool {
    matches!(event, SimSoundEvent::SuperWeaponLaunched { .. })
}

fn launch(sim: &mut Simulation, rules: &RuleSet) -> InternedId {
    let owner = sim.interner.intern("Americans");
    let sw_type = charge_super(sim, owner, STORM);
    assert!(lightning_storm::launch(sim, rules, owner, sw_type, (8, 9)));
    owner
}

/// A storm through production frames: the countdown's last Process starts
/// it after that frame's ambient rung; it rages through `start + duration`,
/// the next Process marks its end and the one after, with no cloud present,
/// restores the ordinary profile, again after the rung. No cloud gathers on
/// these frames, so nothing draws.
#[test]
fn a_storm_starts_and_ends_after_the_frames_ambient_rung() {
    let rules = lighting_timing_rules(1, 2, ".0012");
    assert_eq!(rules.general.ambient_change_interval_frames, 1);
    let mut sim = Simulation::with_seed(0x420);
    let rng_before = sim.scenario_rng.state();
    launch(&mut sim, &rules);
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Normal,
        "a deferred launch selects nothing"
    );
    let globals = |sim: &Simulation| {
        let (active, time_to_end, ..) = sim.lightning_storm.globals_for_test();
        (active, time_to_end)
    };

    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(globals(&sim), (true, false));
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Ion
    );
    assert_eq!(sim.session.lighting.target_ambient, 87);
    assert_eq!(
        sim.session.lighting.current_ambient, 100,
        "the frame's ambient rung ran before the start"
    );

    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(globals(&sim), (true, false));
    assert_eq!(sim.session.lighting.current_ambient, 87);
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(globals(&sim), (true, false), "start + duration still rages");
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(globals(&sim), (true, true));
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Ion
    );

    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(globals(&sim), (false, false));
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Normal
    );
    assert_eq!(sim.session.lighting.target_ambient, 100);
    assert_eq!(
        sim.session.lighting.current_ambient, 87,
        "the end selects Normal after this frame's ambient rung"
    );

    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(sim.session.lighting.current_ambient, 100);
    assert_eq!(sim.scenario_rng.state(), rng_before);
}

/// `StormSound` and the storm's line belong to the countdown's end:
/// `SuperClass::Launch` case 2 hands `LightningDeferment=` to
/// `LightningStorm::Start @ 0x00539EB0`, whose deferred branch returns
/// before them (`0x00539F80`), and Process re-enters Start at zero
/// (`0x0053AACA`). The launch's EVA line is the app's, at the launch.
#[test]
fn the_storm_cue_lands_on_the_countdowns_end_not_on_the_launch() {
    let rules = lighting_timing_rules(3, 2, ".2");
    let mut sim = Simulation::with_seed(0x422);
    launch(&mut sim, &rules);
    assert_eq!(count(&sim, began), 0, "a deferred launch returns first");
    assert_eq!(count(&sim, launched), 1);
    for frame in 1..=2 {
        lightning_storm::process(&mut sim, &rules, None);
        assert_eq!(count(&sim, began), 0, "countdown frame {frame}");
    }
    lightning_storm::process(&mut sim, &rules, None);
    assert_eq!(count(&sim, began), 1);
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Ion
    );
    lightning_storm::process(&mut sim, &rules, None);
    assert_eq!(count(&sim, began), 1, "a storm starts once");
}

/// `LightningDeferment=0` skips Start's deferred branch, so the storm
/// starts inside `SuperClass::Launch`, before the case's EVA line
/// (`0x006CCD81`).
#[test]
fn a_zero_deferment_storm_starts_on_the_launch_frame() {
    let rules = lighting_timing_rules(0, 2, ".2");
    let mut sim = Simulation::with_seed(0x423);
    launch(&mut sim, &rules);
    let position = |wanted: fn(&SimSoundEvent) -> bool| {
        sim.sound_events.iter().position(|event| wanted(event))
    };
    assert!(position(began).unwrap() < position(launched).unwrap());
}

/// `LightningStormDuration=-1` never ends (`0x0053A919 CMP EAX,-1`).
#[test]
fn a_storm_of_duration_minus_one_rages_on() {
    let rules = lighting_timing_rules(0, -1, ".2");
    let mut sim = Simulation::with_seed(0x421);
    launch(&mut sim, &rules);
    for _ in 0..4 {
        lightning_storm::process(&mut sim, &rules, None);
        let (active, time_to_end, _, duration, ..) = sim.lightning_storm.globals_for_test();
        assert_eq!((active, time_to_end, duration), (true, false, -1));
        assert_eq!(
            sim.session.lighting.selected_profile,
            ScenarioLightingProfile::Ion
        );
    }
}

/// With no storm, Process neither relights nor draws.
#[test]
fn a_map_without_a_storm_keeps_its_lighting_and_stream() {
    let rules = lighting_timing_rules(250, 180, ".2");
    let mut sim = Simulation::with_seed(0x42);
    let lighting_before = sim.session.lighting;
    let rng_before = sim.scenario_rng.state();
    for _ in 0..400 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert_eq!(sim.session.lighting, lighting_before);
    assert_eq!(sim.scenario_rng.state(), rng_before);
}

/// LWH is declared only by `[Warheads]`; no object or ordinary weapon
/// points to it, as retail's IonWH.
fn registry_only_warhead_rules() -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=DUMMY\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAPOWR\n\n\
         [Warheads]\n0=LWH\n\n\
         [DUMMY]\nStrength=100\nArmor=none\nSpeed=4\n\n\
         [GAPOWR]\nStrength=200\nArmor=wood\n\n\
         [General]\nLightningDamage=100\nLightningWarhead=LWH\n\
         WeatherConBoltExplosion=EXPLOSION\nWeatherConBolts=WCLBOLT1,WCLBOLT2,WCLBOLT3\n\n\
         [LWH]\nCellSpread=1\nPercentAtMax=1\nAnimList=EXPLOSION\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("lightning test rules should parse");
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[EXPLOSION]\nRate=900\n[WCLBOLT1]\nLayer=ground\n[WCLBOLT2]\nLayer=ground\n\
         [WCLBOLT3]\nLayer=ground\n",
    ));
    for name in ["EXPLOSION", "WCLBOLT1", "WCLBOLT2", "WCLBOLT3"] {
        art.bind_anim_frame_count_for_test(name, 10);
    }
    rules.replace_art_registry_for_test(art);
    rules
}

/// A strike on cell `(x, y)` of a storm the Americans' raging there.
fn strike(
    sim: &mut Simulation,
    rules: &RuleSet,
    (x, y): (u16, u16),
    registry: Option<&OverlayTypeRegistry>,
) {
    let owner = sim.interner.intern("Americans");
    sim.lightning_storm = LightningStorm::raging_for_test(owner, (x as i16, y as i16));
    let _ = lightning_storm::ground_strike(
        sim,
        rules,
        registry,
        [i32::from(x) * 256 + 128, i32::from(y) * 256 + 128, 0],
    );
}

/// The bolt and the `WeatherConBoltExplosion=` explosion are AnimClass
/// instances at the struck cell; the explosion's smudge is the anim's own,
/// not deferred.
#[test]
fn a_strike_constructs_its_bolt_and_explosion_at_the_cell() {
    let rules = registry_only_warhead_rules();
    let mut sim = Simulation::with_seed(1);
    strike(&mut sim, &rules, (5, 5), None);
    assert!(sim.pending_smudge_requests.is_empty());
    let at_cell = |anim: &crate::sim::anim_class::AnimObject| {
        let (rx, ry, ..) = anim.world_coord.to_cell_sub_z();
        (rx, ry) == (5, 5)
    };
    let explosion = sim.interner.intern("EXPLOSION");
    assert!(
        sim.anims().any(|(_, anim)| anim.type_id == explosion
            && at_cell(anim)
            && anim.draw_flags == 0x2600
            && anim.z_adjust == -15),
        "the explosion takes the combat explosion row"
    );
    assert!(
        sim.anims().any(|(_, anim)| {
            sim.interner.resolve(anim.type_id).starts_with("WCLBOLT")
                && at_cell(anim)
                && anim.draw_flags == 0x600
        }),
        "the bolt takes the (0, 1, 0x600, 0, 0) row"
    );
}

/// The explosion's crater (AnimClass::Start) reduces the ore before the
/// area damage clears what is left; dense ore still blocks the crater.
#[test]
fn a_strikes_explosion_precedes_its_area_damage_on_ore() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=LWH\n\
         [OverlayTypes]\n0=ORE\n\
         [SmudgeTypes]\n0=CR1\n\
         [Tiberiums]\n0=Riparius\n\
         [General]\nLightningDamage=100\nLightningWarhead=LWH\n\
         WeatherConBoltExplosion=EXPLOSION\nWeatherConBolts=WCLBOLT1,WCLBOLT2,WCLBOLT3\n\
         [LWH]\nCellSpread=0\nAnimList=EXPLOSION\nTiberium=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ORE]\nTiberium=yes\nChainReaction=yes\n\
         [Riparius]\nImage=1\nValue=25\n\
         [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("ore-order lightning rules");
    rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(
        "[EXPLOSION]\nCrater=yes\nScorch=no\nFrameWidth=100\nFrameHeight=100\n",
    )));
    let overlay_registry = OverlayTypeRegistry::from_ini(&ini, None);
    let ore_id = overlay_registry.id_for_name("ORE").expect("ORE overlay id");

    let mut sim = Simulation::with_seed(1);
    sim.resolved_terrain = Some(test_grid(10, 10, |rx, ry| ResolvedTerrainCell {
        filled_clear: true,
        accepts_smudge: true,
        allows_tiberium: true,
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }));
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(10, 10));
    sim.production.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
    let mut overlay = OverlayGrid::new(10, 10);
    // Raw data 9 is ten density units. The crater reduces six and stays
    // blocked by the surviving overlay; Damage=100 then clears the four left.
    overlay.place_overlay(5, 5, ore_id, 9);
    sim.overlay_grid = Some(overlay);

    strike(&mut sim, &rules, (5, 5), Some(&overlay_registry));

    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None,
        "the area damage clears the reduced ore"
    );
    assert!(
        sim.smudge_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .type_id
            .is_none(),
        "the explosion's crater met dense ore"
    );
    assert!(sim.pending_smudge_requests.is_empty());
    // The bolt's type is the only draw.
    let mut expected = SimRng::new(1);
    let _ = expected.next_u32();
    assert_eq!(sim.scenario_rng.state(), expected.state());
}

/// A wall the strike destroys publishes its navigation and radar changes
/// inline.
#[test]
fn a_strike_destroying_a_wall_publishes_navigation_and_radar_inline() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=IonWH\n\
         [OverlayTypes]\n0=TESTWALL\n\
         [General]\nLightningDamage=100\nLightningWarhead=IonWH\n\
         [IonWH]\nCellSpread=0\nWall=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=1\n",
    );
    let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
    let rules = RuleSet::from_ini(&ini).expect("lightning wall rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let wall_id = registry.id_for_name("TESTWALL").expect("test wall id");

    let mut terrain = test_grid(12, 12, crate::map::resolved_terrain::test_flat_cell);
    let mut overlays = OverlayGrid::new(12, 12);
    overlays.place_overlay(5, 5, wall_id, 0);
    assert!(crate::sim::overlay_grid::recalc_overlay_passability(
        &mut overlays,
        &mut terrain,
        &registry,
        5,
        5,
    ));
    let _ = overlays.take_dirty_cells();

    let mut sim = Simulation::with_seed(1);
    sim.overlay_grid = Some(overlays);
    sim.resolved_terrain = Some(terrain);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert!(!sim.path_grid().unwrap().is_walkable(5, 5));
    let ground_zone = |sim: &Simulation| {
        sim.zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(5, 5, MovementLayer::Ground)
    };
    assert_eq!(
        ground_zone(&sim),
        crate::sim::pathfinding::zone_map::ZONE_INVALID
    );

    strike(&mut sim, &rules, (5, 5), Some(&registry));

    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None
    );
    assert!(sim.path_grid().unwrap().is_walkable(5, 5));
    assert_ne!(
        ground_zone(&sim),
        crate::sim::pathfinding::zone_map::ZONE_INVALID
    );
    assert_eq!(
        sim.radar_terrain_dirty_cells,
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (4, 4),
            (5, 4),
            (4, 6),
            (3, 5),
            (4, 5),
            (6, 6),
            (5, 7),
            (5, 6),
            (7, 5),
            (6, 5),
        ]
    );
    assert_eq!(sim.radar_terrain_dirty_generation, 13);
    assert_eq!(
        sim.tactical_dirty_cells,
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (5, 5),
            (4, 4),
            (5, 4),
            (4, 4),
            (5, 5),
            (4, 6),
            (3, 5),
            (4, 5),
            (5, 5),
            (6, 6),
            (5, 7),
            (4, 6),
            (5, 6),
            (6, 4),
            (7, 5),
            (6, 6),
            (5, 5),
            (6, 5),
        ]
    );
}

/// A bridge cell's strike lands on the deck (`+0x140 & 0x100` adds the
/// bridge height), so only the deck's occupant is hit.
#[test]
fn a_bridge_strike_hits_the_deck_only() {
    let rules = registry_only_warhead_rules();
    let mut sim = Simulation::with_seed(1);
    let owner = sim.interner.intern("Soviet");
    let type_ref = sim.interner.intern("DUMMY");
    for (id, on_bridge, layer) in [
        (1, false, MovementLayer::Ground),
        (2, true, MovementLayer::Bridge),
    ] {
        // Both stand in the cell's lists, on the map: out of limbo and
        // marked (`+0x74`), which Apply_area_damage's dispatch reads.
        let mut entity = GameEntity::test_default(id, "DUMMY", "Soviet", 5, 5);
        entity.owner = owner;
        entity.type_ref = type_ref;
        entity.health = Health { current: 100 };
        entity.on_bridge = on_bridge;
        if on_bridge {
            entity.position.z = 4;
        }
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        sim.substrate.entities.insert(entity);
        sim.substrate
            .occupancy
            .add(5, 5, id, layer, None, CellListInsertion::PrependNonBuilding);
    }
    sim.resolved_terrain = Some(test_grid(10, 10, |rx, ry| {
        let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
        if (rx, ry) == (5, 5) {
            cell.bridge_facts = BridgeCellFacts {
                raw_flags: BRIDGE_FLAG_STRUCTURAL,
                ..BridgeCellFacts::default()
            };
            cell.has_bridge_deck = true;
            cell.bridge_walkable = true;
            cell.bridge_deck_level = 4;
        }
        cell
    }));

    strike(&mut sim, &rules, (5, 5), None);

    assert_eq!(sim.substrate.entities.get(1).unwrap().health.current, 100);
    assert_eq!(sim.substrate.entities.get(2).unwrap().health.current, 0);
}

/// The strike damages a building in the cell and its damage state follows.
#[test]
fn a_strike_damages_a_building() {
    let rules = registry_only_warhead_rules();
    let mut sim = Simulation::with_seed(1);
    let mut building = GameEntity::test_default_of_category(
        10,
        "GAPOWR",
        "Soviet",
        5,
        5,
        EntityCategory::Structure,
    );
    building.lifecycle.in_limbo = false;
    building.lifecycle.cell_marked = true;
    building.owner = sim.interner.intern("Soviet");
    building.type_ref = sim.interner.intern("GAPOWR");
    building.health = Health { current: 150 };
    sim.substrate.entities.insert(building);

    strike(&mut sim, &rules, (5, 5), None);

    let building = sim.substrate.entities.get(10).expect("building remains");
    assert_eq!(building.health.current, 50);
    assert!(matches!(
        building.health.compare_ratio(
            rules
                .object(sim.interner.resolve(building.type_ref()))
                .unwrap()
                .strength,
            rules.general.condition_yellow,
        ),
        crate::util::native_x87::MaskedX87Ordering::Less
            | crate::util::native_x87::MaskedX87Ordering::Equal
    ));
}

/// A strike that kills a unit runs its death transaction inline: the death
/// weapon's wall damage lands within the strike, and one hit point short of
/// the kill leaves the wall and the unit.
#[test]
fn a_fatal_strike_runs_the_death_transaction_inline() {
    fn run(carrier_hp: i32) -> (Simulation, u64) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=BOOMER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=LightningWH\n1=WallWH\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [BOOMER]\nStrength=101\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
             [DeathBoom]\nDamage=214\nWarhead=WallWH\n\
             [General]\nLightningDamage=100\nLightningWarhead=LightningWH\n\
             WeatherConBolts=WCLBOLT1,WCLBOLT2,WCLBOLT3\nMetallicDebris=DBRIS1SM\n\
             [LightningWH]\nCellSpread=1\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [WallWH]\nCellSpread=0\nWall=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
        );
        let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
        let rules = RuleSet::from_ini(&ini).expect("lightning death transaction rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
        let mut sim = Simulation::with_seed(1);
        sim.resolve_rule_animation_lists(&rules);
        let mut carrier = GameEntity::test_default(10, "BOOMER", "Soviet", 5, 5);
        carrier.owner = sim.interner.intern("Soviet");
        carrier.type_ref = sim.interner.intern("BOOMER");
        carrier.health = Health {
            current: carrier_hp,
        };
        sim.substrate.entities.insert(carrier);
        let _ = sim.reveal(10);
        let mut overlays = OverlayGrid::new(12, 12);
        overlays.place_overlay(5, 5, 0, 0);
        sim.overlay_grid = Some(overlays);
        sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(12, 12));
        strike(&mut sim, &rules, (5, 5), Some(&registry));
        let rng = sim.scenario_rng.state();
        (sim, rng)
    }

    let (fatal, fatal_rng) = run(100);
    assert!(fatal.substrate.entities.get(10).is_some_and(|entity| {
        entity.health.current == 0 && entity.dying && !entity.in_logic_vector
    }));
    assert_eq!(
        fatal.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None
    );
    assert!(fatal.substrate.pending_delete.contains(&10));
    assert!(!fatal.live_object_order_snapshot().contains(&10));
    // The bolt's type, the death weapon's wall damage, then the debris
    // count: the death emptied the cell. With one MetallicDebris= type the
    // pieces draw nothing.
    let mut expected = SimRng::new(1);
    let _ = expected.next_u32();
    let _ = expected.next_range_u32_inclusive(0, 400);
    let _ = expected.next_range_i32_inclusive(2, 4);
    assert_eq!(fatal_rng, expected.state());

    let (boundary, boundary_rng) = run(101);
    assert_eq!(
        boundary
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .overlay_id,
        Some(0)
    );
    assert!(boundary.substrate.pending_delete.is_empty());
    assert!(boundary.live_object_order_snapshot().contains(&10));
    assert_eq!(
        boundary.substrate.entities.get(10).unwrap().health.current,
        1
    );
    let mut expected = SimRng::new(1);
    let _ = expected.next_u32();
    assert_eq!(boundary_rng, expected.state());
}

/// Retail rules with the storm's SHP frame counts bound (`WCCLOUD1..3`,
/// `WCLBOLT1..3`, `EXPLOLB`) and `WCLBOLT1.SHP`'s height (108x370), which
/// sets the clouds' height.
fn retail_rules() -> Option<RuleSet> {
    let mut rules = retail_rules_binding(&[
        ("WCCLOUD1", 59),
        ("WCCLOUD2", 60),
        ("WCCLOUD3", 60),
        ("WCLBOLT1", 3),
        ("WCLBOLT2", 3),
        ("WCLBOLT3", 3),
        ("EXPLOLB", 31),
    ])?;
    rules.bind_anim_shp_height_for_test("WCLBOLT1", 370);
    Some(rules)
}

/// The keys the chain reads, through the production readers on the retail
/// INIs.
#[test]
fn retail_storm_rules() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let general = &rules.general;
    assert_eq!(
        (
            general.lightning_deferment,
            general.lightning_damage,
            general.lightning_storm_duration,
            general.lightning_hit_delay,
            general.lightning_scatter_delay,
            general.lightning_cell_spread,
            general.lightning_separation,
        ),
        (250, 250, 180, 10, 5, 10, 3)
    );
    assert_eq!(general.lightning_warhead, "IonWH");
    assert_eq!(general.weather_con_bolt_explosion, "EXPLOLB");
    assert_eq!(
        general.weather_con_clouds,
        ["WCCLOUD1", "WCCLOUD2", "WCCLOUD3"]
    );
    assert_eq!(
        general.weather_con_bolts,
        ["WCLBOLT1", "WCLBOLT2", "WCLBOLT3"]
    );
    // ReadGeneral's 0x80-byte read of the twenty names keeps fourteen and a
    // cut `D` (`0x0066DA90`), a type with no image.
    assert_eq!(general.metallic_debris.len(), 15);
    assert_eq!(
        general.metallic_debris.last().map(String::as_str),
        Some("D")
    );
    assert_eq!(general.lightning_sounds, ["WeatherStrike"]);
    assert_eq!(general.storm_sound.as_deref(), Some("WeatherIntro"));
    assert!(general.lightning_print_text);
    let sw = rules.super_weapon(STORM).unwrap();
    assert_eq!(sw.kind, SuperWeaponKind::LightningStorm);
}

/// A storm on retail rules through production frames: the click launches
/// it and the countdown warns at 225 frames left and starts it at zero,
/// putting the Russians (not the Americans) under a radar outage; clouds
/// gather and strike, damaging the tank under the storm's cell; once the
/// duration is over and the last cloud has played out the storm ends.
#[test]
fn retail_storm_strikes_through_production_frames() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let russians = sim.interner.intern("Russians");
    let target = (30, 30);
    let tank = sim
        .spawn_object_at_height("HTNK", "Russians", target.0, target.1, 0, 0, &rules)
        .expect("HTNK stands at the target");
    let strength = sim.substrate.entities.get(tank).unwrap().health.current;
    let sw_type = charge_super(&mut sim, americans, STORM);

    click(&mut sim, &rules, americans, STORM, target);
    assert!(!sim.super_weapons[&americans][&sw_type].is_ready);
    assert!(sim.sound_events.iter().any(launched));
    assert!(lightning_storm::has_deferment(&sim) && !lightning_storm::raging(&sim));

    // The app drains each frame's events; so does this loop.
    let mut warnings = Vec::new();
    let mut frames = 0;
    while !lightning_storm::raging(&sim) {
        sim.sound_events.clear();
        step(&mut sim, &rules);
        frames += 1;
        if sim
            .sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::LightningStormApproaching))
        {
            warnings.push(frames);
        }
        assert!(frames <= 250, "the countdown ends");
    }
    assert_eq!((frames, warnings), (250, vec![25]));
    assert!(sim.sound_events.iter().any(began));
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Ion
    );
    let (_, _, _, duration, start, ..) = sim.lightning_storm.globals_for_test();
    let outage = |house| {
        sim.power_states
            .get(&house)
            .map(|state| state.radar_outage_for_test())
            .map(|timer| (timer.start_frame(), timer.duration()))
    };
    assert_eq!(duration, 180);
    assert_eq!(outage(russians), Some((start, duration)));
    assert!(outage(americans).is_none_or(|(_, duration)| duration == 0));

    let mut strikes = 0;
    while lightning_storm::raging(&sim) {
        sim.sound_events.clear();
        step(&mut sim, &rules);
        frames += 1;
        strikes += sim
            .sound_events
            .iter()
            .filter(|event| {
                matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "WeatherStrike")
            })
            .count();
        assert!(frames < 2000, "the storm ends");
    }
    assert!(strikes > 0);
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .is_none_or(|entity| entity.health.current < strength),
        "the strikes reached the tank"
    );
    let (active, time_to_end, _, _, _, cell, owner) = sim.lightning_storm.globals_for_test();
    assert_eq!(
        (active, time_to_end, cell, owner),
        (false, false, (0, 0), None)
    );
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Normal
    );
}
