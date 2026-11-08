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
//! - Chain reactions: Ares' `CellChainReact @ 10050DD0` fills the stock RET
//!   of `CellChainReaction__Request @ 0x00489270`. Its callers are
//!   Apply_area_damage's cell spread (Ares hook `0x48964F`, `10051000`),
//!   the SpawnsTiberium terrain death (`0x0071BACD`) and the wave cell tail
//!   (`0x0075F44A..0x0075F45F`).
//!
//! Retail sets none of TiberiumDamageEnabled, TiberiumHealEnabled,
//! TiberiumSpill, TiberiumRemains or TiberiumExplosive, and no tiberium
//! overlay has `ChainReaction=yes`, so all stay dormant there.
//!
//! - Visceroid: a Foot the tiberium damage kills may become a neutral
//!   `[General] SmallVisceroid` (`TechnoExt::SpawnVisceroid @ 100468B0`,
//!   called from `10049280`) on a map whose `[Basic]
//!   TiberiumDeathToVisceroid` (Scenario+0x34B3, default true) holds.

use crate::map::entities::EntityCategory;
use crate::rules::ares_tiberium::HealStepKind;
use crate::rules::object_type::{Ability, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::rules::tiberium_type::{TiberiumType, TiberiumTypeId};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::miner::CargoBale;
use crate::sim::world::{FrameEffects, PlacementEvidence, Simulation};
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

/// [`crate::sim::tiberium::tiberium_cell_view`] for an Ares tiberium effect.
/// A malformed native Image refuses the effect's read of this cell, as an
/// optional action owner does ([`crate::sim::crates::RuntimeOverlayError`]).
fn ares_cell_view(
    grid: &crate::sim::overlay_grid::OverlayGrid,
    registry: &OverlayTypeRegistry,
    rules: &RuleSet,
    cell: (u16, u16),
) -> Option<crate::sim::tiberium::TiberiumCellView> {
    crate::sim::tiberium::tiberium_cell_view(grid, registry, &rules.tiberium_types, cell)
        .unwrap_or_else(|error| {
            log::error!("Ares tiberium cell read refused: {error}");
            None
        })
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
        let view = ares_cell_view(grid, rules.overlay_registry(), rules, (rx, ry))?;
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
    /// no attacker, distance 0 and both flags false. When it kills the Foot,
    /// a set `[General] SmallVisceroid` may take its place at the GetCoords
    /// taken before the damage ([`Self::ares_spawn_visceroid`], chance
    /// `TiberiumTransmogrify`, else `[General] TiberiumTransmogrify`).
    /// Returns whether the call killed it, in which case Per_Cell_Process
    /// returns at once (`0x4D8F29`).
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
        let transmogrify = object
            .techno_combat()
            .tiberium()
            .tiberium_transmogrify(rules.general.tiberium_transmogrify);
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
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
        let killed = self
            .substrate
            .entities
            .get(id)
            .is_none_or(|entity| !entity.lifecycle.object_alive());
        // 3.0 skips the visceroid when SmallVisceroid is null.
        if killed && let Some(visceroid) = rules.general.small_visceroid.as_deref() {
            self.ares_spawn_visceroid(
                coord,
                visceroid,
                transmogrify,
                false,
                rules,
                registry,
                effects,
            );
        }
        killed
    }

    /// Ares 3.0 `TechnoExt::SpawnVisceroid @ 100468B0`. Unless
    /// `ignore_map_flag`, it needs the map's `TiberiumDeathToVisceroid`
    /// (Scenario+0x34B3). It then draws `RandomRanged(0, 99)` from the
    /// Scenario RNG; when the cell of `coord` holds no unit (OccupationFlags
    /// bit 0x20) and the draw is below `chance`, the House of the `Neutral`
    /// country (`HouseClass::FindNeutral`: `HouseTypeClass::FindIndexOfName
    /// @ 0x005117D0`, `FindByCountryIndex @ 0x00502D30`) creates `visceroid`
    /// and Puts it at `coord` facing 0 with `IKnowWhatImDoing` raised.
    /// Returns whether it created the object.
    ///
    /// RESIDUAL: 3.0 keeps (leaks) an object whose Put fails and still
    /// returns true; VERA deletes it. A coordinate off the map creates none.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ares_spawn_visceroid(
        &mut self,
        coord: DriveCoord,
        visceroid: &str,
        chance: i32,
        ignore_map_flag: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if !ignore_map_flag && !self.session.tiberium_death_to_visceroid() {
            return false;
        }
        let roll = self.scenario_rng.next_range_i32_inclusive(0, 99);
        let (Ok(rx), Ok(ry)) = (u16::try_from(coord.x / 256), u16::try_from(coord.y / 256)) else {
            return false;
        };
        let unit_in_cell = self.substrate.raw_cell_occupation.ground_bits(rx, ry)
            & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
            != 0;
        if unit_in_cell || roll >= chance {
            return false;
        }
        let Some(owner) = rules
            .country_index_of_name("Neutral")
            .and_then(|country| self.house_by_country_index(country, rules))
        else {
            return false;
        };
        let owner = self.interner.resolve(owner).to_owned();
        let level =
            u8::try_from(coord.z / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS).unwrap_or(0);
        let Some(created) = self
            .construct_object_limbo_at_height(visceroid, &owner, rx, ry, 0, level, rules, effects)
        else {
            return false;
        };
        let placed = self.with_object_placement_scope(|sim| {
            sim.reveal_constructed_object_at_coord_with_overlay_context(
                created,
                coord,
                0,
                PlacementEvidence::FootEntryAdmitted,
                rules,
                registry,
                effects,
            )
            .is_some()
        });
        if !placed {
            self.discard_constructed_limbo(created, rules, effects);
        }
        true
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
    pub(crate) fn ares_spill_tiberium(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
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
        self.ares_increase_tiberium_cells(rules, |sim, placed| {
            for direction in [9, 2, 7, 1, 4, 3, 0, 5, 6] {
                let amount = sim.scenario_rng.next_range_i32_inclusive(0, 2);
                placed.extend(
                    crate::sim::tiberium::place_tiberium(
                        sim,
                        rules,
                        rules.overlay_registry(),
                        neighbour_cell(center, direction),
                        TiberiumTypeId(0),
                        amount as u8,
                        effects,
                    )?
                    .map(|placement| placement.cell()),
                );
                value -= amount;
                if value <= 0 {
                    break;
                }
            }
            Ok(())
        });
    }

    /// Ares 3.0 1004B050 (hook 0x702216), after the Stun: a dying techno
    /// whose `TiberiumRemains` holds (default: `TiberiumHeal=yes` and
    /// `[General] TiberiumHealEnabled`) leaves `RandomRanged(0, 2)` Riparius
    /// on the four orthogonal neighbours (directions 0, 2, 4, 6) and the
    /// centre (8) of the cell of its GetCoords.
    pub(crate) fn ares_tiberium_remains(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
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
        self.ares_increase_tiberium_cells(rules, |sim, placed| {
            for direction in (0..10).step_by(2) {
                let amount = sim.scenario_rng.next_range_i32_inclusive(0, 2);
                placed.extend(
                    crate::sim::tiberium::place_tiberium(
                        sim,
                        rules,
                        rules.overlay_registry(),
                        neighbour_cell(center, direction),
                        TiberiumTypeId(0),
                        amount as u8,
                        effects,
                    )?
                    .map(|placement| placement.cell()),
                );
            }
            Ok(())
        });
    }

    /// One Ares batch of `CellClass::IncreaseTiberium @ 0x00487190` calls
    /// ([`crate::sim::tiberium::place_tiberium`] on the scenario RNG): `body`
    /// records each placed cell, which then publish together, as the crate
    /// arm's do. A world without terrain or an overlay grid has no Cell to
    /// write: `body` does not run and nothing is drawn. A malformed Image
    /// stops the batch; earlier placements remain and still publish.
    pub(crate) fn ares_increase_tiberium_cells(
        &mut self,
        rules: &RuleSet,
        body: impl FnOnce(
            &mut Self,
            &mut Vec<(u16, u16)>,
        ) -> Result<(), crate::sim::crates::RuntimeOverlayError>,
    ) {
        if self.resolved_terrain.is_none() || self.overlay_grid.is_none() {
            return;
        }
        let mut placed = Vec::new();
        let result = body(self, &mut placed);
        self.publish_tiberium_cells(rules, rules.overlay_registry(), &placed);
        if let Err(error) = result {
            log::error!("Ares tiberium placement stopped: {error}");
        }
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

impl Simulation {
    /// Ares 3.0 DamageArea_ChainReaction (10051000, hook 0x48964F): in
    /// Apply_area_damage's cell spread walk, a cell whose `ChainReaction=yes`
    /// overlay the blast may reduce (a Tiberium overlay needs a `Tiberium=yes`
    /// warhead, and the call must affect tiberium) chain-reacts before its
    /// Reduce_Tiberium. The spread cell is the impact cell plus `(dx, dy)`
    /// through the same `MapClass::GetCellAt` lookup the walk uses; the
    /// shared Dummy has no overlay. A non-tiberium overlay passes the hook
    /// but CellChainReact ignores it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ares_area_damage_chain_reaction(
        &mut self,
        impact: (u16, u16),
        offset: (i16, i16),
        warhead: &crate::rules::warhead_type::WarheadType,
        affects_tiberium: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if !affects_tiberium || !warhead.tiberium {
            return;
        }
        let requested = (
            (impact.0 as i16).wrapping_add(offset.0),
            (impact.1 as i16).wrapping_add(offset.1),
        );
        let cell = match self.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                match cells.lookup(requested) {
                    real @ crate::map::cell_index::NativeCellIdentity::Real(_) => {
                        let coord = cells.coord(real);
                        (coord.0 as u16, coord.1 as u16)
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => return,
                }
            }
            None => (requested.0 as u16, requested.1 as u16),
        };
        self.ares_cell_chain_react(cell, rules, registry, effects);
    }

    /// `CellClass::GetCoords` (`0x00486840`) of the cell `MapClass::GetCellAt`
    /// returns for `cell`.
    fn cell_get_coords(&self, cell: (u16, u16)) -> DriveCoord {
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        crate::sim::movement::navcom::target_cell_coord(cell.0, cell.1, cells.as_ref())
    }

    /// Whether `cell`'s overlay is `ChainReaction=yes` (the stock gate of the
    /// wave cell tail, `0x0075F44A`).
    pub(crate) fn overlay_chain_reacts(
        &self,
        cell: (u16, u16),
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        self.overlay_grid
            .as_ref()
            .zip(registry)
            .and_then(|(grid, registry)| {
                grid.cell(cell.0, cell.1)
                    .overlay_id
                    .and_then(|id| registry.flags(id))
            })
            .is_some_and(|flags| flags.chain_reaction)
    }

    /// Ares 3.0 CellChainReact (10050DD0, the body of `0x00489270`): a cell
    /// holding tiberium under a `ChainReaction=yes` overlay at density
    /// (Powerup) 2 or more reacts when `RandomRanged(0, 99) < 5 * density`.
    /// It then loses `density / 2`, which times the mineral's `Power` is the
    /// damage. The mineral's ExplosionWarhead (else C4Warhead) picks a
    /// damage anim for `4 * damage` on the cell's LandType, built at the
    /// cell's GetCoords (delay 0, loop 1, flags 0x600, zAdjust 0), and deals
    /// Apply_area_damage there (no source or house, tiberium untouched).
    /// With an `INVISO` AnimType, each neighbour in directions 0..7 holding
    /// tiberium above density 2 gets an `INVISO` anim with 80% chance
    /// (`RandomRanged(0, 99) < 80`), delayed `RandomRanged(15, 120)`
    /// frames; INVISO's anim damage carries the reaction on. A cell that was
    /// full grown (11) re-registers for growth (`AddToGrowthQueue @
    /// 0x007235A0`).
    ///
    /// RESIDUAL: an explicit `ExplosionWarhead=none` hands the native anim
    /// selector and Apply_area_damage a null warhead (a crash); VERA builds no
    /// anim and deals no damage then.
    pub(crate) fn ares_cell_chain_react(
        &mut self,
        cell: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(registry) = registry else {
            return;
        };
        if !self.overlay_chain_reacts(cell, Some(registry)) {
            return;
        }
        let Some(view) = self
            .overlay_grid
            .as_ref()
            .and_then(|grid| ares_cell_view(grid, registry, rules, cell))
        else {
            return;
        };
        let density = view.overlay_data;
        let Some(tiberium) = rules.tiberium_types.get(view.tiberium_type) else {
            return;
        };
        if density < 2
            || self.scenario_rng.next_range_i32_inclusive(0, 99) >= 5 * i32::from(density)
        {
            return;
        }
        let was_full_grown = density >= 11;
        let delta = density / 2;
        let damage = tiberium.power.wrapping_mul(i32::from(delta));
        if let Some(grid) = self.overlay_grid.as_mut() {
            grid.set_overlay_data(cell.0, cell.1, density - delta);
        }
        self.dirty_cells.mark_tactical_once(cell);

        let coord = self.cell_get_coords(cell);
        let impact = crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z);
        let anim_coord = crate::sim::anim_class::AnimWorldCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        };
        let c4 = Some(rules.bridge_warheads.c4_name.as_str());
        if let Some(warhead) = tiberium
            .ares()
            .explosion_warhead(c4)
            .and_then(|name| rules.warhead(name))
        {
            let land = crate::sim::combat::detonation_anim::land_at(self, impact);
            if let Some(anim) = crate::sim::combat::detonation_anim::select(
                self,
                rules,
                warhead,
                damage.wrapping_mul(4),
                land,
                impact,
                false,
            ) {
                let anim = self.interner.resolve(anim).to_owned();
                self.spawn_named_anim(rules, &anim, anim_coord, 0, 0x600, 0);
            }
            let warhead_ref = self.interner.intern(&warhead.id);
            crate::sim::combat::world_receiver::apply_area_damage(
                self,
                rules,
                Some(registry),
                impact,
                damage,
                warhead,
                false,
                (RAD_NO_ATTACKER, None, warhead_ref),
                effects,
            );
        }

        // `AnimTypeClass::Find("INVISO")`: no AnimType, no loop and no draws.
        if rules.art().anim_runtime_config("INVISO").is_some() {
            let center = (i32::from(cell.0), i32::from(cell.1));
            for direction in 0..8 {
                let neighbour = neighbour_cell(center, direction);
                let dense = self.overlay_grid.as_ref().is_some_and(|grid| {
                    ares_cell_view(grid, registry, rules, neighbour)
                        .is_some_and(|view| view.overlay_data > 2)
                });
                if !dense || self.scenario_rng.next_range_i32_inclusive(0, 99) >= 80 {
                    continue;
                }
                let delay = self.scenario_rng.next_range_i32_inclusive(15, 120) as u16;
                let at = self.cell_get_coords(neighbour);
                self.spawn_named_anim(
                    rules,
                    "INVISO",
                    crate::sim::anim_class::AnimWorldCoord {
                        x: at.x,
                        y: at.y,
                        z: at.z,
                    },
                    delay,
                    0x600,
                    0,
                );
            }
        }

        if was_full_grown
            && let Some(grid) = self.overlay_grid.as_ref()
            && let Err(error) = self.ore_growth_state.add_native_growth_queue_cell(
                grid,
                registry,
                &rules.tiberium_types,
                self.resolved_terrain.as_ref(),
                self.ore_growth_config.grows(),
                view.tiberium_type,
                cell.0,
                cell.1,
                self.session.binary_frame(),
                &mut self.scenario_rng,
            )
        {
            log::error!("Ares chain reaction growth queue refused: {error}");
        }
    }
}

#[cfg(test)]
#[path = "ares_tiberium_tests.rs"]
mod tests;
