//! The two things a brain may ask for, and where each one is applied.
//!
//! - [`Order`]: one object, one [`Goal`]. Translated by the L1 funnel into the
//!   command a click would produce, then admitted and queued.
//! - [`Intent`]: house-scoped work — a build, a placement, a sale — that a
//!   player's hands do through the sidebar rather than by clicking the map.
//!
//! Both are *requests*. Naming them separately from
//! [`Command`](crate::sim::command::Command) is the point of the layer: a brain
//! says what it wants, L2 decides whether the house may issue it now and which
//! native owner performs it. Production, placement, sale and the queue each
//! keep exactly one owner — this module holds no state of its own and
//! re-implements none of them.
//!
//! Every request is a command. A brain never asks the simulation to hold
//! state that is not a player's own doing: a player's hands cannot raise an AI
//! team or run a house's AI tick, so neither is a request here, and this layer
//! applies nothing through a module of its own.

use crate::sim::behavior::goal::Goal;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;

/// One object order, before admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    pub actor: u64,
    pub goal: Goal,
}

/// House-scoped work: everything a player's hands do through the sidebar
/// rather than by clicking the map. Every variant is a command, so the layer
/// never has to apply anything through a module of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Ask the type's factory to start building it (`HouseClass::Begin_Production`).
    QueueProduction { type_id: InternedId },
    /// Cancel the queued copies of a type (`HouseClass::Abandon_Production`).
    CancelProduction { type_id: InternedId, all: bool },
    /// Put the category's running build on hold (`HouseClass::Suspend_Production`).
    /// A player's sidebar does the same; queueing the type again resumes it.
    SuspendProduction { category: ProductionCategory },
    /// Take the finished product of the category out of its factory
    /// (`Place_Production`'s mobile arm): the click the sidebar's done cameo
    /// takes. A factory holds what it has built until the house takes it, so
    /// a build that is never taken blocks its own category.
    TakeProduct { category: ProductionCategory },
    /// Place a finished building that waits for a cell (`Place_Production`).
    PlaceBuilding {
        type_id: InternedId,
        cell: (u16, u16),
    },
    /// Sell a standing building.
    SellBuilding { entity_id: u64 },
    /// Pack a deployed yard back into its maker (`Command::UndeployBuilding`),
    /// so a captured construction yard can be driven home instead of abandoned.
    UndeployBuilding { entity_id: u64 },
    /// Fire a charged superweapon or house power at a cell: the click on the
    /// sidebar's superweapon button followed by the target. `HouseClass::
    /// Fire_SW` is the owner; a house with no charge, no grant or no warhead
    /// for the type is refused there, not here.
    LaunchSuperWeapon {
        sw_type_id: InternedId,
        cell: (u16, u16),
    },
}

impl Intent {
    /// The command this request is issued as.
    pub fn command(self) -> Command {
        match self {
            Self::QueueProduction { type_id } => Command::QueueProduction { type_id },
            Self::CancelProduction { type_id, all } => {
                Command::CancelProductionByType { type_id, all }
            }
            Self::SuspendProduction { category } => Command::SuspendProduction { category },
            Self::TakeProduct { category } => Command::PlaceProducedMobile { category },
            Self::PlaceBuilding { type_id, cell } => Command::PlaceReadyBuilding {
                type_id,
                rx: cell.0,
                ry: cell.1,
            },
            Self::SellBuilding { entity_id } => Command::SellBuilding { entity_id },
            Self::UndeployBuilding { entity_id } => Command::UndeployBuilding { entity_id },
            Self::LaunchSuperWeapon { sw_type_id, cell } => Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx: cell.0,
                target_ry: cell.1,
            },
        }
    }
}
