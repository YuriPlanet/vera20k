//! UpdateBunker459315 clears Group once, before its ordinary Deselect.
//! Physical retail types/ART/SHP; arena, visibility and window-free input
//! boundaries are supplied. No full UI or native-playback comparison.
use super::*;
use crate::app::input::context_order::{OrderModifier, entry_action, self_click_command};
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::app::input::hotkeys::HotkeyBindings;
use crate::app::input::state::{MatchInputState, PreparedSavedLocalView};
use crate::app::types::HoverTargetKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::command::CommandEnvelope;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{EntryClickAction, FrameEffects, TickLane};
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
use winit::keyboard::{Key, KeyLocation, ModifiersState};

struct Scene {
    runtime: SimRuntime,
    input: MatchInputState,
    bunker: u64,
    tank: u64,
    peers: [u64; 2],
}

impl Scene {
    fn new() -> Option<Self> {
        let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let walls = [
            "NATBNK_A",
            "NATBNK_B",
            "NATBNK_AD",
            "NATBNK_BD",
            "NATBNK_A2",
            "NATBNK_B2",
            "NATBNK_A2D",
            "NATBNK_B2D",
        ]
        .map(str::to_owned);
        assert_eq!(
            retail
                .rules
                .bind_anim_class_assets(&walls, &assets, theater.extension, "TEMPERATE",),
            0
        );
        let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
        retail
            .rules
            .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
        retail
            .rules
            .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
        let bindings = HotkeyBindings::from_ini_bytes(assets.get_archive_ref("KEYBOARDMD.INI"));
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        assert!(rules.object("LTNK").unwrap().bunkerable);
        assert!(rules.object("NATBNK").unwrap().bunker);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x459315);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.set_input_delay_ticks(0);
        let owner = sim.interner().get("Americans").unwrap();
        let rival = sim.interner().get("Russians").unwrap();
        sim.houses_mut_for_test()
            .get_mut(&rival)
            .unwrap()
            .set_is_human_for_test(true);
        sim.houses_mut_for_test()
            .get_mut(&rival)
            .unwrap()
            .set_current_iq_for_test(0);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = false;
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
        for (house, x, y) in [("Americans", 24, 24), ("Russians", 28, 28)] {
            sim.spawn_object_with_overlay_registry("GAPOWR", house, x, y, 0, &rules, &registry)
                .unwrap();
        }
        let bunker = sim
            .spawn_object_with_overlay_registry("NATBNK", "Americans", 10, 10, 0, &rules, &registry)
            .unwrap();
        let tank = sim
            .spawn_object_with_overlay_registry("LTNK", "Americans", 4, 10, 0, &rules, &registry)
            .unwrap();
        let peers = [
            sim.spawn_object("TRUCKA", "Americans", 4, 14, 0, &rules)
                .unwrap(),
            sim.spawn_object("TRUCKA", "Americans", 6, 14, 0, &rules)
                .unwrap(),
        ];
        let terrain_template = sim.resolved_terrain().cloned();
        Some(Self {
            runtime: SimRuntime {
                simulation: sim,
                resources: SimResources {
                    rules,
                    overlay_registry: registry,
                    terrain_template,
                },
            },
            input: MatchInputState::new(bindings),
            bunker,
            tank,
            peers,
        })
    }

    fn frame(&mut self) {
        let commands = self.runtime.simulation.take_due_commands();
        let output = self
            .runtime
            .advance_frame(&commands, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        reconcile_selection_order_for_sim(
            &mut self.input,
            &self.runtime.simulation,
            Some(&self.runtime.resources.rules),
            &output.lifecycle_outputs,
        );
    }

    fn select(&mut self, ids: Vec<u64>) {
        // Existing selection receiver; this fixture supplies the window's
        // command-delivery boundary, without constructing GPU AppState.
        assert!(self.runtime.apply_command_for_test(
            "Americans",
            &Command::Select {
                entity_ids: ids,
                additive: false,
            }
        ));
        reconcile_selection_order_for_sim(
            &mut self.input,
            &self.runtime.simulation,
            Some(&self.runtime.resources.rules),
            &[],
        );
    }

    fn key(&mut self, digit: &str, ctrl: bool) {
        let modifiers = if ctrl {
            ModifiersState::CONTROL
        } else {
            ModifiersState::empty()
        };
        let resolution = self.input.hotkey_bindings.resolve_event(
            &Key::Character(digit.into()),
            KeyLocation::Standard,
            modifiers,
        );
        let (slot, expected) = match resolution {
            HotkeyResolution::Command(HotkeyCommand::TeamCreate(slot)) if ctrl => {
                (slot, GroupPressAction::Assign)
            }
            HotkeyResolution::Command(HotkeyCommand::TeamSelect(slot)) if !ctrl => {
                (slot, GroupPressAction::Recall)
            }
            other => panic!("physical digit binding: {other:?}"),
        };
        let sim = &self.runtime.simulation;
        let selected = selected_stable_ids_in_order(
            Some(sim),
            Some(&self.runtime.resources.rules),
            &self.input.selection_order,
            false,
        );
        assert_eq!(
            control_group_action_for_input(
                KeyModifiers {
                    ctrl,
                    ..Default::default()
                },
                slot,
                &self.input.control_groups[slot],
                &selected,
                None,
                Some(sim),
            ),
            Some(expected)
        );
        if ctrl {
            reassign_control_group(&mut self.input.control_groups, slot, &selected, sim);
        } else {
            self.select(live_group_members(
                Some(sim),
                &self.input.control_groups[slot],
            ));
        }
    }

    fn queue(&mut self, command: Command) {
        let sim = &mut self.runtime.simulation;
        let owner = sim.interner().get("Americans").unwrap();
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session().tick() + 1,
            command,
        ));
    }

    fn saved_view_round_trip(&mut self) {
        let saved = self
            .input
            .capture_saved_local_view(
                &self.runtime.simulation,
                &self.runtime.resources.rules,
                &SidebarGadgetState::new(),
            )
            .unwrap();
        let bytes = crate::util::save_codec::serialize(&saved).unwrap();
        let saved = crate::util::save_codec::deserialize(&bytes).unwrap();
        let prepared = PreparedSavedLocalView::new(
            saved,
            &self.runtime.simulation,
            &self.runtime.resources.rules,
        )
        .unwrap();
        let mut restored = MatchInputState::new(self.input.hotkey_bindings.clone());
        prepared.commit_into(&mut restored, &mut SidebarGadgetState::new());
        self.input = restored;
    }
}

