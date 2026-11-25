use super::*;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;

#[test]
fn ordinary_attack_click_honors_techno_legal_target_but_ctrl_bypasses_it() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let (rules, _, _, _) = crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::Campaign(None),
            &crate::rules::ini_parser::IniFile::from_str("[HTNK]\nLegalTarget=no\n"),
        )
        .unwrap()
        .into_parts();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let tank = sim
        .spawn_object("MTNK", "Americans", 10, 10, 64, &rules)
        .unwrap();
    let target = sim
        .spawn_object("HTNK", "Russians", 12, 10, 64, &rules)
        .unwrap();
    let ordinary = sim
        .spawn_object("MTNK", "Russians", 12, 12, 64, &rules)
        .unwrap();
    let action = |target, forced| {
        attack_action(
            &sim,
            &rules,
            tank,
            &TargetKind::Entity(target),
            forced,
            None,
        )
    };
    assert_ne!(action(target, false), AttackAction::Attack);
    assert_eq!(action(target, true), AttackAction::Attack);
    assert_eq!(action(ordinary, false), AttackAction::Attack);
}

/// Without resolved terrain the attack cursor asks the shared mapless
/// range (`sim::combat::mapless_in_range`), as the fire gate and the
/// threat scan do: `Range=-2` reaches across the map, and MinimumRange
/// refuses a target inside it.
#[test]
fn a_mapless_attack_cursor_takes_the_shared_range_answer() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=ASW\n1=ART\n2=MTNK\n\
             [AircraftTypes]\n[BuildingTypes]\n\
             [ASW]\nStrength=300\nPrimary=Depth\n[ART]\nStrength=300\nPrimary=Shell\n\
             [MTNK]\nStrength=300\n\
             [Depth]\nDamage=10\nROF=30\nRange=-2\nWarhead=WH\n\
             [Shell]\nDamage=10\nROF=30\nRange=8\nMinimumRange=3\nWarhead=WH\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    let mut spawn = |kind: &str, owner: &str, rx: u16, ry: u16| {
        sim.spawn_object(kind, owner, rx, ry, 0, &rules).unwrap()
    };
    let asw = spawn("ASW", "Americans", 5, 5);
    let art = spawn("ART", "Americans", 5, 7);
    let near = spawn("MTNK", "Russians", 6, 7);
    let mid = spawn("MTNK", "Russians", 10, 7);
    let far = spawn("MTNK", "Russians", 40, 5);
    assert!(sim.resolved_terrain().is_none());
    let in_range = |actor, target| {
        resolved_unit_in_range(
            &sim,
            actor,
            &TargetKind::Entity(target),
            Some(&rules),
            None,
            None,
            None,
        )
    };
    assert!(in_range(asw, far), "35 cells away");
    assert!(!in_range(art, near), "inside MinimumRange");
    assert!(in_range(art, mid), "5 cells away");
    assert!(!in_range(art, far), "beyond Range");
}

/// A Cell click asks each receiver's own What_Action ([`cell_harvest_order`]).
/// Over revealed ore the player's War Miner sends MEGAMISSION Harvest with
/// the Cell as Destination. Another player's War Miner, though selected,
/// answers None and sends nothing: the event itself would run it, testing
/// no owner. A Grizzly has no harvest rung. Where a tank holds the ore
/// Cell, the player's War Miner answers NoMove, and its ClickedAction
/// resolves a Cell to move to instead (`0x004D8065`).
#[test]
fn retail_ore_click_harvests_where_the_receivers_cell_action_does() {
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{CellActionQuery, MoveAction};

    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &battle.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    for name in ["Americans", "Russians"] {
        let owner = sim.intern(name);
        let side = rules
            .country_side_index(name)
            .expect("stock country Side")
            .0;
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, side, Some(owner), true, 0, 10));
    }
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    let (open, taken) = ((12, 14), (16, 14));
    let ore = registry.id_for_name("TIB01").unwrap();
    for cell in [open, taken] {
        sim.place_overlay_for_test(cell, ore, 6, &registry);
    }
    let own = sim
        .spawn_object("HARV", "Americans", 10, 10, 0, rules)
        .expect("the player's War Miner");
    let other = sim
        .spawn_object("HARV", "Russians", 14, 10, 0, rules)
        .expect("another player's War Miner");
    let tank = sim
        .spawn_object_with_overlay_registry(
            "MTNK",
            "Americans",
            taken.0,
            taken.1,
            0,
            rules,
            &registry,
        )
        .expect("the player's Grizzly on the ore");
    sim.resolve_type_handles(rules);

    let control = sim.control_projection();
    let harvest = |id, cell| cell_harvest_order(&sim, &control, rules, owner, id, cell, false);
    assert_eq!(
        harvest(own, open),
        Some(Command::clicked_mission(
            own,
            crate::sim::mission::MissionType::Harvest,
            TargetKind::Cell(open.0, open.1),
        ))
    );
    assert_eq!(harvest(other, open), None);
    assert_eq!(harvest(tank, open), None);

    let click = CellActionQuery {
        mouse_over: false,
        shift: false,
    };
    assert_eq!(
        sim.cell_move_action(owner, own, taken, click, rules),
        Ok(MoveAction::NoMove)
    );
    assert_eq!(harvest(own, taken), None);
    assert!(
        crate::app::input::commands::ordinary_cell_move_goal(
            &sim, rules, owner, own, taken, false,
        )
        .is_some()
    );
}

/// The native selection dispatcher clamps its shared Cell before asking
/// receivers' What_Action and ClickedAction (`0x004AE77E`), then each Foot
/// resolves from that same Cell (`0x004AE8BA`, `0x004AE921`, `0x004AE976`).
/// At these retail edge clicks the raw Cell answers NoMove while the
/// clamped one answers the receiver's real action; an interior Cell is
/// unchanged. The click path's call site itself is not driven here.
#[test]
fn mobile_cell_dispatch_clamps_before_action_and_goal_resolution() {
    use crate::sim::world::{CellActionQuery, MoveAction};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let owner = sim.interner().get("Local").unwrap();
    let tank = sim
        .spawn_object("MTNK", "Local", 12, 4, 0, &rules)
        .expect("retail tank spawns");
    sim.resolve_type_handles(&rules);
    let action = |cell| {
        sim.cell_move_action(
            owner,
            tank,
            cell,
            CellActionQuery {
                mouse_over: false,
                shift: false,
            },
            &rules,
        )
        .unwrap()
    };

    for (raw, expected_action) in [((7, 0), MoveAction::NoMove), ((16, 0), MoveAction::Move)] {
        let clamped = sim.clamped_click_cell(raw);
        assert_ne!(clamped, raw, "fixed edge input {raw:?} must clamp");
        assert_eq!(action(raw), MoveAction::NoMove, "raw edge {raw:?}");
        assert_eq!(action(clamped), expected_action, "clamped edge {raw:?}");
    }
    assert_eq!(sim.clamped_click_cell((9, 5)), (9, 5));
}

/// An all-berserk selection's left release follows the best object's
/// class rung: Infantry NoMove (`0x0051F05E`, cell `0x0051FAAB`), Unit
/// NoMove over an object (`0x007403EB`) but 0 on the ground
/// (`0x007404B0`), JumpJet infantry 0 on the ground, and Aircraft 0
/// (`0x00417D7C`). The best is the nearest berserk object.
#[test]
fn an_all_berserk_click_follows_the_best_objects_class() {
    use LeftRelease::{Dispatch, Nothing, Select};
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=GI\n1=JUMPJET\n[VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n0=ORCA\n[GI]\nStrength=125\n\
             [JUMPJET]\nStrength=125\nJumpJet=yes\n[MTNK]\nStrength=300\n\
             [ORCA]\nStrength=200\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let mut spawn = |kind: &str, rx: u16, berserk: bool| {
        let id = sim
            .spawn_object(kind, "Americans", rx, 5, 0, &rules)
            .unwrap();
        sim.entity_mut_for_test(id)
            .unwrap()
            .berserk
            .set_active_for_test(berserk);
        id
    };
    let gi = spawn("GI", 3, true);
    let jumpjet = spawn("JUMPJET", 4, true);
    let tank = spawn("MTNK", 6, true);
    let orca = spawn("ORCA", 8, true);
    let friend = spawn("GI", 10, false);
    let over_friend = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: friend,
    };
    let click = |selection: &[u64], hover: Option<&HoverTargetKindWithId>, cell| {
        berserk_selection_click(&sim, &rules, selection, hover, cell, owner)
    };
    assert_eq!(click(&[gi], None, (3, 5)), Dispatch);
    assert_eq!(click(&[gi], Some(&over_friend), (3, 5)), Dispatch);
    assert_eq!(click(&[jumpjet], None, (3, 5)), Nothing);
    assert_eq!(click(&[jumpjet], Some(&over_friend), (3, 5)), Dispatch);
    assert_eq!(click(&[tank], None, (3, 5)), Nothing);
    assert_eq!(click(&[tank], Some(&over_friend), (3, 5)), Dispatch);
    assert_eq!(click(&[orca], None, (3, 5)), Nothing);
    assert_eq!(click(&[orca], Some(&over_friend), (3, 5)), Select);
    // Equal rank 0: the object nearer the clicked cell decides.
    assert_eq!(click(&[tank, gi], None, (3, 5)), Dispatch);
    assert_eq!(click(&[gi, tank], None, (6, 5)), Nothing);
    // 0 over an object already selected does nothing (`0x004ABE0B`).
    sim.entity_mut_for_test(friend)
        .unwrap()
        .seldom
        .selection
        .set_selected_for_test(true);
    assert_eq!(
        berserk_selection_click(&sim, &rules, &[orca], Some(&over_friend), (3, 5), owner),
        Nothing
    );
}

/// The Ivan's self-bomb gate counts the whole selection (`[0xA8ECC8]`):
/// a calm Ivan clicked with a berserk escort still bombs himself, though
/// he is the only object that can send the order.
#[test]
fn the_ivan_self_bomb_gate_counts_the_whole_selection() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=IVAN\n[IVAN]\nStrength=100\nIvan=yes\n\
             AttackCursorOnFriendlies=yes\nBombable=yes\nPrimary=IvanBomber\n\
             [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
             [Invisible]\nInviso=yes\n[IvanBomb]\nIvanBomb=yes\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    let americans = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            americans, 0, None, false, 0, 10,
        ));
    // The rung is a human player's (`0x007003ED`).
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(americans));
    let ivan = sim
        .spawn_object("IVAN", "Americans", 3, 5, 0, &rules)
        .unwrap();
    assert!(friendly_ivans(&sim, &rules, &[ivan], 1, ivan, false, None).is_empty());
    assert_eq!(
        friendly_ivans(&sim, &rules, &[ivan], 2, ivan, false, None),
        [(ivan, FriendlyIvanAction::Bomb)]
    );
}

/// Role-derived Infiltrate must preserve native C4-before-Capture order,
/// including terminal ordinary Attack when the building refuses C4.
#[test]
fn specialist_infiltration_keeps_c4_click_and_cursor_priority() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=GHOST\n1=SPY\n[BuildingTypes]\n0=PLANT\n1=BUNK\n2=FOLD\n[VehicleTypes]\n0=TRUCK\n\
             [GHOST]\nStrength=100\nC4=yes\nPrimary=Rifle\n\
             [SPY]\nStrength=100\nAgent=yes\n\
             [PLANT]\nStrength=750\nCapturable=yes\nSpyable=yes\nCanC4=yes\n\
             [BUNK]\nStrength=750\nCapturable=yes\nSpyable=yes\nCanC4=no\n\
             [FOLD]\nStrength=750\nCapturable=yes\nSpyable=yes\nUndeploysInto=TRUCK\n\
             [TRUCK]\nStrength=100\n\
             [Rifle]\nDamage=10\nRange=8\nProjectile=Invisible\nWarhead=AP\n\
             [Invisible]\nInviso=yes\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ),
    ).unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    for (name, human) in [("Americans", true), ("Soviets", false)] {
        let owner = sim.intern(name);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, human, 0, 10,
            ));
    }
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let house = sim.interner().get("Americans");
    sim.session_mut_for_test().set_current_house_for_test(house);
    let seal = sim
        .spawn_object("GHOST", "Americans", 3, 5, 0, &rules)
        .unwrap();
    let spy = sim
        .spawn_object("SPY", "Americans", 4, 5, 0, &rules)
        .unwrap();
    let plant = sim
        .spawn_object("PLANT", "Soviets", 6, 5, 0, &rules)
        .unwrap();
    let bunker = sim
        .spawn_object("BUNK", "Soviets", 8, 5, 0, &rules)
        .unwrap();
    let fold = sim
        .spawn_object("FOLD", "Soviets", 10, 5, 0, &rules)
        .unwrap();
    assert!(rules.object("GHOST").unwrap().infiltrate);
    assert!(rules.object("SPY").unwrap().infiltrate);
    for (target, expected_action, expected_cursor) in [
        (
            plant,
            InfantryC4Action::Sabotage,
            crate::app::types::CursorFeedbackKind::Demolish,
        ),
        (
            bunker,
            InfantryC4Action::Attack,
            crate::app::types::CursorFeedbackKind::EnemyStructure,
        ),
    ] {
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::EnemyStructure,
            stable_id: target,
        };
        assert!(sim.infiltrator_building_action(seal, target, &rules));
        let mut selected = vec![seal, spy];
        assert_eq!(
            infiltrator_capture_orders(
                &sim,
                &rules,
                None,
                &mut selected,
                &hover,
                OrderModifier::Normal,
                false,
            ),
            Some(vec![Command::CaptureBuilding {
                engineer_id: spy,
                target_building_id: target
            }])
        );
        assert_eq!(selected, [seal]);
        assert_eq!(
            infantry_c4_action(
                &sim,
                &rules,
                seal,
                &hover,
                infantry_arm_attacks(
                    &sim,
                    &rules,
                    seal,
                    target,
                    OrderModifier::Normal,
                    false,
                    None
                )
            ),
            Some(expected_action)
        );
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[seal],
                Some(seal),
                &hover,
                Some(&rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
                &mut Default::default(),
            ),
            expected_cursor
        );
    }
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::EnemyStructure,
        stable_id: fold,
    };
    assert!(rules.object("FOLD").unwrap().is_1x1_with_undeploy());
    assert!(infantry_arm_attacks(
        &sim,
        &rules,
        seal,
        fold,
        OrderModifier::Normal,
        false,
        None
    ));
    assert_eq!(infantry_c4_action(&sim, &rules, seal, &hover, true), None);
    assert_eq!(
        infiltrator_capture_orders(
            &sim,
            &rules,
            None,
            &mut vec![seal],
            &hover,
            OrderModifier::Normal,
            false,
        ),
        Some(vec![Command::CaptureBuilding {
            engineer_id: seal,
            target_building_id: fold
        }])
    );
    assert_eq!(
        crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[seal],
            Some(seal),
            &hover,
            Some(&rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        ),
        crate::app::types::CursorFeedbackKind::Enter
    );
}

