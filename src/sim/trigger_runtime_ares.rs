//! Ares 3.0 trigger events and actions: TEventExt and TActionExt, absent
//! from default builds. The parent Tag/Trigger owner calls these first, as the
//! hooks at `0x71E949` (HasOccurred) and `0x6DD8D7` (Execute) run before the
//! stock switches; an unhandled kind keeps the stock body.
//!
//! The TEvent ExtData only caches the event's resolved TechnoType and saves
//! nothing (TEventClass_Save_Suffix 100502C0); the type is resolved again on
//! each read here. TActionExt has no state. The House state the events read
//! is owned elsewhere: HouseTracking's KeepAlive counts and the power owner's
//! AuxPower.

use super::{AresEventSource, RuntimeAction, TagEventInput, TriggerActionContext, TriggerEffect};
use crate::map::entities::EntityCategory;
use crate::map::events::EventCondition;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

impl Simulation {
    /// TEventExt::HasOccured (Ares.dll 1004F7A0, hook 0x71E949) for the kinds
    /// Ares handles: its replacements of 60/61 and its events 62..88. `None`
    /// keeps the stock body. Every handled event that does not match is
    /// false; none writes the persistence flag or the retained House, and
    /// none latches: Register7264C0 also asks the stock StateA71F950, which
    /// is false above 59, so Ares' Persistable answer (100504D0, hook
    /// 0x71F9C0) never reaches a latch.
    pub(super) fn ares_has_occurred(
        &self,
        condition: &EventCondition,
        input: TagEventInput,
        incoming_house: Option<InternedId>,
        rules: &RuleSet,
    ) -> Option<bool> {
        let kind = condition.kind;
        let value = condition.value;
        let raised = input.kind == kind;
        // pObject: the Techno the event is about.
        let actor = || input.actor.and_then(|id| self.substrate.entities.get(id));
        // pSource->Owner->ArrayIndex == arg, read now, without a Country or
        // Player@X lookup.
        let source_house_is_value = || {
            input
                .source
                .and_then(|id| self.substrate.entities.get(id))
                .and_then(|source| self.base_reservation_house_index(source.owner()))
                .is_some_and(|index| index == value)
        };
        // A House passed in the source slot: its ArrayIndex (+0x30).
        let source_house_index = || match input.ares_source {
            Some(AresEventSource::House(house)) => self.base_reservation_house_index(house),
            _ => None,
        };
        Some(match kind {
            // The stock TechTypeExists/DoesNotExist, replaced: at least
            // `value` of the type exist (60), or none (61).
            60 => self.ares_tech_type_exists(condition, value, None, rules),
            61 => !self.ares_tech_type_exists(condition, 1, None, rules),
            // Under EMP (62, 63 by House) or no longer (64, 65 by House):
            // 1004F7DC..870 on the raised Techno's current counter.
            62..=65 => {
                raised
                    && actor().is_some_and(|actor| {
                        (!matches!(kind, 63 | 65) || source_house_is_value())
                            && (actor.emp_remaining() > 0) == matches!(kind, 62 | 63)
                    })
            }
            // In a spotlight: 0x101 whatever was raised.
            66 => true,
            // Driver killed (67, 68 by House) and vehicle taken (69, 70 by
            // House): 1004F881..1004F91F.
            67..=70 => {
                raised && actor().is_some() && (matches!(kind, 67 | 69) || source_house_is_value())
            }
            // Abducted (71, 72 by a Techno of House) and abducts something
            // (73, 74 of a House passed as the source).
            71..=74 => {
                raised
                    && actor().is_some()
                    && match kind {
                        72 => source_house_is_value(),
                        74 => source_house_index() == Some(value),
                        _ => true,
                    }
            }
            // A Super whose type has index `value` was activated (75) or
            // deactivated (76).
            75 | 76 => {
                raised
                    && matches!(input.ares_source, Some(AresEventSource::Super(typ))
                        if rules.super_weapon_index(self.interner.resolve(typ))
                            == usize::try_from(value).ok())
            }
            // A Super of the named type fired at a Cell within 5 Cells of
            // waypoint `value`: strcmpi of the type ID, then
            // sqrt(dx*dx+dy*dy) <= 5.0 on the Cell deltas.
            77 => {
                let Some(AresEventSource::Launch { sw_type, cell }) = input.ares_source else {
                    return Some(false);
                };
                let named = condition.type_name.as_deref().is_some_and(|name| {
                    crate::rules::ini_value::truncate_native_bytes(
                        self.interner.resolve(sw_type),
                        24,
                    )
                    .eq_ignore_ascii_case(name)
                });
                raised
                    && named
                    && self.session.waypoint_cell(value).is_some_and(|waypoint| {
                        let dx = i64::from(cell.0) - i64::from(waypoint.0);
                        let dy = i64::from(cell.1) - i64::from(waypoint.1);
                        dx * dx + dy * dy <= 25
                    })
            }
            // The House has reverse engineered the type: HouseExt's
            // ReverseEngineered list, which nothing fills in VERA (Ares
            // reverse engineering is not ported), so it never holds it.
            78 => false,
            // Something (79) or the named type (80, the source's type) was
            // reverse engineered; nothing raises them in VERA.
            79 => raised,
            80 => {
                raised
                    && condition
                        .type_name
                        .as_deref()
                        .and_then(|name| rules.ares_find_techno_type(name))
                        .zip(input.source.and_then(|id| self.substrate.entities.get(id)))
                        .is_some_and(|(object, source)| {
                            source.category() == EntityCategory::from(object.category)
                                && self.interner.resolve(source.type_ref()) == object.id
                        })
            }
            // The incoming House owns at least `value` (81) or at most
            // `value` (82) of the named type.
            81 => self.ares_tech_type_exists(condition, value, incoming_house, rules),
            82 => {
                !self.ares_tech_type_exists(condition, value.wrapping_add(1), incoming_house, rules)
            }
            // Attacked or destroyed by anybody (83) or by House (84); the
            // object is not checked.
            83 => raised,
            84 => raised && source_house_is_value(),
            // Destroyed by the House passed as the source.
            85 => raised && source_house_index() == Some(value),
            // At most `value` of the named type exist.
            86 => !self.ares_tech_type_exists(condition, value.wrapping_add(1), None, rules),
            // House `value` (ResolveHouseParam) has no KeepAlive objects (87)
            // or KeepAlive Buildings (88) left. Native reads a missing House
            // through a null pointer; VERA answers false.
            87 | 88 => self
                .event_value_house(value, rules)
                .and_then(|house| self.houses.get(&house))
                .is_some_and(|house| {
                    let (all, buildings) = house.tracking().keep_alive_counts();
                    (if kind == 87 { all } else { buildings }) <= 0
                }),
            _ => return None,
        })
    }