#[test]
fn stock_bunker_install_clears_old_group_once_and_preserves_later_assignment() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let [first, last] = scene.peers;
    scene.select(vec![first, scene.tank, last]);
    scene.key("1", true);
    // ObjectSelect5F461F prepends a positive-damage Primary, while these
    // unarmed peers append. Group assignment preserves that actual order.
    assert_eq!(scene.input.control_groups[1], [scene.tank, first, last]);
    scene.select(vec![scene.tank]);
    assert_eq!(
        entry_action(
            &scene.runtime.simulation,
            &scene.runtime.resources.rules,
            scene.tank,
            scene.bunker,
            OrderModifier::Normal,
            false,
            Some(&scene.runtime.resources.overlay_registry),
        ),
        Some(EntryClickAction::Enter)
    );
    scene.queue(Command::clicked_mission(
        scene.tank,
        crate::sim::mission::MissionType::Enter,
        crate::sim::combat::TargetKind::Entity(scene.bunker),
    ));
    let mut contacted = false;
    for _ in 0..600 {
        scene.frame();
        let sim = &scene.runtime.simulation;
        let unit = sim.entities().get(scene.tank).unwrap();
        contacted |= unit.radio_contacts.slot(0) == Some(scene.bunker);
        if unit.bunker.link().installed_in() == Some(scene.bunker) {
            break;
        }
        assert_eq!(scene.input.control_groups[1], [scene.tank, first, last]);
    }
    let sim = &scene.runtime.simulation;
    assert!(contacted);
    assert_eq!(
        sim.entities().get(scene.bunker).unwrap().bunker.occupant(),
        Some(scene.tank)
    );
    assert_eq!(
        sim.entities()
            .get(scene.tank)
            .unwrap()
            .bunker
            .link()
            .installed_in(),
        Some(scene.bunker)
    );
    assert_eq!(
        scene.input.control_groups[1],
        [first, last],
        "install clears only its tank"
    );
    scene.key("1", false);
    assert_eq!(scene.input.selection_order, [first, last]);
    assert!(
        !scene
            .runtime
            .simulation
            .entities()
            .get(scene.tank)
            .unwrap()
            .selection
            .is_selected()
    );
    scene.saved_view_round_trip();
    assert_eq!(scene.input.control_groups[1], [first, last]);

    // The retained installed tank is still selectable. Use the real picker,
    // then the same selection receiver and Ctrl+digit owners as above.
    let sim = &scene.runtime.simulation;
    let (x, y) =
        crate::render::locomotor_visual::screen_position(sim.entities().get(scene.tank).unwrap());
    let mutation = compute_click_selection_snapshot_with_playfield(
        sim,
        &map_entity_creation_order(sim.entities()),
        &scene.input.selection_order,
        Some(sim.fog()),
        Some("Americans"),
        x,
        y,
        false,
        Some(&scene.runtime.resources.rules),
        Some(sim.interner()),
        true,
        sim.resolved_terrain(),
    )
    .expect("ordinary click on installed tank");
    assert!(mutation.clear);
    assert_eq!(mutation.select, [scene.tank]);
    scene.select(mutation.select);
    scene.key("2", true);
    for _ in 0..3 {
        scene.frame();
    }
    assert_eq!(
        scene.input.control_groups[2],
        [scene.tank],
        "occupied visits must not repeat install clear"
    );
    scene.key("2", false);
    assert_eq!(scene.input.selection_order, [scene.tank]);
    let sim = &scene.runtime.simulation;
    let release = self_click_command(
        sim,
        &scene.runtime.resources.rules,
        Some(&scene.runtime.resources.overlay_registry),
        sim.interner().get("Americans").unwrap(),
        &[scene.tank],
        Some(&HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: scene.tank,
        }),
        false,
    )
    .expect("ordinary installed-tank self click offers DEPLOY");
    assert!(matches!(release, Command::DeployMcv { entity_id } if entity_id == scene.tank));
    scene.queue(release);
    for _ in 0..120 {
        scene.frame();
        if scene
            .runtime
            .simulation
            .entities()
            .get(scene.tank)
            .unwrap()
            .bunker
            .link()
            .installed_in()
            .is_none()
        {
            break;
        }
    }
    assert_eq!(
        scene
            .runtime
            .simulation
            .entities()
            .get(scene.bunker)
            .unwrap()
            .bunker
            .occupant(),
        None
    );
    assert!(
        scene
            .runtime
            .simulation
            .entities()
            .get(scene.tank)
            .unwrap()
            .is_object_alive()
    );
    scene.saved_view_round_trip();
    assert_eq!(scene.input.control_groups[1], [first, last]);
    assert_eq!(scene.input.control_groups[2], [scene.tank]);
    scene.key("2", false);
    assert_eq!(scene.input.selection_order, [scene.tank]);
}
