//! Trigger action execution: TAction's House selection and helpers, the
//! talk bubble, and `execute_live_trigger_action`, the per-kind action body.

use super::{
    ACTION_ANNOUNCE_LOSE, ACTION_ANNOUNCE_WIN, ACTION_CENTER_CAMERA,
    ACTION_CHANGE_VISIBLE_MAP_AREA, ACTION_CLEAR_ALTERNATE_BASE, ACTION_CLEAR_GLOBAL,
    ACTION_CLEAR_LOCAL, ACTION_DAMAGE_AT_WAYPOINT, ACTION_DISABLE_TRIGGER, ACTION_ENABLE_TRIGGER,
    ACTION_END_SCENARIO, ACTION_FORCE_TRIGGER, ACTION_JUMP_CAMERA, ACTION_PLAY_SOUND,
    ACTION_PLAY_SOUND_AT_WAYPOINT, ACTION_SET_ALTERNATE_BASE, ACTION_SET_GLOBAL, ACTION_SET_LOCAL,
    ACTION_SET_SUPER_CHARGE_PERCENTAGE, DefinitionReference, RuntimeAction, TagAttachment,
    TagEventInput, ThemeRequest, TriggerActionContext, TriggerEffect, parse_visible_map_area,
};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ini_value::crt_atoi;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use std::collections::BTreeSet;

impl Simulation {
    /// `TalkBubble::SetObjectAndFrame @ 0x0070F120`, called by trigger
    /// action 105 and team script action 52. A null object or frame 0 clears
    /// the bubble (`0x0070F194..0x0070F1CC`); otherwise it shows `frame` over
    /// `actor` for `TalkBubbleTime` (Rules `+0x1800`).
    pub(crate) fn talk_bubble_set_object_and_frame(
        &mut self,
        actor: Option<u64>,
        frame: i32,
        context: crate::sim::world::UninitContext<'_>,
    ) {
        let actor = actor.filter(|_| frame != 0);
        let duration = if actor.is_some() {
            context.rules().general.talk_bubble_time
        } else {
            0
        };
        //70F134/13A/173 commits the request BEFORE Map callbacks. A nested
        //105 from discovery must win.
        self.publish_trigger_effects(vec![TriggerEffect::TalkBubble {
            actor,
            value: if actor.is_some() { frame } else { 0 },
            duration,
        }]);
        if let Some(actor) = actor {
            //70F159..70F189: flags0/0/0/1/1, current player (not the Team
            //owner), literal radius2, including negative values.
            self.reveal_talk_bubble_actor(actor, context);
        }
    }
}

