//! A building's damage fires: the eight AnimClass ownership slots and the
//! producer that lights and clears them as the building's health changes.

use super::{
    AnimDrawRuntime, AnimId, AnimSpawnError, AnimWorldCoord, DAMAGE_FIRE_SLOT_COUNT,
    TRAILER_DRAW_FLAGS,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;

/// A building's damage fires: the transition cache (`BuildingClass+0x5E8`),
/// distinct from the generic damaged-art state, and the eight fixed AnimClass
/// ownership slots (`+0x5C8..+0x5E4`). `BuildingClass::Update` calls the
/// producer (or the `0x0043FCA4` recovery) before storing the cache
/// (`0x0043FC92`, `0x0043FCBE`); an anim's expiry clears its slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DamageFires {
    pub(super) active: bool,
    pub(super) anim_ids: [Option<AnimId>; 8],
}

impl DamageFires {
    /// The cached transition state.
    pub(crate) const fn is_active(&self) -> bool {
        self.active
    }

    /// The eight slots.
    pub(crate) const fn anim_ids(&self) -> &[Option<AnimId>; 8] {
        &self.anim_ids
    }

    /// Install a fixture's slot.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_slot_for_test(&mut self, slot: usize, id: Option<AnimId>) {
        self.anim_ids[slot] = id;
    }
}

/// An object that never lit a fire keeps no slots (`GameEntity::damage_fires`).
impl crate::util::sparse::SparseDefault for DamageFires {
    fn sparse_default() -> &'static Self {
        static DEFAULT: std::sync::LazyLock<DamageFires> =
            std::sync::LazyLock::new(DamageFires::default);
        &DEFAULT
    }
}

