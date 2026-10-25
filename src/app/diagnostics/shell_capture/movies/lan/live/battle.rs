//! Real input automation for a bounded Battle opening. This observer never
//! mutates the simulation, fabricates a peer command, or advances a frame.

use super::*;
use crate::app::input::entity_pick::PickSource;
use crate::app::input::hotkeys::{self, HotkeyCommand, HotkeyResolution};
use crate::app::input::{commands, dispatch, keyboard};
use crate::sim::combat::TargetKind;
use crate::sim::command::{
    ArchiveRecord, Command, MegaMissionOrder, MegaMissionRecord, MegaMissionTarget, ObjectAction,
    ObjectActionRecord, PlaceRecord, ProductionEvent, ProductionRecord, RepairRecord, StopRecord,
};
use crate::sim::production::{ProductionCategory, ProductionSelection};
use crate::sim::world::Simulation;
use crate::ui::sidebar::SidebarAction;
use crate::ui::sidebar::cameo_order::CameoId;
use winit::event::ElementState;
use winit::keyboard::{Key, KeyLocation};

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
    RallyPending,
    GiStarted,
    GiQueued,
    GiProducing,
    GiSettling {
        frame: u32,
    },
    Rendezvous,
    Approaching,
    Attacking,
    Stopping,
    Disengaging {
        frame: u32,
    },
    RepairReturning,
    RepairDamaging,
    RepairStopping,
    RepairFirst,
    RepairOff,
    RepairFinishing,
    SaleStarting,
    SalePending,
    SaleReentering,
    Done,
}

struct GiProduct {
    id: u64,
    first_progress: u16,
    completed_frame: Option<u32>,
    delivered_frame: Option<u32>,
    rally_inherited: Option<Value>,
    rally_handoff: Option<Value>,
    rally_walk: Option<Value>,
    rally_arrival: Option<Value>,
}

/// Read-only observations of one Human's actual repair episode. These are
/// journal receipts and frame snapshots, never inputs to the simulation.
#[derive(Default)]
struct RepairObservation {
    attack: Option<MegaMissionRecord>,
    attack_health: Option<i32>,
    attack_execution: Option<Value>,
    damage: Option<Value>,
    stop: Option<StopRecord>,
    stop_execution: Option<Value>,
    records: Vec<RepairRecord>,
    events: Vec<Value>,
    executions: Vec<Value>,
    payments: Vec<Value>,
    previous: Option<RepairSnapshot>,
    off: Option<RepairSnapshot>,
    stopped: Option<RepairSnapshot>,
    full: Option<RepairSnapshot>,
}

#[derive(Clone, serde::Serialize)]
struct RepairSnapshot {
    native_frame: u32,
    id: u64,
    owner: String,
    health: i32,
    estimated_health: i32,
    repairing: bool,
    credits: i32,
    spent: i32,
}

/// Receipt-derived sale observations. The simulation retains every writable
/// Building, Factory, wallet, crew and lifecycle decision in its existing owner.
#[derive(Default, serde::Serialize)]
struct SaleObservation {
    #[serde(skip)]
    produces: Vec<ProductionRecord>,
    #[serde(skip)]
    record: Option<ObjectActionRecord>,
    #[serde(rename = "native_events")]
    events: Vec<Value>,
    baseline: Option<SaleSnapshot>,
    head: Option<u64>,
    head_birth: Option<SaleSnapshot>,
    first_charge: Option<SaleSnapshot>,
    execution: Option<SaleSnapshot>,
    phases: Vec<SaleSnapshot>,
    advanced: Option<SaleSnapshot>,
    charges: Vec<Value>,
    #[serde(rename = "final")]
    previous: Option<SaleSnapshot>,
    refund: Option<i32>,
    crew_count: Option<usize>,
    before_e1: Vec<u64>,
    #[serde(skip)]
    cells: Vec<(u16, u16)>,
    crew: Vec<u64>,
    crew_birth: Vec<Value>,
    crew_walk: Vec<Value>,
    crew_settled: Vec<Value>,
    exited: Option<SaleSnapshot>,
    #[serde(skip)]
    reentry: Option<MegaMissionRecord>,
    reentry_arrival: Option<Value>,
    stable: Option<SaleSnapshot>,
}

#[derive(Clone, serde::Serialize)]
struct SaleFactorySnapshot {
    order: u64,
    head: Option<u64>,
    progress: u16,
    balance: i32,
    running: bool,
    suspended: bool,
    rate: u16,
    timer_start: i32,
    timer_duration: i32,
    queue: Vec<(String, u64)>,
}

#[derive(Clone, serde::Serialize)]
struct SaleBuildingSnapshot {
    health: i32,
    estimated_health: i32,
    repairing: bool,
    in_limbo: bool,
    mission: i32,
    queued_mission: i32,
    status: u32,
    body: Option<i32>,
    queued_body: Option<i32>,
    ready: u8,
    control: [i32; 3],
    stage: i32,
    rate: i32,
}

#[derive(Clone, serde::Serialize)]
struct SaleSnapshot {
    native_frame: u32,
    producer: u64,
    owner: String,
    credits: i32,
    spent: i32,
    tracking_buildings: i32,
    buildings_lost: u32,
    building: Option<SaleBuildingSnapshot>,
    factory: Option<SaleFactorySnapshot>,
    producer_ids: Vec<u64>,
    occupied_cells: Vec<(u16, u16)>,
    in_logic: bool,
    display_layer: Option<u8>,
    pending_delete: bool,
}

