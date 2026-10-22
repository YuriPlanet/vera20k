//! Real input automation for a bounded Battle opening. This observer never
//! mutates the simulation, fabricates a peer command, or advances a frame.

use super::*;
use crate::app::input::{commands, dispatch};
use crate::sim::command::{Command, PlaceRecord, ProductionEvent, ProductionRecord};
use crate::sim::production::{ProductionCategory, ProductionSelection};
use crate::sim::world::Simulation;
use crate::ui::sidebar::SidebarAction;
use crate::ui::sidebar::cameo_order::CameoId;
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
    BarracksStarted,
    BarracksCompleting,
    BarracksPlacing,
    GiStarted,
    GiQueued,
    GiProducing,
    GiSettling {
        frame: u32,
    },
    Done,
}

struct GiProduct {
    id: u64,
    first_progress: u16,
    completed_frame: Option<u32>,
    delivered_frame: Option<u32>,
}

#[derive(Default)]
pub(super) struct BattleOpening {
    step: Step,
    mcv: Option<u64>,
    yard: Option<u64>,
    first_product: Option<u64>,
    placed_product: Option<u64>,
    barracks_product: Option<u64>,
    starting_credits: Option<i32>,
    final_credits: Option<i32>,
    gi_wallet: Option<(i32, i32)>,
    gi_cost: Option<i32>,
    gi_factory_order: Option<u64>,
    gi_products: Vec<GiProduct>,
    successor_promotions: u8,
    replay_cursor: usize,
    gi_places: Vec<PlaceRecord>,
    gi_produces: Vec<ProductionRecord>,
    gi_events: Vec<Value>,
}

