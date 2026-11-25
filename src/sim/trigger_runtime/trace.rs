//! Dev campaign trigger-trace capture: the Spring and trace-event records,
//! the recorder state and the methods that push them. Observation only:
//! nothing here feeds back into simulation state.

use super::{EVENT_ANY, TagEventInput, TriggerRuntime};
use crate::sim::world::{FrameEffects, Simulation};
use std::collections::BTreeMap;

/// Records of the dev campaign trigger-trace capture, taken after each step.
/// Observation only: nothing here feeds back into simulation state.
#[cfg(any(test, feature = "dev-overlays"))]
#[derive(Debug, Clone, Default)]
pub(super) struct TriggerTrace {
    pub(super) springs: Vec<TriggerSpringRecord>,
    pub(super) events: Vec<TriggerTraceEvent>,
    pub(super) next_seq: u32,
    /// The (Spring `seq`, action ordinal) each nested record happens in.
    pub(super) parents: Vec<(u32, usize)>,
    /// Inside a poll or House raise loop, whose own raises are not recorded.
    pub(super) window: bool,
    /// The frame of the latest record, for records made without a session.
    frame: u32,
    /// The latest recorded (result, persist) of each (Trigger, event ordinal).
    predicates: BTreeMap<(usize, usize), (bool, bool)>,
}

#[cfg(any(test, feature = "dev-overlays"))]
impl TriggerTrace {
    pub(super) fn next_seq(&mut self) -> u32 {
        self.next_seq += 1;
        self.next_seq - 1
    }
}

/// A trace record other than a Spring. `seq` orders it among the Springs, so
/// a native replay can rebuild each frame's poll, House raises and the world
/// inputs between them.
#[cfg(any(test, feature = "dev-overlays"))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "event")]
pub enum TriggerTraceEvent {
    /// Logic55AFB0's Tag poll begins (`end` false) or ends.
    Poll {
        seq: u32,
        binary_frame: u32,
        end: bool,
    },
    /// HouseClass::Update's event-8 raise (4F8F87..4F8FBC) begins or ends.
    HouseRaise {
        seq: u32,
        binary_frame: u32,
        house: String,
        end: bool,
    },
    /// A SetGlobal689670/SetLocal689910 write through the shared owner.
    Variable {
        seq: u32,
        binary_frame: u32,
        parent: Option<(u32, usize)>,
        global: bool,
        index: i32,
        set: bool,
    },
    /// A live Tag constructed after the trace started (Team Tags, Tags first
    /// attached at run time), with its Trigger chain.
    TagConstructed {
        seq: u32,
        binary_frame: u32,
        parent: Option<(u32, usize)>,
        tag: TraceTag,
    },
    /// The first record: the live Tags in registry order, the Logic, map and
    /// House Tag vectors, the poll's dirty flags and the set variables.
    Init {
        seq: u32,
        binary_frame: u32,
        tags: Vec<TraceTag>,
        logic: Vec<usize>,
        map: Vec<usize>,
        houses: Vec<(String, Vec<usize>)>,
        dirty: [bool; 4],
        globals: Vec<u32>,
        locals: Vec<u32>,
    },
    /// A Tag6E53A0 raise from outside the poll and House raise loops, or one
    /// nested in an action. The loops' own raises are left to the replay.
    Dispatch {
        seq: u32,
        binary_frame: u32,
        parent: Option<(u32, usize)>,
        tag: usize,
        kind: i32,
        force: bool,
        actor: Option<u64>,
        cell: (i16, i16),
    },
    /// A HasOccurred71E940 result for an event kind that reads the world,
    /// recorded when it differs from the same event's previous result.
    /// `persist` is set when this call wrote the persistence flag.
    Predicate {
        seq: u32,
        binary_frame: u32,
        parent: Option<(u32, usize)>,
        trigger: usize,
        ordinal: usize,
        result: bool,
        persist: bool,
    },
    /// A reinforcement member Place_Reinforcement_Members (0x65E010) could
    /// not place: every direction refused, or the nearby search found no
    /// Cell (`cell` (0,0)). Its residual (the next member left in limbo
    /// natively) needs a refused placement followed by another member; with
    /// no Cell from the start, native deletes every member (0x65E601,
    /// 0x65E618) and leaves none.
    PlacementRefused {
        seq: u32,
        binary_frame: u32,
        parent: Option<(u32, usize)>,
        cell: (i16, i16),
    },
    /// A Tag's attachment count (+2C) after an attach or detach.
    Refs {
        seq: u32,
        binary_frame: u32,
        parent: Option<(u32, usize)>,
        tag: usize,
        refs: i32,
    },
}

/// A live Tag in a trace record: its index, TagType ID, attachment count and
/// Trigger chain (live index, TriggerType ID, enabled), first to last.
#[cfg(any(test, feature = "dev-overlays"))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TraceTag {
    pub tag: usize,
    pub tag_type: String,
    pub refs: i32,
    pub triggers: Vec<(usize, String, bool)>,
}