impl Simulation {
    pub(crate) fn update_building_damage_fire(&mut self, building_id: u64, rules: &RuleSet) {
        let Some((current, type_ref, position, prior_state, category)) =
            self.substrate.entities.get(building_id).map(|entity| {
                (
                    entity.health.current(),
                    entity.type_ref(),
                    entity.position,
                    entity.damage_fires.active,
                    entity.category(),
                )
            })
        else {
            return;
        };
        if category != crate::map::entities::EntityCategory::Structure {
            return;
        }
        let Some(object_type) = self.object_type(type_ref, rules) else {
            return;
        };
        let maximum = object_type.strength;
        let can_be_occupied = object_type.can_be_occupied;
        // Borrowed from the rules, which outlive this visit: the image and
        // foundation are read only when the fires start.
        let image = &object_type.image;
        let foundation = &object_type.foundation;
        let ratio = if can_be_occupied {
            rules.general.condition_red
        } else {
            rules.general.condition_yellow
        };
        // Building43FC39..43FC84: CanBeOccupied selects red versus yellow;
        // TEST AH,0x41 includes unordered, with no HP/Strength positivity gate.
        let active = crate::sim::combat::Health::new(current).compare_ratio(maximum, ratio)
            != crate::util::native_x87::MaskedX87Ordering::Greater;
        if active == prior_state {
            return;
        }
        if !active {
            self.clear_building_damage_fire_slots(building_id, rules);
        } else if !rules.general.damage_fire_types.is_empty() {
            let type_count = rules.general.damage_fire_types.len();
            let mut type_index = self
                .scenario_rng
                .next_range_u32_inclusive(0, type_count.saturating_sub(1) as u32)
                as usize;
            let offsets = rules
                .art()
                .get(image)
                .map_or(&[][..], |entry| entry.damage_fire_offsets.as_slice());
            let (foundation_w, foundation_h) = foundation.dimensions();
            let foundation_sum = i32::from(foundation_w).wrapping_add(i32::from(foundation_h));
            let location = crate::sim::movement::ground_pose::object_location(
                self.substrate.entities.get(building_id).unwrap(),
                self.resolved_terrain.as_ref(),
            );
            let (base, _) = crate::sim::movement::ground_pose::building_render_order_parts(
                location, false, false,
            );

            for slot in 0..DAMAGE_FIRE_SLOT_COUNT {
                let occupied = self
                    .substrate
                    .entities
                    .get(building_id)
                    .and_then(|entity| entity.damage_fires.anim_ids[slot]);
                if occupied.is_some() {
                    break;
                }
                let Some(offset) = offsets.get(slot).copied() else {
                    break;
                };
                // 43C124..43C137: an offset equal to the zero Point2D at
                // 89C848 (never written) ends the fires, as the reader's end
                // marker does; an explicit `DamageFireOffsetN=0,0` stops here.
                if offset.pixel_x == 0 && offset.pixel_y == 0 {
                    break;
                }
                let fire_name = &rules.general.damage_fire_types[type_index].name;
                let fire_type = self.interner.intern(fire_name);
                let descriptor = AnimClassSpawnDescriptor {
                    type_name: fire_type,
                    rx: position.rx(),
                    ry: position.ry(),
                    sub_x: position.sub_x(),
                    sub_y: position.sub_y(),
                    z: position.z(),
                    delay: 0,
                    loop_count: 1,
                    draw_flags: TRAILER_DRAW_FLAGS,
                    z_adjust: 0,
                    reverse: false,
                    use_cell_drawer: false,
                    terrain_attached: false,
                    draw_runtime: AnimDrawRuntime::default(),
                    owner: None,
                };
                let world = AnimWorldCoord {
                    x: base.x.wrapping_add(offset.world_dx),
                    y: base.y.wrapping_add(offset.world_dy),
                    z: base.z,
                };
                let anim_id = match self.spawn_anim_at_world(rules, descriptor, world) {
                    Ok(id) => id,
                    Err(AnimSpawnError::Tiberium(error)) => {
                        log::warn!(
                            "damage-fire animation refused an invalid Tiberium Image start: {error}"
                        );
                        break;
                    }
                    // A map or mod can list a `DamageFireTypes=` entry with
                    // no bound art. Nothing is written for this slot before
                    // the spawn, so the building keeps its earlier fires and
                    // gets no more this pass, as for the trailer above.
                    Err(error) => {
                        log::warn!("damage-fire animation not spawned: {error}");
                        break;
                    }
                };
                // Original43C0D0/43C1EB retains only the Building5C8 slot.
                // The constructor leaves Anim+CC null: absolute coordinates
                // and the expiry listener do not require Object attachment.
                self.substrate
                    .anims
                    .get_mut(anim_id)
                    .expect("new damage fire")
                    .damage_fire_slot = Some((building_id, slot as u8));
                if let Some(entity) = self.substrate.entities.get_mut(building_id) {
                    entity.damage_fires.anim_ids[slot] = Some(anim_id);
                }

                let scaled = offset
                    .pixel_y
                    .wrapping_sub(foundation_sum.wrapping_mul(15))
                    .wrapping_mul(3);
                let z_adjust = (scaled >> 1).wrapping_sub(10).min(0);
                let effective_end = self
                    .substrate
                    .anims
                    .get(anim_id)
                    .map_or(0, |anim| anim.effective_end);
                let frame = if effective_end > 0 {
                    self.scenario_rng
                        .next_range_u32_inclusive(0, effective_end.wrapping_sub(1) as u32)
                        as i32
                } else {
                    0
                };
                self.substrate
                    .set_anim_frame_and_z_adjust(anim_id, frame, z_adjust);
                type_index += 1;
                if type_index == type_count {
                    type_index = 0;
                }
            }
        }
        // Original43FC92 calls the producer (or43FCA4 recovery) before
        // 43FCBE stores5E8, including a producer's early slot/type exit.
        // Native replay: building_death_anims_joined.json, full target AI.
        if let Some(entity) = self.substrate.entities.get_mut(building_id) {
            entity.damage_fires.active = active;
        }
    }

    /// Recovery43FCA4, destructor43BDE0 and Building Limbo 4458E4..445903
    /// call Anim UnInit4255B0 (deferred), not scalar delete, on each of the
    /// eight slots and null it; none of them writes5E8 here.
    /// The destructor follows owner expiry, so its Anims no longer need Rules
    /// for SetOwner's intermediate display resubmission.
    pub(crate) fn clear_building_damage_fire_slots(&mut self, building_id: u64, rules: &RuleSet) {
        if !self.substrate.entities.contains(building_id) {
            return;
        }
        for slot in 0..DAMAGE_FIRE_SLOT_COUNT {
            let anim_id = self
                .substrate
                .entities
                .get(building_id)
                .and_then(|entity| entity.damage_fires.anim_ids[slot]);
            let Some(anim_id) = anim_id else {
                continue;
            };
            self.destroy_anim_with_context(anim_id, rules);
            if let Some(entity) = self.substrate.entities.get_mut(building_id) {
                entity.damage_fires.anim_ids[slot] = None;
            }
        }
    }
}
