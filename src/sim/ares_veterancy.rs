//! Optional Ares rank changes: `TechnoExt::UpdateVeterancy` (`10046AF0`),
//! which replaces the stock promotion detector (`TechnoClass_Update_Veterancy`
//! `1004F2E0`, hook `0x006FA054..0x006FA14B`) and also promotes a gunner or
//! garrison occupant the moment it earns a kill (`1004F2C7`). The kill award
//! itself is `combat::record_kill`'s; the rules are `rules::ares_veterancy`.

use crate::rules::ares_veterancy::PromotedRank;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy::{LEVEL_ELITE, LEVEL_UNSAMPLED, LEVEL_VETERAN};
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

impl Simulation {
    /// `TechnoExt::UpdateVeterancy(pThis, silent, flash)` (`10046AF0`).
    ///
    /// Nothing happens while the rank cache (`+0x13C`) already holds the
    /// accumulator's level; a never-sampled object only caches it. Otherwise,
    /// in this order:
    /// - `Promote.IncludePassengers` copies the accumulator onto every
    ///   trainable passenger (cargo head first) and updates each silently,
    ///   without flash (`10046B54..10046BA2`);
    /// - a veteran or elite rank picks its `Promote.*Sound` (else
    ///   `UpgradeVeteranSound`/`UpgradeEliteSound`), its `Promote.*Flash`
    ///   (else RulesExt `VeteranFlashTimer` / `EliteFlashTimer`), its
    ///   `EVA.*Promoted`, its `Promote.*Type` and its `Promote.*Experience`;
    ///   a demotion picks none of them;
    /// - a `Promote.*Type` converts in place (ConvertToType `10043650`); only
    ///   after a successful conversion is a nonzero experience added, the
    ///   rank re-read and the attached stats recalculated (`100460A0`);
    /// - unless silent, the owner's sound and EVA are queued (the app keeps
    ///   the human-player gate, `10046D03..10046D2D`) at the Transporter's
    ///   Location when it has one;
    /// - with `flash`, a positive duration seeds `+0xF0`;
    /// - the cache stores the final level.
    pub(crate) fn update_ares_veterancy(
        &mut self,
        id: u64,
        silent: bool,
        flash: bool,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let mut level = entity.veterancy.level();
        let cached = entity.veterancy.rank_cache();
        if cached == level {
            return;
        }
        if cached != LEVEL_UNSAMPLED
            && let Some(object) = self.entity_object_type(entity, rules)
        {
            let ext = object.ares_veterancy();
            if ext.include_passengers() {
                let raw = entity.veterancy.raw();
                let passengers = entity
                    .passenger_role
                    .cargo()
                    .map(|cargo| cargo.passengers().to_vec())
                    .unwrap_or_default();
                for passenger in passengers {
                    let trainable = self
                        .substrate
                        .entities
                        .get(passenger)
                        .and_then(|entity| self.entity_object_type(entity, rules))
                        .is_some_and(|object| object.trainable);
                    if !trainable {
                        continue;
                    }
                    if let Some(entity) = self.substrate.entities.get_mut(passenger) {
                        entity.veterancy.inherit_raw(raw);
                    }
                    self.update_ares_veterancy(passenger, true, false, rules, effects);
                }
            }
            let rank = match level {
                LEVEL_VETERAN => Some(PromotedRank::Veteran),
                LEVEL_ELITE => Some(PromotedRank::Elite),
                _ => None,
            };
            if let Some(rank) = rank {
                let elite = rank == PromotedRank::Elite;
                let global_sound = if elite {
                    rules.general.upgrade_elite_sound.as_deref()
                } else {
                    rules.general.upgrade_veteran_sound.as_deref()
                };
                // Veteran10046BBA..10046BD6 and elite10046C12..10046C2D:
                // an unset NullableIdx<Voc> falls back to Rules+228/+22C,
                // an explicit -1 silences the cue but not the EVA.
                let sound = object
                    .promotion_sound_override(elite)
                    .unwrap_or(global_sound);
                let flash_frames = ext.flash(rank).unwrap_or(if elite {
                    rules.general.elite_flash_timer
                } else {
                    rules.ares_veterancy().veteran_flash_timer()
                });
                let eva = object.promotion_eva(elite);
                if let Some(target) = ext.promote_type(rank).cloned()
                    && self.convert_techno_type(id, &target, rules, effects)
                    && let Some(entity) = self.substrate.entities.get_mut(id)
                    && entity.veterancy.add_promotion_experience(
                        ext.promote_experience(rank),
                        rules.general.veteran_cap,
                    )
                {
                    level = entity.veterancy.level();
                    entity.recompute_ares_attached_stats(rules, &self.interner);
                }
                if !silent {
                    self.emit_ares_promotion(id, sound, eva);
                }
                if flash
                    && flash_frames > 0
                    && let Some(entity) = self.substrate.entities.get_mut(id)
                {
                    entity.flashing.start(flash_frames);
                }
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.veterancy.store_level(level);
        }
    }

    /// UpdateVeterancy10046D2F..10046D54: VocClass::PlayAt at the
    /// Transporter's (`+0x11C`) Location when it rides in one, else its own,
    /// then VoxClass::PlayIndex. The app plays both only for a human player.
    fn emit_ares_promotion(&mut self, id: u64, sound: Option<&str>, eva: Option<&str>) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let owner = entity.owner();
        let source = entity
            .passenger_role
            .transporter()
            .and_then(|transporter| self.substrate.entities.get(transporter))
            .unwrap_or(entity);
        let world = crate::sim::movement::ground_pose::object_location(
            source,
            self.resolved_terrain.as_ref(),
        )
        .into();
        let sound_id = sound.map(|name| self.interner.intern(name));
        let eva = eva.map(|name| self.interner.intern(name));
        self.sound_events.emit(SimSoundEvent::UnitPromoted {
            owner,
            sound_id,
            world,
            eva,
        });
    }
}

