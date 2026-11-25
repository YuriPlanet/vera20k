//! `Simulation` house state and ownership: self-heal grants, house add and
//! remove, derived house assessment, build-const lists and owner changes with
//! their docking and mission consequences.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::power_system;
use crate::sim::power_system::PowerStates;
use crate::sim::production;

use super::FrameEffects;
use super::LifecycleOutput;
#[cfg(any(test, feature = "test-support"))]
use super::LifecycleTestEvent;
use super::ObjectSubstrate;
use super::Simulation;
use super::UninitContext;
use super::enter_idle_mode;

impl Simulation {
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` for `owner`'s house;
    /// false for an owner without one.
    pub fn owner_is_human(&self, owner: InternedId) -> bool {
        self.houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.rule_policy()))
    }

    /// `BuildingClass::OnConstructionComplete`'s self-heal grant
    /// (`0x00446382..0x004463B4`): a structure joins its owner's house with the
    /// type's `InfantryGainSelfHeal` (`+0x1564` → house `+0x164`) and
    /// `UnitsGainSelfHeal` (`+0x1568` → house `+0x168`). Each arm skips its add
    /// when the type's count is zero; both use a plain wrapping `add`.
    pub(crate) fn grant_house_self_heal(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((owner, infantry, units)) = self.structure_self_heal_gain(stable_id, rules) else {
            return;
        };
        if infantry == 0 && units == 0 {
            return;
        }
        if let Some(house) = self.houses.get_mut(&owner) {
            house.grant_self_heal(infantry, units);
        }
    }

    /// `BuildingClass::Limbo`'s self-heal share (`0x004459AE..0x004459CA`, the
    /// unit arm at `0x004459E0`): subtract the type's counts from the owner's
    /// house, then raise a negative count to zero.
    pub(crate) fn remove_house_self_heal(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((owner, infantry, units)) = self.structure_self_heal_gain(stable_id, rules) else {
            return;
        };
        if let Some(house) = self.houses.get_mut(&owner) {
            house.revoke_self_heal(infantry, units);
        }
    }

    /// `BuildingClass::ChangeOwner`'s self-heal share
    /// (`0x00448AC8..0x00448B04` for infantry, `0x00448B0A..0x00448B46` for
    /// units): **the old owner loses the type's counts**, and a negative result
    /// is raised to zero. Both arms first test the type's count and the
    /// building's own `+0x6E4` "actually placed on the map" byte.
    ///
    /// There is no new-owner add in that block. Natively the new house gains the
    /// counts in `ChangeOwner`'s tail (`0x00449201..0x00449223`, gated on the
    /// type's counts only). The virtual `Grand_Opening(1)` it calls at
    /// `0x00448CEF` jumps past its own add (`0x00446382..0x004463B4`) when
    /// `+0x6E4` is set (`0x00445FD0`). VERA makes the new owner's add in
    /// `grand_opening` instead, which nets the same for a placed building.
    /// RESIDUAL: a capture with `+0x6E4` clear gains the counts twice natively
    /// (opening and tail) and once in VERA. Unreachable in stock skirmish: the
    /// self-heal types are map tech buildings, opened at placement.
    ///
    /// So this function must not credit anyone, and its caller must run it
    /// with the *passed* old owner: `TechnoClass::ChangeOwner` has already
    /// written the new owner by then, which is why an earlier version that read
    /// the entity's current owner back never took the counters off the house
    /// that lost the building.
    pub(crate) fn remove_old_owner_self_heal(
        &mut self,
        stable_id: u64,
        old_owner: InternedId,
        rules: &RuleSet,
    ) {
        let Some((_, infantry, units)) = self.structure_self_heal_gain(stable_id, rules) else {
            return;
        };
        let placed = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.building_art.actually_placed());
        if !placed || (infantry == 0 && units == 0) {
            return;
        }
        if let Some(house) = self.houses.get_mut(&old_owner) {
            house.revoke_self_heal(infantry, units);
        }
    }

    /// Owner, `InfantryGainSelfHeal` and `UnitsGainSelfHeal` of a structure,
    /// or `None` for a missing entity or a non-structure.
    fn structure_self_heal_gain(
        &self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> Option<(InternedId, i32, i32)> {
        let entity = self.substrate.entities.get(stable_id)?;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        let object = self.object_type(entity.type_ref(), rules)?;
        Some((
            entity.owner(),
            object.infantry_gain_self_heal,
            object.units_gain_self_heal,
        ))
    }

    /// `HouseClass::Added_To_Game @ 0x00502A80` for an object on its owner's
    /// house, priced with that house's current `Cost_Of` factors. It counts
    /// no powered center and invalidates no power; the first GrandOpening
    /// owns that invalidation.
    ///
    /// Building registration502B48..502B52 also adds GetPowerDrain44E880
    /// immediately via5018C0. Earlier Houses may query the retained totals
    /// before this House's first assessment (SOV01's Event58, for example).
    pub(crate) fn house_added_to_game(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if let Some(house) = self.houses.get_mut(&entity.owner()) {
            house.add_to_game_tracking(entity);
            if entity.category() == EntityCategory::Structure {
                let drain = power_system::building_power_drain(entity, rules, &self.interner);
                self.power_states
                    .house_mut(entity.owner())
                    .add_building_drain(drain);
            }
        }
        // When the money takes its Ares form (`RuleSet::ares_form`, the one
        // switch of Ares' stores): the building's store joins the House's
        // (`0x00502B83`, `sim::ares_tiberium_storage`).
        if rules.ares_form() {
            self.ares_storage_added_to_game(stable_id, rules);
        }
    }

    /// `HouseClass::Removed_From_Game @ 0x005025F0` for an object on its
    /// owner's house, priced like [`Self::house_added_to_game`]. Between its
    /// sidebar update and its class switch, a counted `PowersUnit=` building
    /// stops counting (`0x0050263B..0x00502776`,
    /// [`Self::powered_center_removed`]). The prefix counters precede that
    /// callback; class/type/capacity reads follow it on the same receiving House.
    /// A Building invalidates its house's power regardless of
    /// DontScore (`0x005027FA`).
    pub(crate) fn house_removed_from_game(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let owner = entity.owner();
        let structure = entity.category() == EntityCategory::Structure;
        if let Some(house) = self.houses.get_mut(&owner) {
            house.remove_from_game_tracking_prefix(entity);
        }
        self.powered_center_removed(stable_id, rules, effects);
        //50277B resumes after the callback prelude. Pin the original House
        // receiver, but reborrow the live object/type at this actual phase.
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && let Some(house) = self.houses.get_mut(&owner)
        {
            house.remove_from_game_tracking(entity);
        }
        // When the money takes its Ares form (`RuleSet::ares_form`): the
        // building's store leaves the House's (`0x00502801..0x00502821`,
        // `sim::ares_tiberium_storage`).
        if rules.ares_form() && structure {
            self.ares_storage_removed_from_game(stable_id, owner);
        }
        if structure {
            Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
        }
    }

    /// Private retained House dirty bytes, shared by their represented native
    /// writers. An absent House cannot acquire derived state from a callback.
    pub(crate) fn invalidate_house_power(
        houses: &Houses,
        power_states: &mut PowerStates,
        owner: InternedId,
        radar: bool,
    ) {
        if houses.contains_key(&owner) {
            power_states.house_mut(owner).invalidate(radar);
        }
    }

    /// BuildingAI440042..44006C samples live health before the common Techno
    /// tail. Repair701410 itself leaves both House dirty bytes untouched.
    pub(in crate::sim) fn sample_building_health_for_house_update(
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        power_states: &mut PowerStates,
        id: u64,
    ) {
        let owner = substrate
            .entities
            .get_mut_if(
                id,
                crate::sim::game_entity::GameEntity::building_health_sample_changes,
            )
            .and_then(|entity| {
                entity
                    .sample_building_health_for_power()
                    .then_some(entity.owner())
            });
        if let Some(owner) = owner {
            Self::invalidate_house_power(houses, power_states, owner, true);
        }
    }

    /// HouseClass's power (AI_AssessPower508CBD..508CF0), radar
    /// (UpdateTacticalRadarAvailability508ECD..508EF1) and SpySat
    /// (Update_SpySat508FA2..508FDF) scans share this discovery admission.
    /// Source: original gamemd instructions. Noncampaign scans admit hidden
    /// providers, including the current player's; campaign computer Houses
    /// do too. Only controlled campaign Houses require Techno+41B.
    /// Lockstep service decisions cannot use a process's local +41B projection.
    pub(super) fn building_services_require_discovery(&self, house: &HouseState) -> bool {
        self.session.uses_local_player_shroud()
            && house.is_controlled_by_human(self.session.rule_policy())
    }

    /// House4F844B..4F850C: assess dirty power, rewrite all owned Factory
    /// rates without touching timers, then refresh radar and SpySat.
    /// Numeric/order evidence: the engineer-repair native consumer packet.
    pub(super) fn assess_house_derived_state(
        &mut self,
        owner: InternedId,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if !self.houses.contains_key(&owner) {
            return;
        }
        let require_discovery = self.building_services_require_discovery(&self.houses[&owner]);
        #[cfg(feature = "debug-tools")]
        let debug_full_power = self.debug_flags_for(owner).infinite_power;
        let state = self.power_states.house_mut(owner);
        #[cfg(feature = "debug-tools")]
        state.set_debug_full_power(debug_full_power);
        #[cfg(feature = "debug-tools")]
        let debug_power_changed = state.take_debug_power_changed();
        let was_low_power = state.low_power();
        let assessed = power_system::assess_house_power(
            state,
            &self.substrate.entities,
            self.houses[&owner].base_projection().buildings(),
            rules,
            &self.interner,
            self.session.binary_frame(),
            require_discovery,
        );
        if assessed {
            production::refresh_factory_rates_for_house(self, rules, owner);
            // Power508DD5 compares the old/current ratio;508DDB invokes the
            // shared hold/revoke before the radar tail, only on that edge.
            let power_changed = self.power_states[&owner].low_power() != was_low_power;
            #[cfg(feature = "debug-tools")]
            let power_changed = power_changed || debug_power_changed;
            if power_changed {
                crate::sim::superweapon::update_owned_supers(self, rules, owner, effects);
            }
        }
        // House4F84F1..4F850C admits radar THEN SpySat using the same prior
        // +5779. Radar clears it before SpySat, whose Map callbacks may arm
        // it again. A silent Techno70D990 promotion never arms this byte.
        if !self.power_states[&owner].radar_recheck_pending() {
            return;
        }
        // HouseUpdate4F84FB suppresses the local tactical radar update after
        // PlayerPtr's defeat. Our retained radar is per House: in LAN/WOL only
        // that House's own defeat keeps its projection on, independently of
        // which human is this process's input player. Offline keeps native's
        // process-local gate. Source: original gamemd.exe4F84F1..4F8511.
        let viewer_defeated = if self.session.execution_topology().is_lockstep() {
            self.houses
                .get(&owner)
                .is_some_and(|house| house.is_defeated())
        } else {
            self.local_player_defeated()
        };
        if viewer_defeated {
            self.power_states
                .get_mut(&owner)
                .expect("represented House power state")
                .skip_radar_recheck();
            self.assess_house_spy_sat(owner, rules);
            return;
        }
        let buildings = self.houses[&owner].base_projection().buildings();
        let radar_assessed = power_system::assess_house_radar_projection(
            self.power_states
                .get_mut(&owner)
                .expect("represented House power state"),
            &self.substrate.entities,
            buildings,
            rules,
            owner,
            &self.interner,
            self.session.free_radar(),
            self.session.binary_frame(),
            require_discovery,
        );
        // House508F39..508F4F compares the actual process Radar flag and
        // calls SetAvailable656DF0 here, before this House's defeat gate.
        if radar_assessed && self.session.current_house() == Some(owner) {
            effects.set_tactical_radar_available(
                self,
                rules,
                power_system::has_active_radar(self.power_states.as_map(), owner),
            );
        }
        self.assess_house_spy_sat(owner, rules);
    }

    /// Append one successfully committed BuildConst Building to its owning
    /// House's native acquisition-ordered vector.
    ///
    /// gamemd-derived: `BuildingClass::Unlimbo @ 0x004411B6..0x00441223`.
    pub(crate) fn append_live_build_const(&mut self, stable_id: u64) {
        let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
            (entity.category() == EntityCategory::Structure
                && entity.seldom.type_facts.build_const_eligible()
                && entity.lifecycle.object_alive()
                && !entity.lifecycle.in_limbo()
                && entity.lifecycle.cell_marked())
            .then_some(entity.owner())
        }) else {
            return;
        };
        let _appended = self
            .houses
            .get_mut(&owner)
            .is_some_and(|house| house.append_build_const(stable_id));
        #[cfg(any(test, feature = "test-support"))]
        if _appended {
            self.trace_lifecycle_for_test(LifecycleTestEvent::BuildConstAppended { stable_id });
        }
    }

    /// Stable-remove a Building pointer from the owning House's BuildConst
    /// vector before Limbo or owner transfer makes it unavailable.
    ///
    /// gamemd-derived: House pointer expiry around `0x004FBA1D` and
    /// `BuildingClass::ChangeOwner @ 0x00448260`.
    pub(crate) fn remove_build_const_from_owner(
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        stable_id: u64,
    ) {
        let Some((owner, eligible)) = substrate.entities.get(stable_id).map(|entity| {
            (
                entity.owner(),
                entity.seldom.type_facts.build_const_eligible(),
            )
        }) else {
            return;
        };
        if eligible && let Some(house) = houses.get_mut(&owner) {
            house.remove_build_const(stable_id);
        }
    }

    /// Change an entity's owner through the authoritative ownership chokepoint.
    /// House counts, the `by_owner` index, and the entity owner move exactly once
    /// for every live transfer, regardless of whether capture or garrison code
    /// requested it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn change_owner(&mut self, stable_id: u64, new_owner: InternedId) {
        self.change_owner_impl(
            stable_id,
            new_owner,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            &mut FrameEffects::default(),
            false,
        );
    }

    /// Original House502D30: first matching registration, not owner-name order.
    /// Country identity remains RuleSet-owned; duplicate-Country Houses are valid.
    pub(crate) fn house_by_country_index(
        &self,
        country: crate::rules::ruleset::CountryIdx,
        rules: &RuleSet,
    ) -> Option<InternedId> {
        // VERA-only: a House is of `country` when its Country's name is the
        // one `RuleSet::country_index` answers `country` for, so the name is
        // asked of the rules once, not once per House. The index keeps the
        // first Country of an identity: a later one of the same identity is
        // no name's answer, and no House's.
        let identity = rules.country_name(country)?;
        if rules.country_index(identity) != Some(country) {
            return None;
        }
        let found = self.houses.order().iter().copied().find(|owner| {
            self.houses
                .get(owner)
                .and_then(|house| house.country())
                .is_some_and(|id| self.interner.resolve(id).eq_ignore_ascii_case(identity))
        });
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || found
                    == self.houses.order().iter().copied().find(|owner| {
                        self.houses
                            .get(owner)
                            .and_then(|house| house.country())
                            .and_then(|id| rules.country_index(self.interner.resolve(id)))
                            == Some(country)
                    }),
            "the House of {identity:?} differs from the one its Country index finds"
        );
        found
    }

    /// Original House510F60 -> Scenario510ED0, otherwise502D30. Only the
    /// player selectors use the start-slot mapping; VERA extends A..H to
    /// A..AD for thirty players, with the same session-owned mapping.
    /// Executed Steam gamemd controls: trigger_all_change_house.json.
    pub(crate) fn house_by_trigger_selector(
        &self,
        selector: i32,
        rules: &RuleSet,
    ) -> Option<InternedId> {
        let players = crate::rules::native_processing::HOUSE_TYPE_PLAYER_AT_SELECTORS;
        if players.contains(&selector) {
            self.session
                .start_slot_houses()
                .get(&((selector - players.start) as u32))
                .copied()
        } else {
            let country = u16::try_from(selector).ok()?;
            self.house_by_country_index(crate::rules::ruleset::CountryIdx(country), rules)
        }
    }

    pub(crate) fn change_owner_with_rules(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        self.change_owner_impl(stable_id, new_owner, rules, registry, effects, false);
    }

    /// [`Self::change_owner_with_rules`] for a mind control taking or
    /// releasing the object: CaptureManager's Capture (`0x00471DB8`) and
    /// FreeUnit (`0x004720D5`), PsyDom's permanent capture, Ares'
    /// ApplyPermaMC and Dominator transfer. The transfer is the same; only
    /// the Wasm module `HouseTransferred` record says why (`value` 1).
    pub(crate) fn change_owner_by_mind_control(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        self.change_owner_impl(stable_id, new_owner, rules, registry, effects, true);
    }

    fn change_owner_impl(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
        mind_control: bool,
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if entity.category() != EntityCategory::Unit {
            self.change_owner_inherited(
                stable_id,
                new_owner,
                rules,
                registry,
                effects,
                mind_control,
            );
            return;
        }
        // UnitClass::ChangeOwner7463A0..7463F2 (original gamemd instructions)
        // clears its independent follower byte, recursively transfers its
        // retained peer first, restores that link/peer byte, then calls Foot.
        // An explicit continuation stack preserves this order for long chains
        // without consuming one Rust stack frame per linked Unit.
        let mut pending = Vec::new();
        let mut visited = std::collections::BTreeSet::new();
        let mut current = Some(stable_id);
        while let Some(id) = current {
            let Some(entity) = self.substrate.entities.get(id) else {
                log::warn!("UnitChangeOwner7463A0 references missing follower {id}");
                return;
            };
            if entity.category() != EntityCategory::Unit {
                log::warn!("UnitChangeOwner7463A0 references non-Unit follower {id}");
                return;
            }
            if entity.owner() == new_owner {
                break;
            }
            if !visited.insert(id) {
                // Native recursion has no completed result for this graph.
                // Retain it in storage, but diagnose this unsupported call.
                log::warn!("UnitChangeOwner7463A0 has a cyclic follower chain at {id}");
                return;
            }
            current = entity.unit_follower();
            pending.push((id, current));
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .set_is_unit_follower(false);
        }
        for (id, peer) in pending.into_iter().rev() {
            if !self.substrate.entities.contains(id)
                || peer.is_some_and(|peer| !self.substrate.entities.contains(peer))
            {
                // A native retained pointer would now be dangling. Do not
                // resurrect a retired object or invent its callback result.
                log::warn!("UnitChangeOwner7463A0 receiver/follower retired during callbacks");
                return;
            }
            if let Some(peer) = peer {
                self.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .set_unit_follower(Some(peer));
                self.substrate
                    .entities
                    .get_mut(peer)
                    .unwrap()
                    .set_is_unit_follower(true);
            }
            self.change_owner_inherited(id, new_owner, rules, registry, effects, mind_control);
        }
    }

    /// The existing inherited and Building ownership authority. Unit's
    /// subclass prefix enters here only after its follower callbacks finish.
    fn change_owner_inherited(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
        mind_control: bool,
    ) {
        let Some((old_owner, category, has_spawn_manager, build_const_eligible)) =
            self.substrate.entities.get(stable_id).map(|entity| {
                (
                    entity.owner(),
                    entity.category(),
                    entity.spawn_manager.is_some(),
                    entity.seldom.type_facts.build_const_eligible(),
                )
            })
        else {
            return;
        };
        crate::sim::ra2_all02_diagnostic::on_owner_change(
            self,
            stable_id,
            old_owner,
            new_owner,
            rules,
            "BEFORE",
        );
        // FootChangeOwner4DBEFB removes at the current cell before its
        // Techno delegate, including the same-owner refusal at7014A0.
        self.remove_unit_sensors_at_current(stable_id, rules, registry);
        if old_owner == new_owner {
            // Foot4DBFB7 restores the current owner's circle on failure;
            // it does not replace LastMapCoords in this arm.
            self.add_unit_sensors_at_current(stable_id, rules, registry);
            return;
        }
        // `BuildingClass::ChangeOwner` first (`0x00448277..0x0044828E`): a
        // building changing hands loses its bomb unless it is
        // `CanBeOccupied=`.
        // Ares hook448277 (100163B0) first kicks out a Hospital's or
        // Armory's first passenger (BuildingExt::KickOutHospitalArmory
        // 10012920), with the old owner still in place.
        if rules.ares() && category == EntityCategory::Structure {
            self.ares_kick_out_hospital_armory(stable_id, rules, registry, effects);
        }
        // It then takes the building out of its Prism network unless it is
        // a `ToAllies` supporter whose network stays allied, and then
        // forgets its radar jammers.
        if rules.ares()
            && category == EntityCategory::Structure
            && self.ares_prism_change_owner(stable_id, new_owner, rules)
        {
            self.clear_registered_jammers(stable_id);
        }
        // Ares BuildingClass_ChangeOwner_PrismForwardAndLeaveBomb (100163B0)
        // always resumes at 0x448293, past the defuse: a captured building
        // keeps its bomb.
        if !rules.ares()
            && category == EntityCategory::Structure
            && let Some(entity) = self.substrate.entities.get(stable_id)
            && !self
                .interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.can_be_occupied)
        {
            Self::bomb_defuse(
                &mut self.substrate,
                &mut self.bombs,
                &mut self.sound_events,
                stable_id,
            );
        }
        // Ares ChangeOwnership_SensorArrayA10016CA0 (hook448B70) removes an
        // active sensor array for the old owner before the gap removal.
        if rules.ares() && category == EntityCategory::Structure {
            self.ares_sensor_array_dispatch(stable_id, false, rules, registry);
        }
        if category == EntityCategory::Structure {
            //448260: gap removal precedes ordinary sight release and owner
            //swap. This order differs from Techno Limbo's sight-then-gap.
            // `ReReveal(0, 0, 0, 0)` (`0x00448BD9`) releases the old owner's
            // sight only: what a `RevealToAll=` opening showed the current
            // house stays.
            Self::remove_building_gap(
                &self.interner,
                &mut self.fog,
                &self.house_alliances,
                &mut self.substrate,
                &self.houses,
                &self.resolved_terrain,
                stable_id,
            );
            self.release_object_sight_with_context(stable_id, UninitContext::new(rules));
            // Original4483A6 clears PrimaryFactory before the old-house
            //teardown.448C69 initializes it after the delegated owner swap.
            self.production.clear_primary_factory(stable_id);
        }
        // Active YR chain: BuildingClass::ChangeOwner (0x00448260) delegates
        // to TechnoClass::ChangeOwner (0x007014A0), which moves the house
        // counts (`house_tracking`) below.

        // `BuildingClass::ChangeOwner @ 0x004482AA..0x004482F9`, still on the
        // OLD owner: a `MultiplayPassive` old owner and a non-zero
        // `ProduceCashStartup` credit the NEW owner and arm the ProduceCash
        // timer (oil derricks). Runs ahead of the Techno owner swap like the
        // native, before the BuildConst/count moves below.
        crate::sim::credit_income::produce_cash_on_owner_change(
            &self.interner,
            &self.session,
            &mut self.substrate,
            &mut self.houses,
            stable_id,
            old_owner,
            new_owner,
            rules,
        );
        // Ares 3.0 10015A20 and 10015A60 (hooks 0x00448312, 0x00448D95,
        // before and after the owner swap): the new owner's spy bit goes
        // (`sim::spy_infiltration::DisplayProductionTo`); nothing re-adds it
        // between the two.
        if rules.ares()
            && let Some(building) = self.substrate.entities.get_mut(stable_id)
        {
            building.seldom.ares.display_production_to.remove(new_owner);
        }
        // Building4484AF precedes the delegated Techno owner transfer.
        self.enable_building_after_owner_change(stable_id, rules);
        // `0x00448559..0x004486C8`: the objects docked at a building go along
        // with it or are sent off.
        let taken_along = if category == EntityCategory::Structure {
            self.take_docked_along_on_owner_change(stable_id, new_owner, rules, registry, effects)
        } else {
            Vec::new()
        };
        // `0x004486DF..0x00448701`: the building's own factory is abandoned
        // for its old owner.
        if category == EntityCategory::Structure {
            crate::sim::production::detach_building_factory(self, rules, stable_id, effects);
        }
        // gamemd-derived: `BuildingClass::ChangeOwner @ 0x00448260` removes
        // this pointer from the old House BuildConst vector before delegating
        // the Techno owner swap, then appends it to the new House tail.
        if build_const_eligible {
            Self::remove_build_const_from_owner(&self.substrate, &mut self.houses, stable_id);
        }
        if category == EntityCategory::Structure {
            self.houses.leave_house_base_lists(stable_id, old_owner);
            // Ares BuildingClass_ChangeOwnership_Remove_Academy (hook
            // 0x00448AB2 -> 10023690) follows the House+68 removal.
            if rules.ares() {
                self.update_ares_academy_membership(stable_id, false, rules);
            }
        }
        // TechnoClass::ChangeOwner (`0x007014B9..0x007014CF`): the local
        // player's selected object is deselected first (vt+0x150, every
        // Techno class's `ObjectClass::Deselect @ 0x005F44A0`).
        if self.session.current_house() == Some(old_owner) {
            self.object_deselect(stable_id);
        }
        // Ares7014D5 ->10049FB0 unjams the object's radar jammer, then
        //10049FD7 displays the old owner's queued sum before
        // target/destination callbacks and the actual owner store.
        if rules.ares() {
            self.unjam_all(stable_id);
            self.flush_bounty_display(stable_id, rules, effects, true);
        }
        // Original7014DB/7014E9 dispatch the class target and destination
        // setters, then70151A clears Archive and70156E queues Guard. All run
        // on the OLD house, before SpawnManager and the701735 owner swap.
        // The late70182F/70183B setters remain below on the new house.
        use crate::sim::mission::{MissionId, MissionType};
        let entity = self.substrate.entities.get(stable_id).unwrap();
        let current = entity.mission.current().known();
        let unit_deploying = category == EntityCategory::Unit
            && entity.mission_leaf.as_unit().is_some_and(|leaf| {
                leaf.deploy_begin_active() != 0 || leaf.deploy_reverse_active() != 0
            });
        let simple_deployer_unloading = category == EntityCategory::Unit
            && current == Some(MissionType::Unload)
            && self
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.is_simple_deployer);
        let _ = self.assign_target_represented(stable_id, None, rules, effects);
        self.assign_null_destination(stable_id, rules, Some(rules.overlay_registry()), effects);
        if !unit_deploying && let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.set_archive_target(None);
        }
        // VERA deviation from gamemd (#3593): native queues this Guard on a
        // war factory still letting its product out, too. The building is
        // ready at once (`+0x6DD`), so Guard replaces Unload: a product
        // captured before the door opens never leaves the bay, and one already
        // driving out leaves the door open behind it. Keeping Unload lets the
        // product out for the new owner and closes the door; the `0x007017CE`
        // check in `change_owner_mission_half` already skips the idle reset.
        // RESIDUAL: in the one frame between `exit_produced_object` and the
        // Unload commit (Unload only queued), a capture still strands the
        // product as in gamemd: the `0x007017CE` tail reads the current
        // mission only. At most once per product.
        let factory_unloading = self.weapons_factory_unloading(stable_id, rules);
        if current != Some(MissionType::Selling) && !simple_deployer_unloading && !factory_unloading
        {
            // `Queue_Mission(Guard, 1)`: the queue write, then Ready_To_Commence
            // (`+0x200`) and Commence (`+0x1EC`).
            let _ = self.mission_queue_exact(
                stable_id,
                MissionId::from_known(MissionType::Guard),
                1,
                self.session.binary_frame(),
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
        }

        // `TechnoClass::ChangeOwner` calls `SpawnManagerClass::Kill_All_Spawns`
        // before the house swap: a mind-controlled V3/Dreadnought/Boomer loses
        // the pool it built for its old owner, and a Carrier's airborne
        // Hornets crash. The children are destroyed while still
        // attributed to the previous house. The owner is still alive here, so
        // the slots re-arm with a zero regen wait and the new owner's pool is
        // rebuilt on the next manager pass.
        if has_spawn_manager {
            crate::sim::spawn_manager::kill_all_spawns_with_context(
                self,
                stable_id,
                UninitContext::new(rules).with_effects(effects),
            );
        }
        // Techno701585 -> Planning638840 removes this actor's path before
        // the house swap. Already generated command events remain queued.
        self.planning_change_owner(stable_id);
        // Techno70158A..7015E6: Removed_From_Game on the old house (not in
        // limbo), then Remove_Tracking from it and Add_Tracking to the new.
        let on_map = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo());
        if on_map {
            self.house_removed_from_game(stable_id, rules, effects);
        }
        //7015A8 calls RecordTheKill(NULL) while the old owner is still
        //installed, then7015D2 adds CostOf(oldHouse) to the new owner's
        //score even when DontScore suppressed RecordTheKill itself.
        self.record_the_kill(
            stable_id,
            crate::sim::combat::KillSource::Techno(None),
            crate::sim::combat::KillCallback::OwnerChange,
            UninitContext::new(rules).with_effects(effects),
        );
        let cost = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .map_or(0, |object| self.cost_of(old_owner, object, rules));
        if let Some(house) = self.houses.get_mut(&new_owner) {
            house.economy_mut().add_score(cost);
        }
        #[cfg(feature = "phobos")]
        self.phobos_tracking_change_owner(stable_id, new_owner);
        // 7015E6 adds the object to its new House before the actor's owner store.
        HouseTracking::transfer_registration(
            &mut self.substrate,
            &mut self.houses,
            stable_id,
            new_owner,
        );
        crate::sim::ra2_all02_diagnostic::on_owner_change(
            self,
            stable_id,
            old_owner,
            new_owner,
            rules,
            "AFTER",
        );
        // Ares3.0 ChangeOwner7015EB ->1004E7F0 follows the two tracking
        // calls and precedes both the capture stats and owner pointer store.
        // Buildings retain their own existing44936E/449379 recheck tail.
        if rules.ares()
            && category != EntityCategory::Structure
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    rules.is_ares_prerequisite_alternate(
                        entity.category().into(),
                        self.interner.resolve(entity.type_ref()),
                    )
                })
        {
            self.houses.request_house_tech_tree_recheck(old_owner);
            self.houses.request_house_tech_tree_recheck(new_owner);
        }
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && let Some(house) = self.houses.get_mut(&new_owner)
        {
            //701607..70164D: DontScore suppresses this captured-object
            //count, not the preceding old-House price addition.
            if !entity.tracking_facts().dont_score() {
                house.stats_mut().record_kill(category);
            }
        }
        //701674 Mark(UP),70D4A0 detach and701691 Mark(DOWN) all precede
        //the owner store. PlaceDown's Cell discovery therefore observes the
        //old House and can dirty its radar as well as its power.
        let mut mark_context = UninitContext::new(rules).with_effects(effects);
        if on_map {
            self.unmark_entity_remove(stable_id, mark_context.reborrow());
        }
        // `TechnoClass::ChangeOwner` runs the live-detach targeting sweep next,
        // before the house swap: everything shooting at this object is released
        // while the object still belongs to its old house. Engineer capture and
        // garrison transfer both come through here, so a squad that was firing
        // at a building stops the instant the building changes hands instead of
        // shooting at what is now its own structure.
        self.stop_all_targeting_on_detach(stable_id, rules, mark_context.effects());
        if on_map {
            self.mark_entity_put(stable_id, mark_context);
        }
        //Techno701701..719 removes from the old owner using the retained
        //Cell. The same Cell survives the owner write for701774..780.
        let spatial_threat_cell = self.spatial_threat_before_owner_change(stable_id, rules);
        // Techno70172F: `ReReveal(0, 0, 0, 0)` before the owner store takes
        // the sight off every map the old owner's reveal wrote. A building
        // released it at 448BD9 and an object in limbo at its Limbo, so for
        // them the cleared latch makes it a no-op.
        self.release_object_sight_with_context(
            stable_id,
            UninitContext::new(rules).with_effects(effects),
        );
        self.substrate.entities.change_owner(stable_id, new_owner);
        #[cfg(feature = "phobos")]
        self.phobos_attach_effects_on_owner_change(stable_id, old_owner, new_owner, rules);
        *self.ownership_changed_since_output = true;
        self.spatial_threat_after_owner_change(stable_id, spatial_threat_cell, rules);
        if category == EntityCategory::Structure {
            self.houses
                .move_house_base_tracking(stable_id, old_owner, new_owner);
        }
        // `BuildingClass::ChangeOwner @ 0x00448723` marks every transferred
        // building HasBeenCaptured (+0x6E3); survivors read it at death. Its
        // repair stops without a sound (`+0x6E8 = 0`, `0x00448CE8`).
        if category == EntityCategory::Structure
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.building.has_been_captured.mark();
            entity.building.repairing.stop_on_capture();
        }
        // `0x00448C6E..0x00448C94`, after the owner swap: the old house
        // records the building it lost (House+54E0), and the new house
        // forgets it if it had lost it before.
        if category == EntityCategory::Structure {
            if let Some(house) = self.houses.get_mut(&old_owner) {
                house.record_lost_building(stable_id);
            }
            if let Some(house) = self.houses.get_mut(&new_owner) {
                house.forget_lost_building(stable_id);
            }
            // `0x00448C9A..0x00448CD6`: a Construction Yard leaving a human
            // player (`0x0050B6F0`) cancels that player's current placement.
            if self
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .is_some_and(|object| object.construction_yard)
                && Self::is_human_player(&self.session, &self.houses, old_owner)
            {
                self.lifecycle_outputs
                    .emit(LifecycleOutput::ConstructionYardCaptured { owner: old_owner });
            }
        }
        // A factory leaves the old house's counter (Recount, `0x0044870E`)
        // and joins the new one's (IncrementFactoryCount, `0x00448CDD`),
        // whether it is in limbo or not.
        if let Some(entity) = self.substrate.entities.get(stable_id) {
            HouseTracking::transfer_factory(&mut self.houses, entity, old_owner, new_owner);
        }
        // A Helipad's docks leave the old house's AirportDocks
        // (`0x00448B4C..0x00448B6A`) and join the new one's
        // (`0x00449229..0x00449245`), whether it has opened or not.
        if category == EntityCategory::Structure
            && let Some(docks) = self
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .filter(|object| object.helipad)
                .map(|object| object.number_of_docks)
        {
            for (owner, delta) in [(old_owner, docks.wrapping_neg()), (new_owner, docks)] {
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.add_airport_docks(delta);
                }
            }
        }
        // A hospital's or machine shop's self-heal counts leave the old house
        // here (`0x00448AC8..0x00448B04`, `0x00448B0A..0x00448B46`, each arm
        // gated on the type's count and the building's own `+0x6E4` byte). The
        // new house gains them from the `Grand_Opening(1)` below, standing in
        // for native's tail add (see `remove_old_owner_self_heal`), not from
        // this block, so the counters move exactly once.
        if category == EntityCategory::Structure {
            self.remove_old_owner_self_heal(stable_id, old_owner, rules);
        }
        // Techno701735..701751 writes the owner then recomputes only +41A.
        // A former current-house object's +41B history survives the transfer.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity
                .seldom
                .discovery
                .classify_owner(self.session.current_house() == Some(new_owner));
        }
        // Techno701757..70178E: Added_To_Game on the new house (not in limbo).
        if on_map {
            self.house_added_to_game(stable_id, rules);
        }
        // TechnoChangeOwner7017A0 clears only the trigger-held original
        // House; the controlling House+2CC survives an ordinary capture.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity
                .seldom
                .mind_control
                .clear_house_original_owner_on_owner_change();
        }
        if build_const_eligible && let Some(house) = self.houses.get_mut(&new_owner) {
            house.append_build_const(stable_id);
        }
        if category == EntityCategory::Structure {
            self.join_house_base_lists(stable_id, old_owner, new_owner, rules);
            // Ares BuildingClass_ChangeOwnership_Add_Academy (hook 0x004491D5
            // -> 10023650) follows the new House+68 append.
            if rules.ares() {
                self.update_ares_academy_membership(stable_id, true, rules);
            }
        }
        // `TechnoClass::ChangeOwner` closes with the mission half (the
        // `+0x484` call at 0x00701849 reads the NEW owner's
        // IsControlledByHuman).
        self.change_owner_mission_half(stable_id, rules, effects);
        // Techno70187B: `UpdateReveal(0, 0, 0, 0, 0)` closes the Techno
        // owner change for an object out of limbo (7017C0 skips the tail for
        // one in limbo): its sight opens where it stands, on the maps the new
        // owner's reveal writes. A building's own tail follows (448BE8).
        //
        // RESIDUAL: Building448D83 reveals once more when the new owner is
        // the human player (IsHumanPlayer50B6F0). The latch 70187B set makes
        // that a no-op for a placed building; one captured in limbo would
        // natively open its sight there. No VERA producer captures a building
        // in limbo.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo())
        {
            self.update_object_sight_with_context(
                stable_id,
                0,
                None,
                UninitContext::new(rules).with_effects(effects),
            );
        }
        if category == EntityCategory::Structure {
            // Ares ChangeOwnership_SensorArrayB10016CD0 (hook448C3E) adds the
            // array for the new owner before448C69's primary factory.
            if rules.ares() {
                self.ares_sensor_array_dispatch(stable_id, true, rules, registry);
            }
            crate::sim::production::initialize_factory_primary(self, rules, stable_id);
            //448CEF: Grand_Opening(1) also runs on an already placed
            //building; its6E4 gate prevents repeating first-opening slots.
            self.grand_opening(
                stable_id,
                true,
                false,
                rules,
                Some(rules.overlay_registry()),
                effects,
            );
        }
        // `FootClass::ChangeOwner @ 0x004DBF13..0x004DBF32`: a Foot given to
        // a human house leaves its team.
        if category != EntityCategory::Structure
            && self
                .houses
                .get(&new_owner)
                .is_some_and(|house| house.is_human())
        {
            self.leave_team(stable_id, false, rules, effects);
        }
        self.foot_neighbors_after_owner_change(stable_id, rules);
        // Foot4DBF81 samples the live type, new owner and current cell only
        // after Techno/leave-team/history callbacks. High flight keeps +55C.
        self.add_unit_sensors_at_current(stable_id, rules, registry);
        self.reapply_building_gap_after_owner_change(stable_id, rules);
        self.relink_docked_after_owner_change(stable_id, &taken_along, rules, effects);
        // Building449267/449284: old House then new House, after lists,
        // primary and opening moved. Literal BuildCat0 selects the yard's
        // Building slot; Defense has no extra capture callback.
        if category == EntityCategory::Structure
            && let Some(factory_category) = self
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .and_then(crate::sim::production::factory_category_for_building)
        {
            production::update_factory_queue(self, rules, old_owner, factory_category, effects);
            production::update_factory_queue(self, rules, new_owner, factory_category, effects);
        }
        // Building4492D7..449368: the computer's captured-building sale is
        // after both factory queue updates and before the house rechecks.
        if category == EntityCategory::Structure {
            production::sell_after_owner_change(self, rules, stable_id, registry, effects);
        }
        //44936E/449379: both houses' retained1FC asks their next House tail.
        if category == EntityCategory::Structure {
            self.houses.request_house_tech_tree_recheck(old_owner);
            self.houses.request_house_tech_tree_recheck(new_owner);
        }
        // Wasm module `HouseTransferred` (RFC 0001 appendix 4.3), after the
        // transfer completed.
        self.raise_module_house_transferred(rules, effects, stable_id, old_owner, mind_control);
    }

    /// `BuildingClass::ChangeOwner @ 0x00448260`'s docked objects
    /// (`0x00448559..0x004486C8`), before the building's own owner swap. Each
    /// radio slot is read afresh, since the transmits on the way change them.
    ///
    /// A Foot (`AbstractFlags & 4`, `0x004485A2`) that answers NEED_TO_MOVE
    /// (`0x13`) with ROGER changes owner first (vt+0x3D4, `0x00448663`) when
    /// the building is a `WeaponsFactory=` or the Foot is at its dock
    /// ([`Self::docked_at_dock_coord`]). Its tether (`+0x418`) is kept for
    /// [`Self::relink_docked_after_owner_change`].
    ///
    /// Any other contact gets RUN_AWAY (`0x17`), then OVER_OUT (`3`). A
    /// contact that is not a Foot is sent both as a null target
    /// (`0x004485A8`), which the transmit resolves to the building's first
    /// contact on each send.
    ///
    /// A refinery's dock coordinate is the cell beside its pad
    /// ([`crate::sim::movement::navcom::building_dock_cell`]), so a miner
    /// unloading on the pad is sent off.
    fn take_docked_along_on_owner_change(
        &mut self,
        building: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Vec<(u64, bool)> {
        use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
        let mut taken = Vec::new();
        let Some((capacity, weapons_factory)) =
            self.substrate.entities.get(building).map(|entity| {
                (
                    entity.seldom.radio_contacts.capacity(),
                    self.object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.weapons_factory),
                )
            })
        else {
            return taken;
        };
        for slot in 0..capacity {
            let Some(contact) = self
                .substrate
                .entities
                .get(building)
                .and_then(|entity| entity.seldom.radio_contacts.slot(slot))
            else {
                continue;
            };
            let Some(foot) = self
                .substrate
                .entities
                .get(contact)
                .map(|entity| entity.category() != EntityCategory::Structure)
            else {
                continue;
            };
            if !foot {
                for message in [RadioMessage::RunAway, RadioMessage::Break] {
                    crate::sim::radio::transmit_to_contact(self, building, message, rules, effects);
                }
                continue;
            }
            let roger = transmit(
                self,
                building,
                contact,
                RadioMessage::NeedToMove,
                RadioPayload::default(),
                rules,
                effects,
            ) == RadioResponse::Roger;
            if roger && (weapons_factory || self.docked_at_dock_coord(building, contact, rules)) {
                let tethered = self
                    .substrate
                    .entities
                    .get(contact)
                    .is_some_and(|entity| entity.seldom.tether.partner().is_some());
                self.change_owner_impl(contact, new_owner, rules, registry, effects, false);
                taken.push((contact, tethered));
            } else {
                for message in [RadioMessage::RunAway, RadioMessage::Break] {
                    transmit(
                        self,
                        building,
                        contact,
                        message,
                        RadioPayload::default(),
                        rules,
                        effects,
                    );
                }
            }
        }
        taken
    }

    /// Whether `contact`'s coordinate (vt+0x48, `0x00448606`) lies within
    /// `0x3F` leptons (Distance3D, `0x00448644`) of `building`'s dock
    /// coordinate for it (GetDockCoord vt+0xA8, `0x004485F5`).
    fn docked_at_dock_coord(&self, building: u64, contact: u64, rules: &RuleSet) -> bool {
        let terrain = self.resolved_terrain.as_ref();
        let Some(dock) = crate::sim::movement::navcom::building_dock_coordinate(
            &self.substrate.entities,
            building,
            Some(contact),
            terrain,
            rules,
            &self.interner,
        ) else {
            return false;
        };
        self.substrate.entities.get(contact).is_some_and(|entity| {
            let at = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
            crate::util::native_x87::distance_3d_leptons(
                [at.x, at.y, at.z],
                [dock.x, dock.y, dock.z],
            ) <= 0x3F
        })
    }

    /// `BuildingClass::ChangeOwner`'s re-link (`0x00449289..0x004492D7`),
    /// after the owner swap: HELLO (`2`) to each object taken along, then,
    /// when that object was tethered, the tether set on the building
    /// (`0x004492B7`) and on the object (`0x004492C3`).
    fn relink_docked_after_owner_change(
        &mut self,
        building: u64,
        taken: &[(u64, bool)],
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        for &(contact, tethered) in taken {
            crate::sim::radio::transmit(
                self,
                building,
                contact,
                crate::sim::radio::RadioMessage::Hello,
                crate::sim::radio::RadioPayload::default(),
                rules,
                effects,
            );
            if tethered {
                if let Some(entity) = self.substrate.entities.get_mut(building) {
                    entity.seldom.tether.attach(contact);
                }
                if let Some(entity) = self.substrate.entities.get_mut(contact) {
                    entity.seldom.tether.attach(building);
                }
            }
        }
    }

    /// The mission half of `TechnoClass::ChangeOwner @ 0x007014A0`, every
    /// class. The early setters/Guard queue are inline before the house swap
    /// in `change_owner_impl`; this consumer handles only the new-house tail:
    /// - `0x007014D5..0x0070151A`: `Assign_Target(0)` (`+0x3C8`),
    ///   `Assign_Destination(0, 1)` (`+0x480` at `0x007014E9`; a Drive/Ship
    ///   Unit's is the Unit setter, [`Self::assign_null_destination`]), and
    ///   the ArchiveTarget (`+0x218`) cleared unless a Unit is deploying
    ///   (`0x00746DB0`, the `+0x6E1`/`+0x6E2` latches);
    /// - `0x00701524..0x0070156E`: `Queue_Mission(Guard, commence_now = 1)`
    ///   (`+0x1E8` = `MissionClass::Queue_Mission @ 0x005B35E0`) unless current
    ///   is Selling (0x13), or a Unit whose type is `IsSimpleDeployer`
    ///   (`UnitType+0xE13`) is in Unload; a ready object commits Guard on the
    ///   spot. VERA also skips it for a `WeaponsFactory=` building in Unload,
    ///   so a captured factory still lets its product out (#3593);
    /// - `0x00701793..0x007017BA`, after the `+0x21C` house swap: a Rescue
    ///   (0x15) current or queued mission becomes `Assign_Mission(Guard)`;
    /// - `0x007017C0..0x00701849`, unless in limbo (`+0x81`), a
    ///   `WeaponsFactory=` building unloading, or in radio contact with a
    ///   `WeaponsFactory=` building (`BuildingType+0x16BD`, the war-factory
    ///   exit link): `Assign_Destination(0, 1)` (`0x0070182F`),
    ///   `Assign_Target(0)` and `Enter_Idle_Mode(0, 1)` (`+0x484`), which
    ///   reads the NEW owner ([`enter_idle_mode`]):
    ///   - a war or chrono miner takes the Unit leaf's harvester arm
    ///     (`Simulation::unit_enter_idle_mode`): Harvest for the new owner,
    ///     Guard when that owner is human and the miner stands off ore, and
    ///     nothing while in radio contact (a miner docked at its refinery) or
    ///     while the Guard above is still only queued (a miner caught
    ///     mid-track);
    ///   - any other Unit or Infantry takes its class leaf, Unit
    ///     `0x00738970` or Infantry `0x0051CBA0`;
    ///   - a Building (`0x0044D6A0` with `initial = 0`) calls `0x00447780(1)`
    ///     and `Queue_Mission(Guard, 0)`.
    ///
    /// Nothing here clears an attack-move's MegaMission (`Foot+0x5C4`). The
    /// Enter_Idle_Mode above asks RefreshMegaMission
    /// ([`Simulation::foot_refresh_mega_mission`]), which resumes the order
    /// when the object had found a target on the way or Guard was committed
    /// on the spot; otherwise the order ends there. A released object goes
    /// on to its MegaDestination for its old owner; CaptureUnit resets a
    /// captive's orders to Guard afterwards (`0x00471EA5`), which drops the
    /// resumed NavCom but keeps the MegaMission.
    ///
    /// Callers: engineer capture and garrison transfer, and mind control's
    /// CaptureUnit and FreeUnit. No stock miner is ever mind-controlled: HARV,
    /// CMIN and SMIN are `ImmuneToPsionics=yes`.
    ///
    /// RESIDUAL: no stock aircraft can be captured, so the Aircraft
    /// Enter_Idle_Mode (`0x004176F0`) here is dormant.
    ///
    /// The tail (`0x0070184F..0x0070187B`): a radar-registered object
    /// (`+0x423`) calls `UnregisterFromRadar` (vt+0x498, `0x0070CCC0`) and
    /// `RegisterOnRadar` (vt+0x494, `0x0070CC90`), which the client radar
    /// tracker does when it sees the owner change
    /// (`render/radar/tracker.rs`, `registration_identity_changed`); then
    /// `UpdateReveal(0, 0, 0, 0, 0)` (vt+0x488, `0x0070AF50`), ported in
    /// `change_owner_inherited`.
    ///
    /// `+0x484` census (`search_instructions CALL [+0x484]`), the sites a
    /// miner can reach besides Move arrival, this owner change and the depot
    /// exit, with their gates:
    /// - `DriveLocomotionClass::Stop_And_Scatter @ 0x004B48BE` → `+0x484(0,1)`
    ///   after `Stop_Moving` when the NavQueue is non-empty. Not run by VERA:
    ///   VERA scatter is UNCHECKED; natively a human-parked Guard miner
    ///   scattered on ore would re-queue Harvest through this call.
    /// - `FootClass::Queue_Navigation_List @ 0x004DA0E0` → `+0x484(0,1)` at
    ///   `0x004DA1A6` when NavCom == 0, Get_Mission == Guard and contact 0 is
    ///   not a WeaponsFactory building ([`Simulation::queue_navigation_list`]).
    ///   Its callers are ExitObject's Infantry post (`0x00444DAC`) and the
    ///   Unit setter's WeaponsFactory arm (`0x00742E28`,
    ///   `Simulation::unit_factory_exit_arm`).
    /// - `FootClass::Check_Destination_Is_UnitRepair_Dock @ 0x004DBA15` →
    ///   `+0x484(0, !depot-contact)` when NavCom == 0 && TarCom == 0
    ///   (virtual slot; miner reachability unestablished; VERA's
    ///   `track_navigation_gate`).
    /// - `FootClass::ReceiveDamage @ 0x004D74C7`, gated on
    ///   `MissionControl[current].NoThreat && !Zombie` (retail `[Harmless]`)
    ///   for a teamless Foot hit by a source; the damage receiver
    ///   (`combat::world_receiver`) calls it.
    fn change_owner_mission_half(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        use crate::sim::mission::{MissionId, MissionType};
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let factory_unloading = self.weapons_factory_unloading(stable_id, rules);
        let in_limbo = entity.lifecycle.in_limbo();
        let now = self.session.binary_frame();
        let rescue = self.substrate.entities.get(stable_id).is_some_and(|e| {
            e.mission.current().known() == Some(MissionType::Rescue)
                || e.mission.queued().known() == Some(MissionType::Rescue)
        });
        if rescue {
            let _ = self.substrate.mission_assign_exact(
                stable_id,
                MissionId::from_known(MissionType::Guard),
                now,
            );
        }
        if in_limbo || factory_unloading {
            return;
        }
        let in_factory_contact = self.substrate.entities.get(stable_id).is_some_and(|e| {
            e.seldom
                .radio_contacts
                .iter_live()
                .any(|other| self.is_weapons_factory(other, rules))
        });
        if in_factory_contact {
            return;
        }
        self.assign_null_destination(stable_id, rules, Some(rules.overlay_registry()), effects);
        let _ = self.assign_target_represented(stable_id, None, rules, effects);
        enter_idle_mode(
            self,
            stable_id,
            rules,
            Some(rules.overlay_registry()),
            effects,
        );
    }

    /// A `WeaponsFactory=` building whose current mission is Unload: one
    /// still letting its product out of the bay.
    fn weapons_factory_unloading(&self, stable_id: u64, rules: &RuleSet) -> bool {
        use crate::sim::mission::MissionType;
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.mission.current().known() == Some(MissionType::Unload))
            && self.is_weapons_factory(stable_id, rules)
    }
}
