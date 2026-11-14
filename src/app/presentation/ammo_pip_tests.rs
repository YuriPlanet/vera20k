//! The physical retail Harrier's existing shot/debit/return/rearm loop feeds
//! the shared display query. Native raster controls separately establish the
//! original filled-only PIPS2 frame and drawing/material/placement behavior.
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

fn frames(sim: &Simulation, rules: &RuleSet, id: u64, hover: Option<u64>) -> Vec<usize> {
    let before = sim.state_hash();
    let entity = sim.entities().get(id).expect("Harrier survives");
    let result = super::ui_overlays::scale_pip_draws(
        sim,
        entity,
        hover,
        Some(sim.interner().resolve(entity.owner())),
        rules,
    )
    .into_iter()
    .map(|pip| pip.frame)
    .collect();
    assert_eq!(
        before,
        sim.state_hash(),
        "display cannot spend or reload Ammo"
    );
    result
}

fn native_stage_frames(stage: &str) -> Vec<usize> {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/ammo_pips.json");
    let row = native["raster_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == stage)
        .expect("original Ammo transition control");
    row["draws"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|draw| draw["asset"] == "PIPS2.SHP")
        .map(|draw| draw["frame"].as_u64().unwrap() as usize)
        .collect()
}

#[cfg(feature = "phobos")]
#[test]
fn an_authored_empty_ammo_frame_continues_the_strip_to_capacity() {
    // Phobos `TechnoClass_DrawPips_Ammo` (`Ext/Techno/Hooks.Pips.cpp:287-305`):
    // a negative EmptyAmmoPipFrame stops the strip at the fill count (stock),
    // an authored one draws that frame for every slot through GetPipMax.
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;

    let art_ini = IniFile::from_str("[RIPUART]\n[RIPCART]\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=RIPU\n1=RIPC\n\
             [RIPU]\nImage=RIPUART\nStrength=100\nSpeed=5\nAmmo=3\nInitialAmmo=2\n\
             PipScale=Ammo\nEmptyAmmoPipFrame=7\n\
             [RIPC]\nImage=RIPCART\nStrength=100\nSpeed=5\nAmmo=3\nInitialAmmo=2\n\
             PipScale=Ammo\n",
        ),
        &art_ini,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    assert_eq!(
        rules.object("RIPU").unwrap().empty_ammo_pip_frame(),
        7,
        "the reader keeps the authored frame on the drawn type"
    );

    // Every other pip fixture builds its subject through the ordinary
    // construction path: a hand-built entity never reaches DrawPipScale's
    // admission and supplies no strip at all.
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    let authored = sim
        .spawn_object("RIPU", "Americans", 10, 10, 0, &rules)
        .expect("authored EmptyAmmoPipFrame unit");
    let stock = sim
        .spawn_object("RIPC", "Americans", 12, 10, 0, &rules)
        .expect("stock Ammo unit");
    for id in [authored, stock] {
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .install_ammo_for_test(Some(crate::sim::ammo::TechnoAmmo::new(2)));
    }
    assert!(sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![authored, stock],
            additive: false,
        },
        Some(&rules),
    ));
    assert_eq!(frames(&sim, &rules, authored, None), [13, 13, 7]);
    assert_eq!(frames(&sim, &rules, stock, None), [13, 13]);
}

