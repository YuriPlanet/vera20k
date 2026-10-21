//! Building+660 online state and its explicit map/trigger and Temporal writers.
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// Building43B882 initializes this latch to one. It is independent of
/// Techno+270/278: GoOnline can set it while a Temporal chain still exists.
/// Steam gamemd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct BuildingOnline {
    online: bool,
}

impl Default for BuildingOnline {
    fn default() -> Self {
        Self { online: true }
    }
}

impl GameEntity {
    /// Building+660, read by4555D0/44E88F/508EA3/50B020 and factory queries.
    pub fn building_online(&self) -> bool {
        self.building_online.online
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
            .online = false;
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

    /// GoOnline452260, also inlined by ReadINI44FC27..44FD1F. The native
    /// EMP+504 gate remains at its constructor zero: the engine has no EMP
    /// producer yet (the same bound as OperationalFacts' existing adapter).
    pub(crate) fn go_building_online(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure || entity.building_online() {
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
            .online = true;
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
}

impl crate::sim::world::ObjectSubstrate {
    /// `BuildingClass` `0x004521C0` (`online == false`, a Temporal erase
    /// starting) and `0x00452210` (its release): the online latch (`+0x660`),
    /// then each of the 21 animation slots (`+0x55C`) frozen and drawn
    /// translucent, or released ([`AnimObject::set_temporal_erase`]).
    /// Temporal must not call the normal toggles: these force stores have no
    /// factory, light, Stuff or EVA callbacks.
    ///
    /// [`AnimObject::set_temporal_erase`]: crate::sim::anim_class::AnimObject::set_temporal_erase
    pub(crate) fn set_building_online_for_temporal(&mut self, id: u64, online: bool) {
        let Some(entity) = self.entities.get_mut(id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure {
            return;
        }
        entity.building_online.online = online;
        let slots = *entity.building_art.anim_slots();
        for slot in slots.into_iter().flatten() {
            if let Some(anim) = self.anims.get_mut(slot) {
                anim.set_temporal_erase(!online);
            }
        }
    }
}