/// `TechnoClass::What_Action_OnObject @ 0x00700548`: unforced, the
/// attack rung answers Attack only while the actor's GetFireError for the
/// object is not ILLEGAL; otherwise the tail answers, and a player's
/// Infantry turns its 0 into NoMove. Force fire keeps the rung
/// (`0x00700555`). Out of range is RANGE (T61, the last test), not
/// ILLEGAL. A Crazy Ivan's bombed target is ILLEGAL at any distance (T55,
/// `0x006FCBAD`, comes first). ClickedMission promotes only an Attack
/// that passed, so an attack-move click needs it too.
#[test]
fn an_unforced_click_refuses_only_an_illegal_shot() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=IVAN\n1=GI\n[VehicleTypes]\n0=HTNK\n\
             [CombatDamage]\nIvanTimedDelay=450\n\
             [IVAN]\nStrength=125\nPrimary=IvanBomber\nIvan=yes\n\
             [GI]\nStrength=125\nPrimary=Pea\n\
             [HTNK]\nStrength=900\nArmor=heavy\nPrimary=120mm\n\
             [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
             [Pea]\nDamage=10\nRange=5\nProjectile=Invisible\nWarhead=Soft\n\
             [120mm]\nDamage=90\nRange=5.75\nProjectile=Invisible\nWarhead=AP\n\
             [Invisible]\nInviso=yes\n\
             [IvanBomb]\nIvanBomb=yes\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [Soft]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    for house in ["Americans", "Soviets"] {
        let id = sim.intern(house);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, false, 0, 10,
            ));
    }
    // The attack rung is a human player's (`0x007003ED`).
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let house = sim.interner().get("Americans");
    sim.session_mut_for_test().set_current_house_for_test(house);
    let mut spawn =
        |kind: &str, owner: &str, rx: u16| sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap();
    let tank = spawn("HTNK", "Americans", 3);
    let gi = spawn("GI", "Americans", 4);
    let ivan = spawn("IVAN", "Americans", 5);
    let near = spawn("HTNK", "Soviets", 6);
    let far = spawn("HTNK", "Soviets", 20);
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let action = |sim: &Simulation, actor: u64, target: u64, forced: bool| {
        attack_action(
            sim,
            &rules,
            actor,
            &TargetKind::Entity(target),
            forced,
            Some(&overlays),
        )
    };

    assert_eq!(
        action(&sim, gi, near, false),
        AttackAction::NoMove,
        "0% Verses against heavy armour"
    );
    assert_eq!(action(&sim, gi, near, true), AttackAction::Attack);
    assert_eq!(action(&sim, tank, near, false), AttackAction::Attack);
    assert_eq!(
        action(&sim, tank, far, false),
        AttackAction::Attack,
        "out of range"
    );
    assert_eq!(action(&sim, ivan, near, false), AttackAction::Attack);
    let americans = sim.interner().get("Americans").unwrap();
    assert_eq!(
        object_attack_click_command(
            &sim,
            &rules,
            &overlays,
            americans,
            OrderMode::AttackMove,
            OrderModifier::Normal,
            false,
            tank,
            near,
        ),
        Some(Command::AttackMove {
            entity_id: tank,
            target: Some(TargetKind::Entity(near)),
            destination: None,
        })
    );

    sim.bomb_attach_for_test(ivan, Some(near), &rules);
    sim.bomb_attach_for_test(ivan, Some(far), &rules);
    assert_eq!(
        action(&sim, ivan, near, false),
        AttackAction::NoMove,
        "a bombed target"
    );
    assert_eq!(
        action(&sim, ivan, far, false),
        AttackAction::NoMove,
        "the bomb gate precedes the range test"
    );
}

/// The Infiltrate arm keeps an unforced ILLEGAL shot on the rung
/// (`0x007004A0..0x00700531`): an `Agent=` infantryman over a `Spyable=`
/// building, another over a `Capturable=` one, and a `C4=` one over a
/// building that is `CanC4=` (a building's default). Any other building
/// takes the tail. The reader also enables Infiltrate for C4, Engineer
/// and Agent types without an explicit Infiltrate key.
#[test]
fn the_infiltrate_arm_keeps_an_illegal_shot_on_the_rung() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=SPY\n1=ENGI\n2=SEAL\n\
             [BuildingTypes]\n0=LAB\n1=DEPOT\n2=BUNK\n\
             [SPY]\nStrength=100\nInfiltrate=yes\nAgent=yes\nPrimary=Pea\n\
             [ENGI]\nStrength=100\nInfiltrate=yes\nPrimary=Pea\n\
             [SEAL]\nStrength=100\nInfiltrate=yes\nC4=yes\nPrimary=Pea\n\
             [LAB]\nStrength=750\nArmor=heavy\nSpyable=yes\n\
             [DEPOT]\nStrength=750\nArmor=heavy\nCapturable=yes\n\
             [BUNK]\nStrength=750\nArmor=heavy\nCanC4=no\n\
             [Pea]\nDamage=10\nRange=5\nProjectile=Invisible\nWarhead=Soft\n\
             [Invisible]\nInviso=yes\n\
             [Soft]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    for (house, human) in [("Americans", true), ("Soviets", false)] {
        let id = sim.intern(house);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, human, 0, 10,
            ));
    }
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let house = sim.interner().get("Americans");
    sim.session_mut_for_test().set_current_house_for_test(house);
    let mut spawn = |kind: &str, owner: &str, rx: u16, ry: u16| {
        sim.spawn_object(kind, owner, rx, ry, 0, &rules).unwrap()
    };
    let spy = spawn("SPY", "Americans", 4, 5);
    let engineer = spawn("ENGI", "Americans", 5, 5);
    let seal = spawn("SEAL", "Americans", 5, 8);
    let lab = spawn("LAB", "Soviets", 7, 5);
    let depot = spawn("DEPOT", "Soviets", 7, 8);
    let bunker = spawn("BUNK", "Soviets", 10, 8);
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let action = |actor, target| {
        attack_action(
            &sim,
            &rules,
            actor,
            &TargetKind::Entity(target),
            false,
            Some(&overlays),
        )
    };
    assert_eq!(action(spy, lab), AttackAction::Attack);
    assert_eq!(action(spy, depot), AttackAction::NoMove);
    assert_eq!(action(engineer, depot), AttackAction::Attack);
    assert_eq!(action(engineer, lab), AttackAction::NoMove);
    assert_eq!(action(seal, lab), AttackAction::Attack);
    assert_eq!(action(seal, bunker), AttackAction::NoMove);
}

/// A shot ILLEGAL to an armed receiver leaves its What_Action 0
/// (`0x0070056C..0x007005E6`), which a player's Infantry or Unit it may
/// control turns into NoMove (`0x0051F05E..0x0051F08B`,
/// `0x007403EB..0x00740414`): the click moves it near the object, as an
/// attack-move under the chord, and the cursor shows NoMove. A retail
/// Crazy Ivan's bombed target is ILLEGAL to him (`0x006FCBAD`), and a
/// retail Grizzly is ILLEGAL to an attack dog.
#[test]
fn a_refused_shot_moves_the_receiver_near_the_object() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let owner = sim.interner().get("Local").unwrap();
    let ivan = sim.spawn_object("IVAN", "Local", 6, 4, 0, &rules).unwrap();
    let dog = sim.spawn_object("DOG", "Local", 5, 4, 0, &rules).unwrap();
    let tank = sim.spawn_object("MTNK", "Enemy", 6, 6, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
    sim.fog_mut_for_test()
        .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
    sim.bomb_attach_for_test(ivan, Some(tank), &rules);
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    for actor in [ivan, dog] {
        assert_eq!(
            attack_action(
                &sim,
                &rules,
                actor,
                &TargetKind::Entity(tank),
                false,
                Some(&overlays)
            ),
            AttackAction::NoMove
        );
        // The cursor shows that NoMove too.
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[actor],
                Some(actor),
                &HoverTargetKindWithId {
                    kind: HoverTargetKind::EnemyUnit,
                    stable_id: tank,
                },
                Some(&rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
                &mut Default::default(),
            ),
            crate::app::types::CursorFeedbackKind::Invalid
        );
    }
    let click = |order_mode| {
        object_attack_click_command(
            &sim,
            &rules,
            &overlays,
            owner,
            order_mode,
            OrderModifier::Normal,
            false,
            ivan,
            tank,
        )
    };
    let near = crate::app::input::commands::object_move_destination(
        &sim,
        &rules,
        owner,
        ivan,
        tank,
        crate::sim::world::MoveAction::NoMove,
        false,
    );
    let Some(TargetKind::Cell(target_rx, target_ry)) = near else {
        panic!("NoMove searches near the tank: {near:?}");
    };
    assert_eq!(
        click(OrderMode::Move),
        Some(Command::Move {
            entity_id: ivan,
            target_rx,
            target_ry,
        })
    );
    assert_eq!(
        click(OrderMode::AttackMove),
        Some(Command::AttackMove {
            entity_id: ivan,
            target: None,
            destination: near,
        })
    );
}

