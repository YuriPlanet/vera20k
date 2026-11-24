//! Translate CLI arguments into existing simulation orders.

use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    State,
    Tick { ticks: u32 },
    BuildOptions,
    Placement { type_name: String, x: u16, y: u16 },
    Cell { x: u16, y: u16 },
    Quit,
    Order { order: Order },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Order {
    Move {
        unit: u64,
        x: u16,
        y: u16,
    },
    Stop {
        unit: u64,
    },
    Attack {
        unit: u64,
        target: u64,
    },
    AttackMove {
        unit: u64,
        x: u16,
        y: u16,
    },
    AttackCell {
        unit: u64,
        x: u16,
        y: u16,
    },
    Guard {
        unit: u64,
        x: u16,
        y: u16,
    },
    Deploy {
        unit: u64,
    },
    Rally {
        x: u16,
        y: u16,
        producers: Vec<u64>,
    },
    Build {
        type_name: String,
    },
    CancelBuild {
        type_name: String,
        #[serde(default)]
        all: bool,
    },
    Place {
        type_name: String,
        x: u16,
        y: u16,
    },
    PlaceMobile {
        category: ProductionCategory,
    },
    Sell {
        building: u64,
    },
    Repair {
        building: u64,
    },
    Unload {
        transport: u64,
    },
    Capture {
        unit: u64,
        building: u64,
    },
    SuperWeapon {
        type_name: String,
        x: u16,
        y: u16,
    },
}

impl Order {
    /// An order the simulation admits nothing for. Native's own admission
    /// owner drops such an order; the CLI answers an error instead of
    /// queueing one that silently does nothing.
    pub(super) fn rejects(&self) -> Option<&'static str> {
        match self {
            Self::Rally { producers, .. } if producers.is_empty() => {
                Some("a rally order needs at least one producer")
            }
            _ => None,
        }
    }

    /// The objects an order names besides the ones it commands: the attack
    /// target and the building a capture works on.
    pub(super) fn referenced(&self) -> &[u64] {
        match self {
            Self::Attack { target, .. } => std::slice::from_ref(target),
            Self::Capture { building, .. } => std::slice::from_ref(building),
            _ => &[],
        }
    }

    pub(super) fn actors(&self) -> &[u64] {
        match self {
            Self::Move { unit, .. }
            | Self::Stop { unit }
            | Self::Attack { unit, .. }
            | Self::AttackMove { unit, .. }
            | Self::AttackCell { unit, .. }
            | Self::Guard { unit, .. }
            | Self::Deploy { unit }
            | Self::Capture { unit, .. } => std::slice::from_ref(unit),
            Self::Sell { building } | Self::Repair { building } => std::slice::from_ref(building),
            Self::Unload { transport } => std::slice::from_ref(transport),
            Self::Rally { producers, .. } => producers,
            _ => &[],
        }
    }

    pub(super) fn cell(&self) -> Option<(u16, u16)> {
        match *self {
            Self::Move { x, y, .. }
            | Self::AttackMove { x, y, .. }
            | Self::AttackCell { x, y, .. }
            | Self::Guard { x, y, .. }
            | Self::Rally { x, y, .. }
            | Self::Place { x, y, .. }
            | Self::SuperWeapon { x, y, .. } => Some((x, y)),
            _ => None,
        }
    }

    pub(super) fn into_command(
        self,
        type_id: impl Fn(&str, bool) -> Result<InternedId, String>,
    ) -> Result<Command, String> {
        Ok(match self {
            Self::Move { unit, x, y } => Command::Move {
                entity_id: unit,
                target_rx: x,
                target_ry: y,
            },
            Self::Stop { unit } => Command::Stop { entity_id: unit },
            Self::Attack { unit, target } => Command::Attack {
                attacker_id: unit,
                target_id: target,
            },
            Self::AttackMove { unit, x, y } => Command::AttackMove {
                entity_id: unit,
                target: None,
                destination: Some(TargetKind::Cell(x, y)),
            },
            Self::AttackCell { unit, x, y } => Command::ForceAttackCell {
                attacker_id: unit,
                target_rx: x,
                target_ry: y,
            },
            Self::Guard { unit, x, y } => Command::Guard {
                entity_id: unit,
                target: Some(TargetKind::Cell(x, y)),
            },
            Self::Deploy { unit } => Command::DeployMcv { entity_id: unit },
            Self::Rally { x, y, producers } => Command::SetRally {
                rx: x,
                ry: y,
                producer_ids: producers,
            },
            Self::Build { type_name } => Command::QueueProduction {
                type_id: type_id(&type_name, false)?,
            },
            Self::CancelBuild { type_name, all } => Command::CancelProductionByType {
                type_id: type_id(&type_name, false)?,
                all,
            },
            Self::Place { type_name, x, y } => Command::PlaceReadyBuilding {
                type_id: type_id(&type_name, false)?,
                rx: x,
                ry: y,
            },
            Self::PlaceMobile { category } => Command::PlaceProducedMobile { category },
            Self::Sell { building } => Command::SellBuilding {
                entity_id: building,
            },
            Self::Repair { building } => Command::ToggleRepair {
                entity_id: building,
            },
            Self::Unload { transport } => Command::UnloadPassengers {
                transport_id: transport,
            },
            Self::Capture { unit, building } => Command::CaptureBuilding {
                engineer_id: unit,
                target_building_id: building,
            },
            Self::SuperWeapon { type_name, x, y } => Command::LaunchSuperWeapon {
                sw_type_id: type_id(&type_name, true)?,
                target_rx: x,
                target_ry: y,
            },
        })
    }
}
