//! The Force Shield (`force_shield`, the countdown in
//! `super::SuperWeaponInstance`): its native comparisons against
//! `tools/superweapon_oracle.json` (`force_shield_launch`, `super_fade`;
//! `--check` regenerates them), and launches on retail rules through
//! production frames.

use super::{launch, walk};
use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::sim::intern::InternedId;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::superweapon::chronosphere_tests::{
    charge_super, click, retail_rules_binding, step, world_with,
};
use crate::sim::superweapon::invulnerability::InvulnKind;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::Value;

const FORCE_SHIELD: &str = "ForceShieldSpecial";

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/superweapon_oracle.json")
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coord(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| int(&value[axis]))
}

/// The row's events named `name`, in order.
fn events<'a>(row: &'a Value, name: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(move |event| event[0] == name)
}

/// House `k` (named `H<k>`) lists house `j` when bit `j` of its ally bits
/// (`+0x5788`) is set; IsAlliedWith answers a house's own bit first.
fn alliances(row: &Value) -> HouseAllianceMap {
    let mut map = HouseAllianceMap::new();
    for (asker, bits) in rows(row, "allies").iter().enumerate() {
        let bits = bits.as_u64().unwrap();
        for other in (0..32).filter(|&other| other != asker && bits & (1 << other) != 0) {
            map.entry(format!("H{asker}"))
                .or_default()
                .insert(format!("H{other}"));
        }
    }
    map
}

/// Case 10's walk: the buildings the native walk shields, last first, from
/// each row's buildings (owner and GetCoords) and its centre, with the
/// owner's view of the launching house (`H0`) from the production alliance
/// test.
#[test]
fn the_walk_matches_native() {
    let oracle = oracle();
    for row in rows(&oracle, "force_shield_launch") {
        let native: Vec<usize> = events(row, "curtain")
            .map(|event| event[1].as_u64().unwrap() as usize)
            .collect();
        if !row["charged"].as_bool().unwrap() {
            // ClickFire admits only a charged Super (`fire::click_fire`).
            assert!(native.is_empty());
            continue;
        }
        let alliances = alliances(row);
        let buildings: Vec<(bool, [i32; 3])> = rows(row, "buildings")
            .iter()
            .map(|building| {
                let owner = format!("H{}", building[0]);
                (
                    is_allied_with(&alliances, &owner, "H0"),
                    coord(&building[1]),
                )
            })
            .collect();
        let centre = coord(&row["cell_coords"]);
        assert_eq!(
            walk(centre, int(&row["radius"]), &buildings),
            native,
            "{row}"
        );
        // Each call: ForceShieldDuration=, the launching house, the Force
        // Shield byte.
        for event in events(row, "curtain") {
            assert_eq!(
                (int(&event[2]), int(&event[3]), int(&event[4])),
                (int(&row["duration"]), 0, 1)
            );
        }
    }
}

/// SuperClass::AI's head called once a frame: the countdown after each call,
/// and the call on which `SpecialSound=` plays at the stored coordinate.
#[test]
fn the_countdown_matches_native() {
    let oracle = oracle();
    let stored = [10368, 10368, 416];
    for row in rows(&oracle, "super_fade") {
        let id = InternedId::from_index(1);
        let mut instance = SuperWeaponInstance::new(id, id, 0);
        instance.arm_fade(int(&row["start"]), stored);
        let mut values = Vec::new();
        let mut plays = Vec::new();
        for call in 1..=int(&row["calls"]) {
            if let Some(at) = instance.step_fade() {
                plays.push((call, at));
            }
            values.push(instance.fade().0);
        }
        let native_values: Vec<i32> = rows(row, "values").iter().map(int).collect();
        let native_plays: Vec<(i32, [i32; 3])> = rows(row, "plays")
            .iter()
            .map(|play| (int(&play[0]), coord(&play[2])))
            .collect();
        assert_eq!(values, native_values, "{row}");
        assert_eq!(plays, native_plays, "{row}");
    }
}

