//! Building+660 online state and its explicit map/trigger and Temporal writers.
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// Building43B882 initializes this latch to one. It is independent of
/// Techno+270/278: GoOnline can set it while a Temporal chain still exists.
/// Steam gamemd SHA256 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600.
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
    /// The latch stores of ForceOffline4521C0/ForceOnline452210. Temporal
    /// must not call the normal toggles: these force stores have no factory,
    /// light, Stuff or EVA callbacks. Their Anim119 translucency/fade tail
    /// remains the existing Temporal presentation residual.
    pub(crate) fn set_building_online_for_temporal(&mut self, id: u64, online: bool) {
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && entity.category() == EntityCategory::Structure
        {
            entity.building_online.online = online;
        }
    }

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
        self.invalidate_house_power(owner, true);
        self.request_house_tech_tree_recheck(owner);
        self.set_building_stuff_enabled(id, false, rules);
        //4523DE speaks before509140; the online arm reverses that order.
        if self.house_is_human_player(owner) {
            self.sound_events.push(SimSoundEvent::HouseEva {
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
        self.invalidate_house_power(owner, true);
        self.request_house_tech_tree_recheck(owner);
        self.set_building_stuff_enabled(id, true, rules);
        if let Some(category) = factory {
            crate::sim::production::update_factory_queue(self, rules, owner, category, effects);
        }
        if self.house_is_human_player(owner) {
            self.sound_events.push(SimSoundEvent::HouseEva {
                event: "EVA_BuildingOnLine",
                owner,
            });
        }
        true
    }
}
