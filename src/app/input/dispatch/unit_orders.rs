//! Orders issued to the current selection from keys and the command bar:
//! Stop, Guard, area Guard, Scatter and Deploy/Undeploy.

use super::selected_stable_ids_in_order;
use crate::app::PlatformState;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::input::commands::{preferred_command_owner, schedule_command};
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::map::entities::EntityCategory;
use crate::sim::command::Command;
use crate::sim::world::ControlProjection;

/// `Selection__IssueStopCommand @ 0x00730EA0`, reached from the Stop key
/// (`0x00536B80`) and the command bar. Planning refusal at730EF5 ->6FFE00
/// ->639FD0 ends the walk with ScoldSound and no StopSound; it is only
/// reached for a qualifying object.
pub(super) fn queue_stop_for_selected(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    let sim = match_state.sim_runtime.runtime().map(|rt| &rt.simulation);
    let selected_ids = selected_stable_ids_in_order(
        sim,
        crate::app::state::active_rules(frontend, match_state),
        &match_state.input.selection_order,
        match_state.input.selection_order_pending,
    );
    let qualifies = match (sim, crate::app::state::active_rules(frontend, match_state)) {
        (Some(sim), Some(rules)) => {
            let control = sim.control_projection();
            selected_ids
                .iter()
                .any(|&id| stop_qualifies(sim, &control, rules, id))
        }
        _ => false,
    };
    if qualifies && match_state.input.planning.active() {
        crate::app::input::planning_mode::refuse_clicked_event(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            persistence,
            crate::app::input::planning_mode::PlanningRefusal::Stop,
            1,
        );
        return;
    }
    if !super::commands::selection_input_has_capacity(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        selected_ids.len(),
    ) {
        return;
    }
    // VERA ferry: Stop also ends the selected transports' routes.
    crate::app::input::ferry::stop_selected(&mut match_state.input, &selected_ids);
    let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return;
    };
    for sound in queue_stop_orders(
        &mut runtime.simulation,
        &runtime.resources.rules,
        selected_ids,
    ) {
        match_state.match_audio.sound_events.push(sound);
    }
}

/// Whether the Stop walk issues to this object: one it may control (vt+0xA0)
/// or that is active (vt+0x9C, `0x00730ECD..0x00730EE3`), so an offline
/// Robot Tank a click selected stays put. Ares 3.0
/// StopCommandClass_Execute_Berzerk (1005D110, hook 0x730EE5) passes over a
/// berserk one.
fn stop_qualifies(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
) -> bool {
    (sim.local_techno_controllable_with(control, id, rules)
        || sim.local_techno_is_active_with(control, id, rules))
        && !(rules.ares()
            && sim
                .entities()
                .get(id)
                .is_some_and(|entity| entity.berserk.is_active()))
}

/// The Stop walk, then its cue `PlayGlobal(Rules+720, 0x2000, 1.0)`
/// (`0x00730F00..0x00730F17`). An empty selection plays nothing
/// (`0x00730EAB`); the walk's result starts true, so a selection where
/// nothing qualified still plays it. Events are sent as the player's house
/// (`ClickedEvent 0x006FFE00`), as Guard and Scatter do.
pub(super) fn queue_stop_orders(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    mut selected: Vec<u64>,
) -> Vec<crate::audio::events::GameSoundEvent> {
    if selected.is_empty() {
        return Vec::new();
    }
    let control = sim.control_projection();
    let Some(owner) = control.local_command_house() else {
        return Vec::new();
    };
    let owner = sim.interner().resolve(owner).to_owned();
    // A qualifying Stop actor already passed the same tactical check as
    // the ordinary issue gate. Release the read before writing the OutList.
    selected.retain(|&id| stop_qualifies(sim, &control, rules, id));
    for entity_id in selected {
        let payload = Command::Stop { entity_id };
        if super::commands::schedule_command_in_sim(sim, &owner, payload).is_none() {
            // A refused Rust schedule fails closed, as Guard's does.
            return Vec::new();
        }
    }
    rules
        .general
        .stop_sound
        .as_deref()
        .map_or_else(Vec::new, |sound_id| {
            vec![crate::audio::events::GameSoundEvent::UiSound {
                sound_id: rules.sound_name(sound_id),
            }]
        })
}

