//! Sounds and EVA from combat: die and damage sounds, voice feedback, fatal
//! sound draws, and under-attack and unit-lost events.

use super::*;

fn selected_death_sounds_for(
    rules: &RuleSet,
    owner_is_human: bool,
    main_rng: &mut SimRng,
) -> Vec<String> {
    selected_campaign_death_sounds(rules, owner_is_human, owner_is_human, main_rng)
}

/// The Techno death block in the fixture's campaign session, for an owner
/// with the given `IsHuman` (`+0x1EC`) and `PlayerControl` (`+0x1ED`) bytes.
fn selected_campaign_death_sounds(
    rules: &RuleSet,
    is_human: bool,
    player_control: bool,
    main_rng: &mut SimRng,
) -> Vec<String> {
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(2, "E1", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    let owner = test_intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(owner), is_human, 5_000, 10);
    house.set_player_control_for_test(player_control);
    let mut houses = [house].into_iter().collect::<crate::sim::houses::Houses>();
    let mut scenario_rng = SimRng::new(0);
    let mut handled_deaths = Vec::new();
    let handles = crate::sim::type_handle_table::ResolvedRuleHandles::resolve(rules, &mut interner);
    let effects = handle_entity_deaths(
        &mut entities,
        &mut OccupancyGrid::new(),
        rules,
        &mut interner,
        Some(handles),
        &mut houses,
        &HouseAllianceMap::new(),
        main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        &[2],
        &[],
        None,
        None,
        None,
        &mut None,
        false,
        0,
        &mut None,
        &mut None,
    );

    effects
        .death_sounds
        .into_iter()
        .map(|(sound, _)| sound.name_in(rules).to_string())
        .collect()
}

/// `BuildingClass::DestructionEffects`: a dying building whose type has no
/// `DieSound=` of its own falls back to `[AudioVisual] BuildingDieSound`
/// (`0x0044173F` reads the type's `DieSound` vector count at `+0x520`,
/// `0x0044174A CMP ECX,EBX ; JNZ` skips the global when it is non-zero, and
/// `0x00441773`/`0x00441779` play `Rules+0x6E8` at the building's coordinate).
/// The fallback is not owner-gated and draws no RNG.
#[test]
fn a_building_with_no_die_sound_falls_back_to_the_global_building_die_sound() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "\
[General]\nFlightLevel=500\n\n\
[AudioVisual]\nBuildingDieSound=BuildingGenericDie\n\n\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n1=GATECH\n\n\
[E1]\nStrength=1\nArmor=none\nDieSound=DieA\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\n\n\
[GATECH]\nStrength=750\nArmor=wood\nDieSound=OwnCrumble\n",
    ))
    .expect("building die-sound rules parse");

    for (kind, global, expected) in [
        ("GAPOWR", Some("BuildingGenericDie"), true),
        ("GATECH", Some("BuildingGenericDie"), false),
        ("GAPOWR", None, false),
    ] {
        let mut rules = rules.clone();
        rules.general.building_die_sound = global.map(str::to_owned);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 11);
        let building = sim
            .spawn_object(kind, "Americans", 12, 12, 0, &rules)
            .unwrap();
        let location = crate::sim::movement::ground_pose::object_location(
            sim.entities().get(building).unwrap(),
            sim.resolved_terrain.as_ref(),
        );
        sim.take_sound_events_for_test();
        let before = sim.main_rng.state();
        sim.building_destruction_effects(
            &rules,
            building,
            false,
            &mut Vec::new(),
            |world, _| {
                assert_eq!(
                    world.sound_events().len(),
                    usize::from(expected),
                    "the global request precedes the centre mark"
                );
            },
            &mut crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(
            sim.main_rng.state(),
            before,
            "the global cue selects no list item"
        );
        if expected {
            assert!(
                matches!(sim.sound_events(), [crate::sim::world::SimSoundEvent::VocAtCoord {
                sound_id, coordinate, flags: 0,
            }] if sound_id.name_in(&rules) == "BuildingGenericDie" && *coordinate == location)
            );
        } else {
            assert!(
                sim.sound_events().is_empty(),
                "own DieSound or absent global contributes no global cue"
            );
        }
    }
}

