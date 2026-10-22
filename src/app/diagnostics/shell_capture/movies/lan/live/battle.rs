//! Real input automation for a bounded Battle opening. This observer never
//! mutates the simulation, fabricates a peer command, or advances a frame.

use super::*;
use crate::app::input::entity_pick::PickSource;
use crate::app::input::{commands, dispatch};
use crate::sim::combat::TargetKind;
use crate::sim::command::{
    Command, MegaMissionOrder, MegaMissionRecord, MegaMissionTarget, PlaceRecord, ProductionEvent,
    ProductionRecord, StopRecord,
};
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
    Rendezvous,
    Approaching,
    Attacking,
    Stopping,
    Disengaging {
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
        self.observe_fighters(state)?;
        self.observe_commands(state, frame)?;
        self.observe_due_orders(state, frame)?;
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
                if self.all_gi_orders_admitted(state)? {
                    let desired = staging_centres(state)?;
                    let index = usize::from(!host);
                    let id = self.fighter_ids()?[index];
                    ensure!(
                        Some(id) == self.gi_products.first().map(|product| product.id),
                        "encounter actor is not the first original local paid GI"
                    );
                    let cell = legal_move_cell(state, id, desired[index])?;
                    move_with_mouse(state, id, cell)?;
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
                    move_with_mouse(state, ids[0], approach)?;
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
                move_with_mouse(state, id, cell)?;
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
                    self.step = Step::Done;
                    action =
                        Some("manual Attack, rifle damage and raw Stop observed; GIs separated");
                }
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
        for (index, name) in [HOST_NAME, GUEST_NAME].into_iter().enumerate() {
            if self.fighters[index].is_some() {
                continue;
            }
            let Some(owner) = sim.interner().get(name) else {
                continue;
            };
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
                            Command::PlaceProducedMobile { .. } | Command::Attack { .. }
                        ),
                        "live Battle admitted semantic mobile PLACE or Attack"
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
                        if local_owner(state)? == HOST_NAME {
                            ensure!(
                                self.attack_input_frame
                                    .is_some_and(|issued| mission.frame >= issued as i32),
                                "Attack did not follow the actual object click"
                            );
                        }
                        self.pending_attack = Some(mission);
                    }
                }
                if let Some(stop) = StopRecord::decode(record)
                    && stop.entity_id == i32::try_from(actor)?
                {
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
            && self.attack_observation.is_some()
            && self.hit_observation.is_some()
            && self.stop_observation.is_some()
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
            && self.all_gi_orders_admitted(state)?)
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
            "combat":{"fighters":self.fighters,"staging":self.staging,
                "attack_input_frame":self.attack_input_frame,"attack":self.attack_observation,
                "hit":self.hit_observation,"stop":self.stop_observation,"native_events":self.combat_events,
                "final":combat_snapshot(state,self.fighter_ids()?,sim.session().binary_frame())?},
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
    for (index, name) in [HOST_NAME, GUEST_NAME].into_iter().enumerate() {
        let owner = sim.interner().get(name).context("encounter House")?;
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

fn move_with_mouse(state: &mut AppState, id: u64, cell: (u16, u16)) -> Result<()> {
    select_entity(state, id)?;

    let z = state
        .resolved_terrain()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .context("Move terrain cell")?
        .level;
    let world = crate::app::input::camera::cell_centre_world_point(cell.0, cell.1, z);
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

fn select_entity(state: &mut AppState, id: u64) -> Result<()> {
    // Clicking an already-selected GI is a Deploy action. Preserve the actual
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
            "tactical cancel did not clear selection before GI{id}: projected={:?}, pending={}, targeting={:?}",
            projected_selection(state),
            state.match_state.input.selection_order_pending,
            state.match_state.input.targeting_mode
        );
        click_entity(state, id)?;
    }
    ensure!(
        projected_selection(state) == [id],
        "tactical input did not retain GI{id}: projected={:?}, pending={}, position={:?}",
        projected_selection(state),
        state.match_state.input.selection_order_pending,
        simulation(state)?
            .entities()
            .get(id)
            .map(|entity| entity.position)
    );
    Ok(())
}

fn projected_selection(state: &AppState) -> Vec<u64> {
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
