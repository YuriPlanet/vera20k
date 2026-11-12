//! Building+660 online state and its explicit map/trigger and Temporal writers.
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// Building43B882 initializes this latch to one. It is independent of
/// Techno+270/278: GoOnline can set it while a Temporal chain still exists.
/// Steam gamemd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(not(feature = "ares"), serde(transparent))]
pub(crate) struct BuildingOnline {
    online: bool,
    /// Ares BuildingExt `TogglePower_HasPower` (`+0x51`, constructor true):
    /// the state the last GoOnline/GoOffline left, which a Temporal release
    /// restores instead of forcing the latch on.
    #[cfg(feature = "ares")]
    toggle_power_has_power: bool,
}

impl Default for BuildingOnline {
    fn default() -> Self {
        Self {
            online: true,
            #[cfg(feature = "ares")]
            toggle_power_has_power: true,
        }
    }
}

impl BuildingOnline {
    /// GoOnline/GoOffline's admitted latch write (`0x00452287`,
    /// `0x00452393`). Ares 3.0 BuildingClass_GoOnline_TogglePower 10014090
    /// and _GoOffline_TogglePower 10014070 also record it.
    fn toggle(&mut self, online: bool) {
        self.online = online;
        #[cfg(feature = "ares")]
        {
            self.toggle_power_has_power = online;
        }
    }

    /// TemporalGoOffline `0x004521C0` / TemporalGoOnline `0x00452210`. Ares
    /// 3.0 BuildingClass_Enable_TogglePower 10013FB0 (hook `0x00452210`)
    /// restores the last toggle on release instead of storing one.
    fn temporal(&mut self, online: bool) {
        #[cfg(feature = "ares")]
        let online = online && self.toggle_power_has_power;
        self.online = online;
    }

    /// Whether the last GoOffline is still in force (hashed with the latch).
    #[cfg(feature = "ares")]
    pub(crate) fn ares_toggled_off(&self) -> bool {
        !self.toggle_power_has_power
    }
}

impl GameEntity {
    /// Building+660, read by4555D0/44E88F/508EA3/50B020 and factory queries.
    pub fn building_online(&self) -> bool {
        self.building_online.online
    }

    /// Ares BuildingExt `TogglePower_HasPower` (`+0x51`) clear: the last
    /// GoOffline is still in force. TechnoClass_DrawExtras_PowerOff
    /// (10049FE0, hook `0x6F526C`) draws the power-off marker from it.
    #[cfg(feature = "ares")]
    pub fn ares_toggled_off(&self) -> bool {
        self.building_online.ares_toggled_off()
    }

    /// Phobos `TechnoExt::IsActive` (`src/Ext/Techno/Body.cpp`) plus the power
    /// plant enhancer loops' `HasPower` (Building+660) gate. Temporal
    /// targeting and warping are both covered by `is_warped_out`.
    #[cfg(feature = "phobos")]
    pub fn is_power_plant_enhancer_active(&self) -> bool {
        self.is_active()
            && self.health.current() > 0
            && !self.lifecycle.in_limbo()
            && !self.is_warped_out()
            && !self.deactivated.is_offline()
            && self.emp_remaining() <= 0
            && self.building_online()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_building_online_for_test(&mut self, online: bool) {
        self.building_online.online = online;
    }
}

impl Simulation {
    /// GoOffline452360: only positive native PowerDrain or Powered types
    /// can turn off. Return the actual transition, not Execute61's receipt.
    pub(crate) fn go_building_offline(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure || !entity.building_online() {
            return false;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        if crate::sim::power_system::native_building_power_drain(object.power) <= 0
            && !object.powered
        {
            return false;
        }
        let owner = entity.owner();
        let factory = crate::sim::production::factory_category_for_building(object);
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_online
            .toggle(false);
        Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, true);
        self.houses.request_house_tech_tree_recheck(owner);
        self.set_building_stuff_enabled(id, false, rules);
        //4523DE speaks before509140; the online arm reverses that order.
        if self.house_is_human_player(owner) {
            self.sound_events.emit(SimSoundEvent::HouseEva {
                event: "EVA_BuildingOffLine",
                owner,
            });
        }
        if let Some(category) = factory {
            crate::sim::production::update_factory_queue(self, rules, owner, category, effects);
        }
        true
    }

    /// GoOnline452260, also inlined by ReadINI44FC27..44FD1F. A building
    /// held by an EMP (`+0x504` nonzero, `0x00452271`) stays off; only Ares
    /// writes that counter.
    pub(crate) fn go_building_online(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure
            || entity.building_online()
            || entity.emp_remaining() != 0
        {
            return false;
        }
        let owner = entity.owner();
        let factory = self
            .object_type(entity.type_ref(), rules)
            .and_then(crate::sim::production::factory_category_for_building);
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_online
            .toggle(true);
        Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, true);
        self.houses.request_house_tech_tree_recheck(owner);
        self.set_building_stuff_enabled(id, true, rules);
        if let Some(category) = factory {
            crate::sim::production::update_factory_queue(self, rules, owner, category, effects);
        }
        if self.house_is_human_player(owner) {
            self.sound_events.emit(SimSoundEvent::HouseEva {
                event: "EVA_BuildingOnLine",
                owner,
            });
        }
        true
    }

