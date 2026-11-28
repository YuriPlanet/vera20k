//! House orders (RFC 0001 section 6g): `order(house, ptr, len)`, and the
//! order results of section 6i.
//!
//! `order` checks the house, the command table (appendix 8.3) and the
//! per-frame limits, and queues the order in the calling module's slot. The
//! queue is module state: saved with the slot and hashed. The frame after
//! the call, the command step runs the frame's player commands first, then
//! each module's orders, mod before map and in issue order
//! ([`Simulation::run_ext_module_orders`]). Each order becomes the player
//! commands its house's player would send, executed by the same receiver
//! (`apply_command_with_overlays`) that executes received player commands,
//! so it passes the same checks against the state at that moment. A module
//! order is its house's own sender (`CommandSender::HouseOrder`): under
//! custom control grants (shared houses) its house-wide commands need no
//! granted source, and its `QueueProduction` asks the prerequisites the
//! house's player would be asked (`production::buildable_by_player`), which
//! a computer house's own CanBuild skips. Orders never enter the network
//! command stream: every machine's modules issue them in lockstep, and
//! replays resimulate the modules.
//!
//! A client module's `client_send_order` (RFC 10, 6i) goes through the same
//! check ([`check_client_order`]) and becomes the same player commands
//! ([`CheckedOrder::commands`]); those enter the local player's command
//! stream like the player's own input (`app::ui_modules::local`).
//!
//! Commands of the table and the player commands they become:
//! - `Guard` with a cell is the player's area-guard click (`Command::Guard`
//!   with the cell, Foot ClickedAction_Cell `0x004D7E35..0x004D7E8B`);
//! - `Harvest` is the harvest click on a cell (`Command::clicked_mission`
//!   Harvest, the cell as destination);
//! - `Enter` is the entry click on the object: MEGAMISSION Enter, or Eaten
//!   at a `Grinding=` building, the object as destination;
//! - `SuspendProduction` is SUSPEND for the production category of the
//!   type (`production::category_for_object`);
//! - `TakeProduct` is the mobile PLACE (`Command::PlaceProducedMobile`,
//!   House Place_Production `0x004FB0E0`) of the factory kind's category:
//!   Infantry, Vehicle, Naval or Aircraft (kinds 2..5 of `FACTORY_KINDS`).
//!
//! Results (RFC 6i "Order results"), for player commands and module orders
//! alike, raised after the receiver ran ([`Simulation::raise_module_command_outcome`]):
//! - `Commanded` on each actor of an applied command: `value` the command's
//!   appendix 8.3 number, `other` and `cell` its target, `value2` a serial
//!   the commands of one module order share, and the consecutive commands
//!   of one house with the same number and target (a player's click) share
//!   ([`CommandSerial`]);
//! - `OrderRefused` (global) for a refused command: once per actor, or
//!   once with subject 0 and `id` the type (TechnoType, or superweapon
//!   index) for a house-wide one; `house` the issuing house, `value` the
//!   number, `value2` the reason ([`CommandRefusal`]; a refusal the
//!   receiver names `Other` whose actor is no longer alive reads `Gone`).
//!
//! Residuals:
//! - `EjectBunker` answers `Unsupported`: it is a record no player input
//!   sends.
//! - `IgnoreOrders` is not kept yet, so no unit ignores a module order and
//!   no refusal is `IgnoresOrders`; no receiver refuses for credits, so
//!   none is `CannotAfford`.
//! - `Enter` does not ask the entry click's What_Action rows
//!   (`Simulation::entry_click_action` reads the local player's control
//!   view): a target the click would answer NoEnter or Tote for still gets
//!   the Enter mission, which the unit's mission then gives up.
//! - Native AI gives missions directly, not through commands, so its units
//!   raise no `Commanded`. `CommandAllowed` narrowing a rally point to some
//!   producers still raises `Commanded` on every producer named.
//! - A player's `SuspendProduction`, `TakeProduct` and `SellWallAtCell`
//!   refusals carry `id` −1, as their records name no type.
//! - A player's MEGAMISSION record that carries NavComAux (a Planning Mode
//!   waypoint) stays a raw record until the receiver and raises neither
//!   event, nor do Planning Mode records, Ares' paid two-stage superweapon
//!   commands and commands without an 8.3 number.
//! - The orders and unit entries per module per frame are provisional
//!   (open question 7).

