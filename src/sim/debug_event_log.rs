//! Bounded selected-object history, adapted from @1372704087's PR #2083.
//! Only selected objects (at most32) are enrolled. No clocks are read and no
//! event is constructed while logging is off. This is not gameplay state.

use std::collections::VecDeque;
use std::fmt;

pub const DEBUG_EVENT_LOG_CAPACITY: usize = 64;
pub const DEBUG_EVENT_TEXT_BYTES: usize = 512;
pub const DEBUG_EVENT_ENTITY_LIMIT: usize = 32;

#[derive(Clone, Debug, Default)]
pub struct DebugEventLog {
    events: VecDeque<DebugEvent>,
}

#[derive(Clone, Debug)]
pub struct DebugEvent {
    pub tick: u32,
    pub kind: DebugEventKind,
}

/// Events report observed transitions, not guessed pathfinding causes.
#[derive(Clone, Debug)]
pub enum DebugEventKind {
    MissionChange {
        from: i32,
        to: i32,
    },
    MovementChange {
        moving: bool,
        blocked: bool,
    },
    DestinationChange {
        destination: Option<crate::sim::components::NavTargetRef>,
    },
    MinerStateChange {
        from: u32,
        to: u32,
    },
    LocomotorChange {
        from: Option<crate::rules::locomotor_type::LocomotorKind>,
        to: Option<crate::rules::locomotor_type::LocomotorKind>,
    },
    TeleportPhase {
        phase: Option<crate::sim::movement::teleport_movement::TeleportPhase>,
    },
    SpecialMovementStart {
        kind: &'static str,
    },
    SpecialMovementEnd {
        kind: &'static str,
    },
}

impl fmt::Display for DebugEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissionChange { from, to } => write!(f, "Mission {from} → {to}"),
            Self::MovementChange { moving, blocked } => {
                write!(f, "Moving: {moving}; path blocked: {blocked}")
            }
            Self::DestinationChange { destination } => write!(f, "Destination: {destination:?}"),
            Self::MinerStateChange { from, to } => write!(f, "Harvester state {from} → {to}"),
            Self::LocomotorChange { from, to } => write!(f, "Locomotor {from:?} → {to:?}"),
            Self::TeleportPhase { phase } => write!(f, "Teleport phase: {phase:?}"),
            Self::SpecialMovementStart { kind } => write!(f, "{kind} started"),
            Self::SpecialMovementEnd { kind } => write!(f, "{kind} finished"),
        }
    }
}

impl DebugEventLog {
    pub fn events(&self) -> &VecDeque<DebugEvent> {
        &self.events
    }

    fn push(&mut self, tick: u32, kind: DebugEventKind) {
        if self.events.len() == DEBUG_EVENT_LOG_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(DebugEvent { tick, kind });
    }
}

impl crate::sim::game_entity::GameEntity {
    pub fn debug_log(&self) -> Option<&DebugEventLog> {
        self.debug_log.as_ref()
    }

    pub(crate) fn set_debug_logging(&mut self, enabled: bool) {
        if enabled {
            self.debug_log.get_or_insert_with(DebugEventLog::default);
        } else {
            self.debug_log = None;
        }
    }

    pub(crate) fn push_debug_event(&mut self, tick: u32, event: impl FnOnce() -> DebugEventKind) {
        if let Some(log) = self.debug_log.as_mut() {
            log.push(tick, event());
        }
    }
}

/// Cheap value-only observation around the production object turn. It is
/// created only for an enrolled object and owns no names, paths, or clocks.
#[derive(Clone, Copy)]
pub(crate) struct DebugObservation {
    mission: i32,
    handler: u32,
    miner: bool,
    moving: bool,
    blocked: bool,
    destination: Option<crate::sim::components::NavTargetRef>,
    locomotor: Option<crate::rules::locomotor_type::LocomotorKind>,
    falling: bool,
    teleport: Option<crate::sim::movement::teleport_movement::TeleportPhase>,
}

impl DebugObservation {
    pub(crate) fn capture(entity: &crate::sim::game_entity::GameEntity) -> Option<Self> {
        entity.debug_log.as_ref()?;
        Some(Self {
            mission: entity.mission.effective().raw(),
            handler: entity.mission.handler_state(),
            miner: entity.miner.is_some(),
            moving: crate::sim::movement::motion_query::is_moving(entity) == Some(true),
            blocked: entity.navigation.path_runtime.path_blocked(),
            destination: entity.navigation.nav_com(),
            locomotor: entity.locomotor.as_ref().map(|l| l.active_kind()),
            falling: entity.is_falling_down(),
            teleport: entity
                .locomotor
                .as_ref()
                .and_then(|l| l.teleport_runtime())
                .and_then(|t| t.warp())
                .map(|t| t.phase()),
        })
    }

    pub(crate) fn record(self, entity: &mut crate::sim::game_entity::GameEntity, tick: u32) {
        let Some(after) = Self::capture(entity) else {
            return;
        };
        if self.mission != after.mission {
            entity.push_debug_event(tick, || DebugEventKind::MissionChange {
                from: self.mission,
                to: after.mission,
            });
        }
        if self.miner && after.miner && self.handler != after.handler {
            entity.push_debug_event(tick, || DebugEventKind::MinerStateChange {
                from: self.handler,
                to: after.handler,
            });
        }
        if self.moving != after.moving || self.blocked != after.blocked {
            entity.push_debug_event(tick, || DebugEventKind::MovementChange {
                moving: after.moving,
                blocked: after.blocked,
            });
        }
        if self.destination != after.destination {
            entity.push_debug_event(tick, || DebugEventKind::DestinationChange {
                destination: after.destination,
            });
        }
        if self.locomotor != after.locomotor {
            entity.push_debug_event(tick, || DebugEventKind::LocomotorChange {
                from: self.locomotor,
                to: after.locomotor,
            });
        }
        if self.teleport != after.teleport {
            entity.push_debug_event(tick, || DebugEventKind::TeleportPhase {
                phase: after.teleport,
            });
        }
        if self.falling != after.falling {
            entity.push_debug_event(tick, || {
                if after.falling {
                    DebugEventKind::SpecialMovementStart { kind: "Fall" }
                } else {
                    DebugEventKind::SpecialMovementEnd { kind: "Fall" }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_evicts_oldest_and_disabled_hook_does_not_build_event() {
        let mut entity = crate::sim::game_entity::GameEntity::test_default(1, "E1", "Allies", 2, 2);
        entity.push_debug_event(0, || panic!("disabled hook constructed an event"));
        entity.set_debug_logging(true);
        for tick in 0..100 {
            entity.push_debug_event(tick, || DebugEventKind::MissionChange { from: 0, to: 1 });
        }
        let log = entity.debug_log().unwrap();
        assert_eq!(log.events().len(), DEBUG_EVENT_LOG_CAPACITY);
        assert_eq!(log.events().front().unwrap().tick, 36);
        assert!(
            log.events()
                .iter()
                .all(|event| event.kind.to_string().len() <= DEBUG_EVENT_TEXT_BYTES)
        );
        entity.set_debug_logging(false);
        assert!(entity.debug_log().is_none());
    }
}