impl Simulation {
    /// Shared country selection in6E0AA0/6E0B60/6E3120/6E3180..6E32A0.
    ///0x2325 (8997) invokes original726910's retained live+2C getter, not Spring's
    ///incoming authored House. The special/Country lookup has one world owner.
    /// Executed controls: trigger_all_change_house.json (Steam gamemd).
    pub(super) fn live_action_house(
        &self,
        live: usize,
        value: i32,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<crate::sim::intern::InternedId> {
        if value == 0x2325 {
            self.trigger_runtime.trigger(live)?.retained_house
        } else if value == -1 {
            None
        } else {
            self.house_by_trigger_selector(value, rules)
        }
    }

    /// Action36/6E0B60..6E0C91, Steam gamemd. Two forward mutable
    /// Techno-array passes put Powered/PoweredSpecial Buildings last, then
    /// run their original4549B0 dispatcher even for a same-owner callback.
    /// No own RNG, timer or detach; ChangeOwner and operational consumers
    /// retain their lifecycle/side effects. No extra Alive/limbo filter here.
    fn trigger_change_all_house_objects(
        &mut self,
        owner: crate::sim::intern::InternedId,
        target: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let mut found = false;
        for powered_pass in [false, true] {
            let mut ordinal = 0;
            while let Some(id) = self.substrate.entities.techno_registry_at(ordinal) {
                let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                    let powered = entity.category()
                        == crate::map::entities::EntityCategory::Structure
                        && self
                            .interner
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|object| object.powered || object.powered_special);
                    entity.owner() == owner && powered == powered_pass
                });
                if eligible {
                    self.change_owner_with_rules(id, target, rules, registry, effects);
                    if powered_pass {
                        self.dispatch_building_operational(id, rules, registry, effects);
                    }
                    found = true;
                }
                ordinal += 1;
            }
        }
        found
    }

    ///Tag6E5380: identity membership of a live Trigger in the Tag's chain.
    pub(super) fn tag_contains_live_trigger(&self, attachment: TagAttachment, live: usize) -> bool {
        self.trigger_runtime
            .attachments
            .get(&attachment)
            .and_then(|&tag| self.trigger_runtime.tag(tag))
            .is_some_and(|tag| tag.triggers.contains(&live))
    }

    ///TActionCreateBuilding6E4200. Cell486840 runs BEFORE Type+8C45E880;
    ///130's corresponding lookup is after construction. Both callers use
    ///the unique458A80 foundation-clearing placement and ordinary lifecycle.
    fn trigger_create_building_at_waypoint(
        &mut self,
        owner: crate::sim::intern::InternedId,
        waypoint: i32,
        type_name: &str,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some(at) = self.session.waypoint_cell(waypoint) else {
            return false;
        };
        let coord = self.resolved_terrain.as_ref().and_then(|terrain| {
            let cells = NativeCellQuery::canonical(terrain);
            crate::sim::cell_kernel::native_cell_own_coords(cells.lookup(at), &cells).map(
                |(x, y, z)| crate::sim::components::DriveCoord {
                    x: x as i32,
                    y: y as i32,
                    z: z as i32,
                },
            )
        });
        let Some(coord) = coord else {
            return false;
        };
        let Some(object) = rules.object(type_name).filter(|object| {
            object.category == crate::rules::object_type::ObjectCategory::Building
        }) else {
            return false;
        };
        let Some(created) = self.construct_object_limbo_at_height(
            object.id.as_str(),
            owner,
            0,
            0,
            0,
            0,
            rules,
            frame_effects,
        ) else {
            return false;
        };
        if self
            .place_building_with_cleared_foundation(
                created,
                coord,
                0,
                rules,
                overlay_registry,
                frame_effects,
            )
            .is_none()
        {
            //6E42A5 repeats F8 even when458A80 has already rejected/UnInited.
            self.uninit_with_context(
                created,
                crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
            );
            return false;
        }
        self.grand_opening(
            created,
            false,
            false,
            rules,
            overlay_registry,
            frame_effects,
        );
        true
    }

    pub(super) fn apply_live_trigger_action(
        &mut self,
        action: &RuntimeAction,
        context: TriggerActionContext,
        effects: &mut Vec<TriggerEffect>,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        if self.trigger_runtime.stop_before_action == Some(action.entry.kind) {
            std::panic::panic_any(action.entry.kind);
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trigger_runtime
            .action_trace
            .push((action.entry.kind, action.value.unwrap_or(0)));
        // Force22 must stay outside Execute's large nonrecursive action frame.
        // Even a finite authored chain otherwise multiplies that frame until a
        // client with a smaller thread stack aborts before the depth guard.
        // Execute6DF0E7..6DF134 (original instructions) iterates the live registry in order and ignores Spring's
        // result. Re-read its live length after every synchronous callback.
        // 6DF112..6DF119 passes Spring7265C0 a NULL object and the empty
        // Cell [B0E680], never this Spring's own object or Cell.
        if action.entry.kind == ACTION_FORCE_TRIGGER {
            if let Some(DefinitionReference::Trigger(target)) = action.reference {
                let mut ordinal = 0;
                while ordinal < self.trigger_runtime.trigger_order.len() {
                    let live = self.trigger_runtime.trigger_order[ordinal];
                    if self
                        .trigger_runtime
                        .trigger(live)
                        .is_some_and(|trigger| trigger.type_index == target)
                    {
                        self.spring_live_trigger(
                            live,
                            TagEventInput::default(),
                            effects,
                            rules,
                            overlay_registry,
                            frame_effects,
                        );
                    }
                    ordinal += 1;
                }
            }
            return true;
        }
        self.execute_live_trigger_action(
            action,
            context,
            effects,
            rules,
            overlay_registry,
            frame_effects,
        )
    }

    // Keep this frame out of the Force22 recursion path, also in optimized
    // builds. The common dispatcher above owns tracing and forced dispatch.
    #[inline(never)]
    fn execute_live_trigger_action(
        &mut self,
        action: &RuntimeAction,
        context: TriggerActionContext,
        effects: &mut Vec<TriggerEffect>,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        // Ares 3.0 TActionExt::Execute (100396C0, hook 0x6DD8D7) answers its
        // own kinds before the stock switch.
        let ares = rules.ares_form();
        if ares
            && let Some(handled) = self.ares_execute_action(
                action,
                context,
                effects,
                rules,
                overlay_registry,
                frame_effects,
            )
        {
            return handled;
        }
        let reference = action.reference;
        let value = action.value;
        let sound = action.sound.as_ref();
        let action = &action.entry;
        match action.kind {
            6 => {
                // Steam gamemd, Execute6DEBF9..6DEC3B:
                // shared6E45E0 selects the House, then IHouse vt+38 is
                // AllToHunt501400. A NULL Trigger/House is rejected.
                // Native evidence: trigger_action_values --all-to-hunt.
                let (Some(live), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let Some(owner) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                crate::sim::house_strategy::all_to_hunt(
                    self,
                    rules,
                    owner,
                    overlay_registry,
                    frame_effects,
                );
            }
            74 | 75 => {
                // Steam gamemd:6DF2D1/6DF310 -> existing6E45E0 selector,
                // then only House+1F2=1/0 at6DF2FA/6DF339. No immediate
                // timer, Team, sibling-latch or RNG change belongs here.
                // Native actions/consumer streams: --ai-activation corpus.
                let (Some(live), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let Some(house) = self
                    .live_action_house(live, value, rules)
                    .and_then(|owner| self.houses.get_mut(&owner))
                else {
                    return false;
                };
                house.set_ai_triggers_active(action.kind == 74);
            }
            9 => {
                // Execute6DEAD6 -> shared House selector6E45E0;6DEAFF
                // writes only House+250=4. No immediate FireSale/Hunt and
                // no strategy timer reset. Those remain with house_strategy.
                // The shared6E45E0 selector rejects a null Trigger.
                // Steam gamemd, --no-factories-events Action9 controls.
                let (Some(live), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let Some(house) = self
                    .live_action_house(live, value, rules)
                    .and_then(|owner| self.houses.get_mut(&owner))
                else {
                    return false;
                };
                house.strategy_emergency_mut().set_state_four();
            }
            12 => {
                // Steam gamemd, Execute6DF051..6DF08F: reverse
                // live Trigger array, then726720's deferred marker. No
                // immediate detach, RNG or timer write belongs to this action.
                if let Some(DefinitionReference::Trigger(target)) = reference {
                    for ordinal in (0..self.trigger_runtime.trigger_order.len()).rev() {
                        let live = self.trigger_runtime.trigger_order[ordinal];
                        if self
                            .trigger_runtime
                            .trigger(live)
                            .is_some_and(|trigger| trigger.type_index == target)
                        {
                            Self::mark_live_trigger_for_delete(
                                &mut self.substrate,
                                &mut self.trigger_runtime,
                                live,
                            );
                        }
                    }
                }
            }
            70 => {
                // Execute6DF094 -> Tag vt20=6E58B0 ->6E4F60 destroys each
                // matching Tag immediately; linked Triggers remain deferred.
                // Revisit the shifted entry after every destructor. Native
                // captures the old count and can read stale slots past its
                // contracted array; safe storage stops at the live end.
                if let Some(DefinitionReference::Tag(target)) = reference {
                    let mut ordinal = 0;
                    while ordinal < self.trigger_runtime.tag_order.len() {
                        let live = self.trigger_runtime.tag_order[ordinal];
                        if self
                            .trigger_runtime
                            .tag(live)
                            .is_some_and(|tag| tag.type_index == target)
                        {
                            self.delete_live_tag(live, rules, 1, self.session.started());
                        } else {
                            ordinal += 1;
                        }
                    }
                }
            }
            38 => {
                // Execute6DE189..6DE1C8 resolves6E45E0 once, then calls
                // House4F9F90(source,target,0), followed by the reverse call.
                // The diplomacy owner retains anger, fog and threat updates.
                let (Some(live), Some(source), Some(value)) =
                    (context.live_index, context.incoming_house, value)
                else {
                    return false;
                };
                let Some(target) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                self.make_enemy(source, target, rules, frame_effects, false);
                self.make_enemy(target, source, rules, frame_effects, false);
            }
            23..=27 => {
                use crate::sim::scenario_session::ScenarioTimerAction;
                let seconds = value.unwrap_or(0);
                self.session.change_scenario_timer(match action.kind {
                    23 => ScenarioTimerAction::Start,
                    24 => ScenarioTimerAction::Stop,
                    25 => ScenarioTimerAction::Extend(seconds),
                    26 => ScenarioTimerAction::Shorten(seconds),
                    27 => ScenarioTimerAction::Set(seconds),
                    _ => unreachable!(),
                });
            }
            103 => self.session.set_scenario_timer_caption(action.csf_label()),
            20 => {
                // Steam gamemd: Execute6DE8E4..6DE903 passes +90
                // to Theme720B20 and returns AL1, even when audio is gated.
                // Offline type8 reads retain From_Name's index. Network
                // playback keeps the authored name independent of the local
                // playlist. Numeric sentinels pass unchanged. No RNG/timer/detach.
                let request = if (self.session.execution_topology().is_lockstep()
                    || value.is_none())
                    && action.params.first().map_or(0, |v| crt_atoi(v)) == 8
                {
                    ThemeRequest::Name(action.params.get(1).cloned().unwrap_or_default())
                } else {
                    ThemeRequest::Index(value.unwrap_or(-1))
                };
                effects.push(TriggerEffect::MusicTheme { request });
            }
            0 => {
                //Unsigned kind-1 falls through6DFDDD with the Execute
                //entry's AL1. ALL01 has nine authored no-op tail records.
            }
            3 | 13 => {
                //3:6DEA97 ->6E45E0 writes only selected House+1EE;
                //13:6DEB18 ->same selector writes only +1EF at6DEB41.
                //The retained live selector owns8997 and Country/Player lookup.
                let (Some(live), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let Some(owner) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                if let Some(house) = self.houses.get_mut(&owner) {
                    if action.kind == 13 {
                        house.enable_scripted_autocreate();
                    } else {
                        house.enable_scripted_production();
                    }
                }
            }
            10 => {
                //6DE72F..6DE76C brackets Movie5BF260(index,-1,1,1,1)
                //with the existing input/display/audio service boundaries.
                //Movie admission belongs to that shared app owner; Execute
                //still returns AL1 even for an absent movie name.
                effects.push(TriggerEffect::FullscreenMovie {
                    index: value.unwrap_or(0),
                });
            }
            31 => self.grow_shroud(
                crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
            ),
            51 => {
                //6DEF9B calls the incoming House's577C90 map transition,
                //then returns AL1 independently of the void service result.
                self.trigger_reshroud_map(context.incoming_house, rules);
            }
            113 => {
                //6DF848 selects +90 through the retained live House selector,
                //then House50C8C0 captures the initial Techno count and visits
                //the live array forward. No alive/limbo/health gate precedes
                //virtual388(0); the Infantry owner decides CanCheer.
                let (Some(live), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let Some(owner) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                self.house_cheer(owner, rules, frame_effects);
            }
            52 => {
                //Execute6DEFBC..6DF04E: forward live Building array; no House filter.
                let Some(live) = context.live_index else {
                    return false;
                };
                let mut matched = false;
                let mut ordinal = 0;
                while let Some(id) = self.substrate.entities.building_registry_at(ordinal) {
                    let light = self
                        .substrate
                        .entities
                        .get(id)
                        .filter(|e| {
                            e.health.current() > 0
                                && e.lifecycle.object_alive()
                                && e.lifecycle.cell_marked()
                                && !e.lifecycle.in_limbo()
                        })
                        .filter(|e| {
                            self.object_type(e.type_ref(), rules)
                                .is_some_and(|o| o.has_spotlight)
                        })
                        .and_then(|e| e.building.building_light.light());
                    if let Some(light) = light
                        && self.tag_contains_live_trigger(TagAttachment::Object(id), live)
                    {
                        self.set_spotlight_behavior(light, value.unwrap_or(0));
                        matched = true;
                    }
                    ordinal += 1;
                }
                return matched;
            }
            60 => {
                //6DDEF5 scans the live Building registry forward and calls
                //its unique447110 SellBack(1) owner for tagged candidates.
                //A matched candidate makes AL1 even when SellBack refuses.
                let Some(live) = context.live_index else {
                    return false;
                };
                let mut matched = false;
                let mut ordinal = 0;
                while let Some(id) = self.substrate.entities.building_registry_at(ordinal) {
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.category() == crate::map::entities::EntityCategory::Structure
                            && entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && !entity.lifecycle.in_limbo()
                    });
                    if eligible && self.tag_contains_live_trigger(TagAttachment::Object(id), live) {
                        crate::sim::production::sell_back(
                            self,
                            rules,
                            id,
                            crate::sim::production::SellOrder::Computer,
                            overlay_registry,
                            frame_effects,
                        );
                        matched = true;
                    }
                    ordinal += 1;
                }
                return matched;
            }
            61 | 62 => {
                // Execute6DDF65/6DDFD9: live BuildingClass array order,
                // ObjectAlive+90, Marked+74, !Limbo81 and TagContains6E5380.
                // Receipt is true once the callee is invoked, even a no-op.
                let Some(live) = context.live_index else {
                    return false;
                };
                let online = action.kind == 62;
                let mut matched = false;
                let mut ordinal = 0;
                while let Some(id) = self.substrate.entities.building_registry_at(ordinal) {
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && !entity.lifecycle.in_limbo()
                            && entity.building_online() != online
                    });
                    if eligible && self.tag_contains_live_trigger(TagAttachment::Object(id), live) {
                        if online {
                            self.go_building_online(id, rules, frame_effects);
                        } else {
                            self.go_building_offline(id, rules, frame_effects);
                        }
                        matched = true;
                    }
                    ordinal += 1;
                }
                return matched;
            }
            91 => {
                // Steam gamemd:6DDAF9..6DDB5A walks
                // Infantry A83DEC/A83DF8, NOT Foot. Only Alive90,
                // Marked74,!Limbo81 and TagContains6E5380 of this live
                // Trigger gate virtual514/5220F0. Health/House/value do not.
                let Some(live) = context.live_index else {
                    return false;
                };
                let mut found = false;
                let mut ordinal = 0;
                while let Some(id) = self.substrate.entities.infantry_registry_at(ordinal) {
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && !entity.lifecycle.in_limbo()
                            && self.tag_contains_live_trigger(TagAttachment::Object(id), live)
                    });
                    if eligible {
                        self.substrate
                            .entities
                            .get_mut(id)
                            .expect("the eligibility check read this entity just above")
                            .infantry
                            .as_mut()
                            .expect("Infantry constructor owns its runtime")
                            .make_psycho();
                        found = true;
                    }
                    ordinal += 1;
                }
                return found;
            }
            81..=84 => {
                use crate::map::entities::EntityCategory;
                use crate::sim::mission::{MissionId, MissionType};

                // Steam gamemd: Execute6DDB67 ->6E01C0 (81),
                //6DDBF6 (82),6DDC71 (83). Original execution, including
                //Tag6E5380/GetMission5B3040/Queue5B35E0/41BA90:
                //tools/spatial_oracle/trigger_action_values.py --wakeup.
                //81 walks Techno by live Trigger identity and deliberately
                //does NOT test Health or Limbo. 82/83 walk Foot and compare
                //the incoming House, not the scalar or retained House.
                //84's active Execute6DDCEC..6DDD74 (WakeupGroup6E03B0)
                //walks every Techno category and matches raw signed Group,
                //without a House or Tag gate, before Queue(Guard,false).
                let mut ordinal = 0;
                while let Some(id) = if matches!(action.kind, 81 | 84) {
                    self.substrate.entities.techno_registry_at(ordinal)
                } else {
                    self.substrate.entities.foot_registry_at(ordinal)
                } {
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        let mission = entity.mission.effective().known();
                        entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && if action.kind == 81 {
                                entity.category() != EntityCategory::Structure
                                    && matches!(
                                        mission,
                                        Some(MissionType::Sleep | MissionType::Harmless)
                                    )
                                    && context.live_index.is_some_and(|live| {
                                        self.tag_contains_live_trigger(
                                            TagAttachment::Object(id),
                                            live,
                                        )
                                    })
                            } else {
                                entity.health.current() > 0
                                    && !entity.lifecycle.in_limbo()
                                    && if action.kind == 84 {
                                        entity.object_group() == value.unwrap_or(0)
                                            && matches!(
                                                mission,
                                                Some(MissionType::Sleep | MissionType::Harmless)
                                            )
                                    } else if action.kind == 82 {
                                        mission == Some(MissionType::Sleep)
                                            && Some(entity.owner()) != context.incoming_house
                                    } else {
                                        mission == Some(MissionType::Harmless)
                                            && Some(entity.owner()) == context.incoming_house
                                    }
                            }
                    });
                    if eligible && let Some(entity) = self.substrate.entities.get_mut(id) {
                        //Every admitted call is Queue(Guard,0): no readiness,
                        //Commence, timer write, RNG draw or detach. The normal
                        //AI promotion remains with Mission's existing owner.
                        crate::sim::mission::authority::queue_entity_mission_deferred(
                            entity,
                            MissionId::from_known(MissionType::Guard),
                        );
                    }
                    ordinal += 1;
                }
                //Original AL=1 even for an empty registry or no matches.
            }
            32 => {
                //6DEC3B ->6E2050 uses live Tag membership, not owner/name.
                //Repeated forward scans re-read the dynamic registry after
                //every direct C4 receiver, including nested trigger effects.
                let Some(live) = context.live_index else {
                    return false;
                };
                let Some(c4_name) = rules
                    .warhead(&rules.bridge_warheads.c4_name)
                    .map(|warhead| warhead.id.as_str())
                else {
                    return false;
                };
                let c4 = self.interner.intern(c4_name);
                let mut matched = false;
                // VERA untrusted-map security deviation from DestroyAttached
                // 6E2050 (original instructions): a refused C4 receiver must
                // not spin at6E20DF or the outer6E20EA rescan forever. In
                // particular, Building442230 preserves Immune repair huts.
                // Retain that immunity; skip only this call's nonprogressing
                // receivers. Ordinary damage/cleanup order remains unchanged.
                let mut refused: BTreeSet<u64> = BTreeSet::new();
                loop {
                    let mut visited_match = false;
                    let mut ordinal = 0;
                    while let Some(id) = self.substrate.entities.techno_registry_at(ordinal) {
                        if refused.contains(&id) {
                            ordinal += 1;
                            continue;
                        }
                        let health = self.substrate.entities.get(id).and_then(|entity| {
                            (entity.health.current() > 0
                                && entity.lifecycle.object_alive()
                                && entity.lifecycle.cell_marked()
                                && !entity.lifecycle.in_limbo())
                            .then_some(entity.health.current())
                        });
                        let Some(health) = health.filter(|_| {
                            self.tag_contains_live_trigger(TagAttachment::Object(id), live)
                        }) else {
                            ordinal += 1;
                            continue;
                        };
                        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                            id,
                            health,
                            0,
                            crate::sim::combat::RAD_NO_ATTACKER,
                            None,
                            c4,
                            crate::sim::combat::ReceiverCallFlags {
                                ignore_defenses: true,
                                arg6: false,
                            },
                        );
                        self.commit_direct_damage_receiver(
                            rules,
                            overlay_registry,
                            event,
                            frame_effects,
                        );
                        // Ares 3.0 TActionClass_DestroyAttached_Loop (1005D150,
                        // hook 0x6E20D8): a receiver that survives (below
                        // NowDead) is passed over; it neither counts nor
                        // restarts the scan, so no refusal set is needed.
                        if ares {
                            if self.substrate.entities.get(id).is_some_and(|entity| {
                                entity.health.current() > 0 && entity.lifecycle.object_alive()
                            }) {
                                ordinal += 1;
                                continue;
                            }
                        } else if self
                            .substrate
                            .entities
                            .get(id)
                            .is_some_and(|entity| entity.health.current() >= health)
                        {
                            refused.insert(id);
                        }
                        matched = true;
                        visited_match = true;
                        //6E20DF decrements the slot before the loop increment:
                        //a removed object exposes its successor at this slot.
                    }
                    if !visited_match {
                        break;
                    }
                }
                //Static initializer6DCF10 makes B0E680==(0,0). For a real
                //incoming Cell, the same587180 owner as Action63 retries at
                //most four times. No authored waypoint substitutes for Cell.
                if context.input.cell != (0, 0) && self.resolved_terrain.is_some() {
                    let mut driver =
                        crate::sim::world::bridge_orchestrator::BridgeDamageDrivers::new(
                            self,
                            rules,
                            frame_effects.reborrow(),
                        );
                    for _ in 0..4 {
                        if driver.apply_damage_to_cell(context.input.cell) {
                            break;
                        }
                    }
                    if driver.collapsed() {
                        effects.push(TriggerEffect::BridgeStateChanged);
                    }
                    //6E213D..6E21AF invalidates a256px rectangle; the app's
                    //normal draw rebuilds the whole tactical scene each frame.
                }
                return matched;
            }
            116 => {
                //6E1980 clears the first Building/Terrain's custom handle;
                //otherwise750F40 detaches matching coordinate requests,flags1.
                return self.trigger_stop_sound_at_waypoint(
                    action.waypoint_index,
                    rules,
                    frame_effects,
                );
            }
            118 => {
                //6E2C20 ->Map588AC0 clears smudge identities, preserving
                //their data bytes;4F42F0(1) then invalidates tactical drawing.
                //Native6E2C20 returns AL1 after its void map/draw calls.
                //The owner's safe unavailable-map/type result is a Rust
                //boundary, not a reason to retry an admitted action.
                self.trigger_clear_all_smudges(rules);
            }
            125 => {
                let (Some(owner), Some(waypoint)) = (context.incoming_house, action.waypoint_index)
                else {
                    return false;
                };
                return self.trigger_create_building_at_waypoint(
                    owner,
                    waypoint as i32,
                    action.inline_type_name(),
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            111 => {
                //Execute6DF717 scans Building constructors in forward order;
                //first live+marked+nonlimbo match gets SellBuilding457DE0(0,0).
                let Some(live) = context.live_index else {
                    return false;
                };
                let mut ordinal = 0;
                while let Some(id) = self.substrate.entities.techno_registry_at(ordinal) {
                    let eligible = self.substrate.entities.get(id).is_some_and(|e| {
                        e.category() == crate::map::entities::EntityCategory::Structure
                            && e.lifecycle.object_alive()
                            && e.lifecycle.cell_marked()
                            && !e.lifecycle.in_limbo()
                    });
                    if eligible && self.tag_contains_live_trigger(TagAttachment::Object(id), live) {
                        crate::sim::passenger::garrison_eject::sell_building_occupants(
                            self,
                            rules,
                            overlay_registry,
                            id,
                            crate::sim::passenger::garrison_eject::GarrisonEjectMode::Release,
                            frame_effects,
                        );
                        return true;
                    }
                    ordinal += 1;
                }
                return false;
            }
            126 | 130 => {
                //126/6E30C0 and130/6E3120 use the same live-trigger
                // country selector as119..122;50D440/50D320 consume the
                // existing House history, with different restoration modes.
                let (Some(live), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let owner = self.live_action_house(live, value, rules);
                let Some(owner) = owner else {
                    return false;
                };
                self.restore_initial_house_technos(
                    owner,
                    action.kind == 126,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            119..=122 => {
                //6E3180/31E0/3240/32A0 require a live Trigger context.
                //8997 resolves its retained House via726910; other values
                //use the existing510F60/510ED0/502D30 selector owner.
                let (Some(live), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let owner = self.live_action_house(live, value, rules);
                let Some(owner) = owner else {
                    return false;
                };
                use crate::sim::world::HouseBlowupSubset;
                let subset = match action.kind {
                    119 => HouseBlowupSubset::All,
                    120 => HouseBlowupSubset::Buildings,
                    121 => HouseBlowupSubset::Land,
                    122 => HouseBlowupSubset::Naval,
                    _ => unreachable!(),
                };
                self.house_blowup(owner, subset, rules, overlay_registry, frame_effects);
            }
            37 => {
                // Active Execute37 6DE136..6DE17A: resolve the selected
                // House through the same live6E45E0 owner, then MakeAlly
                // incoming->selected and selected->incoming, in that order.
                // Both calls ignore CanAlly's refusal; Execute still AL1.
                let (Some(live), Some(owner), Some(value)) =
                    (context.live_index, context.incoming_house, value)
                else {
                    return false;
                };
                let Some(selected) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                // 0x006DE168/0x006DE171 push EBP, null here: no announcement.
                self.make_ally(
                    owner,
                    selected,
                    rules,
                    overlay_registry,
                    frame_effects,
                    false,
                );
                self.make_ally(
                    selected,
                    owner,
                    rules,
                    overlay_registry,
                    frame_effects,
                    false,
                );
            }
            123 | 124 => {
                // Actual active table entries6DFA0B/6DFA38 dispatch
                // 6E0CA0/6E0D00 ->House50D290/50D2D0. A live Trigger is
                // required even for a literal Country;8997 uses retained House.
                let (Some(live), Some(owner), Some(value)) =
                    (context.live_index, context.incoming_house, value)
                else {
                    return false;
                };
                let Some(selected) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                self.control_house_buildings(
                    owner,
                    selected,
                    action.kind == 124,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            36 => {
                let (Some(live), Some(owner), Some(value)) =
                    (context.live_index, context.incoming_house, value)
                else {
                    return false;
                };
                let Some(target) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                return self.trigger_change_all_house_objects(
                    owner,
                    target,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            14 => {
                //6E0AA0 ->6E0B40 dynamic Techno array;8997 is only726910's
                //live+2C. Never substitute the authored incoming House.
                let (Some(live_index), Some(value)) = (context.live_index, value) else {
                    return false;
                };
                let target = self.live_action_house(live_index, value, rules);
                let Some(target) = target else {
                    return false;
                };
                let mut found = false;
                let mut ordinal = 0;
                while let Some(id) = self.substrate.entities.techno_registry_at(ordinal) {
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && !entity.lifecycle.in_limbo()
                    });
                    let attached =
                        self.tag_contains_live_trigger(TagAttachment::Object(id), live_index);
                    if eligible && attached {
                        //Native announce=false; same-owner early return still
                        //counts as a matched candidate. Shared owner updates
                        //House counts, power, lists, discovery and cleanup.
                        self.change_owner_with_rules(
                            id,
                            target,
                            rules,
                            overlay_registry,
                            frame_effects,
                        );
                        found = true;
                    }
                    ordinal += 1;
                }
                return found;
            }
            16 => {
                return self.trigger_reveal_entire_map(
                    crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
                );
            }
            5 => {
                let Some(DefinitionReference::Team(team)) = reference else {
                    return false;
                };
                // Execute6DEB8D..6DEBAB, TeamType6F0A70. The original
                // removes matching teams, releasing rather than killing members.
                let type_id = self
                    .interner
                    .intern(&self.trigger_runtime.definitions.team_types[team]);
                self.destroy_teams_of_type(type_id, rules, frame_effects);
            }
            104 => {
                //6DF562 ->6F1F30 ->6EF5F0: signed+90 writes each member's
                //flash counter, filtering only TeamType identity. A null
                //Team reference still returns AL1 in the original.
                if let Some(DefinitionReference::Team(team)) = reference {
                    let team_id = self
                        .interner
                        .intern(&self.trigger_runtime.definitions.team_types[team]);
                    Self::flash_teams_of_type(
                        &mut self.substrate,
                        &self.team_script_vm,
                        team_id,
                        value.unwrap_or(0),
                    );
                }
            }
            4 | 7 | 80 | 107 => {
                let Some(DefinitionReference::Team(team)) = reference else {
                    return false;
                };
                let team_id = self
                    .interner
                    .intern(&self.trigger_runtime.definitions.team_types[team]);
                if action.kind == 4 {
                    return self
                        .trigger_create_team(team_id, rules, overlay_registry, frame_effects)
                        .is_some();
                }
                // Action7@6DEBAE calls65D8E0 with the TeamType waypoint. Action80
                //6DEBD0 passes its+44 override to the same65D8E0 owner;
                //null Team or waypoint-1 is rejected before that call.
                let waypoint = if matches!(action.kind, 80 | 107) {
                    let Some(waypoint) = action.waypoint_index else {
                        return false;
                    };
                    waypoint as i32
                } else {
                    -1
                };
                if matches!(action.kind, 80 | 107) && waypoint == -1 {
                    return false;
                }
                //1076DF672..6DF698 shares the same resolved Team/+44 gate,
                //then calls chrono65EC30 instead of ordinary65D8E0. Only -1
                //is rejected, not every signed negative (original execution).
                let (result, recent_event) = if action.kind == 107 {
                    self.spawn_chrono_team_reinforcements(
                        team_id,
                        waypoint,
                        rules,
                        overlay_registry,
                        frame_effects,
                    )
                } else {
                    self.spawn_team_reinforcements(
                        team_id,
                        waypoint,
                        rules,
                        overlay_registry,
                        frame_effects,
                    )
                };
                if let Some(cell) = recent_event {
                    effects.push(TriggerEffect::RecentEventCell { cell });
                }
                return result;
            }
            11 => {
                // Execute6DE0A8 skips an empty inline label. CSF resolution
                // and MessageList5D3BA0 remain ordered presentation consumers.
                let label = action.csf_label();
                if !label.is_empty() {
                    effects.push(TriggerEffect::LocalizedMessage {
                        csf_label: label.to_string(),
                    });
                }
            }
            17 => {
                let Some(waypoint) = value else {
                    return false;
                };
                return self.trigger_reveal_at_waypoint(
                    waypoint,
                    crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
                );
            }
            108 => {
                //Steam gamemd: active Execute6DF69B..6DF6CD ->68BCC0
                //->56BEC0. Unset valid waypoints supply(0,0) to FNPC; the
                //Place AL is returned. Compare the full scalar with20 before
                //the fixed choice's low-byte store, including276 -> byte20.
                //Original execution: trigger_action_values.py --crate.
                let (Some(registry), Some(value), Some(waypoint)) =
                    (overlay_registry, value, action.waypoint_index)
                else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint as i32) else {
                    return false;
                };
                return crate::sim::crates::place_crate(
                    self,
                    rules,
                    registry,
                    cell,
                    (value != 20).then_some(value as u8),
                    frame_effects,
                )
                .unwrap_or_else(|error| {
                    log::error!("Trigger crate placement failed: {error}");
                    false
                });
            }
            101 => {
                let Some(waypoint) = value else {
                    return false;
                };
                return self.trigger_conceal_at_waypoint(
                    waypoint,
                    crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
                );
            }
            18 => {
                let Some(waypoint) = value else {
                    return false;
                };
                return self.trigger_reveal_zone_at_waypoint(
                    waypoint,
                    crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
                );
            }
            ACTION_PLAY_SOUND => {
                // Execute6DD8B0 case19 (6DE780..6DE791): Voc PlayAtPos750920
                // of +90, centre pan2000, volume1.0, no handle. -1 returns at
                //75093B.
                if let Some(sound_id) = sound {
                    self.emit_trigger_sound(
                        sound_id,
                        super::super::world::TriggerSoundTarget::Centered,
                    );
                    frame_effects.flush_sound(self, rules);
                }
            }
            55 => {
                // Execute6DF1AD: +44 waypoint, +90 native radar-event type.
                // Client65FA70 owns admission/dedup; no House filter here.
                let Some(cell) = action
                    .waypoint_index
                    .and_then(|waypoint| self.session.waypoint_cell(waypoint as i32))
                else {
                    return false;
                };
                if let Ok(kind) = crate::sim::radar::RadarEventType::try_from(value.unwrap_or(0)) {
                    self.sound_events.emit(SimSoundEvent::RadarEvent {
                        radar: crate::sim::radar::RadarEventRequest::new(
                            kind,
                            cell.0 as u16,
                            cell.1 as u16,
                        ),
                    });
                    frame_effects.flush_sound(self, rules);
                }
            }
            100 | 117 => {
                // Execute6DF546/6DF90E -> radar movie5BF390. Preparation,
                // the game-mode gate and missing-media handling are app-owned;
                // the action returns success independently of queue admission.
                effects.push(TriggerEffect::RadarMovie {
                    index: value.unwrap_or(0),
                    pause_simulation: action.kind == 117,
                });
            }
            114 => {
                let index = value.unwrap_or(0);
                if !(0..4).contains(&index) {
                    return false;
                }
                effects.push(TriggerEffect::SelectSidebarTab { index });
                //6E4100 ->6ABAC0/6A7590: presentation checks retained strip
                //membership. Native's empty-strip AL0 is only a client
                //receipt: all three Tag callers6E5439/6E5450/6E546D ignore
                //Spring's AL, as does ForceTrigger. Keep that client result
                //out of deterministic simulation; the UI owns actual admission.
            }
            115 => {
                // TAction::FlashCameo6E4150, original instructions: reverse
                // exact-name search of the global constructor chronology.
                // Sidebar6AC5F0 consumes that selected type's class/index;
                // retain those facts instead of resolving its name again.
                let Some(object) = rules.native_techno_type_exact(action.inline_type_name()) else {
                    return false;
                };
                let Some(type_index) = rules.type_array_index(object.category, &object.id) else {
                    return true;
                };
                effects.push(TriggerEffect::FlashSidebarCameo {
                    category: crate::sim::production::category_for_object(object),
                    type_index,
                    duration: value.unwrap_or(0),
                    frame: self.session.binary_frame() as i32,
                });
            }
            139 => {
                // Execute6DFCE3 -> TAction6E3B40 uses the incoming House,
                // not Action.value as a selector. House50BCD0 owns the raw
                // frame-duration timer and5779 dirty flag; reuse that port.
                // Native Steam gamemd; timer/availability goldens:
                // tools/superweapon_oracle.json::radar_outage.
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                self.power_states
                    .house_mut(owner)
                    .start_radar_outage(self.session.binary_frame(), value.unwrap_or(0));
            }
            128 => {
                //Execute6DFAD9 ->6E1A40 ->House50D6D0. Only +44 waypoint
                //and incoming House are read: the numeric +90 is ignored.
                let (Some(owner), Some(waypoint)) = (context.incoming_house, action.waypoint_index)
                else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint as i32) else {
                    return false;
                };
                return self
                    .chronoshift_house_to(owner, cell, rules, overlay_registry, frame_effects)
                    .map_or_else(
                        |error| {
                            log::debug!("Action128: {error}");
                            false
                        },
                        |_| true,
                    );
            }
            127 => {
                // Execute6DFABF -> ChronoScreen53B460. A nonpositive duration
                // is a no-op there; the action still returns success.
                self.start_chrono_screen(value.unwrap_or(0), effects);
            }
            131 => {
                // FlashBuildings6E4560..6E45DE: incoming House's live
                // Building list, reverse order, exact inline type ID; raw
                // Techno+F0 write, without Building::Flash's disguise gate.
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                let ids: Vec<_> = self
                    .houses
                    .get(&owner)
                    .map(|house| {
                        house
                            .base_projection()
                            .buildings()
                            .iter()
                            .rev()
                            .copied()
                            .collect()
                    })
                    .unwrap_or_default();
                for id in ids {
                    let matches = self.substrate.entities.get(id).is_some_and(|entity| {
                        self.resolve(entity.type_ref()) == action.inline_type_name()
                    });
                    if matches && let Some(entity) = self.substrate.entities.get_mut(id) {
                        entity.flashing.start(value.unwrap_or(0));
                    }
                }
            }
            21 => {
                let Some(index) = value else {
                    return false;
                };
                if self.session.execution_topology().is_lockstep()
                    && action.params.first().map_or(0, |v| crt_atoi(v)) != 7
                    && action.params.first().map_or(0, |v| crt_atoi(v)) != 8
                {
                    if let Some(reference) = sound {
                        self.sound_events.emit(SimSoundEvent::TriggerAudio {
                            reference: reference.clone(),
                            target: super::super::world::TriggerSoundTarget::Eva,
                        });
                    }
                } else {
                    self.sound_events.emit(SimSoundEvent::TriggerEva { index });
                }
                frame_effects.flush_sound(self, rules);
            }
            59 => {
                let (Some(registry), Some(waypoint)) = (overlay_registry, value) else {
                    return false;
                };
                return Self::trigger_reduce_tiberium_at_waypoint(
                    &self.terrain_objects,
                    &self.ore_growth_config,
                    &mut self.ore_growth_state,
                    &self.session,
                    &mut self.scenario_rng,
                    &self.substrate,
                    &mut self.resolved_terrain,
                    &mut self.overlay_grid,
                    &mut self.dirty_cells,
                    waypoint,
                    rules,
                    registry,
                );
            }
            85 => {
                // Execute6DDB94/6E0250 stores the literal low byte in34A7.
                self.session.set_vein_growth(value.unwrap_or(0));
            }
            110 => {
                effects.extend(self.session.pause_scenario(value.unwrap_or(0)));
                // ScenarioPause684060 pauses elapsed time and Vox inline,
                // before a following result action or House's Vox poll.
                frame_effects.consume_prefix_effects(effects);
            }
            87 => {
                // Steam gamemd, Execute6DDBD5..6DDBF3.
                // This is the retained ice flag; never run the ore grower.
                let Some(value) = value else {
                    return false;
                };
                self.session.set_ice_growth(value);
            }
            97 => {
                // Execute6DD8EE..6DD910: SETE of current Scenario+34A5;
                // no Rules, actor, House, waypoint or numeric-value gate.
                self.session.toggle_train_crate();
            }
            86 => {
                let Some(value) = value else {
                    return false;
                };
                self.ore_growth_config.set_enabled_from_trigger(value);
            }
            88 => {
                let (Some(index), Some(waypoint)) = (value, action.waypoint_index) else {
                    return false;
                };
                return self.trigger_create_particle_system_at_waypoint(
                    index,
                    waypoint as i32,
                    rules,
                );
            }
            89 => {
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                return Self::trigger_mark_particle_systems_done_at_waypoint(
                    &self.session,
                    &mut self.substrate,
                    waypoint as i32,
                );
            }
            30 => {
                let (Some(owner), Some(value)) = (context.incoming_house, value) else {
                    return false;
                };
                return self.trigger_auto_base_building(
                    owner,
                    value,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            35 => {
                // Execute35 6DE1DC..6DE203 uses the incoming House, not
                // the live selector. A null House succeeds without writes;
                // only its existing superweapon-target owner changes.
                let Some(owner) = context.incoming_house else {
                    return true;
                };
                let Some(value) = value else { return false };
                let Some(house) = self.houses.get_mut(&owner) else {
                    return false;
                };
                house.set_preferred_super_weapon_target_kind(value);
            }
            76..=79 => {
                // Active76..79 at6DF34F/379/3A3/3CD: a null incoming House
                // succeeds without reading +90. Each stores that signed dword
                // in its existing House owner: +565C/5660/5664/5668 respectively.
                // No immediate selection, timer reset, RNG draw or detach.
                let Some(owner) = context.incoming_house else {
                    return true;
                };
                let Some(value) = value else { return false };
                let Some(house) = self.houses.get_mut(&owner) else {
                    return false;
                };
                match action.kind {
                    76 => house.team_creation_mut().set_trigger_ratio(value),
                    77 => house.set_aircraft_team_ratio(value),
                    78 => house.set_infantry_team_ratio(value),
                    79 => house.set_unit_team_ratio(value),
                    _ => unreachable!(),
                }
            }
            8 => {
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                return self.trigger_drop_zone_flare(rules, waypoint as i32);
            }
            98 => {
                return self.trigger_play_sound_at_random_waypoint(sound, rules, frame_effects);
            }
            41 => {
                let (Some(index), Some(waypoint)) = (value, action.waypoint_index) else {
                    return false;
                };
                return self.trigger_play_anim_at_waypoint(
                    rules,
                    index,
                    waypoint as i32,
                    context.incoming_house,
                    frame_effects,
                );
            }
            43 => {
                let (Some(index), Some(waypoint)) = (value, action.waypoint_index) else {
                    return false;
                };
                return self.trigger_play_voxel_anim_at_waypoint(rules, index, waypoint as i32);
            }
            58 => {
                let (Some(intensity), Some(waypoint)) = (value, action.waypoint_index) else {
                    return false;
                };
                return self.trigger_meteor_shower(rules, intensity, waypoint as i32);
            }
            42 => {
                let (Some(index), Some(waypoint)) = (value, action.waypoint_index) else {
                    return false;
                };
                let Some(changed) = self.trigger_explosion_at_waypoint(
                    rules,
                    index,
                    waypoint as i32,
                    overlay_registry,
                    frame_effects,
                ) else {
                    return false;
                };
                if changed {
                    effects.push(TriggerEffect::BridgeStateChanged);
                }
            }
            94 => {
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                return self.trigger_ion_blast(waypoint as i32);
            }
            95 => {
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                return self.trigger_nuke_strike(rules, waypoint as i32);
            }
            96 => {
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                return self.trigger_chemical_strike(rules, waypoint as i32);
            }
            109 => {
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                return crate::sim::superweapon::iron_curtain::at_waypoint(
                    self,
                    rules,
                    waypoint as i32,
                    overlay_registry,
                    frame_effects,
                );
            }
            46 | 47 => {
                let locked = action.kind == 46;
                if locked {
                    // TActionClass::Execute case 46 (0x006DED8B..0x006DEDD7;
                    // 0x006E2660 is an unreferenced copy) clears the Display modes
                    // before Lock_User_Input 0x00684240. The pause 0x00684060,
                    // Flag_To_Win/Lose 0x004FCB39/0x004FCD22 and the chrono
                    // screen 0x0053B54F lock without them.
                    effects.push(TriggerEffect::CancelPlacementModes);
                }
                let was_locked = if locked {
                    self.session.lock_input()
                } else {
                    self.session.unlock_input()
                };
                effects.push(TriggerEffect::InputLockChanged { locked, was_locked });
            }
            ACTION_ENABLE_TRIGGER | ACTION_DISABLE_TRIGGER => {
                if let Some(DefinitionReference::Trigger(target)) = reference {
                    let difficulty = self.trigger_difficulty();
                    let mut ordinal = 0;
                    while ordinal < self.trigger_runtime.trigger_order.len() {
                        let live = self.trigger_runtime.trigger_order[ordinal];
                        if self
                            .trigger_runtime
                            .trigger(live)
                            .is_some_and(|t| t.type_index == target)
                        {
                            match action.kind {
                                ACTION_ENABLE_TRIGGER => {
                                    if self.trigger_runtime.definitions.trigger_types[target]
                                        .admits_difficulty(difficulty)
                                    {
                                        self.trigger_runtime.triggers[live]
                                            .as_mut()
                                            .expect("the scan read this live Trigger just above")
                                            .enabled = true;
                                        Self::reset_trigger_timers(
                                            &self.session,
                                            &mut self.scenario_rng,
                                            &mut self.trigger_runtime,
                                            live,
                                        );
                                    }
                                }
                                ACTION_DISABLE_TRIGGER => {
                                    self.trigger_runtime.triggers[live]
                                        .as_mut()
                                        .expect("the scan read this live Trigger just above")
                                        .enabled = false;
                                }
                                _ => unreachable!(),
                            }
                        }
                        ordinal += 1;
                    }
                }
            }
            ACTION_SET_GLOBAL | ACTION_CLEAR_GLOBAL | ACTION_SET_LOCAL | ACTION_CLEAR_LOCAL => {
                let global = matches!(action.kind, ACTION_SET_GLOBAL | ACTION_CLEAR_GLOBAL);
                if let Some(index) = value {
                    Self::set_trigger_variable(
                        &self.session,
                        &mut self.scenario_rng,
                        &mut self.trigger_runtime,
                        global,
                        index,
                        matches!(action.kind, ACTION_SET_GLOBAL | ACTION_SET_LOCAL),
                    );
                }
            }
            142..=144 => {
                let Some(value) = value else {
                    return false;
                };
                // Original Execute6DFD4F/7C/A9 ->6E2E60/6E2EB0/6E2F00:
                // no House/actor gate;53AD00 uses normal-cache flag0, then
                // the raw normal component is stored. The published snapshot
                // includes that store: Cell refresh reads ambient/Ground/Level,
                // not RGB, and the explicit palette operation preserves order.
                let rgb = self
                    .session
                    .lighting_mut()
                    .retint_normal_component((action.kind - 142) as usize, value);
                self.publish_global_lighting(crate::sim::light_sources::PaletteUpdate::Normal(rgb));
            }
            73 => {
                let Some(value) = value else {
                    return false;
                };
                let storm = crate::sim::superweapon::lightning_storm::raging(self);
                self.session.lighting_mut().set_ambient_light(value, storm);
            }
            64..=66 => {
                let Some(waypoint) = value else {
                    return false;
                };
                // Execute64/65/66 at6DDDF6/6DDE4B/6DDEA0 uses +90,
                // not the +44 tail waypoint, with damage50/100/300.
                // The same C4 reference/coordinate/Flash owners serve ordinary
                // impacts; stock C4=Super is not Bright, so materializes no
                // light. Do not force a screen/nuke flash from the action name.
                // Native: trigger_map_lighting.json flash_controls (+meta).
                let Some([x, y, z]) = self.trigger_waypoint_location(waypoint) else {
                    return false;
                };
                let warhead_ref = self.interner.intern(&rules.bridge_warheads.c4_name);
                self.combat_light_requests
                    .emit(crate::sim::combat::CombatLightRequest::Impact {
                        target_id: None,
                        damage: match action.kind {
                            64 => 50,
                            65 => 100,
                            _ => 300,
                        },
                        warhead_ref,
                        coord: crate::sim::projectile::ProjectileCoord::new(x, y, z),
                        force_create: false,
                        flags: 0,
                    });
            }
            71 | 72 => {
                let Some(bits) = value else {
                    return false;
                };
                self.session
                    .lighting_mut()
                    .set_ambient_change_input(action.kind == 72, bits);
            }
            ACTION_DAMAGE_AT_WAYPOINT => {
                let Some(waypoint) = value else {
                    return false;
                };
                let Some(changed) = crate::sim::world::bridge_orchestrator::damage_at_waypoint(
                    self,
                    rules,
                    waypoint,
                    overlay_registry,
                    frame_effects,
                ) else {
                    return false;
                };
                if changed {
                    effects.push(TriggerEffect::BridgeStateChanged);
                }
            }
            ACTION_CHANGE_VISIBLE_MAP_AREA => {
                // `TActionClass::Read @ 0x006DD5B0` parses ActionID,
                // ParamType, Param3, then stores the four writer dwords at
                // +0x34..+0x40. ActionEntry.params is chunk[1..], so these are
                // exactly indices 2..5; params[0]/[1] must never leak in.
                if let Some(raw_local_size) = parse_visible_map_area(&action.params) {
                    let _ = self.change_visible_map_area(
                        raw_local_size,
                        crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
                    );
                }
            }
            145 => {
                // Execute6DFDD6 calls Home7315A0 for PlayerPtr, independent
                // of the incoming House/value. Resolve coordinates before
                // the frame hash, including House50DF30's Dummy effects.
                let coord = {
                    let network = self.session.execution_topology().is_lockstep();
                    let cells = self.resolved_terrain.as_ref().map(|terrain| {
                        crate::map::resolved_terrain::NativeCellQuery::for_local_input(
                            terrain, network,
                        )
                    });
                    self.home_camera_coordinate(rules, cells.as_ref())
                };
                effects.push(TriggerEffect::HomeCamera { coord });
            }
            ACTION_CENTER_CAMERA | ACTION_JUMP_CAMERA => {
                // Execute6DD8B0 resolves GetWaypoint68BCC0 and native Cell
                // ground/bridge state before FocusOn/CenterMap. Presentation
                // receives the immutable coordinate; the shared simulation
                // query also retains any process CellClass Dummy writes.
                let Some(coord) = action
                    .waypoint_index
                    .and_then(|index| self.trigger_waypoint_coord(index as i32))
                else {
                    return false;
                };
                let velocity = if action.kind == ACTION_CENTER_CAMERA {
                    let Some(velocity) = value else { return false };
                    Some(velocity)
                } else {
                    None
                };
                effects.push(TriggerEffect::CameraAtCoord { coord, velocity });
            }
            ACTION_PLAY_SOUND_AT_WAYPOINT => {
                return self.trigger_play_sound_at_waypoint(
                    sound,
                    action.waypoint_index,
                    rules,
                    frame_effects,
                );
            }
            105 => {
                // Steam gamemd Execute6DF588..6DF5D0, not sibling6E4040.
                // No matching live Team leaves the previous bubble unchanged;
                // an absent Type or an empty first Team instead clears it.
                let actor = if let Some(DefinitionReference::Team(index)) = reference {
                    let name = &self.trigger_runtime.definitions.team_types[index];
                    let Some(team_type) = self.interner.get(name) else {
                        return false;
                    };
                    let Some(team) = self.team_script_vm.first_team_of_type(team_type) else {
                        return false;
                    };
                    team.members().next()
                } else {
                    None
                };
                // Commit earlier cues to the one frame-output queue before
                // the bubble's own cue and reveal.
                self.publish_trigger_effects(std::mem::take(effects));
                self.talk_bubble_set_object_and_frame(
                    actor,
                    value.unwrap_or(0),
                    crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
                );
            }
            106 => {
                // Active inline Execute6DF5D1, not the similarly annotated
                // separate helper6E37F0. Type9's string is+54; raw value is+90.
                let Some(value) = value else {
                    return false;
                };
                self.set_object_type_tech_level(action.inline_type_name(), value, rules);
            }
            33 | 34 | ACTION_SET_SUPER_CHARGE_PERCENTAGE | 132..=134 => {
                // Execute33/34 (6DE925/6DE98E),1296E42D0,1326E4320,
                //1336E4360 and1346E43A0 select
                // the incoming House's Super-array ordinal (+90). Only129
                // has grant/percentage gates.132 writes +48 as the override;
                //133 clears it without restarting the current countdown;
                //134 calls the same Reset owner as spy infiltration.
                // Original Steam identity and executed admissions/numerics:
                // tools/spatial_oracle/trigger_super_charge_percent.meta.json
                // trigger_superweapon_state.meta.json and
                // trigger_superweapon_reset.meta.json.
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                let Some(index) = value.and_then(|value| usize::try_from(value).ok()) else {
                    return false;
                };
                // Native's index==count out-of-array access is excluded; the
                // retained Rust registry admits only actual stock entries.
                let Some(name) = rules.super_weapon_order.get(index) else {
                    return false;
                };
                let Some(type_id) = self.interner.get(name) else {
                    return false;
                };
                let Some(instance) = self.super_weapons.instance_mut(&owner, &type_id) else {
                    return false;
                };
                match action.kind {
                    33 | 34 => {
                        return crate::sim::superweapon::trigger_grant_super(
                            &self.interner,
                            &self.session,
                            &mut self.substrate,
                            &self.houses,
                            &mut self.super_weapons,
                            rules,
                            owner,
                            type_id,
                            action.kind == 33,
                        );
                    }
                    132 => instance.set_recharge_time(action.auxiliary_value()),
                    133 => instance.reset_recharge_time(),
                    134 => crate::sim::superweapon::reset_super(
                        &self.interner,
                        &self.session,
                        &mut self.substrate,
                        &mut self.sound_events,
                        &mut self.super_weapons,
                        rules,
                        owner,
                        name,
                    ),
                    _ => {
                        let Some(sw) = rules.super_weapon(name) else {
                            return false;
                        };
                        return instance.set_charge_percentage(
                            action.auxiliary_value(),
                            sw,
                            self.session.binary_frame(),
                        );
                    }
                }
            }
            135 | 136 | 140 | 141 => {
                // Execute1356E43E0/1366E4440 and1406E4460/1416E44C0 use
                // Spring's incoming House, ignoring scalar+90. Setters135/140
                // read waypoint+44 and reject the empty cell; clears ignore it.
                // 140 calls House50DA20: cell+54F4 and current frame+54FC.
                // 141 calls50DA50: clear cell and restore frame-100.
                // 135/136 controls: trigger_superweapon_state.meta.json.
                // 140/141: original Steam gamemd instructions.
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                let target = if matches!(action.kind, 135 | 140) {
                    let Some(cell) = action
                        .waypoint_index
                        .and_then(|index| self.session.waypoint_cell(index as i32))
                        .filter(|&cell| cell != (0, 0))
                    else {
                        return false;
                    };
                    Some((cell.0 as u16, cell.1 as u16))
                } else {
                    None
                };
                let Some(house) = self.houses.get_mut(&owner) else {
                    return false;
                };
                if let Some(cell) = target {
                    if action.kind == 135 {
                        house.set_preferred_super_weapon_target(cell);
                    } else {
                        house.alert_super_weapon_defense(cell, self.session.binary_frame() as i32);
                    }
                } else if action.kind == 136 {
                    house.clear_preferred_super_weapon_target();
                } else {
                    house.clear_super_weapon_defense();
                }
            }
            90 => {
                // Steam gamemd, Execute6DF51B..6DF537: GetWaypoint68BCC0's
                // packed cell (an unset slot is (0,0)) goes straight to
                // LightningStorm::CreateCloudBolt53A140; the action succeeds.
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint as i32) else {
                    return false;
                };
                crate::sim::superweapon::lightning_storm::create_cloud_bolt(self, rules, cell);
            }
            44 => {
                // Active YR Execute6DED1C..6DED52 calls LightningStorm,
                // despite the legacy IonStorm name. IsActive53A100 tests
                // only the active byte; a countdown still admits this call.
                // Start539EB0 gets +90 duration, Rules+1794 deferment, the
                // zero EmptyCell (initializer6DCF10), and NULL House.
                if crate::sim::superweapon::lightning_storm::raging(self) {
                    return false;
                }
                let Some(duration) = value else {
                    return false;
                };
                crate::sim::superweapon::lightning_storm::start(
                    self,
                    rules,
                    duration,
                    rules.general.lightning_deferment,
                    (0, 0),
                    None,
                );
            }
            45 => {
                // Execute6DED61..6DED7C refuses an inactive/counting-down
                // storm; Stop53A090 merely requests its eventual end. No
                // immediate cloud deletion, timer write, RNG draw or detach.
                if !crate::sim::superweapon::lightning_storm::raging(self) {
                    return false;
                }
                crate::sim::superweapon::lightning_storm::request_end(&mut self.lightning_storm);
            }
            102 => {
                // 6DD9EC..6DDAB8: the waypoint's signed centre/Ground/bridge
                // coordinate is truncated back to a cell (413A30). A raging
                // storm (IsActive53A100) fails at 6DDA98 without restarting;
                // otherwise Start539EB0 takes the same LightningStormDuration
                // (+179C) / LightningDeferment (+1794) as the Super launch,
                // the incoming House, and no charge gate.
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                if crate::sim::superweapon::lightning_storm::raging(self) {
                    return false;
                }
                let Some([x, y, _]) = self.trigger_waypoint_coord(waypoint as i32) else {
                    return false;
                };
                crate::sim::superweapon::lightning_storm::start(
                    self,
                    rules,
                    rules.general.lightning_storm_duration,
                    rules.general.lightning_deferment,
                    ((x / 256) as i16, (y / 256) as i16),
                    context.incoming_house,
                );
            }
            ACTION_SET_ALTERNATE_BASE => {
                let Some(house_id) = context.incoming_house else {
                    return false;
                };
                let Some(waypoint_index) = action.waypoint_index else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint_index as i32) else {
                    return false;
                };
                if cell == (0, 0) {
                    return false;
                }
                // gamemd-derived: `TriggerAction__Execute @ 0x006DD8B0`
                // case 137 calls `FUN_006E44E0`, which rejects the packed-zero
                // waypoint and writes only `HouseClass+0x5494` through
                // `FUN_0050DFE0`.
                if let Some(house) = self.houses.get_mut(&house_id) {
                    house.set_alternate_base((cell.0 as u16, cell.1 as u16));
                }
            }
            ACTION_CLEAR_ALTERNATE_BASE => {
                let Some(house_id) = context.incoming_house else {
                    return false;
                };
                // gamemd-derived: `TriggerAction__Execute @ 0x006DD8B0`
                // case 138 calls `FUN_006E4540`, which writes packed zero only
                // to `HouseClass+0x5494` through `FUN_0050DFF0`.
                if let Some(house) = self.houses.get_mut(&house_id) {
                    house.clear_alternate_base();
                }
            }
            1 | 2 | ACTION_ANNOUNCE_WIN | ACTION_ANNOUNCE_LOSE | ACTION_END_SCENARIO => {
                //6DEA37/6DEA67 and6DDD77/6DDD93/6DDDAF use PlayerPtr, not the
                //incoming Trigger House. The shared House result/timer
                //owner drives normal frame termination and ScenarioExit.
                use crate::sim::house_state::HouseOutcomeKind;
                use crate::sim::world::PlayerOutcomeRequest;
                let request = match action.kind {
                    1 | 2 => PlayerOutcomeRequest::Country {
                        country_index: value.unwrap_or(0),
                        win_on_match: action.kind == 1,
                    },
                    ACTION_ANNOUNCE_WIN | ACTION_ANNOUNCE_LOSE => PlayerOutcomeRequest::Flag {
                        kind: if action.kind == ACTION_ANNOUNCE_WIN {
                            HouseOutcomeKind::Victory
                        } else {
                            HouseOutcomeKind::Defeat
                        },
                        skip_timer: true,
                    },
                    ACTION_END_SCENARIO => PlayerOutcomeRequest::EndScenario,
                    _ => unreachable!("matched native outcome action"),
                };
                return self.request_player_outcome(request, rules, frame_effects);
            }
            // RESIDUAL (dormant, no retail map uses action 49 or 50): action 49, Zoom In (`0x006DEEB8`),
            // sets Tactical's zoom factor from Rules, locks input (0x00A8ED9C), hides the cursor,
            // cancels the drag (0x006938C0) and sleeps 1000 ms; action 50, Zoom Out (`0x006DEF35`),
            // puts the factor back to 1.0 and restores the lock and cursor. VERA has no handler.
            _ => return false,
        }
        true
    }
}