/// GuardObject536D00 and command-bar slot6 both execute730D60 immediately.
/// The input ledger already contains pending selection order; never sort it
/// or require that the selected bits have reached the simulation yet.
pub(super) fn queue_guard_for_selected(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    if match_state.input.planning.active() {
        crate::app::input::planning_mode::refuse(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            persistence,
            crate::app::input::planning_mode::PlanningRefusal::Guard,
        );
        return;
    }
    let selected = selected_stable_ids_in_order(
        match_state.sim_runtime.runtime().map(|rt| &rt.simulation),
        crate::app::state::active_rules(frontend, match_state),
        &match_state.input.selection_order,
        match_state.input.selection_order_pending,
    );
    if !super::commands::selection_input_has_capacity(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        selected.len(),
    ) {
        return;
    }
    let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return;
    };
    for event in queue_area_guard_orders(
        &mut runtime.simulation,
        &runtime.resources.rules,
        &selected,
        match_state.input.selection_voice_enabled,
    ) {
        match_state.match_audio.sound_events.push(event);
    }
}

/// ScatterCommand536D80 ->730FE0 uses the same pending selection-order
/// owner as Guard. No per-object voice or selection-voice latch is involved.
pub(super) fn queue_scatter_for_selected(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    let selected = selected_stable_ids_in_order(
        match_state.sim_runtime.runtime().map(|rt| &rt.simulation),
        crate::app::state::active_rules(frontend, match_state),
        &match_state.input.selection_order,
        match_state.input.selection_order_pending,
    );
    //730FE0 only reaches6FFE00 for a controllable Techno. Use the
    // existing Scatter producer admission; an empty/refused selection
    // produces neither a HUD row nor an ordinary event.
    if match_state.input.planning.active() {
        let eligible = match_state.sim_runtime.runtime().is_some_and(|runtime| {
            selected.iter().any(|&id| {
                runtime
                    .simulation
                    .scatter_key_command(id, &runtime.resources.rules)
                    .is_some()
            })
        });
        if eligible {
            crate::app::input::planning_mode::refuse_clicked_event(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                persistence,
                crate::app::input::planning_mode::PlanningRefusal::Scatter,
                1,
            );
        }
        return;
    }
    if !super::commands::selection_input_has_capacity(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        selected.len(),
    ) {
        return;
    }
    let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return;
    };
    for sound in queue_scatter_orders(&mut runtime.simulation, &runtime.resources.rules, &selected)
    {
        match_state.match_audio.sound_events.push(sound);
    }
}

pub(super) fn queue_scatter_orders(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
) -> Vec<crate::audio::events::GameSoundEvent> {
    use crate::audio::events::GameSoundEvent;

    if selected.is_empty() {
        return Vec::new();
    }
    let Some(owner) = sim.control_projection().local_command_house() else {
        return Vec::new();
    };
    let owner = sim.interner().resolve(owner).to_owned();
    for &entity_id in selected {
        let Some(payload) = sim.scatter_key_command(entity_id, rules) else {
            continue;
        };
        if super::commands::schedule_rules_command_in_sim(sim, rules, &owner, payload).is_none() {
            return Vec::new();
        }
    }
    // 731032..73104A: one PlayAtPos750920, gain1, center2000, arg0.
    // A nonempty selection that admits no actor still produces this cue.
    // PlanningMode637AA0/639FD0 refusal is admitted by handle_scatter_key.
    // The existing offline ingress bypasses bounded native OutList admission;
    // its saturation and network-pump integration remain outside this path.
    rules
        .general
        .scatter_sound
        .as_deref()
        .map_or_else(Vec::new, |sound_id| {
            vec![GameSoundEvent::UiSound {
                sound_id: rules.sound_name(sound_id),
            }]
        })
}

/// The ordinary Guard selection loop730D79..730E8A. Simulation owns each
/// actor's eligibility/current Cell and the shared MegaMission codec. This
/// caller preserves list order, leaves the voice latch enabled for every
/// actor, and emits GuardSound even if a nonempty selection was all skipped.
/// Planning-mode637AA0/63A0D0 is handled before this ordinary producer.
pub(super) fn queue_area_guard_orders(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    voices_enabled: bool,
) -> Vec<crate::audio::events::GameSoundEvent> {
    use crate::audio::events::GameSoundEvent;

    let mut sounds = Vec::new();
    if selected.is_empty() {
        return sounds;
    }
    let Some(owner) = sim.control_projection().local_command_house() else {
        return sounds;
    };
    let owner = sim.interner().resolve(owner).to_owned();
    for &entity_id in selected {
        let Some(payload) = sim.area_guard_key_command(entity_id, rules) else {
            continue;
        };
        // 6FFCBD..6FFDA5 precedes the event constructor/post. Native normal
        // 6FFBE0 still returns true after a full OutList rejects insertion.
        if let Some(event) = crate::app::match_runtime::sound_dispatch::default_order_voice_event(
            sim,
            rules,
            entity_id,
            voices_enabled,
        ) {
            sounds.push(event);
        }
        if super::commands::schedule_rules_command_in_sim(sim, rules, &owner, payload).is_none() {
            // A missing Rust registration or unrepresentable record fails
            // closed. This invariant boundary is not native issuer false;
            // Planning Mode is the separate native false-return route.
            return sounds;
        }
    }
    if let Some(sound_id) = rules.general.guard_sound.as_deref() {
        // 730E73..730E8A: PlayAtPos750920, centre2000, volume1, no handle.
        sounds.push(GameSoundEvent::UiSound {
            sound_id: rules.sound_name(sound_id),
        });
    }
    sounds
}