/// `BuildingClass::ReceiveDamage @ 0x00442230`: the global
/// `[AudioVisual] BuildingDamageSound` (`Rules+0x714`) is a **damage-state**
/// cue, not a per-hit one.
///
/// `0x00442476 JMP [EAX*4 + 0x00442C18]` indexes the four-entry table
/// `{0x004426AC, 0x004426C8, 0x004424A2, 0x0044247D}` with `result - 2`;
/// entry 0 (result 2, the `Strength >> 1` crossing) falls through into entry 1
/// (result 3, the `Strength * Rules+0x1708` crossing) and both reach
/// `0x004426D2 CMP [type+0x538],-1`, so a type with its own `DamageSound=`
/// takes `JNZ 0x0044270B` and the global never plays. Results 1 (no crossing)
/// and 4 (dead) leave the arm unvisited, and `0x0044242C MOV AL,[ESI+0x90]`
/// (`ObjectClass::IsAlive`) skips the whole dispatch for a corpse.
#[test]
fn a_struck_building_sounds_the_global_damage_cue_only_on_a_state_crossing() {
    let ini = IniFile::from_str(
        "\
[General]\nFlightLevel=500\n\n\
[AudioVisual]\nBuildingDamageSound=BuildingDamaged\nConditionRed=25%\n\n\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n1=GAWEAP\n2=CABAD\n\n\
[E1]\nStrength=100\nArmor=none\n\n\
[MTNK]\nStrength=300\nArmor=none\nSpeed=6\nPrimary=Plain\n\n\
[GAPOWR]\nStrength=100\nArmor=none\n\n\
[GAWEAP]\nStrength=100\nArmor=none\nDamageSound=BuildingMetalDamaged\n\n\
[CABAD]\nStrength=100\nArmor=none\nDamageSound=UnknownDamage\n\n\
[Plain]\nDamage=10\nROF=20\nRange=6\nWarhead=PlainWH\n\n\
[PlainWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("building damage-sound rules parse");
    rules.bind_type_sound_references(
        &ini,
        &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=BuildingDamaged\n1=BuildingMetalDamaged\n",
        )),
    );
    // Authored receiver inputs exercise the integration; these health values
    // and coordinates are not claimed as executed native receiver goldens.
    let coordinate = crate::sim::components::DriveCoord {
        x: 1057,
        y: 1859,
        z: 42,
    };

    // One `commit_damage_events` transaction: entity 1 (MTNK) hits entity 2
    // for `damage`. `victim_type`/`victim_category` shape the receiver.
    let hit = |victim_type: &str,
               victim_category: EntityCategory,
               damage: i32,
               ignore_defenses: bool|
     -> (Vec<SimSoundEvent>, i32) {
        // `GameEntity::test_default` interns through the thread-local test
        // interner, and `test_interner()` snapshots it — so the entities must
        // exist before the snapshot or their type ids resolve to whatever the
        // clone happens to hold at that index.
        let mut entities = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        attacker.lifecycle.set_in_limbo_for_test(false);
        attacker.set_in_playfield(true);
        entities.insert(attacker);
        let mut victim = GameEntity::test_default(2, victim_type, "Soviet", 4, 7);
        victim.set_category_for_test(victim_category);
        victim.health = Health::new(100);
        victim.position.put_location(coordinate);
        victim.lifecycle.set_in_limbo_for_test(false);
        victim.set_in_playfield(true);
        entities.insert(victim);

        let mut interner = test_interner();
        let allies = interner.intern("Americans");
        let soviet = interner.intern("Soviet");
        let warhead_ref = interner.intern("PlainWH");

        let mut houses = computer_houses(&[soviet, allies]);
        let mut occupancy = OccupancyGrid::new();
        let mut main_rng = SimRng::new(11);
        let mut scenario_rng = SimRng::new(13);
        let mut handled_deaths = Vec::new();
        let mut hooks = None;
        let mut collected: Vec<SimSoundEvent> = Vec::new();
        let mut sound_sink: Option<&mut Vec<SimSoundEvent>> = Some(&mut collected);

        let records = [EntityDamageEvent::direct_receiver(
            2,
            damage,
            0,
            1,
            Some(allies),
            warhead_ref,
            ReceiverCallFlags {
                ignore_defenses,
                arg6: false,
            },
        )];
        let _ = commit_damage_events(
            &records,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            100,
            &mut hooks,
            &mut sound_sink,
        );
        let hp = entities.get(2).map_or(0, |victim| victim.health.current());
        (collected, hp)
    };

    let damage_cues = |sounds: &[SimSoundEvent]| -> Vec<crate::sim::components::DriveCoord> {
        sounds
            .iter()
            .filter_map(|sound| match sound {
                SimSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate,
                    flags: 0,
                } if sound_id.name_in(&rules) == "BuildingDamaged" => Some(*coordinate),
                _ => None,
            })
            .collect()
    };
    let type_cues = |sounds: &[SimSoundEvent]| -> Vec<crate::sim::components::DriveCoord> {
        sounds
            .iter()
            .filter_map(|sound| match sound {
                SimSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate,
                    flags: 0,
                } if sound_id.name_in(&rules) == "BuildingMetalDamaged" => Some(*coordinate),
                _ => None,
            })
            .collect()
    };

    // 100 -> 40 crosses `Strength >> 1` = 50: result 2, entry 0 of the table,
    // and the cue retains direct Location+9C, including subcell X/Y and Z.
    let (sounds, hp) = hit("GAPOWR", EntityCategory::Structure, 60, false);
    assert_eq!(hp, 40);
    assert_eq!(damage_cues(&sounds), [coordinate]);

    // A crossing straight into red selects result3 and the same global slot.
    let (sounds, hp) = hit("GAPOWR", EntityCategory::Structure, 80, false);
    assert_eq!(hp, 20);
    assert_eq!(damage_cues(&sounds), [coordinate]);

    // 100 -> 80 crosses nothing: result 1, which the `CMP EAX,3 ; JA` bound at
    // `0x0044246D` rejects, so the damage-sound arm is never entered.
    let (sounds, hp) = hit("GAPOWR", EntityCategory::Structure, 20, false);
    assert_eq!(hp, 80);
    assert!(damage_cues(&sounds).is_empty());

    // Result1 has two distinct retained-coordinate requests for a resolved
    // type sound; it neither substitutes the global nor collapses duplicates.
    let (sounds, hp) = hit("GAWEAP", EntityCategory::Structure, 20, false);
    assert_eq!(hp, 80);
    assert_eq!(type_cues(&sounds), [coordinate, coordinate]);
    assert!(damage_cues(&sounds).is_empty());

    // 100 -> 0 is result 4: entry 2 of the table (`0x004424A2`), which never
    // reaches `0x004426D2`. A dying building gets its crumble cue instead.
    let (sounds, _) = hit("GAPOWR", EntityCategory::Structure, 100, false);
    assert!(damage_cues(&sounds).is_empty());

    // `[GAWEAP] DamageSound=` is the `0x004426D2` gate: the type's own cue
    // wins and the global is skipped.
    let (sounds, hp) = hit("GAWEAP", EntityCategory::Structure, 60, false);
    assert_eq!(hp, 40);
    assert!(damage_cues(&sounds).is_empty());
    assert!(type_cues(&sounds).is_empty());

    // Unknown cold names resolve to -1 and therefore permit the global.
    let (sounds, hp) = hit("CABAD", EntityCategory::Structure, 60, false);
    assert_eq!(hp, 40);
    assert_eq!(damage_cues(&sounds), [coordinate]);

    // Original7019D8 raises defended zero to1 before Object's Verses kernel.
    // The executed retail AP/concrete control reduces that1 back to0; this
    // authored PlainWH100% fixture does not. Use the executed raw-zero entry
    // (ignoreDefenses) here; the joined corpus compares defended retail zero.
    let (sounds, hp) = hit("GAWEAP", EntityCategory::Structure, 0, true);
    assert_eq!(hp, 100);
    assert!(type_cues(&sounds).is_empty());
    assert!(damage_cues(&sounds).is_empty());

    for damage in [-20, 80, 100] {
        let (sounds, _) = hit("GAWEAP", EntityCategory::Structure, damage, false);
        assert!(type_cues(&sounds).is_empty(), "packet={damage}");
        assert!(damage_cues(&sounds).is_empty(), "packet={damage}");
    }

    // The arm is BuildingClass-only — an infantryman crossing the same
    // threshold returns through `TechnoClass::ReceiveDamage`, never
    // `0x00442476`.
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 60, false);
    assert_eq!(hp, 40);
    assert!(damage_cues(&sounds).is_empty());
}

