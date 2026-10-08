//! The Psychic Dominator (`psychic_dominator`): its native comparisons
//! against `tools/superweapon_oracle.json` (`psydom_process`,
//! `psydom_start`, `update_lighting`, `ambient_step`,
//! `dominator_lighting_read`; `--check` regenerates them), and a strike on
//! retail rules through production frames.

use super::chronosphere_tests::{charge_super, click, retail_rules_binding, step, world_with};
use super::lightning_storm::LightningStorm;
use super::psychic_dominator::{PsychicDominatorState, fires_for_test};
use crate::map::lighting::{ParsedLightingProfiles, parse_lighting_profiles};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::light_sources::LightingEvent;
use crate::sim::mission::MissionType;
use crate::sim::scenario_session::{
    ScenarioLightProfileUnits, ScenarioLightingProfile, ScenarioLightingState,
};
use crate::sim::timer::CdTimer;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::Value;

const DOMINATOR: &str = "PsychicDominatorSpecial";
const TARGET: (u16, u16) = (40, 40);
/// The oracle rows' cell (`G_PSYDOM_COORDS`).
const ORACLE_CELL: (i16, i16) = (33, 44);
/// `PDFXCLD.SHP`'s and `PDFXLOC.SHP`'s frame counts.
const FIRST_ANIM_FRAMES: i32 = 60;
const SECOND_ANIM_FRAMES: i32 = 21;

fn retail_rules() -> Option<RuleSet> {
    retail_rules_binding(&[
        ("PDFXCLD", FIRST_ANIM_FRAMES),
        ("PDFXLOC", SECOND_ANIM_FRAMES),
        ("MINDANIM", 8),
        ("MINDANIMR", 8),
    ])
}

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn pair(value: &Value) -> (i32, i32) {
    (int(&value[0]), int(&value[1]))
}

fn global_lighting_events(sim: &Simulation) -> usize {
    sim.lighting_sources
        .pending
        .iter()
        .filter(|event| matches!(event, LightingEvent::Global { .. }))
        .count()
}

/// The cell profiles of the pending relight-profile changes, in order.
fn relight_profiles(sim: &Simulation) -> Vec<ScenarioLightingProfile> {
    sim.lighting_sources
        .pending
        .iter()
        .filter_map(|event| match event {
            LightingEvent::RelightProfile { cell_profile } => Some(*cell_profile),
            _ => None,
        })
        .collect()
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, at: (u16, u16)) -> u64 {
    sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} stands at {at:?}"))
}

fn owner_name(sim: &Simulation, id: u64) -> &str {
    let owner = sim.substrate.entities.get(id).expect("alive").owner();
    sim.interner.resolve(owner)
}

/// The anim whose owner object is `id` and whose type is `kind`.
fn attached_anim(sim: &Simulation, id: u64, kind: &str) -> bool {
    sim.anims().any(|(_, anim)| {
        anim.owner_entity == Some(id) && sim.interner.resolve(anim.type_id) == kind
    })
}

fn anim_kind(sim: &Simulation) -> Option<&str> {
    let anim = sim.anim(sim.psychic_dominator.anim()?)?;
    Some(sim.interner.resolve(anim.type_id))
}

/// The keys the chain reads, through the production readers on the retail
/// INIs.
#[test]
fn retail_dominator_rules() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let general = &rules.general;
    assert_eq!(
        (
            general.dominator_warhead.as_str(),
            general.dominator_first_anim.as_str(),
            general.dominator_second_anim.as_str(),
        ),
        ("DominatorWH", "PDFXCLD", "PDFXLOC")
    );
    assert_eq!(
        (
            general.dominator_fire_at_percentage,
            general.dominator_capture_range,
            general.dominator_damage,
        ),
        (20, 1, 1000)
    );
    assert_eq!(
        rules.mind_control.perma_controlled_anim.as_deref(),
        Some("MINDANIMR")
    );
    assert_eq!(
        general.psychic_dominator_activate_sound.as_deref(),
        Some("PsychicDominatorActivate")
    );
    let sw = rules.super_weapon(DOMINATOR).unwrap();
    assert_eq!(sw.kind, SuperWeaponKind::PsychicDominator);
    assert!(!sw.pre_click && !sw.post_click);
}

