//! Tag event dispatch and Trigger polling: House, team, object and cell Tag
//! raises, Tag6E53A0 dispatch, Spring7265C0, Scenario variables and
//! Logic55AFB0's `advance_triggers` poll.

#[cfg(any(test, feature = "dev-overlays"))]
use super::TriggerTraceEvent;
use super::{
    EVENT_ANY, EVENT_ATTACKED, EVENT_BRIDGE_COLLAPSED, MAX_TRIGGER_SPRING_ACTIONS,
    MAX_TRIGGER_SPRING_DEPTH, SCENARIO_GLOBAL_COUNT, TagAttachment, TagEventInput, TeamTagOrder,
    TriggerActionContext, TriggerEffect, TriggerRuntime, resolve_trigger_house,
};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// HouseClass::Update's Tag raise (`0x004F8F87..0x004F8FBC`): every eighth
    /// frame (signed `Frame % 8`), Tag6E53A0 raises event 8 with no object and
    /// the zero Cell on each of the House's Tags (+3C), last to first. The
    /// count (+48) is read once and the list re-read for every slot.
    /// RESIDUAL: a raise that shrinks the list leaves slots native still reads
    /// (the stale pointer past the count); VERA skips them. No retail
    /// skirmish trigger reaches it: their event-8 Tags fire once.
    pub(crate) fn raise_house_tags(
        &mut self,
        house: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        if (self.session.binary_frame() as i32) % 8 != 0 {
            return;
        }
        #[cfg(any(test, feature = "dev-overlays"))]
        let trace_house = |sim: &mut Self, end: bool| {
            let house = sim.interner.resolve(house).to_owned();
            Self::trace_event(
                &mut sim.trigger_runtime,
                sim.session.binary_frame(),
                |seq, binary_frame, _| TriggerTraceEvent::HouseRaise {
                    seq,
                    binary_frame,
                    house,
                    end,
                },
            );
        };
        #[cfg(any(test, feature = "dev-overlays"))]
        trace_house(self, false);
        let count = self
            .trigger_runtime
            .house_tags
            .get(&house)
            .map_or(0, Vec::len);
        let mut effects = Vec::new();
        for slot in (0..count).rev() {
            let Some(&tag) = self
                .trigger_runtime
                .house_tags
                .get(&house)
                .and_then(|tags| tags.get(slot))
            else {
                continue;
            };
            self.dispatch_live_tag_event(
                tag,
                TagEventInput {
                    kind: EVENT_ANY,
                    ..Default::default()
                },
                &mut effects,
                rules,
                overlay_registry,
                frame_effects,
            );
        }
        #[cfg(any(test, feature = "dev-overlays"))]
        trace_house(self, true);
        self.publish_trigger_effects(effects);
    }

    /// `HouseClass::NotifyUnderAttack @ 0x004F93E0`'s Tag raise
    /// (`0x004F95D4..0x004F95FD`), which ends every call after the radar/EVA
    /// arms, for any House: Tag6E53A0 raises event 6 ("attacked by any
    /// house") with no object, the zero Cell `0x00A8EF98` (static init
    /// `0x004F50A0`), unforced and with no source, on each of the House's Tags
    /// (+3C), first to last. Every slot re-reads the list (`0x004F95E3`) and
    /// the count (+48, `0x004F95F7`), so a raise that detaches its Tag shifts
    /// the next one into the slot just passed, which the walk skips, as
    /// natively. Besides event 6, the raise meets event 8 and every event
    /// whose predicate ignores the raised kind (`has_occurred`), so
    /// it also springs a House's condition Triggers (timers, counts) between
    /// the House's own event-8 raises. Its callers own the NotifyUnderAttack
    /// admission. No own RNG draw, timer write or detach:
    /// Register/Spring/actions own those.
    pub(crate) fn raise_house_attacked_tags(
        &mut self,
        house: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        self.raise_house_tags_forward(
            house,
            TagEventInput {
                kind: EVENT_ATTACKED,
                ..Default::default()
            },
            rules,
            overlay_registry,
            frame_effects,
        );
    }

    /// The first-to-last House Tag walk that rereads the list and its count
    /// for every slot: NotifyUnderAttack's above and the Ares superweapon
    /// raises (`RaiseOwnerTagEvent`, Ares.dll 10032020/100325D0).
    pub(super) fn raise_house_tags_forward(
        &mut self,
        house: crate::sim::intern::InternedId,
        input: TagEventInput,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let mut effects = Vec::new();
        let mut slot = 0;
        while let Some(&tag) = self
            .trigger_runtime
            .house_tags
            .get(&house)
            .and_then(|tags| tags.get(slot))
        {
            self.dispatch_live_tag_event(
                tag,
                input,
                &mut effects,
                rules,
                overlay_registry,
                frame_effects,
            );
            slot += 1;
        }
        self.publish_trigger_effects(effects);
    }

    /// Retail gamemd.exe: TeamClass::Recalc6EA49F..6EA4D2 walks the Tag
    /// array backwards; TeamClass::AI6E92D0..6E9320 walks forwards and
    /// revisits the slot after a successful raise (6E9312). Both broadcast
    /// before destroying the empty leaving Team. No own RNG draws, timers
    /// or detach: ordinary Register/Reset/actions own those effects.
    ///
    /// A true one-shot raise destroys a Trigger, so its revisits end. Residual:
    /// Rust revisits a repeating Tag only when the raise shifted another Tag
    /// into the slot. Natively that revisit never ends while one of its
    /// Triggers keeps occurring, and it can first spring a sibling Trigger
    /// that Rust drops.
    pub(crate) fn raise_team_left_map_tags(
        &mut self,
        order: TeamTagOrder,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let mut ordinal = match order {
            TeamTagOrder::Forward => Some(0),
            TeamTagOrder::Reverse => self.trigger_runtime.tag_order.len().checked_sub(1),
        };
        let mut effects = Vec::new();
        while let Some(slot) = ordinal {
            if let Some(&tag) = self.trigger_runtime.tag_order.get(slot) {
                let repeating = self.trigger_runtime.tag(tag).is_some_and(|live| {
                    self.trigger_runtime.definitions.tag_types[live.type_index].repeat == 2
                });
                let raised = self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 23,
                        ..Default::default()
                    },
                    &mut effects,
                    rules,
                    registry,
                    frame_effects,
                );
                if raised && self.trigger_runtime.tag_order.is_empty() {
                    break;
                }
                if raised
                    && order == TeamTagOrder::Forward
                    && (!repeating || self.trigger_runtime.tag_order.get(slot) != Some(&tag))
                {
                    continue;
                }
            } else if order == TeamTagOrder::Forward {
                break;
            }
            // Reverse traversal retains its original count. Shrinking
            // callbacks can leave stale native slots; safe storage skips them.
            ordinal = match order {
                TeamTagOrder::Forward => slot.checked_add(1),
                TeamTagOrder::Reverse => slot.checked_sub(1),
            };
        }
        self.publish_trigger_effects(effects);
    }

    /// Original Tag6E53A0 attached-object ingress. `subject` chooses the
    /// attachment; `actor` and `source` keep the caller's distinct objects.
    /// Discovery6F4A18, Infantry519725/51978B/519B53/519F44/51A025/51A26A/
    /// 51A51C use this same funnel.
    /// Returns whether the caller had a nonnull Tag, independently of whether
    /// an action fired. Pickup481A7F skips its Alive reread without a Tag.
    pub(crate) fn dispatch_object_tag_event(
        &mut self,
        subject: u64,
        kind: i32,
        actor: Option<u64>,
        source: Option<u64>,
        context: crate::sim::world::UninitContext<'_>,
    ) -> bool {
        self.dispatch_object_tag_input(
            subject,
            TagEventInput {
                kind,
                actor,
                source,
                ..Default::default()
            },
            context,
        )
    }

    pub(super) fn dispatch_object_tag_input(
        &mut self,
        subject: u64,
        input: TagEventInput,
        mut context: crate::sim::world::UninitContext<'_>,
    ) -> bool {
        let Some(&tag) = self
            .trigger_runtime
            .attachments
            .get(&TagAttachment::Object(subject))
        else {
            return false;
        };
        let mut effects = Vec::new();
        self.dispatch_live_tag_event(
            tag,
            input,
            &mut effects,
            context.rules(),
            context.registry(),
            context.effects(),
        );
        self.publish_trigger_effects(effects);
        true
    }

    /// FootAI4DA7B0..4DA80C, after high-flight sight and before Process.
    /// Every signed frame divisible by16 raises CellTag59 for a +54 high
    /// flyer, including stationary actors hostile to the local viewer.
    /// Original Unit/Infantry/Aircraft +1B8=41BEA0 and +1BC=5F6960 use
    /// physical Location, independently of the locomotor navigation head.
    /// Source: original retail gamemd instructions and actual class tables;
    /// shared query comparisons: spatial_oracle/object_get_cell and
    /// object_flight_height; caller/getter and live Tag histories:
    /// spatial_oracle/tag_lifecycle.py --periodic-cell-events.
    /// No own RNG, timer or detach operation.
    pub(crate) fn dispatch_airborne_cell_tag_event(
        &mut self,
        actor: u64,
        mut context: crate::sim::world::UninitContext<'_>,
    ) {
        if self.session.binary_frame() & 15 != 0 {
            return;
        }
        let (tag, cell) = {
            let Some(entity) = self.substrate.entities.get(actor) else {
                return;
            };
            if !entity.category().is_foot() {
                return;
            }
            let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                return;
            };
            let cells = NativeCellQuery::canonical(terrain);
            if !crate::sim::movement::air_movement::is_high_flying_in_query(
                entity,
                &cells,
                Some((context.rules(), &self.interner)),
            ) {
                return;
            }
            let physical =
                || crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            //4DA7D2 tests the first Cell's Tag;4DA7F8 obtains the receiver
            //again after41BEA0 packs Location XY. Each5F6960 preserves both
            //Map565730 lookups and the canonical Dummy's retained fields.
            let first = crate::sim::movement::ground_pose::query_object_cell(&cells, physical());
            if !self.trigger_runtime.cell_tags.contains_key(&first) {
                return;
            }
            let cell = crate::sim::movement::coord_cell(physical());
            let second = crate::sim::movement::ground_pose::query_object_cell(&cells, physical());
            let Some(&tag) = self.trigger_runtime.cell_tags.get(&second) else {
                return;
            };
            (tag, cell)
        };
        let mut effects = Vec::new();
        self.dispatch_live_tag_event(
            tag,
            TagEventInput {
                kind: 59,
                actor: Some(actor),
                cell,
                ..Default::default()
            },
            &mut effects,
            context.rules(),
            context.registry(),
            context.effects(),
        );
        self.publish_trigger_effects(effects);
    }

    /// Bridge575EE0's per-cell ingress: each visited CellClass whose +3C Tag
    /// is nonnull calls Tag6E53A0(31, null actor, Cell ABD480, unforced,
    /// null source). ABD480 is the zero Cell (startup5618B0). The raise is
    /// synchronous inside the collapse traversal; HasOccurred71E940 admits
    /// it through71EC9A/71F069 (raised31, clear A8ED6B) without latching.
    pub(crate) fn dispatch_bridge_cell_tag_event(
        &mut self,
        cell: crate::map::cell_index::NativeCellIdentity,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let Some(&tag) = self.trigger_runtime.cell_tags.get(&cell) else {
            return;
        };
        let mut effects = Vec::new();
        self.dispatch_live_tag_event(
            tag,
            TagEventInput {
                kind: EVENT_BRIDGE_COLLAPSED,
                ..Default::default()
            },
            &mut effects,
            rules,
            overlay_registry,
            frame_effects,
        );
        self.publish_trigger_effects(effects);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn tag_dispatch_trace_for_test(&self) -> &[(usize, i32)] {
        &self.trigger_runtime.dispatch_trace
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn clear_tag_dispatch_trace_for_test(&mut self) {
        self.trigger_runtime.dispatch_trace.clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn trigger_local_is_set_for_test(&self, index: u32) -> bool {
        self.trigger_runtime.locals_set.contains(&index)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn cell_trigger_tag_for_test(
        &self,
        cell: crate::map::cell_index::NativeCellIdentity,
    ) -> Option<usize> {
        self.trigger_runtime.cell_tags.get(&cell).copied()
    }

    /// Foot4D8978..4D8A6B, reached from the unique Arrival per-cell owner.
    /// Cell5657A0 and its Tag pointer are retained before the first callback;
    /// each raise re-reads the actor's packed Location cell. Bridge100/400
    /// requires OnBridge8C. This does not invent a Fly per-cell producer.
    /// Source: tools/spatial_oracle/tag_lifecycle.py --entered-cell-events
    /// and --spy-entry-events;
    /// original Steam gamemd. No own RNG/timer/detach.
    pub(crate) fn dispatch_foot_cell_tag_events(
        &mut self,
        actor: u64,
        mut context: crate::sim::world::UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(actor) else {
            return;
        };
        let cell = crate::sim::movement::coord_cell(
            crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        );
        let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
            return;
        };
        let mut identity = NativeCellQuery::canonical(terrain).lookup(cell);
        let direct_tag = (terrain.native_cell_flags(identity) & 0x500 == 0 || entity.on_bridge())
            .then(|| self.trigger_runtime.cell_tags.get(&identity).copied())
            .flatten();
        let mut effects = Vec::new();
        if let Some(tag) = direct_tag {
            for kind in [1, 59] {
                let Some(entity) = self.substrate.entities.get(actor) else {
                    break;
                };
                let cell = crate::sim::movement::coord_cell(
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                );
                self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind,
                        actor: Some(actor),
                        cell,
                        ..Default::default()
                    },
                    &mut effects,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
        }
        //4D8A14..4D8A6B: re-read the Infantry/Agent admission after1/59,
        //then send53 and54 to the cached CellTag. Admission is checked once;
        //each callback re-reads Location even if53 detached that Tag. Its
        //remaining nonpending linked Triggers may still register54.
        let rules = context.rules();
        if let Some(tag) = direct_tag
            && self.substrate.entities.get(actor).is_some_and(|entity| {
                entity.category() == crate::map::entities::EntityCategory::Infantry
                    && self
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|ty| ty.agent)
            })
        {
            for kind in [53, 54] {
                let Some(entity) = self.substrate.entities.get(actor) else {
                    break;
                };
                let cell = crate::sim::movement::coord_cell(
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                );
                self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind,
                        actor: Some(actor),
                        cell,
                        ..Default::default()
                    },
                    &mut effects,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
        }
        //4D8A70..4D8B66: a missing/inadmissible direct CellTag does not
        //suppress line checks. Scan each complete native cell-array row/column,
        //looking up the live attachment at every slot. Input Location is
        //re-read for every callback while the traversal uses the initial XY.
        for kind in [25, 26] {
            let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                break;
            };
            let flag = if kind == 25 { 0x100000 } else { 0x200000 };
            if terrain.native_cell_flags(identity) & flag == 0 {
                continue;
            }
            let bounds =
                crate::sim::scenario_bootstrap::NativeStartBounds::from_session(self, terrain);
            let count = if kind == 25 {
                bounds.width
            } else {
                bounds.height
            };
            for offset in 0..count {
                let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                    break;
                };
                let queried = if kind == 25 {
                    (bounds.min_rx.wrapping_add(offset) as i16, cell.1)
                } else {
                    (cell.0, bounds.min_ry.wrapping_add(offset) as i16)
                };
                let selected = NativeCellQuery::canonical(terrain).lookup(queried);
                //The horizontal loop stores every last lookup into the retained
                //Cell slot4D8AAC; vertical admission then reads that same slot.
                if kind == 25 {
                    identity = selected;
                }
                let Some(tag) = self.trigger_runtime.cell_tags.get(&selected).copied() else {
                    continue;
                };
                let Some(live) = self.trigger_runtime.tag(tag) else {
                    continue;
                };
                if !self
                    .trigger_runtime
                    .definitions
                    .has_event(live.type_index, kind)
                {
                    continue;
                }
                let Some(entity) = self.substrate.entities.get(actor) else {
                    break;
                };
                let cell = crate::sim::movement::coord_cell(
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                );
                self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind,
                        actor: Some(actor),
                        cell,
                        ..Default::default()
                    },
                    &mut effects,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
        }
        // Foot4D8B66..4D8CA5 scans the live Map tag vector with the shared
        // A83DBC cursor. Actions may detach a tag, destroy one (adjusting this
        // cursor), or enter another cell synchronously. Neither a snapshot of
        // the vector nor a local index preserves those callbacks' behavior.
        self.trigger_runtime.map_cursor = 0;
        while let Some(&tag) = self
            .trigger_runtime
            .map_tags
            .get(self.trigger_runtime.map_cursor as usize)
        {
            let attached = self.trigger_runtime.tag(tag).and_then(|live| {
                self.trigger_runtime
                    .definitions
                    .has_event(live.type_index, 24)
                    .then_some(live.attached_cell)
            });
            if let Some(attached) = attached {
                let reachable = (|| {
                    //4D8BAA copies vt+4C's Head_To coordinate before +BC
                    //obtains its own coordinate/layer. A paid head is not
                    //the object's physical Location.
                    let destination = crate::sim::movement::coord_cell(
                        self.foot_navigation_coordinate(actor).ok()?,
                    );
                    let entity = self.substrate.entities.get(actor)?;
                    let current =
                        crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                    let in_tube = entity.active_tube().is_some();
                    let head = if in_tube {
                        current
                    } else {
                        self.foot_navigation_coordinate(actor).ok()?
                    };
                    let terrain = context.terrain().or(self.resolved_terrain.as_ref())?;
                    let cells = NativeCellQuery::canonical(terrain);
                    let destination_bridge =
                        crate::sim::movement::ground_pose::navigation_should_be_on_bridge(
                            &cells,
                            head,
                            current,
                            entity.on_bridge(),
                            in_tube,
                        )
                        .ok()?;
                    //4D8BF4..4D8C39: the Tag's cell is the SOURCE, with its
                    //raw structural bridge bit. Map56D100's fringe shortcuts
                    //are asymmetric; do not reverse these endpoints.
                    let source_bridge = cells.flags(cells.lookup(attached)) & 0x100 != 0;
                    let movement_zone = self.object_type(entity.type_ref(), rules)?.movement_zone;
                    self.zone_grid.as_ref()?.can_reach_native(
                        &cells,
                        crate::sim::pathfinding::zone_map::ZoneQueryCell::Copied(attached),
                        crate::sim::pathfinding::zone_map::ZoneQueryCell::Copied(destination),
                        movement_zone,
                        source_bridge,
                        destination_bridge,
                        false,
                        self.playfield_bounds()?,
                        self.map_size_diamond()?,
                    )
                })();
                if reachable == Some(true) {
                    //4D8C4D re-reads vt+4C for the event payload. Do not
                    //add an Alive gate: earlier callbacks may have changed a
                    //retained actor. Stop only when its storage is gone.
                    let Ok(coordinate) = self.foot_navigation_coordinate(actor) else {
                        break;
                    };
                    self.dispatch_live_tag_event(
                        tag,
                        TagEventInput {
                            kind: 24,
                            actor: Some(actor),
                            cell: crate::sim::movement::coord_cell(coordinate),
                            ..Default::default()
                        },
                        &mut effects,
                        rules,
                        context.registry(),
                        context.effects(),
                    );
                }
            }
            self.trigger_runtime.map_cursor = self.trigger_runtime.map_cursor.wrapping_add(1);
        }
        self.publish_trigger_effects(effects);
        // Ordinary Fly ingress remains unproven.
    }

    pub(super) fn trigger_action_context(
        &self,
        type_index: usize,
        live_index: Option<usize>,
        input: TagEventInput,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> TriggerActionContext {
        TriggerActionContext {
            live_index,
            input,
            incoming_house: resolve_trigger_house(
                self,
                self.trigger_runtime.definitions.trigger_types[type_index]
                    .owner
                    .as_deref(),
                rules,
            ),
        }
    }

    pub(super) fn spring_live_trigger(
        &mut self,
        index: usize,
        input: TagEventInput,
        effects: &mut Vec<TriggerEffect>,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        //7265C0 deliberately does not call RegisterEvent. Force22 springs
        // directly, but disabled/pending instances still reject it.
        let Some(trigger) = self.trigger_runtime.trigger(index) else {
            return false;
        };
        if !trigger.enabled || trigger.pending {
            return false;
        }
        if self.trigger_runtime.spring_depth >= MAX_TRIGGER_SPRING_DEPTH {
            return false;
        }
        let type_index = trigger.type_index;
        let count = self.trigger_runtime.definitions.trigger_types[type_index]
            .actions
            .len();
        if self.trigger_runtime.spring_depth == 0 {
            self.trigger_runtime.spring_actions_left = MAX_TRIGGER_SPRING_ACTIONS;
        }
        self.trigger_runtime.spring_depth += 1;
        #[cfg(any(test, feature = "dev-overlays"))]
        let record = self.trace_spring(index, type_index, input);
        let mut result = false;
        for ordinal in 0..count {
            if self.trigger_runtime.spring_actions_left == 0 {
                break;
            }
            self.trigger_runtime.spring_actions_left -= 1;
            //7265E7..726602 repeats the authored Country lookup for every
            //action and preserves Spring's incoming actor and packed Cell.
            let context = self.trigger_action_context(type_index, Some(index), input, rules);
            // An owned copy: the action runs through `&mut self`, and the
            // Tag events it raises write the same definitions (each event's
            // retained House, Event+54), so it cannot stay borrowed. Dropping
            // the copy means moving `RuntimeEvent::retained_house` out of
            // `RuntimeDefinitions`, which changes the saved snapshot layout
            // and the order `hash_definitions` feeds the state hash.
            let action =
                self.trigger_runtime.definitions.trigger_types[type_index].actions[ordinal].clone();
            #[cfg(any(test, feature = "dev-overlays"))]
            if let (Some((_, seq)), Some(trace)) = (record, self.trigger_runtime.trace.as_mut()) {
                trace.parents.push((seq, ordinal));
            }
            let handled = self.apply_live_trigger_action(
                &action,
                context,
                effects,
                rules,
                overlay_registry,
                frame_effects,
            );
            #[cfg(any(test, feature = "dev-overlays"))]
            if let (Some((record, _)), Some(trace)) = (record, self.trigger_runtime.trace.as_mut())
            {
                trace.parents.pop();
                trace.springs[record]
                    .actions
                    .push((action.entry.kind, handled));
            }
            result |= handled;
        }
        self.trigger_runtime.spring_depth -= 1;
        result
    }
}

