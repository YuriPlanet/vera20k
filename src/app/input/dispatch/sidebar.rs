//! Sidebar input: wheel notches, build-strip scrolling, cameo presses and
//! sidebar actions, plus the local production-queue and EVA helpers they use.

use super::{
    centre_placement_on_factory, is_alt_held, is_ctrl_held, open_pause_menu, press_super_weapon,
    queue_deploy_undeploy_for_selected, queue_selection_snapshot_command,
};
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::input::commands::{
    cancel_build_by_type, preferred_command_owner, queue_build_by_type, schedule_command,
    suspend_build,
};
use crate::app::input::sidebar_eva;
use crate::app::match_audio::MatchAudioState;
use crate::app::match_runtime::sim_host::SimHost;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::sidebar_render::current_sidebar_view;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::map::entities::EntityCategory;
use crate::sim::command::Command;
use crate::ui::game_screen::GameScreen;
use crate::ui::sidebar::SidebarAction;

/// The sidebar command one wheel notch resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WheelAction {
    /// Scroll the active build strip up one row.
    SidebarUp,
    /// Scroll the active build strip down one row.
    SidebarDown,
}

/// Resolve a wheel notch to its sidebar command.
///
/// gamemd's window procedure intercepts every wheel message and executes the
/// command named `SidebarDown` when the delta is negative and `SidebarUp`
/// otherwise — a zero delta goes up, because the test is a signed less-than.
/// Magnitude is not a multiplier: one message is one command is one row.
pub(crate) fn wheel_action(delta_lines: f32) -> WheelAction {
    if delta_lines < 0.0 {
        WheelAction::SidebarDown
    } else {
        WheelAction::SidebarUp
    }
}

/// The sidebar command a Winit wheel event issues, if any.
///
/// Winit also forwards touchpad phase boundaries and horizontal-only events.
/// Their zero vertical delta is no wheel notch, so it must not reach the
/// native sign mapping (which would interpret zero as SidebarUp).
/// winit 0.30.13, macOS View::scroll_wheel:
/// https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/macos/view.rs#L663-L701
pub(crate) fn wheel_notch(delta_lines: f32) -> Option<WheelAction> {
    (delta_lines != 0.0).then(|| wheel_action(delta_lines))
}

/// SidebarUp / SidebarDown (the wheel and their hotkeys): one row on the
/// active build strip.
///
/// SidebarClass::Scroll `0x006A6A00` queues the row in the strip's
/// PendingScroll; StripClass::AI moves it on the next Sidebar Action, like
/// the scroll arrows. The cursor position is deliberately not consulted. The
/// retail binding is a window message routed straight to a command, not a
/// hit-tested gadget, so the wheel scrolls the sidebar from anywhere on the
/// screen — and there is no world zoom in gamemd for the wheel to reach instead.
pub(crate) fn sidebar_scroll_command(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
    action: WheelAction,
) {
    if crate::app::input::campaign_input::is_locked(match_state) {
        return;
    }
    let Some(visible_rows) = current_sidebar_view(&match_state.match_presentation)
        .map(|view| view.layout.side2_tile_count)
    else {
        return;
    };
    // VERA: over a grid of build pictures the side's layout added, the wheel
    // scrolls that grid, a row a notch; anywhere else it scrolls the
    // sidebar's own strip as the original does.
    let (x, y) = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        screen,
        &match_state.match_presentation.sidebar_layout_spec,
    )
    .point_to_layout(match_state.input.cursor_x, match_state.input.cursor_y);
    let over = current_sidebar_view(&match_state.match_presentation).and_then(|view| {
        view.added_grids
            .iter()
            .find(|grid| grid.rect.contains(x, y))
            .copied()
    });
    if let Some(grid) = over {
        let scroll = &mut match_state.match_presentation.added_grid_scroll;
        if scroll.len() <= grid.part {
            scroll.resize(grid.part + 1, 0);
        }
        scroll[grid.part] = if action == WheelAction::SidebarDown {
            (grid.scroll_rows + 1).min(grid.max_scroll_rows)
        } else {
            grid.scroll_rows.saturating_sub(1)
        };
        return;
    }
    let presentation = &mut match_state.match_presentation;
    presentation.sidebar_projection.model.queue_scroll(
        presentation.active_sidebar_tab,
        action == WheelAction::SidebarDown,
        visible_rows,
        1,
    );
}