/// Status 2's test against the native table: for every
/// `DominatorFireAtPercentage=` the oracle sampled and every frame count
/// 1..64, the strike fires exactly from the first native stage.
#[test]
fn status_two_fires_from_the_native_stage() {
    let oracle = oracle();
    let table = &oracle["psydom_process"]["fire_stages"];
    let frame_counts: Vec<i32> = table["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(int)
        .collect();
    let rows = table["first_stage"].as_array().unwrap();
    assert_eq!(rows.len(), 106);
    for row in rows {
        let percent = int(&row[0]);
        let firsts = row[1].as_array().unwrap();
        assert_eq!(firsts.len(), frame_counts.len());
        for (&frames, first) in frame_counts.iter().zip(firsts) {
            let first = first.as_i64().map(|stage| i32::try_from(stage).unwrap());
            for stage in 0..=2 * frames {
                assert_eq!(
                    fires_for_test(stage, frames, percent),
                    first.is_some_and(|first| stage >= first),
                    "percent {percent}, frames {frames}, stage {stage}"
                );
            }
        }
    }
}

/// Process's single steps against the native rows: each status's move, the
/// strike (MindControlArea, here the real one), the end of the strike's
/// writes and UpdateLighting, and the fade back's end. Statuses 6 and -1
/// do nothing natively and VERA has no value for them. The followed anim is
/// one of the retail types bound with the row's frame count; a row with no
/// frames uses no anim, which VERA reads as finished (its stage is then its
/// frame count, 0, so the stage-5 row is replayed through the test alone).
#[test]
fn retail_process_steps_match_native() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut rules, mut sim, americans) = world_with(rules, 64, &[]);
    let oracle = oracle();
    let rows = rows(&oracle["psydom_process"], "steps");
    let mut replayed = 0;
    for row in rows {
        let Some(status) = u8::try_from(int(&row["status"])).ok().filter(|&s| s <= 5) else {
            continue;
        };
        let (stage, frames, percent) = (
            int(&row["stage"]),
            int(&row["frames"]),
            int(&row["percent"]),
        );
        let status_after = int(&row["status_after"]);
        if status == 2 {
            assert_eq!(
                fires_for_test(stage, frames, percent),
                status_after == 3,
                "{row}"
            );
            if frames == 0 && stage != 0 {
                continue;
            }
        }
        rules.general.dominator_fire_at_percentage = percent;
        let anim = match frames {
            0 if status == 2 => None,
            SECOND_ANIM_FRAMES => Some("PDFXLOC"),
            _ => Some("PDFXCLD"),
        }
        .map(|kind| {
            let anim = super::spawn_super_anim(&mut sim, &rules, kind, [8576, 11392, 0])
                .expect("bound anim");
            sim.substrate
                .anims
                .get_mut(anim)
                .unwrap()
                .runtime
                .current_frame = stage;
            anim
        });
        sim.psychic_dominator =
            PsychicDominatorState::for_test(status, ORACLE_CELL, Some(americans), anim);
        let (target, current) = pair(&row["ambient"]);
        sim.session.lighting.target_ambient = target;
        sim.session.lighting.current_ambient = current;
        let lighting_before = global_lighting_events(&sim);
        let relights_before = relight_profiles(&sim).len();

        super::psychic_dominator::process(&mut sim, &rules, None);

        let state = sim.psychic_dominator;
        assert_eq!(i32::from(state.status_number()), status_after, "{row}");
        let (x, y) = pair(&row["coords_after"]);
        assert_eq!(state.cell(), (x as i16, y as i16), "{row}");
        let events = row["events"].as_array().unwrap();
        let struck = events.iter().any(|event| event[0] == "mind_control_area");
        let lit = events.iter().any(|event| event[0] == "update_lighting");
        if struck {
            // MindControlArea follows its second anim from here.
            assert_eq!(anim_kind(&sim), Some("PDFXLOC"), "{row}");
        } else {
            assert_eq!(state.anim(), anim.filter(|_| int(&row["anim_after"]) != 0));
        }
        assert_eq!(
            global_lighting_events(&sim) - lighting_before,
            usize::from(lit),
            "{row}"
        );
        if lit {
            assert_eq!(
                sim.session.lighting.selected_profile,
                ScenarioLightingProfile::Normal
            );
        }
        // The fade back's end refreshes no cell; VERA's app learns that later
        // relights leave the Dominator arm.
        let relights = relight_profiles(&sim);
        let ended = usize::from(status == 5 && status_after == 0);
        assert_eq!(relights.len() - relights_before, ended, "{row}");
        if ended == 1 {
            assert_eq!(relights.last(), Some(&ScenarioLightingProfile::Normal));
        }
        replayed += 1;
    }
    assert_eq!(replayed, 17);
}

