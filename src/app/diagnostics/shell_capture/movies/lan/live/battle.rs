//! Real input automation for a bounded Battle opening. This observer never
//! mutates the simulation, fabricates a peer command, or advances a frame.

use super::*;
use crate::app::input::{commands, dispatch};
use crate::sim::production::ProductionCategory;
use crate::sim::world::Simulation;
use crate::ui::sidebar::SidebarAction;
use winit::event::ElementState;
use winit::keyboard::Key;

#[derive(Debug, Default)]
enum Step {
    #[default]
    Select,
    Deploy,
    Yard,
    Started,
    Holding,
    Held {
        frame: u32,
        progress: u16,
    },
    Resumed {
        progress: u16,
    },
    CancelHold,
    Cancelling,
    Completing,
    Placing,
    Done,
}

#[derive(Default)]
pub(super) struct BattleOpening {
    step: Step,
    mcv: Option<u64>,
    yard: Option<u64>,
    first_product: Option<u64>,
    placed_product: Option<u64>,
    starting_credits: Option<i32>,
    final_credits: Option<i32>,
}

struct FactorySnapshot {
    id: Option<u64>,
    progress: u16,
    running: bool,
    complete: bool,
}

impl BattleOpening {
    pub(super) fn phase(&self) -> String {
        format!("{:?}", self.step)
    }

    pub(super) fn step(
        &mut self,
        state: &mut AppState,
        frame: u32,
        host: bool,
        actions: &mut Vec<Value>,
    ) -> Result<()> {
        let mut action = None;
        match self.step {
            Step::Select if frame >= 10 => {
                let (id, _) = live_type(state, "AMCV")?.context("local retail AMCV")?;
                self.mcv = Some(id);
                click_entity(state, id)?;
                action = Some("select local AMCV with tactical mouse");
                self.step = Step::Deploy;
            }
            Step::Deploy => {
                let id = self.mcv.context("selected MCV")?;
                if simulation(state)?
                    .entities()
                    .get(id)
                    .is_some_and(|e| e.selection.is_selected())
                {
                    if host {
                        chat_key(state, KeyCode::KeyD, &Key::Character("d".into()), Some("d"));
                        action = Some("D key: native DEPLOY09");
                    } else {
                        click_entity(state, id)?;
                        action = Some("AMCV self-click: native MegaMission Unload16");
                    }
                    self.step = Step::Yard;
                }
            }
            Step::Yard => {
                if let Some((id, _)) = live_type(state, "GACNST")?
                    && !simulation(state)?
                        .entities()
                        .get(id)
                        .context("yard")?
                        .in_construction_bstate()
                {
                    self.yard = Some(id);
                    self.starting_credits = Some(credits(state)?);
                    cameo(state, false);
                    action = Some("GAPOWR cameo: Produce");
                    self.step = Step::Started;
                }
            }
            Step::Started => {
                if let Some(factory) = factory(state)?
                    && factory.running
                    && factory.progress >= 2
                {
                    self.first_product = factory.id;
                    cameo(state, true);
                    action = Some("running GAPOWR right-click: Suspend");
                    self.step = Step::Holding;
                }
            }
            Step::Holding => {
                if let Some(factory) = factory(state)?
                    && !factory.running
                {
                    ensure!(
                        factory.id == self.first_product,
                        "hold replaced the paid product"
                    );
                    self.step = Step::Held {
                        frame,
                        progress: factory.progress,
                    };
                    action = Some("observed held Factory; begin 30-frame stability check");
                }
            }
            Step::Held {
                frame: held_at,
                progress,
            } => {
                let factory = factory(state)?.context("held Factory")?;
                ensure!(
                    factory.progress == progress && factory.id == self.first_product,
                    "held Factory changed progress or identity"
                );
                if frame >= held_at + 30 {
                    cameo(state, false);
                    action = Some("held GAPOWR left-click: resume Produce");
                    self.step = Step::Resumed { progress };
                }
            }
            Step::Resumed { progress } => {
                if let Some(factory) = factory(state)?
                    && factory.progress > progress
                {
                    ensure!(
                        factory.id == self.first_product,
                        "resume replaced the paid product"
                    );
                    cameo(state, true);
                    action = Some("resumed Factory advanced; Suspend before cancellation");
                    self.step = Step::CancelHold;
                }
            }
            Step::CancelHold => {
                if let Some(factory) = factory(state)?
                    && !factory.running
                {
                    cameo(state, true);
                    action = Some("held GAPOWR right-click: Abandon");
                    self.step = Step::Cancelling;
                }
            }
            Step::Cancelling => {
                if factory(state)?.is_none_or(|factory| factory.id.is_none()) {
                    ensure!(
                        Some(credits(state)?) == self.starting_credits,
                        "Abandon did not refund the paid production cost"
                    );
                    cameo(state, false);
                    action = Some("observed full refund; restart GAPOWR");
                    self.step = Step::Completing;
                }
            }
            Step::Completing => {
                if let Some(factory) = factory(state)?
                    && factory.complete
                {
                    ensure!(
                        factory.id != self.first_product,
                        "cancelled product was reused"
                    );
                    self.placed_product = factory.id;
                    cameo(state, false);
                    ensure!(
                        state
                            .match_state
                            .input
                            .building_placement_issuer()
                            .is_some(),
                        "completed cameo did not enter manual placement"
                    );
                    let cursor = placement_cursor(state, self.yard.context("yard")?)?;
                    commands::place_ready_building_at_cell(state, "GAPOWR", cursor);
                    action = Some("complete GAPOWR cameo and tactical PLACE");
                    self.step = Step::Placing;
                }
            }
            Step::Placing => {
                if let Some((id, _)) = live_type(state, "GAPOWR")?
                    && !simulation(state)?
                        .entities()
                        .get(id)
                        .context("power plant")?
                        .in_construction_bstate()
                {
                    ensure!(
                        Some(id) == self.placed_product,
                        "PLACE replaced the paid product identity"
                    );
                    self.final_credits = Some(credits(state)?);
                    self.step = Step::Done;
                    action = Some("observed same paid GAPOWR placed and construction completed");
                }
            }
            _ => {}
        }
        if let Some(action) = action {
            actions.push(json!({"native_frame":frame,"input":action,
                "credits":credits(state)?,"factory":factory(state)?.map(|f| json!({
                    "id":f.id,"progress":f.progress,"running":f.running,"complete":f.complete}))}));
        }
        Ok(())
    }