#[test]
#[ignore = "physical retail map, rules and art; natural ORCA strike, return and rearm"]
fn stock_harrier_ammo_pips_follow_real_strike_return_reload_and_selection() {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let crate::sim::arena_fixture::RetailHarrierDockScene {
        mut runtime,
        player,
        airfield,
        harriers,
        target,
    } = crate::sim::arena_fixture::retail_harrier_dock_scene(&retail).expect("retail scene");
    let full = native_stage_frames("full_a127");
    let spent = native_stage_frames("depleted");
    let rearmed = native_stage_frames("rearmed");
    let owner = runtime.simulation.interner().resolve(player).to_owned();
    runtime
        .simulation
        .fog_mut_for_test()
        .reveal_all_for_owner(player);
    assert!(runtime.simulation.apply_command(
        &owner,
        &Command::Select {
            entity_ids: harriers.to_vec(),
            additive: false,
        },
        Some(&runtime.resources.rules)
    ));
    for id in harriers {
        assert_eq!(
            frames(&runtime.simulation, &runtime.resources.rules, id, None),
            full
        );
    }
    let orders: Vec<_> = harriers
        .into_iter()
        .map(|attacker_id| {
            CommandEnvelope::new(
                player,
                runtime.simulation.session().tick() + 1,
                Command::Attack {
                    attacker_id,
                    target_id: target,
                },
            )
        })
        .collect();
    let mut saw_spent = [false; 2];
    let mut saw_enter = [false; 2];
    let mut saw_landed = [false; 2];
    let mut saw_rearm = [false; 2];
    let mut guarding = [false; 2];
    for frame in 0..9000 {
        runtime
            .advance_frame_for_tooling(
                if frame == 0 { &orders } else { &[] },
                crate::headless_scenario::SIM_TICK_MS,
            )
            .expect("ordinary frame");
        let sim = &runtime.simulation;
        for (index, id) in harriers.into_iter().enumerate() {
            let entity = sim.entities().get(id).expect("Harrier survives");
            let ammo = entity.ammo().expect("authoritative Foot Ammo").current();
            if ammo == 0 && !saw_spent[index] {
                saw_spent[index] = true;
                assert_eq!(frames(sim, &runtime.resources.rules, id, None), spent);
            }
            let mission = entity.mission.current().known();
            saw_enter[index] |= saw_spent[index] && mission == Some(MissionType::Enter);
            let linked = entity.radio_contacts.contains(airfield)
                && sim
                    .entities()
                    .get(airfield)
                    .unwrap()
                    .radio_contacts
                    .contains(id);
            let height = crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain(),
            );
            saw_landed[index] |= saw_spent[index] && linked && height == 0;
            if saw_landed[index] && ammo == 1 && !saw_rearm[index] {
                saw_rearm[index] = true;
                assert_eq!(frames(sim, &runtime.resources.rules, id, None), rearmed);
            }
            guarding[index] |=
                saw_rearm[index] && linked && height == 0 && mission == Some(MissionType::Guard);
        }
        if guarding.into_iter().all(|value| value) {
            break;
        }
    }
    assert!(
        saw_spent.into_iter().all(|value| value),
        "both Harriers spend their round"
    );
    assert!(
        saw_enter.into_iter().all(|value| value),
        "both Harriers return"
    );
    assert!(
        saw_landed.into_iter().all(|value| value),
        "both Harriers land on linked pads"
    );
    assert!(
        saw_rearm.into_iter().all(|value| value),
        "both Harriers rearm"
    );
    assert!(
        guarding.into_iter().all(|value| value),
        "both Harriers return to Guard"
    );
    assert!(runtime.simulation.apply_command(
        &owner,
        &Command::Select {
            entity_ids: vec![],
            additive: false,
        },
        Some(&runtime.resources.rules)
    ));
    for id in harriers {
        assert!(frames(&runtime.simulation, &runtime.resources.rules, id, None).is_empty());
        assert_eq!(
            frames(&runtime.simulation, &runtime.resources.rules, id, Some(id)),
            rearmed
        );
    }
}

/// The production ammo-pip strip for one authored type section. `extra` is
/// appended to the `[RIPU]` section, so each key can be varied without a
/// second fixture. Mirrors the ordinary construction path of the existing
/// `EmptyAmmoPipFrame` test (a hand-built entity never reaches DrawPipScale).
fn ammo_pip_scene(max_ammo: i32, initial_ammo: i32, extra: &str) -> (RuleSet, Simulation, u64) {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::ammo::TechnoAmmo;

    let art_ini = IniFile::from_str("[RIPUART]\n");
    let text = format!(
        "[VehicleTypes]\n0=RIPU\n\
         [RIPU]\nImage=RIPUART\nStrength=100\nSpeed=5\nAmmo={max_ammo}\n\
         InitialAmmo={initial_ammo}\nPipScale=Ammo\n{extra}"
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&text),
        &art_ini,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));

    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    let id = sim
        .spawn_object("RIPU", "Americans", 10, 10, 0, &rules)
        .expect("RIPU spawns");
    sim.entities_mut_for_test()
        .get_mut(id)
        .unwrap()
        .install_ammo_for_test(Some(TechnoAmmo::new(initial_ammo)));
    assert!(sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![id],
            additive: false,
        },
        Some(&rules),
    ));
    (rules, sim, id)
}

/// The full production draws, so an offset/size assertion sees the placement
/// as well as the frame.
fn draws(sim: &Simulation, rules: &RuleSet, id: u64) -> Vec<super::ui_overlays::ScalePipDraw> {
    let before = sim.state_hash();
    let entity = sim.entities().get(id).expect("RIPU survives");
    let result = super::ui_overlays::scale_pip_draws(
        sim,
        entity,
        None,
        Some(sim.interner().resolve(entity.owner())),
        rules,
    );
    assert_eq!(before, sim.state_hash(), "display cannot spend Ammo");
    result
}