/// `PsyDom::Start` against the native rows through Launch case 7: the
/// globals, the first anim's coordinates (750 leptons over the cell's
/// GetCoords, a level-2 cell included) and row, the fade timer and
/// UpdateLighting; an unset anim leaves everything as it was.
#[test]
fn retail_start_matches_native() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let oracle = oracle();
    let rows = rows(&oracle, "psydom_start");
    assert_eq!(rows.len(), 4);
    let levels: Vec<((u16, u16), u8)> = rows
        .iter()
        .map(|row| {
            let (x, y) = pair(&row["cell"]);
            (
                (x as u16, y as u16),
                u8::try_from(int(&row["level"])).unwrap(),
            )
        })
        .filter(|&(_, level)| level != 0)
        .collect();
    let (mut rules, mut sim, americans) = world_with(rules, 96, &levels);
    let anims = (
        rules.general.dominator_first_anim.clone(),
        rules.general.dominator_second_anim.clone(),
    );
    for row in rows {
        let (cx, cy) = pair(&row["cell"]);
        let cell = (cx as u16, cy as u16);
        let set = |name: &str, present: &Value| {
            if present.as_bool().unwrap() {
                name.to_string()
            } else {
                String::new()
            }
        };
        rules.general.dominator_first_anim = set(&anims.0, &row["first"]);
        rules.general.dominator_second_anim = set(&anims.1, &row["second"]);
        sim.psychic_dominator = PsychicDominatorState::default();
        sim.session.binary_frame = u32::try_from(int(&row["frame"])).unwrap();
        sim.session.lighting = ScenarioLightingState::default();
        sim.session.lighting.transition_timer = CdTimer::from_raw(77, 55);
        let sw_type = charge_super(&mut sim, americans, DOMINATOR);
        let lighting_before = global_lighting_events(&sim);

        assert!(super::psychic_dominator::launch(
            &mut sim, &rules, americans, sw_type, cell
        ));

        let state = sim.psychic_dominator;
        assert_eq!(
            i32::from(state.status_number()),
            int(&row["status"]),
            "{row}"
        );
        assert_eq!(
            state.owner().is_some(),
            int(&row["owner_set"]) != 0,
            "{row}"
        );
        assert_eq!(state.anim().is_some(), int(&row["anim_set"]) != 0, "{row}");
        let (x, y) = pair(&row["coords"]);
        assert_eq!(state.cell(), (x as i16, y as i16), "{row}");
        let timer = sim.session.lighting.transition_timer;
        assert_eq!(
            (timer.start_frame(), timer.duration()),
            pair(&row["timer"]),
            "{row}"
        );
        let events = row["events"].as_array().unwrap();
        if let Some(anim_event) = events.iter().find(|event| event[0] == "anim") {
            let anim = sim.anim(state.anim().unwrap()).unwrap();
            assert_eq!(anim_event[1], "first");
            assert_eq!(sim.interner.resolve(anim.type_id), "PDFXCLD");
            let coords: Vec<i32> = anim_event[2].as_array().unwrap().iter().map(int).collect();
            let at = anim.world_coord;
            assert_eq!(vec![at.x, at.y, at.z], coords, "{row}");
            // (delay 0, loopCount 1, drawFlags 0x600, zAdjust 0, reverse 0)
            assert_eq!(
                (anim.draw_flags, anim.z_adjust),
                (int(&anim_event[5]) as u32, int(&anim_event[6]))
            );
            assert_eq!(i32::from(anim.runtime.loop_remaining), int(&anim_event[4]));
        }
        let lit = events.iter().any(|event| event[0] == "update_lighting");
        assert_eq!(
            global_lighting_events(&sim) - lighting_before,
            usize::from(lit),
            "{row}"
        );
        assert_eq!(
            sim.session.lighting.selected_profile,
            if lit {
                ScenarioLightingProfile::Dominator
            } else {
                ScenarioLightingProfile::Normal
            }
        );
    }
}

