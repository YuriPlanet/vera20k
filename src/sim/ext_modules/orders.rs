//! House orders (RFC 0001 section 6g): `order(house, ptr, len)`.
//!
//! `order` checks the house, the command table (appendix 8.3) and the
//! per-frame limits, and queues the order in the calling module's slot. The
//! queue is module state: saved with the slot and hashed. The frame after
//! the call, the command step runs the frame's player commands first, then
//! each module's orders, mod before map and in issue order
//! ([`Simulation::run_ext_module_orders`]). Each order becomes the player
//! commands its house's player would send, executed by the same receiver
//! (`apply_command_with_overlays`) that executes received player commands,
//! so it passes the same checks against the state at that moment. Orders
//! never enter the network command stream: every machine's modules issue
//! them in lockstep, and replays resimulate the modules.
//!
//! Residuals:
//! - `SuspendProduction`, `EjectBunker` and `Enter` answer `Unsupported`:
//!   the first needs the production category of a type, `EjectBunker` is a
//!   record no player input sends, and `Enter` has no single player command.
//! - `IgnoreOrders` is not kept yet, so no unit ignores a module order.
//! - With custom control grants (shared houses), the house-wide commands
//!   (production, placement, superweapons) need a granted sender, which a
//!   module order has not: the receiver refuses them.
//! - The orders and unit entries per module per frame are provisional
//!   (open question 7).

use serde::{Deserialize, Serialize};
use vera20k_wasm_ext::{Invalid, NAME_SUPER_WEAPON, NAME_TECHNO_TYPE, Order, Status};

use super::{Host, Ref, decode_ref};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::Simulation;

/// Orders one module may issue per frame.
pub const MAX_ORDERS_PER_FRAME: usize = 64;
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

/// An order's command with its checked target and value.
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
}

/// What a command takes in `target` (appendix 8.3 command table).
#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetKindSpec {
    None,
    Cell,
    Object,
    ObjectOrNone,
}

/// What a command takes in `value`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueSpec {
    None,
    Techno,
    Building,
    SuperWeapon,
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
        0 | 4 | 8 | 21 => (T::Cell, V::None, true),
        1 | 3 | 17 | 18 => (T::Object, V::None, true),
        5 => (T::ObjectOrNone, V::None, true),
        2 | 6 | 7 | 13 | 14 | 16 | 20 | 23 | 24 => (T::None, V::None, true),
        9 | 11 => (T::None, V::Techno, false),
        12 => (T::Cell, V::Building, false),
        19 => (T::Cell, V::SuperWeapon, false),
        22 => (T::Cell, V::None, false),
        // 10 SuspendProduction, 15 EjectBunker, 25 Enter and unknown
        // commands (module residuals).
        _ => return None,
    })
}

impl Host<'_, '_> {
    /// `order`: checks and queues one order.
    pub(super) fn queue_order(&mut self, house: i64, order: &Order) -> Result<Status, Invalid> {
        let Some(owner) = self.house(house)? else {
            return Err(Invalid(format!("{house:#x} names no house")));
        };
        let Some(state) = self.sim.houses.get(&owner) else {
            return Err(Invalid(format!("{house:#x} names no house")));
        };
        // RFC 6g "Houses": only a computer house that is not defeated.
        if state.is_human()
            || state.is_defeated()
            || self.sim.session.current_house() == Some(owner)
        {
            return Ok(Status::Refused);
        }
        let Some((target_spec, value_spec, takes_units)) = command_spec(order.kind) else {
            return Ok(Status::Unsupported);
        };
        let issued = self.orders.iter().filter(|o| o.frame == self.frame_now());
        let (count, units) = issued.fold((0, 0), |(n, u), o| (n + 1, u + o.units.len()));
        if count >= MAX_ORDERS_PER_FRAME || units + order.units.len() > MAX_UNITS_PER_FRAME {
            return Ok(Status::Refused);
        }
        if takes_units == order.units.is_empty() {
            return Ok(Status::Refused);
        }
        let mut unit_ids = Vec::with_capacity(order.units.len());
        for &unit in &order.units {
            match decode_ref(unit)? {
                Some(Ref::Object(id)) => unit_ids.push(id),
                _ => return Ok(Status::Refused),
            }
        }
        let target = match (target_spec, decode_ref(order.target)?) {
            (TargetKindSpec::None | TargetKindSpec::ObjectOrNone, None) => Target::None,
            (TargetKindSpec::Object | TargetKindSpec::ObjectOrNone, Some(Ref::Object(id))) => {
                Target::Object(id)
            }
            (TargetKindSpec::Cell, Some(Ref::Cell { x, y })) => Target::Cell(x, y),
            (TargetKindSpec::Cell, Some(Ref::Waypoint(number))) => {
                match self.waypoint_cell(number) {
                    Some((x, y)) => Target::Cell(x, y),
                    None => return Ok(Status::NotFound),
                }
            }
            _ => return Ok(Status::Refused),
        };
        let value = match (value_spec, decode_ref(order.value)?) {
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
                    .and_then(|id| self.techno_type(id))
                    .ok_or_else(|| Invalid(format!("no TechnoType {id}")))?;
                if value_spec == ValueSpec::Building
                    && object.category != crate::rules::object_type::ObjectCategory::Building
                {
                    return Ok(Status::Refused);
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
                    .and_then(|i| self.rules.super_weapon_order.get(i))
                    .is_none()
                {
                    return Err(Invalid(format!("no SuperWeapon {id}")));
                }
                Some(id)
            }
            _ => return Ok(Status::Refused),
        };
        let Some(command) = order_command(order.kind, target, value) else {
            return Ok(Status::Refused);
        };
        let Some(index) = self.sim.houses.index_of(owner) else {
            return Ok(Status::Gone);
        };
        self.orders.push(PendingOrder {
            house: index,
            frame: self.frame_now(),
            command,
            units: unit_ids,
        });
        Ok(Status::Ok)
    }

    fn frame_now(&self) -> u32 {
        self.sim.session.binary_frame()
    }

    /// A waypoint's cell; `None` for a waypoint the map does not set.
    pub(super) fn waypoint_cell(&self, number: i32) -> Option<(u16, u16)> {
        if !self.sim.session.waypoint_is_valid(number) {
            return None;
        }
        let (x, y) = self.sim.session.waypoint_cell(number)?;
        Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?))
    }
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
        (5, Target::None, _) => C::Guard(None),
        (6, ..) => C::DeployMcv,
        (7, ..) => C::UndeployBuilding,
        (8, Target::Cell(x, y), _) => C::SetRally(x, y),
        (9, _, Some(id)) => C::QueueProduction(id),
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
            let units: Vec<u64> = order
                .units
                .iter()
                .copied()
                .filter(|id| {
                    sim.entities()
                        .get(*id)
                        .is_some_and(|entity| entity.owner() == owner)
                })
                .collect();
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
    let techno = |id: u32| {
        let object = rules.all_objects().nth(usize::try_from(id).ok()?)?;
        sim.interner.get(&object.id)
    };
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
    })
}