/// The deploy key over the selection ([`deploy_key_commands`]).
pub(super) fn queue_deploy_undeploy_for_selected(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    let selected_ids = selected_stable_ids_in_order(
        match_state.sim_runtime.runtime().map(|rt| &rt.simulation),
        crate::app::state::active_rules(frontend, match_state),
        &match_state.input.selection_order,
        match_state.input.selection_order_pending,
    );
    if !super::commands::selection_input_has_capacity(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        selected_ids.len(),
    ) {
        return;
    }
    let Some(sim) = match_state.sim_runtime.runtime().map(|rt| &rt.simulation) else {
        return;
    };
    if selected_ids.is_empty() {
        return;
    }
    // Collect commands first to avoid borrow conflict with schedule_command.
    let Some(owner) = preferred_command_owner(&match_state.sim_runtime) else {
        return;
    };
    let local_owner = sim.interner().get(&owner);
    let commands = deploy_key_commands(
        sim,
        crate::app::state::active_rules(frontend, match_state),
        &selected_ids,
        local_owner,
    );
    // Selection730AF0 sends DEPLOY9 through Techno6FFE00. During
    // Planning Mode that reaches639FD0 and the NoDeploy refusal at638238,
    // rather than executing an immediate deployment or adding a mission.
    if !commands.is_empty() && match_state.input.planning.active() {
        crate::app::input::planning_mode::refuse_clicked_event(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            persistence,
            crate::app::input::planning_mode::PlanningRefusal::Deploy,
            commands.len(),
        );
        return;
    }
    for cmd in commands {
        //730D15 calls ClickedEvent6FFE00 with Event9. Self-click uses the
        //separate MegaMission Unload producer, even for the same AMCV.
        let cmd = if let Command::DeployMcv { entity_id }
        | Command::ToggleInfantryDeploy { entity_id } = cmd
        {
            let Some(record) = match_state.sim_runtime.runtime().and_then(|runtime| {
                let sim = &runtime.simulation;
                sim.encode_deploy_record(sim.interner().get(&owner)?, entity_id)
            }) else {
                continue;
            };
            Command::SynchronizedRecord { record }
        } else {
            cmd
        };
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
            cmd,
        );
    }
}

/// `Selection::IssueDeployCommand @ 0x00730AF0`, over the local player's
/// selection in order.
/// - A first selected building with occupants (`vt+0x408`, `0x004581F0`)
///   unloads them alone.
/// - Otherwise a first walk (`0x00730B8C..0x00730C0E`) notes whether any
///   deployer is deployed and any is not, berserk ones included.
/// - The issue walk then admits only an object the player may control
///   (vt+0xA0) or that is active (vt+0x9C, `0x00730C38`, `0x00730C46`), as
///   the Stop key does, that CanDeploySlashUnload admits (vt+0x314,
///   `0x00730C74`; its vt+0x310 and Techno-flag tests before it refuse
///   nothing more in a selection), skips a berserk object (`0x00730C82`)
///   and, when the first walk found both, every deployed deployer, so only
///   the undeployed ones deploy. A building never passes vt+0x314, so a
///   construction yard does not undeploy from the deploy key.
///
/// Its closing sound reads `Rules+0x72C`, which no INI key sets: the
/// constructor's -1 (`0x0066606A`) plays nothing.
pub(super) fn deploy_key_commands(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selection: &[u64],
    local_owner: Option<crate::sim::intern::InternedId>,
) -> Vec<Command> {
    let control = sim.control_projection();
    let selected = selection.iter().copied().filter(|id| {
        !sim.session().has_custom_control_grants()
            || sim
                .entities()
                .get(*id)
                .is_some_and(|entity| control.can_order(entity.owner()))
    });
    let first = selected
        .clone()
        .next()
        .and_then(|id| sim.entities().get(id));
    if let Some(building) = first.filter(|entity| {
        entity.category() == EntityCategory::Structure
            && entity.passenger_role().occupants().count() > 0
    }) {
        return deploy_key_command_with(sim, &control, rules, building.stable_id(), local_owner)
            .into_iter()
            .collect();
    }
    let deployed = |id: u64| deployer_is_deployed(sim, rules, id);
    let mut found_deployed = false;
    let mut found_undeployed = false;
    for id in selected.clone() {
        match deployed(id) {
            Some(true) => found_deployed = true,
            Some(false) => found_undeployed = true,
            None => continue,
        }
        if found_deployed && found_undeployed {
            break;
        }
    }
    let mixed = found_deployed && found_undeployed;
    selected
        .filter(|&id| {
            rules.is_none_or(|rules| {
                (sim.local_techno_controllable_with(&control, id, rules)
                    || sim.local_techno_is_active_with(&control, id, rules))
                    && sim.can_deploy_slash_unload(id, rules)
            }) && sim
                .entities()
                .get(id)
                .is_some_and(|entity| !entity.berserk.is_active())
                && !(mixed && deployed(id) == Some(true))
        })
        .filter_map(|id| deploy_key_command_with(sim, &control, rules, id, local_owner))
        .collect()
}