/// An unarmed object's What_Action over another house's object skips the
/// attack rung (`0x00700464..0x00700474`) for the tail
/// (`0x0070059D..0x007005E6`): Select over an object the player may
/// select and has not, else 0, which a player's Unit it may control
/// turns into NoMove (`0x007403EB`). A loaner the player owns answers 0.
/// The click selects the object when the object that owns the cursor
/// answers Select (`0x004ABDD3..0x004ABDDA`), and an armed object
/// outranks unarmed ones (`0x00535435..0x0053549C`); NoMove moves the
/// receiver near the object. Retail `[CMIN]` (`Primary=none`) and
/// `[AMCV]` are unarmed. Another house's armed object asks no rung
/// (`0x007003ED`) and no owner test, so it answers Select.
#[test]
fn an_unarmed_receiver_selects_another_houses_object() {
    use crate::sim::world::MoveAction;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let owner = sim.interner().get("Local").unwrap();
    let miner = sim.spawn_object("CMIN", "Local", 2, 4, 0, &rules).unwrap();
    let mcv = sim.spawn_object("AMCV", "Local", 2, 6, 0, &rules).unwrap();
    let tank = sim.spawn_object("MTNK", "Local", 4, 4, 0, &rules).unwrap();
    let enemy = sim.spawn_object("MTNK", "Enemy", 6, 5, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
    sim.fog_mut_for_test()
        .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let action = |sim: &Simulation, actor| {
        attack_action(sim, &rules, actor, &TargetKind::Entity(enemy), false, None)
    };
    let release = |sim: &Simulation, selection: &[u64]| {
        object_click_release(sim, &rules, selection, selection, enemy, &overlays, false)
    };
    let click = |sim: &Simulation, actor| {
        object_attack_click_command(
            sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            OrderModifier::Normal,
            false,
            actor,
            enemy,
        )
    };
    let cursor = |sim: &Simulation, actor| {
        crate::app::input::cursor::capability_cursor_for_hover(
            sim,
            &[actor],
            Some(actor),
            &HoverTargetKindWithId {
                kind: HoverTargetKind::EnemyUnit,
                stable_id: enemy,
            },
            Some(&rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        )
    };

    assert_eq!(action(&sim, miner), AttackAction::Select);
    assert_eq!(action(&sim, mcv), AttackAction::Select);
    assert_eq!(action(&sim, tank), AttackAction::Attack);
    assert_eq!(
        cursor(&sim, miner),
        crate::app::types::CursorFeedbackKind::FriendlyUnit
    );
    assert_eq!(release(&sim, &[miner, mcv]), LeftRelease::Select);
    assert_eq!(
        release(&sim, &[miner, tank]),
        LeftRelease::Dispatch,
        "the armed tank owns the cursor"
    );
    assert_eq!(
        object_click_release(&sim, &rules, &[mcv], &[], enemy, &overlays, false),
        LeftRelease::Dispatch,
        "a capability arm took the object that owns the cursor"
    );
    assert_eq!(click(&sim, mcv), None);

    sim.entity_mut_for_test(enemy)
        .unwrap()
        .seldom
        .selection
        .set_selected_for_test(true);
    assert_eq!(action(&sim, mcv), AttackAction::NoMove);
    assert_eq!(
        cursor(&sim, mcv),
        crate::app::types::CursorFeedbackKind::Invalid
    );
    assert_eq!(release(&sim, &[mcv]), LeftRelease::Dispatch);
    let near = crate::app::input::commands::object_move_destination(
        &sim,
        &rules,
        owner,
        mcv,
        enemy,
        MoveAction::NoMove,
        false,
    );
    let Some(TargetKind::Cell(target_rx, target_ry)) = near else {
        panic!("NoMove searches near the tank: {near:?}");
    };
    assert_eq!(
        click(&sim, mcv),
        Some(Command::Move {
            entity_id: mcv,
            target_rx,
            target_ry,
        })
    );
    sim.entity_mut_for_test(enemy)
        .unwrap()
        .seldom
        .selection
        .set_selected_for_test(false);

    let loaner = sim.entity_mut_for_test(mcv).unwrap();
    loaner.seldom.mission_only.mark();
    loaner
        .seldom
        .discovery
        .set_owned_by_current_house_for_test(true);
    assert_eq!(action(&sim, mcv), AttackAction::NoMove);

    assert_eq!(
        attack_action(&sim, &rules, enemy, &TargetKind::Entity(tank), false, None),
        AttackAction::Select
    );
}

/// `AircraftClass::What_Action` over an object (`0x00417CC0`): the Foot
/// base's 0 becomes the aircraft's own What_Action on the Cell under the
/// object (`0x00417D7C..0x00417DCB`). A retail Harrier (`ORCA`) over an
/// enemy Kirov in flight, which its `AirToGroundMissile` cannot hit, and
/// over an allied Rocketeer, answers Move: the Move cursor, a dispatched
/// release and a Move order towards the object. Attack over an `Immune=`
/// type is NoMove (`0x00417F4A..0x00417F63`), which the click refuses
/// (`0x00417C12`).
#[test]
fn a_harrier_moves_to_an_object_it_may_not_attack() {
    use crate::app::types::CursorFeedbackKind;
    let patch = crate::rules::ini_parser::IniFile::from_str("[MTNK]\nImmune=yes\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &patch,
    ) else {
        return;
    };
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let owner = sim.interner().get("Local").unwrap();
    let harrier = sim.spawn_object("ORCA", "Local", 5, 4, 0, &rules).unwrap();
    let kirov = sim.spawn_object("ZEP", "Enemy", 7, 6, 0, &rules).unwrap();
    let rocketeer = sim
        .spawn_object("JUMPJET", "Ally", 6, 8, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("MTNK", "Enemy", 12, 10, 0, &rules)
        .unwrap();
    let own_tank = sim.spawn_object("MTNK", "Local", 6, 4, 0, &rules).unwrap();
    let wingman = sim.spawn_object("ORCA", "Local", 6, 5, 0, &rules).unwrap();
    sim.entity_mut_for_test(wingman)
        .unwrap()
        .seldom
        .selection
        .set_selected_for_test(true);
    for flier in [kirov, rocketeer] {
        sim.entity_mut_for_test(flier)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(700));
    }
    for (asker, other) in [("LOCAL", "ALLY"), ("ALLY", "LOCAL")] {
        sim.house_alliances_mut_for_test()
            .entry(asker.to_string())
            .or_default()
            .insert(other.to_string());
    }
    sim.resolve_type_handles(&rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
    sim.fog_mut_for_test()
        .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let action = |target| {
        attack_action(
            &sim,
            &rules,
            harrier,
            &TargetKind::Entity(target),
            false,
            None,
        )
    };
    let cursor = |target, kind| {
        crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[harrier],
            Some(harrier),
            &HoverTargetKindWithId {
                kind,
                stable_id: target,
            },
            Some(&rules),
            None,
            OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        )
    };
    let click = |target| {
        object_attack_click_command(
            &sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            OrderModifier::Normal,
            false,
            harrier,
            target,
        )
    };

    for (target, kind) in [
        (kirov, HoverTargetKind::EnemyUnit),
        (rocketeer, HoverTargetKind::FriendlyUnit),
    ] {
        assert_eq!(action(target), AttackAction::Move);
        assert_eq!(cursor(target, kind), CursorFeedbackKind::Move);
        assert_eq!(
            object_click_release(
                &sim,
                &rules,
                &[harrier],
                &[harrier],
                target,
                &overlays,
                false
            ),
            LeftRelease::Dispatch
        );
        assert!(
            matches!(
                click(target),
                Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                    if entity_id == harrier
            ),
            "{:?}",
            click(target)
        );
    }
    assert_eq!(action(tank), AttackAction::NoMove);
    assert_eq!(
        cursor(tank, HoverTargetKind::EnemyUnit),
        CursorFeedbackKind::Invalid
    );
    assert_eq!(click(tank), None);

    // Own objects: one not selected is Techno's Select (7); a selected
    // one in a larger selection is 0, so the Cell action, Move; a lone
    // Harrier over itself is Self, which 417DCD turns into 0 (the arrow).
    assert_eq!(action(own_tank), AttackAction::Select);
    assert_eq!(
        cursor(own_tank, HoverTargetKind::FriendlyUnit),
        CursorFeedbackKind::FriendlyUnit
    );
    assert_eq!(
        crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[harrier, wingman],
            Some(harrier),
            &HoverTargetKindWithId {
                kind: HoverTargetKind::FriendlyUnit,
                stable_id: wingman,
            },
            Some(&rules),
            None,
            OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        ),
        CursorFeedbackKind::Move
    );
    assert_eq!(
        cursor(harrier, HoverTargetKind::FriendlyUnit),
        CursorFeedbackKind::Default
    );
}

/// Foot What_Action's shroud rung (`0x004DDED0`): in a campaign, Techno's
/// Attack over an enemy Tesla Reactor whose coordinate the player's map
/// shrouds becomes Move for a Grizzly (`MoveToShroud=` defaults to yes),
/// so the Move cursor and a move to it, and NoMove for a Chrono
/// Legionnaire (`MoveToShroud=no`), whose click Foot `0x004D74E0`
/// refuses there. Outside a campaign both keep Attack.
#[test]
fn campaign_shroud_over_an_object_answers_move_or_no_move() {
    use crate::app::types::CursorFeedbackKind;
    use crate::sim::scenario_session::ScenarioGameMode;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4DDED0);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let tank = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let legionnaire = sim
        .spawn_object("CLEG", "Americans", 5, 6, 0, &rules)
        .unwrap();
    let reactor = sim
        .spawn_object("NAPOWR", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let centre = sim.object_coords_cell(reactor).unwrap();
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    sim.fog_mut_for_test().reveal_cells_for_owner(
        owner,
        (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|&cell| cell != centre),
    );
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let action = |sim: &Simulation, actor| {
        attack_action(
            sim,
            &rules,
            actor,
            &TargetKind::Entity(reactor),
            false,
            None,
        )
    };
    let click = |sim: &Simulation, actor| {
        object_attack_click_command(
            sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            OrderModifier::Normal,
            false,
            actor,
            reactor,
        )
    };

    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    for actor in [tank, legionnaire] {
        assert_eq!(action(&sim, actor), AttackAction::Attack);
    }
    assert!(matches!(click(&sim, tank), Some(Command::Attack { .. })));

    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Campaign);
    assert_eq!(action(&sim, tank), AttackAction::Move);
    assert_eq!(action(&sim, legionnaire), AttackAction::NoMove);
    for (actor, cursor) in [
        (tank, CursorFeedbackKind::Move),
        (legionnaire, CursorFeedbackKind::Invalid),
    ] {
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[actor],
                Some(actor),
                &HoverTargetKindWithId {
                    kind: HoverTargetKind::EnemyStructure,
                    stable_id: reactor,
                },
                Some(&rules),
                None,
                OrderModifier::Normal,
                false,
                false,
                &mut Default::default(),
            ),
            cursor
        );
    }
    assert!(
        matches!(
            click(&sim, tank),
            Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                if entity_id == tank
        ),
        "{:?}",
        click(&sim, tank)
    );
    assert_eq!(click(&sim, legionnaire), None);
}

/// The campaign arena of the test above, with the player's map shrouding
/// the coordinate of `target`.
fn shrouded_campaign_arena(
    rules: &crate::rules::ruleset::RuleSet,
    seed: u64,
    actors: &[&str],
    target: (&str, &str),
) -> (Simulation, InternedId, Vec<u64>, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, seed);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let actors = (0..)
        .zip(actors)
        .map(|(dy, kind)| sim.spawn_object(kind, "Americans", 5, 5 + dy, 0, rules))
        .collect::<Option<Vec<_>>>()
        .unwrap();
    let target = sim
        .spawn_object(target.0, target.1, 10, 10, 0, rules)
        .unwrap();
    let centre = sim.object_coords_cell(target).unwrap();
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    sim.fog_mut_for_test().reveal_cells_for_owner(
        owner,
        (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|&cell| cell != centre),
    );
    (sim, owner, actors, target)
}

/// Foot's shroud rung (`0x004DDED0`) rewrites Techno's key answers too.
/// Over the player's own Power Plant whose coordinate the map shrouds,
/// Shift's ToggleSelect (`0x00700107`), Alt's Move (`0x007001E9`) and
/// Ctrl+Alt's GuardArea (`0x007001DD`) become Move for a Grizzly and
/// NoMove for a Chrono Legionnaire (`MoveToShroud=no`) in a campaign,
/// cursor and click alike: the Ctrl+Alt click moves the Grizzly and the
/// Legionnaire's is refused (`0x004D77BD..0x004D77EA`). Outside a
/// campaign each chord keeps its own answer.
#[test]
fn campaign_shroud_rewrites_the_key_chords_over_an_object() {
    use crate::app::types::CursorFeedbackKind;
    use crate::sim::scenario_session::ScenarioGameMode;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, owner, actors, plant) =
        shrouded_campaign_arena(&rules, 0x4DDED0, &["MTNK", "CLEG"], ("GAPOWR", "Americans"));
    let [tank, legionnaire] = actors[..] else {
        unreachable!()
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let key = |sim: &Simulation, actor, modifier| {
        object_key_action(sim, &rules, actor, plant, Some(plant), modifier, false)
    };
    let cursor = |sim: &Simulation, actor, modifier| {
        crate::app::input::cursor::capability_cursor_for_hover(
            sim,
            &[actor],
            Some(actor),
            &HoverTargetKindWithId {
                kind: HoverTargetKind::FriendlyStructure,
                stable_id: plant,
            },
            Some(&rules),
            None,
            modifier,
            false,
            false,
            &mut Default::default(),
        )
    };
    let guard_click = |sim: &Simulation, actor| {
        object_attack_click_command(
            sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            OrderModifier::GuardArea,
            false,
            actor,
            plant,
        )
    };
    let chords = [
        OrderModifier::Shift,
        OrderModifier::ForceMove,
        OrderModifier::GuardArea,
    ];

    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    for (modifier, action, feedback) in [
        (
            OrderModifier::Shift,
            ObjectKeyAction::ToggleSelect,
            CursorFeedbackKind::Select,
        ),
        (
            OrderModifier::ForceMove,
            ObjectKeyAction::Move,
            CursorFeedbackKind::Move,
        ),
        (
            OrderModifier::GuardArea,
            ObjectKeyAction::GuardArea,
            CursorFeedbackKind::GuardArea,
        ),
    ] {
        assert_eq!(key(&sim, tank, modifier), Some(action));
        assert_eq!(cursor(&sim, tank, modifier), feedback);
    }
    assert!(matches!(
        guard_click(&sim, tank),
        Some(Command::Guard { .. })
    ));

    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Campaign);
    for modifier in chords {
        assert_eq!(key(&sim, tank, modifier), Some(ObjectKeyAction::Move));
        assert_eq!(cursor(&sim, tank, modifier), CursorFeedbackKind::Move);
        assert_eq!(
            key(&sim, legionnaire, modifier),
            Some(ObjectKeyAction::NoMove)
        );
        assert_eq!(
            cursor(&sim, legionnaire, modifier),
            CursorFeedbackKind::Invalid
        );
    }
    assert!(
        matches!(
            guard_click(&sim, tank),
            Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                if entity_id == tank
        ),
        "{:?}",
        guard_click(&sim, tank)
    );
    assert_eq!(guard_click(&sim, legionnaire), None);
}

/// The passenger rows take the Move Foot's shroud rung (`0x004DDED0`)
/// makes of Shift's ToggleSelect (`0x00700107`), which otherwise returns
/// first (`0x0051E453`): in a campaign a GI's Shift over the player's own
/// IFV whose coordinate the map shrouds enters it, cursor and click
/// alike. In a skirmish Shift toggles the selection.
#[test]
fn campaign_shroud_shift_takes_the_passenger_rows() {
    use crate::app::types::CursorFeedbackKind;
    use crate::sim::scenario_session::ScenarioGameMode;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, _, actors, ifv) =
        shrouded_campaign_arena(&rules, 0x0051_E453, &["E1"], ("FV", "Americans"));
    let gi = actors[0];
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: ifv,
    };
    let cursor = |sim: &Simulation| {
        crate::app::input::cursor::capability_cursor_for_hover(
            sim,
            &[gi],
            Some(gi),
            &hover,
            Some(&rules),
            None,
            OrderModifier::Shift,
            false,
            false,
            &mut Default::default(),
        )
    };
    let click = |sim: &Simulation| {
        entry_orders(
            sim,
            &rules,
            &[gi],
            &mut vec![gi],
            &hover,
            OrderModifier::Shift,
            false,
            None,
        )
    };

    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    assert_eq!(cursor(&sim), CursorFeedbackKind::Select);
    assert_eq!(click(&sim), None);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Campaign);
    assert_eq!(cursor(&sim), CursorFeedbackKind::Enter);
    assert_eq!(
        click(&sim),
        Some(vec![Command::MegaMission {
            entity_id: gi,
            mission: crate::sim::mission::MissionType::Enter,
            target: None,
            destination: Some(TargetKind::Entity(ifv)),
        }])
    );
}

