//! A Building's animation art (`BuildingArt`) and the native functions that
//! change it. Building+6E6 is retained state, independent of actual HP.
//! Original451EE0/442A95/4508D7 write it before replacing occupied animation
//! slots; selfheal never calls this owner.

use crate::rules::art_data::BuildingAnimConfig;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::native_x87::MaskedX87Ordering::Greater;

#[path = "building_art_admission.rs"]
mod admission;
#[cfg(test)]
#[path = "building_art_depot_tests.rs"]
mod depot_tests;
#[cfg(test)]
#[path = "building_art_expiry_tests.rs"]
mod expiry_tests;
#[cfg(test)]
#[path = "building_art_garrison_tests.rs"]
mod garrison_tests;
#[path = "building_art_power.rs"]
mod power;
#[path = "building_art_storage.rs"]
mod storage;
#[path = "building_art_super.rs"]
mod super_anim;
pub(crate) use admission::HasEngineer;
pub(crate) use storage::BuildingStorage;

/// A Building's animation art, owned by this module: its 21 AnimClass slots
/// (Building+55C), their effect-replay marks (+5B0), the damaged-art flag
/// (+6E6), its StorageClass art, the Stuff flag (+6EA) and the
/// OnConstructionComplete guard (+6E4).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildingArt {
    /// Retained Building+6E6 animation-transition flag (saved454469).
    /// Selfheal and arbitrary health writes do not refresh this value.
    damage_state_active: bool,
    /// Building+55C's 21 retained AnimClass references. AnimStore owns the
    /// animation objects and their scheduler/timers; these are slot identities.
    anim_slots: [Option<u64>; 21],
    /// Building+5B0: effect slots marked for replay by4547C0, cleared by4545D0.
    anim_effect_replay: [bool; 21],
    /// Retained raw StorageClass slots and last refinery animation tier.
    /// Only the tier (+6F0) is art. The amounts are TechnoClass+0x33C, whose
    /// native writers are economy code (the sale payout 44A232,
    /// DestructionEffects 441B30, House spend and transfer); no Rust code
    /// writes them yet, and the art reads them. Porting such a writer moves
    /// the amounts to its owner.
    storage: BuildingStorage,
    /// Building+6EA, constructor43B996 true; distinct from operational6C8.
    /// Its other native writers (ChangeOwner 4484C6, ReadScenarioBuildings
    /// 44FC80, GoOnline 452260, EnableStuff 452410, DisableStuff 452480) also
    /// share the light/gap/animation owner. Dormant CloakGenerator and
    /// LaserFencePost tails require their separate mechanisms.
    stuff_enabled: bool,
    /// Building+6E4: OnConstructionComplete's one-time allocation guard.
    actually_placed: bool,
    /// Derived draw cache from UpdateAnimation, before this object's Techno
    /// tint updates. Never read by simulation decisions or the world hash.
    #[serde(default)]
    tint_sample: Option<BuildingTintSample>,
}

/// Inputs to native BuildingClass::UpdateAnimation's relight at 00450A47.
/// Retain the pre-Techno values across save/load because the next render uses
/// them; the gameplay owners remain the entity's live tint and flash state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildingTintSample {
    frame: u32,
    flashed: bool,
    invulnerability: Option<crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    airstrike: crate::sim::superweapon::airstrike_tint::AirstrikeTintState,
    airstrike_active: bool,
}

impl BuildingTintSample {
    pub fn frame(&self) -> u32 {
        self.frame
    }
    pub fn flashed(&self) -> bool {
        self.flashed
    }
    pub fn invulnerability(
        &self,
    ) -> Option<&crate::sim::superweapon::invulnerability::InvulnerabilityState> {
        self.invulnerability.as_ref()
    }
    pub fn airstrike(&self) -> &crate::sim::superweapon::airstrike_tint::AirstrikeTintState {
        &self.airstrike
    }
    pub fn airstrike_active(&self) -> bool {
        self.airstrike_active
    }
}

impl BuildingArt {
    /// The light a relight gives the slot anims
    /// ([`crate::sim::world::ObjectSubstrate::relight_building_anims`]).
    pub fn capture_tint_sample(
        &mut self,
        frame: u32,
        flashed: bool,
        invulnerability: Option<&crate::sim::superweapon::invulnerability::InvulnerabilityState>,
        airstrike: &crate::sim::superweapon::airstrike_tint::AirstrikeTintState,
        airstrike_active: bool,
    ) {
        self.tint_sample = Some(BuildingTintSample {
            frame,
            flashed,
            invulnerability: invulnerability.cloned(),
            airstrike: airstrike.clone(),
            airstrike_active,
        });
    }

    pub fn tint_sample(&self) -> Option<&BuildingTintSample> {
        self.tint_sample.as_ref()
    }

    /// Building+6E6.
    pub fn damage_state_active(&self) -> bool {
        self.damage_state_active
    }

    /// Building+55C's slots.
    pub fn anim_slots(&self) -> &[Option<u64>; 21] {
        &self.anim_slots
    }

    /// Building+5B0's replay marks.
    pub(crate) fn anim_effect_replay(&self) -> &[bool; 21] {
        &self.anim_effect_replay
    }

    /// The StorageClass art.
    pub(crate) fn storage(&self) -> &BuildingStorage {
        &self.storage
    }

    /// Building+6EA.
    pub(crate) fn stuff_enabled(&self) -> bool {
        self.stuff_enabled
    }

    /// Building+6E4: OnConstructionComplete has run.
    pub fn actually_placed(&self) -> bool {
        self.actually_placed
    }

    /// Retained flag transition itself, shared by every reached caller.
    /// Returns whether native451EE0 proceeds to its ordered occupied-slot
    /// replacement loop.
    fn set_damage_state(&mut self, damaged: bool) -> bool {
        if self.damage_state_active == damaged {
            return false;
        }
        self.damage_state_active = damaged;
        true
    }
}

/// Fixture setup for tests outside this module.
#[cfg(any(test, feature = "test-support"))]
impl BuildingArt {
    pub fn set_actually_placed_for_test(&mut self, placed: bool) {
        self.actually_placed = placed;
    }

    pub(crate) fn set_damage_state_active_for_test(&mut self, active: bool) {
        self.damage_state_active = active;
    }

    pub(crate) fn set_stuff_enabled_for_test(&mut self, enabled: bool) {
        self.stuff_enabled = enabled;
    }

    pub(crate) fn anim_slots_mut_for_test(&mut self) -> &mut [Option<u64>; 21] {
        &mut self.anim_slots
    }

    pub(crate) fn anim_effect_replay_mut_for_test(&mut self) -> &mut [bool; 21] {
        &mut self.anim_effect_replay
    }