/// Phobos `AmmoPipFrame` (`Ext/TechnoType/Body.cpp:1002`, constructor 13 at
/// `Body.h:666`, consumer `Ext/Techno/Hooks.Pips.cpp:286`): the unwrapped
/// strip's filled frame is the authored value, not the hardcoded stock 13.
#[cfg(feature = "phobos")]
#[test]
fn an_authored_ammo_pip_frame_replaces_the_stock_thirteen() {
    let (rules, sim, id) = ammo_pip_scene(3, 2, "AmmoPipFrame=5\n");
    assert_eq!(rules.object("RIPU").unwrap().ammo_pip_frame(), 5);
    let authored = draws(&sim, &rules, id);
    assert_eq!(authored.iter().map(|d| d.frame).collect::<Vec<_>>(), [5, 5]);

    let (stock_rules, stock_sim, stock_id) = ammo_pip_scene(3, 2, "");
    assert_eq!(stock_rules.object("RIPU").unwrap().ammo_pip_frame(), 13);
    assert_eq!(frames(&stock_sim, &stock_rules, stock_id, None), [13, 13]);
}

/// Phobos `AmmoPipWrapStartFrame` (`Ext/TechnoType/Body.cpp:1004`,
/// constructor 14 at `Body.h:668`, consumer
/// `Ext/Techno/Hooks.Pips.cpp:249`): the wrapped strip's start frame. This
/// pins the production reader's authored value and constructor default.
#[cfg(feature = "phobos")]
#[test]
fn the_ammo_pip_wrap_start_frame_reads_its_authoring_and_constructor() {
    let (rules, _, _) = ammo_pip_scene(3, 2, "AmmoPipWrapStartFrame=21\n");
    assert_eq!(
        rules.object("RIPU").unwrap().ammo_pip_wrap_start_frame(),
        21
    );
    let (stock, _, _) = ammo_pip_scene(3, 2, "");
    assert_eq!(
        stock.object("RIPU").unwrap().ammo_pip_wrap_start_frame(),
        14
    );
}

/// Phobos `AmmoPipSize` (`Ext/TechnoType/Body.cpp:1005`,
/// `Nullable<Point2D>` seeded empty at `Body.h:669`, consumer
/// `Ext/Techno/Hooks.Pips.cpp:172`): a full pair replaces the strip's X/Y
/// step, while the unset state keeps the stock 4 / 0 layout.
#[cfg(feature = "phobos")]
#[test]
fn the_ammo_pip_size_changes_the_strip_step_only_when_authored() {
    let (rules, sim, id) = ammo_pip_scene(3, 2, "AmmoPipSize=9,3\n");
    assert_eq!(rules.object("RIPU").unwrap().ammo_pip_size(), Some([9, 3]));
    let authored = draws(&sim, &rules, id);
    assert_eq!(
        authored.iter().map(|d| d.offset_pixels).collect::<Vec<_>>(),
        [[0, 7], [9, 10]]
    );

    let (stock_rules, stock_sim, stock_id) = ammo_pip_scene(3, 2, "");
    assert_eq!(stock_rules.object("RIPU").unwrap().ammo_pip_size(), None);
    let stock = draws(&stock_sim, &stock_rules, stock_id);
    assert_eq!(
        stock.iter().map(|d| d.offset_pixels).collect::<Vec<_>>(),
        [[0, 7], [4, 7]]
    );
}

/// Phobos `AmmoPipOffset` (`Ext/TechnoType/Body.cpp:1006`,
/// constructor `{0,0}` at `Body.h:670`, consumer
/// `Ext/Techno/Hooks.Pips.cpp:241`): the pair translates every pip by the
/// same amount, after the optional `AmmoPipSize` step.
#[cfg(feature = "phobos")]
#[test]
fn an_authored_ammo_pip_offset_translates_every_pip_in_the_strip() {
    let (rules, sim, id) = ammo_pip_scene(3, 2, "AmmoPipOffset=6,-2\n");
    assert_eq!(rules.object("RIPU").unwrap().ammo_pip_offset(), [6, -2]);
    let authored = draws(&sim, &rules, id);
    assert_eq!(
        authored.iter().map(|d| d.offset_pixels).collect::<Vec<_>>(),
        [[6, 5], [10, 5]]
    );

    let (both_rules, both_sim, both_id) =
        ammo_pip_scene(3, 2, "AmmoPipSize=9,3\nAmmoPipOffset=6,-2\n");
    let both = draws(&both_sim, &both_rules, both_id);
    assert_eq!(
        both.iter().map(|d| d.offset_pixels).collect::<Vec<_>>(),
        [[6, 5], [15, 8]]
    );

    let (stock_rules, stock_sim, stock_id) = ammo_pip_scene(3, 2, "");
    assert_eq!(
        stock_rules.object("RIPU").unwrap().ammo_pip_offset(),
        [0, 0]
    );
    let stock = draws(&stock_sim, &stock_rules, stock_id);
    assert_eq!(
        stock.iter().map(|d| d.offset_pixels).collect::<Vec<_>>(),
        [[0, 7], [4, 7]]
    );
}