/// Infantry's C4 and infiltration arms need the Attack Foot's What_Action
/// hands them (`cmp ebp,5` at `0x0051E9F1` and `0x0051EE62`). Over an
/// enemy Tesla Reactor Ctrl+Shift+Alt answers Move (Shift clears Ctrl at
/// `0x007000AC..0x007000B4`, Alt's rung `0x007001E9..0x00700217`), and in
/// a campaign the shroud rung (`0x004DDED0`) makes a plain click's Attack
/// Move where the map shrouds the reactor's coordinate. Then Tanya
/// neither shows Demolish nor plants C4, and the Spy neither shows Enter
/// nor infiltrates: both show Move. A plain skirmish click takes both.
#[test]
fn c4_and_infiltration_need_the_attack_foot_hands_them() {
    use crate::app::types::CursorFeedbackKind;
    use crate::sim::scenario_session::ScenarioGameMode;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let (mut sim, _, actors, reactor) =
        shrouded_campaign_arena(&rules, 0x51E9F1, &["TANY", "SPY"], ("NAPOWR", "Russians"));
    let [tanya, spy] = actors[..] else {
        unreachable!()
    };
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::EnemyStructure,
        stable_id: reactor,
    };
    let check = |sim: &Simulation, modifier, alt, takes_arms| {
        let cursor = |actor| {
            crate::app::input::cursor::capability_cursor_for_hover(
                sim,
                &[actor],
                Some(actor),
                &hover,
                Some(&rules),
                None,
                modifier,
                alt,
                false,
                &mut Default::default(),
            )
        };
        let attacks = infantry_arm_attacks(sim, &rules, tanya, reactor, modifier, alt, None);
        let c4 = infantry_c4_action(sim, &rules, tanya, &hover, attacks);
        let infiltrate =
            infiltrator_capture_orders(sim, &rules, None, &mut vec![spy], &hover, modifier, alt);
        if takes_arms {
            assert_eq!(cursor(tanya), CursorFeedbackKind::Demolish);
            assert_eq!(c4, Some(InfantryC4Action::Sabotage));
            assert_eq!(cursor(spy), CursorFeedbackKind::Enter);
            assert_eq!(
                infiltrate,
                Some(vec![Command::CaptureBuilding {
                    engineer_id: spy,
                    target_building_id: reactor,
                }])
            );
        } else {
            for actor in [tanya, spy] {
                assert_eq!(cursor(actor), CursorFeedbackKind::Move, "{modifier:?}");
            }
            assert_eq!(c4, None, "{modifier:?}");
            assert_eq!(infiltrate, None, "{modifier:?}");
        }
    };

    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    check(&sim, OrderModifier::Normal, false, true);
    check(&sim, OrderModifier::AttackMove, true, false);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Campaign);
    check(&sim, OrderModifier::Normal, false, false);
}

/// One of several selected own tanks under the cursor: Techno's self rung
/// needs one selected object (`0x007000C1`), so its tail answers 0 over
/// the selected tank (`0x007005BF`), which Unit turns into NoMove
/// (`0x007403EB`). The cursor is NoMove, and the click dispatches it
/// rather than selecting (`0x004ABE88`): the other tank moves near the
/// clicked one, which refuses itself (`0x007388D5..0x007388E8`).
#[test]
fn a_self_click_in_a_larger_selection_dispatches_no_move() {
    use crate::app::input::cursor::{
        ActionDistanceTarget, capability_cursor_for_hover, cursor_id_for_feedback,
        select_best_for_action,
    };
    use crate::app::types::CursorId;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0070_05BF);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    sim.fog_mut_for_test()
        .reveal_cells_for_owner(owner, (0..32).flat_map(|y| (0..32).map(move |x| (x, y))));
    let other = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let clicked = sim
        .spawn_object("MTNK", "Americans", 9, 5, 0, &rules)
        .unwrap();
    let selected = [other, clicked];
    for id in selected {
        sim.entity_mut_for_test(id)
            .unwrap()
            .seldom
            .selection
            .set_selected_for_test(true);
    }
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let best = select_best_for_action(
        &sim,
        &selected,
        ActionDistanceTarget::Object(clicked),
        Some(&rules),
    );
    let cursor = capability_cursor_for_hover(
        &sim,
        &selected,
        best,
        &HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: clicked,
        },
        Some(&rules),
        None,
        OrderModifier::Normal,
        false,
        false,
        &mut Default::default(),
    );
    assert_eq!(cursor_id_for_feedback(cursor), Some(CursorId::NoMove));
    assert_eq!(
        object_click_release(
            &sim, &rules, &selected, &selected, clicked, &overlays, false
        ),
        LeftRelease::Dispatch
    );
    let click = |actor| {
        object_attack_click_command(
            &sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            OrderModifier::Normal,
            false,
            actor,
            clicked,
        )
    };
    assert!(
        matches!(
            click(other),
            Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                if entity_id == other
        ),
        "{:?}",
        click(other)
    );
    assert_eq!(click(clicked), None);
}

/// The Harrier's Cell query under the object (`0x00417D7C..0x00417DCB`)
/// reaches Techno's unforced Cell Attack rung (`0x007008A6..0x007009F1`):
/// over an enemy Kirov in flight above another house's GAWALL, which the
/// Harrier's `ORCAAP` (`Wall=yes`) may hit, the answer is Attack, not
/// Move: the attack cursor, and the click attacks the Kirov (Aircraft
/// `0x00417BD0` -> Foot `0x004D74E0`). The allied Rocketeer over such a
/// wall answers Attack too. Over the player's own wall it stays Move.
#[test]
fn a_harrier_over_a_flier_above_an_enemy_wall_answers_attack() {
    use crate::app::types::CursorFeedbackKind;
    let patch = crate::rules::ini_parser::IniFile::from_str("[DISK]\nImmune=yes\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &patch,
    ) else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(retail.fixed_art.as_ref()),
    );
    let wall = overlays.id_for_name("GAWALL").expect("retail GAWALL");
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let owner = sim.interner().get("Local").unwrap();
    let harrier = sim.spawn_object("ORCA", "Local", 5, 4, 0, &rules).unwrap();
    let kirov = sim.spawn_object("ZEP", "Enemy", 7, 6, 0, &rules).unwrap();
    let rocketeer = sim
        .spawn_object("JUMPJET", "Ally", 6, 8, 0, &rules)
        .unwrap();
    let own_wall_kirov = sim.spawn_object("ZEP", "Enemy", 10, 6, 0, &rules).unwrap();
    let shrouded_kirov = sim.spawn_object("ZEP", "Enemy", 12, 6, 0, &rules).unwrap();
    let disc = sim.spawn_object("DISK", "Enemy", 9, 9, 0, &rules).unwrap();
    for flier in [kirov, rocketeer, own_wall_kirov, shrouded_kirov, disc] {
        sim.entity_mut_for_test(flier)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(700));
    }
    for (asker, other) in [("LOCAL", "ALLY"), ("ALLY", "LOCAL")] {
        sim.house_alliances_mut_for_test()
            .entry(asker.to_string())
            .or_default()
            .insert(other.to_string());
    }
    let enemy = sim.interner().get("Enemy").unwrap();
    let (width, height) = {
        let terrain = sim.resolved_terrain().unwrap();
        (terrain.width(), terrain.height())
    };
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(width, height);
    for cell in [(7, 6), (6, 8), (12, 6), (9, 9)] {
        grid.place_owned_wall(cell.0, cell.1, wall, 0, enemy);
    }
    grid.place_owned_wall(10, 6, wall, 0, owner);
    *sim.overlay_grid_mut_for_test() = Some(grid);
    sim.resolve_type_handles(&rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
    sim.fog_mut_for_test().reveal_cells_for_owner(
        owner,
        (0..16)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .filter(|&cell| cell != (12, 6)),
    );
    let action = |target| {
        attack_action(
            &sim,
            &rules,
            harrier,
            &TargetKind::Entity(target),
            false,
            Some(&overlays),
        )
    };
    let cursor = |target, kind| {
        crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[harrier],
            Some(harrier),
            &HoverTargetKindWithId {
                kind,
                stable_id: target,
            },
            Some(&rules),
            Some(&overlays),
            OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        )
    };
    let click = |target| {
        object_attack_click_command(
            &sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            OrderModifier::Normal,
            false,
            harrier,
            target,
        )
    };
    // The ally's Rocketeer is a friend: without the wall it would be the
    // Move of `a_harrier_moves_to_an_object_it_may_not_attack`.
    for (target, kind) in [
        (kirov, HoverTargetKind::EnemyUnit),
        (rocketeer, HoverTargetKind::FriendlyUnit),
    ] {
        assert_eq!(action(target), AttackAction::Attack);
        assert!(
            matches!(
                cursor(target, kind),
                CursorFeedbackKind::EnemyUnit | CursorFeedbackKind::EnemyOutOfRange
            ),
            "{:?}",
            cursor(target, kind)
        );
        assert_eq!(
            object_click_release(
                &sim,
                &rules,
                &[harrier],
                &[harrier],
                target,
                &overlays,
                false
            ),
            LeftRelease::Dispatch
        );
        assert_eq!(
            click(target),
            Some(Command::Attack {
                attacker_id: harrier,
                target_id: target,
            })
        );
    }
    // The player's own wall admits no rung; the shroud wrapper takes the
    // Attack back over a shrouded Cell; Attack over an `Immune=` object is
    // NoMove (`0x00417F4A`).
    assert_eq!(action(own_wall_kirov), AttackAction::Move);
    assert_ne!(action(shrouded_kirov), AttackAction::Attack);
    assert_eq!(action(disc), AttackAction::NoMove);
}

/// Unit (`0x0074041B..0x0074049A`) and Infantry
/// (`0x0051F160..0x0051F179`) What_Action over an object turn a final
/// Attack over an `Immune=` type (`+0x233`) into NoMove, with or without
/// Ctrl: the NoMove cursor, and the click is Foot ClickedAction's Move
/// (`0x004D74E0`), not an Attack. Retail's `Immune=yes` object is the
/// bridge repair hut (`CABHUT`); the patched Grizzly shows the same answer
/// over a unit. A Rhino and a GI keep Attack over another enemy tank.
/// Infantry's C4 (`0x0051E9C1..0x0051EA56`) and Crazy Ivan
/// (`0x0051EB24..0x0051EB7E`) arms return before its Immune tail, so
/// Tanya still sabotages the hut and an Ivan still bombs it.
#[test]
fn a_tank_and_a_gi_refuse_to_attack_an_immune_object() {
    use crate::app::types::CursorFeedbackKind;
    let patch = crate::rules::ini_parser::IniFile::from_str("[MTNK]\nImmune=yes\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &patch,
    ) else {
        return;
    };
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    assert!(rules.object("CABHUT").is_some_and(|hut| hut.immune));
    let owner = sim.interner().get("Local").unwrap();
    let rhino = sim.spawn_object("HTNK", "Local", 5, 4, 0, &rules).unwrap();
    let gi = sim.spawn_object("E1", "Local", 5, 6, 0, &rules).unwrap();
    let grizzly = sim.spawn_object("MTNK", "Enemy", 4, 11, 0, &rules).unwrap();
    let enemy_rhino = sim.spawn_object("HTNK", "Enemy", 6, 8, 0, &rules).unwrap();
    let hut = sim
        .spawn_object("CABHUT", "Enemy", 13, 13, 0, &rules)
        .unwrap();
    let ivan = sim.spawn_object("IVAN", "Local", 4, 6, 0, &rules).unwrap();
    let tanya = sim.spawn_object("TANY", "Local", 4, 4, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(16, 16);
    sim.fog_mut_for_test()
        .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    for actor in [rhino, gi] {
        let action = |target, forced| {
            attack_action(
                &sim,
                &rules,
                actor,
                &TargetKind::Entity(target),
                forced,
                None,
            )
        };
        let cursor = |target, kind| {
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[actor],
                Some(actor),
                &HoverTargetKindWithId {
                    kind,
                    stable_id: target,
                },
                Some(&rules),
                None,
                OrderModifier::Normal,
                false,
                false,
                &mut Default::default(),
            )
        };
        let click = |target| {
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                owner,
                OrderMode::Move,
                OrderModifier::Normal,
                false,
                actor,
                target,
            )
        };
        assert_eq!(action(enemy_rhino, false), AttackAction::Attack);
        assert_eq!(
            cursor(enemy_rhino, HoverTargetKind::EnemyUnit),
            CursorFeedbackKind::EnemyUnit
        );
        assert!(matches!(
            click(enemy_rhino),
            Some(Command::Attack { .. } | Command::AttackMove { .. })
        ));
        for (target, kind) in [
            (grizzly, HoverTargetKind::EnemyUnit),
            (hut, HoverTargetKind::EnemyStructure),
        ] {
            if actor == gi {
                // The Infantry tail is infantry_attack_action's last arm.
                assert_eq!(action(target, true), AttackAction::Attack, "{target}");
                assert_eq!(
                    infantry_attack_action(&sim, &rules, gi, target),
                    Some(InfantryAttackAction::NoMove),
                    "{target}"
                );
            } else {
                assert_eq!(action(target, false), AttackAction::NoMove, "{target}");
                assert_eq!(action(target, true), AttackAction::NoMove, "{target}");
            }
            assert_eq!(
                cursor(target, kind),
                CursorFeedbackKind::Invalid,
                "{target}"
            );
            assert!(
                matches!(
                    click(target),
                    Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
                        if entity_id == actor
                ),
                "{target}: {:?}",
                click(target)
            );
        }
    }

    assert_eq!(
        attack_action(&sim, &rules, ivan, &TargetKind::Entity(hut), false, None),
        AttackAction::Attack
    );
    assert_eq!(infantry_attack_action(&sim, &rules, ivan, hut), None);
    assert_eq!(
        crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[ivan],
            Some(ivan),
            &HoverTargetKindWithId {
                kind: HoverTargetKind::EnemyStructure,
                stable_id: hut,
            },
            Some(&rules),
            None,
            OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        ),
        CursorFeedbackKind::IvanBomb
    );
    assert_eq!(
        infantry_attack_action(&sim, &rules, tanya, hut),
        Some(InfantryAttackAction::Sabotage)
    );
    assert!(matches!(
        object_attack_click_command(
            &sim,
            &rules,
            &overlays,
            owner,
            OrderMode::Move,
            OrderModifier::ForceFire,
            false,
            tanya,
            hut,
        ),
        Some(Command::PlantC4 { attacker_id, .. }) if attacker_id == tanya
    ));
}