    pub(crate) fn storage_mut_for_test(&mut self) -> &mut BuildingStorage {
        &mut self.storage
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// `BuildingClass` `0x00452000`, and its copy at the end of Flash
    /// (`0x00456E00`): relight the slot anims from the building's light at
    /// `frame`, which [`BuildingArt::tint_sample`] keeps for the draw.
    /// UpdateAnimation (`0x00450A47..0x00450A77`) relights them the same way
    /// on each Update (Building `0x0043FE22`), before the Techno AI
    /// (`0x0043FE56`) steps either tint stage or the flash.
    pub(crate) fn relight_building_anims(&mut self, id: u64, frame: u32) {
        let airstrike_active = self.airstrike_tint_active(id);
        if let Some(entity) = self.entities.get_mut(id)
            && entity.category() == crate::map::entities::EntityCategory::Structure
        {
            entity.building_art.capture_tint_sample(
                frame,
                entity.flashing.flashed(),
                entity.invulnerability.as_ref(),
                &entity.airstrike_tint,
                airstrike_active,
            );
        }
    }

    /// Building vt+0x148 `Flash` (`0x00456E00`): the Techno body
    /// ([`GameEntity::flash_base`]), then the slot-anim relight. Its redraw
    /// flag (`+0x80`) is presentation.
    pub(crate) fn flash_building(&mut self, id: u64, frames: i32, frame: u32) {
        if let Some(entity) = self.entities.get_mut(id) {
            entity.flash_base(frames);
        }
        self.relight_building_anims(id, frame);
    }
}

impl Default for BuildingArt {
    fn default() -> Self {
        Self {
            damage_state_active: false,
            anim_slots: [None; 21],
            anim_effect_replay: [false; 21],
            storage: BuildingStorage::default(),
            stuff_enabled: true,
            actually_placed: false,
            tint_sample: None,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static GRAND_OPENING_CALLS: std::cell::RefCell<Option<Vec<(u64, bool)>>> = const { std::cell::RefCell::new(None) };
}

/// Observe calls into the production owner, including its native6E4 early
/// return. The trace is test-local and does not enter persisted sim state.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn trace_grand_opening_calls<T>(run: impl FnOnce() -> T) -> (T, Vec<(u64, bool)>) {
    GRAND_OPENING_CALLS.with_borrow_mut(|trace| {
        assert!(trace.is_none(), "nested Grand_Opening traces");
        *trace = Some(Vec::new());
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            GRAND_OPENING_CALLS.with_borrow_mut(|trace| *trace = None);
        }
    }
    let reset = Reset;
    let result = run();
    let calls = GRAND_OPENING_CALLS.with_borrow_mut(|trace| trace.take().unwrap());
    drop(reset);
    (result, calls)
}

impl Simulation {
    fn building_anim_world_coord(
        &self,
        id: u64,
        config: &BuildingAnimConfig,
    ) -> Option<crate::sim::anim_class::AnimWorldCoord> {
        let entity = self.substrate.entities.get(id)?;
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let (base, _) =
            crate::sim::movement::ground_pose::building_render_order_parts(location, false, false);
        let (dx, dy) = self
            .session
            .pixel_conversion_bounds()
            .offset_to_leptons(config.x, config.y);
        Some(crate::sim::anim_class::AnimWorldCoord {
            x: base.x.wrapping_add(dx),
            y: base.y.wrapping_add(dy),
            z: base.z,
        })
    }

    /// Building43F738 repositions the existing 21 slots after its coordinate
    /// changes. It neither reconstructs animations nor resets their timers.
    pub(crate) fn reposition_building_anim_slots(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() != crate::map::entities::EntityCategory::Structure {
            return;
        }
        let slots = entity.building_art.anim_slots;
        for (slot, anim_id) in slots.into_iter().enumerate() {
            let Some(anim_id) = anim_id else { continue };
            let Some(config) = self.building_anim_config(id, slot as u8, rules) else {
                continue;
            };
            let coord = self.building_anim_world_coord(id, &config).unwrap();
            if let Some(anim) = self.substrate.anims.get_mut(anim_id) {
                anim.move_with_building(coord);
            }
        }
    }