#[cfg(feature = "phobos")]
fn set_ammo(sim: &mut Simulation, id: u64, ammo: i32) {
    use crate::sim::ammo::TechnoAmmo;
    sim.entities_mut_for_test()
        .get_mut(id)
        .unwrap()
        .install_ammo_for_test(Some(TechnoAmmo::new(ammo)));
}

/// Phobos `TechnoClass_DrawPips_Ammo`'s wrap arm (`0x70A421..4E6`,
/// `Ext/Techno/Hooks.Pips.cpp:245-279`): `PipWrap` slots count the wrap
/// levels above `AmmoPipWrapStartFrame`; a depleted slot keeps the bare start
/// frame. The wrapped loop advances the origin before each draw, so the
/// strip starts one step in, and `PipWrap=-1` takes Phobos's unwrapped arm.
#[cfg(feature = "phobos")]
#[test]
fn the_pip_wrap_arm_counts_the_wrap_levels() {
    let (rules, mut sim, id) = ammo_pip_scene(
        10,
        10,
        "PipWrap=5
",
    );
    let ty = rules.object("RIPU").unwrap();
    assert_eq!((ty.pip_wrap(), ty.pip_max(&rules)), (5, 10));
    // levels = 10 / 5 - 1 = 1, start frame 14.
    assert_eq!(frames(&sim, &rules, id, None), [16, 16, 16, 16, 16]);
    let advances: Vec<i32> = draws(&sim, &rules, id)
        .iter()
        .map(|draw| draw.offset_pixels[0])
        .collect();
    assert_eq!(advances, [4, 8, 12, 16, 20], "the origin advances first");

    set_ammo(&mut sim, id, 6);
    assert_eq!(frames(&sim, &rules, id, None), [16, 15, 15, 15, 15]);

    set_ammo(&mut sim, id, 1);
    assert_eq!(frames(&sim, &rules, id, None), [15, 14, 14, 14, 14]);

    set_ammo(&mut sim, id, 0);
    assert_eq!(frames(&sim, &rules, id, None), [14, 14, 14, 14, 14]);

    // The authored start frame moves every calculated level with it.
    let (rules, sim, id) = ammo_pip_scene(
        10,
        10,
        "PipWrap=5
AmmoPipWrapStartFrame=20
",
    );
    assert_eq!(frames(&sim, &rules, id, None), [22, 22, 22, 22, 22]);

    // `AmmoPipSize`/`AmmoPipOffset` place the wrapped strip like the
    // unwrapped one, after the same origin advance.
    let (rules, sim, id) = ammo_pip_scene(
        10,
        10,
        "PipWrap=5
AmmoPipSize=9,3
AmmoPipOffset=6,-2
",
    );
    let placed: Vec<[i32; 2]> = draws(&sim, &rules, id)
        .iter()
        .map(|draw| draw.offset_pixels)
        .collect();
    assert_eq!(
        placed,
        [[15, 8], [24, 11], [33, 14], [42, 17], [51, 20]],
        "slot+1 advance, then the authored size and offset"
    );

    // A negative PipWrap takes Phobos's unwrapped strip (fill frame 13).
    let (rules, sim, id) = ammo_pip_scene(
        10,
        10,
        "PipWrap=-1
",
    );
    assert_eq!(frames(&sim, &rules, id, None), [13; 10]);
}

/// Vanilla enters the wrap arm on any nonzero `PipWrap` and draws nothing
/// when it is negative (`0x70A374 jne`, `0x70A42C jle`); positive wraps use
/// the same level counter with the stock start frame 14.
#[cfg(not(feature = "phobos"))]
#[test]
fn a_negative_pip_wrap_draws_no_stock_strip() {
    let (rules, sim, id) = ammo_pip_scene(
        10,
        10,
        "PipWrap=-1
",
    );
    assert!(frames(&sim, &rules, id, None).is_empty());

    let (rules, sim, id) = ammo_pip_scene(
        10,
        10,
        "PipWrap=5
",
    );
    assert_eq!(frames(&sim, &rules, id, None), [16, 16, 16, 16, 16]);
    let advances: Vec<i32> = draws(&sim, &rules, id)
        .iter()
        .map(|draw| draw.offset_pixels[0])
        .collect();
    assert_eq!(advances, [4, 8, 12, 16, 20]);
}