/// The deploy walk's deployer state: a `Deployer=` infantryman
/// (`0x005224E0`) is deployed in Doing 27..30 (`0x00522510`); an
/// `IsSimpleDeployer=` Unit is deployed or still deploying (`0x00746D90`:
/// `+0x6E0` or `+0x6E1`). Anything else is no deployer.
fn deployer_is_deployed(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    id: u64,
) -> Option<bool> {
    let entity = sim.entities().get(id)?;
    let object = sim.interner().object_type(entity.type_ref(), rules?)?;
    match entity.category() {
        EntityCategory::Infantry if object.deployer => Some(entity.infantry_deploy_doing()),
        EntityCategory::Unit if object.is_simple_deployer => Some(
            entity
                .mission_leaf
                .as_unit()
                .is_some_and(|leaf| leaf.deployed() != 0 || leaf.deploy_begin_active() != 0),
        ),
        _ => None,
    }
}

/// The deploy key's per-object event for an object it admits: DEPLOY event
/// opcode9 (vt+0x374(9), `0x00730D15`), whose receiver queues Mission Unload16.
/// The historical `DeployMcv` wire name also carries a SimpleDeployer toggle,
/// a transport's empty or loaded order and a tank bunker's deploy out.
/// The walk admits a bunkered unit only through IsActive (vt+0x9C):
/// vt+0xA0 refuses a bunker link, and a box
/// never selects one, but a click does.
#[cfg(test)]
pub(super) fn deploy_key_command(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    entity_id: u64,
    local_owner: Option<crate::sim::intern::InternedId>,
) -> Option<Command> {
    deploy_key_command_with(
        sim,
        &sim.control_projection(),
        rules,
        entity_id,
        local_owner,
    )
}

fn deploy_key_command_with(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    entity_id: u64,
    local_owner: Option<crate::sim::intern::InternedId>,
) -> Option<Command> {
    let entity = sim.entities().get(entity_id)?;
    let local_owner = if sim.session().has_custom_control_grants() {
        if !control.can_order(entity.owner()) {
            return None;
        }
        control.local_command_house()
    } else {
        local_owner
    };
    let obj = rules.and_then(|rules| sim.interner().object_type(entity.type_ref(), rules));
    match entity.category() {
        // Selection730B28..730B5E sends Event9 for an occupied Building.
        // Its native receiver owns NULL setters and Unload; self-click keeps
        // the separate MEGAMISSION/absorber admission in transport_orders.
        EntityCategory::Structure => (!entity.passenger_role().occupants().is_empty()
            && local_owner.is_none_or(|owner| control.can_order_from(entity.owner(), owner)))
        .then_some(Command::DeployMcv { entity_id }),
        EntityCategory::Infantry => Some(Command::ToggleInfantryDeploy { entity_id }),
        // A unit in a tank bunker deploys out of it: its Unload's bunker arm
        // (`0x0073D63B`) runs before the transport arm.
        EntityCategory::Unit if entity.bunker.link().installed_in().is_some() => {
            Some(Command::DeployMcv { entity_id })
        }
        // Unit D emits Event9 without a cargo-count test (`0x00730D15` →
        // `0x006FFE00`); self-click keeps its separate loaded-only admission.
        // Native controls: tools/spatial_oracle/ground_transport_deploy.json.
        EntityCategory::Unit if obj.is_some_and(|object| object.passengers > 0) => {
            Some(Command::DeployMcv { entity_id })
        }
        _ => {
            // Aircraft and other deployers retain their existing routing.
            super::transport_orders::unload_command_with(sim, control, entity, obj, local_owner)
                .or_else(|| {
                    let object = obj?;
                    (super::context_order::is_simple_deploy_unit(entity, object)
                        || object.deploys_into.is_some()
                        || rules.is_some_and(|rules| {
                            crate::sim::deploy::unit_fires_on_deploy(sim, entity, rules)
                        }))
                    .then_some(Command::DeployMcv { entity_id })
                })
        }
    }
}