    pub(super) fn finished(&self, state: &AppState) -> Result<bool> {
        let sim = simulation(state)?;
        Ok(matches!(self.step, Step::Done)
            && sim
                .entities()
                .values_sorted()
                .filter(|entity| {
                    entity.is_alive()
                        && !entity.in_limbo()
                        && !entity.in_construction_bstate()
                        && sim.interner().resolve(entity.type_ref()) == "GAPOWR"
                })
                .count()
                == 2)
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        let sim = simulation(state)?;
        Ok(json!({"phase":self.phase(),"mcv":self.mcv,"yard":self.yard,
            "cancelled_product":self.first_product,"placed_product":self.placed_product,
            "starting_credits":self.starting_credits,"final_credits":self.final_credits,
            "buildings":sim.entities().values_sorted().filter(|entity|
                entity.is_alive() && !entity.in_limbo() && entity.category() == crate::map::entities::EntityCategory::Structure
            ).map(|entity| json!({"id":entity.stable_id(),"owner":sim.interner().resolve(entity.owner()),
                "type":sim.interner().resolve(entity.type_ref()),"position":entity.position})).collect::<Vec<_>>(),
            "houses":sim.houses().order().iter().filter_map(|owner| sim.houses().get(owner).map(|house|
                json!({"owner":sim.interner().resolve(*owner),"credits":house.economy().credits(),
                    "power":sim.power_states().get(owner).map(|power| (power.total_output(),power.total_drain()))})
            )).collect::<Vec<_>>() }))
    }
}