/// `TechnoClass::ReceiveDamage @ 0x00701900` arm `0x00702695` — index 2 of the
/// switch table at `0x00702D24`, i.e. damage result 2 only. Unlike the
/// BuildingClass cue this is a Techno-level arm, so every category reaches it;
/// and unlike it, result 3 does NOT (index 3 is `0x007027F7`, the tail).
#[test]
fn a_techno_speaks_its_voice_feedback_only_on_the_half_strength_crossing() {
    let ini = IniFile::from_str(
        "\
[General]\nFlightLevel=500\n\n\
[AudioVisual]\nBuildingDamageSound=BuildingDamaged\nConditionRed=25%\n\n\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[E1]\nStrength=100\nArmor=none\nVoiceFeedback=GIFear\n\n\
[E2]\nStrength=100\nArmor=none\n\n\
[MTNK]\nStrength=300\nArmor=none\nSpeed=6\nPrimary=Plain\n\n\
[GAPOWR]\nStrength=100\nArmor=none\nVoiceFeedback=StructureFear\n\n\
[Plain]\nDamage=10\nROF=20\nRange=6\nWarhead=PlainWH\n\n\
[PlainWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("voice-feedback rules parse");
    rules.bind_type_sound_references(
        &ini,
        &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=BuildingDamaged\n1=GIFear\n2=StructureFear\n",
        )),
    );

    let hit = |victim_type: &str,
               victim_category: EntityCategory,
               damage: i32|
     -> (Vec<SimSoundEvent>, i32) {
        let mut entities = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        attacker.lifecycle.set_in_limbo_for_test(false);
        attacker.set_in_playfield(true);
        entities.insert(attacker);
        let mut victim = GameEntity::test_default(2, victim_type, "Soviet", 4, 7);
        victim.set_category_for_test(victim_category);
        victim.health = Health::new(100);
        victim.lifecycle.set_in_limbo_for_test(false);
        victim.set_in_playfield(true);
        entities.insert(victim);

        let mut interner = test_interner();
        let allies = interner.intern("Americans");
        let soviet = interner.intern("Soviet");
        let warhead_ref = interner.intern("PlainWH");

        let mut houses = [
            HouseState::new(soviet, 0, None, true, 0, 10),
            HouseState::new(allies, 1, None, false, 0, 10),
        ]
        .into_iter()
        .collect::<crate::sim::houses::Houses>();
        let mut occupancy = OccupancyGrid::new();
        // Original Random65C7E0 seed2 returns 18 for (0,99), so the
        // result-2 arm reaches its one-entry raw list draw. Native feedback
        // block replay is retained in input_oracle/unit_voice_playback.
        let mut main_rng = SimRng::new(2);
        let mut scenario_rng = SimRng::new(13);
        let mut handled_deaths = Vec::new();
        let mut hooks = None;
        let mut collected: Vec<SimSoundEvent> = Vec::new();
        let mut sound_sink: Option<&mut Vec<SimSoundEvent>> = Some(&mut collected);

        let records = [EntityDamageEvent::area(
            2,
            damage,
            0,
            1,
            Some(allies),
            warhead_ref,
        )];
        let scenario_before = scenario_rng.state();
        let main_before = main_rng.state();
        let _ = commit_damage_events(
            &records,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            100,
            &mut hooks,
            &mut sound_sink,
        );
        // Feedback advances the one process Main stream inside the damage
        // receiver even without an audio device; Scenario remains untouched.
        assert_eq!(
            scenario_rng.state(),
            scenario_before,
            "the damage voice must not spend a scenario draw"
        );
        if damage == 60 && victim_type != "E2" {
            assert_ne!(
                main_rng.state(),
                main_before,
                "the admitted list consumes Main"
            );
        } else {
            assert_eq!(
                main_rng.state(),
                main_before,
                "other damage results and empty lists are silent"
            );
        }
        let hp = entities.get(2).map_or(0, |victim| victim.health.current());
        (collected, hp)
    };

    let voices = |sounds: &[SimSoundEvent]| -> Vec<(u16, u16)> {
        sounds
            .iter()
            .filter_map(|sound| match sound {
                SimSoundEvent::VocAt {
                    sound_id, rx, ry, ..
                } if matches!(sound_id.name_in(&rules), "GIFear" | "StructureFear") => {
                    Some((*rx, *ry))
                }
                _ => None,
            })
            .collect()
    };

    // 100 -> 40 crosses `Strength >> 1` = 50 without reaching ConditionRed
    // (25): result 2, index 2 of `0x00702D24`, spoken at the object's own cell
    // (`0x00702702 CALL [EDX+0x48]`).
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 60);
    assert_eq!(hp, 40);
    assert_eq!(voices(&sounds), [(4, 7)]);

    // 100 -> 80 crosses nothing: result 1, index 1 — the per-type
    // `DamageSound=` arm, never the voice.
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 20);
    assert_eq!(hp, 80);
    assert!(voices(&sounds).is_empty());

    // 100 -> 20 crosses BOTH thresholds, and `ObjectClass::ReceiveDamage @
    // 0x005F5390` lets 3 override 2. Index 3 is `0x007027F7`, the shared tail,
    // so a hit that drops a unit straight into the red says nothing.
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 80);
    assert_eq!(hp, 20);
    assert!(voices(&sounds).is_empty());

    // 100 -> 0 is forced to index 4 by `0x00702035 MOV EDI,0x4`.
    let (sounds, _) = hit("E1", EntityCategory::Infantry, 100);
    assert!(voices(&sounds).is_empty());

    // `0x007026A1 MOV EAX,[EDI+0x4E8]` / `0x007026A9 JLE`: an empty list
    // returns before the roll, so a type with no `VoiceFeedback=` is silent.
    let (sounds, hp) = hit("E2", EntityCategory::Infantry, 60);
    assert_eq!(hp, 40);
    assert!(voices(&sounds).is_empty());

    // The arm is TechnoClass-level, so a building speaks too — and the voice
    // is emitted first, because native runs it inside
    // `TechnoClass::ReceiveDamage`, which `BuildingClass::ReceiveDamage` only
    // resumes after at `0x00442425`.
    let (sounds, hp) = hit("GAPOWR", EntityCategory::Structure, 60);
    assert_eq!(hp, 40);
    assert_eq!(voices(&sounds), [(4, 7)]);
    let order: Vec<&str> = sounds
        .iter()
        .filter_map(|sound| match sound {
            SimSoundEvent::VocAt { sound_id, .. }
                if sound_id.name_in(&rules) == "StructureFear" =>
            {
                Some("voice")
            }
            SimSoundEvent::VocAtCoord { sound_id, .. }
                if sound_id.name_in(&rules) == "BuildingDamaged" =>
            {
                Some("building")
            }
            _ => None,
        })
        .collect();
    assert_eq!(order, ["voice", "building"]);
}