/// UpdateLighting against the native rows: the target and RecalcLighting's
/// tint for every Dominator status, with and without a raging storm and in
/// each nuke flash state, on the oracle's profiles. The chrono screen rows
/// are a residual (VERA has none), and Dominator status 6 has no VERA
/// value.
#[test]
fn update_lighting_matches_native() {
    let oracle = oracle();
    let mut sim = Simulation::with_seed(1);
    let owner = sim.interner.intern("Americans");
    let mut replayed = 0;
    for row in rows(&oracle, "update_lighting") {
        let psydom = u8::try_from(int(&row["psydom"])).unwrap();
        if int(&row["chrono"]) != 0 || psydom > 5 {
            continue;
        }
        sim.session.lighting = ScenarioLightingState::new(
            ScenarioLightProfileUnits {
                ambient_percent: 101,
                ..ScenarioLightProfileUnits::normal_default()
            },
            ScenarioLightProfileUnits {
                ambient_percent: 87,
                red_percent: 30,
                green_percent: 40,
                blue_percent: 75,
                ..ScenarioLightProfileUnits::ion_default()
            },
            ScenarioLightProfileUnits {
                ambient_percent: 150,
                red_percent: 85,
                green_percent: 20,
                blue_percent: 30,
                ..ScenarioLightProfileUnits::dominator_default()
            },
            1,
            1,
        );
        sim.session
            .lighting
            .set_nuke_flash_for_test(int(&row["nuke"]), 0, 30);
        sim.lightning_storm = if row["storm"].as_bool().unwrap() {
            LightningStorm::raging_for_test(owner, (10, 10))
        } else {
            LightningStorm::default()
        };
        sim.psychic_dominator =
            PsychicDominatorState::for_test(psydom, ORACLE_CELL, Some(owner), None);
        let lighting_before = global_lighting_events(&sim);

        sim.update_lighting();

        let lighting = &sim.session.lighting;
        assert_eq!(lighting.target_ambient, int(&row["target"]), "{row}");
        let recalc = &row["events"][0];
        assert_eq!(recalc[0], "recalc");
        let rgb = [int(&recalc[1]), int(&recalc[2]), int(&recalc[3])];
        if int(&recalc[4]) == 0 {
            assert_eq!(rgb, [-1, -1, -1]);
            assert_eq!(lighting.alternate_rgb(), None, "{row}");
        } else {
            assert_eq!(lighting.alternate_rgb(), Some(rgb), "{row}");
        }
        assert_eq!(global_lighting_events(&sim) - lighting_before, 1);
        replayed += 1;
    }
    assert_eq!(replayed, 36);
}

