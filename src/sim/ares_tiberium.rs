//! Ares tiberium effects on objects (`docs/ares-tiberium.md`).
//!
//! - Damage: `FootClass_UpdatePosition_TiberiumDamage @ 10049280` (hook
//!   `0x4D85E4`, the head of `FootClass::Per_Cell_Process @ 0x004D85D0` for
//!   a cell arrival). Gated on `[General] TiberiumDamageEnabled`.
//! - Healing: the tiberium arm of `FootClass_Update @ 10014BA0` (hook
//!   `0x4DA53E`, right after `FootClass::AI`'s TechnoClass::AI call). Gated
//!   on `[General] TiberiumHealEnabled`.
//! - Death: `TiberiumSpill` (`1004AEC0`, hook `0x702200`) and
//!   `TiberiumRemains` (`1004B050`, hook `0x702216`) in
//!   `TechnoClass::ReceiveDamage @ 0x00701900`'s kill arm, and the
//!   harvester blast of `UnitClass::Death_Explosion @ 0x00738680`
//!   (`1004BD90`, hook `0x738749`).
//!
//! Retail sets none of TiberiumDamageEnabled, TiberiumHealEnabled,
//! TiberiumSpill, TiberiumRemains or TiberiumExplosive, so all stay dormant
//! there.
//!
//! RESIDUAL: a Foot the tiberium kills becomes a neutral `[General]
//! SmallVisceroid` (`TechnoExt::SpawnVisceroid @ 100468B0`) only on a map
//! whose `[Basic] TiberiumDeathToVisceroid` (Scenario+0x34B3) is set, with
//! one `RandomRanged(0, 99) < TiberiumTransmogrify` draw. VERA does not read
//! that map flag, so it takes the unset arm: no draw and no visceroid.