// Phobos `ROF.RandomDelay` (`0x006FD0B5`) redraws the full-rearm jitter, so
// this stock golden only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn fatal_sound_selection_uses_human_voice_then_die_sound_main_draws() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=1\nArmor=none\nVoiceDie=VoiceA,VoiceB,VoiceC\nDieSound=DieA,DieB\n\n\
[MTNK]\nStrength=100\nArmor=heavy\nPrimary=Gun\n\n\
[Gun]\nDamage=10\nROF=1\nRange=5\nWarhead=WH\n\n\
[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("death-sound rules parse");

    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet"));
    entities.insert(make_entity_owned(2, "E1", 8, 5, 1, "Americans"));
    const HERE: crate::sim::anim_class::AnimWorldCoord = crate::sim::anim_class::AnimWorldCoord {
        x: 2176,
        y: 1408,
        z: 0,
    };
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut entities, 1, 2);
    let owner = test_intern("Americans");
    let mut houses = [HouseState::new(owner, 0, Some(owner), true, 5_000, 10)]
        .into_iter()
        .collect::<crate::sim::houses::Houses>();
    let mut sounds = Vec::new();
    let mut scenario_rng = SimRng::new(73);
    // The one attacker fires this tick, so the scenario stream advances by its
    // `GetROF` reload jitter (`RandomRanged(0, 2)` @ `0x006FD0B0`) and nothing
    // else — the death-sound choices themselves still draw only on the human
    // and main streams.
    let scenario_before = {
        let mut expected = scenario_rng.clone();
        expected.next_range_u32_inclusive(0, 2);
        expected.state()
    };
    let mut human_rng = SimRng::new(1);
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    align_attackers_to_targets(&mut entities);
    tick_combat_with_fog_and_main_rng(
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        &mut houses,
        &HouseAllianceMap::new(),
        Some(&mut sounds),
        None,
        None,
        None,
        0,
        100,
        0,
        &[1, 2],
        &[],
        None,
        &mut scenario_rng,
        &mut human_rng,
        None,
    );
    assert_eq!(
        sounds
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::EntityDied {
                    die_sound_id,
                    world,
                } => Some((die_sound_id.name_in(&rules), *world)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        // Both play at the victim's Location (`[ESI+0x9C..0xA4]`), cell (8, 5).
        [("VoiceC", HERE), ("DieA", HERE)]
    );
    assert_eq!(
        scenario_rng.state(),
        scenario_before,
        "death-sound choices must not consume Scenario RNG beyond the shot's own draws"
    );
    let mut two_draw_reference = SimRng::new(1);
    two_draw_reference.next_u32();
    two_draw_reference.next_u32();
    assert_eq!(human_rng.state(), two_draw_reference.state());

    let mut ai_rng = SimRng::new(1);
    assert_eq!(
        selected_death_sounds_for(&rules, false, &mut ai_rng),
        ["DieB"]
    );
    let mut one_draw_reference = SimRng::new(1);
    one_draw_reference.next_u32();
    assert_eq!(ai_rng.state(), one_draw_reference.state());
}