/// The ambient fade (`LogicClass::PerTickUpdate`, `0x0055B33D..0x0055B4D7`)
/// against the native rows: its gates, the interval each state selects
/// (`NukeAmbientChangeRate=` while the nuke flash runs,
/// `DominatorAmbientChangeRate=` while the Dominator is active), the
/// target's clamp and the clamped step. The chrono screen's row is a
/// residual (VERA has none).
#[test]
fn ambient_step_matches_native() {
    let oracle = oracle();
    let mut rules = RuleSet::from_ini(&IniFile::from_str("")).expect("empty rules parse");
    let mut sim = Simulation::with_seed(1);
    let owner = sim.interner.intern("Americans");
    let mut replayed = 0;
    for row in rows(&oracle, "ambient_step") {
        if int(&row["chrono"]) != 0 {
            continue;
        }
        let (nonzero, interval, step) = crate::rules::ruleset::ambient_change_terms(
            row["rate"].as_f64().unwrap(),
            row["step"].as_f64().unwrap(),
        );
        rules.general.ambient_change_rate_nonzero = nonzero;
        rules.general.ambient_change_interval_frames = interval;
        rules.general.ambient_change_step = step;
        sim.session.binary_frame = u32::try_from(int(&row["frame"])).unwrap();
        sim.session.lighting = ScenarioLightingState::new(
            ScenarioLightProfileUnits::normal_default(),
            ScenarioLightProfileUnits::ion_default(),
            ScenarioLightProfileUnits::dominator_default(),
            int(&row["dominator_rate"]),
            int(&row["nuke_rate"]),
        );
        let lighting = &mut sim.session.lighting;
        lighting.set_nuke_flash_for_test(int(&row["nuke"]), 0, 30);
        lighting.target_ambient = int(&row["target"]);
        lighting.current_ambient = int(&row["current"]);
        let (start, duration) = pair(&row["timer"]);
        lighting.transition_timer = CdTimer::from_raw(start, duration);
        let psydom = u8::try_from(int(&row["psydom"])).unwrap();
        sim.psychic_dominator =
            PsychicDominatorState::for_test(psydom, ORACLE_CELL, Some(owner), None);
        let lighting_before = global_lighting_events(&sim);

        sim.tick_scenario_lighting_transition(&rules);

        let lighting = &sim.session.lighting;
        assert_eq!(
            (lighting.target_ambient, lighting.current_ambient),
            (int(&row["target_after"]), int(&row["current_after"])),
            "{row}"
        );
        let timer = lighting.transition_timer;
        assert_eq!(
            (timer.start_frame(), timer.duration()),
            pair(&row["timer_after"]),
            "{row}"
        );
        // UpdateCellLighting and the redraw run with each step.
        let stepped = !row["events"].as_array().unwrap().is_empty();
        assert_eq!(
            global_lighting_events(&sim) - lighting_before,
            usize::from(stepped),
            "{row}"
        );
        replayed += 1;
    }
    assert_eq!(replayed, 23);
}

/// The map's `Dominator*=` keys against the native reads
/// (`ScenarioClass::Read_INI_Basic`, `0x0068AAFD..0x0068AC9A`): a missing
/// key keeps the Set_Defaults value, and each authored token converts as
/// natively.
#[test]
fn dominator_lighting_read_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "dominator_lighting_read");
    assert_eq!(rows.len(), 7);
    for row in rows {
        let key = row["key"].as_str().unwrap();
        let field = |profiles: &ParsedLightingProfiles| match key {
            "DominatorAmbient" => profiles.dominator.ambient_percent,
            "DominatorRed" => profiles.dominator.red_percent,
            "DominatorGreen" => profiles.dominator.green_percent,
            "DominatorBlue" => profiles.dominator.blue_percent,
            "DominatorGround" => profiles.dominator.ground_units,
            "DominatorLevel" => profiles.dominator.level_units,
            "DominatorAmbientChangeRate" => profiles.dominator_change_rate,
            other => panic!("{other}"),
        };
        assert_eq!(
            field(&ParsedLightingProfiles::default()),
            int(&row["stored_default"])
        );
        let missing = parse_lighting_profiles(&IniFile::from_str("[Lighting]\nAmbient=1\n"));
        assert_eq!(field(&missing), int(&row["default_units"]), "{key}");
        for authored in row["authored"].as_array().unwrap() {
            let token = authored[0].as_str().unwrap();
            let ini = IniFile::from_str(&format!("[Lighting]\n{key}={token}\n"));
            let profiles = parse_lighting_profiles(&ini);
            assert_eq!(field(&profiles), int(&authored[1]), "{key}={token}");
            let state = ScenarioLightingState::from_map(&profiles);
            assert_eq!(
                (state.dominator, state.dominator_change_rate),
                (profiles.dominator.into(), profiles.dominator_change_rate)
            );
        }
    }
}