use serde::{Deserialize, Serialize};
use vera20k_wasm_ext::triggers::{EVENT_COMMANDED, EVENT_ORDER_REFUSED};
use vera20k_wasm_ext::{Invalid, NAME_SUPER_WEAPON, NAME_TECHNO_TYPE, Order, Status};

use super::{Host, ModuleEvent, Reader, Ref, decode_ref};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope, CommandRefusal};
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::production::{ProductionCategory, category_for_object};
use crate::sim::world::{FrameEffects, Simulation};

/// Orders one module may issue per frame (RFC 15.4).
pub const MAX_ORDERS_PER_FRAME: usize = 256;
/// Unit entries one module's orders may carry per frame.
pub const MAX_UNITS_PER_FRAME: usize = 4096;

/// An order waiting for the command step (RFC 6g "Timing").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct PendingOrder {
    /// The house array index.
    house: usize,
    /// The frame it was issued in; it executes in the next command step.
    frame: u32,
    command: OrderCommand,
    units: Vec<u64>,
}

/// An order's command with its checked target and value. New variants are
/// appended: the queue travels in saves.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum OrderCommand {
    Move(u16, u16),
    MoveToObject(u64),
    Stop,
    Attack(u64),
    AttackMove(u16, u16),
    Guard(Option<u64>),
    DeployMcv,
    UndeployBuilding,
    SetRally(u16, u16),
    /// A TechnoType, by registry ID.
    QueueProduction(u32),
    CancelProductionByType(u32),
    PlaceReadyBuilding(u32, u16, u16),
    SellBuilding,
    ToggleRepair,
    UnloadPassengers,
    CaptureBuilding(u64),
    PlantC4(u64),
    /// A superweapon type, by `[SuperWeaponTypes]` index.
    LaunchSuperWeapon(u32, u16, u16),
    ToggleInfantryDeploy,
    ForceAttackCell(u16, u16),
    SellWallAtCell(u16, u16),
    Scatter,
    TogglePrimaryFactory,
    /// `Guard` with a cell: guard that area.
    GuardArea(u16, u16),
    /// A TechnoType, by registry ID.
    SuspendProduction(u32),
    Enter(u64),
    Harvest(u16, u16),
    /// A factory kind, an index of `FACTORY_KINDS` (2..5).
    TakeProduct(u32),
}

/// What a command takes in `target` (appendix 8.3 command table).
#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetKindSpec {
    None,
    Cell,
    Object,
    /// An object, a cell or nothing (`Guard`).
    Any,
}

/// What a command takes in `value`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueSpec {
    None,
    Techno,
    Building,
    SuperWeapon,
    /// A plain index of `FACTORY_KINDS`, not a reference (`TakeProduct`).
    FactoryKind,
}

/// A checked target.
#[derive(Clone, Copy)]
enum Target {
    None,
    Cell(u16, u16),
    Object(u64),
}

/// The command table row of `kind`: target, value, and whether it takes
/// units; `None` for a command this engine does not run.
const fn command_spec(kind: i32) -> Option<(TargetKindSpec, ValueSpec, bool)> {
    use TargetKindSpec as T;
    use ValueSpec as V;
    Some(match kind {
        0 | 4 | 8 | 21 | 26 => (T::Cell, V::None, true),
        1 | 3 | 17 | 18 | 25 => (T::Object, V::None, true),
        5 => (T::Any, V::None, true),
        2 | 6 | 7 | 13 | 14 | 16 | 20 | 23 | 24 => (T::None, V::None, true),
        9 | 10 | 11 => (T::None, V::Techno, false),
        12 => (T::Cell, V::Building, false),
        19 => (T::Cell, V::SuperWeapon, false),
        22 => (T::Cell, V::None, false),
        27 => (T::None, V::FactoryKind, false),
        // 15 EjectBunker and unknown commands (module residuals).
        _ => return None,
    })
}