/// The local player's queue of record, in queue order (empty without a sim).
pub(crate) fn local_queue_view(
    frontend: &FrontendState,
    match_state: &MatchState,
) -> Vec<crate::sim::production::QueueItemView> {
    let Some(owner) = preferred_command_owner(&match_state.sim_runtime) else {
        return Vec::new();
    };
    match (
        match_state.sim_runtime.runtime().map(|rt| &rt.simulation),
        crate::app::state::active_rules(frontend, match_state),
    ) {
        (Some(sim), Some(rules)) => {
            crate::sim::production::queue_view_for_owner(sim, rules, &owner)
        }
        _ => Vec::new(),
    }
}

/// The interned id a type name already has in the sim (never interns).
fn local_interned(sim_runtime: &SimHost, type_id: &str) -> Option<crate::sim::intern::InternedId> {
    sim_runtime
        .runtime()
        .and_then(|rt| rt.simulation.interner().get(type_id))
}

/// Speak an app-local EVA line (`VoxClass::PlayEVA(name, -1)` inside a click
/// handler): the entry's own `Type=`/`Priority=` route it.
pub(crate) fn push_local_eva(match_audio: &mut MatchAudioState, event: &str) {
    match_audio
        .sound_events
        .push(crate::audio::events::GameSoundEvent::Eva {
            event: event.to_string(),
            type_override: None,
        });
}