/// The player's Dominator on retail rules, from the click through
/// production frames. The launch raises PDFXCLD 750 leptons over the
/// target and tints the scenario; the strike lands on the first frame whose
/// Process reads the anim at stage 12 (`DominatorFireAtPercentage=20` of 60
/// frames, `status_two_fires_from_the_native_stage`): PDFXLOC on the cell,
/// DominatorWH's damage on the dumpster (concrete, 6%) but not the armour,
/// and the 3x3 block's units (`DominatorCaptureRange=1`) join the player for
/// good but for the Iron Curtained tank and the `ImmuneToPsionics=` Robot
/// Tank, two conscripts sharing a cell included; the dumpster, a building,
/// and the tank three cells off stay. The player's own tank in the block
/// keeps its house (SetOwningHouse refuses it) but is held and ringed all the
/// same (`0x0053B298..0x0053B31C`). A second
/// click while it runs is refused and keeps its charge. The tint fades back
/// at `DominatorAmbientChangeRate=` and the Dominator ends when the ambient
/// is back.
#[test]
fn retail_dominator_strike_captures_the_block() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", TARGET);
    let conscript = spawn(&mut sim, &rules, "E2", "Russians", (41, 40));
    let far = spawn(&mut sim, &rules, "HTNK", "Russians", (43, 40));
    let curtained = spawn(&mut sim, &rules, "HTNK", "Russians", (40, 41));
    let frame = sim.session.binary_frame;
    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(curtained).unwrap(),
        frame,
        10_000,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    let own = spawn(&mut sim, &rules, "MTNK", "Americans", (39, 40));
    let robot = spawn(&mut sim, &rules, "ROBO", "Russians", (41, 41));
    let squad = [
        spawn(&mut sim, &rules, "E2", "Russians", (40, 39)),
        spawn(&mut sim, &rules, "E2", "Russians", (40, 39)),
    ];
    // A 1x1 concrete building in the block.
    let dumpster = spawn(&mut sim, &rules, "CAMISC03", "Russians", (39, 39));
    let dumpster_strength = sim.substrate.entities.get(dumpster).unwrap().health.current;
    let normal_ambient = sim.session.lighting.normal.ambient_percent;
    let dominator_ambient = sim.session.lighting.dominator.ambient_percent;
    let sw_type = charge_super(&mut sim, americans, DOMINATOR);

    click(&mut sim, &rules, americans, DOMINATOR, TARGET);
    assert_eq!(sim.psychic_dominator.status_number(), 1);
    assert!(!sim.super_weapons[&americans][&sw_type].is_ready);
    let first = sim.psychic_dominator.anim().expect("PDFXCLD");
    let anim = sim.anim(first).unwrap();
    assert_eq!(sim.interner.resolve(anim.type_id), "PDFXCLD");
    let ground = super::fire::cell_coords(&sim, TARGET);
    assert_eq!(
        [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
        [ground[0], ground[1], ground[2] + 750]
    );
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Dominator
    );
    assert_eq!(sim.session.lighting.target_ambient, dominator_ambient);
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::SuperWeaponRadarEvent { radar } if (radar.rx, radar.ry) == TARGET
    )));

    // The strike.
    let mut frames = 0;
    loop {
        let stage = sim.anim(first).map(|anim| anim.runtime.current_frame);
        step(&mut sim, &rules);
        frames += 1;
        if sim.psychic_dominator.status_number() == 3 {
            assert!(stage.is_some_and(|stage| stage >= 12), "{stage:?}");
            break;
        }
        assert!(stage.is_some_and(|stage| stage < 12), "{stage:?}");
        assert!(frames < 200, "no strike");
    }
    assert_eq!(anim_kind(&sim), Some("PDFXLOC"));
    for id in [tank, conscript, squad[0], squad[1], own] {
        assert_eq!(owner_name(&sim, id), "Americans");
        let link = &sim.substrate.entities.get(id).unwrap().mind_control;
        assert!(link.is_mind_controlled() && link.controller().is_none());
        assert!(attached_anim(&sim, id, "MINDANIMR"), "{id} ringed");
    }
    for id in [far, curtained, robot] {
        assert_eq!(owner_name(&sim, id), "Russians");
        assert!(
            !sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mind_control
                .is_mind_controlled()
        );
    }
    let dumpster_health = sim.substrate.entities.get(dumpster).unwrap().health.current;
    assert!(dumpster_health < dumpster_strength);
    assert_eq!(owner_name(&sim, dumpster), "Russians");
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().health.current,
        400
    );
    // The player's captives take no Hunt.
    let mission = sim.substrate.entities.get(tank).unwrap().mission;
    assert_ne!(mission.queued().known(), Some(MissionType::Hunt));

    // A save mid-strike restores the Dominator and its captives.
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("load")
        .sim;
    loaded
        .restore_after_snapshot_load()
        .expect("mind-control links resolve");
    assert_eq!(loaded.psychic_dominator, sim.psychic_dominator);
    for id in [tank, conscript] {
        let entity = loaded.substrate.entities.get(id).unwrap();
        assert_eq!(
            entity.mind_control,
            sim.substrate.entities.get(id).unwrap().mind_control
        );
        assert!(
            entity.mind_control.is_mind_controlled() && entity.mind_control.controller().is_none()
        );
    }

    // ClickFire refuses another Dominator while this one runs.
    charge_super(&mut sim, americans, DOMINATOR);
    click(&mut sim, &rules, americans, DOMINATOR, (20, 20));
    assert!(sim.super_weapons[&americans][&sw_type].is_ready);
    assert_eq!(
        sim.psychic_dominator.cell(),
        (TARGET.0 as i16, TARGET.1 as i16)
    );

    // The end of the strike and the fade back.
    let mut faded = false;
    while sim.psychic_dominator.status_number() != 0 {
        step(&mut sim, &rules);
        frames += 1;
        assert!(frames < 400, "the Dominator never ends");
        if sim.psychic_dominator.status_number() == 5 && !faded {
            faded = true;
            assert_eq!(sim.psychic_dominator.cell(), (0, 0));
            assert_eq!(sim.psychic_dominator.anim(), None);
            assert_eq!(
                sim.session.lighting.selected_profile,
                ScenarioLightingProfile::Normal
            );
            assert_eq!(sim.session.lighting.target_ambient, normal_ambient);
        }
    }
    assert!(faded);
    assert_eq!(sim.session.lighting.current_ambient, normal_ambient);
    assert_eq!(relight_profiles(&sim), [ScenarioLightingProfile::Normal]);
    // Nothing lets the captives go.
    assert_eq!(owner_name(&sim, tank), "Americans");
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .mind_control
            .is_mind_controlled()
    );
}