    /// Original Grand_Opening445F80. First-opening effects depend on6E4,
    /// independently of the capture argument. Captures of already placed
    /// buildings still rearm ProduceCash and apply activation/house effects.
    pub fn grand_opening(
        &mut self,
        id: u64,
        captured: bool,
        scenario_initialization: bool,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        #[cfg(any(test, feature = "test-support"))]
        GRAND_OPENING_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.push((id, captured));
            }
        });
        // Ares445F80 entry hook100100B0 ->100449D0 precedes the vanilla
        // ActuallyPlaced early return, including repeated and capture calls.
        #[cfg(feature = "ares")]
        self.grant_factory_plans(id, rules);
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() != crate::map::entities::EntityCategory::Structure
            || (entity.building_art.actually_placed && !captured)
        {
            return;
        }
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return;
        };
        let first_opening = !entity.building_art.actually_placed;
        let owner = entity.owner();
        let cell = (entity.position.rx(), entity.position.ry());
        let refinery = object.refinery;
        let powered = object.powered;
        let damaged = requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        );
        let garrisoned = entity
            .passenger_role
            .cargo()
            .is_some_and(|cargo| !cargo.passengers().is_empty());
        // 0x00446382..0x004463B4: the built type's InfantryGainSelfHeal and
        // UnitsGainSelfHeal join the owner's house. Natively a capture of a
        // placed building skips this add (0x00445FD0) and ChangeOwner's tail
        // makes it (0x00449201..0x00449223); VERA makes it here for both
        // (`Simulation::remove_old_owner_self_heal` records the residual).
        self.grant_house_self_heal(id, rules);
        crate::sim::credit_income::produce_cash_on_grand_opening(
            &self.interner,
            &self.session,
            &mut self.substrate,
            id,
            rules,
        );
        if first_opening && !refinery {
            for slot in [18, 3, 4, 5, 6] {
                self.set_building_anim_slot(
                    id,
                    slot,
                    damaged,
                    garrisoned && matches!(slot, 18 | 3),
                    0,
                    rules,
                );
            }
        }
        if first_opening && refinery {
            self.initialize_refinery_storage_anim(id, rules);
        }
        // 0x004463C0..0x004463E0: a Helipad's docks join its house's
        // AirportDocks at the first opening only.
        if first_opening
            && object.helipad
            && let Some(house) = self.houses.get_mut(&owner)
        {
            house
                .tracking_mut()
                .add_airport_docks(object.number_of_docks);
        }
        if first_opening {
            self.open_super_weapon_anims(id, damaged, garrisoned, rules);
        }
        if first_opening && let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.mission_leaf.set_building_ready_latch(1);
        }
        //4467D0 applies the initial Powered policy even before the first
        //operational-edge restoration. New constructors therefore pause here.
        if powered {
            self.apply_building_anim_power(id, false, rules);
        }
        // 0x004467E7..0x004468A8: a computer house's `RevealToAll=` building
        // shows the current house its sight as it opens from construction
        // (0x00446816..0x00446829), and raises radar event 12 at its cell
        // (Location `+0x9C` / 256) on a capture too. The fog border redraw
        // around it (`0x00567DA0`, radius + 3) is presentation: the shroud
        // renderer rebuilds from the vision owner.
        if object.reveal_to_all && !self.house_is_human_player(owner) {
            if !captured {
                self.reveal_sight_to_current_house(
                    id,
                    crate::sim::world::UninitContext::new(rules, registry).with_effects(effects),
                );
            }
            self.sound_events
                .emit(crate::sim::world::SimSoundEvent::RadarEvent {
                    radar: crate::sim::radar::RadarEventRequest::new(
                        crate::sim::radar::RadarEventType::ConstructionComplete,
                        cell.0,
                        cell.1,
                    ),
                });
        }
        //44699A..446A5B: default naval rally, before placement/power publication.
        self.initialize_naval_rally_point(id, rules);
        //446A61 sets the same House tech-recheck consumed at its late update.
        if let Some(house) = self.houses.get_mut(&owner) {
            house.request_tech_recheck();
        }
        //446A77 invalidates retained House power and radar on every reached
        //opening (including capture), before publishing placement at446A84.
        Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, true);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.building_art.actually_placed = true;
        }
        self.add_building_sensor_array_if_powered(id, rules, registry);
        Self::announce_super_weapon_building_complete(
            &mut self.interner,
            &self.substrate,
            &mut self.sound_events,
            id,
            rules,
        );
        if !captured && !scenario_initialization {
            if crate::sim::production::spawn_building_free_unit(self, id, rules, registry, effects)
            {
                self.note_mission_spawned_entities();
            }
        }
    }
    fn building_anim_config(
        &self,
        id: u64,
        slot: u8,
        rules: &RuleSet,
    ) -> Option<BuildingAnimConfig> {
        let entity = self.substrate.entities.get(id)?;
        let name = self.interner.resolve(entity.type_ref());
        let object = rules.object(name)?;
        let mut config = if let Some(record) = object.building_upgrade().power_up_art(slot) {
            // PowerUp records belong to the retained BuildingType, including
            // constructor defaults when no ART section was ever reached.
            rules.art().building_power_up_config(slot, record)
        } else {
            rules
                .art()
                .resolve_metadata_entry(name, &object.image)?
                .building_anims
                .iter()
                .find(|config| config.native_slot == slot)
                .cloned()?
        };
        if let Some(normal_name) = self
            .building_upgrade_art_names()
            .name(entity.type_ref(), slot)
        {
            config.anim_type = normal_name.to_owned();
        }
        Some(config)
    }

    /// PowerUp451420..4514EC restores normal occupied slots, retires recovered
    /// smoke, then fills missing ActiveAnim3..6 through the same slot owner.
    /// EstimatedHealth and Building's later House health sample stay unchanged.
    pub(crate) fn restore_building_art_after_power_up(&mut self, id: u64, rules: &RuleSet) {
        self.set_building_damage_state(id, false, rules);
        if let Some(entity) = self.substrate.entities.get(id)
            && let Some(object) = self.object_type(entity.type_ref(), rules)
            && entity
                .health
                .compare_ratio(object.strength, rules.general.condition_yellow)
                == Greater
        {
            self.substrate.retire_attached_damage_smoke(id);
        }
        for slot in 3..=6 {
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.building_art.anim_slots[slot].is_none())
            {
                self.set_building_anim_slot(id, slot as u8, false, false, 0, rules);
            }
        }
    }

    /// Original451EE0: store retained flag before ascending occupied-slot visits.
    /// A missing requested name leaves the old animation alive in that slot.
    pub(crate) fn set_building_damage_state(&mut self, id: u64, damaged: bool, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if !entity.building_art.set_damage_state(damaged) {
            return;
        }
        for slot in 0..21 {
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| e.building_art.anim_slots[slot].is_some())
            {
                self.set_building_anim_slot(id, slot as u8, damaged, false, 0, rules);
            }
        }
    }

    /// `TechnoClass::EngineerRepair @ 0x00701410` passes `GetHealthRatio <= ConditionYellow`
    /// to `0x00451EE0`; `ReceiveDamage` tests the same ratio at `0x00442AA3..0x00442AC4`.
    pub fn refresh_building_damage_state(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return;
        };
        let damaged = requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        );
        self.set_building_damage_state(id, damaged, rules);
    }

    /// Original458330, called by the garrison allegiance transition. Visit
    /// only live slots, in native order, and sample HP and occupant count
    /// again after each replacement:451890 can replace other slots too.
    /// Selection/caller evidence: tools/garrison_oracle/allegiance.py.
    pub(crate) fn refresh_garrison_anims(&mut self, id: u64, rules: &RuleSet) {
        for slot in [18, 3, 4, 5, 6] {
            let Some(entity) = self.substrate.entities.get(id) else {
                return;
            };
            if entity.building_art.anim_slots[slot].is_none() {
                continue;
            }
            let Some(object) = self.object_type(entity.type_ref(), rules) else {
                return;
            };
            let damaged = requested_damage_state(
                entity.health,
                object.strength,
                rules.general.condition_yellow,
            );
            let garrisoned = entity
                .passenger_role
                .cargo()
                .is_some_and(|cargo| !cargo.is_empty());
            self.set_building_anim_slot(id, slot as u8, damaged, garrisoned, 0, rules);
        }
    }

    /// Original451750. Empty selected names leave before451890 and therefore
    /// do not synchronize Building+6E6. Damaged takes precedence over garrison.
    pub fn set_building_anim_slot(
        &mut self,
        id: u64,
        slot: u8,
        damaged: bool,
        garrisoned: bool,
        delay: u16,
        rules: &RuleSet,
    ) -> Option<u64> {
        let config = self.building_anim_config(id, slot, rules)?;
        let name = if damaged {
            config.damaged_variant.as_ref()?.anim_type.clone()
        } else if garrisoned {
            config.garrisoned_variant.as_ref()?.anim_type.clone()
        } else {
            config.anim_type.clone()
        };
        if name.is_empty() {
            return None;
        }
        self.allocate_building_anim_slot(id, slot, &name, damaged, delay, &config, rules)
    }

    /// Original451890: synchronize other slots, construct new AnimClass, copy
    /// only old current_frame(+AC), clear/delete old slot, then install new.
    /// The new timer, loop count and first-AI guard belong to its constructor.
    fn allocate_building_anim_slot(
        &mut self,
        id: u64,
        slot: u8,
        name: &str,
        damaged: bool,
        delay: u16,
        config: &BuildingAnimConfig,
        rules: &RuleSet,
    ) -> Option<u64> {
        self.set_building_damage_state(id, damaged, rules);
        //427CB0 scans existing types; an art section alone is not a type.
        let canonical = name.to_ascii_uppercase();
        if !rules.anim_type_names.contains(&canonical) {
            return None;
        }
        // Building render coordinate virtual459EF0 precedes the slot offset.
        let world = self.building_anim_world_coord(id, config)?;
        let type_name = self.interner.intern(&name.to_ascii_uppercase());
        let mut descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
            type_name,
            0,
            0,
            crate::util::fixed_math::SIM_ZERO,
            crate::util::fixed_math::SIM_ZERO,
            0,
        );
        descriptor.delay = delay;
        descriptor.loop_count = 1;
        descriptor.draw_flags = 0x1600;
        let spawned = self.spawn_anim_at_world(rules, descriptor, world);
        let new_id = match spawned {
            Ok(new_id) => new_id,
            Err(error) => {
                // A registered AnimType whose sprite no archive holds. Native
                // still constructs the object, with no image to draw; VERA's
                // store refuses a type with no loader bounds, so the slot is
                // emptied and nothing is drawn. The native id above is spent
                // either way.
                //
                // RESIDUAL: natively the slot stays occupied by that invisible
                // object. `set_building_damage_state` revisits occupied slots
                // only, so a building whose damaged animation is unbound does
                // not get its normal animation back after repair, and the
                // unbound type takes no stable id. Retail reach: `CAMOV01`,
                // `CAMOV02`, `NAPSYA` (all `TechLevel=-1`); none authors
                // `RandomRate=`, so no draw is skipped.
                log::debug!("building {id} slot {slot} animation [{canonical}] not shown: {error}");
                self.clear_building_anim_slot(id, slot);
                return None;
            }
        };
        // Original45199B stores Anim+118 independently of the later55C slot
        // installation. Clearing the reference never clears this native flag.
        self.substrate
            .anims
            .get_mut(new_id)?
            .mark_building_anim(config.y_sort);
        let old = self.substrate.entities.get(id)?.building_art.anim_slots[usize::from(slot)];
        if let Some(frame) = old
            .and_then(|old_id| self.anim(old_id))
            .map(|anim| anim.runtime().current_frame)
        {
            self.substrate
                .anims
                .get_mut(new_id)
                .expect("new slot animation")
                .jump_to_frame(frame);
        }
        self.clear_building_anim_slot(id, slot);
        if let Some(anim) = self.substrate.anims.get_mut(new_id) {
            anim.enter_building_slot(id, slot, config.z_adjust);
        }
        self.substrate.entities.get_mut(id)?.building_art.anim_slots[usize::from(slot)] =
            Some(new_id);
        //451A36 calls4555D0 only when retained StuffEnabled6EA is true.
        //Construction/placement and operational history cannot substitute for it.
        let object = rules.object(
            self.interner
                .resolve(self.substrate.entities.get(id)?.type_ref()),
        )?;
        let enabled = self.substrate.entities.get(id)?.building_art.stuff_enabled
            && self.building_operational_state(id, rules) == Some(true);
        if !enabled && object.powered && self.building_anim_power_flags(id, slot, rules).powered {
            self.substrate.anims.get_mut(new_id)?.pause();
        }
        // 0x00451B17..0x00451B25: a building being erased (`vt+0x1D4`,
        // Techno `+0x270`) freezes the new anim.
        if self.substrate.entities.get(id)?.is_warped_out() {
            self.substrate.anims.get_mut(new_id)?.temporal_pause();
        }
        Some(new_id)
    }

    pub fn clear_building_anim_slot(&mut self, id: u64, slot: u8) {
        let old = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.building_art.anim_slots[usize::from(slot)].take());
        if let Some(old) = old {
            self.scalar_delete_anim(old);
        }
    }

    pub(crate) fn clear_all_building_anim_slots(&mut self, id: u64) {
        for slot in 0..21 {
            self.clear_building_anim_slot(id, slot);
        }
    }

    fn building_anim_delayed_fire(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let name = self.interner.resolve(entity.type_ref());
        rules
            .object(name)
            .and_then(|object| rules.art().resolve_metadata_entry(name, &object.image))
            .is_some_and(|entry| entry.is_anim_delayed_fire)
    }

    /// Anim arm of Building PointerExpired44E8F0. Production8 restores Idle18;
    /// Grinding's Special10 restores Active3 before the general slot callback.
    /// The reverse index skips listeners whose slot array cannot name this Anim.
    /// Evidence: tools/spatial_oracle/building_slot_replacement.expiry.{json,md}.
    pub(crate) fn building_anim_pointer_expired(&mut self, expired: u64, rules: &RuleSet) {
        let Some((owner, slot)) = self.anim(expired).and_then(|anim| anim.building_slot()) else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return;
        };
        let damaged = requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        );
        let grinding = object.grinding;
        if slot == 8 {
            self.set_building_anim_slot(owner, 18, damaged, false, 0, rules);
        }
        if slot == 10 && grinding {
            self.set_building_anim_slot(owner, 3, damaged, false, 0, rules);
        }
        self.building_anim_slot_expired(expired, rules);
    }

    /// Building451B40: find and clear the matched slot before constructing
    /// its successor. Anim+179 is normal completion, independent of damage.
    /// Scalar slot replacement/deletion does not call this UnInit receiver.
    fn building_anim_slot_expired(&mut self, expired: u64, rules: &RuleSet) {
        let Some((owner, slot, completed)) = self.anim(expired).and_then(|anim| {
            anim.is_building_anim()
                .then_some(anim.building_slot())
                .flatten()
                .map(|(owner, slot)| (owner, slot, anim.completed()))
        }) else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(owner) else {
            return;
        };
        if !entity.is_object_alive()
            || entity.building_art.anim_slots[usize::from(slot)] != Some(expired)
        {
            return;
        }
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return;
        };
        let unit_repair = object.unit_repair;
        let damaged = requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        );
        let servicing = entity.radio_contacts.iter_live().next().is_some()
            && entity.mission.effective()
                == crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Repair,
                );
        let garrisoned = entity
            .passenger_role
            .cargo()
            .is_some_and(|cargo| !cargo.passengers().is_empty());
        let delayed_fire = self.building_anim_delayed_fire(owner, rules);
        self.substrate.clear_building_anim_reference(expired);
        let next = match slot {
            10 if unit_repair => Some((if servicing { 11 } else { 18 }, false)),
            10 if delayed_fire && completed => Some((3, garrisoned)),
            12 if unit_repair && completed => Some((18, false)),
            15 if completed => Some((16, false)),
            17 if completed => Some((14, false)),
            _ => None,
        };
        if let Some((slot, garrisoned)) = next {
            self.set_building_anim_slot(owner, slot, damaged, garrisoned, 0, rules);
        }
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// Clear the source slot and its derived reverse index without generating
    /// a successor. Scalar deletion and defensive final removal use this owner.
    pub(crate) fn clear_building_anim_reference(&mut self, id: u64) {
        let slot = self
            .anims
            .get_mut(id)
            .and_then(|anim| anim.take_building_slot());
        if let Some((owner, slot)) = slot
            && let Some(entity) = self.entities.get_mut(owner)
            && entity.building_art.anim_slots[usize::from(slot)] == Some(id)
        {
            entity.building_art.anim_slots[usize::from(slot)] = None;
        }
    }
    /// Rebuild an index, never infer slot selection from HP or animation names.
    /// VERA-only: the Anims' load-time reverse slot indexes, from Building+55C
    /// and the damage-fire slots (+5C8), their source of truth.
    pub(crate) fn rebuild_building_anim_slot_indices(&mut self) {
        for anim in self.anims.values_mut() {
            anim.clear_slot_indexes();
        }
        let slots: Vec<_> = self
            .entities
            .values()
            .flat_map(|entity| {
                entity
                    .building_art
                    .anim_slots
                    .iter()
                    .enumerate()
                    .filter_map(move |(slot, id)| id.map(|id| (id, entity.stable_id(), slot as u8)))
            })
            .collect();
        for (id, owner, slot) in slots {
            self.anims
                .get_mut(id)
                .expect("snapshot admission validated every unique Building slot Anim")
                .index_building_slot(owner, slot);
        }
        for entity in self.entities.values() {
            for (slot, id) in entity.damage_fires.anim_ids().iter().enumerate() {
                if let Some(id) = id {
                    self.anims
                        .get_mut(*id)
                        .expect("snapshot admission validated every unique damage-fire Anim")
                        .index_damage_fire_slot(entity.stable_id(), slot as u8);
                }
            }
        }
    }
}