/// `0x00702130..0x00702136` gates VoiceDie on `HouseClass::IsHumanPlayer @
/// 0x0050B6F0`, whose campaign arm also admits `PlayerControl` (`+0x1ED`,
/// `0x0050B703`) without `IsHuman`: retail ALL01UMD and ALL03UMD give
/// Alliance, and SOV04DMD Germans, `PlayerControl=yes`.
#[test]
fn campaign_player_controlled_house_hears_voice_die() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=1\nArmor=none\nVoiceDie=OnlyVoice\nDieSound=OnlyDie\n",
    ))
    .expect("death-sound rules parse");
    let mut rng = SimRng::new(1);
    assert_eq!(
        selected_campaign_death_sounds(&rules, false, true, &mut rng),
        ["OnlyVoice", "OnlyDie"]
    );
}

#[test]
fn fatal_sound_empty_lists_skip_draws_but_single_choices_still_draw() {
    let empty_rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=1\nArmor=none\nVoiceDie= , \nDieSound=\n",
    ))
    .expect("empty death-sound rules parse");
    let mut empty_rng = SimRng::new(1);
    let empty_before = empty_rng.state();
    assert!(selected_death_sounds_for(&empty_rules, true, &mut empty_rng).is_empty());
    assert_eq!(empty_rng.state(), empty_before);

    let single_rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=1\nArmor=none\nVoiceDie=OnlyVoice\nDieSound=OnlyDie\n",
    ))
    .expect("single death-sound rules parse");
    let mut single_rng = SimRng::new(1);
    assert_eq!(
        selected_death_sounds_for(&single_rules, true, &mut single_rng),
        ["OnlyVoice", "OnlyDie"]
    );
    let mut two_draw_reference = SimRng::new(1);
    two_draw_reference.next_u32();
    two_draw_reference.next_u32();
    assert_eq!(single_rng.state(), two_draw_reference.state());
}