impl Host<'_, '_> {
    /// `order`: checks and queues one order.
    pub(super) fn queue_order(&mut self, house: i64, order: &Order) -> Result<Status, Invalid> {
        let Some(owner) = self.reader().house(house)? else {
            return Err(Invalid(format!("{house:#x} names no house")));
        };
        let Some(state) = self.sim.houses.get(&owner) else {
            return Err(Invalid(format!("{house:#x} names no house")));
        };
        // RFC 6g "Houses": only a computer house that is not defeated.
        // `is_human` covers the local player's house; the local house
        // itself is per-client and never decides lockstep state.
        if state.is_human() || state.is_defeated() {
            return Ok(Status::Refused);
        }
        if command_spec(order.kind).is_none() {
            return Ok(Status::Unsupported);
        }
        let issued = self.orders.iter().filter(|o| o.frame == self.frame_now());
        let (count, units) = issued.fold((0, 0), |(n, u), o| (n + 1, u + o.units.len()));
        if count >= MAX_ORDERS_PER_FRAME || units + order.units.len() > MAX_UNITS_PER_FRAME {
            return Ok(Status::Refused);
        }
        let checked = match check_order(self.sim, self.rules, order)? {
            Ok(checked) => checked,
            Err(status) => return Ok(status),
        };
        let Some(index) = self.sim.houses.index_of(owner) else {
            return Ok(Status::Gone);
        };
        self.orders.push(PendingOrder {
            house: index,
            frame: self.frame_now(),
            command: checked.command,
            units: checked.units,
        });
        Ok(Status::Ok)
    }

    fn frame_now(&self) -> u32 {
        self.sim.session.binary_frame()
    }
}

impl Reader<'_> {
    /// A waypoint's cell; `None` for a waypoint the map does not set.
    pub(crate) fn waypoint_cell(&self, number: i32) -> Option<(u16, u16)> {
        waypoint_cell(self.sim, number)
    }
}

/// A waypoint's cell; `None` for a waypoint the map does not set.
pub(super) fn waypoint_cell(sim: &Simulation, number: i32) -> Option<(u16, u16)> {
    if !sim.session.waypoint_is_valid(number) {
        return None;
    }
    let (x, y) = sim.session.waypoint_cell(number)?;
    Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?))
}

/// An order checked against the command table (appendix 8.3): the one
/// check of `order` (RFC 6g) and `client_send_order` (RFC 10, 6i), so both
/// accept the same orders and turn them into the same player commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOrder {
    command: OrderCommand,
    /// The unit entries as object IDs; empty for a house-wide command.
    units: Vec<u64>,
}

impl CheckedOrder {
    /// The units the order names (object IDs).
    pub fn units(&self) -> &[u64] {
        &self.units
    }

    /// A house-wide command (production, placement, superweapon, wall
    /// sale): the command table lists no units for it.
    pub fn house_wide(&self) -> bool {
        self.units.is_empty()
    }

    /// The player commands the order becomes, as the order's house's player
    /// would send them; `None` where a type the order names is gone.
    pub fn commands(&self, sim: &Simulation, rules: &RuleSet) -> Option<Vec<Command>> {
        order_commands(sim, rules, &self.command, &self.units)
    }
}