/// A computer house's strike sends its captives hunting
/// (`0x0053B3A6..0x0053B3C4`).
#[test]
fn retail_computer_dominator_sends_its_captives_hunting() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    let russians: InternedId = sim.interner.intern("Russians");
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", TARGET);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (39, 40));
    charge_super(&mut sim, russians, DOMINATOR);
    click(&mut sim, &rules, russians, DOMINATOR, TARGET);
    let mut frames = 0;
    while sim.psychic_dominator.status_number() != 3 {
        step(&mut sim, &rules);
        frames += 1;
        assert!(frames < 200, "no strike");
    }
    for id in [tank, gi] {
        assert_eq!(owner_name(&sim, id), "Russians");
        let mission = sim.substrate.entities.get(id).unwrap().mission;
        assert!(
            [mission.current().known(), mission.queued().known()]
                .contains(&Some(MissionType::Hunt)),
            "{id}: {mission:?}"
        );
    }
}

/// The computer's own Dominator on retail rules, through AI_TryFireSW's arm
/// (`AI_Fire_PsyDom @ 0x0050A150`): the Russians (a computer) with the
/// Americans as their enemy aim at the group, not the lone tank, and take it
/// and send it hunting. Another try while it runs fires nothing
/// (`PsyDom::Active`).
#[test]
fn retail_computer_aims_its_dominator_at_the_group() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let russians: InternedId = sim.interner.intern("Russians");
    sim.houses.get_mut(&russians).unwrap().enemy_house = Some(americans);
    let lone = spawn(&mut sim, &rules, "MTNK", "Americans", (20, 20));
    let group = [
        spawn(&mut sim, &rules, "E1", "Americans", (40, 40)),
        spawn(&mut sim, &rules, "E1", "Americans", (41, 40)),
        spawn(&mut sim, &rules, "MTNK", "Americans", (40, 41)),
    ];
    let sw_type = charge_super(&mut sim, russians, DOMINATOR);

    super::ai_fire::try_fire(&mut sim, &rules, russians, None);

    assert_eq!(sim.psychic_dominator.status_number(), 1);
    assert_eq!(sim.psychic_dominator.owner(), Some(russians));
    let cell = sim.psychic_dominator.cell();
    assert!(
        [(40, 40), (41, 40), (40, 41)].contains(&cell),
        "aimed at {cell:?}"
    );
    assert!(!sim.super_weapons[&russians][&sw_type].is_ready);
    charge_super(&mut sim, russians, DOMINATOR);
    super::ai_fire::try_fire(&mut sim, &rules, russians, None);
    assert!(sim.super_weapons[&russians][&sw_type].is_ready);
    assert_eq!(sim.psychic_dominator.cell(), cell);

    let mut frames = 0;
    while sim.psychic_dominator.status_number() != 3 {
        step(&mut sim, &rules);
        frames += 1;
        assert!(frames < 200, "no strike");
    }
    for id in group {
        assert_eq!(owner_name(&sim, id), "Russians");
        let mission = sim.substrate.entities.get(id).unwrap().mission;
        assert!(
            [mission.current().known(), mission.queued().known()]
                .contains(&Some(MissionType::Hunt)),
            "{id}: {mission:?}"
        );
    }
    assert_eq!(owner_name(&sim, lone), "Americans");
}