impl Simulation {
    /// `HouseExt::UpdateAcademy` (`10022760`) at the four building hooks:
    /// an `IsAcademy` building (BuildingTypeExt+138) joins (`added`) or
    /// leaves its current owner's Academies. Placement (`0x00446366` ->
    /// `10023750`) and the captor (`0x004491D5` -> `10023650`) add
    /// unconditionally; Limbo (`0x00445905`) and the captured owner
    /// (`0x00448AB2`) remove through `10023690`, only while the building is
    /// marked (Object+74).
    pub(crate) fn update_ares_academy_membership(&mut self, id: u64, added: bool, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() != crate::map::entities::EntityCategory::Structure
            || (!added && !entity.lifecycle.cell_marked())
            || !self
                .entity_object_type(entity, rules)
                .is_some_and(|object| object.academy().is_academy())
        {
            return;
        }
        let owner = entity.owner();
        if let Some(house) = self.houses.get_mut(&owner) {
            house.update_ares_academy(id, added);
        }
    }

    /// `HouseExt::ApplyAcademy` (`10020750`) for a just-constructed `techno`
    /// of type `object`, at the end of its InitFromType (hooks `0x00517D51`,
    /// `0x00735678`/`0x0074689B`, `0x00413FD2`, `0x00442D1B`, after the spy
    /// veterancy). Skipped inside the A8E7AC placement scope and for an
    /// untrainable type. The class is the object's own, except that a Unit
    /// counts as aircraft when `ConsideredAircraft=` and as infantry when
    /// `Organic=`. The largest admitted bonus of the owner's academies raises
    /// the accumulator, capped at `VeteranCap`.
    pub(crate) fn apply_ares_academy(
        &self,
        techno: &mut crate::sim::game_entity::GameEntity,
        object: &crate::rules::object_type::ObjectType,
        rules: &RuleSet,
    ) {
        use crate::map::entities::EntityCategory;
        use crate::rules::ares_veterancy::AcademyClass;
        use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};
        if self.object_placement_scope_active() || !object.trainable {
            return;
        }
        let Some(house) = self.houses.get(&techno.owner()) else {
            return;
        };
        let class = match techno.category() {
            EntityCategory::Infantry => AcademyClass::Infantry,
            EntityCategory::Aircraft => AcademyClass::Aircraft,
            EntityCategory::Structure => AcademyClass::Building,
            EntityCategory::Unit if object.considered_aircraft => AcademyClass::Aircraft,
            EntityCategory::Unit if object.organic => AcademyClass::Infantry,
            EntityCategory::Unit => AcademyClass::Vehicle,
        };
        let reference = crate::rules::superweapon_type::TechnoTypeReference {
            category: object.category,
            id: object.id.clone(),
        };
        let mut bonus = NativeF64Bits::POSITIVE_ZERO;
        for &academy in house.ares_academies() {
            let Some(candidate) = self
                .substrate
                .entities
                .get(academy)
                .and_then(|building| self.entity_object_type(building, rules))
                .and_then(|building| building.academy().bonus_for(&reference, class))
            else {
                continue;
            };
            if X87::compare(X87::load_f64(candidate), X87::load_f64(bonus))
                == MaskedX87Ordering::Greater
            {
                bonus = candidate;
            }
        }
        techno
            .veterancy
            .apply_academy_bonus(bonus, rules.general.veteran_cap);
    }
}

#[cfg(test)]
#[path = "ares_veterancy_tests.rs"]
mod tests;