#[test]
fn under_attack_events_fire_for_sourced_structures_and_harvester_types() {
    use crate::sim::world::SimSoundEvent;
    // The damage-apply producer, per `BuildingClass::ReceiveDamage @
    // 0x00442230` (sourced, non-zero result, `Insignificant=` clear — no
    // attacker-house test) and `UnitClass::ReceiveDamage 0x007384B9`
    // (`Harvester=` type flag, any source).
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=HARV\n1=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=CAGAS\n1=CATREE\n2=YAREFN\n\n\
         [CAGAS]\nStrength=800\nArmor=wood\n\n\
         [CATREE]\nStrength=800\nArmor=wood\nInsignificant=yes\n\n\
         [YAREFN]\nStrength=800\nArmor=wood\nUndeploysInto=HARV\nResourceGatherer=yes\n\n\
         [HARV]\nStrength=1000\nArmor=heavy\nSpeed=4\nHarvester=yes\n\n\
         [MTNK]\nStrength=1000\nArmor=heavy\nSpeed=4\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n",
    ))
    .expect("rules parse");

    let mut main_rng = SimRng::new(1);
    let mut run_attack = |victim: GameEntity| {
        let mut store = EntityStore::new();
        store.insert(victim);
        let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
        attacker.set_owner_for_test(test_intern("Attacker"));
        store.insert(attacker);
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 10);
        let mut sounds = Vec::new();
        let result = tick_combat_with_fog(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            None,
            &BTreeMap::new(),
            Some(&mut sounds),
            None,
            None,
            None,
            0,
            100,
            0,
            &[],
            None,
            &mut main_rng,
        );
        let warnings: Vec<_> = sounds
            .into_iter()
            .filter_map(|event| match event {
                SimSoundEvent::UnderAttack {
                    owner,
                    miner,
                    rx,
                    ry,
                    ..
                } => Some((owner, miner, rx, ry)),
                _ => None,
            })
            .collect();
        (result, warnings)
    };

    // Enemy-owned Structure → one base ping for the VICTIM's owner.
    let mut building = make_entity_owned(10, "CAGAS", 8, 5, 800, "Defender");
    building.set_category_for_test(EntityCategory::Structure);
    let (result, warnings) = run_attack(building);
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "building warning is already submitted inline"
    );
    assert_eq!(warnings, [(test_intern("Defender"), false, 8, 5)]);

    // `Harvester=` vehicle → miner ping (the type flag, `UnitType+0xE0E`,
    // not the Rust miner component).
    let harv = make_entity_owned(10, "HARV", 8, 5, 1000, "Defender");
    let (result, warnings) = run_attack(harv);
    assert!(result.consequences.effects().under_attack_events.is_empty());
    assert_eq!(warnings, [(test_intern("Defender"), true, 8, 5)]);

    // SAME-owner structure damage → still pings: `BuildingClass::
    // ReceiveDamage` never compares the source's house before
    // `NotifyUnderAttack` (force-fire on an own building announces).
    let mut friendly = make_entity_owned(10, "CAGAS", 8, 5, 800, "Attacker");
    friendly.set_category_for_test(EntityCategory::Structure);
    let (_, warnings) = run_attack(friendly);
    assert_eq!(
        warnings,
        [(test_intern("Attacker"), false, 8, 5)],
        "own-fire pings"
    );

    // `Insignificant=yes` building → `BuildingType+0x232` skip.
    let mut prop = make_entity_owned(10, "CATREE", 8, 5, 800, "Defender");
    prop.set_category_for_test(EntityCategory::Structure);
    let (result, warnings) = run_attack(prop);
    assert!(warnings.is_empty());
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "insignificant buildings never ping"
    );

    // A 1x1 building with `UndeploysInto=` (this Foundation-less slave miner)
    // never pings: vt+0x80 (`0x00465D40`) gates NotifyUnderAttack
    // (`0x00442956`). Retail's 2x2 YAREFN takes the ore-miner line
    // (`building_retaliation`'s tests).
    let mut slave = make_entity_owned(10, "YAREFN", 8, 5, 800, "Defender");
    slave.set_category_for_test(EntityCategory::Structure);
    let (result, warnings) = run_attack(slave);
    assert!(warnings.is_empty());
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "1x1 UndeploysInto buildings never ping"
    );

    // Plain vehicle (not `Harvester=`, not a structure) → no ping.
    let plain = make_entity_owned(10, "MTNK", 8, 5, 1000, "Defender");
    let (result, warnings) = run_attack(plain);
    assert!(warnings.is_empty());
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "plain unit hits do not ping"
    );
}