/// Checks `order` against the command table: target and value of the
/// listed kinds, unit entries exactly where the table lists units.
/// `Ok(Err(status))` for an order the table refuses (`Unsupported` for a
/// command this engine does not run, `NotFound` for a missing waypoint).
pub(crate) fn check_order(
    sim: &Simulation,
    rules: &RuleSet,
    order: &Order,
) -> Result<Result<CheckedOrder, Status>, Invalid> {
    let Some((target_spec, value_spec, takes_units)) = command_spec(order.kind) else {
        return Ok(Err(Status::Unsupported));
    };
    if takes_units == order.units.is_empty() {
        return Ok(Err(Status::Refused));
    }
    let mut unit_ids = Vec::with_capacity(order.units.len());
    for &unit in &order.units {
        match decode_ref(unit)? {
            Some(Ref::Object(id)) => unit_ids.push(id),
            _ => return Ok(Err(Status::Refused)),
        }
    }
    let target = match (target_spec, decode_ref(order.target)?) {
        (TargetKindSpec::None | TargetKindSpec::Any, None) => Target::None,
        (TargetKindSpec::Object | TargetKindSpec::Any, Some(Ref::Object(id))) => Target::Object(id),
        (TargetKindSpec::Cell | TargetKindSpec::Any, Some(Ref::Cell { x, y })) => {
            Target::Cell(x, y)
        }
        (TargetKindSpec::Cell | TargetKindSpec::Any, Some(Ref::Waypoint(number))) => {
            match waypoint_cell(sim, number) {
                Some((x, y)) => Target::Cell(x, y),
                None => return Ok(Err(Status::NotFound)),
            }
        }
        _ => return Ok(Err(Status::Refused)),
    };
    let value = if value_spec == ValueSpec::FactoryKind {
        // Infantry, Vehicle, Naval and Aircraft: a building or defense
        // is placed with `PlaceReadyBuilding`.
        match order.value {
            2..=5 => Some(order.value as u32),
            _ => return Ok(Err(Status::Refused)),
        }
    } else {
        match (value_spec, decode_ref(order.value)?) {
            (ValueSpec::None, None) => None,
            (
                ValueSpec::Techno | ValueSpec::Building,
                Some(Ref::Type {
                    kind: NAME_TECHNO_TYPE,
                    id,
                }),
            ) => {
                let object = i32::try_from(id)
                    .ok()
                    .and_then(|id| super::techno_type(rules, id))
                    .ok_or_else(|| Invalid(format!("no TechnoType {id}")))?;
                if value_spec == ValueSpec::Building
                    && object.category != crate::rules::object_type::ObjectCategory::Building
                {
                    return Ok(Err(Status::Refused));
                }
                Some(id)
            }
            (
                ValueSpec::SuperWeapon,
                Some(Ref::Type {
                    kind: NAME_SUPER_WEAPON,
                    id,
                }),
            ) => {
                if usize::try_from(id)
                    .ok()
                    .and_then(|i| rules.super_weapon_order.get(i))
                    .is_none()
                {
                    return Err(Invalid(format!("no SuperWeapon {id}")));
                }
                Some(id)
            }
            _ => return Ok(Err(Status::Refused)),
        }
    };
    let Some(command) = order_command(order.kind, target, value) else {
        return Ok(Err(Status::Refused));
    };
    Ok(Ok(CheckedOrder {
        command,
        units: unit_ids,
    }))
}

/// `client_send_order`'s check (RFC 10, 6i): [`check_order`], then only the
/// units `house` (a house reference) owns stay, as a player's input names
/// only the player's own units. An order that lists units but keeps none is
/// `Refused`.
pub fn check_client_order(
    sim: &Simulation,
    rules: &RuleSet,
    house: i64,
    order: &Order,
) -> Result<Result<CheckedOrder, Status>, Invalid> {
    let owner = match decode_ref(house)? {
        Some(Ref::House(index)) => sim.houses.order().get(index).copied(),
        _ => None,
    };
    let Some(owner) = owner else {
        return Ok(Err(Status::Refused));
    };
    let mut checked = match check_order(sim, rules, order)? {
        Ok(checked) => checked,
        Err(status) => return Ok(Err(status)),
    };
    if checked.house_wide() {
        return Ok(Ok(checked));
    }
    checked.units = owned_units(sim, owner, &checked.units);
    if checked.units.is_empty() {
        return Ok(Err(Status::Refused));
    }
    Ok(Ok(checked))
}

/// The units of `units` that exist and belong to `owner`, in order.
fn owned_units(sim: &Simulation, owner: InternedId, units: &[u64]) -> Vec<u64> {
    units
        .iter()
        .copied()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| entity.owner() == owner)
        })
        .collect()
}