/// What_Action_OnCell's force-fire rung is a human player's too
/// (`0x007008A6..0x007008B3`): another house's armed object, which the
/// player may select to inspect, answers 0 over a cell in its range
/// (`0x00700AB7..0x00700AC4`), where the player's answers Attack.
#[test]
fn force_fire_on_a_cell_is_a_human_players() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
    let tank = sim.spawn_object("MTNK", "Local", 4, 4, 0, &rules).unwrap();
    let enemy = sim.spawn_object("MTNK", "Enemy", 6, 5, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    let over_cell = |actor| armed_action(&sim, &rules, actor, &TargetKind::Cell(5, 6), true, None);
    assert_eq!(over_cell(tank), Some(AttackAction::Attack));
    assert_eq!(over_cell(enemy), Some(AttackAction::None));
}

/// An unarmed `Infiltrate=` infantryman keeps the attack rung
/// (`0x00700444..0x00700474`), but his class turns its Attack into 0 over
/// an object he may not infiltrate (`0x0051F04E..0x0051F05C`), and its
/// tail turns 0 into NoMove (`0x0051F05E..0x0051F08B`). Retail's SPY
/// carries its MakeupKit, so only modded data reaches this.
#[test]
fn an_unarmed_infiltrator_moves_near_an_object_it_may_not_enter() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=AGENT\n[VehicleTypes]\n0=MTNK\n\
             [AGENT]\nStrength=100\nInfiltrate=yes\nAgent=yes\n[MTNK]\nStrength=300\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    for (house, human) in [("Americans", true), ("Russians", false)] {
        let id = sim.intern(house);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, human, 0, 10,
            ));
    }
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let house = sim.interner().get("Americans");
    sim.session_mut_for_test().set_current_house_for_test(house);
    let agent = sim
        .spawn_object("AGENT", "Americans", 3, 5, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("MTNK", "Russians", 6, 5, 0, &rules)
        .unwrap();
    assert_eq!(
        attack_action(&sim, &rules, agent, &TargetKind::Entity(tank), false, None),
        AttackAction::NoMove
    );
}

/// The rally click names the local player's selected buildings that take
/// a rally: not their power plant, not another house's factory, not units.
#[test]
fn rally_click_names_only_the_players_rally_buildings() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[BuildingTypes]\n0=GAWEAP\n1=GAPOWR\n[VehicleTypes]\n0=MTNK\n\
             [GAWEAP]\nFactory=UnitType\n[GAPOWR]\nPower=100\n[MTNK]\nStrength=300\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let other = sim.intern("Russians");
    let factory_type = sim.intern("GAWEAP");
    let power_type = sim.intern("GAPOWR");
    let tank_type = sim.intern("MTNK");
    for (id, rx, house, kind, category) in [
        (1, 10, owner, factory_type, EntityCategory::Structure),
        (2, 11, owner, tank_type, EntityCategory::Unit),
        (3, 12, owner, power_type, EntityCategory::Structure),
        (4, 13, other, factory_type, EntityCategory::Structure),
        (5, 14, owner, factory_type, EntityCategory::Structure),
    ] {
        sim.entities_mut_for_test()
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                rx,
                10,
                0,
                0,
                house,
                Health::new(300),
                kind,
                category,
                0,
                5,
                category == EntityCategory::Unit,
            ));
    }

    let producer_ids = selected_rally_producer_ids(
        &sim,
        &sim.control_projection(),
        &rules,
        &[5, 4, 3, 2, 1, 5],
        owner,
    );

    assert_eq!(producer_ids, vec![5, 1]);
}

#[test]
fn refinery_cell_click_archives_before_repacking_and_self_click_does_not_repack() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[BuildingTypes]\n0=REFN\n[VehicleTypes]\n0=SMIN\n\
             [REFN]\nStrength=1000\nUndeploysInto=SMIN\nResourceGatherer=yes\nResourceDestination=yes\n\
             [SMIN]\nStrength=1000\nSpeed=3\n")).unwrap();
    let mut sim = Simulation::with_seed(6);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let refinery = sim
        .spawn_object("REFN", "Americans", 15, 15, 0, &rules)
        .unwrap();
    let enemy = sim
        .spawn_object("REFN", "Russians", 17, 15, 0, &rules)
        .unwrap();
    let owner = sim.interner().get("Americans").unwrap();
    let (orders, announce) = building_cell_orders(
        &sim,
        &rules,
        &[refinery, enemy, refinery],
        owner,
        (17, 16),
        false,
        false,
    );
    assert!(
        matches!(orders.as_slice(), [Command::SetRally { producer_ids, .. },
        Command::SellBuilding { entity_id }] if producer_ids == &[refinery] && *entity_id == refinery)
    );
    assert!(
        !announce,
        "ResourceDestination does not announce a factory rally"
    );
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyStructure,
        stable_id: refinery,
    };
    assert_eq!(
        self_click_command(&sim, &rules, None, owner, &[refinery], Some(&hover), false),
        None
    );
    // A missing input map cannot produce an archive;4437AD still queues SELL.
    *sim.resolved_terrain_mut_for_test() = None;
    let (orders, _) =
        building_cell_orders(&sim, &rules, &[refinery], owner, (17, 16), false, false);
    assert_eq!(
        orders,
        [Command::SellBuilding {
            entity_id: refinery
        }]
    );
}

/// The retail modifier map, one row per chord. Ctrl force-fires, Alt forces
/// a move, Ctrl+Shift is attack move and Ctrl+Alt is guard area.
#[test]
fn modifier_map_matches_retail_chords() {
    // (ctrl, shift, alt) -> verb
    assert_eq!(
        resolve_order_modifiers(false, false, false),
        OrderModifier::Normal
    );
    assert_eq!(
        resolve_order_modifiers(true, false, false),
        OrderModifier::ForceFire
    );
    assert_eq!(
        resolve_order_modifiers(false, false, true),
        OrderModifier::ForceMove
    );
    assert_eq!(
        resolve_order_modifiers(true, true, false),
        OrderModifier::AttackMove
    );
    assert_eq!(
        resolve_order_modifiers(true, false, true),
        OrderModifier::GuardArea
    );
    assert_eq!(
        resolve_order_modifiers(false, true, false),
        OrderModifier::Shift
    );
    // The chord test reads the raw key state, so a held Alt does not defeat
    // attack move, and Shift outranks Alt without Ctrl.
    assert_eq!(
        resolve_order_modifiers(true, true, true),
        OrderModifier::AttackMove
    );
    assert_eq!(
        resolve_order_modifiers(false, true, true),
        OrderModifier::Shift
    );
}

/// Each order speaks the voice slot of the mission it commits, and missions
/// with no dedicated slot fall to the VoiceSpecialAttack default branch.
#[test]
fn order_voice_key_follows_the_committed_mission() {
    // A self-click's deploy, unload or deploy-fire is the Unload mission.
    for command in [
        Command::DeployMcv { entity_id: 1 },
        Command::ToggleInfantryDeploy { entity_id: 1 },
        Command::UnloadPassengers { transport_id: 1 },
    ] {
        assert_eq!(order_voice_key(&command), Some("VoiceDeploy"));
    }
    assert_eq!(
        order_voice_key(&Command::Move {
            entity_id: 1,
            target_rx: 0,
            target_ry: 0,
        }),
        Some("VoiceMove")
    );
    // Attack-move commits through the Move voice slot, not the Attack one.
    assert_eq!(
        order_voice_key(&Command::AttackMove {
            entity_id: 1,
            target: None,
            destination: Some(crate::sim::combat::TargetKind::Cell(0, 0)),
        }),
        Some("VoiceMove")
    );
    assert_eq!(
        order_voice_key(&Command::Attack {
            attacker_id: 1,
            target_id: 2,
        }),
        Some("VoiceAttack")
    );
    assert_eq!(
        order_voice_key(&Command::clicked_mission(
            1,
            crate::sim::mission::MissionType::Harvest,
            TargetKind::Cell(0, 0)
        )),
        Some("VoiceHarvest")
    );
    assert_eq!(
        order_voice_key(&Command::clicked_mission(
            1,
            crate::sim::mission::MissionType::Enter,
            TargetKind::Entity(2)
        )),
        Some("VoiceEnter")
    );
    assert_eq!(
        order_voice_key(&Command::CaptureBuilding {
            engineer_id: 1,
            target_building_id: 2,
        }),
        Some("VoiceCapture")
    );
    // Area guard and sabotage have no slot of their own.
    assert_eq!(
        order_voice_key(&Command::Guard {
            entity_id: 1,
            target: None,
        }),
        Some("VoiceSpecialAttack")
    );
    assert_eq!(
        order_voice_key(&Command::PlantC4 {
            attacker_id: 1,
            target_building_id: 2,
        }),
        Some("VoiceSpecialAttack")
    );
}

fn chord_rules() -> crate::rules::ruleset::RuleSet {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=MTNK\n\
         1=HARV\n\
         2=SECONLY\n\
         3=CMIN\n\
         4=SHAD\n\
         5=SREF\n\
         [AircraftTypes]\n\
         0=ORCA\n\
         [BuildingTypes]\n\
         0=GAWEAP\n\
         [MTNK]\n\
         Strength=300\n\
         Primary=105mm\n\
         [HARV]\n\
         Strength=1000\n\
         Harvester=yes\n\
         Primary=105mm\n\
         [CMIN]\n\
         Strength=1000\n\
         Harvester=yes\n\
         Primary=none\n\
         [SHAD]\n\
         Strength=200\n\
         Primary=105mm\n\
         PreventAttackMove=yes\n\
         [SECONLY]\n\
         Strength=200\n\
         Secondary=105mm\n\
         [SREF]\n\
         Strength=200\n\
         TurretCount=4\n\
         WeaponCount=1\n\
         Weapon1=105mm\n\
         [ORCA]\n\
         Strength=200\n\
         Primary=105mm\n\
         [GAWEAP]\n\
         Strength=1000\n\
         Primary=105mm\n\
         [WeaponTypes]\n\
         0=105mm\n\
         [105mm]\n\
         Damage=60\n\
         Range=5\n",
    );
    crate::rules::ruleset::RuleSet::from_ini(&ini).expect("chord rules")
}

/// Insert an entity of an explicit category, so the building and aircraft
/// halves of the type rule can be exercised directly.
fn insert_typed(
    sim: &mut Simulation,
    stable_id: u64,
    type_name: &str,
    category: EntityCategory,
) -> u64 {
    let owner = sim.intern("Americans");
    let type_ref = sim.intern(type_name);
    sim.entities_mut_for_test()
        .insert(GameEntity::new_at_frame_zero_for_test(
            stable_id,
            5,
            5,
            0,
            0,
            owner,
            Health::new(300),
            type_ref,
            category,
            0,
            5,
            true,
        ));
    stable_id
}

/// The retail per-type rule: a real `Primary=` and nothing else.
///
/// `Secondary=` is not consulted, `Primary=none` counts as unarmed, and
/// being a harvester is irrelevant — the armed War Miner accepts the order
/// while the Chrono Miner is refused for having no primary weapon.
#[test]
fn attack_move_eligibility_follows_the_primary_weapon() {
    let rules = chord_rules();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);

    let tank = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .expect("tank");
    let war_miner = sim
        .spawn_object("HARV", "Americans", 6, 5, 0, &rules)
        .expect("armed miner");
    let chrono_miner = sim
        .spawn_object("CMIN", "Americans", 8, 5, 0, &rules)
        .expect("unarmed miner");
    let arty = sim
        .spawn_object("SECONLY", "Americans", 7, 5, 0, &rules)
        .expect("secondary-only unit");

    assert!(entity_can_attack_move(&sim, Some(&rules), tank));
    // An armed harvester is an ordinary member of a defended-expansion group.
    assert!(entity_can_attack_move(&sim, Some(&rules), war_miner));
    // `Primary=none` resolves to no weapon at all.
    assert!(!entity_can_attack_move(&sim, Some(&rules), chrono_miner));
    // A secondary-only type is refused: the rule reads Primary only.
    assert!(!entity_can_attack_move(&sim, Some(&rules), arty));
}

/// GSI-08.02 regression: a `TurretCount>0` type carries its weapon in the
/// `Primary` field even though it authors no `Primary=` key, because
/// `Weapon1=` and `Primary=` are the same storage
/// (`TechnoTypeClass::ReadINI`: cursor `0x007128D6 LEA EDI,[EBP+0xA94]`,
/// base store `0x0071294A MOV [EDI-0x1FC],EAX`, and `0xA94-0x1FC = 0x898`
/// which is the `Primary` field `Can_Attack_Move @ 0x00711E90` reads).
///
/// The group case is the one a player feels: `selection_can_attack_move` is
/// an `.all(..)`, so a single Prism Tank answering "no" used to silently
/// disable the attack-move chord for every other unit selected with it.
#[test]
fn gsi_08_02_turret_count_type_attack_moves_through_weapon_one() {
    let rules = chord_rules();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);

    let prism = sim
        .spawn_object("SREF", "Americans", 7, 6, 0, &rules)
        .expect("TurretCount type");
    let tank = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .expect("tank");

    assert!(entity_can_attack_move(&sim, Some(&rules), prism));
    assert!(selection_can_attack_move(
        &sim,
        Some(&rules),
        &[prism, tank]
    ));
}

