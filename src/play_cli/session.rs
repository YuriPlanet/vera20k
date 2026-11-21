//! Own the CLI's match and adapt requests to the headless engine APIs.

use std::path::PathBuf;

use vera20k_core::headless_scenario;
use vera20k_core::rules::object_type::ObjectCategory;
use vera20k_core::sim::command::{Command, CommandEnvelope};
use vera20k_core::sim::house_state::HouseOutcomeKind;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::production::{
    ProductionCategory, build_options_for_owner, placement_preview_for_owner_with_overlays,
    power_balance_for_owner,
};
use vera20k_core::sim::runtime::SimRuntime;
use vera20k_core::sim::world::{FrameEffects, Simulation, TickLane};
use vera20k_core::skirmish_launch::{AiDifficulty, LaunchCountry};
use vera20k_core::util::fixed_math::SIM_TICK_HZ;

use super::command::{Order, Request};
use super::observe::{
    BuildOption, Event, Factory, FinishReason, House, Object, Observation, SCHEMA,
};

const MAX_STEP: u32 = 4096;
const FACTORY_CATEGORIES: [ProductionCategory; 6] = [
    ProductionCategory::Building,
    ProductionCategory::Defense,
    ProductionCategory::Infantry,
    ProductionCategory::Vehicle,
    ProductionCategory::Aircraft,
    ProductionCategory::Ship,
];

pub struct SessionOptions {
    pub retail_dir: PathBuf,
    pub map: String,
    pub seed: u32,
    pub opponents: Vec<(LaunchCountry, AiDifficulty)>,
    pub max_ticks: Option<u64>,
}

pub struct Session {
    runtime: SimRuntime,
    house: InternedId,
    max_ticks: Option<u64>,
}

impl Session {
    pub fn start(options: SessionOptions) -> Result<(Self, Event), String> {
        let launch = headless_scenario::battle_session(&options.map, &options.opponents);
        let loaded = headless_scenario::load_session(
            &options.retail_dir,
            &options.map,
            options.seed,
            launch,
        )?;
        let sim = loaded.sim();
        let house = sim
            .session()
            .current_house()
            .filter(|id| sim.houses().contains_key(id))
            .ok_or("skirmish did not load a local house")?;
        let session = Self {
            runtime: loaded.runtime,
            house,
            max_ticks: options.max_ticks,
        };
        let ready = Event::Ready {
            schema: SCHEMA,
            map: options.map,
            seed: options.seed,
            observation: session.observe(),
        };
        Ok((session, ready))
    }

    fn sim(&self) -> &Simulation {
        &self.runtime.simulation
    }

    pub fn eof(&self) -> Event {
        self.finish(FinishReason::Eof)
    }

    pub fn handle(&mut self, request: Request) -> Event {
        let result = match request {
            Request::State => {
                return Event::State {
                    observation: self.observe(),
                };
            }
            Request::BuildOptions => return self.build_options(),
            Request::Quit => return self.finish(FinishReason::Quit),
            Request::Tick { ticks } if (1..=MAX_STEP).contains(&ticks) => return self.step(ticks),
            Request::Tick { .. } => Err(format!("ticks must be in 1..={MAX_STEP}")),
            Request::Order { order } => self.queue(order),
            Request::Placement { type_name, x, y } => self.placement(&type_name, x, y),
            Request::Cell { x, y } => self.cell(x, y),
        };
        result.unwrap_or_else(|message| Event::Error {
            message,
            fatal: false,
        })
    }

    fn queue(&mut self, order: Order) -> Result<Event, String> {
        let sim = self.sim();
        if sim.session().input_locked() {
            return Err("scenario input is locked".into());
        }
        for &id in order.actors() {
            if !sim.entities().get(id).is_some_and(|actor| {
                actor.owner() == self.house
                    && actor.is_alive()
                    && actor.is_active()
                    && !actor.in_limbo()
            }) {
                return Err(format!("object {id} is not an active local actor"));
            }
        }
        if let Some((x, y)) = order.cell() {
            self.terrain_cell(x, y)?;
        }
        let rules = &self.runtime.resources.rules;
        let payload = order.into_command(|name, super_weapon| {
            let known = if super_weapon {
                rules.super_weapon(name).is_some()
            } else {
                rules.object(name).is_some()
            };
            known
                .then(|| sim.interner().get(name))
                .flatten()
                .ok_or_else(|| format!("unknown order type {name:?}"))
        })?;
        let tick = sim.session().tick();
        let envelope = match payload {
            Command::DeployMcv { entity_id } => {
                // D-key Event9 (0x00730AF0 → 0x004C76BC), including the
                // receiver's weapons-factory cell gate. The sender above
                // checks the actor's ownership before constructing it.
                let record = sim
                    .encode_deploy_record(self.house, entity_id)
                    .ok_or("could not encode deploy event")?;
                CommandEnvelope::synchronized(record, tick)
            }
            payload => CommandEnvelope::new(self.house, tick, payload),
        };
        if !self.runtime.simulation.issue_local_command(envelope) {
            return Err("local command queue refused the order".into());
        }
        Ok(Event::Queued { tick })
    }