/// The command of a checked order; `None` where target and value do not fit.
fn order_command(kind: i32, target: Target, value: Option<u32>) -> Option<OrderCommand> {
    use OrderCommand as C;
    Some(match (kind, target, value) {
        (0, Target::Cell(x, y), _) => C::Move(x, y),
        (1, Target::Object(id), _) => C::MoveToObject(id),
        (2, ..) => C::Stop,
        (3, Target::Object(id), _) => C::Attack(id),
        (4, Target::Cell(x, y), _) => C::AttackMove(x, y),
        (5, Target::Object(id), _) => C::Guard(Some(id)),
        (5, Target::Cell(x, y), _) => C::GuardArea(x, y),
        (5, Target::None, _) => C::Guard(None),
        (6, ..) => C::DeployMcv,
        (7, ..) => C::UndeployBuilding,
        (8, Target::Cell(x, y), _) => C::SetRally(x, y),
        (9, _, Some(id)) => C::QueueProduction(id),
        (10, _, Some(id)) => C::SuspendProduction(id),
        (11, _, Some(id)) => C::CancelProductionByType(id),
        (12, Target::Cell(x, y), Some(id)) => C::PlaceReadyBuilding(id, x, y),
        (13, ..) => C::SellBuilding,
        (14, ..) => C::ToggleRepair,
        (16, ..) => C::UnloadPassengers,
        (17, Target::Object(id), _) => C::CaptureBuilding(id),
        (18, Target::Object(id), _) => C::PlantC4(id),
        (19, Target::Cell(x, y), Some(id)) => C::LaunchSuperWeapon(id, x, y),
        (20, ..) => C::ToggleInfantryDeploy,
        (21, Target::Cell(x, y), _) => C::ForceAttackCell(x, y),
        (22, Target::Cell(x, y), _) => C::SellWallAtCell(x, y),
        (23, ..) => C::Scatter,
        (24, ..) => C::TogglePrimaryFactory,
        (25, Target::Object(id), _) => C::Enter(id),
        (26, Target::Cell(x, y), _) => C::Harvest(x, y),
        (27, _, Some(kind)) => C::TakeProduct(kind),
        _ => return None,
    })
}

/// Takes the orders due in this command step, each as the player commands
/// its house's player would send. Units that are gone or no longer belong
/// to the house drop out (RFC 6g "Validation"); a defeated house's orders
/// are dropped.
pub(crate) fn take_due_orders(sim: &mut Simulation, rules: &RuleSet) -> Vec<Vec<CommandEnvelope>> {
    let now = sim.session.binary_frame();
    let mut due = Vec::new();
    for slot in &mut sim.ext_modules.slots {
        let (ready, waiting) = std::mem::take(&mut slot.orders)
            .into_iter()
            .partition(|order| order.frame != now);
        slot.orders = waiting;
        due.extend(ready);
    }
    let tick = sim.session.tick();
    due.into_iter()
        .filter_map(|order| {
            let owner = *sim.houses.order().get(order.house)?;
            if sim
                .houses
                .get(&owner)
                .is_none_or(|house| house.is_defeated())
            {
                return None;
            }
            let units = owned_units(sim, owner, &order.units);
            let commands = order_commands(sim, rules, &order.command, &units)?;
            Some(
                commands
                    .into_iter()
                    .map(|command| CommandEnvelope::new(owner, tick, command))
                    .collect(),
            )
        })
        .collect()
}