/// AreaDamage489AB6 completes each ReceiveDamage before its next receiver.
/// Unit73851D/738530 must finish its warning before Building44297B reaches
/// HouseNotify4F93E0. Both fresh radar types can admit; stock STANDARD/NORMAL
/// EVA entries share one pending slot: with an unrelated line playing and an
/// empty pending slot, reversing these warnings changes the next cue.
#[test]
fn retail_area_damage_delivers_miner_warning_before_neighboring_base_warning() {
    use crate::sim::world::{FrameEffects, SimSoundEvent, SoundBoundary};
    use std::cell::{Cell, RefCell};

    struct Warnings {
        delivered: Cell<usize>,
        seen: RefCell<Vec<(bool, i32)>>,
        building: u64,
    }
    impl SoundBoundary for Warnings {
        fn consume(&mut self, sim: &Simulation, _: &RuleSet, events: &[SimSoundEvent]) {
            for event in &events[self.delivered.get()..] {
                if let SimSoundEvent::UnderAttack { miner, .. } = event {
                    let hp = sim.entities().get(self.building).unwrap().health.current();
                    self.seen.borrow_mut().push((*miner, hp));
                }
            }
            self.delivered.set(events.len());
        }
        fn delivered(&self) -> usize {
            self.delivered.get()
        }
    }

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let weapon = rules.weapon("BlimpBomb").unwrap();
    let warhead = rules.warhead("BlimpHE").unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7385_1D);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let local = sim.intern("Americans");
    sim.session.set_current_house_for_test(Some(local));
    let miner = sim
        .spawn_object("HARV", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let building = sim
        .spawn_object("NAFLAK", "Americans", 13, 12, 0, &rules)
        .unwrap();
    let source = sim
        .spawn_object("ZEP", "Russians", 24, 24, 0, &rules)
        .unwrap();
    let source_owner = sim.entities().get(source).unwrap().owner();
    let before_building = sim.entities().get(building).unwrap().health.current();
    let position = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(miner).unwrap().position,
    );
    let warhead_id = sim.intern("BlimpHE");
    let mut warnings = Warnings {
        delivered: Cell::new(0),
        seen: RefCell::new(Vec::new()),
        building,
    };
    let mut effects = FrameEffects::for_sound(&mut warnings);
    crate::sim::combat::world_receiver::apply_area_damage(
        &mut sim,
        &rules,
        None,
        crate::sim::projectile::ProjectileCoord::new(position.x, position.y, position.z),
        weapon.damage,
        warhead,
        true,
        (source, Some(source_owner), warhead_id),
        &mut effects,
    );
    effects.flush_sound(&sim, &rules);
    let after_building = sim.entities().get(building).unwrap().health.current();
    assert!(0 < after_building && after_building < before_building);
    assert_eq!(
        *warnings.seen.borrow(),
        [(true, before_building), (false, after_building)],
        "the miner warning must reach the client before the later building receiver"
    );
}