use crate::map::entities::EntityCategory;
use crate::rules::ares_tiberium::HealStepKind;
use crate::rules::object_type::{Ability, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::rules::tiberium_type::{TiberiumType, TiberiumTypeId};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::game_entity::GameEntity;
use crate::sim::miner::CargoBale;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::MaskedX87Chop53 as X87;

/// The bales of a techno's Storage (`TechnoClass+0x33C`): a harvester's
/// hold or a slave's load.
fn stored_bales(entity: &GameEntity) -> impl Iterator<Item = &CargoBale> {
    entity
        .miner
        .as_ref()
        .map_or(&[][..], |miner| miner.cargo())
        .iter()
        .chain(entity.slave.cargo())
}

/// `CellClass::GetNeighbourCell(direction)`: the adjacent cell, or the cell
/// itself for a direction of 8 or more (`0x00481810`). Off the map it names
/// no overlay cell.
fn neighbour_cell(center: (i32, i32), direction: usize) -> (u16, u16) {
    let (dx, dy) = crate::util::direction::DIRECTION_DELTAS
        .get(direction)
        .copied()
        .unwrap_or((0, 0));
    (
        u16::try_from(center.0 + dx).unwrap_or(u16::MAX),
        u16::try_from(center.1 + dy).unwrap_or(u16::MAX),
    )
}

/// `TechnoTypeClass::GetRepairStep` (vt+0xB4): InfantryType `0x00524790`
/// answers `[General] IRepairStep`, every other type `0x00712120`
/// `[General] RepairStep`.
fn repair_step(category: EntityCategory, rules: &RuleSet) -> i32 {
    if category == EntityCategory::Infantry {
        rules.general.infantry_repair_step
    } else {
        rules.general.repair_step
    }
}

/// The step `TiberiumExt::GetHealStep @ 10050840` picks by the type's class.
fn heal_step_kind(category: EntityCategory) -> HealStepKind {
    match category {
        EntityCategory::Infantry => HealStepKind::Infantry,
        EntityCategory::Unit => HealStepKind::Unit,
        _ => HealStepKind::Other,
    }
}

impl Simulation {
    /// The tiberium on the cell an object stands in (`GetCell`, vt+0x1BC,
    /// then `CellClass::GetContainedTiberiumIndex`).
    fn tiberium_under<'r>(&self, id: u64, rules: &'r RuleSet) -> Option<&'r TiberiumType> {
        let entity = self.substrate.entities.get(id)?;
        let grid = self.overlay_grid.as_ref()?;
        let (rx, ry) = (entity.position.rx(), entity.position.ry());
        if rx >= grid.width() || ry >= grid.height() {
            return None;
        }
        let view = crate::sim::tiberium::tiberium_cell_view(
            grid,
            rules.overlay_registry(),
            &rules.tiberium_types,
            (rx, ry),
        )?;
        rules.tiberium_types.get(view.tiberium_type)
    }

    /// Whether `object` stands no higher than `[General] HoverHeight`
    /// (`GetHeight`, vt+0x1C8).
    fn at_tiberium_height(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate.entities.get(id).is_some_and(|entity| {
            crate::sim::movement::air_movement::current_fly_height(
                entity,
                self.resolved_terrain.as_ref(),
            ) <= rules.general.hover_height
        })
    }

    /// Ares 3.0 10049280 (hook 0x4D85E4): a Foot arriving on a tiberium cell
    /// takes that tiberium's `Damage` with its `Warhead` (TiberiumExt
    /// getters) unless it is `TiberiumProof` (default: everything but
    /// infantry) or holds the TIBERIUM_PROOF ability. The ReceiveDamage has
    /// no attacker, distance 0 and both flags false. Returns whether the
    /// call killed it, in which case Per_Cell_Process returns at once
    /// (`0x4D8F29`).
    pub(crate) fn ares_foot_tiberium_damage(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if !rules.ares_tiberium_globals().damage_enabled() || !self.at_tiberium_height(id, rules) {
            return false;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        let proof_default = entity.category() != EntityCategory::Infantry;
        if object
            .techno_combat()
            .tiberium()
            .tiberium_proof(proof_default)
            || crate::sim::combat::veterancy::has_weapon_ability(
                entity.veterancy.rank_kind(),
                object,
                Ability::TiberiumProof,
            )
            || entity.health.current() <= 0
        {
            return false;
        }
        let Some(tiberium) = self.tiberium_under(id, rules) else {
            return false;
        };
        let c4 = Some(rules.bridge_warheads.c4_name.as_str());
        let damage = tiberium.ares().damage(tiberium.power);
        let Some(warhead) = tiberium
            .ares()
            .warhead(c4)
            .and_then(|name| rules.warhead(name))
        else {
            return false;
        };
        if damage == 0 {
            return false;
        }
        let warhead_ref = self.interner.intern(&warhead.id);
        let receiver = EntityDamageEvent::direct_receiver(
            id,
            damage,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead_ref,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        self.commit_direct_damage_receiver(rules, registry, receiver, effects);
        self.substrate
            .entities
            .get(id)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    }

    /// Ares 3.0 10014BA0 (hook 0x4DA53E), the tiberium arm: a live
    /// `TiberiumHeal=` Foot (or one holding the TIBERIUM_HEAL ability), hurt
    /// and no higher than HoverHeight, on a Tiberium land cell heals the
    /// mineral's `GetHealStep` on frames divisible by
    /// `ftol(GetHealDelay * 900)`, capped at Strength. A Tiberium land cell
    /// without a mineral uses the type's repair step and `[General]
    /// TiberiumHeal`.
    ///
    /// RESIDUAL: a delay below one frame divides by zero natively (a crash);
    /// VERA heals nothing then.
    pub(crate) fn ares_foot_tiberium_heal(&mut self, id: u64, rules: &RuleSet) {
        if !rules.ares_tiberium_globals().heal_enabled() || !self.at_tiberium_height(id, rules) {
            return;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !entity.lifecycle.object_alive() {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if !Self::tiberium_heals(entity, object) {
            return;
        }
        let (health, strength) = (entity.health.current(), object.strength);
        if health <= 0 || health >= strength {
            return;
        }
        let land_is_tiberium = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            terrain
                .cell(entity.position.rx(), entity.position.ry())
                .is_some_and(|cell| cell.land_type == LandType::Tiberium.as_index())
        });
        if !land_is_tiberium {
            return;
        }
        let category = entity.category();
        let mut delay = rules.general.tiberium_heal_minutes;
        let mut step = repair_step(category, rules);
        if let Some(tiberium) = self.tiberium_under(id, rules) {
            delay = tiberium.ares().heal_delay(delay);
            step = tiberium.ares().heal_step(heal_step_kind(category), step);
        }
        let interval = crate::rules::ruleset::native_minutes_to_frames(delay);
        let frame = self.session.binary_frame() as i32;
        if interval == 0 || frame.wrapping_rem(interval) != 0 {
            return;
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.health.heal(step);
            if entity.health.current() > strength {
                entity.health.set(strength);
            }
        }
    }

    /// `TiberiumHeal=` or the TIBERIUM_HEAL ability by rank
    /// (`TechnoTypeClass::HasAbility @ 100453B0`).
    fn tiberium_heals(entity: &crate::sim::game_entity::GameEntity, object: &ObjectType) -> bool {
        object.tiberium_heal
            || crate::sim::combat::veterancy::has_weapon_ability(
                entity.veterancy.rank_kind(),
                object,
                Ability::TiberiumHeal,
            )
    }
}

impl Simulation {
    /// The cell of an object's GetCoords (vt+0x48), as `MapClass::GetCellAt`
    /// takes it.
    fn coords_cell(&self, id: u64) -> Option<(i32, i32)> {
        let entity = self.substrate.entities.get(id)?;
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        Some((coord.x / 256, coord.y / 256))
    }

    /// Ares 3.0 1004AEC0 (hook 0x702200), after the death sounds and before
    /// the Stun: a dying `TiberiumSpill=yes` non-building with stored
    /// tiberium and no HarvesterImmune spills Riparius (type 0) around the
    /// cell of its GetCoords, `RandomRanged(0, 2)` per cell in the order
    /// 9 (centre), 2, 7, 1, 4, 3, 0, 5, 6, until `min(Storage, 9)` scaled
    /// by its fill (`ftol(stored / Storage * max)`, else `max / 2`) is
    /// spent. 3.0 sums the stored amounts truncated to integers.
    pub(crate) fn ares_spill_tiberium(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if !object.techno_combat().tiberium().tiberium_spill() {
            return;
        }
        let stored = stored_bales(entity).count() as i32;
        if entity.category() == EntityCategory::Structure
            || stored <= 0
            || self.session.harvester_immune()
        {
            return;
        }
        let storage = object.storage;
        let max = storage.min(9);
        let mut value = if storage > 0 {
            X87::ftol_i32_low_masked(X87::mul(
                X87::div(X87::load_i32(stored), X87::load_i32(storage)),
                X87::load_i32(max),
            ))
        } else {
            max / 2
        };
        let Some(center) = self.coords_cell(id) else {
            return;
        };
        self.increase_tiberium_cells(rules, rules.overlay_registry(), |context| {
            let mut placements = Vec::new();
            for direction in [9, 2, 7, 1, 4, 3, 0, 5, 6] {
                let amount = context.rng.next_range_i32_inclusive(0, 2);
                placements.extend(crate::sim::tiberium::place_tiberium(
                    context,
                    neighbour_cell(center, direction),
                    TiberiumTypeId(0),
                    amount as u8,
                ));
                value -= amount;
                if value <= 0 {
                    break;
                }
            }
            placements
        });
    }

    /// Ares 3.0 1004B050 (hook 0x702216), after the Stun: a dying techno
    /// whose `TiberiumRemains` holds (default: `TiberiumHeal=yes` and
    /// `[General] TiberiumHealEnabled`) leaves `RandomRanged(0, 2)` Riparius
    /// on the four orthogonal neighbours (directions 0, 2, 4, 6) and the
    /// centre (8) of the cell of its GetCoords.
    pub(crate) fn ares_tiberium_remains(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let default = object.tiberium_heal && rules.ares_tiberium_globals().heal_enabled();
        if !object.techno_combat().tiberium().tiberium_remains(default) {
            return;
        }
        let Some(center) = self.coords_cell(id) else {
            return;
        };
        self.increase_tiberium_cells(rules, rules.overlay_registry(), |context| {
            let mut placements = Vec::new();
            for direction in (0..10).step_by(2) {
                let amount = context.rng.next_range_i32_inclusive(0, 2);
                placements.extend(crate::sim::tiberium::place_tiberium(
                    context,
                    neighbour_cell(center, direction),
                    TiberiumTypeId(0),
                    amount as u8,
                ));
            }
            placements
        });
    }

    /// Ares 3.0 1004BD90 (hook 0x738749), in place of stock's dead sum
    /// (`0x00738749..0x007387C4`): with `[CombatDamage] TiberiumExplosive`
    /// and no HarvesterImmune, a dying unit holding tiberium deals the sum
    /// of `ftol(amount * Power)` over the types as Apply_area_damage at its
    /// GetCoords (source itself, no house, affects-tiberium false) with
    /// `[CombatDamage] TiberiumExplosiveWarhead`, then plays `[AudioVisual]
    /// TiberiumExplosiveAnim` there (flags 0x2600, zAdjust -15), each only
    /// when set and the sum is positive.
    pub(crate) fn ares_unit_tiberium_explosive(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if !rules.combat_damage.tiberium_explosive || self.session.harvester_immune() {
            return;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let mut amounts = vec![0i32; rules.tiberium_types.len()];
        for bale in stored_bales(entity) {
            if let Some(amount) = amounts.get_mut(usize::from(bale.tiberium_type.0)) {
                *amount += 1;
            }
        }
        if amounts.iter().all(|&amount| amount <= 0) {
            return;
        }
        let damage = rules
            .tiberium_types
            .types()
            .iter()
            .zip(&amounts)
            .fold(0i32, |sum, (tiberium, &amount)| {
                sum.wrapping_add(amount.wrapping_mul(tiberium.power))
            });
        if damage <= 0 {
            return;
        }
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let globals = rules.ares_tiberium_globals();
        if let Some(warhead) = globals
            .explosive_warhead()
            .and_then(|name| rules.warhead(name))
        {
            let warhead_ref = self.interner.intern(&warhead.id);
            crate::sim::combat::world_receiver::apply_area_damage(
                self,
                rules,
                registry,
                crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z),
                damage,
                warhead,
                false,
                (id, None, warhead_ref),
                effects,
            );
        }
        if let Some(anim) = globals.explosive_anim() {
            self.spawn_named_anim(
                rules,
                anim,
                crate::sim::anim_class::AnimWorldCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                },
                0,
                0x2600,
                -15,
            );
        }
    }
}

#[cfg(test)]
#[path = "ares_tiberium_tests.rs"]
mod tests;