/// Event kinds a native replay computes itself from the trigger state, the
/// frame and the variables; the trace records every other kind's results.
#[cfg(any(test, feature = "dev-overlays"))]
const REPLAYED_EVENT_KINDS: [i32; 8] = [EVENT_ANY, 13, 14, 27, 28, 36, 37, 47];

/// One Spring7265C0 call, recorded for the dev campaign trigger-trace capture.
/// Observation only: it never feeds back into simulation state.
#[cfg(any(test, feature = "dev-overlays"))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TriggerSpringRecord {
    /// Order among all trace records.
    pub seq: u32,
    /// The (Spring `seq`, action ordinal) this Spring happens in, if nested.
    pub parent: Option<(u32, usize)>,
    pub binary_frame: u32,
    /// The TriggerType ID, as authored in the map's [Triggers].
    pub trigger: String,
    /// The live Tag holding the Trigger and its TagType ID, if any.
    pub tag: Option<(usize, String)>,
    /// Whether the raise was forced (Force bypasses the event predicates).
    pub forced: bool,
    /// The event kind raised to the Tag (0 for a Trigger forced by Force22).
    pub raised_kind: i32,
    /// The object Spring received (stable ID and type ID), if any.
    pub actor: Option<(u64, String)>,
    /// The packed Cell Spring received; (0,0) is the empty Cell.
    pub cell: (i16, i16),
    /// Each executed action kind and whether Execute reported it handled.
    pub actions: Vec<(i32, bool)>,
}