/// `TechnoClass::Death_Announcement @ 0x004D98C0` inputs: a damage kill of a
/// non-building emits one `UnitLostEvent`; a `Spawned=` type (`0x004D98DD`)
/// and a building (no `+0x3B8` caller in `BuildingClass`) emit none.
#[test]
fn unit_lost_events_come_from_damage_kills_of_unspawned_non_buildings() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n0=HORNET\n\n\
         [BuildingTypes]\n0=CAGAS\n\n\
         [CAGAS]\nStrength=10\nArmor=wood\n\n\
         [MTNK]\nStrength=10\nArmor=heavy\nSpeed=4\n\n\
         [HORNET]\nStrength=10\nArmor=light\nSpeed=4\nSpawned=yes\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [M60]\nDamage=250\nROF=20\nRange=5\nWarhead=SA\n\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("rules parse");

    let mut main_rng = SimRng::new(1);
    let mut run_kill = |victim: GameEntity| -> CombatTickResult {
        let mut store = EntityStore::new();
        store.insert(victim);
        let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
        attacker.set_owner_for_test(test_intern("Attacker"));
        store.insert(attacker);
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 10);
        tick(&mut store, &rules, &mut interner, 0, &mut main_rng)
    };

    let tank = make_entity_owned(10, "MTNK", 8, 5, 10, "Defender");
    let result = run_kill(tank);
    assert_eq!(
        result.consequences.effects().unit_lost_events.len(),
        1,
        "vehicle kill announces"
    );
    assert_eq!(
        result.consequences.effects().unit_lost_events[0].owner,
        test_intern("Defender")
    );
    assert_eq!(
        result.consequences.effects().unit_lost_events[0].type_id,
        test_intern("MTNK"),
        "the loss carries the dead techno's own type, not a coordinate guess"
    );
    assert_eq!(
        (
            result.consequences.effects().unit_lost_events[0].rx,
            result.consequences.effects().unit_lost_events[0].ry
        ),
        (8, 5)
    );

    let mut hornet = make_entity_owned(10, "HORNET", 8, 5, 10, "Defender");
    hornet.set_category_for_test(EntityCategory::Aircraft);
    let result = run_kill(hornet);
    assert!(
        result.consequences.effects().unit_lost_events.is_empty(),
        "Spawned= types are silent"
    );

    let mut shack = make_entity_owned(10, "CAGAS", 8, 5, 10, "Defender");
    shack.set_category_for_test(EntityCategory::Structure);
    let result = run_kill(shack);
    assert!(
        result.consequences.effects().unit_lost_events.is_empty(),
        "buildings have no Death_Announcement caller"
    );
}

/// `UnitClass::ReceiveDamage @ 0x00737C90`: `0x00737D69 CMP EAX,4` splits the
/// result. The `Harvester=` ping (`0x007384B9..0x00738530`) lives only in the
/// `result != 4` arm; a killing blow takes the death arm, where only
/// `Death_Announcement` (`+0x3B8`) speaks. So a one-shot miner kill is "Unit
/// lost" alone, never "Ore miner under attack" as well.
#[test]
fn harvester_killing_blow_announces_unit_lost_without_the_miner_ping() {
    use crate::sim::world::SimSoundEvent;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=HARV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [HARV]\nStrength=1000\nArmor=heavy\nSpeed=4\nHarvester=yes\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [M60]\nDamage=250\nROF=20\nRange=5\nWarhead=SA\n\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("rules parse");

    let mut main_rng = SimRng::new(1);
    let mut run_attack = |victim: GameEntity| {
        let mut store = EntityStore::new();
        store.insert(victim);
        let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
        attacker.set_owner_for_test(test_intern("Attacker"));
        store.insert(attacker);
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 10);
        let mut sounds = Vec::new();
        let result = tick_combat_with_fog(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            None,
            &BTreeMap::new(),
            Some(&mut sounds),
            None,
            None,
            None,
            0,
            100,
            0,
            &[],
            None,
            &mut main_rng,
        );
        let warnings = sounds
            .into_iter()
            .filter(|event| matches!(event, SimSoundEvent::UnderAttack { .. }))
            .collect::<Vec<_>>();
        (result, warnings)
    };

    // Non-lethal hit: the `result != 4` arm pings and nothing dies.
    let (result, warnings) = run_attack(make_entity_owned(10, "HARV", 8, 5, 1000, "Defender"));
    assert!(result.consequences.effects().under_attack_events.is_empty());
    assert!(matches!(warnings.as_slice(), [SimSoundEvent::UnderAttack {
        owner, miner: true, rx: 8, ry: 5, ..
    }] if *owner == test_intern("Defender")));
    assert!(
        result.consequences.effects().unit_lost_events.is_empty(),
        "nothing died"
    );

    // One-shot kill: result 4 skips the ping; only the death arm speaks.
    let (result, warnings) = run_attack(make_entity_owned(10, "HARV", 8, 5, 10, "Defender"));
    assert!(
        warnings.is_empty(),
        "a killing blow never submits a miner warning"
    );
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "a killing blow never reaches the miner ping"
    );
    assert_eq!(
        result.consequences.effects().unit_lost_events.len(),
        1,
        "the kill announces once"
    );
    assert_eq!(
        result.consequences.effects().unit_lost_events[0].owner,
        test_intern("Defender")
    );
}