    /// The Ares temporal factory refreshes
    /// ([`ObjectSubstrate::set_building_online_for_temporal`]), in the order
    /// the latch changed, before the factory sweep. A building already gone
    /// from storage is skipped: its Limbo has refreshed the queues since.
    ///
    /// Residual (sim order): natively the refresh runs inside the warp or
    /// release. Trigger: a factory warped or released by a Temporal weapon
    /// or an Ares ChronoWarp. Effect: until this sweep, the factory's
    /// queue keeps its hold state from before; the sweep itself sees the
    /// refreshed one. Frequency: each such warp and release. Risk: a
    /// queue order or another refresh in between reads the old hold.
    ///
    /// [`ObjectSubstrate::set_building_online_for_temporal`]: crate::sim::world::ObjectSubstrate::set_building_online_for_temporal
    #[cfg(feature = "ares")]
    pub(crate) fn ares_refresh_temporal_factories(
        &mut self,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        for id in self.substrate.take_ares_temporal_factory_refresh() {
            self.ares_update_factory_queues(id, rules, effects);
        }
    }

    /// BuildingExt::UpdateFactoryQueues `10013250`: a building's owner runs
    /// `HouseClass::Update_FactoriesQueues @ 0x00509140` for the building's
    /// `Factory=` type and `Naval=`, BuildCat DontCare; a building with no
    /// `Factory=` or no longer stored does nothing. Callers: the temporal
    /// refresh above, and Ares Activate `1004CCB0` / Deactivate `1004CDC0`
    /// on a Building's transition (`Simulation::techno_reactivate`,
    /// `Simulation::techno_deactivate`).
    #[cfg(feature = "ares")]
    pub(crate) fn ares_update_factory_queues(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure {
            return;
        }
        let owner = entity.owner();
        if let Some(category) = self
            .object_type(entity.type_ref(), rules)
            .and_then(crate::sim::production::factory_category_for_building)
        {
            crate::sim::production::update_factory_queue(self, rules, owner, category, effects);
        }
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// `BuildingClass` `0x004521C0` (`online == false`, a Temporal erase
    /// starting) and `0x00452210` (its release): the online latch (`+0x660`),
    /// then each of the 21 animation slots (`+0x55C`) frozen and drawn
    /// translucent, or released ([`AnimObject::set_temporal_erase`]).
    /// Temporal must not call the normal toggles: these force stores have no
    /// factory, light, Stuff or EVA callbacks.
    ///
    /// Ares 3.0 BuildingClass_Disable_Temporal_Factories and
    /// _Enable_Temporal_Factories (both `10013FA0`, hooks `0x004521C8` and
    /// `0x00452218`, right after the latch) add one:
    /// BuildingExt::UpdateFactoryQueues `10013250` runs the owner's
    /// `HouseClass::Update_FactoriesQueues @ 0x00509140` for the building's
    /// `Factory=` type. VERA-only: the building is queued for the next
    /// factory sweep ([`Simulation::ares_refresh_temporal_factories`]), as
    /// TemporalClass::LetGo's callers hold the substrate apart from the
    /// Simulation.
    ///
    /// [`AnimObject::set_temporal_erase`]: crate::sim::anim_class::AnimObject::set_temporal_erase
    pub(crate) fn set_building_online_for_temporal(&mut self, id: u64, online: bool) {
        let Some(entity) = self.entities.get_mut(id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure {
            return;
        }
        entity.building_online.temporal(online);
        let slots = *entity.building_art.anim_slots();
        #[cfg(feature = "ares")]
        self.queue_ares_temporal_factory_refresh(id);
        for slot in slots.into_iter().flatten() {
            if let Some(anim) = self.anims.get_mut(slot) {
                anim.set_temporal_erase(!online);
            }
        }
    }
}

#[cfg(all(test, feature = "ares"))]
mod ares_tests {
    use crate::rules::ruleset::RuleSet;
    use crate::sim::world::{FrameEffects, Simulation};

    /// Ares 10013FB0 (hook 0x452210): a building the player powered down
    /// stays offline when a Temporal erase lets it go; one left online
    /// comes back, and GoOnline still powers it up afterwards.
    #[test]
    fn ares_temporal_release_restores_the_power_toggle() {
        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[BuildingTypes]\n0=DRAIN\n[DRAIN]\nStrength=100\nPower=-10\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let owner = sim.interner.intern("A");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, false, 0, 10,
            ));
        let off = sim.spawn_object("DRAIN", "A", 10, 10, 0, &rules).unwrap();
        let on = sim.spawn_object("DRAIN", "A", 14, 10, 0, &rules).unwrap();
        assert!(sim.go_building_offline(off, &rules, FrameEffects::default()));
        for id in [off, on] {
            sim.substrate.set_building_online_for_temporal(id, false);
            sim.substrate.set_building_online_for_temporal(id, true);
        }
        let online = |sim: &Simulation, id| sim.entities().get(id).unwrap().building_online();
        assert!(!online(&sim, off));
        assert!(online(&sim, on));
        assert!(sim.go_building_online(off, &rules, FrameEffects::default()));
        sim.substrate.set_building_online_for_temporal(off, false);
        sim.substrate.set_building_online_for_temporal(off, true);
        assert!(online(&sim, off));
    }
}