impl Simulation {
    /// Push a Spring record; returns its index and `seq` while recording.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub(super) fn trace_spring(
        &mut self,
        live: usize,
        type_index: usize,
        input: TagEventInput,
    ) -> Option<(usize, u32)> {
        let runtime = &self.trigger_runtime;
        runtime.trace.as_ref()?;
        let tag = runtime.tag_order.iter().copied().find(|&tag| {
            runtime
                .tag(tag)
                .is_some_and(|live_tag| live_tag.triggers.contains(&live))
        });
        let tag = tag.map(|tag| {
            let tag_type = runtime
                .tag(tag)
                .expect("the Tag was found live just above")
                .type_index;
            (tag, runtime.definitions.tag_types[tag_type].id.clone())
        });
        let record = TriggerSpringRecord {
            seq: 0,
            parent: None,
            binary_frame: self.session.binary_frame(),
            trigger: runtime.definitions.trigger_types[type_index].id.clone(),
            tag,
            forced: input.force,
            raised_kind: input.kind,
            actor: input.actor.map(|id| {
                let type_id = self
                    .substrate
                    .entities
                    .get(id)
                    .map_or("?", |entity| self.interner.resolve(entity.type_ref()));
                (id, type_id.to_owned())
            }),
            cell: input.cell,
            actions: Vec::new(),
        };
        let trace = self.trigger_runtime.trace.as_mut()?;
        let seq = trace.next_seq();
        let parent = trace.parents.last().copied();
        trace.springs.push(TriggerSpringRecord {
            seq,
            parent,
            ..record
        });
        Some((trace.springs.len() - 1, seq))
    }

    /// Push a non-Spring trace record built from its `seq`, `binary_frame`
    /// and parent while recording.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub(super) fn trace_event(
        trigger_runtime: &mut TriggerRuntime,
        binary_frame: u32,
        event: impl FnOnce(u32, u32, Option<(u32, usize)>) -> TriggerTraceEvent,
    ) {
        if let Some(trace) = trigger_runtime.trace.as_mut() {
            let seq = trace.next_seq();
            let parent = trace.parents.last().copied();
            let event = event(seq, binary_frame, parent);
            if let TriggerTraceEvent::Poll { end, .. } | TriggerTraceEvent::HouseRaise { end, .. } =
                event
            {
                trace.window = !end;
            }
            trace.frame = binary_frame;
            trace.events.push(event);
        }
    }

    /// Record a reinforcement member the placement could not place.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub(crate) fn trace_placement_refused(&mut self, cell: (i16, i16)) {
        Self::trace_event(
            &mut self.trigger_runtime,
            self.session.binary_frame(),
            |seq, binary_frame, parent| TriggerTraceEvent::PlacementRefused {
                seq,
                binary_frame,
                parent,
                cell,
            },
        );
    }

    /// Record a HasOccurred71E940 result for a world-reading event kind when
    /// it differs from that event's previous recorded result.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub(super) fn trace_predicate(
        &mut self,
        trigger: usize,
        ordinal: usize,
        result: bool,
        persist: bool,
    ) {
        let runtime = &mut *self.trigger_runtime;
        let Some(trace) = runtime.trace.as_mut() else {
            return;
        };
        let type_index = runtime.triggers[trigger]
            .as_ref()
            .expect("callers trace a live Trigger")
            .type_index;
        let kind = runtime.definitions.trigger_types[type_index].events[ordinal]
            .condition
            .kind;
        if REPLAYED_EVENT_KINDS.contains(&kind)
            || trace
                .predicates
                .insert((trigger, ordinal), (result, persist))
                == Some((result, persist))
        {
            return;
        }
        let seq = trace.next_seq();
        let parent = trace.parents.last().copied();
        trace.events.push(TriggerTraceEvent::Predicate {
            seq,
            binary_frame: self.session.binary_frame(),
            parent,
            trigger,
            ordinal,
            result,
            persist,
        });
    }

    /// Start or stop recording the dev trigger-trace capture. Stopping drops
    /// anything not yet taken. Starting records the live Tags first.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub fn record_trigger_springs(&mut self, on: bool) {
        self.trigger_runtime.trace = on.then(TriggerTrace::default);
        if !on {
            return;
        }
        let runtime = &self.trigger_runtime;
        let tags = runtime
            .tag_order
            .iter()
            .map(|&tag| runtime.trace_tag(tag))
            .collect();
        let houses = runtime
            .house_tags
            .iter()
            .map(|(&house, tags)| (self.interner.resolve(house).to_owned(), tags.clone()))
            .collect();
        let init = TriggerTraceEvent::Init {
            seq: 0,
            binary_frame: self.session.binary_frame(),
            tags,
            logic: runtime.logic_tags.clone(),
            map: runtime.map_tags.clone(),
            houses,
            dirty: runtime.dirty,
            globals: runtime.globals_set.iter().copied().collect(),
            locals: runtime.locals_set.iter().copied().collect(),
        };
        let trace = self
            .trigger_runtime
            .trace
            .as_mut()
            .expect("recording was switched on at the top");
        trace.next_seq = 1;
        trace.frame = self.session.binary_frame();
        trace.events.push(init);
    }

    /// The Spring calls and other trace records made since the last take,
    /// each oldest first.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub fn take_trigger_trace(&mut self) -> (Vec<TriggerSpringRecord>, Vec<TriggerTraceEvent>) {
        self.trigger_runtime
            .trace
            .as_mut()
            .map(|trace| {
                (
                    std::mem::take(&mut trace.springs),
                    std::mem::take(&mut trace.events),
                )
            })
            .unwrap_or_default()
    }

    /// Diagnostic admission for a later campaign countdown without playing
    /// its preceding objectives. Replay only the parsed timer actions through
    /// their existing dispatcher; do not enable the source trigger or supply
    /// elapsed time. Shipping builds omit this entry point.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub fn dev_replay_scenario_timer_actions(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        trigger_id: &str,
    ) -> Option<usize> {
        let source = self.trigger_runtime.definitions.trigger_type(trigger_id)?;
        let actions: Vec<_> = self.trigger_runtime.definitions.trigger_types[source]
            .actions
            .iter()
            .filter(|action| matches!(action.entry.kind, 23..=27 | 103))
            .cloned()
            .collect();
        if actions.is_empty() {
            return None;
        }
        let mut effects = Vec::new();
        for action in &actions {
            if !self.apply_live_trigger_action(
                action,
                self.trigger_action_context(source, None, TagEventInput::default(), rules),
                &mut effects,
                rules,
                None,
                &mut FrameEffects::default(),
            ) {
                return None;
            }
        }
        debug_assert!(effects.is_empty());
        Some(actions.len())
    }
}

impl TriggerRuntime {
    /// A live Tag as trace records show it.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub(super) fn trace_tag(&self, tag: usize) -> TraceTag {
        let live = self.tag(tag).expect("callers trace a live Tag");
        TraceTag {
            tag,
            tag_type: self.definitions.tag_types[live.type_index].id.clone(),
            refs: live.refs,
            triggers: live
                .triggers
                .iter()
                .map(|&trigger| {
                    let live = self
                        .trigger(trigger)
                        .expect("a live Tag chains only live Triggers");
                    let id = self.definitions.trigger_types[live.type_index].id.clone();
                    (trigger, id, live.enabled)
                })
                .collect(),
        }
    }

    /// Record a Tag's attachment count after it changed.
    #[cfg(any(test, feature = "dev-overlays"))]
    pub(super) fn trace_refs(&mut self, tag: usize) {
        let refs = self.tag(tag).expect("callers trace a live Tag").refs;
        if let Some(trace) = self.trace.as_mut() {
            let seq = trace.next_seq();
            let parent = trace.parents.last().copied();
            trace.events.push(TriggerTraceEvent::Refs {
                seq,
                binary_frame: trace.frame,
                parent,
                tag,
                refs,
            });
        }
    }
}
