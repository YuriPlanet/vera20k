//! Phobos `FootExt::UpdateTiberiumEater` (`Ext/Foot/Body.cpp:15-121`), the
//! `FootClass_AI` tail step (`Ext/Techno/Hooks.cpp:44`, hook `0x4DA54E`).
//!
//! A limbo'd or (`!UnderEMP`) deactivated/EMP'd object holds a running timer
//! with `StartTime++` and does nothing. Otherwise a running timer defers the
//! scan; when it is free, every `TiberiumEater.Cell{i}` offset is turned into
//! a world coordinate (Phobos `GetFLHAbsoluteCoords`, body matrix only), and
//! an occupied tiberium cell is reduced by `min(AmountPerCell, value / per)`
//! (or the full amount when `AmountPerCell <= 0`), paying
//! `round(amount * per * CashMultiplier * (1 + purifiers * PurifierBonus))`
//! credits, an optional money label, and an anim from the tiberium index's
//! list (`GetElements`: the per-tiberium list when set, else `Anims`), with
//! the 1-entry, 8-facing and random-count arms. A cell eaten starts the
//! timer.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/tiberium, combat's FLH helper.

use crate::rules::flh::Flh;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::combat::fire_coord::{FireSource, custom_flh_coordinate};
use crate::sim::flying_text::BountyText;
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    pub(crate) fn phobos_update_tiberium_eater(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let settings = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .and_then(|object| object.phobos_tiberium_eater.clone());
        let Some(settings) = settings else {
            return;
        };
        let now = self.session.binary_frame() as i32;
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let frozen = entity.lifecycle.in_limbo()
            || (!settings.under_emp
                && (entity.deactivated.is_offline() || entity.emp_remaining() > 0));
        if frozen {
            // `CDTimerClass::InProgress` holds only while time remains: the
            // one-frame-later anchor keeps an active delay from expiring.
            if settings.trans_delay != 0 && entity.phobos_tiberium_eater_timer().remaining(now) > 0
            {
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.phobos_tiberium_eater_timer_mut().delay_one_frame();
                }
            }
            return;
        }
        let timer = entity.phobos_tiberium_eater_timer();
        if settings.trans_delay != 0 && timer.remaining(now) > 0 {
            return;
        }
        let owner = entity.owner();
        // `PrimaryFacing.Current().GetFacing<8>()`, then the native
        // `facing >= 7 ? 0 : facing + 1` list index.
        let facing8 = crate::util::direction_tables::dir_from_facing16(
            entity.body_facing_current(self.session.binary_frame()),
        );
        let anim_facing = if facing8 >= 7 { 0 } else { facing8 + 1 };
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let owner_name = self.interner.resolve(owner).to_owned();
        let purifiers =
            crate::sim::miner::miner_system::count_purifiers_for_owner(self, rules, &owner_name);
        let purifier_bonus = f32::from_bits(rules.general.purifier_bonus.bits());
        let multiplier = settings.cash_multiplier * (1.0 + purifiers as f32 * purifier_bonus);
        let mut active = false;
        for &(offset_x, offset_y) in &settings.cells {
            let Some(coord) = self.eater_flh_coordinate(id, rules, offset_x, offset_y) else {
                continue;
            };
            let rx = coord.x.div_euclid(256);
            let ry = coord.y.div_euclid(256);
            let (Ok(rx), Ok(ry)) = (u16::try_from(rx), u16::try_from(ry)) else {
                continue;
            };
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                continue;
            };
            if terrain.cell(rx, ry).is_none() {
                continue;
            }
            let Some(overlay_registry) = overlay_registry else {
                continue;
            };
            let overlay = self.overlay_grid.as_ref().map(|grid| grid.cell(rx, ry));
            let Some(cell_overlay) = overlay.and_then(|cell| cell.overlay_id) else {
                continue;
            };
            let overlay_data = overlay.map_or(0, |cell| cell.overlay_data);
            let Ok(contained) = crate::map::tiberium_cell::tiberium_value(
                Some(cell_overlay),
                overlay_data,
                overlay_registry,
                &rules.tiberium_types,
            ) else {
                continue;
            };
            if contained == 0 {
                continue;
            }
            let Ok(Some(tiberium_type)) =
                overlay_registry.tiberium_type_for_overlay(&rules.tiberium_types, cell_overlay)
            else {
                continue;
            };
            let Some(value_per) = rules
                .tiberium_types
                .get(tiberium_type)
                .map(|kind| kind.value)
                .filter(|&value| value > 0)
            else {
                continue;
            };
            let amount = (contained as f64 / value_per as f64) as i32;
            let amount = if settings.amount_per_cell > 0 {
                settings.amount_per_cell.min(amount)
            } else {
                amount
            };
            if amount <= 0 {
                continue;
            }
            // `CellClass::ReduceTiberium(amount)`.
            let reduction = Simulation::reduce_tiberium_at_with_native_context(
                &self.terrain_objects,
                &self.ore_growth_config,
                &mut self.ore_growth_state,
                &self.session,
                &mut self.scenario_rng,
                &self.substrate,
                &mut self.resolved_terrain,
                &mut self.overlay_grid,
                &mut self.dirty_cells,
                (rx, ry),
                amount,
                rules,
                Some(overlay_registry),
            );
            if reduction.is_err() {
                continue;
            }
            active = true;
            let product = amount.wrapping_mul(value_per);
            let value = (product as f32 * multiplier).round() as i32;
            crate::sim::credit_income::transact_money(&mut self.houses, owner, value);
            let cell_x = i32::from(rx) * 256 + 128;
            let cell_y = i32::from(ry) * 256 + 128;
            let z = location.z.max(coord.z);
            if settings.display {
                effects.emit_bounty_text(
                    self,
                    rules,
                    BountyText {
                        entity_id: Some(id),
                        amount: value,
                        coord: crate::sim::projectile::ProjectileCoord::new(cell_x, cell_y, z),
                        z_offset: 0,
                        display_to: Some(settings.display_to_house),
                        display_owner: Some(owner),
                        pixel_offset: settings.display_offset,
                    },
                );
            }
            let index = usize::from(tiberium_type.0);
            let list = settings
                .anims_tiberiums
                .get(index)
                .and_then(|slot| slot.as_ref())
                .unwrap_or(&settings.anims);
            // The native vector holds only resolvable AnimTypes; unknown
            // tokens are dropped before the 1/8/random arms count them.
            let resolved: Vec<&str> = list
                .iter()
                .map(String::as_str)
                .filter(|name| rules.has_anim_type(name))
                .collect();
            let picked = match resolved.len() {
                0 => None,
                1 => Some(resolved[0].to_owned()),
                8 => Some(resolved[usize::from(anim_facing)].to_owned()),
                count => {
                    let index = self
                        .scenario_rng
                        .next_range_i32_inclusive(0, count as i32 - 1);
                    Some(resolved[index as usize].to_owned())
                }
            };
            if let Some(name) = picked {
                let type_id = self.interner.intern(&name);
                self.spawn_owned_anim(
                    rules,
                    type_id,
                    AnimWorldCoord {
                        x: coord.x,
                        y: coord.y,
                        z: coord.z,
                    },
                    owner,
                    settings.anim_move.then_some(id),
                    None,
                );
            }
        }
        if active && settings.trans_delay != 0 {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity
                    .phobos_tiberium_eater_timer_mut()
                    .start(now, settings.trans_delay);
            }
        }
    }

    /// `TechnoExt::GetFLHAbsoluteCoords(pThis, {x, y, 0}, false)`: the body
    /// matrix only, no turret offset.
    fn eater_flh_coordinate(
        &self,
        id: u64,
        rules: &RuleSet,
        x: i32,
        y: i32,
    ) -> Option<crate::sim::components::DriveCoord> {
        let entity = self.substrate.entities.get(id)?;
        let object: &ObjectType = self.object_type(entity.type_ref(), rules)?;
        let snap = FireSource::of_entity(entity);
        custom_flh_coordinate(
            self,
            rules,
            &snap,
            object,
            Flh {
                forward: x,
                lateral: y,
                height: 0,
            },
            false,
        )
    }
}