pub fn requested_damage_state(health: Health, strength: i32, yellow: f64) -> bool {
    health.compare_ratio(strength, yellow) != Greater
}

/// Completed CanBeOccupied body frame, original43EF90 (43F05C/43F089).
/// Kept in sim so receiver before/after checks and presentation share predicates.
pub fn occupied_body_frame(
    occupants: u32,
    current: i32,
    strength: i32,
    tech_level: i32,
    yellow: f64,
    red: f64,
) -> u16 {
    let health = Health::new(current);
    let mut frame = if occupants > 0 { 2 } else { 0 };
    if health.compare_ratio(strength, red) != Greater
        || (tech_level > 0 && health.compare_ratio(strength, yellow) != Greater)
    {
        frame += 1;
    }
    if tech_level == -1 && frame == 3 {
        1
    } else {
        frame
    }
}

/// Raw inputs to original Building GetCurrentFrame43EF90. Native frame arithmetic
/// stays signed32 until the rendering adapter validates an available SHP frame.
#[derive(Clone, Copy, Debug)]
pub struct BodyFrameInput {
    pub state: i32,
    pub base_frame: i32,
    pub laser_frame: Option<i32>,
    pub firestorm_frame: Option<i32>,
    pub gate_stages: Option<i32>,
    pub occupants: Option<i32>,
    pub tech_level: i32,
    pub selling: bool,
    pub buildup: [i32; 2],
    pub ordinary: [[i32; 2]; 4],
}

pub fn body_frame(
    input: BodyFrameInput,
    health: Health,
    strength: i32,
    yellow: f64,
    red: f64,
) -> i32 {
    if let Some(frame) = input.laser_frame {
        return frame;
    }
    if let Some(frame) = input.firestorm_frame {
        return frame;
    }
    let mut frame = input.base_frame;
    if input.state == 0 {
        let end = input.buildup[0].wrapping_add(input.buildup[1]);
        if input.gate_stages.is_some() {
            frame = end.wrapping_sub(frame).wrapping_sub(1);
        }
        if input.selling {
            frame = end.wrapping_sub(frame).wrapping_sub(1);
        }
        return frame;
    }
    if let Some(occupants) = input.occupants {
        return i32::from(occupied_body_frame(
            u32::from(occupants > 0),
            health.current(),
            strength,
            input.tech_level,
            yellow,
            red,
        ));
    }
    let damaged = requested_damage_state(health, strength, yellow);
    if let Some(stages) = input.gate_stages {
        return if damaged { stages.wrapping_add(1) } else { 0 };
    }
    if damaged {
        let offset = if input.state == 1 {
            1
        } else {
            input
                .ordinary
                .map(|[start, count]| start.wrapping_add(count))
                .into_iter()
                .max()
                .expect("four native body ranges")
        };
        frame = frame.wrapping_add(offset);
    }
    frame
}

/// Building DrawBody's gate branch (`0x0043D546..0x0043D68F`): a building
/// on Mission Open (Get_Mission, vt+0x184) whose door (`+0x350`) is opening,
/// closing or open draws this frame instead of GetCurrentFrame's. The frame
/// is `ftol(progress * GateStages)` of the door's drawing progress
/// (`0x004A52F0`, [`crate::sim::door::DoorClass::draw_progress`]), counted
/// back from GateStages while closing, `GateStages - 1` once open, clamped
/// to `0..GateStages`, plus `GateStages + 1` at ConditionYellow health or
/// below (`0x005F5C60` against Rules `+0x1700`). The branch then returns, so
/// no bib is drawn. None outside it.
///
/// RESIDUAL: the branch lights the body without the type's `ExtraLight=`
/// (its vt+0x1D0 call adds no `+0x1548`); VERA keeps the ordinary body
/// light. Retail's only gate, GAGATE_A, sets no ExtraLight.
pub fn gate_open_body_frame(
    entity: &GameEntity,
    gate_stages: i32,
    strength: i32,
    yellow: f64,
    frame: u32,
) -> Option<i32> {
    use crate::sim::door::DoorPhase;
    if entity.mission.effective().known() != Some(crate::sim::mission::MissionType::Open) {
        return None;
    }
    let phase = entity.door_phase();
    let mut stage = match phase {
        DoorPhase::ClosedStable => return None,
        DoorPhase::OpenStable => gate_stages.wrapping_sub(1),
        DoorPhase::Opening | DoorPhase::Closing => {
            let done = (entity.door_draw_progress(frame) * f64::from(gate_stages)) as i32;
            if phase == DoorPhase::Closing {
                gate_stages.wrapping_sub(done)
            } else {
                done
            }
        }
    };
    if stage >= gate_stages {
        stage = gate_stages.wrapping_sub(1);
    }
    stage = stage.max(0);
    let damaged = requested_damage_state(entity.health, strength, yellow);
    Some(if damaged {
        stage.wrapping_add(gate_stages.wrapping_add(1))
    } else {
        stage
    })
}