/// The player commands of one order.
fn order_commands(
    sim: &Simulation,
    rules: &RuleSet,
    command: &OrderCommand,
    units: &[u64],
) -> Option<Vec<Command>> {
    use OrderCommand as C;
    let object = |id: u32| rules.all_objects().nth(usize::try_from(id).ok()?);
    let techno = |id: u32| sim.interner.get(&object(id)?.id);
    let each = |make: &dyn Fn(u64) -> Command| units.iter().map(|&id| make(id)).collect();
    Some(match *command {
        C::Move(x, y) => each(&|entity_id| Command::Move {
            entity_id,
            target_rx: x,
            target_ry: y,
        }),
        C::MoveToObject(target_id) => each(&|entity_id| Command::MoveToObject {
            entity_id,
            target_id,
        }),
        C::Stop => each(&|entity_id| Command::Stop { entity_id }),
        C::Attack(target_id) => each(&|attacker_id| Command::Attack {
            attacker_id,
            target_id,
        }),
        C::AttackMove(x, y) => each(&|entity_id| Command::AttackMove {
            entity_id,
            target: None,
            destination: Some(TargetKind::Cell(x, y)),
        }),
        C::Guard(target) => each(&|entity_id| Command::Guard {
            entity_id,
            target: target.map(TargetKind::Entity),
        }),
        C::GuardArea(x, y) => each(&|entity_id| Command::Guard {
            entity_id,
            target: Some(TargetKind::Cell(x, y)),
        }),
        C::DeployMcv => each(&|entity_id| Command::DeployMcv { entity_id }),
        C::UndeployBuilding => each(&|entity_id| Command::UndeployBuilding { entity_id }),
        C::SetRally(rx, ry) => vec![Command::SetRally {
            rx,
            ry,
            producer_ids: units.to_vec(),
        }],
        C::QueueProduction(id) => vec![Command::QueueProduction {
            type_id: techno(id)?,
        }],
        C::SuspendProduction(id) => vec![Command::SuspendProduction {
            category: category_for_object(object(id)?),
        }],
        C::CancelProductionByType(id) => vec![Command::CancelProductionByType {
            type_id: techno(id)?,
            all: false,
        }],
        C::PlaceReadyBuilding(id, rx, ry) => vec![Command::PlaceReadyBuilding {
            type_id: techno(id)?,
            rx,
            ry,
        }],
        C::SellBuilding => each(&|entity_id| Command::SellBuilding { entity_id }),
        C::ToggleRepair => each(&|entity_id| Command::ToggleRepair { entity_id }),
        C::UnloadPassengers => each(&|transport_id| Command::UnloadPassengers { transport_id }),
        C::CaptureBuilding(target_building_id) => each(&|engineer_id| Command::CaptureBuilding {
            engineer_id,
            target_building_id,
        }),
        C::PlantC4(target_building_id) => each(&|attacker_id| Command::PlantC4 {
            attacker_id,
            target_building_id,
        }),
        C::LaunchSuperWeapon(id, target_rx, target_ry) => {
            let name = rules.super_weapon_order.get(usize::try_from(id).ok()?)?;
            vec![Command::LaunchSuperWeapon {
                sw_type_id: sim.interner.get(name)?,
                target_rx,
                target_ry,
            }]
        }
        C::ToggleInfantryDeploy => each(&|entity_id| Command::ToggleInfantryDeploy { entity_id }),
        C::ForceAttackCell(target_rx, target_ry) => each(&|attacker_id| Command::ForceAttackCell {
            attacker_id,
            target_rx,
            target_ry,
        }),
        C::SellWallAtCell(x, y) => vec![Command::SellWallAtCell {
            x: x as i16,
            y: y as i16,
        }],
        C::Scatter => each(&|entity_id| Command::Scatter { entity_id }),
        C::TogglePrimaryFactory => each(&|entity_id| Command::TogglePrimaryFactory { entity_id }),
        C::Enter(target) => {
            // The entry click's mission: Eaten at a Grinder, else Enter.
            let grinder = sim
                .entities()
                .get(target)
                .filter(|entity| {
                    entity.category() == crate::map::entities::EntityCategory::Structure
                })
                .and_then(|entity| sim.entity_object_type(entity, rules))
                .is_some_and(|object| object.grinding);
            let mission = if grinder {
                MissionType::Eaten
            } else {
                MissionType::Enter
            };
            each(&|entity_id| {
                Command::clicked_mission(entity_id, mission, TargetKind::Entity(target))
            })
        }
        C::Harvest(x, y) => each(&|entity_id| {
            Command::clicked_mission(entity_id, MissionType::Harvest, TargetKind::Cell(x, y))
        }),
        C::TakeProduct(kind) => vec![Command::PlaceProducedMobile {
            category: match kind {
                2 => ProductionCategory::Infantry,
                3 => ProductionCategory::Vehicle,
                4 => ProductionCategory::Ship,
                5 => ProductionCategory::Aircraft,
                _ => return None,
            },
        }],
    })
}

/// One command as module events describe it (appendix 4.3 `Commanded` and
/// `OrderRefused`, 4.5 `CommandAllowed`): its appendix 8.3 number, its
/// actors (none for a house-wide command), the target object and cell,
/// and for a house-wide command the type it names (−1 for none).
pub(super) struct CommandRequest {
    pub(super) number: i32,
    pub(super) actors: Vec<u64>,
    pub(super) other: u64,
    pub(super) cell: Option<(u16, u16)>,
    pub(super) id: i32,
}