struct FactorySnapshot {
    id: Option<u64>,
    progress: u16,
    running: bool,
    complete: bool,
    queue_len: usize,
    construction_order: u64,
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
        self.observe_commands(state)?;
        self.observe_infantry(state, frame, actions)?;
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
                    cameo(state, "GAPOWR", false)?;
                    action = Some("GAPOWR cameo: Produce");
                    self.step = Step::Started;
                }
            }
            Step::Started => {
                if let Some(factory) = factory(state, ProductionCategory::Building)?
                    && factory.running
                    && factory.progress >= 2
                {
                    self.first_product = factory.id;
                    cameo(state, "GAPOWR", true)?;
                    action = Some("running GAPOWR right-click: Suspend");
                    self.step = Step::Holding;
                }
            }
            Step::Holding => {
                if let Some(factory) = factory(state, ProductionCategory::Building)?
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
                let factory =
                    factory(state, ProductionCategory::Building)?.context("held Factory")?;
                ensure!(
                    factory.progress == progress && factory.id == self.first_product,
                    "held Factory changed progress or identity"
                );
                if frame >= held_at + 30 {
                    cameo(state, "GAPOWR", false)?;
                    action = Some("held GAPOWR left-click: resume Produce");
                    self.step = Step::Resumed { progress };
                }
            }
            Step::Resumed { progress } => {
                if let Some(factory) = factory(state, ProductionCategory::Building)?
                    && factory.progress > progress
                {
                    ensure!(
                        factory.id == self.first_product,
                        "resume replaced the paid product"
                    );
                    cameo(state, "GAPOWR", true)?;
                    action = Some("resumed Factory advanced; Suspend before cancellation");
                    self.step = Step::CancelHold;
                }
            }
            Step::CancelHold => {
                if let Some(factory) = factory(state, ProductionCategory::Building)?
                    && !factory.running
                {
                    cameo(state, "GAPOWR", true)?;
                    action = Some("held GAPOWR right-click: Abandon");
                    self.step = Step::Cancelling;
                }
            }
            Step::Cancelling => {
                if factory(state, ProductionCategory::Building)?
                    .is_none_or(|factory| factory.id.is_none())
                {
                    ensure!(
                        Some(credits(state)?) == self.starting_credits,
                        "Abandon did not refund the paid production cost"
                    );
                    cameo(state, "GAPOWR", false)?;
                    action = Some("observed full refund; restart GAPOWR");
                    self.step = Step::Completing;
                }
            }
            Step::Completing => {
                if let Some(factory) = factory(state, ProductionCategory::Building)?
                    && factory.complete
                {
                    ensure!(
                        factory.id != self.first_product,
                        "cancelled product was reused"
                    );
                    self.placed_product = factory.id;
                    cameo(state, "GAPOWR", false)?;
                    ensure!(
                        state
                            .match_state
                            .input
                            .building_placement_issuer()
                            .is_some(),
                        "completed cameo did not enter manual placement"
                    );
                    let cursor = placement_cursor(state, self.yard.context("yard")?, "GAPOWR")?;
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
                    cameo(state, "GAPILE", false)?;
                    self.step = Step::BarracksStarted;
                    action = Some("same paid GAPOWR completed; GAPILE cameo: Produce");
                }
            }
            Step::BarracksStarted => {
                if let Some(factory) = factory(state, ProductionCategory::Building)?
                    && let Some(id) = factory.id
                {
                    self.barracks_product = Some(id);
                    self.step = Step::BarracksCompleting;
                    action = Some("observed paid GAPILE held identity");
                }
            }
            Step::BarracksCompleting => {
                if let Some(factory) = factory(state, ProductionCategory::Building)?
                    && factory.complete
                {
                    ensure!(
                        factory.id == self.barracks_product,
                        "GAPILE completion replaced its paid identity"
                    );
                    cameo(state, "GAPILE", false)?;
                    ensure!(
                        state
                            .match_state
                            .input
                            .building_placement_issuer()
                            .is_some(),
                        "ready GAPILE did not enter placement"
                    );
                    let cursor = placement_cursor(state, self.yard.context("yard")?, "GAPILE")?;
                    commands::place_ready_building_at_cell(state, "GAPILE", cursor);
                    self.step = Step::BarracksPlacing;
                    action = Some("ready GAPILE cameo and tactical PLACE");
                }
            }
            Step::BarracksPlacing => {
                if let Some((id, _)) = live_type(state, "GAPILE")?
                    && !simulation(state)?
                        .entities()
                        .get(id)
                        .context("barracks")?
                        .in_construction_bstate()
                {
                    ensure!(
                        Some(id) == self.barracks_product,
                        "GAPILE PLACE replaced its paid identity"
                    );
                    let sim = simulation(state)?;
                    let rules = state.rules().context("retail rules")?;
                    let owner = sim
                        .interner()
                        .get(&local_owner(state)?)
                        .context("local House")?;
                    self.gi_cost =
                        Some(sim.cost_of(owner, rules.object("E1").context("retail E1")?, rules));
                    self.gi_wallet = Some(wallet(state)?);
                    cameo(state, "E1", false)?;
                    self.step = Step::GiStarted;
                    action = Some("same paid GAPILE completed; E1 cameo: Produce");
                }
            }
            Step::GiStarted => {
                if let Some(factory) = factory(state, ProductionCategory::Infantry)?
                    && factory.running
                    && factory.id.is_some()
                {
                    ensure!(
                        self.gi_products.len() == 1 && factory.id == Some(self.gi_products[0].id),
                        "first E1 was not retained before queueing successor"
                    );
                    cameo(state, "E1", false)?;
                    self.step = Step::GiQueued;
                    action = Some("running paid E1 retained; second E1 cameo: Produce");
                }
            }
            Step::GiQueued => {
                if let Some(factory) = factory(state, ProductionCategory::Infantry)?
                    && factory.queue_len != 0
                {
                    ensure!(
                        factory.queue_len == 1 && factory.id == Some(self.gi_products[0].id),
                        "second E1 did not become one unconstructed FIFO tail"
                    );
                    self.step = Step::GiProducing;
                    action = Some("observed exactly one E1 FIFO successor; no ready clicks follow");
                }
            }
            Step::GiProducing => {
                if self.gi_products.len() == 2
                    && self
                        .gi_products
                        .iter()
                        .all(|product| product.delivered_frame.is_some())
                    && factory(state, ProductionCategory::Infantry)?
                        .is_none_or(|factory| factory.id.is_none() && factory.queue_len == 0)
                {
                    self.check_gi_payment(state)?;
                    self.step = Step::GiSettling { frame };
                    action =
                        Some("both original paid E1 identities delivered; empty Factory and FIFO");
                }
            }
            Step::GiSettling { frame: delivered } if frame >= delivered + 60 => {
                self.check_gi_payment(state)?;
                ensure!(
                    factory(state, ProductionCategory::Infantry)?
                        .is_none_or(|factory| factory.id.is_none() && factory.queue_len == 0),
                    "empty GI Factory constructed a third product"
                );
                self.final_credits = Some(credits(state)?);
                self.step = Step::Done;
                action = Some("paid E1 delivery remained stable for60 frames");
            }
            _ => {}
        }
        if let Some(action) = action {
            actions.push(json!({"native_frame":frame,"input":action,
                "credits":credits(state)?,"factory":factory(state, ProductionCategory::Building)?.as_ref().map(factory_json),
                "infantry_factory":factory(state, ProductionCategory::Infantry)?.as_ref().map(factory_json)}));
        }
        Ok(())
    }

    fn observe_commands(&mut self, state: &AppState) -> Result<()> {
        let Some(log) = &state.match_state.match_diagnostics.replay_log else {
            return Ok(());
        };
        ensure!(
            log.ticks.len() >= self.replay_cursor,
            "Battle diagnostic replay timeline replaced"
        );
        let rules = state.rules().context("retail rules")?;
        let gi = ProductionSelection::for_type(rules, rules.object("E1").context("retail E1")?)
            .context("E1 production selector")?;
        for tick in &log.ticks[self.replay_cursor..] {
            for command in &tick.commands {
                let Command::SynchronizedRecord { record } = &command.payload else {
                    ensure!(
                        !matches!(&command.payload, Command::PlaceProducedMobile { .. }),
                        "live Battle admitted semantic mobile PLACE"
                    );
                    continue;
                };
                // Original gamemd.exe StripClass::AI6A8EB8..6A8F42 issues
                // object RTTI15/index-1/Cell0 for GI; EventCtor4C6AE0 keeps
                // the raw House and frame. Observe the admitted carrier from
                // the existing recorder: after-present OutList may be drained.
                if let Some(place) = PlaceRecord::decode(record)
                    && place.rtti == 15
                {
                    ensure!(
                        place.type_index == -1 && place.naval == 0 && place.x == 0 && place.y == 0,
                        "automatic GI PLACE has the wrong native selector"
                    );
                    self.gi_places.push(place);
                    ensure!(
                        self.gi_places
                            .iter()
                            .filter(|event| event.house_id == place.house_id)
                            .count()
                            <= 2,
                        "a House admitted a duplicate or third GI PLACE"
                    );
                    self.gi_events.push(json!({"admission_tick":tick.tick,"kind":"PLACE","house":place.house_id,"issue_frame":place.frame,"bytes":record.as_bytes().as_slice()}));
                }
                if let Some(produce) = ProductionRecord::decode(record)
                    && produce.event == ProductionEvent::Produce
                    && produce.rtti == gi.rtti
                    && produce.type_index == gi.type_index
                {
                    ensure!(produce.naval == 0, "GI PRODUCE carried Naval");
                    self.gi_produces.push(produce);
                    ensure!(
                        self.gi_produces
                            .iter()
                            .filter(|event| event.house_id == produce.house_id)
                            .count()
                            <= 2,
                        "a House admitted an extra GI PRODUCE"
                    );
                    self.gi_events.push(json!({"admission_tick":tick.tick,"kind":"PRODUCE","house":produce.house_id,"issue_frame":produce.frame,"bytes":record.as_bytes().as_slice()}));
                }
            }
        }
        self.replay_cursor = log.ticks.len();
        Ok(())
    }

    fn observe_infantry(
        &mut self,
        state: &AppState,
        frame: u32,
        actions: &mut Vec<Value>,
    ) -> Result<()> {
        if self.gi_wallet.is_none() {
            return Ok(());
        }
        let factory = factory(state, ProductionCategory::Infantry)?;
        let sim = simulation(state)?;
        let owner = sim
            .interner()
            .get(&local_owner(state)?)
            .context("local House")?;
        if let Some(factory) = &factory {
            if let Some(order) = self.gi_factory_order {
                ensure!(
                    factory.construction_order == order,
                    "GI delivery replaced the retained Factory"
                );
            } else {
                self.gi_factory_order = Some(factory.construction_order);
            }
            if let Some(id) = factory.id
                && !self.gi_products.iter().any(|product| product.id == id)
            {
                ensure!(
                    self.gi_products.len() < 2,
                    "GI Factory constructed a third identity"
                );
                if let Some(first) = self.gi_products.first() {
                    let entity = sim
                        .entities()
                        .get(first.id)
                        .context("first GI before successor")?;
                    ensure!(
                        entity.is_alive() && !entity.in_limbo() && factory.queue_len == 0,
                        "FIFO successor constructed before first GI delivery or retained a tail"
                    );
                    self.successor_promotions += 1;
                }
                self.gi_products.push(GiProduct {
                    id,
                    first_progress: factory.progress,
                    completed_frame: None,
                    delivered_frame: None,
                });
                actions.push(json!({"native_frame":frame,"observation":"paid GI held identity","id":id,"factory":factory_json(factory)}));
            }
        }
        for product in &mut self.gi_products {
            let entity = sim.entities().get(product.id).context("retained paid GI")?;
            ensure!(
                entity.is_alive()
                    && entity.owner() == owner
                    && sim.interner().resolve(entity.type_ref()) == "E1",
                "paid GI identity died or changed owner/type"
            );
            if factory
                .as_ref()
                .is_some_and(|factory| factory.id == Some(product.id) && factory.complete)
                && product.completed_frame.is_none()
            {
                ensure!(
                    entity.in_limbo(),
                    "complete GI escaped before automatic PLACE"
                );
                product.completed_frame = Some(frame);
                actions.push(json!({"native_frame":frame,"observation":"paid GI complete and retained in limbo","id":product.id}));
            }
            if !entity.in_limbo() && product.delivered_frame.is_none() {
                product.delivered_frame = Some(frame);
                actions.push(json!({"native_frame":frame,"observation":"same paid GI delivered by automatic PLACE","id":product.id,"position":entity.position}));
            }
            ensure!(
                product.delivered_frame.is_none() || !entity.in_limbo(),
                "delivered GI returned to limbo"
            );
        }
        Ok(())
    }

    fn check_gi_payment(&self, state: &AppState) -> Result<()> {
        let (before_credits, before_spent) = self.gi_wallet.context("GI wallet baseline")?;
        let cost = self
            .gi_cost
            .context("GI resolved cost")?
            .checked_mul(2)
            .context("two GI costs")?;
        let (credits, spent) = wallet(state)?;
        ensure!(
            Some(credits) == before_credits.checked_sub(cost)
                && Some(spent) == before_spent.checked_add(cost),
            "two paid GIs did not debit exactly two resolved costs"
        );
        ensure!(
            self.successor_promotions == 1,
            "GI FIFO successor did not promote exactly once"
        );
        let house = local_house_id(state)?;
        ensure!(
            self.gi_places
                .iter()
                .filter(|event| event.house_id == house)
                .count()
                == 2
                && self
                    .gi_produces
                    .iter()
                    .filter(|event| event.house_id == house)
                    .count()
                    == 2,
            "paid GI delivery did not use exactly two raw PRODUCE and two raw automatic PLACE records"
        );
        Ok(())
    }

    pub(super) fn finished(&self, state: &AppState) -> Result<bool> {
        let sim = simulation(state)?;
        Ok(matches!(self.step, Step::Done)
            && ["GAPOWR", "GAPILE"].iter().all(|type_id| {
                sim.entities()
                    .values_sorted()
                    .filter(|entity| {
                        entity.is_alive()
                            && !entity.in_limbo()
                            && !entity.in_construction_bstate()
                            && sim.interner().resolve(entity.type_ref()) == *type_id
                    })
                    .count()
                    == 2
            })
            && sim
                .houses()
                .order()
                .iter()
                .enumerate()
                .filter(|(_, owner)| {
                    sim.houses()
                        .get(owner)
                        .is_some_and(|house| house.is_human())
                })
                .all(|(index, _)| {
                    self.gi_places
                        .iter()
                        .filter(|event| i32::from(event.house_id) == index as i32)
                        .count()
                        == 2
                        && self
                            .gi_produces
                            .iter()
                            .filter(|event| i32::from(event.house_id) == index as i32)
                            .count()
                            == 2
                }))
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        let sim = simulation(state)?;
        Ok(json!({"phase":self.phase(),"mcv":self.mcv,"yard":self.yard,
            "cancelled_product":self.first_product,"placed_product":self.placed_product,
            "owner":local_owner(state)?,"barracks_product":self.barracks_product,
            "starting_credits":self.starting_credits,"final_credits":self.final_credits,
            "gi_wallet_before":self.gi_wallet,"gi_cost":self.gi_cost,"gi_factory_order":self.gi_factory_order,
            "gi_products":self.gi_products.iter().map(|product| json!({"id":product.id,"first_progress":product.first_progress,
                "completed_frame":product.completed_frame,"delivered_frame":product.delivered_frame})).collect::<Vec<_>>(),
            "successor_promotions":self.successor_promotions,"gi_native_events":self.gi_events,
            "infantry":sim.entities().values_sorted().filter(|entity| sim.interner().resolve(entity.type_ref()) == "E1")
                .map(|entity| json!({"id":entity.stable_id(),"owner":sim.interner().resolve(entity.owner()),
                    "alive":entity.is_alive(),"in_limbo":entity.in_limbo(),"position":entity.position})).collect::<Vec<_>>(),
            "buildings":sim.entities().values_sorted().filter(|entity|
                entity.is_alive() && !entity.in_limbo() && entity.category() == crate::map::entities::EntityCategory::Structure
            ).map(|entity| json!({"id":entity.stable_id(),"owner":sim.interner().resolve(entity.owner()),
                "type":sim.interner().resolve(entity.type_ref()),"position":entity.position})).collect::<Vec<_>>(),
            "houses":sim.houses().order().iter().filter_map(|owner| sim.houses().get(owner).map(|house|
                json!({"owner":sim.interner().resolve(*owner),"credits":house.economy().credits(),"spent":house.economy().spent_credits(),
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
    Ok(wallet(state)?.0)
}

fn wallet(state: &AppState) -> Result<(i32, i32)> {
    let sim = simulation(state)?;
    let owner = sim
        .interner()
        .get(&local_owner(state)?)
        .context("local House")?;
    let economy = sim.houses().get(&owner).context("local House")?.economy();
    Ok((economy.credits(), economy.spent_credits()))
}

fn local_house_id(state: &AppState) -> Result<i8> {
    let sim = simulation(state)?;
    let owner = sim
        .interner()
        .get(&local_owner(state)?)
        .context("local House")?;
    Ok(i8::try_from(
        sim.houses()
            .order()
            .iter()
            .position(|candidate| *candidate == owner)
            .context("local native House index")?,
    )?)
}

fn factory(state: &AppState, category: ProductionCategory) -> Result<Option<FactorySnapshot>> {
    let sim = simulation(state)?;
    let owner = sim
        .interner()
        .get(&local_owner(state)?)
        .context("local House")?;
    Ok(sim
        .production()
        .factories()
        .view(owner, category)
        .map(|factory| FactorySnapshot {
            id: factory.object.and_then(|object| object.entity_id),
            progress: factory.progress,
            running: factory.is_running(),
            complete: factory.complete_object().is_some(),
            queue_len: factory.queue.len(),
            construction_order: factory.construction_order(),
        }))
}

fn factory_json(factory: &FactorySnapshot) -> Value {
    json!({"id":factory.id,"progress":factory.progress,"running":factory.running,
        "complete":factory.complete,"queue_len":factory.queue_len,"construction_order":factory.construction_order})
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

fn cameo(state: &mut AppState, type_id: &str, right: bool) -> Result<()> {
    let id = simulation(state)?
        .interner()
        .get(type_id)
        .context("retail cameo type")?;
    ensure!(
        crate::ui::sidebar::SidebarTab::all().iter().any(|tab| state
            .match_state
            .match_presentation
            .sidebar_projection
            .cameo_strips()
            .items(*tab)
            .iter()
            .any(|cameo| cameo.id == CameoId::Object(id))),
        "{type_id} is absent from the actual retained local sidebar"
    );
    dispatch::apply_sidebar_action(
        state,
        SidebarAction::CameoPress {
            type_id: type_id.into(),
            right,
            shift: false,
            at_build_limit: false,
        },
    );
    Ok(())
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

fn placement_cursor(state: &AppState, yard: u64, type_id: &str) -> Result<(u16, u16)> {
    let sim = simulation(state)?;
    let rules = state.rules().context("loaded rules")?;
    let yard = sim.entities().get(yard).context("yard")?;
    let owner = local_owner(state)?;
    let object = rules
        .object(type_id)
        .with_context(|| format!("retail {type_id}"))?;
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
                type_id,
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
    anyhow::bail!("no legal {type_id} placement near deployed yard")
}