    /// TEventExt::ExtData::TechTypeExists (Ares.dll 1004FE60): whether at
    /// least `count` objects of the event's type (TechnoTypeClass::Find
    /// 10007EA0 of its name) exist, owned by `house` or by anyone. An unknown
    /// type is false and a count below 1 true. Insignificant or DontScore
    /// types, which Houses do not track, are counted in their class's object
    /// array, Limbo included; others by each House's CountOwnedNow, in House
    /// array order.
    fn ares_tech_type_exists(
        &self,
        condition: &EventCondition,
        count: i32,
        house: Option<InternedId>,
        rules: &RuleSet,
    ) -> bool {
        let Some(object) = condition
            .type_name
            .as_deref()
            .and_then(|name| rules.ares_find_techno_type(name))
        else {
            return false;
        };
        if count < 1 {
            return true;
        }
        let category = EntityCategory::from(object.category);
        let Some(type_id) = self.interner.get(&object.id) else {
            return false;
        };
        if object.insignificant || object.dont_score {
            let found = self
                .substrate
                .entities
                .values()
                .filter(|entity| {
                    entity.category() == category
                        && entity.type_ref() == type_id
                        && house.is_none_or(|house| entity.owner() == house)
                })
                .take(count as usize)
                .count();
            return found >= count as usize;
        }
        let houses = match house {
            Some(house) => vec![house],
            None => self.houses.order().to_vec(),
        };
        let mut remaining = count;
        for house in houses {
            let owned = self
                .houses
                .get(&house)
                .map_or(0, |state| state.tracking().owned_count(category, type_id));
            remaining = remaining.wrapping_sub(owned);
            if remaining < 1 {
                return true;
            }
        }
        false
    }

    /// Tag6E53A0 on `subject`'s attached Tag, the subject being the event's
    /// object, for an Ares event whose source is not a Techno.
    pub(crate) fn dispatch_object_ares_event(
        &mut self,
        subject: u64,
        kind: i32,
        source: Option<AresEventSource>,
        context: UninitContext<'_>,
    ) -> bool {
        self.dispatch_object_tag_input(
            subject,
            TagEventInput {
                kind,
                actor: Some(subject),
                ares_source: source,
                ..Default::default()
            },
            context,
        )
    }