/// `TechnoTypeClass::Can_Attack_Move @ 0x00711E90` reads BOTH halves: a real
/// `Primary=` and `PreventAttackMove=` off. The nine stock types that carry
/// the key and are not aircraft — the three Engineers, the Spy, Boris and
/// the four helicopters — all have a real primary, so the weapon half alone
/// let every one of them through.
#[test]
fn gsi_07_34_prevent_attack_move_refuses_an_armed_type() {
    let rules = chord_rules();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);

    let nighthawk = sim
        .spawn_object("SHAD", "Americans", 5, 6, 0, &rules)
        .expect("helicopter carrying PreventAttackMove=yes");
    assert!(!entity_can_attack_move(&sim, Some(&rules), nighthawk));
}

/// Buildings and aircraft answer no unconditionally, whatever they are armed
/// with.
#[test]
fn buildings_and_aircraft_never_attack_move() {
    let rules = chord_rules();
    let mut sim = Simulation::new();

    let factory = insert_typed(&mut sim, 1, "GAWEAP", EntityCategory::Structure);
    let orca = insert_typed(&mut sim, 2, "ORCA", EntityCategory::Aircraft);
    let tank = insert_typed(&mut sim, 3, "MTNK", EntityCategory::Unit);

    assert!(!entity_can_attack_move(&sim, Some(&rules), factory));
    assert!(!entity_can_attack_move(&sim, Some(&rules), orca));
    assert!(entity_can_attack_move(&sim, Some(&rules), tank));
}

/// The chord walks the whole selection — buildings included — and dies on
/// the first member that refuses.
#[test]
fn attack_move_chord_requires_every_selected_object() {
    let rules = chord_rules();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);

    let tank = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .expect("tank");
    let war_miner = sim
        .spawn_object("HARV", "Americans", 6, 5, 0, &rules)
        .expect("armed miner");
    let chrono_miner = sim
        .spawn_object("CMIN", "Americans", 8, 5, 0, &rules)
        .expect("unarmed miner");
    let factory = insert_typed(&mut sim, 900, "GAWEAP", EntityCategory::Structure);

    assert!(selection_can_attack_move(
        &sim,
        Some(&rules),
        &[tank, war_miner]
    ));
    assert!(!selection_can_attack_move(
        &sim,
        Some(&rules),
        &[tank, chrono_miner]
    ));
    // A selected structure kills the chord for the whole group.
    assert!(!selection_can_attack_move(
        &sim,
        Some(&rules),
        &[tank, factory]
    ));
    // An empty selection cannot attack-move either.
    assert!(!selection_can_attack_move(&sim, Some(&rules), &[]));
}

/// A chorded click on an enemy *object* attack-moves with the object as its
/// target, because ClickedMission promotes a committed Attack just as it
/// promotes a committed Move (`0x006FFC09..0x006FFC1E`). The promotion is
/// per object: a member whose type refuses attack-move still commits the
/// plain attack.
#[test]
fn chorded_click_on_an_enemy_object_attack_moves() {
    let Some((retail, _)) = retail_entry_tests::retail() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = retail_entry_tests::arena(rules);
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 0, rules)
        .unwrap();
    let target = sim
        .spawn_object("E1", "Russians", 20, 16, 0, rules)
        .unwrap();
    let helicopter = sim
        .spawn_object("SHAD", "Americans", 16, 20, 0, rules)
        .unwrap();
    let click = |mode, actor| object_click_payload(&sim, rules, mode, actor, target);
    assert_eq!(
        click(OrderMode::AttackMove, tank),
        Some(Command::AttackMove {
            entity_id: tank,
            target: Some(TargetKind::Entity(target)),
            destination: None,
        })
    );
    assert_eq!(
        click(OrderMode::AttackMove, helicopter),
        Some(Command::Attack {
            attacker_id: helicopter,
            target_id: target,
        })
    );
    // Outside the attack-move mode the Attack stays plain, forced or
    // not: the mode's chord test passes before any key read
    // (`0x00731BF0`).
    assert_eq!(
        click(OrderMode::Move, tank),
        Some(Command::Attack {
            attacker_id: tank,
            target_id: target,
        })
    );
}

/// The attack-move promotion keeps a Move click's destination token
/// (`0x006FFDA5..0x006FFDE6`), so an object stays the destination; a
/// receiver whose type refuses attack-move commits the plain Move.
#[test]
fn attack_move_keeps_the_move_click_destination() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=AMCV\n\
             [E1]\nStrength=125\nPrimary=M60\n\
             [AMCV]\nStrength=1000\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    let gi = sim
        .spawn_object("E1", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 9, 5, 0, &rules)
        .unwrap();
    let command = |id, mode, destination| move_click_command(&sim, &rules, mode, id, destination);

    for destination in [TargetKind::Entity(mcv), TargetKind::Cell(12, 5)] {
        assert_eq!(
            command(gi, OrderMode::AttackMove, destination),
            Command::AttackMove {
                entity_id: gi,
                target: None,
                destination: Some(destination),
            }
        );
    }
    assert_eq!(
        command(mcv, OrderMode::AttackMove, TargetKind::Entity(gi)),
        Command::MoveToObject {
            entity_id: mcv,
            target_id: gi,
        }
    );
    assert_eq!(
        command(gi, OrderMode::Move, TargetKind::Cell(12, 5)),
        Command::Move {
            entity_id: gi,
            target_rx: 12,
            target_ry: 5,
        }
    );
}

/// A click on the selected object itself is its self action only while it
/// is the one selected object and the local player's
/// (`TechnoClass::What_Action_OnObject`, `0x007000B9..0x007000DF`); a click
/// on one of several selected objects takes the tail instead
/// (`a_self_click_in_a_larger_selection_dispatches_no_move`).
#[test]
fn self_click_needs_the_one_selected_own_object() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=AMCV\n\
             [BuildingTypes]\n0=GACNST\n\
             [E1]\nStrength=125\nDeployFire=yes\nDeployer=yes\n\
             [AMCV]\nStrength=1000\nDeploysInto=GACNST\n\
             [GACNST]\nStrength=1000\n[Clear]\nBuildable=yes\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    // Ground the MCV's yard may stand on (Unit What_Action's placement
    // test, `0x0073FF9C`).
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for house in ["Americans", "Soviets"] {
        let id = sim.intern(house);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, false, 0, 10,
            ));
    }
    let americans = sim.interner().get("Americans").unwrap();
    let mut spawn =
        |kind: &str, owner: &str, rx: u16| sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap();
    let gi = spawn("E1", "Americans", 5);
    let other_gi = spawn("E1", "Americans", 7);
    let mcv = spawn("AMCV", "Americans", 9);
    let enemy_mcv = spawn("AMCV", "Soviets", 20);
    let hover = |stable_id| HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id,
    };
    let click = |selected: &[u64], target: u64| {
        self_click_command(
            &sim,
            &rules,
            None,
            americans,
            selected,
            Some(&hover(target)),
            false,
        )
    };

    assert_eq!(
        click(&[gi], gi),
        Some(Command::ToggleInfantryDeploy { entity_id: gi })
    );
    assert_eq!(
        click(&[mcv], mcv),
        Some(Command::DeployMcv { entity_id: mcv })
    );
    assert_eq!(
        click(&[gi, other_gi], gi),
        None,
        "a group click takes the tail"
    );
    assert_eq!(click(&[mcv, gi], mcv), None, "a group click takes the tail");
    assert_eq!(click(&[gi], other_gi), None, "not the selected object");
    assert_eq!(click(&[enemy_mcv], enemy_mcv), None, "not the player's");
}

/// Building447210 keeps Action4 for a lone selected own nonprimary
/// factory. Click443410 then queues TargetEvent17 for that same building;
/// its receiver toggles Building448160. This covers the input boundary,
/// with existing FindFactory proving the initial primary independently.
#[test]
fn retail_nonprimary_war_factory_self_click_queues_its_primary_selection() {
    use crate::rules::object_type::FactoryType;
    use crate::sim::house_state::HouseState;

    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &battle.rules;
    let factory = rules.object("GAWEAP").expect("stock Allied War Factory");
    assert_eq!(factory.factory_type(), Some(FactoryType::UnitType));
    assert!(!factory.naval && !factory.invisible_in_game);
    let mut sim = Simulation::with_seed(0x0044_8160);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    for name in ["Americans", "Russians"] {
        let owner = sim.intern(name);
        let side = rules
            .country_side_index(name)
            .expect("stock country Side")
            .0;
        let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
        house.project_country_mults(rules, sim.interner());
        sim.houses_mut_for_test().register(house);
    }
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    let first = sim
        .spawn_object("GAWEAP", "Americans", 6, 6, 0, rules)
        .expect("first own retail War Factory");
    let second = sim
        .spawn_object("GAWEAP", "Americans", 20, 6, 0, rules)
        .expect("second own retail War Factory");
    sim.spawn_object("GAWEAP", "Russians", 20, 20, 0, rules)
        .expect("foreign owner's initial primary War Factory");
    let foreign = sim
        .spawn_object("GAWEAP", "Russians", 6, 20, 0, rules)
        .expect("foreign owner's nonprimary War Factory");
    let product = sim
        .spawn_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, rules)
        .expect("actual retail tank for the existing producer query");
    assert_eq!(
        crate::sim::production::object_find_factory(&sim, rules, product, false, false),
        Some(first),
        "automatic initialization must preserve the first primary; without one FindFactory returns the last eligible building"
    );
    let hover = |stable_id| HoverTargetKindWithId {
        kind: if stable_id == foreign {
            HoverTargetKind::EnemyStructure
        } else {
            HoverTargetKind::FriendlyStructure
        },
        stable_id,
    };
    let click = |selected: &[u64], target| {
        self_click_command(
            &sim,
            rules,
            None,
            owner,
            selected,
            Some(&hover(target)),
            false,
        )
    };
    assert_eq!(click(&[first], first), None, "already primary is Action0");
    assert_eq!(
        click(&[first, second], second),
        None,
        "a group takes the tail"
    );
    assert_eq!(click(&[first], second), None, "not the selected object");
    assert_eq!(
        click(&[foreign], foreign),
        None,
        "not the player's building"
    );
    let command = click(&[second], second)
        .expect("native nonprimary GAWEAP self-click queues its target-bearing Event17");
    assert_eq!(command, Command::TogglePrimaryFactory { entity_id: second });
    assert_eq!(
        order_voice_key(&command),
        None,
        "Event17 has no input voice"
    );

    use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
    use crate::app::types::{CursorFeedbackKind, CursorId};

    let cursor = |selected: &[u64], target| {
        capability_cursor_for_hover(
            &sim,
            selected,
            selected.first().copied(),
            &hover(target),
            Some(rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
            &mut Default::default(),
        )
    };
    let nonprimary_cursor = cursor(&[second], second);
    assert_eq!(nonprimary_cursor, CursorFeedbackKind::Deploy);
    assert_eq!(
        cursor_id_for_feedback(nonprimary_cursor),
        Some(CursorId::Deploy),
        "native main-map Action4 uses the existing row27"
    );
    let primary_cursor = cursor(&[first], first);
    assert_eq!(primary_cursor, CursorFeedbackKind::Default);
    assert_eq!(
        cursor_id_for_feedback(primary_cursor),
        Some(CursorId::Default),
        "already-primary Action0 uses row0"
    );
    assert_eq!(
        cursor_id_for_feedback(cursor(&[first], second)),
        Some(CursorId::Select),
        "the hovered factory must be the selected object"
    );
    assert_ne!(
        cursor(&[foreign], foreign),
        CursorFeedbackKind::Deploy,
        "a foreign nonprimary must not offer the local self action"
    );
    // DetermineAction692610 asks the best object alone; with two
    // selected, Techno's self rung (0x007000C1) is closed and its tail
    // answers 0 over a selected object (0x007005BF), which Building
    // keeps (0x0044750D).
    for id in [first, second] {
        sim.entity_mut_for_test(id)
            .unwrap()
            .seldom
            .selection
            .set_selected_for_test(true);
    }
    let group = capability_cursor_for_hover(
        &sim,
        &[first, second],
        Some(first),
        &hover(second),
        Some(rules),
        None,
        crate::app::input::context_order::OrderModifier::Normal,
        false,
        false,
        &mut Default::default(),
    );
    assert_eq!(
        cursor_id_for_feedback(group),
        Some(CursorId::Default),
        "a group hover over a selected factory is the arrow"
    );
}

#[test]
fn a_loaded_bio_reactor_self_click_and_hover_cursor_offer_unload() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[BuildingTypes]\n0=YAPOWR\n[YAPOWR]\nStrength=1000\n\
             InfantryAbsorb=yes\nPassengers=5\n",
        ))
        .expect("Bio Reactor input rules");
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let reactor = sim
        .spawn_object_limbo_at_height("YAPOWR", "Americans", 10, 10, 0, 0, &rules)
        .expect("Bio Reactor builds");
    let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0.0);
    cargo.board_forced_chain(&[(99, 1.0)]);
    sim.entity_mut_for_test(reactor)
        .unwrap()
        .set_passenger_role_for_test(crate::sim::passenger::PassengerRole::transport(cargo));
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyStructure,
        stable_id: reactor,
    };

    let command = self_click_command(&sim, &rules, None, owner, &[reactor], Some(&hover), false);
    let cursor = crate::app::input::cursor::capability_cursor_for_hover(
        &sim,
        &[reactor],
        Some(reactor),
        &hover,
        Some(&rules),
        None,
        crate::app::input::context_order::OrderModifier::Normal,
        false,
        false,
        &mut Default::default(),
    );

    assert_eq!(
        command,
        Some(Command::UnloadPassengers {
            transport_id: reactor
        })
    );
    assert_eq!(cursor, crate::app::types::CursorFeedbackKind::Deploy);
}

