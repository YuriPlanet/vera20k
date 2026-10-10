//! Building health-triggered animation state. Building+6E6 is retained state,
//! independent of actual HP. Original451EE0/442A95/4508D7 write it before
//! replacing occupied animation slots; selfheal never calls this owner.

use crate::rules::art_data::BuildingAnimConfig;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;
use crate::util::native_x87::MaskedX87Ordering::Greater;

#[path = "building_art_admission.rs"]
mod admission;
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
pub(crate) use storage::BuildingStorage;

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
            .pixel_conversion_bounds
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
        if entity.category != crate::map::entities::EntityCategory::Structure {
            return;
        }
        let slots = entity.building_anim_slots;
        for (slot, anim_id) in slots.into_iter().enumerate() {
            let Some(anim_id) = anim_id else { continue };
            let Some(config) = self.building_anim_config(id, slot as u8, rules) else {
                continue;
            };
            let coord = self.building_anim_world_coord(id, &config).unwrap();
            if let Some(anim) = self.substrate.anims.get_mut(anim_id) {
                anim.world_coord = coord;
            }
        }
    }

    /// Original Grand_Opening445F80. First-opening effects depend on6E4,
    /// independently of the capture argument. Captures of already placed
    /// buildings still rearm ProduceCash and apply activation/house effects.
    pub(crate) fn grand_opening(
        &mut self,
        id: u64,
        captured: bool,
        scenario_initialization: bool,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category != crate::map::entities::EntityCategory::Structure
            || (entity.building_actually_placed && !captured)
        {
            return;
        }
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
            return;
        };
        let first_opening = !entity.building_actually_placed;
        let owner = entity.owner();
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
            .is_some_and(|cargo| !cargo.passengers.is_empty());
        // 0x00446382..0x004463B4: the built type's InfantryGainSelfHeal and
        // UnitsGainSelfHeal join the owner's house on every reached opening
        // (a capture counts), with the +0x6E4 test done by the entry guard
        // above rather than per arm.
        self.grant_house_self_heal(id, rules);
        crate::sim::credit_income::produce_cash_on_grand_opening(self, id, rules);
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
            house.tracking.add_airport_docks(object.number_of_docks);
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
        //446A77 invalidates retained House power and radar on every reached
        //opening (including capture), before publishing placement at446A84.
        self.invalidate_house_power(owner, true);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.building_actually_placed = true;
        }
        self.add_building_sensor_array_if_powered(id, rules);
        self.announce_super_weapon_building_complete(id, rules);
        if !captured && !scenario_initialization {
            self.mission_spawned_entities |=
                crate::sim::production::spawn_building_free_unit(self, id, rules, registry);
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
        rules
            .art()
            .resolve_metadata_entry(name, &object.image)?
            .building_anims
            .iter()
            .find(|config| config.native_slot == slot)
            .cloned()
    }

    /// Original451EE0: store retained flag before ascending occupied-slot visits.
    /// A missing requested name leaves the old animation alive in that slot.
    pub(crate) fn set_building_damage_state(&mut self, id: u64, damaged: bool, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if !set_damage_state(entity, damaged) {
            return;
        }
        for slot in 0..21 {
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| e.building_anim_slots[slot].is_some())
            {
                self.set_building_anim_slot(id, slot as u8, damaged, false, 0, rules);
            }
        }
    }

    pub(crate) fn refresh_building_damage_state(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
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
            if entity.building_anim_slots[slot].is_none() {
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
    pub(crate) fn set_building_anim_slot(
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
        self.substrate.anims.get_mut(new_id)?.mark_building_anim();
        let old = self.substrate.entities.get(id)?.building_anim_slots[usize::from(slot)];
        if let Some(frame) = old
            .and_then(|old_id| self.anim(old_id))
            .map(|anim| anim.runtime.current_frame)
        {
            self.substrate
                .anims
                .get_mut(new_id)
                .expect("new slot animation")
                .runtime
                .current_frame = frame;
        }
        self.clear_building_anim_slot(id, slot);
        if let Some(anim) = self.substrate.anims.get_mut(new_id) {
            anim.building_slot = Some((id, slot));
            anim.z_adjust = config.z_adjust;
        }
        self.substrate.entities.get_mut(id)?.building_anim_slots[usize::from(slot)] = Some(new_id);
        //451A36 calls4555D0 only when retained StuffEnabled6EA is true.
        //Construction/placement and operational history cannot substitute for it.
        let object = rules.object(
            self.interner
                .resolve(self.substrate.entities.get(id)?.type_ref()),
        )?;
        let enabled = self.substrate.entities.get(id)?.building_stuff_enabled
            && self.building_operational_state(id, rules) == Some(true);
        if !enabled && object.powered && self.building_anim_power_flags(id, slot, rules).powered {
            self.substrate.anims.get_mut(new_id)?.runtime.paused = true;
        }
        Some(new_id)
    }

    pub(crate) fn clear_building_anim_slot(&mut self, id: u64, slot: u8) {
        let old = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.building_anim_slots[usize::from(slot)].take());
        if let Some(old) = old {
            self.scalar_delete_building_anim(old);
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
    pub(crate) fn building_anim_pointer_expired(&mut self, expired: u64, rules: Option<&RuleSet>) {
        let Some((owner, slot)) = self.anim(expired).and_then(|anim| anim.building_slot) else {
            return;
        };
        let Some(rules) = rules else {
            // Rules-less teardown is the defensive destructor route; ordinary
            // Anim AI and service destruction always carry the live RuleSet.
            self.clear_building_anim_reference(expired);
            return;
        };
        let Some(entity) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
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
                .then_some(anim.building_slot)
                .flatten()
                .map(|(owner, slot)| (owner, slot, anim.completed()))
        }) else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(owner) else {
            return;
        };
        if !entity.is_object_alive()
            || entity.building_anim_slots[usize::from(slot)] != Some(expired)
        {
            return;
        }
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
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
            .is_some_and(|cargo| !cargo.passengers.is_empty());
        let delayed_fire = self.building_anim_delayed_fire(owner, rules);
        self.clear_building_anim_reference(expired);
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

    /// Clear the source slot and its derived reverse index without generating
    /// a successor. Scalar deletion and defensive final removal use this owner.
    pub(crate) fn clear_building_anim_reference(&mut self, id: u64) {
        let slot = self
            .substrate
            .anims
            .get_mut(id)
            .and_then(|anim| anim.building_slot.take());
        if let Some((owner, slot)) = slot
            && let Some(entity) = self.substrate.entities.get_mut(owner)
            && entity.building_anim_slots[usize::from(slot)] == Some(id)
        {
            entity.building_anim_slots[usize::from(slot)] = None;
        }
    }

    /// Rebuild an index, never infer slot selection from HP or animation names.
    pub(crate) fn rebuild_building_anim_slot_indices(&mut self) {
        for anim in self.substrate.anims.values_mut() {
            anim.building_slot = None;
            anim.damage_fire_slot = None;
        }
        let slots: Vec<_> = self
            .substrate
            .entities
            .values()
            .flat_map(|entity| {
                entity
                    .building_anim_slots
                    .iter()
                    .enumerate()
                    .filter_map(move |(slot, id)| id.map(|id| (id, entity.stable_id(), slot as u8)))
            })
            .collect();
        for (id, owner, slot) in slots {
            self.substrate
                .anims
                .get_mut(id)
                .expect("snapshot admission validated every unique Building slot Anim")
                .building_slot = Some((owner, slot));
        }
        for entity in self.substrate.entities.values() {
            for (slot, id) in entity.damage_fire_anim_ids.iter().enumerate() {
                if let Some(id) = id {
                    self.substrate
                        .anims
                        .get_mut(*id)
                        .expect("snapshot admission validated every unique damage-fire Anim")
                        .damage_fire_slot = Some((entity.stable_id(), slot as u8));
                }
            }
        }
    }
}

pub(crate) fn requested_damage_state(health: Health, strength: i32, yellow: f64) -> bool {
    health.compare_ratio(strength, yellow) != Greater
}

/// Completed CanBeOccupied body frame, original43EF90 (43F05C/43F089).
/// Kept in sim so receiver before/after checks and presentation share predicates.
pub(crate) fn occupied_body_frame(
    occupants: u32,
    current: i32,
    strength: i32,
    tech_level: i32,
    yellow: f64,
    red: f64,
) -> u16 {
    let health = Health { current };
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
pub(crate) struct BodyFrameInput {
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

pub(crate) fn body_frame(
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
            health.current,
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

/// Equality key for health-only receiver continuation442B37. Represented
/// building body states are construction(0) and completed(1). Native fences
/// and construction read no health, so their unchanged frame cancels here.
/// This key is not a rendering frame or a replacement for their body timers.
pub(crate) fn receiver_body_frame(
    entity: &GameEntity,
    object: &ObjectType,
    rules: &RuleSet,
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
                    .map_or(0, |cargo| cargo.passengers.len() as i32),
            ),
            tech_level: object.tech_level,
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

/// Retained flag transition itself, shared by every reached caller. Returns
/// whether native451EE0 proceeds to its ordered occupied-slot replacement loop.
pub(crate) fn set_damage_state(entity: &mut GameEntity, damaged: bool) -> bool {
    if entity.building_damage_state_active == damaged {
        return false;
    }
    entity.building_damage_state_active = damaged;
    true
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
            let before = body_frame(input, Health { current: i.before }, i.strength, yellow, red);
            let after = body_frame(input, Health { current: i.after }, i.strength, yellow, red);
            assert_eq!(
                (before as u32, after as u32),
                (row.output.before_frame, row.output.after_frame),
                "native row {index}: {input:?}"
            );
            let changed = before != after;
            assert_eq!(u8::from(changed), row.output.dirty, "native row {index}");
            let retained = if changed {
                requested_damage_state(Health { current: i.after }, i.strength, yellow)
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

#[cfg(test)]
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
    let id = sim.allocate_stable_id();
    let mut entity = GameEntity::test_default_of_category(
        id,
        "B",
        "A",
        2,
        2,
        crate::map::entities::EntityCategory::Structure,
    );
    entity.type_ref = sim.interner.intern("B");
    entity.owner = sim.interner.intern("A");
    entity.health.current = 100;
    sim.substrate.entities.insert(entity);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    (sim, rules, id)
}

#[cfg(test)]
mod slot_tests {
    use super::*;
    #[test]
    fn shared_bounds_apply_to_slot_create_replace_and_reveal_reposition() {
        use crate::sim::world::{
            PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, UninitContext,
        };
        let (mut sim, mut rules, id) = slot_test_fixture();
        sim.session.pixel_conversion_bounds =
            crate::util::pixel_conversion::PixelConversionBounds {
                width: 640,
                height: 480,
            };
        let config = &mut rules.art_entry_mut_for_test("B").unwrap().building_anims[0];
        config.x = 640;
        config.y = 0;
        let first = sim
            .set_building_anim_slot(id, 3, false, false, 0, &rules)
            .unwrap();
        assert_eq!(sim.anim(first).unwrap().world_coord.x, 512);
        sim.set_building_damage_state(id, true, &rules);
        let replaced = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        assert_eq!(sim.anim(replaced).unwrap().world_coord.x, 512);
        let timer = sim.anim(replaced).unwrap().runtime.frame_timer;
        // This synthetic owner has never been marked; enter through the real
        // coordinate commit and verify its already retained slot moves in place.
        let entity = sim.entities_mut().get_mut(id).unwrap();
        entity.lifecycle.in_limbo = true;
        entity.lifecycle.cell_marked = false;
        let outcome = sim.try_reveal_entity_with_context(
            id,
            RevealRequest {
                position: RevealPosition {
                    exact_z_leptons: None,
                    rx: 3,
                    ry: 4,
                    z: 0,
                    sub_x: crate::util::fixed_math::SimFixed::from_num(128),
                    sub_y: crate::util::fixed_math::SimFixed::from_num(128),
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::with_rules(&rules),
        );
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));
        assert_eq!(
            sim.entities().get(id).unwrap().building_anim_slots[3],
            Some(replaced)
        );
        assert_eq!(sim.anim(replaced).unwrap().world_coord.x, 768);
        assert_eq!(sim.anim(replaced).unwrap().world_coord.y, 1024);
        assert_eq!(sim.anim(replaced).unwrap().runtime.frame_timer, timer);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(
            restored.session.pixel_conversion_bounds,
            sim.session.pixel_conversion_bounds
        );
        assert_eq!(
            restored.anim(replaced).unwrap().world_coord,
            sim.anim(replaced).unwrap().world_coord
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
        let slots = sim.substrate.entities.get(id).unwrap().building_anim_slots;
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
            assert_eq!(sim.anim(early).unwrap().runtime.rate_reload, 3);
            assert_eq!(
                sim.anim(late).unwrap().runtime.rate_reload,
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
            assert_eq!(sim.anim(missing).unwrap().effective_end, 0);
            assert_eq!(sim.scenario_rng.logical_state(), before_rng);
            sim.visit_anim(missing, &rules, None);
            assert!(!sim.anim(missing).unwrap().runtime.first_ai_guard);
            assert!(!sim.anim(missing).unwrap().runtime.inactive);
            sim.session.binary_frame += 1;
            sim.visit_anim(missing, &rules, None);
            assert!(sim.substrate.pending_delete.contains(&missing));
            assert!(!sim.anim(missing).unwrap().in_logic_vector);
            assert_eq!(sim.entities().get(id).unwrap().building_anim_slots[5], None);
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
            .runtime
            .current_frame = 17;
        sim.substrate
            .anims
            .get_mut(old)
            .unwrap()
            .runtime
            .first_ai_guard = false;
        sim.substrate
            .anims
            .get_mut(old)
            .unwrap()
            .runtime
            .loop_remaining = 8;
        sim.substrate.anims.get_mut(old).unwrap().start_sound_active = true;
        sim.session.binary_frame = 91;
        let before_rng = sim.scenario_rng.logical_state();
        sim.set_building_damage_state(id, true, &rules);
        let new = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        assert_ne!(old, new);
        assert!(sim.anim(old).is_none());
        assert!(!sim.substrate.pending_delete.contains(&old));
        let anim = sim.anim(new).unwrap();
        assert_eq!(sim.interner.resolve(anim.type_id), "D");
        assert_eq!(anim.runtime.current_frame, 17);
        assert!(anim.runtime.first_ai_guard);
        assert_eq!(anim.runtime.loop_remaining, 255);
        assert_eq!(anim.owner_entity, None);
        assert_eq!(anim.building_slot, Some((id, 3)));
        assert_eq!(anim.draw_flags, 0x1600);
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
        assert_eq!(sim.entities().get(id).unwrap().building_anim_slots[3], None);
        assert!(sim.substrate.pending_delete.contains(&anim));
        assert!(!sim.anim(anim).unwrap().in_logic_vector);
        sim.process_pending_delete();
        assert!(sim.anim(anim).is_none());
        assert_eq!(sim.entities().get(id).unwrap().building_anim_slots[3], None);
    }

    /// Whole original ObjectUnInit5F65F0 retains ordinary slot3 and its
    /// live Anim (owner+CC=0, inactive+19B=0). BuildingDestructor43BDC5
    /// calls ClearAnimSlot451E40(-2) only at deferred scalar destruction.
    /// Executed control: ordinary-building-destruction-research/continuation-1
    /// cleanup-registered-output.json, gamemd SHA1cdd1180e49024fb.
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
        assert!(!building.lifecycle.object_alive);
        assert!(building.lifecycle.in_limbo);
        assert_eq!(building.building_anim_slots[3], Some(anim_id));
        let anim = sim.anim(anim_id).expect("slot survives Building UnInit");
        assert_eq!(anim.owner_entity, None);
        assert_eq!(anim.building_slot, Some((id, 3)));
        assert!(anim.in_logic_vector);
        assert!(!anim.runtime.inactive);
        assert_eq!(sim.logic_order(), before_logic);
        assert_eq!(
            (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            before_rng
        );
        assert!(sim.substrate.pending_delete.contains(&id));
        assert!(!sim.substrate.pending_delete.contains(&anim_id));

        sim.process_pending_delete();
        assert!(sim.entities().get(id).is_none());
        assert!(sim.anim(anim_id).is_none());
        assert!(!sim.logic_order().contains(&anim_id));
        assert!(!sim.substrate.pending_delete.contains(&anim_id));
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
        sim.grand_opening(id, false, true, &rules, None);
        let old = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        sim.entities_mut().get_mut(id).unwrap().health.current = 25;
        sim.grand_opening(id, false, true, &rules, None);
        assert_eq!(
            sim.entities().get(id).unwrap().building_anim_slots[3],
            Some(old)
        );
        assert!(!sim.entities().get(id).unwrap().building_damage_state_active);
        sim.refresh_building_damage_state(id, &rules);
        assert_ne!(
            sim.entities().get(id).unwrap().building_anim_slots[3],
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
            let (mut sim, mut rules, id) = slot_test_fixture();
            let mut art = ArtRegistry::from_ini(&IniFile::from_str(&format!(
                "[B]\nActiveAnim=N\n[N]\nLoopCount=-1\nRate=300\nLoopEnd=24\nReverse={}\n",
                if reverse { "yes" } else { "no" }
            )));
            art.bind_anim_frame_count_for_test("N", 40);
            art.get_mut("B").unwrap().building_anims[0].native_slot = slot;
            rules.install_art_data(art);
            let old = if has_old {
                let old = sim
                    .set_building_anim_slot(id, slot, false, false, 0, &rules)
                    .unwrap();
                let runtime = &mut sim.substrate.anims.get_mut(old).unwrap().runtime;
                runtime.current_frame = old_frame;
                runtime.loop_remaining = 8;
                runtime.first_ai_guard = false;
                runtime.frame_timer = crate::sim::timer::CdTimer::started(7, 13);
                Some(old)
            } else {
                None
            };
            sim.session.binary_frame = 91;
            let new = sim
                .set_building_anim_slot(id, slot, false, false, 0, &rules)
                .unwrap();
            let anim = sim.anim(new).unwrap();
            assert_eq!(
                anim.runtime.current_frame,
                output["frame"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                anim.runtime.loop_remaining,
                output["loop_remaining"].as_u64().unwrap() as u8
            );
            assert_eq!(anim.runtime.first_ai_guard, output["first_guard"] == 1);
            assert_eq!(anim.runtime.frame_timer.start_frame(), 91);
            assert_eq!(anim.runtime.frame_timer.duration(), 3);
            assert_eq!(anim.building_slot, Some((id, slot)));
            assert_eq!(
                sim.entities().get(id).unwrap().building_anim_slots[usize::from(slot)],
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
            let mut text = String::from("[B]\nActiveAnim=OLD\n[OLD]\nLoopCount=-1\n");
            for slot in 0..21 {
                text.push_str(&format!(
                    "[N{slot:02}]\nLoopCount=-1\n[D{slot:02}]\nLoopCount=-1\n"
                ));
            }
            let mut art = ArtRegistry::from_ini(&IniFile::from_str(&text));
            let mut registry =
                String::from("[BuildingTypes]\n0=B\n[B]\nStrength=100\n[Animations]\n0=OLD\n");
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
            let template = art.get("B").unwrap().building_anims[0].clone();
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
                    .runtime
                    .current_frame = n as i32 + 123;
                sim.substrate.anims.get_mut(anim).unwrap().building_slot = Some((id, n as u8));
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .building_anim_slots[n] = Some(anim);
                old[n] = Some(anim);
            }
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .building_damage_state_active = input["old_flag"] == 1;
            sim.set_building_damage_state(id, input["requested"] == 1, &rules);
            assert_eq!(
                sim.entities().get(id).unwrap().building_damage_state_active,
                row["output"]["retained_flag"] == 1,
                "{row}"
            );
            let replacements = row["output"]["replacements"].as_array().unwrap();
            let mut previous_new = None;
            for n in 0..21 {
                let new = sim.entities().get(id).unwrap().building_anim_slots[n];
                if let Some(expected) = replacements.iter().find(|r| r["slot"] == n) {
                    let anim = sim.anim(new.unwrap()).unwrap();
                    assert_ne!(new, old[n], "{row}");
                    assert!(sim.anim(old[n].unwrap()).is_none());
                    assert_eq!(
                        sim.interner.resolve(anim.type_id),
                        expected["name"].as_str().unwrap()
                    );
                    assert_eq!(anim.runtime.current_frame, n as i32 + 123);
                    assert!(anim.runtime.first_ai_guard);
                    assert!(previous_new.is_none_or(|previous| previous < anim.stable_id));
                    previous_new = Some(anim.stable_id);
                } else {
                    assert_eq!(new, old[n], "native row retains slot{n}: {row}");
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 36);
    }
}