/// A press on a build cameo: `SelectClass::Action @ 0x006AAD00`'s decision
/// (`sidebar_eva::cameo_click`) against the local player's queue, then its
/// `GUIBuildSound`, EVA line and event.
///
/// `shift` also feeds VERA's own Shift repeat, off unless the player ticks
/// Shift-Click Batch Queue in Options. It is not a native input:
/// gamemd reads the modifier for the right button only (`0x006AAD66`), so an
/// unshifted or right-button press is byte-for-byte the ported decision. The
/// sound and the EVA line are spoken once per click regardless of the repeat.
/// When Ares plays in the game (`RuleSet::ares`) Shift repeats without the
/// option, five copies ([`sidebar_eva::produce_copies`]).
fn sidebar_cameo_press(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    type_id: &str,
    press: sidebar_eva::CameoPress,
    at_build_limit: bool,
    shift: bool,
) {
    let Some(category) = crate::app::state::active_rules(frontend, match_state)
        .and_then(|rules| rules.object(type_id))
        .map(crate::sim::production::category_for_object)
    else {
        return;
    };
    let queue = local_queue_view(frontend, match_state);
    let type_iid = local_interned(&match_state.sim_runtime, type_id);
    let linked_factory = preferred_command_owner(&match_state.sim_runtime).and_then(|owner| {
        let sim = &match_state.sim_runtime.runtime()?.simulation;
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .cameo_factory(sim, sim.interner().get(&owner)?, type_iid?)
            .map(|factory| {
                (
                    factory.cameo_state(),
                    factory
                        .complete_object()
                        .and_then(|object| object.entity_id.map(|held| (object.type_id, held))),
                )
            })
    });
    // The linked Factory's state also decides whether Shift may repeat: a held
    // or finished build takes the resume/placement arm, not a fresh PRODUCE.
    let own_for_repeat = linked_factory.map(|(status, _)| status);
    // The scenario's rules, as the simulation reads them, not the Options
    // box: a change there waits for the next scenario.
    let building_production_queue = crate::app::state::active_rules(frontend, match_state)
        .is_some_and(|rules| rules.general.building_production_queue);
    let click = sidebar_eva::cameo_click_with_queue(
        &queue,
        category,
        type_iid,
        own_for_repeat,
        type_iid.and_then(|type_id| {
            match_state
                .match_presentation
                .sidebar_projection
                .model
                .cameo_request(type_id)
        }),
        press,
        at_build_limit,
        building_production_queue,
    );
    // VERA's own modifiers, all behind the Options page's Shift-Click Batch
    // Queue (`[VERA] ShiftQueue=`): Shift + right cancels a running build and
    // its copies at once; Ctrl + left builds a mobile type endlessly, and a
    // second Ctrl + left or any cancel ends that.
    let vera_keys = persistence.options_profile.shift_queue;
    let click = if vera_keys {
        sidebar_eva::shift_right_cancels_running(click, press)
    } else {
        click
    };
    let ctrl_repeat = vera_keys
        && press == sidebar_eva::CameoPress::Left
        && is_ctrl_held(&match_state.input)
        && super::build_repeat::repeats(category);
    let repeat = &mut match_state.input.build_repeat;
    if ctrl_repeat && repeat.stop(type_id) {
        crate::app::App::play_shell_rules_sound(
            frontend,
            match_state,
            process_assets,
            sfx_player,
            |rules| rules.general.gui_build_sound.as_deref(),
        );
        return;
    }
    match click.order {
        Some(sidebar_eva::CameoOrder::Produce) if ctrl_repeat => {
            repeat.toggle(type_id);
        }
        Some(sidebar_eva::CameoOrder::Abandon { .. }) => {
            repeat.stop(type_id);
        }
        _ if click.then_abandon.is_some() => {
            repeat.stop(type_id);
        }
        _ => {}
    }
    if click.clear_building_placement {
        match_state.input.cancel_building_placement();
    }
    if click.sound {
        crate::app::App::play_shell_rules_sound(
            frontend,
            match_state,
            process_assets,
            sfx_player,
            |rules| rules.general.gui_build_sound.as_deref(),
        );
    }
    if let Some(line) = click.eva {
        push_local_eva(&mut match_state.match_audio, line);
        // Accessibility: `EVA_Building` / `EVA_Training` / `EVA_Canceled`
        // name no object, but the clicked cameo's type key is right here.
        // Other click lines (hold, refusal, target select) post nothing.
        crate::app::input::messages::post_eva_production_start(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            line,
            type_id,
        );
        // A single ABANDON removed exactly this cameo's build. ABANDON_ALL
        // removed a set the voice does not name either, so it names nothing.
        if line == sidebar_eva::EVA_CANCELED && sidebar_eva::cancel_names_clicked_type(click.order)
        {
            crate::app::input::messages::post_eva_cancel(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                sfx_player,
                persistence,
                type_id,
                sidebar_eva::is_structure_strip(category),
            );
        }
    }
    let owner = preferred_command_owner(&match_state.sim_runtime).and_then(|name| {
        match_state
            .sim_runtime
            .runtime()?
            .simulation
            .interner()
            .get(&name)
    });
    if let (Some(runtime), Some(owner), Some(type_id)) =
        (match_state.sim_runtime.runtime(), owner, type_iid)
    {
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .record_cameo_click(
                &runtime.simulation,
                &runtime.resources.rules,
                owner,
                type_id,
                click,
            );
    }
    match click.order {
        None => {}
        Some(sidebar_eva::CameoOrder::Produce) => {
            // The copies carry the current tick, so they execute in one frame in
            // issue order; the sound and EVA above were spoken once. Shift
            // repeats while the Options page's Shift-Click Batch Queue
            // (`[VERA] ShiftQueue=`) is ticked, `[VERA] ShiftQueueCount=` times,
            // and always when Ares plays in the game (`RuleSet::ares`; Ares.dll
            // 10064520, hook 0x6AB773).
            let ares = crate::app::state::active_rules(frontend, match_state)
                .is_some_and(|rules| rules.ares());
            let profile = &persistence.options_profile;
            let repeat = sidebar_eva::produce_copies(
                category,
                own_for_repeat,
                shift,
                profile.shift_queue,
                profile.shift_queue_count,
                building_production_queue,
                ares,
            );
            // VERA's Alt + left click (same option): the new copies jump the
            // waiting builds of other types, through ordinary ABANDON and
            // PRODUCE events in one frame (`sidebar_eva::front_of_queue`).
            // A held or finished build of this type takes the resume arm.
            let appends = !matches!(
                own_for_repeat,
                Some(
                    crate::sim::production::BuildQueueState::Paused
                        | crate::sim::production::BuildQueueState::Done
                )
            );
            let front = (profile.shift_queue
                && press == sidebar_eva::CameoPress::Left
                && is_alt_held(&match_state.input)
                && !sidebar_eva::is_structure_strip(category)
                && appends)
                .then(|| {
                    sidebar_eva::front_of_queue(
                        &queue,
                        category,
                        type_iid?,
                        repeat,
                        crate::app::state::active_rules(frontend, match_state)?
                            .general
                            .maximum_queued_objects,
                    )
                })
                .flatten();
            match front {
                Some((jumped, copies)) => {
                    let jumped: Vec<String> = jumped
                        .into_iter()
                        .filter_map(|id| {
                            let sim = &match_state.sim_runtime.runtime()?.simulation;
                            Some(sim.interner().resolve(id).to_string())
                        })
                        .collect();
                    for queued in &jumped {
                        cancel_build_by_type(
                            platform,
                            renderer,
                            diag,
                            frontend,
                            match_state,
                            process_assets,
                            sfx_player,
                            persistence,
                            queued,
                            false,
                        );
                    }
                    queue_build_by_type(
                        platform,
                        renderer,
                        diag,
                        frontend,
                        match_state,
                        process_assets,
                        sfx_player,
                        persistence,
                        type_id,
                        copies,
                    );
                    for queued in &jumped {
                        queue_build_by_type(
                            platform,
                            renderer,
                            diag,
                            frontend,
                            match_state,
                            process_assets,
                            sfx_player,
                            persistence,
                            queued,
                            1,
                        );
                    }
                }
                None => queue_build_by_type(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    type_id,
                    repeat,
                ),
            }
        }
        Some(sidebar_eva::CameoOrder::Suspend) => {
            suspend_build(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                sfx_player,
                persistence,
                type_id,
            );
            // Select6AB1D4 plays GUIBuildSound again before this ABANDON.
            if let Some(all) = click.then_abandon {
                crate::app::App::play_shell_rules_sound(
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    |rules| rules.general.gui_build_sound.as_deref(),
                );
                cancel_build_by_type(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    type_id,
                    all,
                );
            }
        }
        Some(sidebar_eva::CameoOrder::Abandon { all }) => {
            cancel_build_by_type(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                sfx_player,
                persistence,
                type_id,
                all,
            );
        }
        Some(sidebar_eva::CameoOrder::Place) => {
            // Select6AB30D plays GUIBuildSound before ObjectFindFactory5F5C20
            // (0,0) at6AB31A, including repeated pending clicks. A null builder
            // instead queues ABANDON16 before EVA_UnableToComply6AB3A9.
            let ready_target =
                preferred_command_owner(&match_state.sim_runtime).and_then(|owner| {
                    let runtime = match_state.sim_runtime.runtime()?;
                    let sim = &runtime.simulation;
                    let rules = &runtime.resources.rules;
                    let owner_id = sim.interner().get(&owner)?;
                    // Select6AB2CC reads this linked Factory's actual product;
                    // an older cameo can still refer to a rearmed Factory.
                    let (held_type, held) = linked_factory?.1?;
                    let builder = ready_cameo_factory(sim, rules, held)?;
                    Some((
                        owner_id,
                        held,
                        sim.interner().resolve(held_type).to_string(),
                        sim.entities().get(held)?.category() == EntityCategory::Structure,
                        builder,
                    ))
                });
            let Some((owner_id, held, held_type, is_building, builder)) = ready_target else {
                return;
            };
            let Some(builder) = builder else {
                cancel_build_by_type(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    type_id,
                    false,
                );
                push_local_eva(
                    &mut match_state.match_audio,
                    sidebar_eva::EVA_UNABLE_TO_COMPLY,
                );
                return;
            };
            if !is_building {
                // Select6AB3D9..6AB459 emits PLACE11 using the requested
                // cameo's kind/Naval slot and index -1, without HasChanged.
                // The existing receiver resolves the current complete head.
                // Source: tools/sidebar_oracle/factory_sale_sidebar.md.
                let issued = match_state.sim_runtime.runtime().and_then(|runtime| {
                    let sim = &runtime.simulation;
                    let rules = &runtime.resources.rules;
                    let mut selection = crate::sim::production::ProductionSelection::for_type(
                        rules,
                        rules.object(type_id)?,
                    )?;
                    selection.type_index = -1;
                    //6AB3D9 reads the clicked type's Naval; EBP retains its
                    //type RTTI.6AB3F4 instead takes the House from the product.
                    let product_owner = sim.entities().get(held)?.owner();
                    let record = sim.encode_place_record(product_owner, selection, (0, 0))?;
                    Some((sim.interner().resolve(product_owner).to_owned(), record))
                });
                if let Some((owner, record)) = issued {
                    schedule_command(
                        platform,
                        renderer,
                        diag,
                        frontend,
                        match_state,
                        process_assets,
                        sfx_player,
                        persistence,
                        &owner,
                        Command::SynchronizedRecord { record },
                    );
                }
                return;
            }
            // ManualPlace4FB852 refuses while any building placement is pending.
            if match_state
                .input
                .begin_building_placement(&held_type, held, owner_id)
            {
                centre_placement_on_factory(
                    platform,
                    renderer,
                    frontend,
                    match_state,
                    persistence,
                    builder,
                );
                // ManualPlace4FB881 calls UnselectAll48DC90 only on entry.
                // This existing owner clears immediate selection/follow and
                // schedules the authoritative membership commit. A clear has
                // no additions, so its effects do not depend on cursor mode.
                queue_selection_snapshot_command(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    sfx_player,
                    persistence,
                    Vec::new(),
                    false,
                );
                let gadgets = &mut match_state.match_presentation.sidebar_gadget_state;
                gadgets.disable_cursor_modes();
            }
        }
    }
}