/// The image `BuildingClass::GetImage @ 0x004513D0` hands DrawBody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingImage {
    /// The type's own image (vt+0x9C, `0x0045F040`).
    Body,
    /// The type's Buildup SHP (vt+0xC0, `0x00465960`).
    Buildup,
    /// A null Buildup: DrawBody returns before drawing anything
    /// (`0x0043D2BB`).
    Null,
}

/// `BuildingClass::GetImage @ 0x004513D0`: the type's image once BState
/// (+0x534) is nonzero and OnConstructionComplete has set +0x6E4
/// (`0x00446A6E`); its Buildup until then, whatever the BState, so a human
/// player's placement draws Buildup in its idle frame before Construction
/// begins. Either image is drawn at GetCurrentFrame ([`body_frame`]).
pub fn building_image(entity: &GameEntity, has_buildup: bool) -> BuildingImage {
    if !entity.in_construction_bstate() && entity.building_art.actually_placed() {
        BuildingImage::Body
    } else if has_buildup {
        BuildingImage::Buildup
    } else {
        BuildingImage::Null
    }
}

/// Equality key for health-only receiver continuation442B37. Represented
/// building body states are construction(0) and completed(1). Native fences
/// and construction read no health, so their unchanged frame cancels here.
/// This key is not a rendering frame or a replacement for their body timers.
pub(crate) fn receiver_body_frame(
    entity: &GameEntity,
    object: &ObjectType,
    rules: &RuleSet,
    tech_level: i32,
) -> i32 {
    let art = rules
        .art()
        .resolve_metadata_entry(&object.id, &object.image);
    body_frame(
        BodyFrameInput {
            state: if entity.in_construction_bstate() {
                0
            } else {
                1
            },
            base_frame: 0,
            laser_frame: object.laser_fence.then_some(0),
            firestorm_frame: object.firestorm_wall.then_some(0),
            gate_stages: object
                .gate
                .then_some(art.map_or(9, |art| art.building_gate_stages)),
            occupants: object.can_be_occupied.then_some(
                entity
                    .passenger_role
                    .cargo()
                    .map_or(0, |cargo| cargo.passengers().len() as i32),
            ),
            tech_level,
            selling: false,
            buildup: [0, 1],
            ordinary: art.map_or([[0, 1]; 4], |art| {
                art.building_body_ranges
                    .map(|[start, count, _]| [start, count])
            }),
        },
        entity.health,
        object.strength,
        rules.general.condition_yellow,
        rules.general.condition_red,
    )
}

#[cfg(test)]
mod body_tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Corpus {
        rows: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        input: Input,
        output: Output,
    }
    #[derive(Deserialize)]
    struct Input {
        before: i32,
        after: i32,
        strength: i32,
        old_flag: u8,
        yellow_bits: String,
        red_bits: String,
        gate: bool,
        laser_fence: bool,
        firestorm_wall: bool,
        can_be_occupied: bool,
        state: i32,
        base_frame: i32,
        gate_stages: i32,
        laser_frame: i32,
        firestorm_frame: i32,
        occupants: i32,
        tech_level: i32,
        mission: i32,
        queued_mission: i32,
        buildup_start: i32,
        buildup_count: i32,
        frame_pairs: [[i32; 2]; 4],
    }
    #[derive(Deserialize)]
    struct Output {
        before_frame: u32,
        after_frame: u32,
        retained_flag: u8,
        dirty: u8,
    }
    #[test]
    fn whole_original_body_and_second_receiver_transition_435_rows() {
        let corpus: Corpus = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_body_transition.json",
        ))
        .unwrap();
        assert_eq!(corpus.rows.len(), 435);
        for (index, row) in corpus.rows.into_iter().enumerate() {
            let i = row.input;
            let yellow = f64::from_bits(u64::from_str_radix(&i.yellow_bits, 16).unwrap());
            let red = f64::from_bits(u64::from_str_radix(&i.red_bits, 16).unwrap());
            let input = BodyFrameInput {
                state: i.state,
                base_frame: i.base_frame,
                laser_frame: i.laser_fence.then_some(i.laser_frame),
                firestorm_frame: i.firestorm_wall.then_some(i.firestorm_frame),
                gate_stages: i.gate.then_some(i.gate_stages),
                occupants: i.can_be_occupied.then_some(i.occupants),
                tech_level: i.tech_level,
                selling: if i.mission == -1 {
                    i.queued_mission
                } else {
                    i.mission
                } == 19,
                buildup: [i.buildup_start, i.buildup_count],
                ordinary: i.frame_pairs,
            };
            let before = body_frame(input, Health::new(i.before), i.strength, yellow, red);
            let after = body_frame(input, Health::new(i.after), i.strength, yellow, red);
            assert_eq!(
                (before as u32, after as u32),
                (row.output.before_frame, row.output.after_frame),
                "native row {index}: {input:?}"
            );
            let changed = before != after;
            assert_eq!(u8::from(changed), row.output.dirty, "native row {index}");
            let retained = if changed {
                requested_damage_state(Health::new(i.after), i.strength, yellow)
            } else {
                i.old_flag != 0
            };
            assert_eq!(
                u8::from(retained),
                row.output.retained_flag,
                "native row {index}"
            );
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn slot_test_fixture() -> (Simulation, RuleSet, u64) {
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
    let art_ini = IniFile::from_str(
        "[B]\nActiveAnim=N\nActiveAnimDamaged=D\nActiveAnimGarrisoned=G\n[N]\nLoopCount=-1\nRate=150\nRandomRate=450,150\nStopSound=OLDSTOP\n[D]\nLoopCount=-1\nRate=300\nRandomRate=900,300\n[G]\nLoopCount=-1\nRate=450\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[BuildingTypes]\n0=B\n[B]\nStrength=100\n[Animations]\n0=N\n1=D\n2=G\n",
        ),
        &art_ini,
    )
    .unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    for name in ["N", "D", "G"] {
        art.bind_anim_frame_count_for_test(name, 40);
    }
    rules.install_art_data(art);
    let mut sim = Simulation::new();
    let id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default_of_category(
        id,
        "B",
        "A",
        2,
        2,
        crate::map::entities::EntityCategory::Structure,
    );
    entity.set_type_ref_for_test(sim.interner.intern("B"));
    entity.set_owner_for_test(sim.interner.intern("A"));
    entity.health.set_for_test(100);
    sim.substrate.entities.insert(entity);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    (sim, rules, id)
}

#[cfg(test)]
mod slot_tests {
    use super::*;