fn simulation(state: &AppState) -> Result<&Simulation> {
    Ok(&state
        .match_state
        .sim_runtime
        .as_ref()
        .context("live simulation")?
        .simulation)
}

fn credits(state: &AppState) -> Result<i32> {
    let sim = simulation(state)?;
    let owner = sim
        .interner()
        .get(&local_owner(state)?)
        .context("local House")?;
    Ok(sim
        .houses()
        .get(&owner)
        .context("local House")?
        .economy()
        .credits())
}

fn factory(state: &AppState) -> Result<Option<FactorySnapshot>> {
    let sim = simulation(state)?;
    let owner = sim
        .interner()
        .get(&local_owner(state)?)
        .context("local House")?;
    Ok(sim
        .production()
        .factories()
        .view(owner, ProductionCategory::Building)
        .map(|factory| FactorySnapshot {
            id: factory.object.and_then(|object| object.entity_id),
            progress: factory.progress,
            running: factory.is_running(),
            complete: factory.complete_object().is_some(),
        }))
}

fn live_type(state: &AppState, type_id: &str) -> Result<Option<(u64, (u16, u16))>> {
    let sim = simulation(state)?;
    let owner = sim
        .interner()
        .get(&local_owner(state)?)
        .context("local House")?;
    Ok(sim
        .entities()
        .values_sorted()
        .find(|entity| {
            entity.owner() == owner
                && entity.is_alive()
                && !entity.in_limbo()
                && sim.interner().resolve(entity.type_ref()) == type_id
        })
        .map(|entity| {
            (
                entity.stable_id(),
                (entity.position.rx(), entity.position.ry()),
            )
        }))
}

fn cameo(state: &mut AppState, right: bool) {
    dispatch::apply_sidebar_action(
        state,
        SidebarAction::CameoPress {
            type_id: "GAPOWR".into(),
            right,
            shift: false,
            at_build_limit: false,
        },
    );
}

fn click_entity(state: &mut AppState, id: u64) -> Result<()> {
    let entity = simulation(state)?
        .entities()
        .get(id)
        .context("mouse actor")?;
    let cell = (entity.position.rx(), entity.position.ry());
    let world = crate::render::locomotor_visual::screen_position(entity);
    crate::app::input::camera::center_camera_on_cell(state, cell.0, cell.1);
    let input = &mut state.match_state.input;
    input.cursor_x = (world.0 - input.camera_x) * input.zoom_level;
    input.cursor_y = (world.1 - input.camera_y) * input.zoom_level;
    dispatch::handle_cursor_moved_in_game(state);
    for edge in [ElementState::Pressed, ElementState::Released] {
        dispatch::handle_mouse_input(state, MouseButton::Left, edge);
    }
    Ok(())
}

fn placement_cursor(state: &AppState, yard: u64) -> Result<(u16, u16)> {
    let sim = simulation(state)?;
    let rules = state.rules().context("loaded rules")?;
    let yard = sim.entities().get(yard).context("yard")?;
    let owner = local_owner(state)?;
    let object = rules.object("GAPOWR").context("retail GAPOWR")?;
    for dy in -8..=8 {
        for dx in -8..=8 {
            let x = i32::from(yard.position.rx()) + dx;
            let y = i32::from(yard.position.ry()) + dy;
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let origin =
                crate::rules::foundation::placement_origin_at_cursor(&object.foundation, (x, y));
            let preview = crate::sim::production::placement_preview_for_owner_with_overlays(
                sim,
                rules,
                &owner,
                "GAPOWR",
                origin.0,
                origin.1,
                state.overlay_registry(),
            );
            if preview.is_some_and(|preview| {
                preview.valid && preview.in_build_area && preview.foundation_visible
            }) {
                return Ok((x, y));
            }
        }
    }
    anyhow::bail!("no legal GAPOWR placement near deployed yard")
}
