//! Phobos `AutoDeath`: `TechnoExt::CheckDeathConditions`
//! (`Ext/Techno/Body.Update.cpp:138-255`) and `TechnoExt::KillSelf`'s `Kill`
//! behavior (`:809-821`).
//!
//! ## Scope
//!
//! Ported arms, in native order: `ShouldBeDead` (`:159-163`),
//! `AutoDeath.Behavior=kill` with `OnAmmoDepletion` (`:166`), `AfterDelay`
//! (`:173-184`), `PlayerPowerState` (`:210-221`), `PlayerMoney.Max/Min`
//! (`:223-234`), `TechnosDontExist` (`:236-245`) and `TechnosExist`
//! (`:247-255`). `OnOwnerChange.*` sets the flag at the owner-change hook.
//!
//! The `Vanish`/`Sell` behaviors dispatch from the same triggers through
//! [`Simulation::self_death`] (`KillSelf` `:771-807`).
//!
//! A limboed object is reached by the scenario-level `AutoDeathObjects`
//! collection instead of the live object walk (`Ext/Scenario/Body.cpp:101-134`,
//! `293-301`): Ares' initial-payload admissions register it once
//! (`Misc/Hooks.Ares.cpp:31-57`) and the collection sweeps it every frame, with
//! `AllowLimboed` as the only way out. `KillSelf`'s limbo arm (`:721-766`) then
//! registers the kill, uninits, and settles the transport it was riding.
//!
//! Never depends on render/ui/audio/net (sim invariant #1).

use crate::rules::phobos_auto_death::{AutoDeathBehavior, AutoDeathPowerStatus};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{KillCallback, KillSource};
use crate::sim::timer::PAUSED_START_FRAME;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