/// A captive another house's Yuri holds is let go first (FreeUnit through
/// the controller's manager, `0x0053B287`): it leaves the manager and its
/// MINDANIM ring, then joins the launcher's house for good with the
/// MINDANIMR ring.
#[test]
fn retail_dominator_takes_a_captive_from_its_controller() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let yuri_house = sim.interner.intern("YuriCountry");
    sim.houses.insert(
        yuri_house,
        HouseState::new(yuri_house, 2, None, true, 0, 10),
    );
    sim.session.house_order.push(yuri_house);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", (37, 40));
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", TARGET);
    assert!(sim.capture_unit(yuri, tank, &rules, None));
    assert_eq!(owner_name(&sim, tank), "YuriCountry");
    assert!(attached_anim(&sim, tank, "MINDANIM"));
    charge_super(&mut sim, americans, DOMINATOR);
    click(&mut sim, &rules, americans, DOMINATOR, TARGET);
    let mut frames = 0;
    while sim.psychic_dominator.status_number() != 3 {
        step(&mut sim, &rules);
        frames += 1;
        assert!(frames < 200, "no strike");
    }
    assert_eq!(owner_name(&sim, tank), "Americans");
    let link = &sim.substrate.entities.get(tank).unwrap().mind_control;
    assert!(link.is_mind_controlled() && link.controller().is_none());
    let manager = sim
        .substrate
        .entities
        .get(yuri)
        .unwrap()
        .capture_manager
        .as_ref()
        .unwrap();
    assert_eq!(manager.victims().count(), 0);
    assert!(!attached_anim(&sim, tank, "MINDANIM"));
    assert!(attached_anim(&sim, tank, "MINDANIMR"));
}
