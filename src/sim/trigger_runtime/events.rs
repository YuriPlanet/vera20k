//! Trigger event evaluation: HasOccurred71E940's predicates, their House
//! lookups, and EventRegister7264C0's per-Trigger event registration.

use super::{EVENT_ANY, TagEventInput, resolve_trigger_house};
use crate::sim::world::Simulation;

impl Simulation {
    /// HasOccurred's House lookup of Event.value (Country502D30 at 71ED01,
    /// 71ED33, 71F06C, 71F1C9 and 71F1ED). Ares 3.0 replaces each with
    /// TEventExt::ResolveHouseParam (PlayerAtX1 10050430, PlayerAtX2
    /// 10050460): 8997 names no House here, a Player@X selector its start
    /// slot's House (HouseClass::FindByIndex), any other value a Country.
    pub(super) fn event_value_house(
        &self,
        value: i32,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<crate::sim::intern::InternedId> {
        if rules.ares_form() {
            (value != 0x2325)
                .then(|| self.house_by_trigger_selector(value, rules))
                .flatten()
        } else {
            u16::try_from(value).ok().and_then(|country| {
                self.house_by_country_index(crate::rules::ruleset::CountryIdx(country), rules)
            })
        }
    }

    /// The shared lookup at 71F069 continues with a null House in stock
    /// gamemd. Ares PlayerAtX1 (10050430, hook 0x71F06C) instead returns
    /// false (71F0D5) when Event.value is a Player@X selector without a House,
    /// so events do not fire for players who are not in the game.
    fn event_house_lookup_rejects(
        &self,
        value: i32,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        rules.ares_form()
            && crate::rules::native_processing::HOUSE_TYPE_PLAYER_AT_SELECTORS.contains(&value)
            && self.house_by_trigger_selector(value, rules).is_none()
    }

    /// Selected HasOccurred71E940 predicates. Numeric/timer controls execute
    /// the retail body; shared +54 and mutable persistence belong to this owner.
    pub(super) fn has_occurred(
        &mut self,
        ordinal: usize,
        input: TagEventInput,
        incoming_house: Option<crate::sim::intern::InternedId>,
        persistence: &mut bool,
        rules: &crate::rules::ruleset::RuleSet,
        live_index: usize,
    ) -> bool {
        crate::sim::ra2_all02_diagnostic::on_trigger_evaluation(self, rules);
        let type_index = self
            .trigger_runtime
            .trigger(live_index)
            .expect("callers pass a live Trigger")
            .type_index;
        // The definition is read in place: every predicate below reads
        // through `&self`, and the one write (the entered-House arm's
        // retained Event+54) follows its last read of the condition.
        let condition =
            &self.trigger_runtime.definitions.trigger_types[type_index].events[ordinal].condition;
        // Ares 3.0 TEventExt::HasOccured (1004F7A0, hook 0x71E949) answers
        // its own kinds before the stock switch.
        let ares = rules.ares_form();
        if ares
            && let Some(occurred) = self.ares_has_occurred(condition, input, incoming_house, rules)
        {
            return occurred;
        }
        match condition.kind {
            // TEventClass::HasOccurred, original gamemd.exe:
            // 0x0071E950/0x0071EC83 bypass the raised-kind/editor gates for
            // negative or >=62 kinds; 0x0071F084 reaches AL1 at0x0071F1B4.
            // No persistence or retained-House write. Keep Ares' replacement
            // dispatcher and the known0..61 predicates separate; when Ares
            // plays only kinds outside 0..=88 get here.
            kind if !(0..=61).contains(&kind) => true,
            53 => {
                // HasOccurred71ECF5..71ED56, original gamemd: raised53
                // and clear A8ED6B admit a forced GetDisplayOwner(+D0,1).
                // Compare the first Country502D30 House identity, not its
                // Country or actual owner. Only persistence is written.
                // Source: tag_lifecycle.py --spy-entry-events.
                if input.kind != 53 || self.buildup_bypass_active() {
                    return false;
                }
                let Some(actor) = input.actor.and_then(|id| self.substrate.entities.get(id)) else {
                    return false;
                };
                // VERA network adaptation: an undisguised agent falls back
                // to PlayerPtr, which differs per client and now feeds the
                // match checksum, so a network game compares the worn
                // identity alone.
                let viewer = (!self.session.execution_topology().is_lockstep())
                    .then(|| self.session.current_house())
                    .flatten();
                let displayed = actor.disguise_house(true, viewer, false);
                let selected = self.event_value_house(condition.value, rules);
                let matched = displayed.is_some() && displayed == selected;
                if matched {
                    *persistence = true;
                }
                matched
            }
            54 => {
                // HasOccurred71ED59..71EDB8: raised54/non-NULL actor,
                // forced GetDisguise(1), RTTI InfantryType16, valid+34 and
                // native array index. Match writes persistence1. Primary
                // table71F284[53]=0 selects71EC9A's A8ED6B gate first.
                let (54, Some(actor)) = (input.kind, input.actor) else {
                    return false;
                };
                if self.buildup_bypass_active() {
                    return false;
                }
                let Some(kind) = self.forced_disguise_type(actor, rules) else {
                    return false;
                };
                let matched = condition.value != -1
                    && rules
                        .object_in_category(
                            crate::rules::object_type::ObjectCategory::Infantry,
                            self.interner.resolve(kind),
                        )
                        .is_some()
                    && self.interner.type_array_index(
                        kind,
                        crate::rules::object_type::ObjectCategory::Infantry,
                        rules,
                    ) == Some(condition.value);
                if matched {
                    *persistence = true;
                }
                matched
            }
            23 => {
                //71EC9A gate, then71EEA9 scans all Teams for the retained
                //Type pointer, null head and nonzero leaving byte. A match
                //sets persistence before the generic true tail; a null
                //incoming House skips that scan without latching.
                if input.kind != 23 || self.buildup_bypass_active() {
                    return false;
                }
                if incoming_house.is_none() {
                    return !self.event_house_lookup_rejects(condition.value, rules);
                }
                let team_type = match condition.team_name.as_deref() {
                    Some(name) => {
                        let Some(id) = self.interner.get(name) else {
                            return false;
                        };
                        Some(id)
                    }
                    None => None,
                };
                if self.team_script_vm.has_empty_leaving_team(team_type) {
                    *persistence = true;
                    true
                } else {
                    false
                }
            }
            34 => {
                //71EDBB..71EE54: 68BD00 waypoint Location versus actor
                //GetCoords, signed wrapping deltas, sqrtApprox/ftol <=1280.
                //No persistence, Event+54, timer, RNG or detach write.
                if input.kind != 34 || self.buildup_bypass_active() {
                    return false;
                }
                let Some(waypoint) = self.trigger_waypoint_location(condition.value) else {
                    return false;
                };
                let Some(actor) = input.actor.and_then(|id| self.substrate.entities.get(id)) else {
                    return false;
                };
                let point = crate::sim::movement::ground_pose::object_get_coords(
                    actor,
                    self.resolved_terrain.as_ref(),
                );
                crate::util::native_x87::distance_3d_leptons([point.x, point.y, point.z], waypoint)
                    <= 0x500
            }
            14 => self.session.scenario_timer_elapsed(),
            // HasOccurred45@71EAE7 /46@71EB06 read Scenario+352C,
            // not normal/target ambient: signed inclusive comparisons with
            // Event+34. Both return directly, without House/raised-kind or
            // A8ED6B admission, persistence, timer, RNG or detach writes.
            // Original Steam gamemd, instruction reading.
            45 => self.session.lighting().current_ambient() <= condition.value,
            46 => self.session.lighting().current_ambient() >= condition.value,
            17 => {
                // Steam gamemd:71EE82 takes the incoming House;
                // 71EF3F..71EF89 reads tracked+2F0, then its ordered+68 list.
                // Only null/limbo entries are skipped: neither Health nor
                // Alive gates this leaf. Factory is the raw nonzero RTTI,
                // independent of Event.value, actor, raised kind and init.
                // No persistence, Event+54, timer, RNG or detach write.
                // Executed controls: tag_lifecycle --no-factories-events.
                let Some(owner) = incoming_house else {
                    return !self.event_house_lookup_rejects(condition.value, rules);
                };
                let Some(house) = self.houses.get(&owner) else {
                    return false;
                };
                if house.tracking().buildings() <= 0 {
                    return true;
                }
                !house.base_projection().buildings().iter().any(|&id| {
                    self.substrate.entities.get(id).is_some_and(|building| {
                        !building.lifecycle.in_limbo()
                            && self
                                .object_type(building.type_ref(), rules)
                                .is_some_and(|ty| ty.has_factory())
                    })
                })
            }
            18 => {
                // HasOccurred71E940, arm71EF8E: null incoming House is
                // true, otherwise read its retained CiviliansEvacuated+1F9.
                // No raised-kind/value gate, persistence or timer write.
                // Source: original gamemd.exe instructions.
                incoming_house.is_none_or(|owner| {
                    self.houses
                        .get(&owner)
                        .is_some_and(|house| house.civilians_evacuated())
                })
            }
            19..=22 => {
                // Steam gamemd: HasOccurred71EFEB..71F03D reads incoming
                // House+26C/+274/+270/+278, not a Country or built count.
                // Match sets persistence; mismatch leaves it alone. The null
                // House takes the generic true tail without that write.
                // Original execution: tag_lifecycle --build-type-events.
                let Some(owner) = incoming_house else {
                    return !self.event_house_lookup_rejects(condition.value, rules);
                };
                use crate::rules::object_type::ObjectCategory;
                let category = match condition.kind {
                    19 => ObjectCategory::Building,
                    20 => ObjectCategory::Vehicle,
                    21 => ObjectCategory::Infantry,
                    22 => ObjectCategory::Aircraft,
                    _ => unreachable!(),
                };
                let matched = self
                    .houses
                    .get(&owner)
                    .is_some_and(|house| house.last_built_type_index(category) == condition.value);
                if matched {
                    *persistence = true;
                }
                matched
            }
            12 | 52 => {
                // Steam gamemd: HasOccurred71EF01/71EF20 calls
                // IHouse::AvailableMoney4F6990 on Register7264C0's incoming
                // House, then compares the literal signed threshold inclusively.
                // No raised-kind gate, persistence, retained-House or timer write.
                // Executed: tag_lifecycle --funds-events. The shared Economy
                // getter retains its documented empty-silo coverage boundary.
                let Some(owner) = incoming_house else {
                    return !self.event_house_lookup_rejects(condition.value, rules);
                };
                let funds = crate::sim::credit_income::available_money(&self.houses, owner);
                if condition.kind == 12 {
                    funds >= condition.value
                } else {
                    funds <= condition.value
                }
            }
            1 | 24 | 25 | 26 | 59 => {
                //71ECAF ->71F1BD compares actor6F9DB0's House ArrayIndex
                //with Country502D30's first matching House+30, then retains
                //the actor's House via6F9DC0. Rust House identities represent
                //unique registered slots. Wildcard-1 skips the comparison.
                //The duplicate-Country distinction is executed by the
                //--entered-cell-events controls, not a Country-value match.
                if input.kind != condition.kind || self.buildup_bypass_active() {
                    return false;
                }
                let Some(owner) = input
                    .actor
                    .and_then(|actor| self.substrate.entities.get(actor))
                    .map(|actor| actor.owner())
                else {
                    return false;
                };
                if !self.houses.contains_key(&owner) {
                    return false;
                }
                let selected_house = self.event_value_house(condition.value, rules);
                if condition.value != -1 && selected_house != Some(owner) {
                    return false;
                }
                self.trigger_runtime.definitions.trigger_types[type_index].events[ordinal]
                    .retained_house = Some(owner);
                *persistence = true;
                true
            }
            32 | 57 => {
                //71EFA8/71EFCB ->49FAE0 with House+5550 (active, not owned).
                //The null incoming House reaches the original generic true
                //tail without writing persistence.
                let Some(owner) = incoming_house else {
                    let fallback = !self.event_house_lookup_rejects(condition.value, rules);
                    crate::sim::ra2_all02_diagnostic::on_building_count_event(
                        self,
                        rules,
                        None,
                        condition.kind,
                        condition.value,
                        -1,
                        fallback,
                    );
                    return fallback;
                };
                let Some(type_name) = usize::try_from(condition.value).ok().and_then(|index| {
                    rules
                        .type_array_ids(crate::rules::object_type::ObjectCategory::Building)
                        .get(index)
                }) else {
                    return false;
                };
                let count = self.houses.get(&owner).map_or(0, |house| {
                    self.interner.get(type_name).map_or(0, |id| {
                        house
                            .tracking()
                            .active_count(crate::map::entities::EntityCategory::Structure, id)
                    })
                });
                let result = (count != 0) == (condition.kind == 32);
                crate::sim::ra2_all02_diagnostic::on_building_count_event(
                    self,
                    rules,
                    Some(owner),
                    condition.kind,
                    condition.value,
                    count,
                    result,
                );
                if result {
                    *persistence = true;
                }
                result
            }
            15 | 16 => {
                // HasOccurred71EE82 takes the Trigger's incoming House,
                // not Event.value's Country. 71F03F/71F058 compare signed
                // House+5488/+5434 loss counters with Event+34 using >=.
                // The null-House tail is true; neither arm latches, writes
                // the timer/retained House, draws RNG or detaches anything.
                // Steam gamemd; tag_lifecycle --loss-threshold-events.
                let Some(owner) = incoming_house else {
                    return !self.event_house_lookup_rejects(condition.value, rules);
                };
                let Some(house) = self.houses.get(&owner) else {
                    return false;
                };
                let losses = if condition.kind == 15 {
                    house.stats().buildings_lost()
                } else {
                    house.stats().units_lost()
                };
                (losses as i32) >= condition.value
            }
            3 | 5 | 9 | 10 | 11 | 55 | 56 => {
                // Steam gamemd,71EC9A/71F0E1/71F0FB:3 requires
                // its own raised kind and clear A8ED6B before reading244;
                // 5 skips that gate and reads1F4. Both use the existing
                // House notifications. In particular, do not invent a3
                // raise from Engineer capture or translate a House event8.
                if condition.kind == 3 && (input.kind != 3 || self.buildup_bypass_active()) {
                    return false;
                }
                //71F069 ->502D30: Event.value selects a Country's first
                //registered House independently of the incoming House. After
                //the above gate, no matching House reaches the AL1 tail; this
                //does not write persistence or the Type-owned Event+54.
                //The private HouseTracking owner preserves separate signed
                //Unit/Infantry totals and the tracked Building scalar.
                //10's71F115..71F12E tests tracked buildings <=0, including
                //constructor-complete Limbo until actual destruction. Steam
                //gamemd; tag_lifecycle --building-loss-events.
                //55/56: Steam gamemd, 71F159 /71F16F..71F187;
                //tools/spatial_oracle/tag_lifecycle.py --naval-land-events.
                //56's TEST after SUB observes wrapping subtraction, not a
                //signed comparison of the two source counts. These events
                //read tracked scalars, unlike9's on-map CounterClass totals.
                if self.event_house_lookup_rejects(condition.value, rules) {
                    return false;
                }
                let owner = self.event_value_house(condition.value, rules);
                owner.is_none_or(|owner| {
                    self.houses
                        .get(&owner)
                        .is_none_or(|house| match condition.kind {
                            3 => house.building_capture_notified(),
                            5 => house.discovered_by_current_house(),
                            9 => house.tracking().units_destroyed(),
                            10 => house.tracking().buildings() <= 0,
                            11 => house.tracking().all_destroyed(),
                            55 => house.tracking().navy() <= 0,
                            56 => {
                                house
                                    .tracking()
                                    .vehicles()
                                    .wrapping_sub(house.tracking().navy())
                                    <= 0
                                    && house.tracking().infantry() <= 0
                            }
                            _ => unreachable!(),
                        })
                })
            }
            30 | 58 => {
                //71F099/71F0BD select Event.value's Country, then the retained
                //House power ratio. Neither branch writes persistence.
                if self.event_house_lookup_rejects(condition.value, rules) {
                    return false;
                }
                let owner = self.event_value_house(condition.value, rules);
                owner.is_none_or(|owner| {
                    self.power_states
                        .get(&owner)
                        .is_none_or(crate::sim::power_system::PowerState::has_full_power)
                        == (condition.kind == 58)
                })
            }
            44 => {
                //Object5F57FA supplies source Techno;71EE57/71EE70 reads
                //that live object's +21C -> House+30 at predicate time.
                //The --damage-source-events full-body controls establish the
                //shared A8ED6B gate71EC9A..71ECA9 before this source arm;
                //neither timer nor Event+54 is written.
                if input.kind != 44 || self.buildup_bypass_active() {
                    return false;
                }
                // Ares TEventClass_HasOccured_PlayerAtX3 (10050480, hook
                // 0x71EE79) first turns a Player@X value into that House's
                // index, -1 for an empty slot.
                let expected = if ares
                    && crate::rules::native_processing::HOUSE_TYPE_PLAYER_AT_SELECTORS
                        .contains(&condition.value)
                {
                    self.house_by_trigger_selector(condition.value, rules)
                        .and_then(|house| self.base_reservation_house_index(house))
                        .unwrap_or(-1)
                } else {
                    condition.value
                };
                input
                    .source
                    .and_then(|source| self.substrate.entities.get(source))
                    .and_then(|source| self.base_reservation_house_index(source.owner()))
                    .is_some_and(|index| index == expected)
            }
            13 | 51 => self
                .trigger_runtime
                .trigger(live_index)
                .expect("callers pass a live Trigger")
                .timer
                .expired(self.session.binary_frame() as i32),
            //71EC83/71F284 routes these discrete events through71EC9A:
            //matching raised kind and clear A8ED6B, then71F1B1 AL1.
            //All secondary switch entries are the generic true tail; no
            //House-retention or persistence write. Original Steam gamemd;
            //RecordKill702D40 publishes4/6/7/48/29 here.
            //Object5F55EA..5F573F raises38..43 through this same gate. The
            //original bodies ignore value/incoming House/actor/source here;
            //tag_lifecycle.py --damage-events / trigger_damage_events.json
            //execute30 controls, including prior persistence and NULL House.
            //49/50 use the same gate. Steam gamemd,71EC9A/71F1B1;
            //trigger_action_values --crate records their original predicates.
            //31: bridge575EE0 raises it on each tagged span cell. After this
            //gate,71EE91 routes it to71F069, whose Country502D30 lookup
            //selects only the71F1B1 true tail either way.
            //All of them then reach71F069's lookup of Event.value, which only
            //an Ares Player@X selector without a House turns false.
            35 => {
                input.kind == 35
                    && !self.buildup_bypass_active()
                    && !self.event_house_lookup_rejects(condition.value, rules)
            }
            2 | 4 | 6 | 7 | 29 | 31 | 33 | 38 | 39 | 40 | 41 | 42 | 43 | 48 | 49 | 50 => {
                input.kind == condition.kind
                    && !self.buildup_bypass_active()
                    && !self.event_house_lookup_rejects(condition.value, rules)
            }
            //71F28B selects the generic true tail71F1B4, no persistence write.
            EVENT_ANY => true,
            _ => self.trigger_runtime.evaluate_event(
                condition,
                self.session.binary_frame(),
                self,
                rules,
            ),
        }
    }

    pub(super) fn register_live_trigger_event(
        &mut self,
        index: usize,
        input: TagEventInput,
        mut persistence: bool,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        //7264C0 admits enabled/nonpending instances. Force bypasses predicate
        //and House forwarding; its caller-seeded persistence still resets.
        let Some(trigger) = self.trigger_runtime.trigger(index) else {
            return false;
        };
        if !trigger.enabled || trigger.pending {
            return false;
        }
        let type_index = trigger.type_index;
        let incoming_house = resolve_trigger_house(
            self,
            self.trigger_runtime.definitions.trigger_types[type_index]
                .owner
                .as_deref(),
            rules,
        );
        let mut result = true;
        if !input.force {
            let count = self.trigger_runtime.definitions.trigger_types[type_index]
                .events
                .len();
            for ordinal in 0..count {
                let bit = 1u32.wrapping_shl(ordinal as u32);
                let completed = self
                    .trigger_runtime
                    .trigger(index)
                    .expect("the Trigger was read live at the top of this check")
                    .completed
                    & bit
                    != 0;
                if !completed {
                    #[cfg(any(test, feature = "dev-overlays"))]
                    let before = persistence;
                    let occurred = self.has_occurred(
                        ordinal,
                        input,
                        incoming_house,
                        &mut persistence,
                        rules,
                        index,
                    );
                    #[cfg(any(test, feature = "dev-overlays"))]
                    self.trace_predicate(index, ordinal, occurred, persistence && !before);
                    if !occurred {
                        result = false;
                        continue;
                    }
                    let kind = self.trigger_runtime.definitions.trigger_types[type_index].events
                        [ordinal]
                        .condition
                        .kind;
                    //726564..726582 uses the current mutable flag, then BOTH
                    //native71F950/71F9C0 classifications. Entered/count/power
                    //do not latch despite writes to persistence.
                    if persistence
                        && matches!(
                            kind,
                            2 | 3
                                | 4
                                | 7
                                | 18
                                | 19
                                | 20
                                | 21
                                | 22
                                | 23
                                | 24
                                | 25
                                | 26
                                | 29
                                | 33
                                | 34
                                | 35
                                | 38
                                | 39
                                | 40
                                | 41
                                | 42
                                | 43
                                | 48
                                | 49
                                | 50
                                | 53
                                | 54
                        )
                    {
                        self.trigger_runtime.triggers[index]
                            .as_mut()
                            .expect("the Trigger was read live at the top of this check")
                            .completed |= bit;
                    }
                }
                //726584 forwards shared TEvent+54 even for an already latched
                //node. Null never clears the Live+2C retained House.
                if let Some(house) = self.trigger_runtime.definitions.trigger_types[type_index]
                    .events[ordinal]
                    .retained_house
                {
                    self.trigger_runtime.triggers[index]
                        .as_mut()
                        .expect("the Trigger was read live at the top of this check")
                        .retained_house = Some(house);
                }
            }
        }
        if result && persistence {
            Self::reset_trigger_timers(
                &self.session,
                &mut self.scenario_rng,
                &mut self.trigger_runtime,
                index,
            );
        }
        result
    }
}