/// Unit What_Action asks CanDeploySlashUnload of a loaded transport's
/// self action (`0x0073FFEA`); on a bridge it refuses and the cursor
/// answers NoDeploy (`0x0073FFF4`).
#[test]
fn a_loaded_transport_on_a_bridge_offers_no_deploy() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=IFV\n[IFV]\nStrength=200\nPassengers=1\n",
        ))
        .expect("transport input rules");
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let ifv = sim
        .spawn_object_limbo_at_height("IFV", "Americans", 10, 10, 0, 0, &rules)
        .expect("transport builds");
    let mut cargo = crate::sim::passenger::PassengerCargo::new(1, 0.0);
    cargo.board_forced_chain(&[(99, 1.0)]);
    sim.entity_mut_for_test(ifv)
        .unwrap()
        .set_passenger_role_for_test(crate::sim::passenger::PassengerRole::transport(cargo));
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: ifv,
    };
    let probe = |sim: &Simulation| {
        (
            self_click_command(sim, &rules, None, owner, &[ifv], Some(&hover), false),
            crate::app::input::cursor::capability_cursor_for_hover(
                sim,
                &[ifv],
                Some(ifv),
                &hover,
                Some(&rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
                &mut Default::default(),
            ),
        )
    };
    assert_eq!(
        probe(&sim),
        (
            Some(Command::UnloadPassengers { transport_id: ifv }),
            crate::app::types::CursorFeedbackKind::Deploy
        )
    );
    sim.entity_mut_for_test(ifv).unwrap().set_on_bridge(true);
    assert_eq!(
        probe(&sim),
        (None, crate::app::types::CursorFeedbackKind::NoDeploy)
    );
}

/// Retail SCHP has IsSimpleDeployer without DeploysInto/Deployer. Native
/// Unit WhatAction74000B and CanDeploy700E81 still offer its deploy action,
/// including while the deploy animation owns the visible body.
#[test]
fn retail_siege_chopper_self_click_and_cursor_offer_the_same_deploy_action() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &battle.rules;
    let object = rules.object("SCHP").expect("retail Siege Chopper");
    assert!(object.is_simple_deployer);
    assert!(object.deploys_into.is_none());
    assert!(!object.deployer);
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_limbo_at_height("SCHP", "Americans", 10, 10, 0, 0, rules)
        .expect("retail Siege Chopper builds");
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: id,
    };
    for (deployed, begin, reverse) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, false, true),
    ] {
        sim.entity_mut_for_test(id)
            .unwrap()
            .set_unit_simple_deploy_for_test(deployed, begin, reverse);
        assert_eq!(
            self_click_command(&sim, rules, None, owner, &[id], Some(&hover), false),
            Some(Command::DeployMcv { entity_id: id }),
        );
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[id],
                Some(id),
                &hover,
                Some(rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
                &mut Default::default(),
            ),
            crate::app::types::CursorFeedbackKind::Deploy,
        );
    }
    assert_eq!(
        self_click_command(&sim, rules, None, owner, &[id, id + 1], Some(&hover), false),
        None,
        "a group self-click remains selection"
    );
    sim.entity_mut_for_test(id).unwrap().low_bridge_tube_state = Some(
        crate::sim::movement::tube_movement::LowBridgeTubeMovementState::for_test(
            crate::map::tube_facts::TubeId(0),
            0,
            crate::sim::components::DriveCoord::cell(10, 10, 0),
        ),
    );
    assert_eq!(
        self_click_command(&sim, rules, None, owner, &[id], Some(&hover), false),
        None
    );
    let feedback = crate::app::input::cursor::capability_cursor_for_hover(
        &sim,
        &[id],
        Some(id),
        &hover,
        Some(rules),
        None,
        crate::app::input::context_order::OrderModifier::Normal,
        false,
        false,
        &mut Default::default(),
    );
    assert_eq!(feedback, crate::app::types::CursorFeedbackKind::NoDeploy);
    assert_eq!(
        crate::app::input::cursor::cursor_id_for_feedback(feedback),
        Some(crate::app::types::CursorId::NoDeploy),
        "native action30 must display the blocked deploy cursor"
    );
}

/// A deploying or deployed receiver takes no Move click on a Cell,
/// whatever its locomotor, so no Event clears its target. Retail SCHP is a
/// JumpJet IsSimpleDeployer Unit: over an open Cell, Unit7404B0 makes Move
/// action0 while 6E0, 6E1 or 6E2 is set (740709..740727). A retail E1
/// Deployer in Doing28 answers NoMove (51F8ED..51F94E), which its Cell
/// click refuses (51F25D..51F285). Over an object their What_Action turns
/// Move into NoMove only for 6E0 or Doing27..30 (`0x0074037A`,
/// `0x0051EB81`), and the object receiver 4D74E0 still sends NoMove, as a
/// Cell near the object. A non-player House orders neither: its Cell
/// What_Action answers 0, and 4D74E0 asks IsControllable (`0x004D77FF`).
#[test]
fn retail_deployment_answers_move_clicks_through_the_class_what_action() {
    use crate::app::input::commands::{object_move_destination, ordinary_cell_move_goal};
    use crate::sim::world::MoveAction;
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &battle.rules;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    // Successful Unlimbo now publishes sight immediately; establish the
    // viewer-plane dimensions before creating any sources.
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    let chopper = sim
        .spawn_object("SCHP", "Americans", 10, 10, 0, rules)
        .expect("retail Siege Chopper spawns");
    let gi = sim
        .spawn_object("E1", "Americans", 12, 10, 0, rules)
        .expect("retail GI spawns");
    let tank = sim
        .spawn_object("MTNK", "Americans", 14, 10, 0, rules)
        .expect("retail tank spawns");
    let owner = sim.interner().get("Americans").unwrap();
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    let goals = |sim: &Simulation, id| {
        (
            ordinary_cell_move_goal(sim, rules, owner, id, (14, 12), false),
            object_move_destination(sim, rules, owner, id, tank, MoveAction::Move, false),
        )
    };
    let moves = Some((14, 12));
    let onto_tank = Some(TargetKind::Entity(tank));
    assert_eq!(goals(&sim, chopper), (moves, onto_tank));
    assert_eq!(goals(&sim, gi), (moves, onto_tank));
    for (deployed, begin, reverse) in [
        (false, true, false),
        (true, false, false),
        (true, false, true),
    ] {
        sim.entity_mut_for_test(chopper)
            .unwrap()
            .set_unit_simple_deploy_for_test(deployed, begin, reverse);
        let (on_cell, over_object) = goals(&sim, chopper);
        let state = format!("6E0={deployed} 6E1={begin} 6E2={reverse}");
        assert_eq!(on_cell, None, "{state}");
        if deployed {
            assert!(
                matches!(over_object, Some(TargetKind::Cell(..))),
                "{state}: {over_object:?}"
            );
        } else {
            assert_eq!(over_object, onto_tank, "{state}");
        }
    }
    assert!(rules.object("E1").unwrap().deployer);
    sim.entity_mut_for_test(gi)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(28)
        .unwrap();
    let (on_cell, over_object) = goals(&sim, gi);
    assert_eq!(on_cell, None);
    assert!(
        matches!(over_object, Some(TargetKind::Cell(..))),
        "{over_object:?}"
    );
    sim.entity_mut_for_test(gi)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(0)
        .unwrap();
    sim.session_mut_for_test().set_current_house_for_test(None);
    assert_eq!(goals(&sim, gi), (None, None));
}

/// The click side of the bomb actions: an Engineer's DisarmBomb and a
/// Crazy Ivan's IvanBomb on a friend are Attack orders. An Ivan never
/// attacks a bombed target, nor a
/// friend his shot cannot hit (the rung's ILLEGAL test), where the cursor
/// shows Select too.
#[test]
fn bomb_click_orders() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=IVAN\n1=ENGINEER\n[VehicleTypes]\n0=HTNK\n1=HVY\n\
             [CombatDamage]\nIvanTimedDelay=450\n\
             [IVAN]\nStrength=125\nIvan=yes\nAttackCursorOnFriendlies=yes\n\
             Primary=IvanBomber\n\
             [ENGINEER]\nStrength=75\nEngineer=yes\nBombSight=4\n\
             [HTNK]\nStrength=900\n[HVY]\nStrength=900\nArmor=heavy\n\
             [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
             [Invisible]\nInviso=yes\n\
             [IvanBomb]\nIvanBomb=yes\n\
             Verses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    for house in ["Americans", "Soviets"] {
        let id = sim.intern(house);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, false, 0, 10,
            ));
    }
    let americans = sim.interner().get("Americans").unwrap();
    // The rung is a human player's (`0x007003ED`).
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(americans));
    let mut spawn =
        |kind: &str, owner: &str, rx: u16| sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap();
    let ivan = spawn("IVAN", "Americans", 5);
    let engineer = spawn("ENGINEER", "Americans", 7);
    let own_tank = spawn("HTNK", "Americans", 9);
    let spare_tank = spawn("HTNK", "Americans", 11);
    let armoured = spawn("HVY", "Americans", 13);
    let enemy_ivan = spawn("IVAN", "Soviets", 20);
    let far_tank = spawn("HTNK", "Soviets", 22);
    let selected = [ivan, engineer];

    assert_eq!(
        friendly_ivans(
            &sim,
            &rules,
            &selected,
            selected.len(),
            own_tank,
            false,
            None
        ),
        vec![(ivan, FriendlyIvanAction::Bomb)]
    );
    // 0% Verses: ILLEGAL to the Ivan, so the friend stays a selection
    // click, as the cursor shows it.
    assert!(
        friendly_ivans(
            &sim,
            &rules,
            &selected,
            selected.len(),
            armoured,
            false,
            None
        )
        .is_empty()
    );
    {
        let cursor = |target| {
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[ivan],
                Some(ivan),
                &HoverTargetKindWithId {
                    kind: HoverTargetKind::FriendlyUnit,
                    stable_id: target,
                },
                Some(&rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
                false,
                &mut Default::default(),
            )
        };
        assert_eq!(
            cursor(armoured),
            crate::app::types::CursorFeedbackKind::FriendlyUnit
        );
        assert_eq!(
            cursor(spare_tank),
            crate::app::types::CursorFeedbackKind::IvanBomb
        );
    }
    assert!(disarm_bomb_engineers(&sim, &rules, &selected, own_tank, americans).is_empty());
    assert!(!ivan_cannot_bomb(&sim, &rules, ivan, own_tank));

    sim.bomb_attach_for_test(ivan, Some(own_tank), &rules);
    sim.bomb_attach_for_test(enemy_ivan, Some(far_tank), &rules);
    sim.bomb_list_update_for_test(&rules);
    sim.bomb_list_update_for_test(&rules);

    assert_eq!(
        disarm_bomb_engineers(&sim, &rules, &selected, own_tank, americans),
        vec![engineer]
    );
    assert!(
        disarm_bomb_engineers(&sim, &rules, &selected, far_tank, americans).is_empty(),
        "an unseen bomb"
    );
    assert!(
        friendly_ivans(
            &sim,
            &rules,
            &selected,
            selected.len(),
            own_tank,
            false,
            None
        )
        .is_empty(),
        "a bombed friend stays a selection click"
    );
    assert_eq!(
        friendly_ivans(
            &sim,
            &rules,
            &selected,
            selected.len(),
            spare_tank,
            false,
            None
        ),
        vec![(ivan, FriendlyIvanAction::Bomb)]
    );
    assert!(ivan_cannot_bomb(&sim, &rules, ivan, own_tank));
    assert!(!ivan_cannot_bomb(&sim, &rules, engineer, own_tank));

    // The Engineer carrying a bomb itself is no target of its own click.
    sim.bomb_attach_for_test(ivan, Some(engineer), &rules);
    sim.bomb_list_update_for_test(&rules);
    sim.bomb_list_update_for_test(&rules);
    assert!(disarm_bomb_engineers(&sim, &rules, &[engineer], engineer, americans).is_empty());
}
/// Supplies the ordinary object-route prerequisites, through production
/// object constructors and the existing terrain/type authorities.
fn engineer_hut_click_fixture(
    relation: &str,
    collapsed: bool,
    repairable: bool,
    capturable: bool,
    extra_hut_flags: &str,
    extra_actor_flags: &str,
) -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    u64,
    HoverTargetKindWithId,
) {
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[InfantryTypes]\n0=ENGINEER\n[VehicleTypes]\n0=TRUCK\n[BuildingTypes]\n0=CABHUT\n\
             [TRUCK]\nStrength=100\nSpeed=4\n\
             [ENGINEER]\nStrength=75\nEngineer=yes\n{}\n\
             [CABHUT]\nStrength=200\nFoundation=1x1\nBridgeRepairHut=yes\nRepairable={}\nCapturable={}\n{}\n",
            extra_actor_flags,
            if repairable { "yes" } else { "no" },
            if capturable { "yes" } else { "no" },
            extra_hut_flags,
        )),
    ).unwrap();
    let mut sim = Simulation::with_seed(0x587410);
    sim.resolve_type_handles(&rules);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let house = Some(sim.intern("Americans"));
    sim.session_mut_for_test().set_current_house_for_test(house);
    let mut terrain = crate::map::resolved_terrain::test_grid(20, 20, |rx, ry| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        }
    });
    terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
    sim.install_resolved_terrain_for_new_map(terrain);
    for y in 9..=11 {
        sim.overlay_cell_mut_for_test(12, y).overlay_id = Some(if collapsed { 0xE7 } else { 0xD4 });
    }
    *sim.bridge_state_mut_for_test() =
        Some(crate::sim::bridge_state::BridgeRuntimeState::default());
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let owner = if relation == "self" {
        "Americans"
    } else {
        "Soviets"
    };
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 8, 10, 0, &rules)
        .unwrap();
    let hut = sim
        .spawn_object("CABHUT", owner, 10, 10, 0, &rules)
        .unwrap();
    sim.entity_mut_for_test(hut)
        .unwrap()
        .health
        .set_for_test(150);
    if relation == "allied" {
        sim.house_alliances_mut_for_test()
            .entry("AMERICANS".into())
            .or_default()
            .insert("SOVIETS".into());
    }
    let kind = if relation == "hostile" {
        HoverTargetKind::EnemyStructure
    } else {
        HoverTargetKind::FriendlyStructure
    };
    (
        sim,
        rules,
        engineer,
        HoverTargetKindWithId {
            kind,
            stable_id: hut,
        },
    )
}