/// The request of a command the receiver executes; `None` for one without
/// an 8.3 number. A player's production, placement and superweapon records
/// read as the semantic commands they stand for
/// (`Simulation::house_record_command`).
pub(super) fn command_request(
    sim: &Simulation,
    rules: &RuleSet,
    command: &Command,
) -> Option<CommandRequest> {
    let request = |number, actor, other, cell| {
        Some(CommandRequest {
            number,
            actors: vec![actor],
            other,
            cell,
            id: -1,
        })
    };
    let house = |number, cell, id| {
        Some(CommandRequest {
            number,
            actors: Vec::new(),
            other: 0,
            cell,
            id,
        })
    };
    // A MegaMission token: an object goes to `other`, a cell to `cell`.
    let token = |kind: &Option<TargetKind>| match *kind {
        Some(TargetKind::Entity(id)) => (id, None),
        Some(TargetKind::Cell(x, y)) => (0, Some((x, y))),
        None => (0, None),
    };
    let techno = |type_id: InternedId| {
        rules
            .type_handle(sim.interner.resolve(type_id))
            .and_then(|handle| i32::try_from(handle.0).ok())
            .unwrap_or(-1)
    };
    match *command {
        Command::Move {
            entity_id,
            target_rx,
            target_ry,
        } => request(0, entity_id, 0, Some((target_rx, target_ry))),
        Command::MoveToObject {
            entity_id,
            target_id,
        } => request(1, entity_id, target_id, None),
        Command::Stop { entity_id } => request(2, entity_id, 0, None),
        Command::Attack {
            attacker_id,
            target_id,
        } => request(3, attacker_id, target_id, None),
        Command::AttackMove {
            entity_id,
            ref target,
            ref destination,
        } => {
            // The object attacked, else the destination's object; the
            // destination's cell, else the attacked cell.
            let (target_object, target_cell) = token(target);
            let (destination_object, destination_cell) = token(destination);
            let other = if target_object != 0 {
                target_object
            } else {
                destination_object
            };
            request(4, entity_id, other, destination_cell.or(target_cell))
        }
        Command::Guard {
            entity_id,
            ref target,
        } => {
            let (other, cell) = token(target);
            request(5, entity_id, other, cell)
        }
        Command::DeployMcv { entity_id } => request(6, entity_id, 0, None),
        Command::UndeployBuilding { entity_id } => request(7, entity_id, 0, None),
        Command::SetRally {
            rx,
            ry,
            ref producer_ids,
        } => Some(CommandRequest {
            number: 8,
            actors: producer_ids.clone(),
            other: 0,
            cell: Some((rx, ry)),
            id: -1,
        }),
        Command::QueueProduction { type_id } => house(9, None, techno(type_id)),
        Command::SuspendProduction { .. } => house(10, None, -1),
        Command::CancelProductionByType { type_id, .. } => house(11, None, techno(type_id)),
        Command::PlaceReadyBuilding { type_id, rx, ry } => {
            house(12, Some((rx, ry)), techno(type_id))
        }
        Command::SellBuilding { entity_id } => request(13, entity_id, 0, None),
        Command::ToggleRepair { entity_id } => request(14, entity_id, 0, None),
        Command::EjectBunker { bunker_id } => request(15, bunker_id, 0, None),
        Command::UnloadPassengers { transport_id } => request(16, transport_id, 0, None),
        Command::CaptureBuilding {
            engineer_id,
            target_building_id,
        } => request(17, engineer_id, target_building_id, None),
        Command::PlantC4 {
            attacker_id,
            target_building_id,
        } => request(18, attacker_id, target_building_id, None),
        Command::LaunchSuperWeapon {
            sw_type_id,
            target_rx,
            target_ry,
        } => {
            let id = rules
                .super_weapon_index(sim.interner.resolve(sw_type_id))
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1);
            house(19, Some((target_rx, target_ry)), id)
        }
        Command::ToggleInfantryDeploy { entity_id } => request(20, entity_id, 0, None),
        Command::ForceAttackCell {
            attacker_id,
            target_rx,
            target_ry,
        } => request(21, attacker_id, 0, Some((target_rx, target_ry))),
        Command::SellWallAtCell { x, y } => {
            let cell = u16::try_from(x).ok().zip(u16::try_from(y).ok());
            house(22, cell, -1)
        }
        Command::Scatter { entity_id } => request(23, entity_id, 0, None),
        Command::TogglePrimaryFactory { entity_id } => request(24, entity_id, 0, None),
        // The Enter click (`Command::clicked_mission`): a transport, bunker,
        // depot, refinery or Grinder as the destination.
        Command::MegaMission {
            entity_id,
            mission: MissionType::Enter | MissionType::Eaten,
            ref destination,
            ..
        } => {
            let (other, cell) = token(destination);
            request(25, entity_id, other, cell)
        }
        // The harvest click: the cell as the destination.
        Command::MegaMission {
            entity_id,
            mission: MissionType::Harvest,
            ref destination,
            ..
        } => {
            let (other, cell) = token(destination);
            request(26, entity_id, other, cell)
        }
        Command::PlaceProducedMobile { .. } => house(27, None, -1),
        Command::SynchronizedRecord { ref record } => {
            let command = sim.house_record_command(record, rules)?;
            command_request(sim, rules, &command)
        }
        _ => None,
    }
}