    fn step(&mut self, ticks: u32) -> Event {
        let tick = self.sim().session().tick();
        let remaining = self
            .max_ticks
            .map_or(u64::MAX, |limit| limit.saturating_sub(tick));
        let mut effects = FrameEffects::default();
        for _ in 0..u64::from(ticks).min(remaining) {
            let frame = match self.runtime.advance_frame(
                &[],
                1000 / SIM_TICK_HZ,
                TickLane::Ordinary,
                &mut effects,
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    return Event::Error {
                        message: error.to_string(),
                        fatal: true,
                    };
                }
            };
            if frame.network_waiting || frame.network_failure.is_some() || frame.outcome_waiting {
                return Event::Error {
                    message: "headless skirmish suspended its frame".into(),
                    fatal: true,
                };
            }
            if !frame.tick.frame_committed {
                let reason = match self.sim().ready_outcome_for_owner(self.house) {
                    Some(outcome) => match outcome.kind {
                        HouseOutcomeKind::Victory => FinishReason::Victory,
                        HouseOutcomeKind::Defeat => FinishReason::Defeat,
                    },
                    None => FinishReason::MatchEnded,
                };
                return self.finish(reason);
            }
        }
        if self
            .max_ticks
            .is_some_and(|limit| self.sim().session().tick() >= limit)
        {
            return self.finish(FinishReason::TickLimit);
        }
        Event::State {
            observation: self.observe(),
        }
    }

    fn finish(&self, reason: FinishReason) -> Event {
        Event::Finished {
            reason,
            observation: self.observe(),
        }
    }

    fn build_options(&self) -> Event {
        let sim = self.sim();
        let names = sim.interner();
        let options = build_options_for_owner(
            sim,
            &self.runtime.resources.rules,
            names.resolve(self.house),
        )
        .into_iter()
        .filter(|option| option.enabled)
        .map(|option| BuildOption {
            type_name: names.resolve(option.type_id).to_owned(),
            category: option.queue_category,
            cost: option.cost,
        })
        .collect();
        Event::BuildOptions { options }
    }

    fn placement(&self, name: &str, x: u16, y: u16) -> Result<Event, String> {
        self.terrain_cell(x, y)?;
        let rules = &self.runtime.resources.rules;
        if !rules
            .object(name)
            .is_some_and(|object| object.category == ObjectCategory::Building)
        {
            return Err(format!("unknown building type {name:?}"));
        }
        let sim = self.sim();
        let preview = placement_preview_for_owner_with_overlays(
            sim,
            rules,
            sim.interner().resolve(self.house),
            name,
            x,
            y,
            Some(rules.overlay_registry()),
        )
        .ok_or("placement preview unavailable")?;
        Ok(Event::Placement {
            valid: preview.valid,
            in_build_area: preview.in_build_area,
            foundation_visible: preview.foundation_visible,
            reason: preview.reason.map(|reason| reason.label()),
        })
    }

    fn terrain_cell(
        &self,
        x: u16,
        y: u16,
    ) -> Result<&vera20k_core::map::resolved_terrain::ResolvedTerrainCell, String> {
        self.sim()
            .resolved_terrain()
            .and_then(|terrain| terrain.cell(x, y))
            .ok_or_else(|| format!("cell ({x}, {y}) is outside the map"))
    }

    fn cell(&self, x: u16, y: u16) -> Result<Event, String> {
        let cell = self.terrain_cell(x, y)?;
        let fog = self.sim().fog();
        let visible = fog.is_cell_visible(self.house, x, y);
        let revealed = fog.is_cell_revealed(self.house, x, y);
        Ok(Event::Cell {
            x,
            y,
            revealed,
            visible,
            level: revealed.then_some(cell.level),
            land_type: revealed.then_some(cell.land_type),
        })
    }

    fn observe(&self) -> Observation {
        let sim = self.sim();
        let names = sim.interner();
        let economy = sim.houses().get(&self.house).map(|house| house.economy());
        let (power_output, power_drain) = power_balance_for_owner(
            sim,
            &self.runtime.resources.rules,
            names.resolve(self.house),
        );
        let house = House {
            name: names.resolve(self.house).to_owned(),
            credits: economy.map_or(0, |economy| economy.credits()),
            spent_credits: economy.map_or(0, |economy| economy.spent_credits()),
            power_output,
            power_drain,
        };
        let mut units = Vec::new();
        let mut contacts = Vec::new();
        for entity in sim.entities().values_sorted() {
            if !entity.is_alive() || entity.in_limbo() {
                continue;
            }
            let (x, y) = (entity.position.rx(), entity.position.ry());
            let rows = if entity.owner() == self.house {
                &mut units
            } else if sim.fog().is_cell_visible(self.house, x, y) {
                &mut contacts
            } else {
                continue;
            };
            rows.push(Object {
                id: entity.stable_id(),
                type_name: names.resolve(entity.type_ref()).to_owned(),
                owner: names.resolve(entity.owner()).to_owned(),
                category: entity.category(),
                x,
                y,
                health: entity.health.current(),
            });
        }
        let registry = sim.production().factories();
        let factories = FACTORY_CATEGORIES
            .into_iter()
            .filter_map(|category| {
                let view = registry.view(self.house, category)?;
                let object = view.object?;
                Some(Factory {
                    category,
                    type_name: names.resolve(object.type_id).to_owned(),
                    progress: view.progress,
                    state: view.cameo_state(),
                    queue: view
                        .queue
                        .iter()
                        .map(|entry| names.resolve(entry.type_id).to_owned())
                        .collect(),
                })
            })
            .collect();
        let (map_width, map_height) = sim
            .resolved_terrain()
            .map_or((0, 0), |terrain| (terrain.width(), terrain.height()));
        Observation {
            tick: sim.session().tick(),
            binary_frame: sim.session().binary_frame(),
            state_hash: format!("{:016x}", sim.state_hash()),
            map_width,
            map_height,
            house,
            units,
            contacts,
            factories,
        }
    }
}