impl Simulation {
    /// `TechnoExt::CheckDeathConditions`, called from
    /// `TechnoExt::OnEarlyUpdate` (`Ext/Techno/Body.Update.cpp:26`) after
    /// `ApplyMindControlRangeLimit` and before `:29` `ApplyInterceptor`.
    ///
    /// Native's `if (CheckDeathConditions()) return;` aborts `OnEarlyUpdate`, so
    /// the `ApplyInterceptor` on the next line never scans on a visit that just
    /// killed its object. Every arm that answers true (`:161`, `:168`, `:181`,
    /// `:218`, `:231`, `:241`, `:252` in `:138-259`) called `KillSelf`, so the
    /// skip only covers an object that is already gone, or the Building `Sell`
    /// arm whose `Selling` mission `ApplyInterceptor`'s own `:60-61` guard
    /// refuses anyway. Native likewise keeps running the rest of
    /// `TechnoClass_AI` on the object `ReceiveDamage` already erased, and
    /// VERA's later steps carry the same "entity gone" guards. The return value
    /// is that gate: `world/techno_ai.rs` skips `apply_interceptor` when it is
    /// true, and the limbo sweep (`Ext/Scenario/Body.cpp:101-110`) never runs
    /// the interceptor at all.
    #[cfg(feature = "phobos")]
    pub(crate) fn check_auto_death(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        // Only the scalar keys are copied: the type's presence lists are read
        // later, under a borrow that ends before any mutation.
        let (
            behavior,
            allow_limboed,
            in_limbo,
            after_delay,
            on_ammo_depletion,
            power_state,
            money_max,
            money_min,
            should_be_dead,
        ) = {
            let Some(entity) = self.substrate.entities.get(id) else {
                return false;
            };
            let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
                return false;
            };
            let settings = &object.phobos_auto_death;
            (
                settings.behavior,
                settings
                    .allow_limboed
                    .unwrap_or(rules.phobos_globals.auto_death_allow_limboed),
                entity.lifecycle.in_limbo(),
                settings.after_delay,
                settings.on_ammo_depletion,
                settings.player_power_state,
                settings.player_money_max,
                settings.player_money_min,
                entity.should_be_dead(),
            )
        };
        // `:142-143`: the type key is tested before the limbo gate.
        let Some(behavior) = behavior else {
            return false;
        };
        // `:147-153`: a limboed object is reached by the scenario-level
        // `AutoDeathObjects` sweep, and survives the visit unless this type (or
        // the `[CombatDamage]` global, which constructs `yes`) allows it.
        if in_limbo && !allow_limboed {
            return false;
        }
        // `:159-163`: a flag another hook set kills the object on this visit.
        if should_be_dead {
            return self.self_death(id, behavior, in_limbo, rules, registry, effects);
        }
        // `:165-170`: a type with an `Ammo=` capacity dies at zero or below.
        if on_ammo_depletion {
            let (capacity, current) = {
                let Some(entity) = self.substrate.entities.get(id) else {
                    return false;
                };
                (
                    self.interner
                        .object_type(entity.type_ref(), rules)
                        .map_or(0, |object| object.ammo),
                    entity.ammo().map_or(0, |ammo| ammo.current()),
                )
            };
            if capacity > 0 && current <= 0 {
                return self.self_death(id, behavior, in_limbo, rules, registry, effects);
            }
        }
        // `:172-184`: the timed arm. Phobos tests `HasStarted()` before
        // `Completed()`, which matters because an unstarted `CDTimerClass`
        // reads as already completed.
        if after_delay > 0 {
            if self.auto_death_clock_expires(id, after_delay) {
                return self.self_death(id, behavior, in_limbo, rules, registry, effects);
            }
        }
        // `:210-221`: the low-power predicate. Phobos writes
        // `(Full && !isLowPower) || (Low && isLowPower) && !isFirstFrame`; `&&`
        // binds tighter than `||`, so the first-frame guard applies to the
        // `Low` arm only and a `Full` type dies on frame 0 too. Reproduced as
        // written rather than as it reads.
        if power_state != AutoDeathPowerStatus::None {
            let low_power = self
                .power_states()
                .get(
                    &self
                        .substrate
                        .entities
                        .get(id)
                        .map_or(crate::sim::intern::InternedId::default(), |entity| {
                            entity.owner()
                        }),
                )
                .is_some_and(crate::sim::power_system::PowerState::is_low_power);
            let first_frame = self.session.binary_frame() == 0;
            let dies = (power_state == AutoDeathPowerStatus::Full && !low_power)
                || (power_state == AutoDeathPowerStatus::Low && low_power && !first_frame);
            if dies {
                return self.self_death(id, behavior, in_limbo, rules, registry, effects);
            }
        }
        // `:223-234`: `-1` on either bound means that bound is open.
        if money_max != -1 || money_min != -1 {
            let Some(owner) = self.substrate.entities.get(id).map(|entity| entity.owner()) else {
                return false;
            };
            let money = self
                .houses()
                .get(&owner)
                .map_or(0, |house| house.economy().available_money());
            let (max, min) = (money_max, money_min);
            if (max == -1 || money <= max) && (min == -1 || money >= min) {
                return self.self_death(id, behavior, in_limbo, rules, registry, effects);
            }
        }
        let Some(owner) = self.substrate.entities.get(id).map(|entity| entity.owner()) else {
            return false;
        };
        // `:236-255`: both presence scans, evaluated under one immutable borrow
        // so no type settings are cloned per visit.
        let kill = {
            let Some(entity) = self.substrate.entities.get(id) else {
                return false;
            };
            let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
                return false;
            };
            let settings = &object.phobos_auto_death;
            let dont = self.auto_death_resolve_technos(&settings.technos_dont_exist, rules);
            let dont_kills = !dont.is_empty()
                && !self.auto_death_technos_exist(
                    &dont,
                    settings.technos_dont_exist_houses,
                    !settings.technos_dont_exist_any,
                    settings.technos_dont_exist_allow_limboed.unwrap_or(
                        rules
                            .phobos_globals
                            .auto_death_technos_dont_exist_allow_limboed,
                    ),
                    owner,
                );
            if dont_kills {
                true
            } else {
                let exist = self.auto_death_resolve_technos(&settings.technos_exist, rules);
                !exist.is_empty()
                    && self.auto_death_technos_exist(
                        &exist,
                        settings.technos_exist_houses,
                        settings.technos_exist_any,
                        settings
                            .technos_exist_allow_limboed
                            .unwrap_or(rules.phobos_globals.auto_death_technos_exist_allow_limboed),
                        owner,
                    )
            }
        };
        if kill {
            return self.self_death(id, behavior, in_limbo, rules, registry, effects);
        }
        false
    }

    /// The native list is a `ValueableVector<TechnoTypeClass*>`: unknown tokens
    /// never reach it (`Utilities/TemplateDef.h:1727-1742`), so a typo leaves
    /// the arm's emptiness guard in charge rather than changing the decision.
    /// Tokens arrive untrimmed, as `strtok(",")` yields them.
    fn auto_death_resolve_technos(
        &self,
        list: &[String],
        rules: &RuleSet,
    ) -> Vec<(
        Option<crate::sim::intern::InternedId>,
        crate::map::entities::EntityCategory,
    )> {
        let mut resolved = Vec::new();
        for name in list {
            // The type registry is the authority, as native's `TechnoTypeClass::Find`
            // is; the interner only knows names this match has already used, so a
            // known type with no instance resolves to `None` and counts zero.
            let Some(object) = rules.object(name) else {
                continue;
            };
            let category = match object.category {
                crate::rules::object_type::ObjectCategory::Infantry => {
                    crate::map::entities::EntityCategory::Infantry
                }
                crate::rules::object_type::ObjectCategory::Vehicle => {
                    crate::map::entities::EntityCategory::Unit
                }
                crate::rules::object_type::ObjectCategory::Aircraft => {
                    crate::map::entities::EntityCategory::Aircraft
                }
                crate::rules::object_type::ObjectCategory::Building => {
                    crate::map::entities::EntityCategory::Structure
                }
            };
            resolved.push((self.interner.get(name), category));
        }
        resolved
    }

    /// `TechnoClass_Captured_UpdateTracking @ 0x7015C9`'s AutoDeath block
    /// (`Ext/House/Hooks.cpp:231-259`), run before that hook's tracking work.
    /// Sets `ShouldBeDead` from the owner-change direction keys. The
    /// `IgnoreRevertOnExit` arm (`:236-239`) and the transporter gate's
    /// revert term (`:253-258`) read the `FootExt::
    /// IsOwnerChangeFromRevertOnExit` latch the exit revert holds
    /// (`Ext/Techno/Hooks.Transport.cpp:115-117`).
    ///
    /// RESIDUAL: the AttachEffect `DiscardOn.OwnerChange.
    /// IgnoreRevertOnExit` chain also reads the latch and still sees
    /// `false` there.
    #[cfg(feature = "phobos")]
    pub(crate) fn check_auto_death_owner_change(
        &mut self,
        id: u64,
        new_owner: crate::sim::intern::InternedId,
        rules: &RuleSet,
    ) {
        let (
            master,
            to_computer,
            to_human,
            ignore_revert_key,
            allow_limboed,
            old_owner,
            has_transporter,
            revert_latch,
        ) = {
            let Some(entity) = self.substrate.entities.get(id) else {
                return;
            };
            let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
                return;
            };
            let settings = &object.phobos_auto_death;
            if settings.behavior.is_none() {
                return;
            }
            (
                settings.on_owner_change,
                settings.on_owner_change_human_to_computer,
                settings.on_owner_change_computer_to_human,
                settings.on_owner_change_ignore_revert_on_exit,
                settings.allow_limboed,
                entity.owner(),
                entity.passenger_role.transporter().is_some(),
                entity.phobos_revert_on_exit_latch(),
            )
        };
        // `HouseClass::IsControlledByHuman`, the same mode-aware predicate the
        // AttachEffect owner-change hook reads.
        let game_mode_nonzero = self.session.game_mode_nonzero();
        let old_human = self
            .houses()
            .get(&old_owner)
            .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
        let new_human = self
            .houses()
            .get(&new_owner)
            .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
        let to_computer = to_computer.unwrap_or(master);
        let to_human = to_human.unwrap_or(master);
        let mut should = false;
        if to_computer && to_human {
            should = true;
        } else if to_computer || to_human {
            if old_human != new_human && ((old_human && to_computer) || (!old_human && to_human)) {
                should = true;
            }
        }
        // Native `:236-239`: with `AutoDeath.OnOwnerChange.IgnoreRevertOnExit`
        // and the revert latch set, the arm assigns `ShouldBeDead = false`
        // and the direction arms do not run. Native `:253-258` then clears
        // `ShouldBeDead` on a transported object unless `AllowLimboed`
        // (default `yes`) or the latch is set.
        let already = self
            .substrate
            .entities
            .get(id)
            .is_some_and(crate::sim::game_entity::GameEntity::should_be_dead);
        let ignore_revert = revert_latch
            && ignore_revert_key.unwrap_or(
                rules
                    .phobos_globals
                    .auto_death_on_owner_change_ignore_revert_on_exit,
            );
        let mut value = if ignore_revert {
            false
        } else {
            already || should
        };
        if value
            && has_transporter
            && !revert_latch
            && !allow_limboed.unwrap_or(rules.phobos_globals.auto_death_allow_limboed)
        {
            value = false;
        }
        if value != already
            && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.set_should_be_dead(value);
        }
    }

    /// `Ext/Techno/Body.Update.cpp:188-208`: whether the listed types exist for
    /// the selected houses. `any` selects `any_of`; `all_of` otherwise. The
    /// non-limbo count is `HouseClass::CountOwnedAndPresent`; VERA reads the
    /// on-map `+0x5550` family (`active_count`). RESIDUAL: the wrapper's body
    /// was not read instruction-level, and `docs/phobos-harvester-counter.md`
    /// records `+0x5500` as the same query, so the counter identity is not
    /// proven here. `allow_limbo` adds the distinct limbo count.
    #[allow(clippy::too_many_arguments)]
    fn auto_death_technos_exist(
        &self,
        list: &[(
            Option<crate::sim::intern::InternedId>,
            crate::map::entities::EntityCategory,
        )],
        houses: crate::rules::phobos_shields::AffectedHouse,
        any: bool,
        allow_limbo: bool,
        owner: crate::sim::intern::InternedId,
    ) -> bool {
        let exist_single = |(type_id, category): &(
            Option<crate::sim::intern::InternedId>,
            crate::map::entities::EntityCategory,
        )| {
            let (type_id, category) = (*type_id, *category);
            // A known type this match has not interned yet has no instances.
            let Some(type_id) = type_id else {
                return false;
            };
            let count_in = |house: &crate::sim::house_state::HouseState| {
                if allow_limbo {
                    house
                        .tracking()
                        .count_owned_present_and_limboed(category, type_id)
                } else {
                    house.tracking().active_count(category, type_id)
                }
            };
            if houses == crate::rules::phobos_shields::AffectedHouse::OWNER {
                return self
                    .houses()
                    .get(&owner)
                    .is_some_and(|house| count_in(house) > 0);
            }
            self.houses().iter().any(|(house_id, house)| {
                crate::sim::combat::can_target_house(
                    houses,
                    owner,
                    *house_id,
                    self.houses(),
                    self.house_alliances(),
                    self.interner(),
                ) && count_in(house) > 0
            })
        };
        if any {
            list.iter().any(|name| exist_single(name))
        } else {
            list.iter().all(|name| exist_single(name))
        }
    }

    /// The `AfterDelay` arm's clock
    /// whether it has completed. `HasStarted()` is the `PAUSED_START_FRAME`
    /// test VERA uses everywhere else for an unstarted `CDTimerClass`.
    fn auto_death_clock_expires(&mut self, id: u64, after_delay: i32) -> bool {
        let frame = self.session.binary_frame() as i32;
        let Some(timer) = self
            .substrate
            .entities
            .get(id)
            .map(crate::sim::game_entity::GameEntity::auto_death_timer)
        else {
            return false;
        };
        // `:175-183`: start on the first visit, then kill once `Completed()`.
        // Phobos checks `HasStarted()` before `Completed()`, which matters
        // because an unstarted `CDTimerClass` reads as already completed.
        if timer.start_frame() == PAUSED_START_FRAME {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.auto_death_timer_mut().start(frame, after_delay);
            }
            return false;
        }
        timer.expired(frame)
    }

    /// `TechnoExt::KillSelf`'s dispatch (`Ext/Techno/Body.Update.cpp:719-823`)
    /// on the type's `AutoDeath.Behavior`. Every trigger calls it and reports
    /// the visit as handled, exactly as `CheckDeathConditions` returns true
    /// after any `KillSelf`. A limboed caller ignores the behavior: native
    /// tests `isInLimbo` first and returns from that arm (`:721-766`), so
    /// `kill`, `vanish` and `sell` all take the same limbo path there.
    fn self_death(
        &mut self,
        id: u64,
        behavior: AutoDeathBehavior,
        in_limbo: bool,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if in_limbo {
            return self.kill_self_in_limbo(id, rules, registry, effects);
        }
        match behavior {
            AutoDeathBehavior::Kill => self.kill_self(id, rules, registry, effects),
            AutoDeathBehavior::Vanish => self.vanish_self(id, rules, registry, effects),
            AutoDeathBehavior::Sell => self.sell_self(id, rules, registry, effects),
        }
    }

    /// `ScenarioExt::ExtData::RegisterAutoDeath` (`Ext/Scenario/Body.cpp:123-134`):
    /// add the Techno to `AutoDeathObjects` when its type sets
    /// `AutoDeath.Behavior=` and it is not already listed, in first-registration
    /// order. Native reaches this only from the three Ares initial-payload
    /// admissions (`Misc/Hooks.Ares.cpp:31-57`); ordinary boarding, a Chrono
    /// limbo and a sell/repair limbo never register.
    #[cfg(feature = "phobos")]
    pub(crate) fn register_auto_death(&mut self, id: u64, rules: &RuleSet) {
        let keyed = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| self.interner.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.phobos_auto_death.behavior.is_some());
        if keyed && !self.auto_death_objects.contains(&id) {
            self.auto_death_objects.push(id);
        }
    }

    /// `ScenarioExt::ExtData::UpdateAutoDeathObjectsInLimbo`
    /// (`Ext/Scenario/Body.cpp:101-110`, `DEFINE_HOOK(0x55B4E1,
    /// LogicClass_Update_BeforeAll, 0x5)` at `:293-301`): visit each registered
    /// Techno that is not in the live Logic vector but is still alive, with
    /// `isInLimbo = true`. The collection is copied first because a visit may
    /// uninit the object; dead or erased entries are dropped at the end, which
    /// native reaches through the same erase that removes it from the vector.
    #[cfg(feature = "phobos")]
    pub(crate) fn update_auto_death_objects_in_limbo(
        &mut self,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if self.auto_death_objects.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.auto_death_objects);
        for &id in &pending {
            // `!IsInLogic && IsAlive`: `Object+0x98` is the object's own
            // membership byte, which `ObjectClass::Conceal`
            // (`sim::world::lifecycle`) clears, and the visit itself may kill
            // the object.
            let visitable = self.substrate.entities.get(id).is_some_and(|entity| {
                entity.lifecycle.object_alive() && !entity.in_logic_vector.is_member()
            });
            if visitable {
                self.check_auto_death(id, rules, registry, effects);
            }
        }
        let mut survivors = pending;
        survivors.retain(|id| {
            self.substrate
                .entities
                .get(*id)
                .is_some_and(|entity| entity.lifecycle.object_alive())
        });
        std::mem::swap(&mut self.auto_death_objects, &mut survivors);
    }

    /// `TechnoExt::KillSelf`'s `Vanish` arm (`:771-790`): a random
    /// `AutoDeath.VanishAnimation` at the object's GetCoords with its owner
    /// house, then the bunker unload, `KillPassengers(pThis)` (the object is
    /// its own killer), `Stun`, `Limbo`, `RegisterKill(pThis->Owner)` and
    /// `UnInit`.
    ///
    /// `:773` is `AnimExt::CreateRandomAnim(pVanishAnimation,
    /// pThis->GetCoords(), pThis, nullptr, true)`: five arguments, so `invoker`
    /// is true and `ownedObject` keeps its `Ext/Anim/Body.h:122` default
    /// `false`. The animation records the dying object as
    /// `Invoker`/`InvokerHouse` and its house as owner house but gets no owner
    /// object, because `AnimClass::SetOwnerObject @ 0x00424B50` runs only under
    /// `if (ownedObject)` (`Ext/Anim/Body.cpp:420-421`). The `None` below is
    /// that absent owner object.
    fn vanish_self(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        let owner = entity.owner();
        let (anims, coords, structure_with_bunker) = {
            let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
                return true;
            };
            (
                object.phobos_auto_death.vanish_animation.clone(),
                crate::sim::movement::ground_pose::object_get_coords(
                    entity,
                    self.resolved_terrain.as_ref(),
                ),
                entity.category() == crate::map::entities::EntityCategory::Structure
                    && entity.bunker.occupant().is_some(),
            )
        };
        self.spawn_random_owned_anim(
            rules,
            &anims,
            crate::sim::anim_class::AnimWorldCoord {
                x: coords.x,
                y: coords.y,
                z: coords.z,
            },
            owner,
            None,
            Some(id),
        );
        if structure_with_bunker {
            // `BuildingClass::UnloadBunker @ 0x004593A0`: the sell/death
            // release, which does not reposition the unit or queue a Move.
            crate::sim::docking::bunker_link::release_sell_destroy(self, id, rules, effects);
        }
        self.kill_passengers(id, Some(id), context);
        self.stun(id, context);
        let _ = self.techno_limbo_with_context(id, context);
        self.record_the_kill(
            id,
            KillSource::House(Some(owner)),
            KillCallback::Terminal,
            context,
        );
        self.uninit_with_context(id, context);
        true
    }

    /// `TechnoExt::KillSelf`'s `Sell` arm (`:792-807`): a Building with a bound
    /// Buildup SHP (`+0x6E9`) takes the Selling mission unless it is already
    /// selling (`Queue_Mission(Selling, 0)` then Commence, the owner
    /// [`crate::sim::production::begin_selling`]); anything else falls through
    /// to the `Kill` arm, as the native developer warning does.
    fn sell_self(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let sellable = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.category() == crate::map::entities::EntityCategory::Structure
                && self
                    .interner
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| rules.has_buildup(&object.id))
        });
        if sellable {
            let selling = self.substrate.entities.get(id).is_some_and(|entity| {
                entity.mission.current().known() == Some(crate::sim::mission::MissionType::Selling)
            });
            if !selling {
                crate::sim::production::begin_selling(self, rules, id, false);
            }
            return true;
        }
        self.kill_self(id, rules, registry, effects)
    }

    /// `TechnoExt::KillSelf`'s non-limbo `Kill` arm (`:809-821`):
    /// `[CombatDamage] C4Warhead=` with the object's own Health, no attacker,
    /// no source house and ignoring defenses, through the shared self-kill
    /// owner. arg6 is `false`, unlike the locomotor self-destruct's `true`.
    fn kill_self(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        // RESIDUAL: Phobos calls Ares `SpawnSurvivors(pThis, nullptr, false,
        // false)` immediately before this hit `:810-816`, because the hit's
        // `ignoreDefenses` makes the death path's own survivor release refuse
        // (it marks the transport handled and ejects nobody). VERA's
        // `release_ares_vehicle_survivors` assumes the damage path has already
        // lifted the unit (`foot_mark_remove`), so calling it from here places
        // no survivor; trigger: `AutoDeath.Behavior=kill` on a `Crewed=` or
        // driver Unit/Aircraft in an Ares build; effect: the crew is lost where
        // native ejects it. Reproducing it needs the survivor owner taught the
        // pre-damage context, which is its own chain.
        self.receive_own_health_c4(id, rules, registry, effects, false);
        true
    }

    /// `TechnoExt::KillSelf`'s limbo arm (`Ext/Techno/Body.Update.cpp:721-766`),
    /// in native order. A limboed object plays nothing, is never stunned, limboed
    /// again or sold: the arm registers the kill under its own house, uninits,
    /// and settles the transport it was riding. Reached from
    /// [`Self::update_auto_death_objects_in_limbo`] and from any ordinary visit
    /// that `AutoDeath.AllowLimboed` lets through.
    fn kill_self_in_limbo(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        let owner = entity.owner();
        let category = entity.category();
        let transport = entity.passenger_role.transporter();
        // `Absorbed` is Techno `+0x439` (`set_infantry_absorber_entry`).
        let absorbed = entity.house_tracking_latches().is_absorbed();
        let type_ref = entity.type_ref();
        let counts_tracking = entity.tracking_facts.counts_toward_limbo_tracking();
        // `:723-728`: a parasite leaves its victim first, so the delete cannot
        // strand the host linked to a gone rider. `ExitUnit 0x0062A4A0`.
        if category != crate::map::entities::EntityCategory::Structure {
            self.parasite_exit_unit(id, rules, effects);
        }
        // `:730-737`: a limboed Building is not truly in limbo, so its
        // `AddToLimboTracking` entry leaves here — the same
        // `RemoveFromLimboTracking` owner `sim::superweapon::limbo` uses for a
        // delivered building. A `!Insignificant && !DontScore` type never
        // entered the count in the first place.
        if category == crate::map::entities::EntityCategory::Structure
            && counts_tracking
            && let Some(house) = self.houses.get_mut(&owner)
        {
            house
                .tracking_mut()
                .remove_limbo_delivered_tracking(category, type_ref);
        }
        // `:739-743`: an absorbed passenger's `ExtraPower=` leaves with it, so
        // the absorber's owner reassesses power (House `+0x5778`). An absorbed
        // Infantry carries no Cargo link (the absorber pulls it out of the
        // hold and counts it through house tracking), so the relation answers
        // only the object's own house in that case; a regular occupant dirties
        // its transport's owner.
        if absorbed {
            let power_owner = transport
                .and_then(|transport| {
                    self.substrate
                        .entities
                        .get(transport)
                        .map(crate::sim::game_entity::GameEntity::owner)
                })
                .unwrap_or(owner);
            Self::invalidate_house_power(&self.houses, &mut self.power_states, power_owner, false);
        }
        // `:745-746`: `RegisterKill(pThis->Owner)` then `UnInit`. The owner is
        // the source house, not an attacking Techno. UnInit drops the object
        // from its transport's cargo (`ObjectClass::Detach_All`) and kills its
        // own passengers, so the gunner step below reads the remaining cargo.
        self.record_the_kill(
            id,
            KillSource::House(Some(owner)),
            KillCallback::Terminal,
            context,
        );
        self.uninit_with_context(id, context);
        // `:749-763`: a `Gunner=` transport empties its gunner slot and hands it
        // to the last remaining passenger — native walks the cargo list to its
        // tail, unlike `Detonate.cpp`'s head. Both calls are the existing
        // owners (`UnitClass` `+0x4D4`/`+0x4D8`), which keep their own
        // Unit-only and `Gunner=` guards.
        let Some(transport) = transport else {
            return true;
        };
        let gunner_transport = self
            .substrate
            .entities
            .get(transport)
            .is_some_and(|entity| {
                entity.category() != crate::map::entities::EntityCategory::Structure
                    && self
                        .interner
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.gunner)
            });
        if !gunner_transport {
            return true;
        }
        self.remove_gunner(rules, transport, None);
        let last = self
            .substrate
            .entities
            .get(transport)
            .and_then(|entity| entity.passenger_role.cargo())
            .and_then(|cargo| cargo.passengers().last().copied());
        if let Some(pax_id) = last {
            crate::sim::passenger::receive_gunner(
                &self.interner,
                &self.session,
                &mut self.scenario_rng,
                &mut self.substrate,
                &self.houses,
                rules,
                transport,
                pax_id,
            );
        }
        true
    }
}

#[cfg(all(test, feature = "phobos"))]
#[path = "auto_death_tests.rs"]
mod tests;

#[cfg(all(test, feature = "phobos"))]
#[path = "auto_death_limbo_tests.rs"]
mod limbo_tests;