/// The `Commanded` serials of one frame (appendix 4.3 `value2`): the frame
/// in the high bits, then a count of command groups. A group is one module
/// order ([`ExtModules::begin_command_group`](super::ExtModules::begin_command_group)),
/// or the consecutive commands of one house with the same number and target,
/// as a player's click sends them. Not saved: it starts again each frame.
#[derive(Default)]
pub(super) struct CommandSerial {
    frame: u32,
    groups: u32,
    last: Option<(InternedId, i32, u64, Option<(u16, u16)>)>,
}

impl CommandSerial {
    pub(super) fn begin_group(&mut self) {
        self.last = None;
    }

    fn serial(&mut self, frame: u32, key: (InternedId, i32, u64, Option<(u16, u16)>)) -> i64 {
        if self.frame != frame {
            *self = Self {
                frame,
                ..Self::default()
            };
        }
        if self.last != Some(key) {
            self.groups = self.groups.wrapping_add(1);
            self.last = Some(key);
        }
        (i64::from(frame) << 24) | i64::from(self.groups & 0x00FF_FFFF)
    }
}

impl Simulation {
    /// `Commanded` and `OrderRefused` (RFC 0001 6i, module docs) for one
    /// command the receiver ran, applied or refused with `outcome`. Nothing
    /// is read unless a module trigger listens.
    pub(crate) fn raise_module_command_outcome(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        command: &CommandEnvelope,
        outcome: Result<(), CommandRefusal>,
    ) {
        let kind = match outcome {
            Ok(()) => EVENT_COMMANDED,
            Err(_) => EVENT_ORDER_REFUSED,
        };
        if !self.trigger_runtime.module_listens(kind) {
            return;
        }
        let Some(request) = command_request(self, rules, &command.payload) else {
            return;
        };
        let Err(refusal) = outcome else {
            let key = (command.owner, request.number, request.other, request.cell);
            let frame = self.session.binary_frame();
            let serial = self.ext_modules.command_serial.serial(frame, key);
            for &actor in &request.actors {
                let event = ModuleEvent {
                    other: request.other,
                    cell: request.cell,
                    value: i64::from(request.number),
                    value2: serial,
                    ..ModuleEvent::new(EVENT_COMMANDED, actor)
                };
                self.raise_module_event(rules, frame_effects, event);
            }
            return;
        };
        // The house that issued the order.
        let house = self
            .houses
            .index_of(command.owner)
            .and_then(|index| u32::try_from(index).ok());
        let subjects = if request.actors.is_empty() {
            vec![0]
        } else {
            request.actors.clone()
        };
        for subject in subjects {
            // The receiver names what it checked; an actor that is no
            // longer alive explains an unnamed refusal.
            let gone = subject != 0
                && self
                    .entities()
                    .get(subject)
                    .is_none_or(|entity| !entity.is_active());
            let reason = match refusal {
                CommandRefusal::Other if gone => CommandRefusal::Gone,
                reason => reason,
            };
            let event = ModuleEvent {
                other: request.other,
                cell: request.cell,
                value: i64::from(request.number),
                value2: reason as i64,
                id: if subject == 0 { request.id } else { -1 },
                house,
                ..ModuleEvent::new(EVENT_ORDER_REFUSED, subject)
            };
            // Client modules' `SimEvent` ([`super::client_outbox`]).
            self.forward_client_event(rules, &event);
            for handle in self
                .trigger_runtime
                .module_triggers()
                .global(EVENT_ORDER_REFUSED)
            {
                self.offer_module_event(rules, frame_effects, handle, 0, &event);
            }
        }
    }
}