    #[test]
    fn installed_normal_name_allocates_with_constructor_metadata_when_host_has_no_art_section() {
        use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
        let (mut sim, _, id) = slot_test_fixture();
        let art_ini = IniFile::from_str("[N]\nLoopCount=-1\nRate=150\n");
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[BuildingTypes]\n0=B\n[B]\nStrength=100\nUpgrades=1\n[Animations]\n0=N\n",
            ),
            &art_ini,
        )
        .unwrap();
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("N", 40);
        assert!(art.get("B").is_none());
        rules.install_art_data(art);
        let ty = sim.entities().get(id).unwrap().type_ref();
        sim.set_building_upgrade_art_name(ty, 0, "N", &rules);
        let anim = sim
            .set_building_anim_slot(id, 0, false, false, 0, &rules)
            .expect(
                "PowerUp uses the retained constructor record, without requiring an ART section",
            );
        assert_eq!(sim.anim(anim).unwrap().building_slot(), Some((id, 0)));
        assert_eq!(sim.interner.resolve(sim.anim(anim).unwrap().type_id()), "N");
    }

    #[test]
    fn bunker_and_active_slot_sort_adjustments_match_original_read_and_copy() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ini_parser::IniFile;
        let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_slot_replacement.display.json",
        ))
        .unwrap();
        let mut art_text = String::new();
        for (section, values) in golden["art"]["physical_sections"].as_object().unwrap() {
            art_text.push_str(&format!("[{section}]\n"));
            for (key, value) in values.as_object().unwrap() {
                art_text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
        }
        // This regression isolates the slot field transfer; the supplied
        // frame bounds make those retained Anim instances constructible.
        // Their frame timing and pixels are not native goldens in this corpus.
        let mut names = std::collections::BTreeSet::new();
        for row in golden["rows"].as_array().unwrap() {
            for variant in ["normal", "damaged", "garrisoned"] {
                let name = row["slot"][variant].as_str().unwrap();
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
        }
        let mut registry = String::from(
            "[BuildingTypes]\n0=NATBNK\n1=CAOILD\n2=GAHPAD\n\
            [NATBNK]\nStrength=100\n[CAOILD]\nStrength=100\n[GAHPAD]\nStrength=100\n[Animations]\n",
        );
        for (index, name) in names.iter().enumerate() {
            registry.push_str(&format!("{index}={name}\n"));
            art_text.push_str(&format!("[{name}]\nLayer=ground\nLoopCount=-1\n"));
        }
        let art_ini = IniFile::from_str(&art_text);
        let mut rules =
            RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&registry), &art_ini)
                .unwrap();
        let mut art = ArtRegistry::from_ini(&art_ini);
        for name in &names {
            art.bind_anim_frame_count_for_test(name, 40);
        }
        rules.install_art_data(art);
        for row in golden["rows"].as_array().unwrap() {
            for damaged in [false, true] {
                let mut sim = Simulation::new();
                let name = row["building"].as_str().unwrap();
                let id = sim.substrate.allocate_stable_id();
                let mut building = GameEntity::test_default_of_category(
                    id,
                    name,
                    "A",
                    64,
                    60,
                    crate::map::entities::EntityCategory::Structure,
                );
                building.set_type_ref_for_test(sim.interner.intern(name));
                building.set_owner_for_test(sim.interner.intern("A"));
                building.health.set_for_test(if damaged { 40 } else { 100 });
                sim.substrate.entities.insert(building);
                let slot = row["slot"]["slot"].as_u64().unwrap() as u8;
                let anim_id = sim
                    .set_building_anim_slot(id, slot, damaged, false, 0, &rules)
                    .unwrap();
                let anim = sim.anim(anim_id).unwrap();
                let expected = &row["copied"];
                assert!(anim.is_building_anim(), "{row}");
                assert_eq!(
                    anim.display().y_sort_adjust(),
                    expected["y_sort"].as_i64().unwrap() as i32,
                    "original4519A2 must replace the Anim type's sort adjustment: {row}, damaged={damaged}"
                );
                assert_eq!(
                    anim.z_adjust(),
                    expected["z_adjust"].as_i64().unwrap() as i32
                );
                assert_eq!(
                    sim.interner.resolve(anim.type_id()),
                    row["slot"][if damaged { "damaged" } else { "normal" }]
                        .as_str()
                        .unwrap()
                );
                let key = crate::sim::anim_class::anim_own_sort_term(anim);
                assert_eq!(
                    key,
                    anim.world_coord()
                        .x
                        .wrapping_add(anim.world_coord().y)
                        .wrapping_add(expected["y_sort"].as_i64().unwrap() as i32)
                );
                let config = rules
                    .art()
                    .get(name)
                    .unwrap()
                    .building_anims
                    .iter()
                    .find(|config| config.native_slot == slot)
                    .unwrap();
                assert_eq!(config.x, row["slot"]["x"].as_i64().unwrap() as i32);
                assert_eq!(config.y, row["slot"]["y"].as_i64().unwrap() as i32);
            }
        }
    }

    #[test]
    fn shared_bounds_apply_to_slot_create_replace_and_reveal_reposition() {
        use crate::sim::world::{
            PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, UninitContext,
        };
        let (mut sim, mut rules, id) = slot_test_fixture();
        sim.session.set_pixel_conversion_bounds_for_test(
            crate::util::pixel_conversion::PixelConversionBounds {
                width: 640,
                height: 480,
            },
        );
        let config = rules
            .art_entry_mut_for_test("B")
            .unwrap()
            .building_anims
            .iter_mut()
            .find(|config| config.native_slot == 3)
            .unwrap();
        config.x = 640;
        config.y = 0;
        let first = sim
            .set_building_anim_slot(id, 3, false, false, 0, &rules)
            .unwrap();
        assert_eq!(sim.anim(first).unwrap().world_coord().x, 512);
        sim.set_building_damage_state(id, true, &rules);
        let replaced = sim.entities().get(id).unwrap().building_art.anim_slots[3].unwrap();
        assert_eq!(sim.anim(replaced).unwrap().world_coord().x, 512);
        let timer = sim.anim(replaced).unwrap().runtime().frame_timer;
        // This synthetic owner has never been marked; enter through the real
        // coordinate commit and verify its already retained slot moves in place.
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.lifecycle.set_in_limbo_for_test(true);
        entity.lifecycle.set_cell_marked_for_test(false);
        let outcome = sim.try_reveal_entity_with_context(
            id,
            RevealRequest {
                position: RevealPosition::new(
                    3,
                    4,
                    0,
                    None,
                    crate::util::fixed_math::SimFixed::from_num(128),
                    crate::util::fixed_math::SimFixed::from_num(128),
                ),
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::with_rules(&rules),
        );
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));
        assert_eq!(
            sim.entities().get(id).unwrap().building_art.anim_slots[3],
            Some(replaced)
        );
        assert_eq!(sim.anim(replaced).unwrap().world_coord().x, 768);
        assert_eq!(sim.anim(replaced).unwrap().world_coord().y, 1024);
        assert_eq!(sim.anim(replaced).unwrap().runtime().frame_timer, timer);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(
            restored.session.pixel_conversion_bounds(),
            sim.session.pixel_conversion_bounds()
        );
        assert_eq!(
            restored.anim(replaced).unwrap().world_coord(),
            sim.anim(replaced).unwrap().world_coord()
        );
    }

    /// A registered AnimType whose art body was read but whose sprite no archive
    /// holds (the tolerant binder skipped it) must leave the slot empty, not
    /// panic. Retail reaches this for building animations whose files never
    /// shipped; it used to abort the match when such a building changed state.
    #[test]
    fn unbound_registered_slot_animation_leaves_the_slot_empty() {
        use crate::rules::native_processing::RulesLayerStack;
        use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=B\n[B]\nStrength=100\n[Animations]\n0=EARLY\n1=GHOST\n",
        );
        let art_ini = IniFile::from_str(
            "[B]\nActiveAnim=EARLY\nActiveAnimTwo=GHOST\n\
             [EARLY]\nRate=300\nLoopCount=-1\n[GHOST]\nRate=300\nLoopCount=-1\n",
        );
        let processed = RulesLayerStack::new(ini)
            .process_with_fixed_art(&art_ini)
            .unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("EARLY", 40);
        rules.install_art_data(art);
        assert!(
            rules
                .art()
                .anim_runtime_config("GHOST")
                .is_some_and(|config| config.art_body_read && config.raw_shp_frame_count.is_none()),
            "fixture: GHOST is registered and read, but has no sprite"
        );
        let (mut sim, _, id) = slot_test_fixture();

        assert!(
            sim.set_building_anim_slot(id, 3, false, false, 0, &rules)
                .is_some()
        );
        assert!(
            sim.set_building_anim_slot(id, 4, false, false, 0, &rules)
                .is_none(),
            "no sprite, no slot animation, no panic"
        );
        let slots = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .building_art
            .anim_slots;
        assert!(slots[3].is_some());
        assert!(slots[4].is_none());
    }

    #[test]
    fn canonical_art_read_receipt_controls_real_slot_constructor_and_logic() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=B\n[B]\nStrength=100\nExplosion=LATE,MISSING\n[Animations]\n0=EARLY\n",
        );
        let art_ini = IniFile::from_str(
            "[B]\nActiveAnim=EARLY\nActiveAnimTwo=LATE\nActiveAnimThree=MISSING\nActiveAnimFour=UNREGISTERED\n[EARLY]\nRate=300\nLoopCount=-1\n[LATE]\nRate=300\nLoopCount=-1\n[UNREGISTERED]\nRate=900\n",
        );
        let mut layers = RulesLayerStack::new(ini);
        for next_pass in [false, true] {
            if next_pass {
                layers.push(RulesLayerKind::Scenario, IniFile::from_str(""));
            }
            let processed = layers.process_with_fixed_art(&art_ini).unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            let mut art = ArtRegistry::from_ini(&art_ini);
            for name in ["EARLY", "LATE", "UNREGISTERED"] {
                art.bind_anim_frame_count_for_test(name, 40);
            }
            rules.install_art_data(art);
            let (mut sim, _, id) = slot_test_fixture();
            let before_rng = sim.scenario_rng.logical_state();
            let early = sim
                .set_building_anim_slot(id, 3, false, false, 0, &rules)
                .unwrap();
            let late = sim
                .set_building_anim_slot(id, 4, false, false, 0, &rules)
                .unwrap();
            let missing = sim
                .set_building_anim_slot(id, 5, false, false, 0, &rules)
                .unwrap();
            assert!(
                sim.set_building_anim_slot(id, 6, false, false, 0, &rules)
                    .is_none()
            );
            assert_eq!(sim.anim(early).unwrap().runtime().rate_reload, 3);
            assert_eq!(
                sim.anim(late).unwrap().runtime().rate_reload,
                if next_pass { 3 } else { 1 }
            );
            assert_eq!(
                rules
                    .art()
                    .anim_runtime_config("LATE")
                    .unwrap()
                    .art_body_read,
                next_pass
            );
            assert_eq!(
                rules
                    .art()
                    .anim_runtime_config("MISSING")
                    .unwrap()
                    .raw_shp_frame_count,
                None
            );
            assert_eq!(sim.anim(missing).unwrap().effective_end(), 0);
            assert_eq!(sim.scenario_rng.logical_state(), before_rng);
            sim.visit_anim(
                missing,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            assert!(!sim.anim(missing).unwrap().runtime().first_ai_guard);
            assert!(!sim.anim(missing).unwrap().runtime().inactive);
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame() + 1);
            sim.visit_anim(
                missing,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            assert!(sim.substrate.pending_delete.contains_object(&missing));
            assert!(!sim.anim(missing).unwrap().in_logic_vector());
            assert_eq!(
                sim.entities().get(id).unwrap().building_art.anim_slots[5],
                None
            );
        }
    }
    #[test]
    fn replacement_copies_only_frame_deletes_synchronously_and_keeps_constructor_rng() {
        let (mut sim, rules, id) = slot_test_fixture();
        let old = sim
            .set_building_anim_slot(id, 3, false, false, 0, &rules)
            .unwrap();
        sim.substrate
            .anims
            .get_mut(old)
            .unwrap()
            .runtime_mut_for_test()
            .current_frame = 17;
        sim.substrate
            .anims
            .get_mut(old)
            .unwrap()
            .runtime_mut_for_test()
            .first_ai_guard = false;
        sim.substrate
            .anims
            .get_mut(old)
            .unwrap()
            .runtime_mut_for_test()
            .loop_remaining = 8;
        sim.substrate
            .anims
            .get_mut(old)
            .unwrap()
            .set_start_sound_active_for_test(true);
        sim.session.set_binary_frame_for_test(91);
        let before_rng = sim.scenario_rng.logical_state();
        sim.set_building_damage_state(id, true, &rules);
        let new = sim.entities().get(id).unwrap().building_art.anim_slots[3].unwrap();
        assert_ne!(old, new);
        assert!(sim.anim(old).is_none());
        assert!(!sim.substrate.pending_delete.contains_object(&old));
        let anim = sim.anim(new).unwrap();
        assert_eq!(sim.interner.resolve(anim.type_id()), "D");
        assert_eq!(anim.runtime().current_frame, 17);
        assert!(anim.runtime().first_ai_guard);
        assert_eq!(anim.runtime().loop_remaining, 255);
        assert_eq!(anim.owner_entity(), None);
        assert_eq!(anim.building_slot(), Some((id, 3)));
        assert_eq!(anim.draw_flags(), 0x1600);
        assert_ne!(before_rng, sim.scenario_rng.logical_state());
        assert!(sim.sound_events.iter().any(|event| matches!(event,
            crate::sim::world::SimSoundEvent::ObjectSoundReleased{owner} if *owner==old)));
        assert!(!sim.sound_events.iter().any(|event| matches!(event,
            crate::sim::world::SimSoundEvent::AnimationStopped{anim_id,stop_sound_id:Some(_),..} if *anim_id==old)));
        let unchanged = sim.state_hash();
        sim.set_building_damage_state(id, true, &rules);
        assert_eq!(sim.state_hash(), unchanged);
    }
    #[test]
    fn slot_uninit_clears_reference_before_deferred_physical_expiry() {
        let (mut sim, rules, id) = slot_test_fixture();
        let anim = sim
            .set_building_anim_slot(id, 3, false, false, 0, &rules)
            .unwrap();
        sim.destroy_anim(anim, &rules);
        // Object UnInit5F6616 broadcasts before Limbo/storage removal;
        // Building44EA3A→451B40 clears the matched slot synchronously.
        assert_eq!(
            sim.entities().get(id).unwrap().building_art.anim_slots[3],
            None
        );
        assert!(sim.substrate.pending_delete.contains_object(&anim));
        assert!(!sim.anim(anim).unwrap().in_logic_vector());
        sim.process_pending_delete();
        assert!(sim.anim(anim).is_none());
        assert_eq!(
            sim.entities().get(id).unwrap().building_art.anim_slots[3],
            None
        );
    }

    /// Whole original ObjectUnInit5F65F0 retains ordinary slot3 and its
    /// live Anim (owner+CC=0, inactive+19B=0). BuildingDestructor43BDC5
    /// calls ClearAnimSlot451E40(-2) only at deferred scalar destruction.
    /// Executed control: ordinary-building-destruction-research/continuation-1
    /// cleanup-registered-output.json, gamemd.
    #[test]
    fn building_uninit_retains_ordinary_slots_until_scalar_destruction() {
        let (mut sim, rules, id) = slot_test_fixture();
        let anim_id = sim
            .set_building_anim_slot(id, 3, false, false, 0, &rules)
            .unwrap();
        let before_rng = (
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let before_logic = sim.logic_order().to_vec();
        sim.uninit_with_rules(id, &rules);

        let building = sim.entities().get(id).expect("deferred Building");
        assert!(!building.lifecycle.object_alive());
        assert!(building.lifecycle.in_limbo());
        assert_eq!(building.building_art.anim_slots[3], Some(anim_id));
        let anim = sim.anim(anim_id).expect("slot survives Building UnInit");
        assert_eq!(anim.owner_entity(), None);
        assert_eq!(anim.building_slot(), Some((id, 3)));
        assert!(anim.in_logic_vector());
        assert!(!anim.runtime().inactive);
        assert_eq!(sim.logic_order(), before_logic);
        assert_eq!(
            (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            before_rng
        );
        assert!(sim.substrate.pending_delete.contains_object(&id));
        assert!(!sim.substrate.pending_delete.contains_object(&anim_id));

        sim.process_pending_delete();
        assert!(sim.entities().get(id).is_none());
        assert!(sim.anim(anim_id).is_none());
        assert!(!sim.logic_order().contains(&anim_id));
        assert!(!sim.substrate.pending_delete.contains_object(&anim_id));
        assert_eq!(
            (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            before_rng
        );
    }
    #[test]
    fn construction_initializes_real_slot_once_and_health_write_retains_it() {
        let (mut sim, rules, id) = slot_test_fixture();
        sim.grand_opening(
            id,
            false,
            true,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let old = sim.entities().get(id).unwrap().building_art.anim_slots[3].unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(25);
        sim.grand_opening(
            id,
            false,
            true,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.entities().get(id).unwrap().building_art.anim_slots[3],
            Some(old)
        );
        assert!(
            !sim.entities()
                .get(id)
                .unwrap()
                .building_art
                .damage_state_active
        );
        sim.refresh_building_damage_state(id, &rules);
        assert_ne!(
            sim.entities().get(id).unwrap().building_art.anim_slots[3],
            Some(old)
        );
    }
}

#[cfg(test)]
mod native_slot_tests {
    use super::*;
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
    #[test]
    fn original_420_scalar_replacements_copy_frame_without_copying_constructor_state() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_slot_replacement.json",
        ))
        .unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 420);
        for row in rows.as_array().unwrap() {
            let input = &row["input"];
            let output = &row["output"];
            let slot = input["slot"].as_u64().unwrap() as u8;
            let has_old = input["has_old"].as_bool().unwrap();
            let old_frame = input["old_frame"].as_i64().unwrap() as i32;
            let reverse = input["new_frame"] == 23;
            let (mut sim, _, id) = slot_test_fixture();
            let art_ini = IniFile::from_str(&format!(
                "[B]\nActiveAnim=N\nPowerUp1Anim=N\nPowerUp2Anim=N\nPowerUp3Anim=N\n\
                 [N]\nLoopCount=-1\nRate=300\nLoopEnd=24\nReverse={}\n",
                if reverse { "yes" } else { "no" }
            ));
            let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(
                    "[BuildingTypes]\n0=B\n[B]\nStrength=100\nUpgrades=3\n[Animations]\n0=N\n",
                ),
                &art_ini,
            )
            .unwrap();
            let mut art = ArtRegistry::from_ini(&art_ini);
            art.bind_anim_frame_count_for_test("N", 40);
            art.get_mut("B")
                .unwrap()
                .building_anims
                .retain(|config| config.native_slot == 3);
            art.get_mut("B").unwrap().building_anims[0].native_slot = slot;
            rules.install_art_data(art);
            let old = if has_old {
                let old = sim
                    .set_building_anim_slot(id, slot, false, false, 0, &rules)
                    .unwrap();
                let runtime = sim
                    .substrate
                    .anims
                    .get_mut(old)
                    .unwrap()
                    .runtime_mut_for_test();
                runtime.current_frame = old_frame;
                runtime.loop_remaining = 8;
                runtime.first_ai_guard = false;
                runtime.frame_timer = crate::sim::timer::CdTimer::started(7, 13);
                Some(old)
            } else {
                None
            };
            sim.session.set_binary_frame_for_test(91);
            let new = sim
                .set_building_anim_slot(id, slot, false, false, 0, &rules)
                .unwrap();
            let anim = sim.anim(new).unwrap();
            assert_eq!(
                anim.runtime().current_frame,
                output["frame"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                anim.runtime().loop_remaining,
                output["loop_remaining"].as_u64().unwrap() as u8
            );
            assert_eq!(anim.runtime().first_ai_guard, output["first_guard"] == 1);
            assert_eq!(anim.runtime().frame_timer.start_frame(), 91);
            assert_eq!(anim.runtime().frame_timer.duration(), 3);
            assert_eq!(anim.building_slot(), Some((id, slot)));
            assert_eq!(
                sim.entities().get(id).unwrap().building_art.anim_slots[usize::from(slot)],
                Some(new)
            );
            if let Some(old) = old {
                assert!(sim.anim(old).is_none());
            }
        }
    }

    #[test]
    fn original_36_retained_transitions_replace_only_selected_occupied_slots() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_art_transition.json",
        ))
        .unwrap();
        let mut checked = 0;
        for row in rows
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["input"]["source"] != "selfheal")
        {
            let (mut sim, _, id) = slot_test_fixture();
            let input = &row["input"];
            let mut text = String::from("[B]\nActiveAnim=OLD\n");
            // Slots0..2 are retained native BuildingType records. Supply the
            // same raw priors to that reader, not only the legacy ART view.
            for slot in input["slots"].as_array().unwrap() {
                let n = slot["slot"].as_u64().unwrap() as usize;
                if n < 3 {
                    if slot["normal"] == true {
                        text.push_str(&format!("PowerUp{}Anim=N{n:02}\n", n + 1));
                    }
                    if slot["damaged"] == true {
                        text.push_str(&format!("PowerUp{}DamagedAnim=D{n:02}\n", n + 1));
                    }
                }
            }
            text.push_str("[OLD]\nLoopCount=-1\n");
            for slot in 0..21 {
                text.push_str(&format!(
                    "[N{slot:02}]\nLoopCount=-1\n[D{slot:02}]\nLoopCount=-1\n"
                ));
            }
            let mut art = ArtRegistry::from_ini(&IniFile::from_str(&text));
            let mut registry = String::from(
                "[BuildingTypes]\n0=B\n[B]\nStrength=100\nUpgrades=3\n[Animations]\n0=OLD\n",
            );
            for slot in 0..21 {
                registry.push_str(&format!(
                    "{}=N{slot:02}\n{}=D{slot:02}\n",
                    slot * 2 + 1,
                    slot * 2 + 2
                ));
            }
            let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(&registry),
                &IniFile::from_str(&text),
            )
            .unwrap();
            for name in ["OLD".to_string()]
                .into_iter()
                .chain((0..21).flat_map(|slot| [format!("N{slot:02}"), format!("D{slot:02}")]))
            {
                art.bind_anim_frame_count_for_test(&name, 200);
            }
            let template = art
                .get("B")
                .unwrap()
                .building_anims
                .iter()
                .find(|config| config.native_slot == 3)
                .unwrap()
                .clone();
            let mut configs = Vec::new();
            for slot in input["slots"].as_array().unwrap() {
                let n = slot["slot"].as_u64().unwrap() as usize;
                let mut config = template.clone();
                config.native_slot = n as u8;
                config.anim_type = if slot["normal"] == true {
                    format!("N{n:02}")
                } else {
                    String::new()
                };
                let variant = crate::rules::art_data::BuildingAnimVariantConfig {
                    anim_type: format!("D{n:02}"),
                    loop_start: 0,
                    loop_end: 0,
                    loop_count: -1,
                    rate: 0,
                    start_frame: 0,
                    ping_pong: false,
                };
                config.damaged_variant = (slot["damaged"] == true).then_some(variant);
                configs.push(config);
            }
            art.get_mut("B").unwrap().building_anims = configs;
            rules.install_art_data(art);
            let mut old = [None; 21];
            for slot in input["slots"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|slot| slot["occupied"] == true)
            {
                let n = slot["slot"].as_u64().unwrap() as usize;
                let name = sim.interner.intern("OLD");
                let descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
                    name,
                    2,
                    2,
                    crate::util::fixed_math::SIM_ZERO,
                    crate::util::fixed_math::SIM_ZERO,
                    0,
                );
                let anim = sim.spawn_anim_object(&rules, descriptor).unwrap();
                sim.substrate
                    .anims
                    .get_mut(anim)
                    .unwrap()
                    .runtime_mut_for_test()
                    .current_frame = n as i32 + 123;
                sim.substrate
                    .anims
                    .get_mut(anim)
                    .unwrap()
                    .set_building_slot_for_test(Some((id, n as u8)));
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .building_art
                    .anim_slots[n] = Some(anim);
                old[n] = Some(anim);
            }
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .building_art
                .damage_state_active = input["old_flag"] == 1;
            sim.set_building_damage_state(id, input["requested"] == 1, &rules);
            assert_eq!(
                sim.entities()
                    .get(id)
                    .unwrap()
                    .building_art
                    .damage_state_active,
                row["output"]["retained_flag"] == 1,
                "{row}"
            );
            let replacements = row["output"]["replacements"].as_array().unwrap();
            let mut previous_new = None;
            for (n, &old_slot) in old.iter().enumerate() {
                let new = sim.entities().get(id).unwrap().building_art.anim_slots[n];
                if let Some(expected) = replacements.iter().find(|r| r["slot"] == n) {
                    let anim = sim.anim(new.unwrap()).unwrap();
                    assert_ne!(new, old_slot, "{row}");
                    assert!(sim.anim(old_slot.unwrap()).is_none());
                    assert_eq!(
                        sim.interner.resolve(anim.type_id()),
                        expected["name"].as_str().unwrap()
                    );
                    assert_eq!(anim.runtime().current_frame, n as i32 + 123);
                    assert!(anim.runtime().first_ai_guard);
                    assert!(previous_new.is_none_or(|previous| previous < anim.stable_id()));
                    previous_new = Some(anim.stable_id());
                } else {
                    assert_eq!(new, old_slot, "native row retains slot{n}: {row}");
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 36);
    }
}