#[derive(Default)]
pub(super) struct BattleOpening {
    sale_enabled: bool,
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
    rally_input: Option<Value>,
    rally_prepared: Option<(u16, u16)>,
    rally_records: [Option<ArchiveRecord>; 2],
    rally_executions: [Option<Value>; 2],
    rally_events: Vec<Value>,
    rally_planning_commits: [Option<Value>; 2],
    /// Actual first held paid identities, observed from the two House Factories.
    /// Index0 is Host, index1 Guest; neither starting infantry nor a guessed ID.
    fighters: [Option<u64>; 2],
    /// First accepted ordinary Move destination per fighter; receipt-derived,
    /// including a remote admission before the local staging input begins.
    staging: [Option<(u16, u16)>; 2],
    attack_input_frame: Option<u32>,
    pending_attack: Option<MegaMissionRecord>,
    pending_stop: Option<StopRecord>,
    attack_frame: Option<u32>,
    attack_health: Option<i32>,
    attack_observation: Option<Value>,
    hit_observation: Option<Value>,
    stop_observation: Option<Value>,
    combat_events: Vec<Value>,
    repair: [RepairObservation; 2],
    sale: [SaleObservation; 2],
    sale_local_input: Option<Value>,
    sale_local_cleanup: Option<Value>,
    sale_local_move: Option<(u16, u16)>,
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
    pub(super) fn new(sale_enabled: bool) -> Self {
        Self {
            sale_enabled,
            ..Self::default()
        }
    }

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
        ensure!(
            host == state.match_state.session.host(),
            "Battle input role differs from admitted Session"
        );
        self.observe_fighters(state)?;
        self.observe_commands(state, frame)?;
        self.observe_due_orders(state, frame)?;
        self.observe_repair(state, frame)?;
        if self.sale_enabled {
            self.observe_sale(state, frame)?;
        }
        self.observe_infantry(state, frame, actions)?;
        self.observe_hit(state, frame, actions)?;
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
                    let (clicked, prepared) = rally_click(state, id)?;
                    let eva_requests_added = planning_rally_with_mouse(state, id, clicked)?;
                    ensure!(
                        simulation(state)?
                            .entities()
                            .get(id)
                            .context("rallied GAPILE")?
                            .archive_target()
                            .is_none(),
                        "rally click changed producer Archive before synchronized execution"
                    );
                    self.rally_prepared = Some(prepared);
                    self.rally_input = Some(json!({"native_frame":frame,"producer":id,
                        "clicked":clicked,"prepared":prepared,"eva_requests_added":eva_requests_added,
                        "planning_at_mouse_release":true,"planning_after_key_release":false}));
                    self.step = Step::RallyPending;
                    action = Some(
                        "real Planning interleaved GAPILE rally and EVA; key release commits before E1",
                    );
                }
            }
            Step::RallyPending => {
                let index = usize::from(!host);
                ensure!(
                    !state.match_state.input.planning.active(),
                    "Planning remained active before E1 production input"
                );
                if self.rally_executions[index].is_some()
                    && self.rally_planning_commits[index].is_some()
                {
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
                    action = Some("raw ARCHIVE executed on GAPILE; E1 cameo: Produce");
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
                        .all(|product| product.rally_arrival.is_some())
                    && factory(state, ProductionCategory::Infantry)?
                        .is_none_or(|factory| factory.id.is_none() && factory.queue_len == 0)
                {
                    self.check_gi_payment(state)?;
                    self.step = Step::GiSettling { frame };
                    action = Some(
                        "both original paid E1 identities followed rally and settled; empty Factory and FIFO",
                    );
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
                if self.all_gi_orders_admitted(state)? {
                    let desired = staging_centres(state)?;
                    let index = usize::from(!host);
                    let id = self.fighter_ids()?[index];
                    ensure!(
                        Some(id) == self.gi_products.first().map(|product| product.id),
                        "encounter actor is not the first original local paid GI"
                    );
                    let cell = legal_move_cell(state, id, desired[index])?;
                    ground_order_with_mouse(state, id, cell)?;
                    self.step = Step::Rendezvous;
                    action = Some("paid E1 delivery stable; tactical Move to encounter staging");
                }
            }
            Step::Rendezvous if host => {
                let ids = self.fighter_ids()?;
                if let [Some(host_cell), Some(guest_cell)] = self.staging
                    && ids.iter().zip([host_cell, guest_cell]).all(|(&id, cell)| {
                        simulation(state)
                            .ok()
                            .and_then(|sim| sim.entities().get(id))
                            .is_some_and(|entity| {
                                entity.navigation.nav_com().is_none()
                                    && entity.position.rx().abs_diff(cell.0) <= 1
                                    && entity.position.ry().abs_diff(cell.1) <= 1
                            })
                    })
                {
                    let target = simulation(state)?
                        .entities()
                        .get(ids[1])
                        .context("opposing paid GI")?;
                    let target_cell = (target.position.rx(), target.position.ry());
                    let approach = offset_toward(target_cell, host_cell, 3);
                    let approach = legal_move_cell(state, ids[0], approach)?;
                    ground_order_with_mouse(state, ids[0], approach)?;
                    self.step = Step::Approaching;
                    action = Some("both paid GIs staged; Host approaches through tactical Move");
                }
            }
            Step::Approaching if host => {
                let [actor, target] = self.fighter_ids()?;
                if mouse_can_pick(state, target)? {
                    select_entity(state, actor)?;
                    click_entity(state, target)?;
                    self.attack_input_frame = Some(frame);
                    self.step = Step::Attacking;
                    action =
                        Some("Host selected paid GI and object-clicked visible opposing paid GI");
                }
            }
            Step::Attacking if host && self.hit_observation.is_some() => {
                let actor = self.fighter_ids()?[0];
                select_entity(state, actor)?;
                chat_key(state, KeyCode::KeyS, &Key::Character("s".into()), Some("s"));
                self.step = Step::Stopping;
                action = Some("observed post-command rifle hit; Host S-key Stop");
            }
            Step::Rendezvous | Step::Attacking | Step::Stopping
                if self.stop_observation.is_some() =>
            {
                // Stop's immediate clear was observed before these later inputs.
                // Guard may legitimately reacquire while an enemy remains nearby.
                let index = usize::from(!host);
                let id = self.fighter_ids()?[index];
                let staging = [
                    self.staging[0].context("accepted Host staging Move")?,
                    self.staging[1].context("accepted Guest staging Move")?,
                ];
                let cell = if host {
                    staging[0]
                } else {
                    offset_toward(staging[1], staging[0], -6)
                };
                let cell = legal_move_cell(state, id, cell)?;
                if !host {
                    select_entity(state, id)?;
                    chat_key(state, KeyCode::KeyS, &Key::Character("s".into()), Some("s"));
                }
                ground_order_with_mouse(state, id, cell)?;
                self.step = Step::Disengaging { frame };
                action = Some("raw Stop cleared orders; tactical Move separates the surviving GIs");
            }
            Step::Disengaging { frame: stopped } if frame >= stopped + 180 => {
                let [actor, target] = self.fighter_ids()?;
                let sim = simulation(state)?;
                let actor = sim.entities().get(actor).context("surviving attacker")?;
                let target = sim.entities().get(target).context("surviving target")?;
                if actor
                    .position
                    .rx()
                    .abs_diff(target.position.rx())
                    .max(actor.position.ry().abs_diff(target.position.ry()))
                    >= 10
                    && [actor, target].iter().all(|entity| {
                        entity.navigation.nav_com().is_none()
                            && entity.attack_target.target().is_none()
                    })
                {
                    let index = usize::from(!host);
                    let id = self.fighter_ids()?[index];
                    let building = sim
                        .entities()
                        .get(self.barracks_product.context("paid repair GAPILE")?)
                        .context("paid repair GAPILE")?;
                    let gi = sim.entities().get(id).context("returning paid GI")?;
                    let desired = offset_toward(
                        (building.position.rx(), building.position.ry()),
                        (gi.position.rx(), gi.position.ry()),
                        4,
                    );
                    let goal = legal_move_cell(state, id, desired)?;
                    ground_order_with_mouse(state, id, goal)?;
                    self.step = Step::RepairReturning;
                    action =
                        Some("GIs separated after Attack/Stop; real Move back to own paid GAPILE");
                }
            }
            Step::RepairReturning => {
                let id = self.fighter_ids()?[usize::from(!host)];
                let sim = simulation(state)?;
                let gi = sim.entities().get(id).context("returned paid GI")?;
                let producer = self.barracks_product.context("paid repair GAPILE")?;
                let building = sim.entities().get(producer).context("paid repair GAPILE")?;
                if gi.navigation.nav_com().is_none()
                    && gi
                        .position
                        .rx()
                        .abs_diff(building.position.rx())
                        .max(gi.position.ry().abs_diff(building.position.ry()))
                        <= 6
                {
                    force_fire_with_mouse(state, id, producer)?;
                    self.step = Step::RepairDamaging;
                    action =
                        Some("real Control force-fire click from paid GI onto own paid GAPILE");
                }
            }
            Step::RepairDamaging => {
                let index = usize::from(!host);
                let rules = state.rules().context("retail repair rules")?;
                let strength = rules.object("GAPILE").context("retail GAPILE")?.strength;
                let damaged = simulation(state)?
                    .entities()
                    .get(self.barracks_product.context("paid repair GAPILE")?)
                    .context("paid repair GAPILE")?
                    .health
                    .current();
                if self.repair[index].damage.is_some()
                    && damaged <= strength - 8 * rules.general.repair_step
                {
                    select_entity(state, self.fighter_ids()?[index])?;
                    chat_key(state, KeyCode::KeyS, &Key::Character("s".into()), Some("s"));
                    self.step = Step::RepairStopping;
                    action = Some("real rifle damage accumulated; S-key Stop before paid repair");
                }
            }
            Step::RepairStopping => {
                let observation = &self.repair[usize::from(!host)];
                let period = repair_period(state)?;
                if observation.stop_execution.is_some()
                    && let Some(stop) = observation.stop
                    && frame >= u32::try_from(stop.frame)? + 1 + 2 * period
                {
                    repair_with_mouse(state, self.barracks_product.context("paid repair GAPILE")?)?;
                    self.step = Step::RepairFirst;
                    action =
                        Some("verified Repair hotkey enables real cursor; damaged GAPILE click");
                }
            }
            Step::RepairFirst if !self.repair[usize::from(!host)].payments.is_empty() => {
                repair_with_mouse(state, self.barracks_product.context("paid repair GAPILE")?)?;
                self.step = Step::RepairOff;
                action =
                    Some("first real paid heal observed; sticky Repair cursor click stops repair");
            }
            Step::RepairOff if self.repair[usize::from(!host)].stopped.is_some() => {
                repair_with_mouse(state, self.barracks_product.context("paid repair GAPILE")?)?;
                self.step = Step::RepairFinishing;
                action = Some(
                    "manual stop held health and cash across two periods; real repair restart",
                );
            }
            Step::RepairFinishing if self.repair[usize::from(!host)].full.is_some() => {
                if self.sale_enabled {
                    repair_mode_key(state, false)?;
                    cameo(state, "E1", false)?;
                    self.step = Step::SaleStarting;
                    action = Some(
                        "paid repair reached full health; Repair key off and real E1 sale-head Produce",
                    );
                } else {
                    repair_mode_key(state, false)?;
                    self.step = Step::Done;
                    action = Some(
                        "paid repair reached full actual/estimated health and stopped; Repair key off",
                    );
                }
            }
            Step::SaleStarting if self.sale[usize::from(!host)].first_charge.is_some() => {
                let producer = self.barracks_product.context("original paid sale GAPILE")?;
                cameo(state, "E1", false)?;
                sell_mode_key(state, true)?;
                click_entity(state, producer)?;
                ensure!(
                    state
                        .match_state
                        .match_presentation
                        .sidebar_gadget_state
                        .sell_mode_on,
                    "ordinary Sell click did not retain sticky mode"
                );
                sell_mode_key(state, false)?;
                select_entity(state, producer)?;
                self.sale_local_input = Some(json!({"native_frame":frame,"producer":producer,
                    "sell_mode_after_click":true,"sell_mode_after_key_release":false,
                    "selected_before_execution":projected_selection(state)==[producer]}));
                self.step = Step::SalePending;
                action = Some(
                    "real E1 successor then Sell key/GAPILE click; leave mode and select pending seller",
                );
            }
            Step::SalePending => {
                let index = usize::from(!host);
                let sale = &self.sale[index];
                if sale.exited.is_some()
                    && sale.crew_settled.len() == sale.crew_count.context("sale crew count")?
                {
                    let id = self.fighter_ids()?[index];
                    let sim = simulation(state)?;
                    let rules = state.rules().context("retail reentry rules")?;
                    let owner = sim.entities().get(id).context("paid reentry GI")?.owner();
                    let goal = sale
                        .cells
                        .iter()
                        .copied()
                        .find(|&cell| {
                            commands::ordinary_cell_move_goal(
                                sim,
                                rules,
                                state.overlay_registry(),
                                owner,
                                id,
                                cell,
                                false,
                            ) == Some(cell)
                        })
                        .context("no legal ordinary GI Move into the former GAPILE foundation")?;
                    ground_order_with_mouse(state, id, goal)?;
                    self.sale_local_move = Some(goal);
                    self.step = Step::SaleReentering;
                    action = Some(
                        "sale crew settled; real original-GI Move into former factory foundation",
                    );
                }
            }
            Step::SaleReentering if self.sale[usize::from(!host)].stable.is_some() => {
                ensure!(
                    !state
                        .match_state
                        .match_presentation
                        .sidebar_gadget_state
                        .sell_mode_on,
                    "Sell mode remained active after sale"
                );
                self.step = Step::Done;
                action =
                    Some("sale, queue refund, full-health crew and former-foundation Move settled");
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

    fn fighter_ids(&self) -> Result<[u64; 2]> {
        Ok([
            self.fighters[0].context("Host's first paid GI")?,
            self.fighters[1].context("Guest's first paid GI")?,
        ])
    }

    fn observe_fighters(&mut self, state: &AppState) -> Result<()> {
        let sim = simulation(state)?;
        for (index, owner) in battle_owners(&state.match_state.session, sim)?
            .into_iter()
            .enumerate()
        {
            if self.fighters[index].is_some() {
                continue;
            }
            if let Some(id) = sim
                .production()
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .and_then(|factory| factory.object.and_then(|object| object.entity_id))
            {
                let entity = sim.entities().get(id).context("first held GI")?;
                ensure!(
                    entity.owner() == owner
                        && entity.in_limbo()
                        && sim.interner().resolve(entity.type_ref()) == "E1",
                    "encounter Factory held a different object"
                );
                self.fighters[index] = Some(id);
            }
        }
        Ok(())
    }

    fn observe_hit(
        &mut self,
        state: &AppState,
        frame: u32,
        actions: &mut Vec<Value>,
    ) -> Result<()> {
        if self.hit_observation.is_some() {
            return Ok(());
        }
        let Some(admitted) = self.attack_frame else {
            return Ok(());
        };
        let [actor, target] = self.fighter_ids()?;
        let sim = simulation(state)?;
        let actor = sim.entities().get(actor).context("firing paid GI")?;
        let target = sim.entities().get(target).context("shot paid GI")?;
        if target.health.current() < self.attack_health.context("Attack health baseline")?
            && actor.rearm_timer.start_frame() >= admitted as i32
            && actor.rearm_timer.duration() > 0
        {
            ensure!(
                actor.attack_target.target() == Some(TargetKind::Entity(target.stable_id())),
                "observed damage after the attacker lost its manual target"
            );
            let snapshot = combat_snapshot(state, self.fighter_ids()?, frame)?;
            actions.push(json!({"native_frame":frame,"observation":"post-manual-Attack rearm and opposing GI health reduction",
                "combat":snapshot}));
            self.hit_observation = Some(snapshot);
        }
        Ok(())
    }

    /// Replay records DoList admission, which differs between peers and can
    /// precede execution by MaxAhead. DoList dispatches raw frameN after Logic;
    /// MainTick's late commit makes its first observable state binary_frameN+1.
    /// Source: command_schedule::admit_queued_command and commit_frame_tail.
    /// The capture handler renders one runtime service pass (handler.rs), whose
    /// advance_one_simulation_frame loop is0..1. If a failed presentation ever
    /// skips the required boundary, refuse the capture instead of labelling a
    /// later mutable state as the command's immediate effect.
    fn observe_due_orders(&mut self, state: &AppState, frame: u32) -> Result<()> {
        for index in 0..2 {
            // The canonical full-repair marker closes this earlier producer
            // observation before subsequent authorized sale can remove it.
            if self.repair[index].full.is_some() {
                continue;
            }
            let Some(record) = self.rally_records[index] else {
                continue;
            };
            let after_execution = u32::try_from(record.frame)?
                .checked_add(1)
                .context("ARCHIVE frame")?;
            let sim = simulation(state)?;
            let source = sim
                .entities()
                .get(u64::try_from(record.entity_id)?)
                .context("rally producer")?;
            ensure!(
                sim.planning_paths(source.owner()).is_empty(),
                "manual GAPILE rally created a Planning path"
            );
            if self.rally_executions[index].is_some() {
                continue;
            }
            let MegaMissionTarget::Cell { x, y } = record.target else {
                anyhow::bail!("manual GAPILE rally did not carry a prepared Cell");
            };
            if frame < after_execution {
                ensure!(
                    source.archive_target().is_none(),
                    "GAPILE Archive changed before admitted raw ARCHIVE executed"
                );
                continue;
            }
            ensure!(
                frame == after_execution,
                "capture missed ARCHIVE execution boundary: expected{after_execution}, observed{frame}"
            );
            ensure!(
                source.archive_target()
                    == Some(TargetKind::Cell(u16::try_from(x)?, u16::try_from(y)?)),
                "executed raw ARCHIVE did not set the prepared GAPILE target"
            );
            self.rally_executions[index] =
                Some(json!({"native_frame":frame,"event_frame":record.frame,
                "source":record.entity_id,"owner":sim.interner().resolve(source.owner()),
                "archive":source.archive_target(),"planning_paths":0}));
        }
        if let Some(attack) = self.pending_attack {
            let after_execution = u32::try_from(attack.frame)?
                .checked_add(1)
                .context("Attack frame")?;
            if frame >= after_execution {
                ensure!(
                    frame == after_execution,
                    "capture missed Attack execution boundary: expected{after_execution}, observed{frame}"
                );
                let [actor, target] = self.fighter_ids()?;
                let sim = simulation(state)?;
                let entity = sim.entities().get(actor).context("manual Attack actor")?;
                ensure!(
                    !entity.is_deployed() && !entity.infantry_deploy_doing(),
                    "manual Attack actor was not an upright GI"
                );
                ensure!(
                    entity.attack_target.target() == Some(TargetKind::Entity(target))
                        && (entity.mission.current().raw() == 1
                            || entity.mission.queued().raw() == 1),
                    "executed raw Attack did not install its target and current/queued mission"
                );
                self.attack_frame = Some(after_execution);
                self.attack_health = Some(
                    sim.entities()
                        .get(target)
                        .context("manual Attack target")?
                        .health
                        .current(),
                );
                self.attack_observation = Some(combat_snapshot(state, [actor, target], frame)?);
                self.pending_attack = None;
                self.step = Step::Attacking;
            }
        }
        if let Some(stop) = self.pending_stop {
            let after_execution = u32::try_from(stop.frame)?
                .checked_add(1)
                .context("Stop frame")?;
            if frame >= after_execution {
                ensure!(
                    frame == after_execution,
                    "capture missed Stop execution boundary: expected{after_execution}, observed{frame}"
                );
                let [actor, target] = self.fighter_ids()?;
                ensure!(
                    self.hit_observation.is_some(),
                    "Host Stop executed before the manual Attack's shot"
                );
                let entity = simulation(state)?
                    .entities()
                    .get(actor)
                    .context("stopped GI")?;
                ensure!(
                    entity.attack_target.target().is_none()
                        && entity.navigation.nav_com().is_none()
                        && entity.navigation.suspended_nav_com().is_none()
                        && entity.suspended_attack_target.target().is_none()
                        && entity.navigation.nav_queue().is_empty(),
                    "executed raw Stop did not clear the ordinary GI's target/navigation references"
                );
                self.stop_observation = Some(combat_snapshot(state, [actor, target], frame)?);
                self.pending_stop = None;
            }
        }
        Ok(())
    }

    fn all_gi_orders_admitted(&self, state: &AppState) -> Result<bool> {
        let sim = simulation(state)?;
        Ok(sim
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

    /// Repair450821..4508D0 runs in Logic before Event15's4C6ED2 tail
    /// dispatch. A paid Logic frameN is therefore observed at binary frameN+1;
    /// the preceding snapshot must already have its repair byte enabled.
    fn observe_repair(&mut self, state: &AppState, frame: u32) -> Result<()> {
        let sim = simulation(state)?;
        let rules = state.rules().context("retail repair rules")?;
        let object = rules.object("GAPILE").context("retail GAPILE")?;
        let cost = crate::sim::production::repair_step_cost(rules, object);
        let period = repair_period(state)?;
        for index in 0..2 {
            let observation = &mut self.repair[index];
            if observation.full.is_some() {
                continue;
            }
            let Some(attack) = observation.attack else {
                continue;
            };
            let producer = u64::try_from(
                self.rally_records[index]
                    .context("rallied repair GAPILE")?
                    .entity_id,
            )?;
            let gi = sim
                .entities()
                .get(u64::try_from(attack.source_id)?)
                .context("friendly-fire GI")?;
            let current = repair_snapshot(state, producer, frame)?;
            ensure!(
                current.health > 0 && gi.is_alive(),
                "repair episode destroyed a paid object"
            );
            let attack_due = u32::try_from(attack.frame)?
                .checked_add(1)
                .context("force-fire frame")?;
            if observation.attack_execution.is_none() && frame >= attack_due {
                ensure!(
                    frame == attack_due,
                    "capture missed friendly force-fire execution boundary"
                );
                ensure!(
                    gi.attack_target.target() == Some(TargetKind::Entity(producer))
                        && (gi.mission.current().raw() == 1 || gi.mission.queued().raw() == 1),
                    "executed friendly force-fire did not install GAPILE target/Attack"
                );
                observation.attack_health = Some(current.health);
                observation.attack_execution = Some(json!({"event_frame":attack.frame,
                    "state":current,"combat":combat_snapshot(state,[gi.stable_id(),producer],frame)?}));
            }
            if observation.damage.is_none()
                && observation
                    .attack_health
                    .is_some_and(|health| current.health < health)
                && gi.rearm_timer.start_frame() >= attack_due as i32
                && gi.rearm_timer.duration() > 0
            {
                ensure!(
                    gi.attack_target.target() == Some(TargetKind::Entity(producer)),
                    "GAPILE damage observed after losing friendly force-fire target"
                );
                observation.damage = Some(json!({"state":current,
                    "combat":combat_snapshot(state,[gi.stable_id(),producer],frame)?}));
            }
            if observation.stop_execution.is_none()
                && let Some(stop) = observation.stop
            {
                let due = u32::try_from(stop.frame)?
                    .checked_add(1)
                    .context("friendly-fire Stop frame")?;
                if frame >= due {
                    ensure!(
                        frame == due,
                        "capture missed friendly-fire Stop execution boundary"
                    );
                    ensure!(
                        observation.damage.is_some(),
                        "friendly-fire Stop preceded real rifle damage"
                    );
                    ensure!(
                        gi.attack_target.target().is_none()
                            && gi.suspended_attack_target.target().is_none()
                            && gi.navigation.nav_com().is_none()
                            && gi.navigation.suspended_nav_com().is_none()
                            && gi.navigation.nav_queue().is_empty(),
                        "executed friendly-fire Stop retained target/navigation references"
                    );
                    observation.stop_execution = Some(json!({"event_frame":stop.frame,
                        "state":current,"combat":combat_snapshot(state,[gi.stable_id(),producer],frame)?}));
                }
            }
            if let Some(record) = observation.records.get(observation.executions.len()) {
                let due = u32::try_from(record.frame)?
                    .checked_add(1)
                    .context("REPAIR frame")?;
                let toggle = observation.executions.len();
                if frame < due {
                    ensure!(
                        current.repairing == (toggle == 1),
                        "REPAIR changed its byte before synchronized execution"
                    );
                }
                if frame >= due {
                    ensure!(frame == due, "capture missed REPAIR execution boundary");
                    ensure!(
                        current.repairing == (toggle != 1) && current.health < object.strength,
                        "raw REPAIR did not perform the expected damaged start/off/restart toggle"
                    );
                    observation
                        .executions
                        .push(json!({"event_frame":record.frame,"state":current}));
                    if toggle == 1 {
                        observation.off = Some(current.clone());
                    }
                }
            }
            if let Some(previous) = &observation.previous {
                if current.health > previous.health {
                    ensure!(
                        !observation.executions.is_empty() && previous.repairing,
                        "GAPILE healed before manual REPAIR was enabled for Logic"
                    );
                    ensure!(
                        frame == previous.native_frame + 1 && (frame - 1) % period == 0,
                        "paid repair did not occur on its global native Logic frame: previous={}, current={frame}, period={period}, before_health={}, after_health={}",
                        previous.native_frame,
                        previous.health,
                        current.health
                    );
                    ensure!(
                        current.health - previous.health
                            == rules
                                .general
                                .repair_step
                                .min(object.strength - previous.health),
                        "paid repair changed actual health by the wrong retail step/clamp"
                    );
                    ensure!(
                        previous.credits - current.credits == cost
                            && current.spent - previous.spent == cost,
                        "paid repair did not debit the existing owner's exact cost/statistics"
                    );
                    ensure!(
                        current.estimated_health >= previous.estimated_health,
                        "paid repair decreased estimated health"
                    );
                    observation.payments.push(json!({"logic_frame":frame-1,
                        "cost":cost,"before":previous,"after":current}));
                } else if !observation.executions.is_empty() {
                    ensure!(
                        current.credits == previous.credits && current.spent == previous.spent,
                        "repair changed cash/statistics without a paid health step"
                    );
                }
            }
            if observation.stopped.is_none()
                && let Some(off) = &observation.off
                && frame >= off.native_frame + 2 * period
            {
                ensure!(
                    frame == off.native_frame + 2 * period,
                    "capture missed manual repair stop interval boundary"
                );
                ensure!(
                    !current.repairing
                        && current.health == off.health
                        && current.credits == off.credits
                        && current.spent == off.spent,
                    "manually stopped repair changed health/cash during two periods"
                );
                observation.stopped = Some(current.clone());
            }
            if observation.full.is_none()
                && observation.executions.len() == 3
                && !current.repairing
                && current.health == object.strength
                && current.estimated_health == object.strength
            {
                ensure!(
                    !observation.payments.is_empty(),
                    "full repair had no paid visits"
                );
                observation.full = Some(current.clone());
            }
            observation.previous = Some(current);
        }
        Ok(())
    }

    fn observe_sale(&mut self, state: &AppState, frame: u32) -> Result<()> {
        let sim = simulation(state)?;
        let rules = state.rules().context("retail sale rules")?;
        let object = rules.object("GAPILE").context("retail GAPILE")?;
        let crew_type = rules
            .general
            .allied_crew
            .as_deref()
            .context("retail AlliedCrew")?;
        ensure!(
            crew_type == "E1",
            "selected GAPILE sale control requires ordinary Allied E1 crew"
        );
        let crew_strength = rules
            .object(crew_type)
            .context("retail sale crew")?
            .strength;
        for index in 0..2 {
            let observation = &mut self.sale[index];
            let Some(first) = observation.produces.first().copied() else {
                continue;
            };
            let producer = u64::try_from(
                self.rally_records[index]
                    .context("original paid seller")?
                    .entity_id,
            )?;
            let owner = battle_owners(&state.match_state.session, sim)?[index];
            if observation.cells.is_empty() {
                let entity = sim
                    .entities()
                    .get(producer)
                    .context("seller before production")?;
                observation.cells = crate::sim::production::building_base_foundation_cells(
                    entity.position.rx(),
                    entity.position.ry(),
                    &object.foundation,
                );
            }
            let current = sale_snapshot(state, index, producer, &observation.cells, frame)?;
            let first_frame = u32::try_from(first.frame)?;
            if observation.baseline.is_none() && frame >= first_frame {
                ensure!(
                    frame == first_frame,
                    "capture missed sale-queue pre-PRODUCE frame"
                );
                ensure!(
                    current
                        .building
                        .as_ref()
                        .is_some_and(|building| building.health == object.strength
                            && building.estimated_health == object.strength
                            && !building.repairing
                            && !building.in_limbo),
                    "sale began before completed paid repair"
                );
                let full = self.repair[index]
                    .full
                    .as_ref()
                    .context("sale before this House completed repair")?;
                ensure!(
                    frame >= full.native_frame
                        && current.credits == full.credits
                        && current.spent == full.spent
                        && current
                            .building
                            .as_ref()
                            .is_some_and(|building| building.health == full.health
                                && building.estimated_health == full.estimated_health
                                && building.repairing == full.repairing),
                    "sale baseline did not continue that House's completed repair health/cash"
                );
                ensure!(
                    !current.occupied_cells.is_empty()
                        && current.in_logic
                        && current.display_layer.is_some()
                        && current.producer_ids.contains(&producer),
                    "sale baseline did not contain the live mapped producer"
                );
                observation.baseline = Some(current.clone());
            }
            if observation.head_birth.is_none() && frame >= first_frame + 1 {
                ensure!(
                    frame == first_frame + 1,
                    "capture missed sale-head construction boundary"
                );
                let factory = current
                    .factory
                    .as_ref()
                    .context("new paid sale-head Factory")?;
                let head = factory.head.context("new paid sale head")?;
                ensure!(
                    !self.gi_products.iter().any(|product| product.id == head)
                        && factory.progress == 0
                        && factory.queue.is_empty(),
                    "new sale head reused a delivered identity or advanced before Logic"
                );
                observation.head = Some(head);
                observation.before_e1 = sim
                    .entities()
                    .values_sorted()
                    .filter(|entity| {
                        entity.owner() == owner && sim.interner().resolve(entity.type_ref()) == "E1"
                    })
                    .map(|entity| entity.stable_id())
                    .collect();
                observation.head_birth = Some(current.clone());
            }
            for &id in &observation.before_e1 {
                if Some(id) == observation.head {
                    continue;
                }
                ensure!(
                    sim.entities()
                        .get(id)
                        .is_some_and(|entity| entity.is_alive()
                            && !entity.in_limbo()
                            && entity.owner() == owner
                            && sim.interner().resolve(entity.type_ref()) == "E1"),
                    "sale lost an existing original GI"
                );
            }
            if observation.first_charge.is_none()
                && observation
                    .baseline
                    .as_ref()
                    .is_some_and(|baseline| current.spent > baseline.spent)
            {
                let factory = current
                    .factory
                    .as_ref()
                    .context("charged sale-head Factory")?;
                ensure!(
                    factory.progress > 0
                        && factory.progress < crate::sim::production::PRODUCTION_STEPS,
                    "sale head had no unfinished paid progress"
                );
                observation.first_charge = Some(current.clone());
            }
            if let Some(record) = observation.record {
                let due = u32::try_from(record.frame)? + 1;
                if observation.execution.is_none() && frame >= due {
                    ensure!(frame == due, "capture missed SELL execution boundary");
                    let building = current
                        .building
                        .as_ref()
                        .context("SELL removed producer immediately")?;
                    let factory = current
                        .factory
                        .as_ref()
                        .context("SELL canceled Factory at admission")?;
                    ensure!(
                        (building.mission == 0x13 || building.queued_mission == 0x13)
                            && building.status == 0
                            && !building.in_limbo
                            && factory.head == observation.head
                            && factory.queue.len() == 1
                            && factory.queue[0].0 == "E1",
                        "SELL did not retain selling producer and unfinished head/successor"
                    );
                    observation.refund = Some(
                        crate::sim::production::techno_refund(sim, rules, producer)
                            .context("live seller refund")?,
                    );
                    observation.crew_count = Some(usize::try_from(
                        sim.building_survivor_count(rules, producer),
                    )?);
                    ensure!(
                        observation.crew_count.is_some_and(|count| count > 0),
                        "ordinary GAPILE has no expected crew"
                    );
                    observation.execution = Some(current.clone());
                } else if frame < due {
                    ensure!(
                        current
                            .building
                            .as_ref()
                            .is_some_and(|building| building.mission != 0x13
                                && building.queued_mission != 0x13),
                        "SELL began before retained event execution"
                    );
                }
            }
            if let Some(building) = &current.building {
                if let Some(head) = observation.head {
                    let factory = current
                        .factory
                        .as_ref()
                        .context("on-map seller lost its Factory")?;
                    ensure!(
                        factory.head == Some(head)
                            && factory.progress < crate::sim::production::PRODUCTION_STEPS
                            && sim
                                .entities()
                                .get(head)
                                .is_some_and(|entity| entity.in_limbo()),
                        "unfinished sale head changed or escaped before actual map exit"
                    );
                }
                if observation.execution.is_some() {
                    ensure!(
                        !building.in_limbo
                            && !building.repairing
                            && building.control == rules.buildup_control("GAPILE"),
                        "seller lost bound Buildup or resumed repair"
                    );
                    let birth = observation.head_birth.as_ref().context("sale head birth")?;
                    ensure!(
                        current.factory.as_ref().map(|factory| factory.order)
                            == birth.factory.as_ref().map(|factory| factory.order),
                        "sale replaced its retained Factory"
                    );
                    let factory = current.factory.as_ref().context("live Selling Factory")?;
                    ensure!(
                        factory.running
                            && !factory.suspended
                            && observation
                                .execution
                                .as_ref()
                                .and_then(|snapshot| snapshot.factory.as_ref())
                                .is_some_and(|at_sale| at_sale.queue == factory.queue),
                        "Selling suspended the unfinished Factory or changed its queued successor early"
                    );
                    let changed = observation
                        .phases
                        .last()
                        .and_then(|snapshot| snapshot.building.as_ref())
                        .is_none_or(|previous| {
                            previous.status != building.status
                                || previous.body != building.body
                                || previous.ready != building.ready
                        });
                    if changed {
                        if let Some(previous) = observation
                            .phases
                            .last()
                            .and_then(|snapshot| snapshot.building.as_ref())
                        {
                            ensure!(
                                building.status == previous.status
                                    || building.status == previous.status + 1,
                                "Selling skipped a genuine mission stage"
                            );
                        }
                        observation.phases.push(current.clone());
                    }
                    if observation.advanced.is_none()
                        && building.status == 2
                        && building.body == Some(0)
                        && building.rate > 0
                        && building.stage > building.control[0]
                    {
                        observation.advanced = Some(current.clone());
                    }
                    if building.status == 2 && observation.crew.is_empty() {
                        observation.crew = sim
                            .entities()
                            .values_sorted()
                            .filter(|entity| {
                                entity.owner() == owner
                                    && sim.interner().resolve(entity.type_ref()) == crew_type
                                    && !observation.before_e1.contains(&entity.stable_id())
                            })
                            .map(|entity| entity.stable_id())
                            .collect();
                        ensure!(
                            Some(observation.crew.len()) == observation.crew_count,
                            "stage1 sale crew differs from owner count or paid identities"
                        );
                        for &id in &observation.crew {
                            let crew = sim.entities().get(id).context("new sale crew")?;
                            ensure!(
                                crew.health.current() == crew_strength
                                    && crew.estimated_health.get() == crew_strength
                                    && crew.is_alive()
                                    && !crew.in_limbo()
                                    && sim.logic_order().contains(&id)
                                    && sim.display_layers().layer_of(id).is_some()
                                    && sim.occupancy().contains_entity(
                                        crew.position.rx(),
                                        crew.position.ry(),
                                        id
                                    ),
                                "sale crew did not enter normally at full health"
                            );
                            observation
                                .crew_birth
                                .push(sale_crew_snapshot(state, id, frame)?);
                        }
                    }
                }
                if let Some(previous) = &observation.previous {
                    let charge = current.spent - previous.spent;
                    ensure!(
                        charge >= 0 && current.credits == previous.credits - charge,
                        "on-map sale Factory refunded early or charged wrong wallet"
                    );
                    if charge > 0 {
                        observation
                            .charges
                            .push(json!({"logic_frame":frame-1,"before":previous,"after":current}));
                    }
                }
            } else if observation.execution.is_some() && observation.exited.is_none() {
                let previous = observation
                    .previous
                    .as_ref()
                    .context("last live seller snapshot")?;
                let baseline = observation
                    .baseline
                    .as_ref()
                    .context("sale wallet baseline")?;
                let refund = observation.refund.context("live sale refund")?;
                ensure!(
                    previous
                        .building
                        .as_ref()
                        .is_some_and(|building| building.status == 2)
                        && observation.advanced.is_some()
                        && observation.phases.iter().any(|snapshot| snapshot
                            .building
                            .as_ref()
                            .is_some_and(|building| building.status == 1)),
                    "seller vanished without complete mission/Buildup stages"
                );
                ensure!(
                    observation
                        .charges
                        .iter()
                        .any(|charge| charge["after"]["building"]["status"].as_u64() == Some(2)),
                    "paid Factory did not keep charging through reverse buildup"
                );
                ensure!(
                    current.credits == baseline.credits + refund
                        && current.spent == previous.spent
                        && current.credits - previous.credits
                            == refund + (current.spent - baseline.spent),
                    "physical sale exit did not independently refund building and all paid-head charges"
                );
                ensure!(
                    current.tracking_buildings == baseline.tracking_buildings - 1,
                    "sale failed to remove exactly one tracked building"
                );
                ensure!(
                    current.buildings_lost == baseline.buildings_lost,
                    "ordinary sale counted a building loss"
                );
                observation.exited = Some(current.clone());
            }
            if let Some(exited) = &observation.exited {
                ensure!(
                    current.building.is_none()
                        && current.factory.is_none()
                        && observation
                            .head
                            .is_some_and(|head| !sim.entities().contains(head)
                                && !sim.logic_order().contains(&head)
                                && sim.display_layers().layer_of(head).is_none()
                                && !sim.object_is_pending_delete(head))
                        && !current.in_logic
                        && current.display_layer.is_none()
                        && !current.pending_delete
                        && current.occupied_cells.is_empty()
                        && current.producer_ids.is_empty(),
                    "sale left producer/head/FIFO/occupancy/Logic/display/delete/producer membership"
                );
                ensure!(
                    current.credits == exited.credits
                        && current.spent == exited.spent
                        && current.tracking_buildings == exited.tracking_buildings
                        && current.buildings_lost == exited.buildings_lost,
                    "sale tail changed refunded cash/tracking twice"
                );
                ensure!(
                    !crate::sim::production::build_options_for_owner(sim, rules, &current.owner)
                        .iter()
                        .any(|option| sim.interner().resolve(option.type_id) == "E1"
                            && option.enabled),
                    "E1 remained enabled without a producer"
                );
                if sim.session().current_house() == Some(owner) && self.sale_local_cleanup.is_none()
                {
                    let e1 = sim.interner().get("E1").context("sale E1")?;
                    let input = &state.match_state.input;
                    let linked = state
                        .match_state
                        .match_presentation
                        .sidebar_projection
                        .cameo_factory(sim, owner, e1)
                        .is_some();
                    let cameo_present = state
                        .match_state
                        .match_presentation
                        .sidebar_projection
                        .cameo_strips()
                        .items(crate::ui::sidebar::SidebarTab::for_category(
                            ProductionCategory::Infantry,
                        ))
                        .iter()
                        .any(|cameo| cameo.id == CameoId::Object(e1));
                    ensure!(
                        !projected_selection(state).contains(&producer)
                            && !input.selection_order.contains(&producer)
                            && !linked
                            && !state
                                .match_state
                                .match_presentation
                                .sidebar_gadget_state
                                .sell_mode_on,
                        "local sale selection/cameo/mode cleanup failed"
                    );
                    // Retention follows the existing House/sidebar recheck;
                    // observe its actual disappearance without advancing it.
                    if !cameo_present {
                        self.sale_local_cleanup = Some(
                            json!({"native_frame":frame,"producer":producer,
                            "selected":projected_selection(state),"selection_ledger":input.selection_order,
                            "cameo_factory":linked,"cameo_present":cameo_present,"sell_mode":false}),
                        );
                    }
                }
            }
            for (crew_index, &id) in observation.crew.iter().enumerate() {
                let crew = sim.entities().get(id).context("retained sale crew")?;
                ensure!(
                    crew.is_alive()
                        && !crew.in_limbo()
                        && crew.owner() == owner
                        && crew.health.current() == crew_strength,
                    "sale crew died, changed owner or lost full health"
                );
                let born = &observation.crew_birth[crew_index];
                if !observation
                    .crew_walk
                    .iter()
                    .any(|snapshot| snapshot["id"].as_u64() == Some(id))
                    && json!(crew.position) != born["position"]
                    && crew
                        .locomotor
                        .as_ref()
                        .is_some_and(|walk| walk.walk_is_moving() == Some(true))
                {
                    observation
                        .crew_walk
                        .push(sale_crew_snapshot(state, id, frame)?);
                }
                if observation
                    .crew_walk
                    .iter()
                    .any(|snapshot| snapshot["id"].as_u64() == Some(id))
                    && !observation
                        .crew_settled
                        .iter()
                        .any(|snapshot| snapshot["id"].as_u64() == Some(id))
                    && crew.navigation.nav_com().is_none()
                    && crew
                        .locomotor
                        .as_ref()
                        .is_some_and(|walk| walk.walk_is_moving() == Some(false))
                {
                    observation
                        .crew_settled
                        .push(sale_crew_snapshot(state, id, frame)?);
                }
            }
            if observation.reentry_arrival.is_none()
                && let Some(record) = observation.reentry
                && let MegaMissionOrder::Move { target_x, target_y } = record.order
            {
                let gi = sim
                    .entities()
                    .get(u64::try_from(record.source_id)?)
                    .context("foundation reentry GI")?;
                if frame > u32::try_from(record.frame)?
                    && gi.navigation.nav_com().is_none()
                    && (gi.position.rx(), gi.position.ry())
                        == (u16::try_from(target_x)?, u16::try_from(target_y)?)
                {
                    observation.reentry_arrival =
                        Some(sale_crew_snapshot(state, gi.stable_id(), frame)?);
                }
            }
            if observation.stable.is_none()
                && let Some(arrival) = &observation.reentry_arrival
                && arrival["native_frame"]
                    .as_u64()
                    .is_some_and(|arrived| u64::from(frame) >= arrived + 60)
            {
                observation.stable = Some(current.clone());
            }
            observation.previous = Some(current);
        }
        Ok(())
    }

    fn observe_commands(&mut self, state: &AppState, frame: u32) -> Result<()> {
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
                        !matches!(
                            &command.payload,
                            Command::PlaceProducedMobile { .. }
                                | Command::Attack { .. }
                                | Command::SetRally { .. }
                                | Command::ArchiveObject { .. }
                                | Command::ToggleRepair { .. }
                                | Command::SellBuilding { .. }
                        ),
                        "live Battle admitted a semantic mobile PLACE, Attack, rally, Repair or Sell"
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
                    && produce.rtti == gi.rtti
                    && (produce.type_index == gi.type_index || produce.type_index == -1)
                {
                    ensure!(produce.naval == 0, "GI PRODUCE carried Naval");
                    let index = human_index_for_house(state, produce.house_id)?;
                    // Admission can precede this peer's observation of the
                    // completed repair. The first two PRODUCE events belong
                    // to the opening; later ones start the sale queue.
                    // observe_sale checks repair completion at their due frame.
                    let initial_produces = self
                        .gi_produces
                        .iter()
                        .filter(|event| event.house_id == produce.house_id)
                        .count();
                    let sale_queue = self.sale_enabled
                        && (!self.sale[index].produces.is_empty()
                            || (produce.event == ProductionEvent::Produce
                                && initial_produces == 2));
                    if sale_queue {
                        let observation = &mut self.sale[index];
                        if produce.event == ProductionEvent::Produce {
                            ensure!(
                                produce.type_index == gi.type_index
                                    && observation.produces.len() < 2,
                                "extra or wildcard sale-queue PRODUCE"
                            );
                            observation.produces.push(produce);
                        }
                        observation.events.push(json!({"kind":format!("{:?}",produce.event),
                            "admission_tick":tick.tick,"admission_observed_frame":frame,
                            "event_frame":produce.frame,"opcode":record.opcode(),"house":produce.house_id,
                            "bytes":record.as_bytes().as_slice()}));
                    } else if produce.event == ProductionEvent::Produce {
                        ensure!(
                            produce.type_index == gi.type_index,
                            "initial GI PRODUCE wildcard"
                        );
                        self.gi_produces.push(produce);
                        ensure!(
                            self.gi_produces
                                .iter()
                                .filter(|event| event.house_id == produce.house_id)
                                .count()
                                <= 2,
                            "a House admitted an extra initial GI PRODUCE"
                        );
                        self.gi_events.push(json!({"admission_tick":tick.tick,"kind":"PRODUCE","house":produce.house_id,"issue_frame":produce.frame,"bytes":record.as_bytes().as_slice()}));
                    }
                }
                if self.sale_enabled
                    && let Some(sell) = ObjectActionRecord::decode(record)
                        .filter(|event| event.action == ObjectAction::Sell)
                {
                    let index = human_index_for_house(state, sell.house_id)?;
                    ensure!(
                        self.rally_records[index]
                            .is_some_and(|archive| archive.entity_id == sell.entity_id),
                        "raw SELL did not name this Human's original paid GAPILE"
                    );
                    let observation = &mut self.sale[index];
                    ensure!(
                        observation.record.is_none()
                            && observation.produces.len() == 2
                            && observation.produces[1].frame <= sell.frame,
                        "SELL preceded ordered sale head+successor or was duplicated"
                    );
                    observation.record = Some(sell);
                    observation
                        .events
                        .push(json!({"kind":"SELL","admission_tick":tick.tick,
                        "admission_observed_frame":frame,"event_frame":sell.frame,
                        "opcode":record.opcode(),"source":sell.entity_id,"house":sell.house_id,
                        "bytes":record.as_bytes().as_slice()}));
                }
                if let Some(archive) = ArchiveRecord::decode(record) {
                    let sim = simulation(state)?;
                    let source = sim
                        .entities()
                        .get(u64::try_from(archive.entity_id)?)
                        .context("admitted ARCHIVE source")?;
                    if sim.interner().resolve(source.type_ref()) == "GAPILE" {
                        let index =
                            battle_human_index(&state.match_state.session, sim, source.owner())?;
                        ensure!(
                            archive.house_id == house_id_for_owner(state, source.owner())?,
                            "raw GAPILE ARCHIVE issuing House differs from producer"
                        );
                        ensure!(
                            self.rally_records[index].is_none(),
                            "duplicate manual GAPILE ARCHIVE"
                        );
                        let MegaMissionTarget::Cell { x, y } = archive.target else {
                            anyhow::bail!("manual GAPILE rally did not carry a prepared Cell");
                        };
                        let prepared = (u16::try_from(x)?, u16::try_from(y)?);
                        if sim.session().current_house() == Some(source.owner()) {
                            ensure!(
                                self.rally_prepared == Some(prepared)
                                    && self.barracks_product == Some(source.stable_id()),
                                "local raw ARCHIVE differs from clicked GAPILE/prepared target"
                            );
                        }
                        self.rally_records[index] = Some(archive);
                        self.rally_events.push(json!({"admission_tick":tick.tick,"admission_observed_frame":frame,
                            "event_frame":archive.frame,"opcode":record.opcode(),"source":archive.entity_id,
                            "house":archive.house_id,"prepared":prepared,"bytes":record.as_bytes().as_slice()}));
                    }
                }
                // The real Planning key release reaches PlanMgr637A10 and
                // encode_plan_commit_record. Keep its ordinary raw2B after
                // this House's raw1E; an empty commit must still cross LAN.
                if record.opcode() == 0x2b {
                    let sim = simulation(state)?;
                    let owner = *sim
                        .houses()
                        .order()
                        .get(usize::try_from(record.house_id())?)
                        .context("PLAN_COMMIT issuing House")?;
                    let index = battle_human_index(&state.match_state.session, sim, owner)?;
                    let archive = self.rally_records[index]
                        .context("PLAN_COMMIT admitted before its House's manual ARCHIVE")?;
                    ensure!(
                        self.rally_planning_commits[index].is_none(),
                        "duplicate rally PLAN_COMMIT"
                    );
                    ensure!(
                        record.frame_stamp() >= archive.frame
                            && record.house_id() == archive.house_id,
                        "PLAN_COMMIT frame/House preceded the manual ARCHIVE"
                    );
                    self.rally_planning_commits[index] = Some(json!({
                        "admission_tick":tick.tick,"admission_observed_frame":frame,
                        "event_frame":record.frame_stamp(),"opcode":record.opcode(),
                        "house":record.house_id(),"bytes":record.as_bytes().as_slice()}));
                }
                if let Some(repair) = RepairRecord::decode(record) {
                    let index = self
                        .rally_records
                        .iter()
                        .position(|archive| {
                            archive.is_some_and(|archive| archive.entity_id == repair.entity_id)
                        })
                        .context("raw REPAIR did not target an original paid GAPILE")?;
                    ensure!(
                        self.rally_records[index]
                            .is_some_and(|archive| archive.house_id == repair.house_id),
                        "manual REPAIR issuing House differs from its own paid GAPILE"
                    );
                    ensure!(
                        self.repair[index].stop_execution.is_some(),
                        "manual REPAIR preceded the friendly-fire Stop execution"
                    );
                    let observation = &mut self.repair[index];
                    ensure!(observation.records.len() < 3, "extra manual REPAIR toggle");
                    observation.records.push(repair);
                    observation.events.push(json!({"kind":"REPAIR",
                        "admission_tick":tick.tick,"admission_observed_frame":frame,
                        "event_frame":repair.frame,"opcode":record.opcode(),
                        "source":repair.entity_id,"house":repair.house_id,
                        "bytes":record.as_bytes().as_slice()}));
                }
                let [Some(actor), Some(target)] = self.fighters else {
                    continue;
                };
                if let Some(mission) = MegaMissionRecord::decode(record)
                    && [actor, target].contains(&(mission.source_id as u64))
                {
                    let index = usize::from(mission.source_id == i32::try_from(target)?);
                    let entity = simulation(state)?
                        .entities()
                        .get([actor, target][index])
                        .context("ordinary GI Move/Attack source")?;
                    ensure!(
                        mission.house_id == house_id_for_owner(state, entity.owner())?,
                        "raw GI Move/Attack issuing House differs from its source"
                    );
                    if self.staging[index].is_none()
                        && let MegaMissionOrder::Move { target_x, target_y } = mission.order
                    {
                        self.staging[index] =
                            Some((u16::try_from(target_x)?, u16::try_from(target_y)?));
                    }
                    if self.sale[index].exited.is_some()
                        && let MegaMissionOrder::Move { target_x, target_y } = mission.order
                    {
                        let goal = (u16::try_from(target_x)?, u16::try_from(target_y)?);
                        ensure!(
                            self.sale[index].reentry.is_none()
                                && self.sale[index].cells.contains(&goal),
                            "extra Move or reentry destination outside the sold foundation"
                        );
                        if mission.house_id == local_house_id(state)? {
                            ensure!(
                                self.sale_local_move == Some(goal),
                                "raw reentry differs from actual local Cell click"
                            );
                        }
                        self.sale[index].reentry = Some(mission);
                        self.sale[index].events.push(json!({"kind":"REENTRY","admission_tick":tick.tick,
                            "admission_observed_frame":frame,"event_frame":mission.frame,
                            "opcode":record.opcode(),"source":mission.source_id,"house":mission.house_id,
                            "bytes":record.as_bytes().as_slice()}));
                    }
                    if self.rally_records[index].is_some_and(|archive| {
                        mission.order
                            == (MegaMissionOrder::Attack {
                                target: MegaMissionTarget::Object {
                                    id: archive.entity_id,
                                },
                            })
                    }) {
                        ensure!(
                            self.stop_observation.is_some(),
                            "friendly force-fire preceded opposing-GI Stop"
                        );
                        let observation = &mut self.repair[index];
                        ensure!(
                            observation.attack.is_none(),
                            "duplicate friendly force-fire"
                        );
                        observation.attack = Some(mission);
                        observation.events.push(json!({"kind":"FORCE_FIRE",
                            "admission_tick":tick.tick,"admission_observed_frame":frame,
                            "event_frame":mission.frame,"opcode":record.opcode(),
                            "source":mission.source_id,"house":mission.house_id,
                            "bytes":record.as_bytes().as_slice()}));
                    }
                    self.combat_events.push(json!({"admission_tick":tick.tick,"admission_observed_frame":frame,
                        "event_frame":mission.frame,
                        "opcode":record.opcode(),"source":mission.source_id,"house":mission.house_id,
                        "order":format!("{:?}",mission.order),"bytes":record.as_bytes().as_slice()}));
                    if mission.source_id == i32::try_from(actor)?
                        && mission.order
                            == (MegaMissionOrder::Attack {
                                target: MegaMissionTarget::Object {
                                    id: i32::try_from(target)?,
                                },
                            })
                    {
                        ensure!(
                            self.attack_observation.is_none() && self.pending_attack.is_none(),
                            "duplicate manual GI Attack"
                        );
                        let sim = simulation(state)?;
                        let entity = sim.entities().get(actor).context("manual Attack actor")?;
                        ensure!(
                            mission.house_id == house_id_for_owner(state, entity.owner())?,
                            "raw Attack issuing House differs from the Host"
                        );
                        if state.match_state.session.host() {
                            ensure!(
                                self.attack_input_frame
                                    .is_some_and(|issued| mission.frame >= issued as i32),
                                "Attack did not follow the actual object click"
                            );
                        }
                        self.pending_attack = Some(mission);
                    }
                }
                if let Some(stop) = StopRecord::decode(record) {
                    let index = [actor, target]
                        .iter()
                        .position(|&id| id == stop.entity_id as u64);
                    if let Some(index) = index
                        && self.repair[index].attack.is_some()
                    {
                        let entity = simulation(state)?
                            .entities()
                            .get([actor, target][index])
                            .context("friendly-fire GI")?;
                        ensure!(
                            stop.house_id == house_id_for_owner(state, entity.owner())?,
                            "friendly-fire Stop House differs from its GI"
                        );
                        let observation = &mut self.repair[index];
                        ensure!(observation.stop.is_none(), "duplicate friendly-fire Stop");
                        observation.stop = Some(stop);
                        observation.events.push(json!({"kind":"STOP",
                            "admission_tick":tick.tick,"admission_observed_frame":frame,
                            "event_frame":stop.frame,"opcode":record.opcode(),
                            "source":stop.entity_id,"house":stop.house_id,
                            "bytes":record.as_bytes().as_slice()}));
                    }
                    if stop.entity_id != i32::try_from(actor)? || self.stop_observation.is_some() {
                        continue;
                    }
                    ensure!(
                        self.stop_observation.is_none() && self.pending_stop.is_none(),
                        "duplicate Host GI Stop"
                    );
                    let entity = simulation(state)?
                        .entities()
                        .get(actor)
                        .context("stopped GI")?;
                    ensure!(
                        stop.house_id == house_id_for_owner(state, entity.owner())?,
                        "raw Stop issuing House differs from the Host"
                    );
                    self.combat_events.push(
                        json!({"admission_tick":tick.tick,"admission_observed_frame":frame,
                        "event_frame":stop.frame,
                        "opcode":record.opcode(),"source":stop.entity_id,"house":stop.house_id,
                        "bytes":record.as_bytes().as_slice()}),
                    );
                    self.pending_stop = Some(stop);
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
        if self.sale_enabled
            && self.gi_products.len() == 2
            && self
                .gi_products
                .iter()
                .all(|product| product.rally_arrival.is_some())
        {
            let sim = simulation(state)?;
            let owner = sim
                .interner()
                .get(&local_owner(state)?)
                .context("original paid-GI House")?;
            for product in &self.gi_products {
                let entity = sim
                    .entities()
                    .get(product.id)
                    .context("original paid GI after output phase")?;
                ensure!(
                    entity.is_alive()
                        && !entity.in_limbo()
                        && entity.owner() == owner
                        && sim.interner().resolve(entity.type_ref()) == "E1",
                    "original paid GI was lost or replaced"
                );
            }
            return Ok(());
        }
        let factory = factory(state, ProductionCategory::Infantry)?;
        let sim = simulation(state)?;
        let owner = sim
            .interner()
            .get(&local_owner(state)?)
            .context("local House")?;
        let rally = self.rally_prepared.context("executed local rally")?;
        ensure!(
            sim.entities()
                .get(self.barracks_product.context("paid barracks")?)
                .context("rally producer")?
                .archive_target()
                == Some(TargetKind::Cell(rally.0, rally.1)),
            "GAPILE lost its rally while the paid GIs consumed their own Archives"
        );
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
                    rally_inherited: None,
                    rally_handoff: None,
                    rally_walk: None,
                    rally_arrival: None,
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
                // Exit44499C copies Archive at PLACE, then444CE7 retains the
                // rally while the GI walks to its dock; Begin alone does not.
                ensure!(
                    entity.archive_target() == Some(TargetKind::Cell(rally.0, rally.1))
                        && entity.navigation.nav_com().is_some(),
                    "paid GI did not inherit the producer's rally at automatic PLACE"
                );
                product.delivered_frame = Some(frame);
                product.rally_inherited = Some(gi_rally_snapshot(entity, frame));
                actions.push(json!({"native_frame":frame,"observation":"same paid GI delivered by automatic PLACE","id":product.id,"position":entity.position}));
            }
            ensure!(
                product.delivered_frame.is_none() || !entity.in_limbo(),
                "delivered GI returned to limbo"
            );
            let walk = entity.locomotor.as_ref().context("paid GI Walk")?;
            let destination = Some(crate::sim::components::NavTargetRef::cell(rally.0, rally.1));
            if product.delivered_frame.is_some()
                && product.rally_handoff.is_none()
                && entity.archive_target().is_none()
                && entity.navigation.nav_com() == destination
                && walk.walk_is_moving() == Some(true)
            {
                let snapshot = gi_rally_snapshot(entity, frame);
                actions.push(json!({"native_frame":frame,"observation":"paid GI naturally consumed Archive into rally navigation","gi":snapshot}));
                product.rally_handoff = Some(snapshot);
            }
            if product.rally_walk.is_none()
                && let Some(handoff) = &product.rally_handoff
                && handoff["native_frame"]
                    .as_u64()
                    .is_some_and(|start| u64::from(frame) > start)
                && json!(entity.position) != handoff["position"]
                && entity.navigation.nav_com() == destination
                && walk.walk_is_moving() == Some(true)
                && walk.step_head().is_some()
            {
                product.rally_walk = Some(gi_rally_snapshot(entity, frame));
            }
            if product.rally_arrival.is_none()
                && product.rally_walk.is_some()
                && (entity.position.rx(), entity.position.ry()) == rally
                && entity.archive_target().is_none()
                && entity.navigation.nav_com().is_none()
                && walk.walk_is_moving() == Some(false)
                && entity.mission.current().raw() == 5
            {
                let snapshot = gi_rally_snapshot(entity, frame);
                actions.push(json!({"native_frame":frame,"observation":"paid GI naturally arrived at rally in Guard with cleared Archive/navigation","gi":snapshot}));
                product.rally_arrival = Some(snapshot);
            }
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
            && self.rally_executions.iter().all(Option::is_some)
            && self.rally_planning_commits.iter().all(Option::is_some)
            && self.gi_products.len() == 2
            && self
                .gi_products
                .iter()
                .all(|product| product.rally_arrival.is_some())
            && self.attack_observation.is_some()
            && self.hit_observation.is_some()
            && self.stop_observation.is_some()
            && self.repair.iter().all(|observation| {
                observation.records.len() == 3
                    && observation.executions.len() == 3
                    && observation.damage.is_some()
                    && observation.stop_execution.is_some()
                    && observation.stopped.is_some()
                    && observation.full.is_some()
            })
            && (!self.sale_enabled
                || (self.sale.iter().all(|observation| {
                    observation.produces.len() == 2
                        && observation.record.is_some()
                        && observation.execution.is_some()
                        && observation.exited.is_some()
                        && observation.stable.is_some()
                        && observation.crew_count == Some(observation.crew_settled.len())
                        && observation.reentry_arrival.is_some()
                }) && self.sale_local_input.is_some()
                    && self.sale_local_cleanup.is_some()))
            && [
                ("GAPOWR", 2),
                ("GAPILE", if self.sale_enabled { 0 } else { 2 }),
            ]
            .iter()
            .all(|(type_id, count)| {
                sim.entities()
                    .values_sorted()
                    .filter(|entity| {
                        entity.is_alive()
                            && !entity.in_limbo()
                            && !entity.in_construction_bstate()
                            && sim.interner().resolve(entity.type_ref()) == *type_id
                    })
                    .count()
                    == *count
            })
            && self.all_gi_orders_admitted(state)?)
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        let sim = simulation(state)?;
        let mut receipt = json!({"phase":self.phase(),"mcv":self.mcv,"yard":self.yard,
            "cancelled_product":self.first_product,"placed_product":self.placed_product,
            "owner":local_owner(state)?,"barracks_product":self.barracks_product,
            "starting_credits":self.starting_credits,"final_credits":self.final_credits,
            "gi_wallet_before":self.gi_wallet,"gi_cost":self.gi_cost,"gi_factory_order":self.gi_factory_order,
            "gi_products":self.gi_products.iter().map(|product| json!({"id":product.id,"first_progress":product.first_progress,
                "completed_frame":product.completed_frame,"delivered_frame":product.delivered_frame,
                "rally_inherited":product.rally_inherited,"rally_handoff":product.rally_handoff,
                "rally_walk":product.rally_walk,"rally_arrival":product.rally_arrival})).collect::<Vec<_>>(),
            "rally":{"input":self.rally_input,"native_events":self.rally_events,
                "executions":self.rally_executions,"planning_commits":self.rally_planning_commits},
            "successor_promotions":self.successor_promotions,"gi_native_events":self.gi_events,
            "combat":{"fighters":self.fighters,"staging":self.staging,
                "attack_input_frame":self.attack_input_frame,"attack":self.attack_observation,
                "hit":self.hit_observation,"stop":self.stop_observation,"native_events":self.combat_events,
                "final":combat_snapshot(state,self.fighter_ids()?,sim.session().binary_frame())?},
            "repair":{"period":repair_period(state)?,"episodes":self.repair.iter().map(|observation|
                json!({"native_events":observation.events,"attack":observation.attack_execution,
                    "damage":observation.damage,"stop":observation.stop_execution,
                    "executions":observation.executions,"payments":observation.payments,
                    "stopped":observation.stopped,"full":observation.full,
                    "episode_end":observation.previous})).collect::<Vec<_>>()},
            "infantry":sim.entities().values_sorted().filter(|entity| sim.interner().resolve(entity.type_ref()) == "E1")
                .map(|entity| json!({"id":entity.stable_id(),"owner":sim.interner().resolve(entity.owner()),
                    "alive":entity.is_alive(),"in_limbo":entity.in_limbo(),"position":entity.position,
                    "health":entity.health.current()})).collect::<Vec<_>>(),
            "buildings":sim.entities().values_sorted().filter(|entity|
                entity.is_alive() && !entity.in_limbo() && entity.category() == crate::map::entities::EntityCategory::Structure
            ).map(|entity| json!({"id":entity.stable_id(),"owner":sim.interner().resolve(entity.owner()),
                "type":sim.interner().resolve(entity.type_ref()),"position":entity.position})).collect::<Vec<_>>(),
            "houses":sim.houses().order().iter().filter_map(|owner| sim.houses().get(owner).map(|house|
                json!({"owner":sim.interner().resolve(*owner),"credits":house.economy().credits(),"spent":house.economy().spent_credits(),
                    "power":sim.power_states().get(owner).map(|power| (power.total_output(),power.total_drain()))})
            )).collect::<Vec<_>>() });
        if self.sale_enabled {
            receipt["sale"] = json!({"local_input":self.sale_local_input,
                "local_cleanup":self.sale_local_cleanup,"local_move":self.sale_local_move,
                "episodes":self.sale});
        }
        Ok(receipt)
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

fn repair_period(state: &AppState) -> Result<u32> {
    let rules = state.rules().context("retail repair rules")?;
    let period = u32::try_from(crate::sim::combat::veterancy::self_heal_interval_frames(
        rules.general.repair_rate_minutes,
    ))?;
    ensure!(period != 0, "retail repair period is zero");
    Ok(period)
}

fn repair_snapshot(state: &AppState, id: u64, frame: u32) -> Result<RepairSnapshot> {
    let sim = simulation(state)?;
    let building = sim
        .entities()
        .get(id)
        .context("repair observation GAPILE")?;
    let economy = sim
        .houses()
        .get(&building.owner())
        .context("repair House")?
        .economy();
    Ok(RepairSnapshot {
        native_frame: frame,
        id,
        owner: sim.interner().resolve(building.owner()).into(),
        health: building.health.current(),
        estimated_health: building.estimated_health.get(),
        repairing: building.repairing.is_on(),
        credits: economy.credits(),
        spent: economy.spent_credits(),
    })
}

fn human_index_for_house(state: &AppState, house: i8) -> Result<usize> {
    let sim = simulation(state)?;
    let owner = *sim
        .houses()
        .order()
        .get(usize::try_from(house)?)
        .context("record Human House")?;
    battle_human_index(&state.match_state.session, sim, owner)
}

fn sale_snapshot(
    state: &AppState,
    index: usize,
    producer: u64,
    cells: &[(u16, u16)],
    frame: u32,
) -> Result<SaleSnapshot> {
    let sim = simulation(state)?;
    let rules = state.rules().context("retail sale rules")?;
    let owner = battle_owners(&state.match_state.session, sim)?[index];
    let name = sim.interner().resolve(owner);
    let house = sim.houses().get(&owner).context("sale House")?;
    if let Some(entity) = sim.entities().get(producer) {
        ensure!(
            entity.owner() == owner && sim.interner().resolve(entity.type_ref()) == "GAPILE",
            "original seller changed owner or type"
        );
    }
    let view = sim
        .production()
        .factories()
        .view(owner, ProductionCategory::Infantry);
    let factory = if let Some(view) = view {
        let factory = sim
            .production()
            .factories()
            .iter_insertion_ordered()
            .into_iter()
            .find(|factory| {
                factory.owner() == owner
                    && factory.category() == ProductionCategory::Infantry
                    && factory.insertion_seq() == view.construction_order()
            })
            .context("retained sale Factory identity")?;
        Some(SaleFactorySnapshot {
            order: factory.insertion_seq(),
            head: factory.object().and_then(|object| object.entity_id),
            progress: factory.progress(),
            balance: factory.balance(),
            running: view.is_running(),
            suspended: factory.suspended(),
            rate: factory.step_rate_frames(),
            timer_start: factory.step_timer().start_frame(),
            timer_duration: factory.step_timer().duration(),
            queue: factory
                .queue()
                .iter()
                .map(|entry| {
                    (
                        sim.interner().resolve(entry.type_id).into(),
                        entry.enqueue_order,
                    )
                })
                .collect(),
        })
    } else {
        None
    };
    let building = sim
        .entities()
        .get(producer)
        .map(|building| SaleBuildingSnapshot {
            health: building.health.current(),
            estimated_health: building.estimated_health.get(),
            repairing: building.repairing.is_on(),
            in_limbo: building.in_limbo(),
            mission: building.mission.current().raw(),
            queued_mission: building.mission.queued().raw(),
            status: building.mission.handler_state(),
            body: building.building_body_state(),
            queued_body: building.queued_building_body_state(),
            ready: building.building_ready_latch(),
            control: building.building_construction_control(),
            stage: building.construction_stage_value(),
            rate: building.native_stage().rate(),
        });
    Ok(SaleSnapshot {
        native_frame: frame,
        producer,
        owner: name.into(),
        credits: house.economy().credits(),
        spent: house.economy().spent_credits(),
        tracking_buildings: house.tracking().buildings(),
        buildings_lost: house.stats().buildings_lost(),
        building,
        factory,
        producer_ids: crate::sim::production::producer_candidates_for_owner_category(
            sim.entities(),
            rules,
            name,
            ProductionCategory::Infantry,
            true,
            sim.interner(),
        )
        .into_iter()
        .map(|(id, _, _, _)| id)
        .collect(),
        occupied_cells: cells
            .iter()
            .copied()
            .filter(|&(x, y)| sim.occupancy().contains_entity(x, y, producer))
            .collect(),
        in_logic: sim.logic_order().contains(&producer),
        display_layer: sim
            .display_layers()
            .layer_of(producer)
            .map(|layer| layer.index()),
        pending_delete: sim.object_is_pending_delete(producer),
    })
}

fn sale_crew_snapshot(state: &AppState, id: u64, frame: u32) -> Result<Value> {
    let sim = simulation(state)?;
    let entity = sim.entities().get(id).context("sale crew")?;
    Ok(
        json!({"native_frame":frame,"id":id,"owner":sim.interner().resolve(entity.owner()),
        "type":sim.interner().resolve(entity.type_ref()),"health":entity.health.current(),
        "estimated_health":entity.estimated_health.get(),"nominal":entity.is_nominal_survivor(),
        "in_limbo":entity.in_limbo(),"alive":entity.is_alive(),"position":entity.position,
        "mission":entity.mission.current().raw(),"queued_mission":entity.mission.queued().raw(),
        "destination":entity.navigation.nav_com(),"in_logic":sim.logic_order().contains(&id),
        "display_layer":sim.display_layers().layer_of(id).map(|layer|layer.index()),
        "occupied":sim.occupancy().contains_entity(entity.position.rx(),entity.position.ry(),id),
        "walk":gi_rally_snapshot(entity,frame)}),
    )
}

/// Host/guest are input roles, never literal account names or native House
/// positions. Resolve through the same Session→House owner as real transport;
/// LAN's local-first admission and online roster order may differ.
fn battle_owners(
    session: &crate::app::match_runtime::session::MatchSession,
    sim: &Simulation,
) -> Result<[crate::sim::intern::InternedId; 2]> {
    let launch = session.network_setup().context("Battle network setup")?;
    ensure!(
        launch.humans.len() == 2 && launch.local_player_index < 2,
        "Battle opening requires two admitted Humans"
    );
    let local = launch.local_player_index;
    let remote = 1 - local;
    let sources = if session.host() {
        [local, remote]
    } else {
        [remote, local]
    };
    let owners = sources.map(|source| {
        let house = crate::app::match_runtime::network_match::source_house(session, sim, source)
            .context("Battle participant's canonical House")?;
        sim.houses()
            .order()
            .get(usize::try_from(house)?)
            .copied()
            .context("Battle native House")
    });
    let [host, guest] = owners;
    let [host, guest] = [host?, guest?];
    ensure!(host != guest, "Battle participants share a House");
    Ok([host, guest])
}

fn battle_human_index(
    session: &crate::app::match_runtime::session::MatchSession,
    sim: &Simulation,
    owner: crate::sim::intern::InternedId,
) -> Result<usize> {
    battle_owners(session, sim)?
        .iter()
        .position(|candidate| *candidate == owner)
        .context("command belongs to an unrepresented Battle Human")
}

fn local_house_id(state: &AppState) -> Result<i8> {
    let sim = simulation(state)?;
    let owner = sim
        .interner()
        .get(&local_owner(state)?)
        .context("local House")?;
    house_id_for_owner(state, owner)
}

fn house_id_for_owner(state: &AppState, owner: crate::sim::intern::InternedId) -> Result<i8> {
    let sim = simulation(state)?;
    Ok(i8::try_from(
        sim.houses()
            .order()
            .iter()
            .position(|candidate| *candidate == owner)
            .context("local native House index")?,
    )?)
}

fn combat_snapshot(state: &AppState, ids: [u64; 2], frame: u32) -> Result<Value> {
    let sim = simulation(state)?;
    let mut entities = Vec::new();
    for id in ids {
        let entity = sim.entities().get(id).context("paid encounter GI")?;
        entities.push(json!({"id":id,"owner":sim.interner().resolve(entity.owner()),
            "alive":entity.is_alive(),"position":entity.position,"health":entity.health.current(),
            "deployed":entity.is_deployed(),"deploying":entity.infantry_deploy_doing(),
            "mission":entity.mission.current().raw(),"queued_mission":entity.mission.queued().raw(),
            "target":entity.attack_target.target(),"destination":entity.navigation.nav_com(),
            "suspended_target":entity.suspended_attack_target.target(),
            "suspended_destination":entity.navigation.suspended_nav_com(),
            "nav_queue":entity.navigation.nav_queue(),
            "rearm_start":entity.rearm_timer.start_frame(),"rearm_duration":entity.rearm_timer.duration()}));
    }
    Ok(json!({"native_frame":frame,"entities":entities}))
}

fn gi_rally_snapshot(entity: &crate::sim::game_entity::GameEntity, frame: u32) -> Value {
    let walk = entity.locomotor.as_ref();
    json!({"native_frame":frame,"id":entity.stable_id(),"position":entity.position,
        "archive":entity.archive_target(),"destination":entity.navigation.nav_com(),
        "mission":entity.mission.current().raw(),"queued_mission":entity.mission.queued().raw(),
        "tether":entity.tether.partner(),"radio_partner":entity.radio_contacts.slot(0),
        "walk_moving":walk.and_then(|walk| walk.walk_is_moving()),
        "walk_destination":walk.and_then(|walk| walk.walk_destination()),
        "step_head":walk.and_then(|walk| walk.step_head())})
}

fn rally_eva_requests(state: &AppState) -> usize {
    state
        .match_state
        .match_audio
        .sound_events
        .iter()
        .filter(|event| {
            matches!(event,
        crate::audio::events::GameSoundEvent::Eva { event, .. }
        if event == crate::app::match_runtime::eva_producers::EVA_NEW_RALLY_POINT_ESTABLISHED)
        })
        .count()
}

fn rally_click(state: &AppState, producer: u64) -> Result<((u16, u16), (u16, u16))> {
    let sim = simulation(state)?;
    let source = sim
        .entities()
        .get(producer)
        .context("local rally producer")?;
    ensure!(
        sim.session().current_house() == Some(source.owner()),
        "diagnostic attempted a remote House's rally input query"
    );
    let origin = (source.position.rx(), source.position.ry());
    let rules = state.rules().context("retail rules")?;
    for (dx, dy) in [
        (6, 0),
        (0, 6),
        (-6, 0),
        (0, -6),
        (6, 3),
        (-6, -3),
        (3, 6),
        (-3, -6),
    ] {
        let (Ok(x), Ok(y)) = (
            u16::try_from(i32::from(origin.0) + dx),
            u16::try_from(i32::from(origin.1) + dy),
        ) else {
            continue;
        };
        let Some(cell) = state
            .resolved_terrain()
            .and_then(|terrain| terrain.cell(x, y))
        else {
            continue;
        };
        let world = crate::app::input::camera::cell_centre_world_point(x, y, cell.level);
        if crate::app::input::entity_pick::hover_object_at_point(
            crate::app::input::entity_pick::presented(state, sim),
            world.0,
            world.1,
            &local_owner(state)?,
            false,
        )
        .is_some()
        {
            continue;
        }
        let Some(prepared) = sim
            .factory_rally_cell_input(producer, (x, y), rules)
            .map_err(anyhow::Error::msg)?
        else {
            continue;
        };
        let distance = origin
            .0
            .abs_diff(prepared.0)
            .max(origin.1.abs_diff(prepared.1));
        if (4..=12).contains(&distance) {
            return Ok(((x, y), prepared));
        }
    }
    anyhow::bail!("no nearby ground rally click for GAPILE{producer}")
}

/// Diagnostic rendezvous geometry only; actual Cell admission/pathing remains
/// in the existing click owner and each ordinary synchronized Move receiver.
fn offset_toward(from: (u16, u16), toward: (u16, u16), cells: i32) -> (u16, u16) {
    let dx = i32::from(toward.0) - i32::from(from.0);
    let dy = i32::from(toward.1) - i32::from(from.1);
    let (x, y) = if dx.abs() > dy.abs() {
        (i32::from(from.0) + dx.signum() * cells, i32::from(from.1))
    } else {
        (i32::from(from.0), i32::from(from.1) + dy.signum() * cells)
    };
    (
        x.clamp(0, i32::from(u16::MAX)) as u16,
        y.clamp(0, i32::from(u16::MAX)) as u16,
    )
}

fn staging_centres(state: &AppState) -> Result<[(u16, u16); 2]> {
    let sim = simulation(state)?;
    let mut yards = [(0, 0); 2];
    for (index, owner) in battle_owners(&state.match_state.session, sim)?
        .into_iter()
        .enumerate()
    {
        let yard = sim
            .entities()
            .values_sorted()
            .find(|entity| {
                entity.owner() == owner
                    && entity.is_alive()
                    && !entity.in_limbo()
                    && sim.interner().resolve(entity.type_ref()) == "GACNST"
            })
            .context("encounter yard")?;
        yards[index] = (yard.position.rx(), yard.position.ry());
    }
    let middle = (
        ((u32::from(yards[0].0) + u32::from(yards[1].0)) / 2) as u16,
        ((u32::from(yards[0].1) + u32::from(yards[1].1)) / 2) as u16,
    );
    Ok([
        offset_toward(middle, yards[0], 6),
        offset_toward(middle, yards[1], 6),
    ])
}

fn legal_move_cell(state: &AppState, actor: u64, desired: (u16, u16)) -> Result<(u16, u16)> {
    let sim = simulation(state)?;
    let owner = sim.entities().get(actor).context("moving GI")?.owner();
    ensure!(
        sim.session().current_house() == Some(owner),
        "diagnostic attempted a remote House's local Move query"
    );
    let rules = state.rules().context("retail rules")?;
    for (dx, dy) in [
        (0, 0),
        (2, 0),
        (-2, 0),
        (0, 2),
        (0, -2),
        (2, 2),
        (-2, -2),
        (2, -2),
        (-2, 2),
    ] {
        let (Ok(x), Ok(y)) = (
            u16::try_from(i32::from(desired.0) + dx),
            u16::try_from(i32::from(desired.1) + dy),
        ) else {
            continue;
        };
        if let Some(cell) = commands::ordinary_cell_move_goal(
            sim,
            rules,
            state.overlay_registry(),
            owner,
            actor,
            (x, y),
            false,
        ) && cell == (x, y)
        {
            return Ok(cell);
        }
    }
    anyhow::bail!("no ordinary GI Move goal near {desired:?}")
}

fn mouse_can_pick(state: &AppState, id: u64) -> Result<bool> {
    let sim = simulation(state)?;
    let entity = sim.entities().get(id).context("opposing GI")?;
    let presented = crate::app::input::entity_pick::presented(state, sim);
    let world = presented.screen_position(entity);
    Ok(crate::app::input::entity_pick::hover_object_at_point(
        presented,
        world.0,
        world.1,
        &local_owner(state)?,
        false,
    )
    .and_then(|object| object.techno())
    .is_some_and(|target| target.stable_id == id))
}

fn ground_order_with_mouse(state: &mut AppState, id: u64, cell: (u16, u16)) -> Result<()> {
    select_entity(state, id)?;
    point_at_ground(state, cell)?;
    for edge in [ElementState::Pressed, ElementState::Released] {
        dispatch::handle_mouse_input(state, MouseButton::Left, edge);
    }
    Ok(())
}

/// The bounded live-network input for maps with preplaced mobile units. It uses
/// the same tactical selection, ground-click and key owners as the Battle route.
pub(super) fn mobile_move_with_mouse(
    state: &mut AppState,
) -> Result<(u64, (u16, u16), (u16, u16))> {
    let sim = simulation(state)?;
    let owner = sim.session().current_house().context("local House")?;
    let entity = sim
        .entities()
        .values_sorted()
        .filter(|entity| {
            entity.owner() == owner
                && entity.is_alive()
                && !entity.in_limbo()
                && entity.locomotor.is_some()
                && matches!(
                    entity.category(),
                    crate::map::entities::EntityCategory::Unit
                        | crate::map::entities::EntityCategory::Infantry
                )
        })
        // Real computer opponents can kill a starting infantryman before the
        // command observation ends. Prefer a durable local mobile object.
        .max_by_key(|entity| (entity.health.current(), entity.stable_id()))
        .context("local mobile object")?;
    let id = entity.stable_id();
    let from = (entity.position.rx(), entity.position.ry());
    let to = [(18, 0), (-18, 0), (0, 18), (0, -18)]
        .into_iter()
        .find_map(|(dx, dy)| {
            let desired = (
                u16::try_from(i32::from(from.0) + dx).ok()?,
                u16::try_from(i32::from(from.1) + dy).ok()?,
            );
            legal_move_cell(state, id, desired)
                .ok()
                .filter(|cell| *cell != from)
        })
        .context("reachable tactical Move target")?;
    ground_order_with_mouse(state, id, to)?;
    Ok((id, from, to))
}

pub(super) fn mobile_stop_with_keyboard(state: &mut AppState, id: u64) -> Result<()> {
    select_entity(state, id)?;
    selected_stop_with_keyboard(state)
}

pub(super) fn selected_stop_with_keyboard(state: &mut AppState) -> Result<()> {
    let key = Key::Character("s".into());
    ensure!(
        state.match_state.input.hotkey_bindings.resolve_event(
            &key,
            KeyLocation::Standard,
            state.match_state.input.hotkey_modifiers
        ) == HotkeyResolution::Command(HotkeyCommand::StopObject),
        "loaded S key is not the Stop binding"
    );
    chat_key(state, KeyCode::KeyS, &key, Some("s"));
    Ok(())
}

/// Select through the configured retail P binding twice: screen then map.
pub(super) fn combatants_with_keyboard(state: &mut AppState) -> Result<()> {
    let key = Key::Character("p".into());
    ensure!(
        state.match_state.input.hotkey_bindings.resolve_event(
            &key,
            KeyLocation::Standard,
            state.match_state.input.hotkey_modifiers
        ) == HotkeyResolution::Command(HotkeyCommand::CombatantSelect),
        "loaded P key is not the CombatantSelect binding"
    );
    for _ in 0..2 {
        chat_key(state, KeyCode::KeyP, &key, Some("p"));
    }
    Ok(())
}

/// Keep the full current selection and click ordinary empty ground.
pub(super) fn selected_move_with_mouse(
    state: &mut AppState,
    actor: u64,
    direction: usize,
) -> Result<()> {
    let sim = simulation(state)?;
    let entity = sim.entities().get(actor).context("mass leader")?;
    let from = (entity.position.rx(), entity.position.ry());
    let mut chosen = None;
    for distance in [18, 30, 48, 64] {
        for (dx, dy) in [
            (distance, 0),
            (-distance, 0),
            (0, distance),
            (0, -distance),
            (distance, distance),
            (-distance, -distance),
        ]
        .into_iter()
        .cycle()
        .skip(direction)
        .take(6)
        {
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(from.0) + dx),
                u16::try_from(i32::from(from.1) + dy),
            ) else {
                continue;
            };
            let Ok(cell) = legal_move_cell(state, actor, (x, y)) else {
                continue;
            };
            let Some(terrain) = state
                .resolved_terrain()
                .and_then(|terrain| terrain.cell(cell.0, cell.1))
            else {
                continue;
            };
            let world =
                crate::app::input::camera::cell_centre_world_point(cell.0, cell.1, terrain.level);
            if crate::app::input::entity_pick::hover_object_at_point(
                crate::app::input::entity_pick::presented(state, sim),
                world.0,
                world.1,
                &local_owner(state)?,
                false,
            )
            .is_none()
            {
                chosen = Some(cell);
                break;
            }
        }
        if chosen.is_some() {
            break;
        }
    }
    point_at_ground(state, chosen.context("empty ground for mass Move")?)?;
    for edge in [ElementState::Pressed, ElementState::Released] {
        dispatch::handle_mouse_input(state, MouseButton::Left, edge);
    }
    Ok(())
}

fn force_fire_with_mouse(state: &mut AppState, gi: u64, target: u64) -> Result<()> {
    select_entity(state, gi)?;
    ensure!(
        state.match_state.input.hotkey_modifiers.is_empty(),
        "force-fire began with held modifiers"
    );
    // The ordinary window handler and literal-key capture both enter this
    // physical-key owner; no direct modifier-view assignment belongs here.
    let modifier_edge = |state: &mut AppState, pressed| {
        hotkeys::record_modifier_key(
            &mut state.platform.held_modifier_keys,
            &mut state.platform.live_modifiers,
            &mut state.match_state.input.hotkey_modifiers,
            KeyCode::ControlLeft,
            pressed,
            false,
        );
    };
    modifier_edge(state, true);
    ensure!(
        state.match_state.input.hotkey_modifiers.control_key(),
        "Control press was not retained"
    );
    let clicked = click_entity(state, target);
    modifier_edge(state, false);
    ensure!(
        state.match_state.input.hotkey_modifiers.is_empty(),
        "Control release stayed held"
    );
    clicked
}

fn repair_mode_key(state: &mut AppState, enabled: bool) -> Result<()> {
    if state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .repair_mode_on
        != enabled
    {
        let key = Key::Character("k".into());
        let input = &state.match_state.input;
        ensure!(
            input.hotkey_bindings.resolve_event(
                &key,
                KeyLocation::Standard,
                input.hotkey_modifiers
            ) == HotkeyResolution::Command(HotkeyCommand::ToggleRepair),
            "loaded K does not resolve Repair; current binding={:?}",
            input.hotkey_bindings.first_key(HotkeyCommand::ToggleRepair)
        );
        chat_key(state, KeyCode::KeyK, &key, Some("k"));
    }
    ensure!(
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .repair_mode_on
            == enabled,
        "real Repair key did not set the expected cursor mode"
    );
    if enabled {
        ensure!(
            projected_selection(state).is_empty(),
            "Repair cursor did not clear projected selection"
        );
    }
    Ok(())
}

fn sell_mode_key(state: &mut AppState, enabled: bool) -> Result<()> {
    if state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .sell_mode_on
        != enabled
    {
        let key = Key::Character("l".into());
        let input = &state.match_state.input;
        ensure!(
            input.hotkey_bindings.resolve_event(
                &key,
                KeyLocation::Standard,
                input.hotkey_modifiers
            ) == HotkeyResolution::Command(HotkeyCommand::ToggleSell),
            "loaded L does not resolve Sell; current binding={:?}",
            input.hotkey_bindings.first_key(HotkeyCommand::ToggleSell)
        );
        chat_key(state, KeyCode::KeyL, &key, Some("l"));
    }
    ensure!(
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .sell_mode_on
            == enabled,
        "real Sell key did not set the expected cursor mode"
    );
    if enabled {
        ensure!(
            projected_selection(state).is_empty(),
            "Sell cursor did not clear projected selection"
        );
    }
    Ok(())
}

fn repair_with_mouse(state: &mut AppState, producer: u64) -> Result<()> {
    repair_mode_key(state, true)?;
    click_entity(state, producer)?;
    ensure!(
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .repair_mode_on,
        "GAPILE Repair click did not retain the sticky cursor mode"
    );
    Ok(())
}

fn planning_rally_with_mouse(state: &mut AppState, id: u64, cell: (u16, u16)) -> Result<usize> {
    ensure!(
        !state.match_state.input.planning.active(),
        "rally gesture began with Planning active"
    );
    let sim = simulation(state)?;
    let owner = sim.entities().get(id).context("rally GAPILE")?.owner();
    ensure!(
        sim.planning_paths(owner).is_empty(),
        "rally gesture began with an existing Planning path"
    );
    let key = Key::Character("z".into());
    let input = &state.match_state.input;
    ensure!(
        input
            .hotkey_bindings
            .resolve_event(&key, KeyLocation::Standard, input.hotkey_modifiers)
            == HotkeyResolution::Command(HotkeyCommand::PlanningMode),
        "loaded Z binding does not resolve PlanningMode; current binding={:?}",
        input.hotkey_bindings.first_key(HotkeyCommand::PlanningMode)
    );
    let key_edge = |state: &mut AppState, edge| {
        keyboard::in_game_key_edge(
            state,
            keyboard::InGameKeyEdge {
                physical: KeyCode::KeyZ,
                logical: &key,
                unmodified: &key,
                location: KeyLocation::Standard,
                state: edge,
                repeat: false,
                text: None,
            },
            false,
        );
    };
    select_entity(state, id)?;
    point_at_ground(state, cell)?;
    // A Building press already in Planning refuses before tactical capture.
    // Take ordinary capture first, then enter PlanMgr6379C0 through the real
    // binding. Its drag cancellation leaves capture for LeftUp4ABF7F's Click
    // fallback, which dispatches this direct Building ARCHIVE while held.
    dispatch::handle_mouse_input(state, MouseButton::Left, ElementState::Pressed);
    key_edge(state, ElementState::Pressed);
    ensure!(
        state.match_state.input.planning.active(),
        "real Planning key press did not enter Planning"
    );
    let before = rally_eva_requests(state);
    dispatch::handle_mouse_input(state, MouseButton::Left, ElementState::Released);
    ensure!(
        state.match_state.input.planning.active(),
        "Planning was inactive at the rally mouse release/EVA observation"
    );
    let after = rally_eva_requests(state);
    ensure!(
        after == before + 1,
        "Planning-active GAPILE rally did not synchronously announce NewRallyPoint"
    );
    ensure!(
        simulation(state)?.planning_paths(owner).is_empty(),
        "direct Planning-active GAPILE rally created a Building path"
    );
    key_edge(state, ElementState::Released);
    ensure!(
        !state.match_state.input.planning.active(),
        "real Planning key release did not leave Planning before E1 production"
    );
    Ok(after - before)
}

fn point_at_ground(state: &mut AppState, cell: (u16, u16)) -> Result<()> {
    let z = state
        .resolved_terrain()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .context("ground input terrain cell")?
        .level;
    let world = crate::app::input::camera::cell_centre_world_point(cell.0, cell.1, z);
    crate::app::input::camera::center_camera_on_cell(state, cell.0, cell.1);
    let input = &mut state.match_state.input;
    input.cursor_x = (world.0 - input.camera_x) * input.zoom_level;
    input.cursor_y = (world.1 - input.camera_y) * input.zoom_level;
    dispatch::handle_cursor_moved_in_game(state);
    Ok(())
}

fn select_entity(state: &mut AppState, id: u64) -> Result<()> {
    // An already-selected object can have a self action (GI Deploy, Primary).
    // Preserve the actual
    // input selection, including its pending frame-ingress projection, which
    // is also what the following Move/Attack/Stop input consumes.
    if projected_selection(state) != [id] {
        // The prior command may have left a building selected; clear it through
        // the tactical cancel ladder before a GI click can mean Building Rally.
        // Point first: a right-click over the last E1 cameo is production input.
        point_at_entity(state, id)?;
        for edge in [ElementState::Pressed, ElementState::Released] {
            dispatch::handle_mouse_input(state, MouseButton::Right, edge);
        }
        ensure!(
            projected_selection(state).is_empty(),
            "tactical cancel did not clear selection before object{id}: projected={:?}, pending={}, targeting={:?}",
            projected_selection(state),
            state.match_state.input.selection_order_pending,
            state.match_state.input.targeting_mode
        );
        click_entity(state, id)?;
    }
    ensure!(
        projected_selection(state) == [id],
        "tactical input did not retain object{id}: projected={:?}, pending={}, position={:?}",
        projected_selection(state),
        state.match_state.input.selection_order_pending,
        simulation(state)?
            .entities()
            .get(id)
            .map(|entity| entity.position)
    );
    Ok(())
}

pub(super) fn projected_selection(state: &AppState) -> Vec<u64> {
    dispatch::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    )
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
    let picked = point_at_entity(state, id)?;
    ensure!(
        picked == Some(id),
        "tactical pointer missed object{id}: picked={picked:?}, projected={:?}, pending={}, position={:?}",
        projected_selection(state),
        state.match_state.input.selection_order_pending,
        simulation(state)?
            .entities()
            .get(id)
            .map(|entity| entity.position)
    );
    for edge in [ElementState::Pressed, ElementState::Released] {
        dispatch::handle_mouse_input(state, MouseButton::Left, edge);
    }
    Ok(())
}

fn point_at_entity(state: &mut AppState, id: u64) -> Result<Option<u64>> {
    let entity = simulation(state)?
        .entities()
        .get(id)
        .context("mouse actor")?;
    let cell = (entity.position.rx(), entity.position.ry());
    let world = crate::app::input::entity_pick::presented(state, simulation(state)?)
        .screen_position(entity);
    crate::app::input::camera::center_camera_on_cell(state, cell.0, cell.1);
    let input = &mut state.match_state.input;
    input.cursor_x = (world.0 - input.camera_x) * input.zoom_level;
    input.cursor_y = (world.1 - input.camera_y) * input.zoom_level;
    dispatch::handle_cursor_moved_in_game(state);
    let sim = simulation(state)?;
    Ok(crate::app::input::entity_pick::hover_object_at_point(
        crate::app::input::entity_pick::presented(state, sim),
        world.0,
        world.1,
        &local_owner(state)?,
        false,
    )
    .map(|object| object.id()))
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
            let origin = object.foundation.placement_origin_at_cursor((x, y));
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

#[cfg(test)]
mod identity_tests {
    use super::*;
    use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};
    use crate::sim::house_state::HouseState;

    fn admitted_pair(
        local_first: bool,
    ) -> (
        MatchSession,
        Simulation,
        [crate::sim::intern::InternedId; 2],
    ) {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(fixture["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.humans[0].name = "VH1a2b3c4d5e".into();
        launch.humans[0].raw_slot_index = Some(0);
        let mut guest = launch.humans[0].clone();
        guest.name = "VG6f7a8b9c0d".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        launch.local_player_index = 1;
        if local_first {
            launch.humans.swap(0, 1);
            launch.local_player_index = 0;
        }
        let mut session = MatchSession::default();
        session.enter_network(if local_first {
            NetworkReturnContext::LanLobby
        } else {
            NetworkReturnContext::Wol
        });
        session.accept_network_setup(launch, false, None).unwrap();
        let mut sim = Simulation::new();
        let host = sim.intern("VH1a2b3c4d5e");
        let guest = sim.intern("VG6f7a8b9c0d");
        let neutral = sim.intern("Neutral");
        let special = sim.intern("Special");
        // Deliberately distinct native/participant order, with nonparticipant
        // Houses. Neither actual account name is a diagnostic role label.
        for (index, (owner, human)) in [
            (neutral, false),
            (guest, true),
            (special, false),
            (host, true),
        ]
        .into_iter()
        .enumerate()
        {
            sim.houses_mut_for_test().register(HouseState::new(
                owner,
                index as u8,
                None,
                human,
                0,
                10,
            ));
        }
        (session, sim, [host, guest])
    }

    #[test]
    fn battle_roles_resolve_dynamic_accounts_through_the_actual_native_house_table() {
        let (session, sim, owners) = admitted_pair(false);
        assert_eq!(battle_owners(&session, &sim).unwrap(), owners);
        assert_eq!(battle_human_index(&session, &sim, owners[0]).unwrap(), 0);
        assert_eq!(battle_human_index(&session, &sim, owners[1]).unwrap(), 1);
        assert_eq!(
            crate::app::match_runtime::network_match::source_house(&session, &sim, 0),
            Some(3)
        );
        assert_eq!(
            crate::app::match_runtime::network_match::source_house(&session, &sim, 1),
            Some(1)
        );
        let mut host_session = session.clone();
        let mut launch = host_session.network_setup().unwrap().clone();
        launch.local_player_index = 0;
        host_session
            .accept_network_setup(launch, true, None)
            .unwrap();
        assert_eq!(battle_owners(&host_session, &sim).unwrap(), owners);
    }

    #[test]
    fn battle_roles_preserve_lan_local_first_admission_and_reject_foreign_houses() {
        let (session, sim, owners) = admitted_pair(true);
        assert_eq!(session.network_setup().unwrap().local_player_index, 0);
        assert_eq!(
            session.network_source_owner(0).as_deref(),
            Some("VG6f7a8b9c0d")
        );
        assert_eq!(battle_owners(&session, &sim).unwrap(), owners);
        assert_eq!(battle_human_index(&session, &sim, owners[0]).unwrap(), 0);
        assert_eq!(battle_human_index(&session, &sim, owners[1]).unwrap(), 1);
        assert!(
            battle_human_index(&session, &sim, sim.interner().get("Neutral").unwrap()).is_err()
        );
    }
}