/// A row's launch on retail rules carrying its `[General]` values, a
/// 64-square flat map with the row's level and bridge bit at its cell, and
/// no buildings: the invoke anim's coordinate and row, the countdown and its
/// coordinate, and the blackout. The fixture-only row whose cell answers the
/// zero coordinate is left out.
#[test]
fn the_launch_matches_native() {
    let Some(mut rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    let oracle = oracle();
    let mut compared = 0;
    for row in rows(&oracle, "force_shield_launch") {
        let cell = (int(&row["cell"][0]) as u16, int(&row["cell"][1]) as u16);
        let level = int(&row["level"]);
        let centre = [
            i32::from(cell.0) * 256 + 128,
            i32::from(cell.1) * 256 + 128,
            level * 104,
        ];
        if !row["charged"].as_bool().unwrap() || coord(&row["cell_coords"]) != centre {
            continue;
        }
        rules.general.force_shield_radius = int(&row["radius"]);
        rules.general.force_shield_duration = int(&row["duration"]);
        rules.general.force_shield_blackout_duration = int(&row["blackout"]);
        rules.general.force_shield_fade_sound_time = int(&row["fade"]);
        let (world_rules, mut sim, _) = world_with(rules, 64, &[(cell, level as u8)]);
        rules = world_rules;
        if row["bridge"].as_bool().unwrap()
            && let Some(terrain) = sim.resolved_terrain.as_mut()
        {
            let index = terrain.index(cell.0, cell.1).unwrap();
            terrain.cells_mut_for_test()[index].bridge_facts.raw_flags =
                crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
        let russians = sim.interner.intern("Russians");
        let sw_type = charge_super(&mut sim, russians, FORCE_SHIELD);
        let frame = sim.session.binary_frame();
        assert!(launch(
            &mut sim,
            &rules,
            russians,
            cell.0,
            cell.1,
            sw_type,
            crate::sim::world::FrameEffects::default()
        ));

        let native_anim = events(row, "anim").next().unwrap();
        let invoke = sim.interner.intern("FORCSHLD");
        let anims: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| anim)
            .filter(|anim| anim.type_id() == invoke)
            .collect();
        assert_eq!(anims.len(), 1, "{row}");
        let world = anims[0].world_coord();
        assert_eq!([world.x, world.y, world.z], coord(&native_anim[2]), "{row}");
        // (delay, loopCount, drawFlags, zAdjust, reverse) = (0, 1, 0x600, 0, 0).
        assert_eq!(native_anim[3], serde_json::json!([0, 1, 0x600, 0, 0]));
        assert_eq!(
            (anims[0].draw_flags(), anims[0].z_adjust()),
            (0x600, 0),
            "{row}"
        );

        let instance = &sim.super_weapons[&russians][&sw_type];
        assert_eq!(
            instance.fade(),
            (int(&row["fade_countdown"]), coord(&row["fade_coords"])),
            "{row}"
        );
        let native_blackout = events(row, "blackout").next().unwrap();
        assert_eq!(
            sim.power_states[&russians].blackout_remaining(frame),
            int(&native_blackout[2]),
            "{row}"
        );
        compared += 1;
    }
    assert_eq!(compared, 21);
}

/// Retail rules (`ForceShieldRadius=4`, `ForceShieldDuration=500`,
/// `ForceShieldPlayFadeSoundTime=75`) through production frames: the click's
/// launch shields the Russian buildings within four cells and the American
/// one only while the Americans count the Russians as allies, and
/// `ForceShieldFading` plays at the cell on the 425th frame after the launch
/// frame, once.
#[test]
fn retail_force_shield_shields_allied_buildings_and_fades() {
    let Some(mut rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    for americans_list_russians in [false, true] {
        let (world_rules, mut sim, _) = world_with(rules, 64, &[]);
        rules = world_rules;
        let russians = sim.interner.intern("Russians");
        // The Russians click as a human player: the DoList runs only human or
        // PlayerControl Houses' events (0x0064C4FB..0x0064C50D), and SPECIAL_PLACE
        // fires the sender House's own Super (0x004C78D6).
        sim.houses
            .get_mut(&russians)
            .unwrap()
            .set_is_human_for_test(true);
        if americans_list_russians {
            sim.house_alliances_mut_for_test()
                .entry("AMERICANS".to_string())
                .or_default()
                .insert("RUSSIANS".to_string());
        } else {
            // The Russians' own list does not reach the Americans' buildings.
            sim.house_alliances_mut_for_test()
                .entry("RUSSIANS".to_string())
                .or_default()
                .insert("AMERICANS".to_string());
        }
        let spawn = |sim: &mut Simulation, kind: &str, owner: &str, at: (u16, u16)| {
            sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, 0, &rules)
                .unwrap()
        };
        // The battle lab provides the Super, far from the cell.
        spawn(&mut sim, "NATECH", "Russians", (10, 10));
        // GetCoords is the 2x2 foundation's centre, so from cell (40, 40)'s
        // centre: 652, 905, 905 and 1159 leptons.
        let west = spawn(&mut sim, "GAPOWR", "Russians", (37, 40));
        let north = spawn(&mut sim, "GAPOWR", "Russians", (40, 36));
        let american = spawn(&mut sim, "GAPOWR", "Americans", (40, 43));
        let far = spawn(&mut sim, "GAPOWR", "Russians", (44, 40));
        // Supply settled power before the launch-only charged fixture.
        // Building registration already counts the lab's native drain.
        sim.assess_power_for_test(&rules);
        assert!(!sim.power_states[&russians].is_low_power());
        charge_super(&mut sim, russians, FORCE_SHIELD);
        let launch_frame = sim.session.binary_frame();
        click(&mut sim, &rules, russians, FORCE_SHIELD, (40, 40));

        let shield = |sim: &Simulation, id: u64| {
            sim.substrate
                .entities
                .get(id)
                .and_then(|entity| entity.invulnerability.as_ref())
                .map(|state| (state.kind(), state.timer().duration()))
        };
        let shielded = Some((InvulnKind::ForceShield, 500));
        assert_eq!(shield(&sim, west), shielded);
        assert_eq!(shield(&sim, north), shielded);
        assert_eq!(
            shield(&sim, american),
            shielded.filter(|_| americans_list_russians)
        );
        assert_eq!(shield(&sim, far), None);

        let fading = |sim: &Simulation| {
            sim.sound_events
                .iter()
                .filter(|event| {
                    matches!(event, SimSoundEvent::VocAt { sound_id, audible_to: None, rx: 40, ry: 40, world_z_leptons: 0, .. }
                        if sound_id == "ForceShieldFading")
                })
                .count()
        };
        let mut heard = Vec::new();
        while sim.session.binary_frame() < launch_frame + 430 {
            sim.sound_events.clear_for_test();
            let frame = sim.session.binary_frame();
            step(&mut sim, &rules);
            if fading(&sim) > 0 {
                heard.push((frame - launch_frame, fading(&sim)));
            }
        }
        assert_eq!(heard, [(425, 1)]);
    }
}

/// Over a bridge the walk still measures from the cell's ground centre while
/// the countdown keeps the deck (`force_shield_launch` row 7 shields the
/// ground building 1000 leptons out, not the one 416 leptons above it): a
/// click on bridge cell (40, 40) shields the Russian GAPOWR 974 leptons from
/// its ground centre, 1059 from its deck.
#[test]
fn a_bridge_cell_walks_from_the_ground_centre() {
    let Some(rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    let terrain = sim.resolved_terrain.as_mut().expect("the world has a map");
    let index = terrain.index(40, 40).unwrap();
    terrain.cells_mut_for_test()[index].bridge_facts.raw_flags =
        crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    let russians = sim.interner.intern("Russians");
    // The Russians click as a human player: the DoList runs only human or
    // PlayerControl Houses' events (0x0064C4FB..0x0064C50D), and SPECIAL_PLACE
    // fires the sender House's own Super (0x004C78D6).
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_is_human_for_test(true);
    sim.spawn_object_at_height("NATECH", "Russians", 10, 10, 0, 0, &rules)
        .unwrap();
    // The 2x2 foundation's centre (11264, 10752).
    let plant = sim
        .spawn_object_at_height("GAPOWR", "Russians", 43, 41, 0, 0, &rules)
        .unwrap();
    // This sparse geometry fixture supplies assessed power, then charge.
    sim.assess_power_for_test(&rules);
    assert!(!sim.power_states[&russians].is_low_power());
    let sw_type = charge_super(&mut sim, russians, FORCE_SHIELD);
    click(&mut sim, &rules, russians, FORCE_SHIELD, (40, 40));

    assert_eq!(
        sim.substrate
            .entities
            .get(plant)
            .and_then(|entity| entity.invulnerability.as_ref())
            .map(|state| state.kind()),
        Some(InvulnKind::ForceShield)
    );
    assert_eq!(
        sim.super_weapons[&russians][&sw_type].fade().1,
        [10368, 10368, 416]
    );
}

/// SuperClass::Grant leaves the countdown and its coordinate and releases
/// the held anim (`0x006CB6B4..0x006CB6D2`): a Force Shield whose provider
/// was lost and rebuilt still fades where it was launched.
#[test]
fn a_grant_keeps_the_countdown_and_releases_the_anim() {
    let Some(rules) = retail_rules_binding(&[("FORCSHLD", 20)]) else {
        return;
    };
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    let russians = sim.interner.intern("Russians");
    let sw_type = sim.interner.intern(FORCE_SHIELD);
    let anim = super::super::spawn_super_anim(&mut sim, &rules, "FORCSHLD", [10368, 10368, 5])
        .expect("FORCSHLD constructs");
    let mut instance = SuperWeaponInstance::new(sw_type, russians, 0);
    instance.arm_fade(100, [10368, 10368, 416]);
    instance.placement_anim = Some(anim);
    sim.super_weapons
        .map_mut_for_test()
        .entry(russians)
        .or_default()
        .insert(sw_type, instance);
    sim.spawn_object_at_height("NATECH", "Russians", 20, 20, 0, 0, &rules)
        .unwrap();

    super::super::grant_provided_supers(&mut sim, &rules, russians);

    let instance = &sim.super_weapons[&russians][&sw_type];
    assert!(instance.is_active);
    assert_eq!(instance.fade(), (100, [10368, 10368, 416]));
    assert_eq!(instance.placement_anim(), None);
    assert_eq!(
        sim.substrate
            .anims
            .get(anim)
            .unwrap()
            .runtime()
            .loop_remaining,
        0
    );
}

mod sound_reference_history {
    use std::sync::Arc;

    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::ProcessedRulesLayers;
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    use crate::rules::ruleset::RuleSet;
    use crate::rules::sound_ini::SoundRegistry;
    use crate::sim::command::Command;
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::superweapon::{construct_house_supers, superweapon_views_for_owner};
    use crate::sim::world::{FrameEffects, SimSoundEvent};

    const SHIELD: &str = "LayeredShield";
    const OWNER: &str = "Americans";

    fn process_owner(language: &str) -> (NativeRulesProcessOwner, Arc<SoundRegistry>) {
        let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=RootStart\n1=RootFade\n2=LanguageStart\n3=LanguageFade\n\
             4=MapStart\n5=MapFade\n[SectionOnly]\nSounds=unregistered\n",
        )));
        let owner = NativeRulesProcessOwner::from_cold_start_sources(
            crate::sim::arena_fixture::clear_ground_ini(
                "[Countries]\n0=Americans\n1=Russians\n\
                 [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n0=PROVIDER\n\
                 [PROVIDER]\nStrength=1000\nTurret=no\nSight=0\nSuperWeapon=LayeredShield\n\
                 [SuperWeaponTypes]\n0=LayeredShield\n\
                 [LayeredShield]\nType=ForceShield\nAction=ForceShield\nRechargeTime=0.1\n\
                 IsPowered=no\nStartSound=RootStart\nSpecialSound=RootFade\n\
                 [General]\nForceShieldDuration=5\nForceShieldPlayFadeSoundTime=2\n\
                 ForceShieldBlackoutDuration=0\n",
            ),
            Some(IniFile::from_str(language)),
            IniFile::from_str("[PROVIDER]\nFoundation=1x1\n"),
            Arc::clone(&sounds),
            Default::default(),
        )
        .unwrap();
        (owner, sounds)
    }

    /// Authored short countdown in a supplied static arena. Real Building
    /// Unlimbo/House grant, finite charge and semantic command own the state;
    /// no paid factory placement, whole startup or audio device is modeled.
    fn launch_and_fade(rules: &RuleSet, expected_start: &str, expected_fade: &str) {
        let sw = rules.super_weapon(SHIELD).unwrap();
        assert_eq!(sw.start_sound.as_deref(), Some(expected_start));
        assert_eq!(sw.special_sound.as_deref(), Some(expected_fade));
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 23);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session.set_game_mode(ScenarioGameMode::Skirmish);
        sim.session.game_options_mut_for_test().super_weapons = true;
        for name in [OWNER, "Russians"] {
            let owner = sim.interner.get(name).unwrap();
            let house = sim.houses.get_mut(&owner).unwrap();
            house.set_is_human_for_test(true);
            house.set_player_control_for_test(true);
            construct_house_supers(&mut sim, rules, owner, false);
        }
        let owner = sim.interner.get(OWNER).unwrap();
        sim.session.set_current_house_for_test(Some(owner));
        let provider = sim
            .spawn_object("PROVIDER", OWNER, 10, 10, 0, rules)
            .unwrap();
        sim.spawn_object("PROVIDER", "Russians", 27, 27, 0, rules)
            .unwrap();
        sim.recheck_house_tech_tree(owner, rules);
        let type_id = sim.interner.get(SHIELD).unwrap();
        assert!(sim.super_weapons[&owner][&type_id].is_active());
        assert!(!sim.super_weapons[&owner][&type_id].is_ready());
        assert!(sw.recharge_time_frames > 0 && sw.recharge_time_frames <= 90);
        for _ in 0..=sw.recharge_time_frames {
            sim.advance_tick(&[], Some(rules), None, None, 33);
        }
        assert!(
            superweapon_views_for_owner(&sim, rules, &owner)
                .iter()
                .any(|view| view.type_id == type_id && view.can_fire)
        );
        sim.sound_events.clear_for_test();
        assert!(sim.apply_command_with_overlays(
            OWNER,
            &Command::LaunchSuperWeapon {
                sw_type_id: type_id,
                target_rx: 10,
                target_ry: 10
            },
            rules,
            None,
            FrameEffects::default(),
        ));
        // StartSound is expanded by the app from this actual Launch fact and
        // the bound type. Its cue consumer is tested in sound_dispatch; core
        // owns the real delayed SpecialSound producer6CBCA8..6CBCCF below.
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| {
                    matches!(event,
                        SimSoundEvent::SuperWeaponLaunched { owner: actual_owner, sw_type, world }
                            if *actual_owner == owner && *sw_type == type_id
                                && world.to_cell_sub_z().0 == 10 && world.to_cell_sub_z().1 == 10)
                })
                .count(),
            1
        );
        assert!(!sim.super_weapons[&owner][&type_id].is_ready());
        assert_eq!(
            sim.super_weapons[&owner][&type_id].fade(),
            (3, [2688, 2688, 0])
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(provider)
                .unwrap()
                .invulnerability
                .as_ref()
                .unwrap()
                .kind(),
            crate::sim::superweapon::invulnerability::InvulnKind::ForceShield
        );
        let mut fade_visits = Vec::new();
        for visit in 1..=4 {
            sim.sound_events.clear_for_test();
            sim.advance_tick(&[], Some(rules), None, None, 33);
            for event in sim.sound_events.iter() {
                if let SimSoundEvent::VocAt {
                    sound_id,
                    audible_to: None,
                    rx: 10,
                    ry: 10,
                    sub_x,
                    sub_y,
                    world_z_leptons: 0,
                } = event
                {
                    assert_eq!(
                        (*sub_x, *sub_y),
                        (
                            crate::util::lepton::CELL_CENTER_LEPTON,
                            crate::util::lepton::CELL_CENTER_LEPTON
                        )
                    );
                    assert_eq!(sound_id, expected_fade);
                    fade_visits.push(visit);
                }
            }
        }
        assert_eq!(
            fade_visits,
            [3],
            "one native countdown edge, no duplicate cue"
        );
    }

    #[test]
    fn force_shield_keeps_fixed_catalog_sounds_across_rejected_and_empty_layers() {
        // Original6CEA20 retains the current Voc index when7514D0 returns-1;
        // missing/empty strings also keep it. SOUNDMD is not a Rules layer.
        let (mut owner, sounds) =
            process_owner("[LayeredShield]\nStartSound=UnknownStart\nSpecialSound=UnknownFade\n");
        let mode = IniFile::from_str(
            "[LayeredShield]\nStartSound=\nSpecialSound=\n\
             [SoundList]\n0=ModeOnly\n[ModeOnly]\nSounds=not_loaded\n",
        );
        let map =
            IniFile::from_str("[LayeredShield]\nStartSound=ModeOnly\nSpecialSound=SectionOnly\n");
        let (rules, _, _, _) = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
            .unwrap()
            .into_parts();
        assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));
        assert!(sounds.get("ModeOnly").is_none() && sounds.get("SectionOnly").is_none());
        launch_and_fade(&rules, "RootStart", "RootFade");
    }

    #[test]
    fn force_shield_replacement_sounds_survive_saved_rules_projection_and_drive_fade() {
        let (mut owner, sounds) =
            process_owner("[LayeredShield]\nStartSound=LanguageStart\nSpecialSound=LanguageFade\n");
        let mode = IniFile::from_str(
            "[LayeredShield]\nStartSound= lAnGuAgEsTaRt \nSpecialSound=lAnGuAgEfAdE\n",
        );
        let map = IniFile::from_str("[LayeredShield]\nStartSound=MapStart\nSpecialSound=mApFaDe\n");
        let (rules, _, _, _) = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
            .unwrap()
            .into_parts();
        assert_eq!(
            rules.super_weapon(SHIELD).unwrap().start_sound.as_deref(),
            Some("MapStart")
        );
        assert_eq!(
            rules.super_weapon(SHIELD).unwrap().special_sound.as_deref(),
            Some("MapFade")
        );
        // The saved resource owner carries this existing ProcessedRulesLayers
        // wire state. Reproject through the actual loader's fixed-catalog API;
        // this is same-build rules reconstruction, not a whole-world save test.
        let bytes =
            crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
        let saved: ProcessedRulesLayers = crate::util::save_codec::deserialize(&bytes).unwrap();
        let restored = owner.project_saved_scenario_rules(&saved).unwrap();
        assert_eq!(
            restored.simulation_config_hash(),
            rules.simulation_config_hash()
        );
        assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));
        launch_and_fade(&restored, "MapStart", "MapFade");
    }
}