/// The factory a ready cameo's click places `held` from: the product's
/// `ObjectClass::FindFactory(0, 0)` (`0x005F5C20`, called at `0x006AB31A`),
/// or with Ares `HouseExt::FindFactory(owner, type, 0, 1, 0, 1)`
/// (SidebarClass_ProcessCameoClick_Power, Ares.dll `10025170`, hook
/// `0x006AB312`). None where Ares ends the click because only factories
/// without power fit (`0x006AB95A`): no abandon and no EVA follow.
pub(in crate::app::input) fn ready_cameo_factory(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    held: u64,
) -> Option<Option<u64>> {
    use crate::sim::production::AresObjectFactory;
    if !rules.ares_form() {
        return Some(crate::sim::production::object_find_factory(
            sim, rules, held, false, false,
        ));
    }
    match crate::sim::production::ares_object_factory(sim, rules, held) {
        AresObjectFactory::Unpowered => None,
        AresObjectFactory::Missing => Some(None),
        AresObjectFactory::Found(id) => Some(Some(id)),
    }
}

pub(crate) fn apply_sidebar_action(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    action: SidebarAction,
) {
    apply_sidebar_action_inner(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        action,
    );
    super::commands::reconcile_paid_super_weapon_cursor(match_state);
}

fn apply_sidebar_action_inner(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    action: SidebarAction,
) {
    if crate::app::input::campaign_input::is_locked(match_state) {
        return;
    }
    match action {
        SidebarAction::None => {}
        // `RadarClass::AI @ 0x00653850`: input 0x80F3 (OPTBTN) writes
        // in-scenario state 1, the options menu; 0x80F2 (DIPLOBTN) goes
        // through the diplomacy owner's state 8/9 choice.
        SidebarAction::OpenPauseMenu => open_pause_menu(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        ),
        SidebarAction::OpenDiplomacy => crate::app::input::diplomacy::open(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            crate::app::input::diplomacy::DiplomacyRequest::SidebarButton,
        ),
        SidebarAction::SelectTab(tab) => {
            // Retained strips own their rows; changing tabs only selects a strip.
            match_state.match_presentation.active_sidebar_tab = tab;
        }
        SidebarAction::CameoPress {
            type_id,
            right,
            shift,
            at_build_limit,
        } => {
            let press = if right {
                sidebar_eva::CameoPress::Right { shift }
            } else {
                sidebar_eva::CameoPress::Left
            };
            sidebar_cameo_press(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                &mut audio.sfx_player,
                persistence,
                &type_id,
                press,
                at_build_limit,
                shift,
            );
        }
        SidebarAction::SuperWeaponPress { section, right } => {
            press_super_weapon(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                &mut audio.sfx_player,
                persistence,
                &section,
                right,
                true,
            );
        }
        SidebarAction::ToggleRepairMode | SidebarAction::ToggleSellMode => {
            use crate::ui::sidebar::gadget_flash::SelectionMode;
            let mode = if matches!(action, SidebarAction::ToggleRepairMode) {
                SelectionMode::Repair
            } else {
                SelectionMode::Sell
            };
            let has_building = match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| local_has_manageable_building(&runtime.simulation));
            let placement_armed = match_state.input.armed_building_type().is_some();
            if match_state
                .match_presentation
                .sidebar_gadget_state
                .toggle_selection_mode(mode, placement_armed, i32::from(has_building))
            {
                queue_selection_snapshot_command(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    &mut audio.sfx_player,
                    persistence,
                    Vec::new(),
                    false,
                );
            }
        }
        SidebarAction::Deploy => {
            queue_deploy_undeploy_for_selected(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                &mut audio.sfx_player,
                persistence,
            );
        }
    }
}

/// The Repair and Sell modes need a building the issuing seat can manage.
pub(in crate::app::input) fn local_has_manageable_building(
    sim: &crate::sim::world::Simulation,
) -> bool {
    let control = sim.control_projection();
    let tracked_buildings = |owner| {
        sim.houses()
            .get(&owner)
            .is_some_and(|house| house.tracking().buildings() > 0)
    };
    if sim.session().has_custom_control_grants() {
        sim.houses()
            .order()
            .iter()
            .copied()
            .any(|owner| control.can_manage(owner) && tracked_buildings(owner))
    } else {
        control.local_command_house().is_some_and(tracked_buildings)
    }
}