    /// RaiseOwnerTagEvent (inlined in SWTypeExt::Activate 10032020 and
    /// Deactivate 100325D0): `kind` with no object and the empty Cell on each
    /// of `owner`'s House Tags, first to last, rereading the list.
    pub(crate) fn raise_ares_super_event(
        &mut self,
        owner: InternedId,
        kind: i32,
        source: AresEventSource,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        self.raise_house_tags_forward(
            owner,
            TagEventInput {
                kind,
                ares_source: Some(source),
                ..Default::default()
            },
            rules,
            overlay_registry,
            frame_effects,
        );
    }

    /// TActionExt::Execute (Ares.dll 100396C0, hook 0x6DD8D7) for Ares' own
    /// actions 146..149 and the Firestorm actions 92/93; `None` keeps the
    /// stock switch. 98's replacement lives in its stock owner.
    pub(super) fn ares_execute_action(
        &mut self,
        action: &RuntimeAction,
        context: TriggerActionContext,
        effects: &mut Vec<TriggerEffect>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Option<bool> {
        let value = action.value;
        Some(match action.entry.kind {
            92 | 93 => {
                // ActivateFirestorm/DeactivateFirestorm: the trigger's House
                // raises or drops its firestorm (SetFirestormState 10022590)
                // unless it already is so; handled either way. Native reads
                // a null House; VERA refuses.
                let Some(house) = context.incoming_house else {
                    return Some(false);
                };
                self.ares_set_firestorm_state(
                    house,
                    action.entry.kind == 92,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
                true
            }
            146 => {
                // AuxPower: the House FindHouseByIndex6E45E0 selects by
                // Value gains Value2 (+0x48) power. Native writes through a
                // null House; VERA refuses.
                let Some(house) = context
                    .live_index
                    .zip(value)
                    .and_then(|(live, value)| self.live_action_house(live, value, rules))
                else {
                    return Some(false);
                };
                self.power_states
                    .house_mut(house)
                    .add_aux_power(action.entry.auxiliary_value());
                true
            }
            147 => self.ares_trigger_kill_drivers(
                context,
                value,
                rules,
                overlay_registry,
                frame_effects,
            ),
            148 => {
                // The EVA voice: VoxClass::EVAIndex = max(Value, -1) when
                // Value is below the voice count (3 + EVATypes). That
                // process cache belongs to the client's announcer.
                let Some(index) = value else {
                    return Some(false);
                };
                effects.push(TriggerEffect::EvaVoice {
                    index: index.max(-1),
                });
                true
            }
            149 => {
                // The triggering object's Group (TechnoClass+0x214) becomes
                // Value; Execute passes no object once it is dead.
                let Some(value) = value else {
                    return Some(false);
                };
                if let Some(entity) = context
                    .input
                    .actor
                    .and_then(|id| self.substrate.entities.get_mut(id))
                    .filter(|entity| entity.lifecycle.object_alive())
                {
                    entity.assign_object_group(value);
                }
                true
            }
            _ => return None,
        })
    }

    /// Action 147: kill the drivers of every Foot whose Tag holds this
    /// Trigger: alive, positive Health, placed and not in Limbo, its driver
    /// alive and CanKillDriver(1.0). They go to the House FindHouseByIndex
    /// selects by Value, else to `Special`. A Foot whose owner changed is
    /// visited again (the native index steps back), and the FootClass array
    /// pass repeats until no owner changes. True when any Foot qualified.
    fn ares_trigger_kill_drivers(
        &mut self,
        context: TriggerActionContext,
        value: Option<i32>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(live) = context.live_index else {
            return false;
        };
        let Some(owner) = value
            .and_then(|value| self.live_action_house(live, value, rules))
            .or_else(|| {
                self.house_kind(
                    crate::rules::ares_warhead::HouseSelector::Special,
                    None,
                    None,
                    None,
                    None,
                    rules,
                    None,
                )
            })
        else {
            return false;
        };
        let mut any = false;
        loop {
            let mut changed = false;
            let mut ordinal = 0;
            while let Some(id) = self.substrate.entities.foot_registry_at(ordinal) {
                let eligible = self.substrate.entities.get(id).is_some_and(|foot| {
                    foot.health.current() > 0
                        && foot.lifecycle.object_alive()
                        && foot.lifecycle.cell_marked()
                        && !foot.lifecycle.in_limbo()
                        && !foot.driver_killed()
                }) && self
                    .tag_contains_live_trigger(super::TagAttachment::Object(id), live)
                    && self.ares_can_kill_driver(
                        id,
                        crate::util::native_x87::NativeF64Bits::ONE,
                        rules,
                    );
                if eligible {
                    any = true;
                    if self.ares_replace_driver(
                        id,
                        None,
                        owner,
                        false,
                        rules,
                        overlay_registry,
                        frame_effects,
                    ) {
                        changed = true;
                        continue;
                    }
                }
                ordinal += 1;
            }
            if !changed {
                return any;
            }
        }
    }
}