impl Simulation {
    pub(super) fn dispatch_live_tag_event(
        &mut self,
        index: usize,
        input: TagEventInput,
        effects: &mut Vec<TriggerEffect>,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        self.trigger_runtime
            .dispatch_trace
            .push((index, input.kind));
        #[cfg(any(test, feature = "dev-overlays"))]
        if self.trigger_runtime.tag(index).is_some()
            && let Some(trace) = self.trigger_runtime.trace.as_mut()
        {
            let parent = trace.parents.last().copied();
            if parent.is_some() || !trace.window {
                let seq = trace.next_seq();
                trace.events.push(TriggerTraceEvent::Dispatch {
                    seq,
                    binary_frame: self.session.binary_frame(),
                    parent,
                    tag: index,
                    kind: input.kind,
                    force: input.force,
                    actor: input.actor,
                    cell: input.cell,
                });
            }
        }
        let Some(tag) = self.trigger_runtime.tag(index) else {
            return false;
        };
        if self.buildup_bypass_active() || tag.pending || tag.dispatch_guard {
            return false;
        }
        let repeat = self.trigger_runtime.definitions.tag_types[tag.type_index].repeat;
        self.trigger_runtime.tags[index]
            .as_mut()
            .expect("the Tag was read live just above")
            .dispatch_guard = true;
        let mut result = false;
        let mut detach_attachment = false;
        let mut detach_all = false;
        // ProcessTriggerEvent6E53A0 walks the live chain, reading each
        // Trigger's next link (6E5484) after its Register/Spring. A live
        // Tag's chain changes only when the Tag is built and when the
        // pending-delete drain frees a Trigger, and no Spring reaches either,
        // so reading the chain at each position is that walk.
        let mut position = 0;
        while let Some(&trigger) = self
            .trigger_runtime
            .tag(index)
            .and_then(|tag| tag.triggers.get(position))
        {
            position += 1;
            if !self.register_live_trigger_event(trigger, input, repeat == 2, rules) {
                continue;
            }
            match repeat {
                0 => {
                    self.spring_live_trigger(
                        trigger,
                        input,
                        effects,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                    Self::mark_live_trigger_for_delete(
                        &mut self.substrate,
                        &mut self.trigger_runtime,
                        trigger,
                    );
                    result = true;
                    detach_attachment = true;
                    detach_all = true;
                }
                1 => {
                    if self
                        .trigger_runtime
                        .tag(index)
                        .expect("the walk read this Tag's chain just above")
                        .refs
                        == 1
                    {
                        self.spring_live_trigger(
                            trigger,
                            input,
                            effects,
                            rules,
                            overlay_registry,
                            frame_effects,
                        );
                        Self::mark_live_trigger_for_delete(
                            &mut self.substrate,
                            &mut self.trigger_runtime,
                            trigger,
                        );
                        result = true;
                        detach_all = true;
                    } else {
                        // TagClass::ProcessTriggerEvent, original gamemd.exe:
                        // refs1 takes 0x006E5463 to 0x006E5484, skipping the
                        // source-cleanup flag write at 0x006E547F.
                        detach_attachment = true;
                    }
                }
                2 => {
                    self.spring_live_trigger(
                        trigger,
                        input,
                        effects,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                    result = true;
                }
                _ => {}
            }
            // Action70 (6DF094 ->6E58B0) can immediately destroy this Tag,
            // including through a nested Force22. Native retains a stale
            // pointer here; safe storage must stop this dispatch rather than
            // reading the freed Tag or executing another linked Trigger.
            if self.trigger_runtime.tag(index).is_none() {
                return result;
            }
        }
        self.trigger_runtime.tags[index]
            .as_mut()
            .expect("the Tag was checked live just above")
            .dispatch_guard = false;
        let object = input.actor;
        let cell = input.cell;
        if detach_attachment {
            if let Some(object) = object
                && self
                    .trigger_runtime
                    .attachments
                    .get(&TagAttachment::Object(object))
                    == Some(&index)
            {
                self.trigger_runtime
                    .replace_trigger_tag_attachment(TagAttachment::Object(object), None);
            }
            if cell != (0, 0) {
                let identity = Self::trigger_cell_identity(&self.resolved_terrain, cell);
                self.trigger_runtime
                    .replace_cell_trigger_tag(identity, None);
            }
        }
        if detach_all {
            self.trigger_runtime.detach_live_tag(index);
            self.substrate.pending_delete.queue_tag(index);
        }
        result
    }

    /// The 50 Scenario global values, read as the campaign carry-over record
    /// copies their bytes (`0x004C6187..0x004C61A1`).
    pub fn global_variables(&self) -> [bool; SCENARIO_GLOBAL_COUNT] {
        std::array::from_fn(|index| self.trigger_runtime.globals_set.contains(&(index as u32)))
    }

    /// Controlled campaign capture input through SetGlobal689670, including
    /// its ordinary Tag timer resets. This does not replace the incoming
    /// carry-over record; Victory4C6180 must capture it normally.
    #[cfg(feature = "dev-overlays")]
    pub fn dev_set_scenario_global(&mut self, index: i32, set: bool) {
        Self::set_trigger_variable(
            &self.session,
            &mut self.scenario_rng,
            &mut self.trigger_runtime,
            true,
            index,
            set,
        );
    }

    /// Shared Scenario SetGlobal689670/SetLocal689910 mutation, including
    /// Tag timer resets. Trigger and Team scripts use this same owner.
    pub(crate) fn set_trigger_variable(
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        trigger_runtime: &mut TriggerRuntime,
        global: bool,
        index: i32,
        set: bool,
    ) -> bool {
        let count = if global { SCENARIO_GLOBAL_COUNT } else { 100 };
        if !(0..count as i32).contains(&index) {
            return false;
        }
        #[cfg(any(test, feature = "dev-overlays"))]
        Self::trace_event(
            trigger_runtime,
            session.binary_frame(),
            |seq, binary_frame, parent| TriggerTraceEvent::Variable {
                seq,
                binary_frame,
                parent,
                global,
                index,
                set,
            },
        );
        let values = if global {
            &mut trigger_runtime.globals_set
        } else {
            &mut trigger_runtime.locals_set
        };
        let previous = values.contains(&(index as u32));
        if set {
            values.insert(index as u32);
        } else {
            values.remove(&(index as u32));
        }
        if previous != set {
            trigger_runtime.dirty[0] = true;
            let events = if global { [27, 28] } else { [36, 37] };
            //689670/689910 -> Tag6E57F0/6E5820 ->7263A0/7263D0:
            // Tag registry order, then each reversed live chain, including
            // disabled instances. This differs from Enable53's global array.
            let mut tag_ordinal = 0;
            while tag_ordinal < trigger_runtime.tag_order.len() {
                let tag = trigger_runtime.tag_order[tag_ordinal];
                // Timer resets change no Tag chain: walk it in place.
                let mut position = 0;
                while let Some(&live) = trigger_runtime
                    .tag(tag)
                    .and_then(|tag| tag.triggers.get(position))
                {
                    position += 1;
                    let typ = trigger_runtime
                        .trigger(live)
                        .expect("a live Tag chains only live Triggers")
                        .type_index;
                    if trigger_runtime.definitions.trigger_types[typ]
                        .events
                        .iter()
                        .any(|e| events.contains(&e.condition.kind) && e.condition.value == index)
                    {
                        Self::reset_trigger_timers(session, scenario_rng, trigger_runtime, live);
                    }
                }
                tag_ordinal += 1;
            }
        }
        previous
    }

    /// Logic55AFB0 reads the mutable list length and dirty bytes afresh for
    /// every Tag. A removed current Tag shifts the next entry, which is skipped
    /// until the next visit; an enabled later Tag can fire during this pass.
    /// `rules` are the frame's bound match rules, which action callbacks share
    /// with ordinary Techno runtime calculations.
    pub fn advance_triggers(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Vec<TriggerEffect> {
        let mut effects = Vec::new();
        #[cfg(any(test, feature = "dev-overlays"))]
        Self::trace_event(
            &mut self.trigger_runtime,
            self.session.binary_frame(),
            |seq, binary_frame, _| TriggerTraceEvent::Poll {
                seq,
                binary_frame,
                end: false,
            },
        );
        self.trigger_runtime.logic_cursor = 0;
        while let Some(&tag) = self
            .trigger_runtime
            .logic_tags
            .get(self.trigger_runtime.logic_cursor as usize)
        {
            let mut fired = false;
            if self.trigger_runtime.dirty[3] {
                fired = self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 50,
                        ..Default::default()
                    },
                    &mut effects,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            if !fired && self.trigger_runtime.dirty[0] {
                for event in [27, 28, 36, 37] {
                    if self.dispatch_live_tag_event(
                        tag,
                        TagEventInput {
                            kind: event,
                            ..Default::default()
                        },
                        &mut effects,
                        rules,
                        overlay_registry,
                        frame_effects,
                    ) {
                        fired = true;
                        break;
                    }
                }
            }
            if !fired && self.trigger_runtime.dirty[2] {
                for event in [45, 46] {
                    if self.dispatch_live_tag_event(
                        tag,
                        TagEventInput {
                            kind: event,
                            ..Default::default()
                        },
                        &mut effects,
                        rules,
                        overlay_registry,
                        frame_effects,
                    ) {
                        fired = true;
                        break;
                    }
                }
            }
            if !fired {
                fired = self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 13,
                        ..Default::default()
                    },
                    &mut effects,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            if !fired {
                fired = self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 51,
                        ..Default::default()
                    },
                    &mut effects,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            // Logic55B11E..55B159 checks the current timer only after both
            // ordinary timer events fail. Earlier actions may have restarted it.
            if !fired && self.session.scenario_timer_elapsed() {
                self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 14,
                        ..Default::default()
                    },
                    &mut effects,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            self.trigger_runtime.logic_cursor = self.trigger_runtime.logic_cursor.wrapping_add(1);
        }
        self.session.finish_scenario_timer_poll();
        self.trigger_runtime.dirty = [false; 4];
        #[cfg(any(test, feature = "dev-overlays"))]
        Self::trace_event(
            &mut self.trigger_runtime,
            self.session.binary_frame(),
            |seq, binary_frame, _| TriggerTraceEvent::Poll {
                seq,
                binary_frame,
                end: true,
            },
        );
        effects
    }
}