/// Original object-click51F190 maps action29 to Capture with Destination
/// hut; action32 consumes the click without queueing. Owner friendship is
/// absent from the admitted hut arm, including damaged/capturable traps.
#[test]
fn engineer_hut_context_orders_match_native_terminal_action_decisions() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
    ))
    .unwrap();
    for relation in ["hostile", "allied", "self"] {
        for collapsed in [false, true] {
            let (sim, rules, engineer, hover) =
                engineer_hut_click_fixture(relation, collapsed, true, true, "", "");
            let before = sim.state_hash();
            let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
            let rng = (
                sim.scenario_rng().logical_state(),
                sim.main_rng().logical_state(),
                sim.mapgen_rng().logical_state(),
            );
            let commands = engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover)
                .expect("an admitted hut arm always consumes the click, including false query");
            let row = native["object_clicks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| {
                    row["input"]["name"]
                        == format!("{relation}_{}", if collapsed { "True" } else { "False" })
                        && row["input"]["emp_blocked"] == false
                })
                .unwrap();
            assert_eq!(
                commands.len() as u64,
                row["output"]["queue_calls"].as_u64().unwrap()
            );
            let expected = if collapsed {
                vec![Command::CaptureBuilding {
                    engineer_id: engineer,
                    target_building_id: hover.stable_id,
                }]
            } else {
                vec![]
            };
            assert_eq!(
                commands, expected,
                "relation={relation} collapsed={collapsed}"
            );
            assert_eq!(
                sim.state_hash(),
                before,
                "input producer must not mutate gameplay"
            );
            assert_eq!(
                serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                actor
            );
            assert_eq!(
                (
                    sim.scenario_rng().logical_state(),
                    sim.main_rng().logical_state(),
                    sim.mapgen_rng().logical_state()
                ),
                rng
            );
        }
    }
}

/// The native object route checks Repairable before entering the hut
/// branch. Native repairable_false establishes exclusion/query_calls=0;
/// its later ordinary WhatAction continuation is outside those goldens.
#[test]
fn nonrepairable_noncapturable_hut_does_not_produce_hut_capture_order() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
    ))
    .unwrap();
    let row = native["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["route"] == "object" && row["input"]["name"] == "repairable_false")
        .unwrap();
    assert_eq!(row["output"]["query_calls"], 0);
    for relation in ["hostile", "allied", "self"] {
        let (sim, rules, engineer, hover) =
            engineer_hut_click_fixture(relation, true, false, false, "", "");
        let before = sim.state_hash();
        let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
        let rng = (
            sim.scenario_rng().logical_state(),
            sim.main_rng().logical_state(),
            sim.mapgen_rng().logical_state(),
        );
        assert_eq!(
            sim.engineer_building_action(engineer, hover.stable_id, &rules),
            None
        );
        assert_eq!(
            engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover),
            None
        );
        assert_eq!(sim.state_hash(), before);
        assert_eq!(
            serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
            actor
        );
        assert_eq!(
            (
                sim.scenario_rng().logical_state(),
                sim.main_rng().logical_state(),
                sim.mapgen_rng().logical_state()
            ),
            rng
        );
    }
}
/// Executed local_false and undeploy_True_foundation_0 object rows stop
/// before the hut query. Both assertions are hut-arm exclusion, without
/// claiming the later ordinary WhatAction continuation's final action.
#[test]
fn engineer_hut_object_prerequisites_exclude_nonlocal_and_undeploy_targets() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
    ))
    .unwrap();
    for gate in ["nonlocal", "undeploy"] {
        let flags = if gate == "undeploy" {
            "UndeploysInto=TRUCK"
        } else {
            ""
        };
        let (mut sim, rules, engineer, hover) =
            engineer_hut_click_fixture("hostile", true, true, false, flags, "");
        if gate == "nonlocal" {
            let house = Some(sim.intern("Soviets"));
            sim.session_mut_for_test().set_current_house_for_test(house);
        } else {
            assert!(rules.object("CABHUT").unwrap().is_1x1_with_undeploy());
        }
        if gate == "nonlocal" {
            let row = native["actions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| {
                    row["input"]["route"] == "object" && row["input"]["name"] == "local_false"
                })
                .unwrap();
            assert_eq!(row["output"]["query_calls"], 0);
        }
        if gate == "undeploy" {
            let row = native["undeploy_controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| {
                    row["input"]["route"] == "object"
                        && row["input"]["name"] == "undeploy_True_foundation_0"
                })
                .unwrap();
            assert_eq!(row["output"]["query_calls"], 0);
        }
        let before = sim.state_hash();
        let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
        let rng = (
            sim.scenario_rng().logical_state(),
            sim.main_rng().logical_state(),
            sim.mapgen_rng().logical_state(),
        );
        assert_eq!(
            sim.engineer_building_action(engineer, hover.stable_id, &rules),
            None,
            "gate={gate}"
        );
        assert_eq!(
            engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover),
            None,
            "gate={gate}"
        );
        assert_eq!(sim.state_hash(), before);
        assert_eq!(
            serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
            actor
        );
        assert_eq!(
            (
                sim.scenario_rng().logical_state(),
                sim.main_rng().logical_state(),
                sim.mapgen_rng().logical_state()
            ),
            rng
        );
    }
}
/// Original700C40 and its object-click caller are independently executed
/// in each paired golden group. These adapters supply represented owner
/// state; they do not assert that retail constructs missile-spawn Engineers
/// or imports arbitrary native EMP/Robot latch bytes.
#[test]
fn represented_infantry_controllability_gates_match_native_hut_clicks() {
    use crate::sim::spawn_manager::{SpawnManagerState, SpawnSlot, SpawnSlotState};
    use crate::sim::timer::CdTimer;
    let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
    ))
    .unwrap();
    let mut compared = 0;
    let mut unrepresented = Vec::new();
    for row in packet["controllability_controls"].as_array().unwrap() {
        let input = &row["native"]["input"];
        let name = input["name"].as_str().unwrap();
        if matches!(
            name,
            "robot_native_save_control" | "emp_native_save_positive" | "emp_negative"
        ) {
            unrepresented.push(name);
            continue;
        }
        let gates = &input["gates"];
        let spawned = gates["spawned"].as_bool().unwrap_or(false);
        let capacity = gates["spawns_number"].as_i64().unwrap_or(1);
        let missile_child = gates["spawn_slots"]
            .as_array()
            .is_some_and(|slots| slots.iter().any(|slot| slot["missile_spawn"] == true));
        let actor_flags = format!(
            "Spawned={}\nSpawnsNumber={capacity}\nMissileSpawn={}\n",
            if spawned { "yes" } else { "no" },
            if missile_child { "yes" } else { "no" }
        );
        let (mut sim, rules, engineer, hover) =
            engineer_hut_click_fixture("hostile", true, true, false, "", &actor_flags);
        sim.session_mut_for_test()
            .set_binary_frame_for_test(gates["frame"].as_u64().unwrap_or(0) as u32);
        if gates["bunker"] == true {
            // Typed existing owner state: positive native pointer control,
            // not an assertion of ordinary Infantry bunker installation.
            sim.entity_mut_for_test(engineer)
                .unwrap()
                .bunker
                .set_bunker_link_for_test(crate::sim::docking::bunker_link::BunkerLink::Installed(
                    hover.stable_id,
                ));
        }
        if gates["paralysis_start"].is_number() {
            sim.entity_mut_for_test(engineer)
                .unwrap()
                .parasite
                .set_paralysis_for_test(CdTimer::from_raw(
                    gates["paralysis_start"].as_i64().unwrap() as i32,
                    gates["paralysis_duration"].as_i64().unwrap() as i32,
                ));
        }
        if gates["warp_out"] == true {
            sim.entity_mut_for_test(engineer)
                .unwrap()
                .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(
                    hover.stable_id,
                ));
        }
        if gates["warp_in"] == true {
            sim.entity_mut_for_test(engineer)
                .unwrap()
                .install_teleport_state_for_test(Some(
                    crate::sim::movement::teleport_movement::TeleportState::for_test(
                        crate::sim::movement::teleport_movement::TeleportPhase::ChronoDelay,
                        8,
                        10,
                        1,
                    ),
                ));
        }
        if gates["slave_owner"] == true {
            sim.entity_mut_for_test(engineer).unwrap().seldom.slave =
                crate::sim::slave_manager::SlaveLink::for_test(Some(hover.stable_id), vec![]);
        }
        if let Some(native_slots) = gates["spawn_slots"].as_array() {
            let mut slots = Vec::new();
            for (index, native_slot) in native_slots.iter().enumerate() {
                let child = if native_slot["state"] == 2 && native_slot["child"] != false {
                    let child = sim
                        .spawn_object("ENGINEER", "Americans", 5 + index as u16, 12, 0, &rules)
                        .unwrap();
                    sim.entity_mut_for_test(child)
                        .unwrap()
                        .lifecycle
                        .set_in_limbo_for_test(native_slot["limbo"] == true);
                    Some(child)
                } else {
                    None
                };
                slots.push(SpawnSlot::for_test(
                    child,
                    match native_slot["state"].as_i64().unwrap() {
                        0 => SpawnSlotState::ReadyDocked,
                        1 => SpawnSlotState::KamikazeWait,
                        2 => SpawnSlotState::InFlight,
                        other => panic!("unrepresented native slot state {other}"),
                    },
                    CdTimer::default(),
                    false,
                ));
            }
            let spawn_type = sim.intern("ENGINEER");
            sim.entity_mut_for_test(engineer).unwrap().spawn_manager =
                Some(Box::new(SpawnManagerState::for_test(spawn_type, slots)));
        }
        let before = sim.state_hash();
        let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
        let rng = (
            sim.scenario_rng().logical_state(),
            sim.main_rng().logical_state(),
            sim.mapgen_rng().logical_state(),
        );
        assert_eq!(
            sim.techno_player_controllable(engineer, &rules),
            row["native"]["output"]["returned_al"] == 1,
            "{name}"
        );
        assert_eq!(
            sim.engineer_building_action(engineer, hover.stable_id, &rules),
            Some(crate::sim::world::EngineerBuildingAction::Repair(true)),
            "control admission must not change WhatAction: {name}"
        );
        let mut selected = vec![engineer];
        let commands = engineer_capture_orders(&sim, &rules, &mut selected, &hover)
            .expect("hut arm consumes blocked and allowed clicks");
        assert_eq!(
            commands.len() as u64,
            row["object_click"]["output"]["queue_calls"]
                .as_u64()
                .unwrap(),
            "{name}"
        );
        assert!(
            selected.is_empty(),
            "handled Engineer must not reach a later input branch: {name}"
        );
        if !commands.is_empty() {
            assert_eq!(
                commands,
                [Command::CaptureBuilding {
                    engineer_id: engineer,
                    target_building_id: hover.stable_id
                }],
                "{name}"
            );
        }
        assert_eq!(sim.state_hash(), before, "{name}");
        assert_eq!(
            serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
            actor,
            "{name}"
        );
        assert_eq!(
            (
                sim.scenario_rng().logical_state(),
                sim.main_rng().logical_state(),
                sim.mapgen_rng().logical_state()
            ),
            rng,
            "{name}"
        );
        compared += 1;
    }
    assert_eq!(compared, 21);
    assert_eq!(
        unrepresented,
        [
            "robot_native_save_control",
            "emp_native_save_positive",
            "emp_negative"
        ]
    );
}

/// A hut's terminal action consumes each handled Engineer, while other
/// selected actors continue through their own original action routes.
#[test]
fn engineer_hut_context_keeps_unhandled_actors_in_selection_order() {
    for collapsed in [false, true] {
        let (mut sim, rules, engineer, hover) =
            engineer_hut_click_fixture("hostile", collapsed, true, true, "", "C4=yes");
        // Unit Unlimbo requires configured playfield bounds; the hut
        // action fixture otherwise only constructs Infantry/Buildings.
        *sim.playfield_bounds_mut_for_test() = Some(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        });
        let first = sim
            .spawn_object("TRUCK", "Americans", 4, 10, 0, &rules)
            .unwrap();
        let last = sim
            .spawn_object("TRUCK", "Americans", 6, 10, 0, &rules)
            .unwrap();
        let second_engineer = sim
            .spawn_object("ENGINEER", "Americans", 7, 10, 0, &rules)
            .unwrap();
        let mut selected = vec![first, engineer, last, second_engineer];
        let before = sim.state_hash();
        let commands = engineer_capture_orders(&sim, &rules, &mut selected, &hover).unwrap();
        assert_eq!(selected, [first, last]);
        let expected = if collapsed {
            vec![
                Command::CaptureBuilding {
                    engineer_id: engineer,
                    target_building_id: hover.stable_id,
                },
                Command::CaptureBuilding {
                    engineer_id: second_engineer,
                    target_building_id: hover.stable_id,
                },
            ]
        } else {
            vec![]
        };
        assert_eq!(commands, expected);
        assert_eq!(sim.state_hash(), before);
        let before_selected = selected.clone();
        assert_eq!(
            engineer_capture_orders(&sim, &rules, &mut selected, &hover),
            None
        );
        assert_eq!(
            selected, before_selected,
            "non-Engineers retain their next action route"
        );
    }
}
