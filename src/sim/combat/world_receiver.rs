//! World-owned synchronous receiver execution.
//!
//! Gameplay authorities stay in Simulation across nested receiver/lifecycle
//! calls. Local contexts retain call arguments, recursion guards and ordered
//! publication receipts; they never own competing gameplay state.

use super::world_fire::{object_get_coords, retaliation_reaches};
use super::*;
use crate::sim::movement::{DamageRetaliation, ScatterFlags};
use crate::sim::world::techno_ai_cloak::emit_configured_cloak_sound;
use crate::sim::world::{FrameEffects, Simulation};

#[path = "area_rocker.rs"]
mod area_rocker;

/// FootClass::AI `0x004DA554..0x004DA635`: after the whole Techno AI and
/// before locomotor Process, read this Foot's current cell and synchronously
/// enter its concrete ReceiveDamage. Unit/Infantry/Aircraft call Foot at
/// `0x0073647B` / `0x0051BC9F` / `0x00414DA3`; buildings never reach it.
/// Numeric/receiver controls: `tools/spatial_oracle/radiation_damage_boundary`.
///
/// With the Phobos capability the cell's per-site levels group by bound
/// radiation type, as native groups `CellExt::RadLevels` by `RadTypeClass*`:
/// each type's sites sort by level descending, the summed level caps at the
/// type's `RadLevelMax`, and each site commits `min(level, remain) ×
/// RadLevelFactor` through the type's own `RadSiteWarhead`. A commit that
/// kills the target ends the whole loop, as native's `ApplyRadiationDamage`
/// returning false does.
pub(crate) fn apply_foot_radiation(
    world: &mut Simulation,
    rules: &RuleSet,
    stable_id: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> Option<crate::sim::world::damage_consequences::DamageCommitReceipt> {
    if !radiation_enabled(world) {
        return None;
    }
    // Phobos `UseGlobalRadApplicationDelay`: when false, each bound type's
    // own `RadApplicationDelay` gates its sites below, so the shared gate is
    // skipped entirely (native's `!UseGlobalRadApplicationDelay` arm).
    #[cfg(feature = "phobos")]
    let delay_admitted = !rules.radiation.use_global_rad_application_delay
        || world
            .session
            .binary_frame()
            .is_multiple_of(rules.radiation.application_delay as u32);
    #[cfg(not(feature = "phobos"))]
    let delay_admitted = world
        .session
        .binary_frame()
        .is_multiple_of(rules.radiation.application_delay as u32);
    if !delay_admitted {
        return None;
    }
    let entity = world.substrate.entities.get(stable_id)?;
    if !matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    ) || !entity.lifecycle.object_alive()
        || entity.mover_facts.immune_to_radiation()
        || crate::sim::movement::air_movement::is_high_flying(
            entity,
            world.resolved_terrain.as_ref(),
            Some((rules, &world.interner)),
        )
        || entity.lifecycle.in_limbo()
    {
        return None;
    }
    // GetCoords -> signed /256 -> packed CellStruct -> Map5657A0. Use the
    // canonical cell identity, including stride aliases and dummy stamping.
    let coord = object_get_coords(world, stable_id)?;
    let cell = crate::sim::cell_rect::get_cellclass_fallback(
        world.resolved_terrain.as_ref(),
        coord.x / 256,
        coord.y / 256,
    );
    #[cfg(not(feature = "phobos"))]
    {
        let crate::sim::cell_rect::CellRef::Real(cell) = cell else {
            return None;
        };
        // FootClass::AI @ 0x004DA5EB..0x004DA629 reads the retained
        // Cell487CB0 total once and makes one direct receiver call. Site
        // membership is irrelevant, including residual F0 after site deletion.
        let level = world
            .radiation
            .damaging_level((cell.rx, cell.ry), rules.radiation.level_max);
        if level <= 0 {
            return None;
        }
        let warhead = rules.warhead(&rules.radiation.site_warhead)?;
        let warhead_id = world.interner.intern(&warhead.id);
        let receiver = EntityDamageEvent::direct_receiver(
            stable_id,
            scale_rad_level(level, rules.radiation.level_factor),
            0,
            RAD_NO_ATTACKER,
            None,
            warhead_id,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: true,
            },
        );
        return Some(world.commit_direct_damage_receiver(
            rules,
            overlay_registry,
            receiver,
            effects,
        ));
    }
    #[cfg(feature = "phobos")]
    {
        // Group the cell's per-site levels by bound radiation type. A type whose
        // warhead does not resolve contributes nothing, as native's
        // `!GetWarhead()` skip does.
        let mut groups: Vec<(
            Option<String>,
            crate::rules::phobos_radiation::EffectiveRadiation<'_>,
            Vec<((u16, u16), f64)>,
        )> = Vec::new();
        if let crate::sim::cell_rect::CellRef::Real(cell) = cell {
            for (site_center, level) in world.radiation.cell_site_levels((cell.rx, cell.ry)) {
                if !(level > 0.0) {
                    continue;
                }
                let Some(site) = world.radiation.site_at(site_center) else {
                    continue;
                };
                let effective =
                    crate::rules::phobos_radiation::effective_radiation(rules, site.rad_type());
                if rules.warhead(effective.site_warhead).is_none() {
                    continue;
                }
                #[cfg(feature = "phobos")]
                {
                    if !rules.radiation.use_global_rad_application_delay
                        && (effective.application_delay <= 0
                            || !world
                                .session
                                .binary_frame()
                                .is_multiple_of(effective.application_delay as u32))
                    {
                        continue;
                    }
                }
                let bound = site.rad_type().map(str::to_owned);
                if let Some(group) = groups
                    .iter_mut()
                    .find(|(name, _, _)| name.as_deref() == bound.as_deref())
                {
                    group.2.push((site_center, level));
                } else {
                    groups.push((bound, effective, vec![(site_center, level)]));
                }
            }
        }
        // Foot4DA5FA..4DA60A multiplies the clamped integer by the stored double
        // factor under PC53/chop, then calls Math::ftol before its direct receiver.
        let mut last_receipt = None;
        'groups: for (_, effective, mut sites) in groups {
            // Native stable-sorts each type's sites by level, descending.
            sites.sort_by(|left, right| {
                right
                    .1
                    .partial_cmp(&left.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut level_sum = 0_i32;
            for (site_center, level) in sites {
                let remain = effective.level_max - level_sum;
                let (level, damage) = rad_damage(level, remain, effective.level_factor);
                let Some(warhead) = rules.warhead(effective.site_warhead) else {
                    continue;
                };
                let warhead_id = world.interner.intern(&warhead.id);
                // Phobos keeps the round's firing house and invoker on the site
                // when the bound type asks for them (`RadHasOwner`,
                // `RadHasInvoker`); the receiver call then carries them.
                let (attacker, source_house) =
                    world
                        .radiation
                        .site_at(site_center)
                        .map_or((RAD_NO_ATTACKER, None), |site| {
                            (
                                site.invoker().unwrap_or(RAD_NO_ATTACKER),
                                site.firing_house(),
                            )
                        });
                // Phobos `RadSiteWarhead.Detonate`: the site detonates its warhead
                // at the target instead of calling the receiver directly.
                // `.Detonate.Full` additionally runs the ordinary detonation tail
                // (the anim and flash `WarheadTypeExt::DetonateAt` reaches through
                // `BulletExt::Detonate`); the plain form is the bare area call that
                // `DamageAreaWithTarget` makes.
                #[cfg(feature = "phobos")]
                let detonate = effective.site_warhead_detonate;
                #[cfg(not(feature = "phobos"))]
                let detonate = false;
                if detonate {
                    let Some(coord) = object_get_coords(world, stable_id) else {
                        continue;
                    };
                    let impact = ProjectileCoord::new(coord.x, coord.y, coord.z);
                    let receipt = apply_area_damage(
                        world,
                        rules,
                        overlay_registry,
                        impact,
                        damage,
                        warhead,
                        true,
                        (attacker, source_house, warhead_id),
                        effects,
                    );
                    #[cfg(not(feature = "phobos"))]
                    let _ = receipt;
                    #[cfg(feature = "phobos")]
                    if effective.site_warhead_detonate_full {
                        // `BulletExt::Detonate` reaches the same
                        // `BulletClass_Logics_DamageAnimSelected` (`0x469C46`)
                        // creation tail as a fired Bullet; its default bullet
                        // type is not Inviso and the RadInvoker is the owner.
                        // The fake Bullet's DamageArea is the call just made,
                        // so its receipt gates the land check and the Iron
                        // Curtain substitution exactly as the Bullet path.
                        let ignore_units = warhead
                            .phobos_anim
                            .conventional_ignore_units(&rules.phobos_warhead_anim);
                        let land = crate::sim::combat::detonation_anim::bullet_land(
                            world,
                            rules,
                            impact,
                            false,
                            receipt.area_result == AreaDamageResult::ReceiverDispatched,
                            ignore_units,
                        );
                        let selection = crate::sim::combat::detonation_anim::select_with_context(
                            world,
                            rules,
                            warhead,
                            damage,
                            land,
                            impact,
                            receipt.crit_active,
                        );
                        if receipt.area_result == AreaDamageResult::IronCurtain {
                            if !rules.general.weapon_nullify_anim.is_empty() {
                                let type_id =
                                    world.interner.intern(&rules.general.weapon_nullify_anim);
                                let effect = crate::sim::combat::detonation_anim::placed_effect(
                                    type_id, impact,
                                );
                                crate::sim::world::damage_consequences::admit_explosion_effect(
                                    world, rules, effect,
                                );
                            }
                        } else if let Some(selection) = selection {
                            for effect in crate::sim::combat::detonation_anim::bullet_effects(
                                world,
                                rules,
                                warhead,
                                warhead_id,
                                selection,
                                false,
                                impact,
                                attacker,
                                receipt.crit_active,
                                source_house,
                            ) {
                                crate::sim::world::damage_consequences::admit_explosion_effect(
                                    world, rules, effect,
                                );
                            }
                        }
                    }
                } else {
                    let receiver = EntityDamageEvent::direct_receiver(
                        stable_id,
                        damage,
                        0,
                        attacker,
                        source_house,
                        warhead_id,
                        ReceiverCallFlags {
                            ignore_defenses: false,
                            arg6: true,
                        },
                    );
                    // Native calls the concrete receiver directly: Apply_area_damage's
                    // marked/positive-health gates must not suppress this Foot call.
                    last_receipt = Some(world.commit_direct_damage_receiver(
                        rules,
                        overlay_registry,
                        receiver,
                        effects,
                    ));
                }
                if !world
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
                {
                    break 'groups;
                }
                if level >= remain {
                    break;
                }
                level_sum += level;
            }
        }
        last_receipt
    }
}

/// `min(level, remain) * RadLevelFactor` with the native clamp and masked
/// double-to-int conversions. The Foot loop (`0x004DA5FA`) and the building
/// loop (`0x0043FB23`) scale a cell's summed level the same way.
#[cfg(feature = "phobos")]
fn rad_damage(level: f64, remain: i32, factor: f64) -> (i32, i32) {
    let level = crate::sim::radiation::clamp_level(level, remain);
    (level, scale_rad_level(level, factor))
}

/// FootClass::AI @ 0x004DA5FA..0x004DA60A scales the converted cell
/// level before its direct ReceiveDamage call. Phobos shares this arithmetic.
fn scale_rad_level(level: i32, factor: f64) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
    X87::ftol_i32_low_masked(X87::mul(
        X87::load_i32(level),
        X87::load_f64(NativeF64Bits::from_bits(factor.to_bits())),
    ))
}

/// Phobos `BuildingClass_AI_Radiation @ 0x0043FB23`, the branch that follows
/// the operational edge sample at `0x0043FB20` and runs for every live
/// building each frame: each foundation cell's sites group by bound type,
/// each type clamps its summed level at `RadLevelMax` and applies its
/// `RadLevelFactor` through its own `RadSiteWarhead`. Retail has no such path
/// (Phobos labels it "Too OP"), so a build without the capability keeps
/// buildings immune.
///
/// `RadBuildingDamageMaxCount` is read and retained: Phobos' hook tests a
/// per-site counter it never increments, so the key has no observable effect
/// there and none here.
#[cfg(feature = "phobos")]
pub(crate) fn apply_building_radiation(
    world: &mut Simulation,
    rules: &RuleSet,
    stable_id: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> Option<crate::sim::world::damage_consequences::DamageCommitReceipt> {
    if !radiation_enabled(world) {
        return None;
    }
    let (origin, foundation, exempt) = {
        let entity = world.substrate.entities.get(stable_id)?;
        if entity.category() != EntityCategory::Structure
            || !entity.lifecycle.object_alive()
            || entity.lifecycle.in_limbo()
            || entity.is_warped_out()
            || entity.ai_frozen()
        {
            return None;
        }
        let object = world.object_type(entity.type_ref(), rules)?;
        (
            (entity.position.rx(), entity.position.ry()),
            object.foundation.clone(),
            object.immune_to_radiation,
        )
    };
    if exempt {
        return None;
    }
    let frame = world.session.binary_frame();
    // The shared gate is the building-specific delay; native's `delay == 0`
    // leaves buildings untouched even on a boundary frame.
    if rules.radiation.use_global_rad_application_delay {
        let delay = rules.radiation.application_delay_building;
        if delay <= 0 || !frame.is_multiple_of(delay as u32) {
            return None;
        }
    }
    let mut last_receipt = None;
    'cells: for cell in crate::sim::crew_survival::foundation_cells(origin.0, origin.1, &foundation)
    {
        let mut groups: Vec<(
            Option<String>,
            crate::rules::phobos_radiation::EffectiveRadiation<'_>,
            Vec<((u16, u16), f64)>,
        )> = Vec::new();
        for (site_center, level) in world.radiation.cell_site_levels(cell) {
            if !(level > 0.0) {
                continue;
            }
            let Some(site) = world.radiation.site_at(site_center) else {
                continue;
            };
            let effective =
                crate::rules::phobos_radiation::effective_radiation(rules, site.rad_type());
            if rules.warhead(effective.site_warhead).is_none() {
                continue;
            }
            if !rules.radiation.use_global_rad_application_delay {
                let delay = effective.application_delay_building;
                if delay <= 0 || !frame.is_multiple_of(delay as u32) {
                    continue;
                }
            }
            let bound = site.rad_type().map(str::to_owned);
            if let Some(group) = groups
                .iter_mut()
                .find(|(name, _, _)| name.as_deref() == bound.as_deref())
            {
                group.2.push((site_center, level));
            } else {
                groups.push((bound, effective, vec![(site_center, level)]));
            }
        }
        for (_, effective, mut sites) in groups {
            // Native stable-sorts each type's sites by level, descending.
            sites.sort_by(|left, right| {
                right
                    .1
                    .partial_cmp(&left.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut level_sum = 0_i32;
            for (_, level) in sites {
                let remain = effective.level_max - level_sum;
                let (level, damage) = rad_damage(level, remain, effective.level_factor);
                let Some(warhead) = rules.warhead(effective.site_warhead) else {
                    continue;
                };
                let warhead_id = world.interner.intern(&warhead.id);
                let receiver = EntityDamageEvent::direct_receiver(
                    stable_id,
                    damage,
                    0,
                    RAD_NO_ATTACKER,
                    None,
                    warhead_id,
                    ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: true,
                    },
                );
                last_receipt = Some(world.commit_direct_damage_receiver(
                    rules,
                    overlay_registry,
                    receiver,
                    effects,
                ));
                if !world
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
                {
                    break 'cells;
                }
                if level >= remain {
                    break;
                }
                level_sum += level;
            }
        }
    }
    last_receipt
}

fn respond_to_base_attack(
    world: &mut Simulation,
    rules: &RuleSet,
    _site: BaseDefenseResponseCallSite,
    victim_id: u64,
    attacker_id: u64,
    effects: FrameEffects<'_>,
) {
    #[cfg(test)]
    if let Some(fixture) = world.receiver_fixture.as_mut() {
        let victim = world
            .substrate
            .entities
            .get(victim_id)
            .expect("response victim represented");
        let last_attacker_house_index = world.houses.get(&victim.owner()).map_or(-1, |house| {
            house.strategy_emergency().last_attacker_house_index()
        });
        fixture
            .trace
            .entries
            .push(super::receiver_fixture::BaseDefenseResponseTraceEntry {
                site: _site,
                victim_id,
                health: victim.health.current(),
                last_attacker_house_index,
            });
        return;
    }
    base_defense_response::respond_to_base_attack(world, rules, victim_id, attacker_id, effects);
}

pub(crate) fn collect_area(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    cell: (u16, u16),
    damage: i32,
    warhead: &WarheadType,
    affects_tiberium: bool,
    origin: (u64, Option<InternedId>, InternedId),
    air_impact: Option<combat_aoe::AoEAirImpact>,
    impact_z: i32,
    effects: FrameEffects<'_>,
) -> combat_aoe::AoEDamageResult {
    #[cfg(not(test))]
    let include_terrain_objects = true;
    #[cfg(test)]
    let include_terrain_objects = world
        .receiver_fixture
        .as_ref()
        .is_none_or(|fixture| fixture.terrain_collection);
    let playfield_bounds = world.playfield_bounds();
    let rule_handles = world.resolved_rule_handles();
    let Some(mut collector) = combat_aoe::AoECollector::begin(
        &world.substrate.entities,
        cell.0,
        cell.1,
        damage,
        warhead,
        rules,
        &world.interner,
        rule_handles,
        origin.into(),
        &combat_aoe::AoELayerContext {
            occupancy: Some(&world.substrate.occupancy),
            terrain: world.resolved_terrain.as_mut(),
            overlay_grid: world.overlay_grid.as_mut(),
            overlay_registry,
            scenario_rng: Some(&mut world.scenario_rng),
            air_impact,
            impact_z,
        },
        world.session.no_damage(),
    ) else {
        return combat_aoe::AoEDamageResult::default();
    };
    for &(dx, dy) in cell_spread::splash_cells(warhead.cell_spread_f64) {
        let (spread_cell, detached_cell) = {
            let mut prelude = crate::sim::world::simulation_area_damage_cell_prelude(
                rules,
                warhead,
                damage,
                affects_tiberium,
                world.session.no_damage(),
                &mut world.ore_growth_state,
                world.terrain_objects.spawning_cells(),
                world.terrain_objects.cell_index(),
                world.session.binary_frame(),
                world.ore_growth_config.spreads(),
                &mut world.dirty_cells,
                &mut world.zone_grid,
                &mut world.path_grid,
                world.bridge_state.as_ref(),
                playfield_bounds,
            );
            #[cfg(test)]
            let mut deferred_prelude = world.receiver_fixture.as_mut().map(|fixture| {
                super::receiver_fixture::DeferredCellPrelude {
                    amount: (!world.session.no_damage())
                        .then(|| tiberium_reduction_amount(damage, affects_tiberium, warhead))
                        .flatten(),
                    deferred: &mut fixture.deferred_tiberium,
                }
            });
            #[cfg(test)]
            let prelude: &mut dyn combat_aoe::AoECellPrelude = match deferred_prelude.as_mut() {
                Some(deferred) => deferred,
                None => &mut prelude,
            };
            #[cfg(not(test))]
            let prelude: &mut dyn combat_aoe::AoECellPrelude = &mut prelude;
            let mut context = combat_aoe::AoELayerContext {
                occupancy: Some(&world.substrate.occupancy),
                terrain: world.resolved_terrain.as_mut(),
                overlay_grid: world.overlay_grid.as_mut(),
                overlay_registry,
                scenario_rng: Some(&mut world.scenario_rng),
                air_impact,
                impact_z,
            };
            let spread_cell =
                collector.prepare_cell(dx, dy, warhead, &mut context, &mut Some(prelude));
            let detached_cell = spread_cell.detached_cell(&context);
            (spread_cell, detached_cell)
        };
        // AreaDamage4896BA explicitly calls70D4A0 AFTER recursive480CB0 and
        // BEFORE4896BF captures this cell's residents. Concrete Restore4D8F80
        // runs its destination setter synchronously with all world authorities.
        if let Some((rx, ry)) = detached_cell {
            let _receipt = world.stop_all_targeting_cell(rx, ry, rules, overlay_registry, effects);
            #[cfg(any(test, feature = "test-support"))]
            collector
                .result
                .cell_target_detaches
                .extend(_receipt.cell_detaches);
        }
        collector.capture_cell(
            &spread_cell,
            &world.substrate.entities,
            &combat_aoe::AoELayerContext {
                occupancy: Some(&world.substrate.occupancy),
                terrain: world.resolved_terrain.as_mut(),
                overlay_grid: world.overlay_grid.as_mut(),
                overlay_registry,
                scenario_rng: Some(&mut world.scenario_rng),
                air_impact,
                impact_z,
            },
            include_terrain_objects.then_some(combat_aoe::TerrainCollectionView {
                store: &world.terrain_objects,
            }),
        );
    }
    collector.finish(
        &world.substrate.entities,
        &combat_aoe::AoELayerContext {
            occupancy: Some(&world.substrate.occupancy),
            terrain: world.resolved_terrain.as_mut(),
            overlay_grid: world.overlay_grid.as_mut(),
            overlay_registry,
            scenario_rng: Some(&mut world.scenario_rng),
            air_impact,
            impact_z,
        },
        warhead,
    )
}

/// `Apply_area_damage @ 0x00489280` outside a damage transaction (an
/// anim's landing at `0x00423EAB`, the Psychic Dominator at `0x0053B16B`, a
/// bomb's fuse at `0x006FA712`):
/// the area's receivers around `impact` committed in order, then area rocking
/// (`0x00489B49`) and bridge damage (`0x00489E87`). `origin` is the source object, the source
/// house and the warhead's id.
/// `affects_tiberium` gates only the per-cell Reduce_Tiberium call at
/// `0x00489648..0x00489665` (original gamemd.exe); all other area effects remain active.
pub(crate) fn apply_area_damage(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &WarheadType,
    affects_tiberium: bool,
    origin: (u64, Option<InternedId>, InternedId),
    effects: FrameEffects<'_>,
) -> AreaDamageApply {
    let mut run = ReceiverRun::default().with_effects(effects);
    let (death, pings, crit_active, area_result) = apply_area_damage_in_run(
        world,
        &mut run,
        rules,
        overlay_registry,
        impact,
        damage,
        warhead,
        affects_tiberium,
        origin,
    );
    let navigation = run.finish();
    let receipt = world.absorb_noncombat_damage_effects(
        rules,
        overlay_registry,
        death,
        pings,
        navigation,
        effects,
    );
    AreaDamageApply {
        bridge_state_changed: receipt.bridge_state_changed,
        crit_active,
        area_result,
    }
}

/// What one [`apply_area_damage`] or [`apply_area_damage_in_run`] changed
/// besides its receivers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaDamageApply {
    pub(crate) bridge_state_changed: bool,
    /// Phobos `WarheadTypeExt::Crit_Active`: a damage-animation selection that
    /// follows the area damage reads it for the `Crit.AnimList` swap.
    pub(crate) crit_active: bool,
    /// The native `AreaDamage` receipt: the water-unit gate and the Iron
    /// Curtain animation substitution read it.
    pub(crate) area_result: AreaDamageResult,
}

/// [`apply_area_damage`] inside a damage transaction (a TerrainClass death's
/// C4 at `0x0071BABF`, a dying carrier's bomb at `0x0070267F`): the receivers
/// commit in `run`, then area rocking and bridge damage. Their effects and pings go
/// back to the receiver that set the area off.
pub(crate) fn apply_area_damage_in_run(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &WarheadType,
    affects_tiberium: bool,
    origin: (u64, Option<InternedId>, InternedId),
) -> (DeathEffects, Vec<UnderAttackEvent>, bool, AreaDamageResult) {
    apply_area_damage_in_run_with_target(
        world,
        run,
        rules,
        overlay_registry,
        impact,
        damage,
        warhead,
        affects_tiberium,
        origin,
        None,
    )
}

/// [`apply_area_damage_in_run`] with Phobos `WarheadTypeExt::DamageAreaWithTarget`'s
/// `DamageAreaTarget` (`WarheadType/Body.cpp:137..146`): the target the
/// per-target pass of a warhead without `CellSpread` visits. Stock never
/// reads it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_area_damage_in_run_with_target(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &WarheadType,
    affects_tiberium: bool,
    origin: (u64, Option<InternedId>, InternedId),
    area_target: Option<u64>,
) -> (DeathEffects, Vec<UnderAttackEvent>, bool, AreaDamageResult) {
    let area = collect_area_at(
        world,
        rules,
        overlay_registry,
        impact,
        damage,
        warhead,
        affects_tiberium,
        origin,
        run.effects(),
    );
    let mut effects = DeathEffects::default();
    #[cfg(test)]
    append_fixture_tiberium(world, &mut effects.tiberium_reduction_requests);
    #[cfg(any(test, feature = "test-support"))]
    effects.wall_mutations.extend(area.aoe.wall_mutations);
    #[cfg(any(test, feature = "test-support"))]
    effects
        .cell_target_detaches
        .extend(area.aoe.cell_target_detaches);
    // Phobos `MapClass_DamageArea @ 0x489286`: a direct `DamageArea` caller's
    // per-target pass runs before the native area damage, exactly like the
    // bullet path.
    #[cfg(feature = "phobos")]
    let crit_active = super::crit::detonate(
        world,
        run,
        rules,
        overlay_registry,
        warhead,
        origin.2,
        origin.0,
        origin.1,
        impact,
        area_target,
        &area.aoe.receivers,
        &mut effects,
    );
    #[cfg(not(feature = "phobos"))]
    let _ = area_target;
    #[cfg(not(feature = "phobos"))]
    let crit_active = false;
    let (nested, mut pings, area_result) =
        commit_area_with_dispatch(world, run, &area.aoe.receivers, rules, overlay_registry);
    effects.append(nested);
    let (tail, mut tail_pings) = continue_area_damage(
        world,
        run,
        rules,
        overlay_registry,
        area.continuation,
        area_result,
    );
    effects.append(tail);
    pings.append(&mut tail_pings);
    (effects, pings, crit_active, area_result)
}

/// Original AreaDamage call arguments and the layer selected at489357..48958D,
/// retained across synchronous receiver callbacks. The source is the original
/// argument, even if a callback clears the owning Bullet's Owner pointer.
#[derive(Clone, Copy)]
struct AreaDamageContinuation<'r> {
    cell: (u16, u16),
    impact: ProjectileCoord,
    damage: i32,
    source_id: u64,
    source_house: Option<InternedId>,
    warhead: &'r WarheadType,
    warhead_ref: InternedId,
    object_list_layer: crate::sim::map::bridge_topology::ListLayer,
    routed_wall: bool,
    /// The entry gate precedes every receiver callback. A nested effect must
    /// not replace this call's arguments or its admitted/selected-layer state.
    entered: bool,
}

struct AreaAtImpact<'r> {
    continuation: AreaDamageContinuation<'r>,
    aoe: combat_aoe::AoEDamageResult,
}

/// The shared first half of Apply_area_damage489280: wall routing and
/// receiver collection, including the air query at the exact impact.
fn collect_area_at<'r>(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    impact: ProjectileCoord,
    damage: i32,
    warhead: &'r WarheadType,
    affects_tiberium: bool,
    origin: (u64, Option<InternedId>, InternedId),
    effects: FrameEffects<'_>,
) -> AreaAtImpact<'r> {
    // Observe the shared entry, including Bullet and nested receiver calls;
    // an external-only wrapper misses those paths and must not double-count.
    #[cfg(test)]
    super::receiver_fixture::observe_area_visit(
        world,
        impact,
        damage,
        warhead,
        affects_tiberium,
        origin,
    );
    let (rx, ry, sub_x, sub_y, z_leptons) = projectile_impact_cell(impact);
    let entered = damage != 0 && !world.session.no_damage();
    let routed_wall = entered && area_routes_to_wall(world, overlay_registry, (rx, ry), warhead);
    let aoe = collect_area(
        world,
        rules,
        overlay_registry,
        (rx, ry),
        damage,
        warhead,
        affects_tiberium,
        origin,
        Some(combat_aoe::AoEAirImpact {
            sub_x,
            sub_y,
            z_leptons,
        }),
        z_leptons.div_euclid(LEPTONS_PER_LEVEL as i32),
        effects,
    );
    AreaAtImpact {
        continuation: AreaDamageContinuation {
            cell: (rx, ry),
            impact,
            damage,
            source_id: origin.0,
            source_house: origin.1,
            warhead,
            warhead_ref: origin.2,
            object_list_layer: aoe.object_list_layer,
            routed_wall,
            entered,
        },
        aoe,
    }
}

pub(super) fn commit_smudges(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    requests: Vec<SmudgeSpawnRequest>,
    _deferred: &mut Vec<SmudgeSpawnRequest>,
) {
    #[cfg(test)]
    if world.receiver_fixture.is_some() {
        _deferred.extend(requests);
        return;
    }
    for request in requests {
        world.commit_smudge_request_inline(rules, overlay_registry, request);
    }
}

#[derive(Default)]
pub(crate) struct ReceiverRun<'a> {
    pub(crate) handled_deaths: Vec<u64>,
    pub(super) finalizing_terrain: BTreeSet<u64>,
    pub(crate) navigation_changed_cells: Vec<(u16, u16)>,
    /// Units the local player had selected when their receiver was last
    /// entered: `UnitClass::ReceiveDamage` reads it first
    /// (`0x00737C98..0x00737CB6`: IsSelected `+0x83` and
    /// `HouseClass::IsHumanPlayer @ 0x0050B6F0`), before the kill's Destroy
    /// callback deselects the unit, and a dying one hands it to its
    /// passengers and crewman.
    pub(crate) selected_units: Vec<u64>,
    /// Phobos `WarheadTypeExt::InApplyCrit`: the warhead types whose crit is
    /// currently applying, so a nested detonation of the same warhead cannot
    /// re-enter. Transaction-local and never saved.
    #[cfg(feature = "phobos")]
    pub(crate) crit_warheads: Vec<InternedId>,
    effects: FrameEffects<'a>,
}

impl<'a> ReceiverRun<'a> {
    pub(crate) fn with_effects(mut self, effects: FrameEffects<'a>) -> Self {
        self.effects = effects;
        self
    }

    pub(crate) const fn effects(&self) -> FrameEffects<'a> {
        self.effects
    }

    pub(crate) fn finish(self) -> Vec<(u16, u16)> {
        debug_assert!(self.finalizing_terrain.is_empty());
        self.navigation_changed_cells
    }
}

pub(crate) fn commit_area(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    receivers: &[combat_aoe::AreaDamageReceiver],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let (effects, pings, _) =
        commit_area_with_dispatch(world, run, receivers, rules, overlay_registry);
    (effects, pings)
}

/// Apply_area_damage48935C initializes its receiver flag to zero, sets it
/// after each dispatched virtual ReceiveDamage489ABC (including ReturnZero),
/// and returns its inverse at48A47E. Keep this receipt transaction-local;
/// nested areas must not overwrite their parent's dispatched status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AreaDamageResult {
    ReceiverDispatched = 0,
    #[default]
    NoReceiver = 1,
    IronCurtain = 2,
}

pub(crate) fn commit_area_with_dispatch(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    receivers: &[combat_aoe::AreaDamageReceiver],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>, AreaDamageResult) {
    let current_tick = receiver_tick(world);

    let isolation_armed =
        area_near_center_ic_isolation_armed(receivers, &world.substrate.entities, current_tick);
    let mut effects = DeathEffects::default();
    let mut under_attack_events = Vec::new();
    let mut dispatched = false;

    for receiver in receivers {
        match *receiver {
            combat_aoe::AreaDamageReceiver::Entity(event) => {
                if !area_record_dispatches(world, rules, &event) {
                    continue;
                }
                if !area_isolation_dispatches(world, &event, isolation_armed, current_tick) {
                    continue;
                }
                dispatched = true;
                let (nested, mut pings) = commit_entities(
                    world,
                    run,
                    std::slice::from_ref(&event),
                    Some(false),
                    rules,
                    overlay_registry,
                );
                effects.append(nested);
                under_attack_events.append(&mut pings);
            }
            combat_aoe::AreaDamageReceiver::Terrain(event) => {
                if isolation_armed && event.near_center_ic_isolation_eligible {
                    continue;
                }
                if !world
                    .terrain_objects
                    .objects()
                    .get(&event.stable_id)
                    .is_some_and(|terrain| terrain.is_live() && terrain.health() > 0)
                {
                    continue;
                }
                dispatched = true;
                let (nested, mut pings) =
                    commit_terrain(world, run, event, false, rules, overlay_registry);
                effects.append(nested);
                under_attack_events.append(&mut pings);
            }
        }
    }

    let result = if isolation_armed {
        AreaDamageResult::IronCurtain
    } else if dispatched {
        AreaDamageResult::ReceiverDispatched
    } else {
        AreaDamageResult::NoReceiver
    };
    (effects, under_attack_events, result)
}

fn area_isolation_dispatches(
    world: &Simulation,
    event: &EntityDamageEvent,
    isolation_armed: bool,
    current_tick: u64,
) -> bool {
    !isolation_armed
        || !event.near_center_ic_isolation_eligible
        || world
            .substrate
            .entities
            .get(event.target_id)
            // 0x00489A4F: vt+0x160 IsIronCurtained (0x0041BF40).
            .is_some_and(|target| {
                crate::sim::superweapon::invulnerability::is_invulnerable(
                    target.invulnerability.as_ref(),
                    current_tick as u32,
                )
            })
}

/// `Apply_area_damage @ 0x00489280`'s per-record dispatch gates
/// (`0x004899F1..0x00489A95`), read live when the record's turn comes: an
/// earlier record's receiver (a death weapon, an Ivan bomb, a C4 cascade) can
/// kill, remove or limbo a later record's object before it is reached. The
/// object must be alive (`+0x90`), not an `InvisibleInGame=` building
/// (BuildingType `+0x1701`, `0x00489A1B`), have Health above zero (`+0x6C`,
/// `0x00489A79`), be marked on the map (`+0x74`, `0x00489A80`) and be out of
/// limbo (`+0x81`, `0x00489A87`). The distance bound (`0x00489A91`) and the
/// airborne-aircraft halving (`0x00489A59..0x00489A77`) are fixed at
/// collection; the near-centre Iron Curtain filter is `area_isolation_dispatches`.
fn area_record_dispatches(world: &Simulation, rules: &RuleSet, event: &EntityDamageEvent) -> bool {
    world
        .substrate
        .entities
        .get(event.target_id)
        .is_some_and(|target| {
            target.lifecycle.object_alive()
                && !(target.category() == EntityCategory::Structure
                    && world
                        .interner
                        .object_type(target.type_ref(), rules)
                        .is_some_and(|object| object.invisible_in_game))
                && target.health.current() > 0
                && target.lifecycle.cell_marked()
                && !target.lifecycle.in_limbo()
        })
}

/// Complete one TerrainClass receiver, including its nested C4 and removal.
/// Direct bridge calls use ignore_defenses=true; ordinary area calls retain
/// their damage kernel and perform the area-wide IC gate before entering here.
pub(crate) fn commit_terrain(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    event: TerrainDamageEvent,
    ignore_defenses: bool,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let mut effects = DeathEffects::default();
    let mut under_attack_events = Vec::new();
    let Some(warhead) = rules.warhead(world.interner.resolve(event.warhead_ref)) else {
        return (effects, under_attack_events);
    };
    let mut packet = event.damage;
    let receive = world.terrain_objects.receive_damage(
        &mut run.finalizing_terrain,
        event.stable_id,
        (event.rx, event.ry),
        &mut packet,
        event.distance_leptons,
        warhead,
        rules,
        &world.interner,
        world.session.no_damage(),
        ignore_defenses,
    );
    if let Some(writeback) = event.damage_writeback {
        writeback.write(world, rules, packet);
    }
    let TerrainAreaReceiveResult::Lethal(lethal) = receive else {
        return (effects, under_attack_events);
    };

    // ObjectReceiveDamage5F57AF calls DetachAll before returning lethal4
    // to Terrain71B920. Its observers run before Terrain's nested C4 and
    // the three later DetachAll/UnInit/Limbo announcements.
    let uninit_context =
        crate::sim::world::UninitContext::new(rules, overlay_registry).with_effects(run.effects());
    world.notify_pointer_expired(lethal.stable_id, uninit_context);

    if lethal.spawns_tiberium
        && let Some(c4_warhead) = rules.warhead(&rules.bridge_warheads.c4_name)
    {
        let c4_id = world.interner.intern(&c4_warhead.id);
        // Terrain71BABF passes its retained Object Location, not a fresh
        // sample of the ground/deck after nested callbacks.
        let impact = world.terrain_objects.objects()[&lethal.stable_id].world_coord();
        // TerrainClass::Take_Damage71BA2E..71BA85 selects with the raw
        // cell LandType and damage100, then constructs the ordinary combat
        // Anim (2600h/-15) BEFORE its nested C4. Original gamemd.exe
        // instructions; use the shared selector and constructor so Start,
        // sound, RNG, persistence and deferred cleanup keep their owners.
        let land = detonation_anim::land_at(world, impact);
        if let Some(effect) =
            detonation_anim::effect(world, rules, c4_warhead, 100, land, impact, impact, false)
        {
            crate::sim::world::damage_consequences::admit_explosion_effect(world, rules, effect);
        }
        let (nested, mut pings, _crit_active, _area_result) = apply_area_damage_in_run(
            world,
            run,
            rules,
            overlay_registry,
            impact,
            100,
            c4_warhead,
            true,
            (RAD_NO_ATTACKER, None, c4_id),
        );
        effects.append(nested);
        under_attack_events.append(&mut pings);
        // The following CellChainReaction request71BAD5 calls489270,
        // whose complete active-YR body is RET; there is no queued effect.
    }

    world.terrain_lethal_uninit(lethal, &mut run.finalizing_terrain, uninit_context);
    (effects, under_attack_events)
}

pub(crate) fn commit_entities(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    damage_events: &[EntityDamageEvent],
    near_center_ic_isolation_override: Option<bool>,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let sound_enabled = sound_enabled(world);

    let current_tick = receiver_tick(world);
    let scenario_no_damage = world.session.no_damage();

    let mut death = DeathEffects::default();
    let mut under_attack_events = Vec::new();
    // Apply_area_damage finishes collecting its fixed target/distance records
    // before dispatch. A qualifying near-center Iron Curtain therefore
    // isolates the entire eligible transaction, including records collected
    // before the arming record. The per-record check below remains live so an
    // earlier receiver or nested death effect can change later protection.
    let near_center_ic_isolation = near_center_ic_isolation_override.unwrap_or_else(|| {
        near_center_ic_isolation_armed(damage_events, &world.substrate.entities, current_tick)
    });

    for event in damage_events {
        if !area_isolation_dispatches(world, event, near_center_ic_isolation, current_tick) {
            continue;
        }
        #[cfg(feature = "debug-tools")]
        let Some(debug_event) = world.debug_damage_event(*event) else {
            continue;
        };
        #[cfg(feature = "debug-tools")]
        let event = &debug_event;
        let target_id = event.target_id;
        let attacker_id = event.attacker_id;
        run.selected_units.retain(|&id| id != target_id);
        if world
            .substrate
            .entities
            .get(target_id)
            .is_some_and(|target| {
                target.category() == EntityCategory::Unit
                    && target.selection.is_selected()
                    && world.session.current_house() == Some(target.owner())
            })
        {
            run.selected_units.push(target_id);
        }
        match apply_building_receive_prelude(
            event,
            &world.substrate.entities,
            rules,
            &world.interner,
            &mut world.houses,
            current_tick,
        ) {
            BuildingReceivePrelude::ReturnZero => continue,
            BuildingReceivePrelude::Respond => {
                if let Some(victim_owner) = world
                    .substrate
                    .entities
                    .get(event.target_id)
                    .map(|victim| victim.owner())
                {
                    let attacker_house_index = world
                        .substrate
                        .entities
                        .get(event.attacker_id)
                        .and_then(|attacker| {
                            world
                                .houses
                                .order()
                                .iter()
                                .position(|owner| *owner == attacker.owner())
                        })
                        .map_or(-1, |index| index as i32);
                    if let Some(owner) = world.houses.get_mut(&victim_owner) {
                        owner
                            .strategy_emergency_mut()
                            .note_building_attacker(attacker_house_index);
                    }
                }
                {
                    respond_to_base_attack(
                        world,
                        rules,
                        BaseDefenseResponseCallSite::BuildingPrelude,
                        event.target_id,
                        event.attacker_id,
                        run.effects(),
                    );
                }
            }
            BuildingReceivePrelude::Continue => {}
        }
        // `UnitClass::ReceiveDamage 0x00737CB6..0x00737D2E`: a unit still
        // inside its war factory returns 0 before FootClass::ReceiveDamage.
        if !event.receiver_flags.ignore_defenses
            && world.unit_in_contact_war_factory(target_id, rules)
        {
            continue;
        }
        // Building4422F9..44234A copies its sparse contacts before calling
        // Techno442425. Destroy/Stun can remove the live links before the
        // concrete NowDead loop442511 resumes, so this per-receiver snapshot
        // must survive the synchronous common death effects.
        let building_contacts = if callbacks_enabled(world) {
            world
                .substrate
                .entities
                .get(target_id)
                .filter(|target| target.category() == EntityCategory::Structure)
                .map(|target| target.radio_contacts.iter_live().collect::<Vec<_>>())
        } else {
            None
        };
        let prepared_event = super::receiver_call::prepare_infantry_receiver(
            event,
            &world.substrate.entities,
            rules,
            &world.interner,
            world.resolved_terrain.as_ref(),
        );
        let event = &prepared_event;
        if let Some(writeback) = event.damage_writeback {
            writeback.write(world, rules, event.damage);
        }
        // FootClass::ReceiveDamage 0x004D7330..0x004D7413 runs its parasite
        // prefix on the raw damage before TechnoClass::ReceiveDamage.
        world.foot_receive_damage_parasite_prefix(
            target_id,
            (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
            event.damage,
            event.warhead_ref,
            rules,
            run.effects(),
        );
        // Foot's synchronous parasite exit can re-enter damage and mutate the
        // same caller-owned field. Techno reads its current pointed value.
        let mut receiver_event = *event;
        if let Some(writeback) = event.damage_writeback {
            receiver_event.damage = writeback.read(world, rules);
        }
        let event = &receiver_event;
        // Phobos `TechnoClass_ReceiveDamage @ 0x701900+0x6` runs the shield
        // before gamemd's receiver body and replaces `args->Damage` with the
        // surviving damage. The stage owns the mutation here, before the
        // otherwise-immutable resolver is entered.
        #[cfg(feature = "phobos")]
        let shield_receipt = {
            let warhead = rules.warhead(world.interner.resolve(event.warhead_ref));
            let target = world.substrate.entities.get(target_id);
            match (target, warhead) {
                (Some(target), Some(_)) => {
                    let target_type = world.interner.object_type(target.type_ref(), rules);
                    let divisors = super::receiver_divisors(
                        target,
                        target_type,
                        rules,
                        &world.interner,
                        &world.houses,
                        target.phobos_armor_multiplier(),
                    );
                    let input = event.damage;
                    crate::sim::shield::pre_receive_damage(world, rules, event, input, &divisors)
                }
                _ => None,
            }
        };
        #[cfg(feature = "phobos")]
        if shield_receipt
            .as_ref()
            .is_some_and(|receipt| receipt.shield_damage_positive)
            && rules
                .warhead(world.interner.resolve(event.warhead_ref))
                .is_some_and(|warhead| rules.decloak_damaged_targets_for(warhead))
        {
            // Phobos `ShieldClass::ReceiveDamage` (`ShieldClass.cpp:249..255`)
            // uncloaks the target before the native receiver body runs, so a
            // shield absorbed hit still surfaces it. The later native-slot
            // call rejects the already-uncloaking state.
            start_uncloaking_for_damage(world, target_id, rules, current_tick, sound_enabled);
        }

        #[cfg(feature = "phobos")]
        #[cfg(feature = "ares")]
        let ares_initially_positive = shield_receipt
            .as_ref()
            .map_or(event.damage > 0, |receipt| receipt.remaining > 0);
        #[cfg(all(feature = "ares", not(feature = "phobos")))]
        let ares_initially_positive = event.damage > 0;
        #[cfg(feature = "phobos")]
        let post_shield_input = shield_receipt
            .as_ref()
            .map_or(event.damage, |receipt| receipt.remaining);
        // The Phobos wrapper's Unkillable arm runs after the shield and
        // before the original ReceiveDamage body.
        #[cfg(feature = "phobos")]
        let unkillable_clamp =
            world.apply_unkillable_clamp(target_id, event, post_shield_input, rules);
        // Phobos TechnoClass_ReceiveDamage_ArmorMultiplier (0x00701966): the
        // effects' armor chances belong to the positive, defended divide on
        // the post-shield damage. `None` keeps the cached unrestricted product.
        #[cfg(feature = "phobos")]
        let receiver_ae_armor = {
            if !event.receiver_flags.ignore_defenses
                && post_shield_input >= 0
                && rules
                    .warhead(world.interner.resolve(event.warhead_ref))
                    .is_some()
            {
                let source_house = event.source_house.or_else(|| {
                    world
                        .substrate
                        .entities
                        .get(event.attacker_id)
                        .map(|attacker| attacker.owner())
                });
                Some(world.attach_effect_armor_multiplier(
                    target_id,
                    Some(event.warhead_ref),
                    source_house,
                    rules,
                ))
            } else {
                None
            }
        };
        #[cfg(feature = "phobos")]
        let (receiver_input, skip_low_damage_check) = if unkillable_clamp {
            (0, true)
        } else {
            (
                post_shield_input,
                shield_receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.remaining == 0),
            )
        };
        #[cfg(feature = "phobos")]
        let receiver_outcome = resolve_receive_damage_with_input(
            event,
            &world.substrate.entities,
            rules,
            &world.interner,
            &world.houses,
            world.house_alliances.as_map(),
            scenario_no_damage,
            current_tick,
            world.resolved_terrain.as_ref(),
            receiver_input,
            skip_low_damage_check,
            receiver_ae_armor,
        );
        #[cfg(not(feature = "phobos"))]
        let receiver_outcome = resolve_receive_damage(
            event,
            &world.substrate.entities,
            rules,
            &world.interner,
            &world.houses,
            world.house_alliances.as_map(),
            scenario_no_damage,
            current_tick,
            world.resolved_terrain.as_ref(),
            None,
        );
        // Techno701ADB..701B62 precedes bunker/warhead/Object gates, even
        // when those gates suppress the HP write. It also runs for healing
        // and IgnoreDefenses; only the earlier receiver exits skip it.
        if let Some(damage) =
            receiver_outcome.and_then(|resolved| resolved.outcome.readiness_damage)
            && let Some(target) = world.substrate.entities.get_mut(target_id)
            && let Some(object) = world.interner.object_type(target.type_ref(), rules)
        {
            target.reduce_ammo_for_damage(object, damage, world.session.binary_frame() as i32);
        }
        // Techno70196C..701D69 writes its defended/gated/kernel value before
        // team removal and Object's synchronous Tag/Kill/Destroy callbacks.
        if let Some(writeback) = event.damage_writeback
            && let Some(value) =
                receiver_outcome.and_then(|resolved| resolved.outcome.post_object_damage)
        {
            writeback.write(world, rules, value);
        }
        if let Some(effect) = receiver_outcome.and_then(|resolved| resolved.invulnerability_impact)
        {
            death.combat_light_requests.push(effect);
        }
        // `0x00701D71..0x00701DB4`: a Psychedelic hit that turns its target
        // berserk takes it out of its team, with its idle order
        // (`0x006EA870`), clears its target (`vt+0x3C8(0)`) and queues Hunt
        // (`vt+0x1E8(0xF, 0)`). The removal runs here, before the receiver's
        // timer and flag writes, which it does not read; the other two follow
        // them, as natively.
        let starts_berserk = receiver_outcome
            .and_then(|resolved| resolved.outcome.psychedelic_value)
            .is_some()
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| !target.berserk.is_active());
        if starts_berserk {
            world.leave_team(target_id, false, rules, run.effects());
        }
        let Some(receiver_health::ReceiverHealthCommit {
            building_entry_frame,
            state: receive_state,
            entered_techno_death,
            #[cfg(any(test, feature = "test-support"))]
                reached_exact_zero: _,
            postmortem_candidate,
            fatal_category,
            positive_postlude,
            synchronous_retaliation,
            smoke_maintenance,
            healing_only,
            latch_hostile_hit,
            uncloak_after_damage,
            house_threat_delta,
            psychedelic,
            #[cfg(feature = "phobos")]
            object_packet,
        }) = receiver_health::commit_receiver_health(
            event,
            world,
            rules,
            overlay_registry,
            receiver_outcome,
            run.effects(),
            true,
        )
        else {
            continue;
        };
        // Object Tags, Kill and Destroy have already returned, followed by
        // the immediate live-owner House anger update at Techno701E68.
        let _ = house_threat_delta;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(delta) = house_threat_delta {
            death
                .receiver_stage_trace
                .push(ReceiverStageTrace::HouseThreat { target_id, delta });
        }
        // Phobos `TechnoClass_ReceiveDamage_ReflectDamage` (`0x701E18`): after
        // Object's body (`0x00701DF8`), only with an attacker
        // (`0x00701DFF..0x00701E0C`) and a positive `*pDamage`, the hit is
        // reflected to its source and counted for `DiscardOn=ReceivedDamage`
        // against the call's source house, IgnoreDefenses included. It runs
        // here after the additive anger update at `0x00701E68`. A null source
        // house would fault in the count's `IsNeutral`; VERA reads the
        // attacker's owner instead.
        #[cfg(feature = "phobos")]
        if attacker_id != RAD_NO_ATTACKER
            && let Some(damage) = object_packet
            && damage > 0
        {
            world.apply_attach_effect_reflect_damage(
                target_id,
                attacker_id,
                event.source_house,
                event.warhead_ref,
                damage,
                rules,
                overlay_registry,
                run.effects(),
            );
            let source_house = event.source_house.or_else(|| {
                world
                    .substrate
                    .entities
                    .get(attacker_id)
                    .map(|attacker| attacker.owner())
            });
            world.record_phobos_attach_damage(target_id, source_house, rules);
        }
        if let Some(duration_frames) = postmortem_candidate {
            let current_frame = current_tick as u32 as i32;
            let target = world
                .substrate
                .entities
                .get_mut(target_id)
                .expect("PostMortem exact-zero callbacks retain the represented target");
            // RecordKill already consumed this exact-zero attribution in the
            // shared RecordTheKill receiver before the PostMortem Destroy hook. Native retains no killer on the
            // restored object: a fresh null-source timer expiry must stay
            // uncredited, while a later sourced lethal hit captures anew.
            target.kill_record.clear();
            target
                .pending_c4_detonation
                .arm_post_mortem(current_frame, duration_frames);
            // PostMortem's restore (`0x00701F60`, `0x00701F67`).
            target.lifecycle.restore_alive();
            target.health.set(1);
            target.death.restore();
            #[cfg(any(test, feature = "test-support"))]
            death
                .receiver_stage_trace
                .push(ReceiverStageTrace::PostMortem { target_id });
            continue;
        }
        if starts_berserk {
            // The class setter: Infantry's (`0x0051B1F0`) also resets its
            // action and path head.
            let _ = world.assign_target_represented(
                target_id,
                None,
                rules,
                overlay_registry,
                run.effects(),
            );
            let _ = world.mission_queue_exact(
                target_id,
                MissionId::from_known(MissionType::Hunt),
                0,
                world.session.binary_frame(),
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
        }

        // Techno701900 result1 (switch table702D24 entry 1) selects
        // 702713..7027A9: two distinct Voc750E20 retained-coordinate requests
        // at direct Location+9C. The arm's own tail7027AE (only reached from
        // 702775) holds the protected-Techno call7027E9 before common7027F7.
        // Each request reaches the existing synchronous sound boundary before
        // later receiver consequences can spend Main.
        // The exact-zero override70202E takes the death arm instead, and the
        // Psychedelic arm returns its result 1 before this tail.
        let result_one_arm =
            receive_state == damage::DamageState::Damaged && !entered_techno_death && !psychedelic;
        if result_one_arm && sound_enabled {
            for _ in 0..2 {
                if let Some(target) = world.substrate.entities.get(target_id)
                    && let Some(sound_id) = world
                        .interner
                        .object_type(target.type_ref(), rules)
                        .and_then(|object| object.damage_sound())
                {
                    let coordinate =
                        crate::sim::movement::ground_pose::position_world_coord(&target.position);
                    world.sound_events.emit(SimSoundEvent::VocAtCoord {
                        sound_id: sound_id.to_owned(),
                        coordinate,
                        //702734/70277D PUSH0 before each750E20;
                        //executed coordinate_attempts retain args[0].
                        flags: 0,
                    });
                    run.effects().flush_sound(world, rules);
                }
            }
        }

        // Techno ReceiveDamage702049 selects result 2's arm702695. The
        // percent draw precedes House50B6F0; accepted lists spend one raw
        // Main draw even for a singleton. This belongs before common7027F7,
        // Uncloak70281D, smoke and retaliation, not at app sound publication.
        // Original executed controls: input_oracle/unit_voice_playback.
        if receive_state == damage::DamageState::Yellow
            && let Some(target) = world.substrate.entities.get(target_id)
            && let Some(object) = world.interner.object_type(target.type_ref(), rules)
            && !object.voice_feedback.is_empty()
            && world.main_rng.next_range_i32_inclusive(0, 99) < 0x1E
            && world.house_is_human_player(target.owner())
        {
            let index = (world.main_rng.next_u32() % object.voice_feedback.len() as u32) as usize;
            // Virtual+48 is Object/Building GetCoords, not a cell centre or
            // the separate navigation+4C getter. Reuse the coordinate owner;
            // pack only this presentation receipt, never move the actor.
            let coord = crate::sim::movement::ground_pose::object_get_coords(
                target,
                world.resolved_terrain.as_ref(),
            );
            let mut position = target.position;
            position.put_location(coord);
            if sound_enabled {
                world.sound_events.emit(SimSoundEvent::voc_at(
                    object.voice_feedback[index].clone(),
                    &position,
                ));
            }
        }

        // gamemd-derived: `TechnoClass__ReceiveDamage @
        // 0x007027AE..0x007027EE`, the tail of result 1's own arm, invokes the
        // protected-Techno response after ObjectClass has committed
        // health/visual state. Unaffected hits, threshold crossings and kills
        // never reach it. `ShouldProtect+0x3CF` has no active YR writer;
        // `ToProtect=` is the exact live gate retained here. An owner a human
        // controls (`0x0050B730` at `0x007027D2`) refuses.
        let protected_techno_response = result_one_arm
            && attacker_id != RAD_NO_ATTACKER
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| {
                    world
                        .interner
                        .object_type(target.type_ref(), rules)
                        .is_some_and(|object| object.to_protect)
                        && world.houses.contains_key(&target.owner())
                        && !world.owner_is_human(target.owner())
                });
        if protected_techno_response {
            respond_to_base_attack(
                world,
                rules,
                BaseDefenseResponseCallSite::ProtectedTechno,
                target_id,
                attacker_id,
                run.effects(),
            );
        }

        // Installed Ares702819 ->1004AB50 is a receiver aftermath, after
        // protected responses and before Uncloak. The early701914 hook
        // records the sign before defenses; callbacks may alter a pointed
        // damage field before the aftermath reads it again.
        #[cfg(feature = "ares")]
        {
            let current_damage = event
                .damage_writeback
                .map(|writeback| writeback.read(world, rules))
                .or_else(|| {
                    receiver_outcome.and_then(|resolved| resolved.outcome.post_object_damage)
                })
                .unwrap_or(event.damage);
            world.ares_driver_kill_aftermath(
                event,
                receive_state,
                ares_initially_positive,
                current_damage,
                rules,
                overlay_registry,
                run.effects(),
            );
        }

        //1004AB68..1004AB88 returns702823 for result0, skipping Uncloak
        //even when no extension effect is configured or admitted.
        #[cfg(feature = "ares")]
        let uncloak_after_damage =
            uncloak_after_damage && receive_state != damage::DamageState::Unaffected;

        // Phobos `TechnoClass_ReceiveDamage_Decloak @ 0x00702819` replaces the
        // retail call with a per-warhead switch: only an admitting warhead
        // reaches `Uncloak(false)`, and a missing extension skips it.
        #[cfg(feature = "phobos")]
        let uncloak_after_damage = uncloak_after_damage
            && rules
                .warhead(world.interner.resolve(event.warhead_ref))
                .is_some_and(|warhead| rules.decloak_damaged_targets_for(warhead));

        // `0x0070281D`, in its native slot: after the `ToProtect` response
        // (`0x007027E9`); Object's Kill/Destroy have already returned. The
        // `StartUncloaking(0)` argument is zero, so it owns one positional
        // `[AudioVisual] CloakSound`.
        if uncloak_after_damage {
            start_uncloaking_for_damage(world, target_id, rules, current_tick, sound_enabled);
        }

        if latch_hostile_hit && let Some(target) = world.substrate.entities.get_mut(target_id) {
            target.ai_sell_repair.hit_by_enemy();
        }

        if let Some((category, state)) = smoke_maintenance
            && callbacks_enabled(world)
        {
            world.apply_fatal_lifecycle_stage(
                rules,
                FatalLifecycleStage::MaintainDamageSmoke { state },
                target_id,
                category,
                crate::sim::world::UninitContext::with_rules(rules)
                    .with_registry(overlay_registry)
                    .with_effects(run.effects()),
            );
        };
        if healing_only {
            finish_building_art_receiver(
                world,
                target_id,
                receive_state,
                building_entry_frame,
                rules,
            );
            continue;
        }

        // TechnoClass::ReceiveDamage's retaliation tail. `ShouldRetaliate`
        // (`0x007087C0`, called at `0x00702A43`) answers 0 for a null source.
        // An accepted source within reach takes `Override_Mission`
        // (`vt+0x1F4`, `0x00702B41`); either answer then reaches the scatter
        // tail (`0x00702B47..0x00702D0B`), whose Scatter can run its locomotor
        // Process at once.
        if synchronous_retaliation {
            let retaliates = attacker_id != RAD_NO_ATTACKER && {
                #[cfg(any(test, feature = "test-support"))]
                death
                    .receiver_stage_trace
                    .push(ReceiverStageTrace::ShouldRetaliate { target_id });
                combat_targeting::should_retaliate(world, rules, target_id, attacker_id)
            };
            if retaliates
                && retaliation_reaches(world, rules, target_id, attacker_id, overlay_registry)
            {
                world.override_mission_on_damage_response(
                    target_id,
                    attacker_id,
                    rules,
                    overlay_registry,
                    run.effects(),
                );
            }
            let retaliation = if retaliates {
                DamageRetaliation::Accepted
            } else {
                DamageRetaliation::Refused
            };
            match world.receive_damage_scatter(
                target_id,
                retaliation,
                rules,
                overlay_registry,
                run.effects(),
            ) {
                Ok(changed) => death.bridge_state_changed |= changed,
                Err(cause) => log::debug!("damaged {target_id} did not scatter: {cause}"),
            }
        }

        if entered_techno_death {
            // 0x00737D97: the dying unit's own bunker link (`+0x2E4`) names a
            // building — `BuildingClass::ClearBunker @ 0x00459470` at the
            // kill instant, ahead of the death-explosion gates
            // (`0x00737DA7`) and the kill-site Death_Announcement calls. The
            // despawn safety net clears the links later but leaves the walls
            // up.
            if fatal_category == EntityCategory::Unit
                && let Some(bunker) = world
                    .substrate
                    .entities
                    .get(target_id)
                    .and_then(|entity| entity.bunker.link().installed_in())
                && world
                    .substrate
                    .entities
                    .get(bunker)
                    .is_some_and(|building| building.category() == EntityCategory::Structure)
            {
                crate::sim::docking::bunker_link::clear_bunker(world, bunker, rules, run.effects());
            }
            {
                if callbacks_enabled(world) {
                    let attacker = killing_attacker(world, std::slice::from_ref(event), target_id);
                    world.apply_fatal_lifecycle_stage(
                        rules,
                        FatalLifecycleStage::BeforeDeathEffects { attacker },
                        target_id,
                        fatal_category,
                        crate::sim::world::UninitContext::with_rules(rules)
                            .with_registry(overlay_registry)
                            .with_effects(run.effects()),
                    );
                };
            }
            let mut nested = handle_death(
                world,
                run,
                &[target_id],
                std::slice::from_ref(event),
                rules,
                overlay_registry,
                building_contacts.as_deref(),
            );
            under_attack_events.append(&mut nested.under_attack_events);
            if matches!(
                fatal_category,
                EntityCategory::Unit | EntityCategory::Structure
            ) {
                // Case 4 UnInits a building, and runs AfterDestruction
                // (`0x0044269A`, `0x004426A2`), only while its death fuse has
                // time left (`0x0044266B..0x00442690`). One that outlives the
                // blow stays alive at Health 0 until its Update retires it
                // (`0x00440072..0x004400F1`); its bunker is still released.
                let frame = world.session.binary_frame() as i32;
                let outlives = fatal_category == EntityCategory::Structure
                    && world
                        .substrate
                        .entities
                        .get(target_id)
                        .is_some_and(|building| {
                            !building.pending_c4_detonation.has_time_left(frame)
                        });
                if callbacks_enabled(world) {
                    if outlives {
                        crate::sim::docking::bunker_link::release_sell_destroy(
                            world,
                            target_id,
                            rules,
                            run.effects(),
                        );
                    } else {
                        world.apply_fatal_lifecycle_stage(
                            rules,
                            FatalLifecycleStage::AfterDeathEffects,
                            target_id,
                            fatal_category,
                            crate::sim::world::UninitContext::with_rules(rules)
                                .with_registry(overlay_registry)
                                .with_effects(run.effects()),
                        );
                    }
                }
                if callbacks_enabled(world) {
                    nested
                        .immediate_uninit_ids
                        .retain(|&dead_id| dead_id != target_id);
                }
            }
            death.append(nested);
        }

        // Building442425 has returned from the complete Techno receiver,
        // including its retaliation tail, before this live dispatch:
        // alive44242C, results2/3 at442476 and resolved type+538==-1 at4426D2.
        // The global Rules+714 request442706 uses direct Location+9C, not
        // virtual+48/GetCoords. Read here instead of latching admission and
        // coordinates before callbacks. Flush before Building's own response.
        // Residual outside this no-emitter route: result2's non-null gap
        // generator +30C/+E8 multiplication at4426AC precedes this gate.
        if matches!(
            receive_state,
            damage::DamageState::Yellow | damage::DamageState::Red
        ) && sound_enabled
            && let Some(target) = world.substrate.entities.get(target_id)
            && target.category() == EntityCategory::Structure
            && target.lifecycle.object_alive()
            && world
                .interner
                .object_type(target.type_ref(), rules)
                .is_some_and(|object| object.damage_sound().is_none())
            && let Some(sound_id) = rules.general.building_damage_sound()
        {
            let coordinate =
                crate::sim::movement::ground_pose::position_world_coord(&target.position);
            world.sound_events.emit(SimSoundEvent::VocAtCoord {
                sound_id: sound_id.to_owned(),
                coordinate,
                //4426E1 PUSH0 before750E20 at442706 (same executed owner).
                flags: 0,
            });
            run.effects().flush_sound(world, rules);
        }

        // `BuildingClass::ReceiveDamage`'s retaliation block
        // (`0x00442942..0x00442A90`, [`Simulation::building_hit_response`])
        // runs after the IsAlive re-test (`0x00442905`; `+0x90` is
        // `object_alive`); result 5 returned at `0x0044247D`. Case 4 calls
        // `DestructionEffects` (slot `+0x4EC` = `0x004415F0`, which never
        // writes `+0x90`) and then, for a fuse with time left (`+0x530` = 8),
        // `ObjectClass::UnInit` (`0x005F65F0`, clears `+0x90` at `0x005F6625`),
        // so a killing blow skips the block. The zero fuse of an `Explodes=`
        // type (TechnoType `+0xD15`: stock GAYARD, NAYARD, YAYARD, NANRCT,
        // AMMOCRAT, CAOILD, CAMISC01/02, YAPPPT) or a building dying while
        // Selling leaves `+0x90` set and the block live on its killing blow.
        // `FootClass::ReceiveDamage @ 0x004D7442..0x004D7453`: a team member
        // whose hit returned other than none (0) or gone (5) reports it to
        // its team; a killed member has already left it.
        if !matches!(
            receive_state,
            damage::DamageState::Unaffected | damage::DamageState::AlreadyDead
        ) && world
            .substrate
            .entities
            .get(target_id)
            .is_some_and(|target| target.category() != EntityCategory::Structure)
        {
            Simulation::team_took_damage(
                &world.interner,
                &world.substrate,
                &mut world.team_script_vm,
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                rules,
            );
        }
        // FootClass::ReceiveDamage's own tail, then the concrete Foot
        // receivers once it returns (`0x00518042`, `0x00737D52`), all after
        // the retaliation and scatter tail and the team report above. Both
        // concrete receivers return on 5 and take 4 to their death arms;
        // Infantry returns on 0 (`0x0051805B`) and Unit skips its ping on 0
        // (`0x007384BB`), so what follows runs for 1..=3.
        if let Some((damage, survivor_tail, _hostile_source)) = positive_postlude
            && survivor_tail
            && matches!(
                receive_state,
                damage::DamageState::Damaged
                    | damage::DamageState::Yellow
                    | damage::DamageState::Red
            )
        {
            // The source's Location, read after the Techno tail; a source no
            // longer represented is a null source.
            let attacker_coord = (attacker_id != RAD_NO_ATTACKER)
                .then(|| world.substrate.entities.get(attacker_id))
                .flatten()
                .map(|attacker| {
                    let [x, y] =
                        crate::sim::movement::ground_pose::position_world_xy(&attacker.position);
                    (x, y)
                });
            // FootClass (`0x004D746D..0x004D74C7`): a teamless Foot hit by a
            // source, whose current mission's MissionControl sets `NoThreat=`
            // and clears `Zombie=` (`0x005B3A00` +4, +5), takes
            // `Enter_Idle_Mode(0, 1)` (vt+0x484). Retail `[Harmless]` is such
            // a mission.
            if attacker_coord.is_some()
                && world
                    .substrate
                    .entities
                    .get(target_id)
                    .is_some_and(|target| {
                        matches!(
                            target.category(),
                            EntityCategory::Infantry
                                | EntityCategory::Unit
                                | EntityCategory::Aircraft
                        ) && crate::sim::mission::control::current_control(rules, &target.mission)
                            .is_some_and(|entry| entry.no_threat && !entry.zombie)
                    })
                && world.team_script_vm.team_for_member(target_id).is_none()
            {
                crate::sim::world::enter_idle_mode(
                    world,
                    target_id,
                    rules,
                    overlay_registry,
                    run.effects(),
                );
            }
            let (is_infantry, has_locomotor) =
                world
                    .substrate
                    .entities
                    .get(target_id)
                    .map_or((false, false), |target| {
                        (
                            target.category() == EntityCategory::Infantry,
                            target.locomotor.is_some(),
                        )
                    });
            // InfantryClass (`0x00518BF2..0x00518C3F`): a computer's
            // `Engineer=` man whose Get_Mission (vt+0x184) is Guard or
            // Area_Guard queues Hunt, with or without a source.
            let engineer_hunts = is_infantry
                && world
                    .substrate
                    .entities
                    .get(target_id)
                    .is_some_and(|target| {
                        !world.owner_is_human(target.owner())
                            && world
                                .interner
                                .object_type(target.type_ref(), rules)
                                .is_some_and(|obj| obj.engineer)
                            && matches!(
                                target.mission.effective().known(),
                                Some(MissionType::Guard | MissionType::AreaGuard)
                            )
                    });
            if engineer_hunts && let Some(target) = world.substrate.entities.get_mut(target_id) {
                queue_entity_mission_deferred(target, MissionId::from_known(MissionType::Hunt));
            }
            // Then, only with a source, `Scatter(source Location, 0, 0)`
            // (`0x00518C56`) before the fear latch; a null source jumps to the
            // fear ladder (`0x00518C47`). A locomotor-less man is refused, as
            // in `Simulation::receive_damage_scatter`.
            if is_infantry
                && has_locomotor
                && let Some(source) = attacker_coord
                && let Err(cause) = world.infantry_scatter_from(
                    target_id,
                    source,
                    ScatterFlags::new(false, false),
                    rules,
                    overlay_registry,
                    run.effects(),
                )
            {
                log::debug!(
                    "damaged infantry {target_id} did not scatter from its source: {cause}"
                );
            }
            if let Some(target) = world.substrate.entities.get_mut(target_id)
                && let Some(obj) = world.interner.object_type(target.type_ref(), rules)
            {
                infantry::apply_fear_from_damage(
                    obj,
                    target,
                    damage,
                    attacker_coord.is_some(),
                    rules.general.condition_red,
                    rules.general.condition_yellow,
                );
            }
            // UnitClass7384B9..738530 submits the Harvester warning before
            // its AI response/return, with or without a source. Keep this at
            // the Unit receiver; Infantry/Aircraft do not own this class tail.
            if let Some(target) = world.substrate.entities.get(target_id)
                && target.category() == EntityCategory::Unit
                && world
                    .interner
                    .object_type(target.type_ref(), rules)
                    .is_some_and(|obj| obj.harvester)
            {
                let event = UnderAttackEvent {
                    rx: target.position.rx(),
                    ry: target.position.ry(),
                    owner: target.owner(),
                    victim_type: target.type_ref(),
                };
                world.submit_under_attack_warning(event, rules, run.effects());
            }
        }
        // UnitClass's computer-owner response after its ping
        // (`0x00738535..0x0073866E`), for results 0 to 3: result 0 jumps
        // there past the ping (`0x007384BB`).
        if !entered_techno_death
            && matches!(
                receive_state,
                damage::DamageState::Unaffected
                    | damage::DamageState::Damaged
                    | damage::DamageState::Yellow
                    | damage::DamageState::Red
            )
        {
            world.unit_damage_response(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                rules,
                run.effects(),
            );
        }
        if receive_state != damage::DamageState::AlreadyDead
            && world
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|target| {
                    target.category() == EntityCategory::Structure
                        && target.lifecycle.object_alive()
                })
        {
            // Building44297B completes NotifyUnderAttack inline, including
            // its warning before House callbacks; do not queue it again.
            let _ = world.building_hit_response(
                target_id,
                (attacker_id != RAD_NO_ATTACKER).then_some(attacker_id),
                receive_state,
                rules,
                overlay_registry,
                run.effects(),
            );
        }
        finish_building_art_receiver(world, target_id, receive_state, building_entry_frame, rules);
    }

    (death, under_attack_events)
}

/// Building442A95 refreshes for nonzero returned result;442B37 independently
/// refreshes if current body frame differs from the entry value. Result5 and
/// cleared ObjectAlive leave before either arm. In particular healing result0
/// must not unconditionally synchronize the retained animation flag.
fn finish_building_art_receiver(
    world: &mut Simulation,
    target_id: u64,
    state: damage::DamageState,
    entry_frame: Option<i32>,
    rules: &RuleSet,
) {
    let Some(entry_frame) = entry_frame else {
        return;
    };
    if state == damage::DamageState::AlreadyDead {
        return;
    }
    let Some(target) = world.substrate.entities.get(target_id) else {
        return;
    };
    if !target.lifecycle.object_alive() {
        return;
    }
    let Some(object) = world.interner.object_type(target.type_ref(), rules) else {
        return;
    };
    if state != damage::DamageState::Unaffected
        || crate::sim::building_art::receiver_body_frame(
            target,
            object,
            rules,
            world.object_tech_level(object),
        ) != entry_frame
    {
        world.refresh_building_damage_state(target_id, rules);
    }
}

pub(crate) fn handle_death(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    dead_entities: &[u64],
    damage_events: &[EntityDamageEvent],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    building_contacts: Option<&[u64]>,
) -> DeathEffects {
    debug_assert!(
        dead_entities.len() <= 1,
        "ReceiveDamage enters one concrete fatal postlude at a time"
    );
    let mut death_sounds: Vec<(String, crate::sim::anim_class::AnimWorldCoord)> = Vec::new();
    let mut explosion_effects: Vec<ExplosionEffect> = Vec::new();
    let mut voxel_debris: Vec<crate::sim::voxel_anim::VoxelDebrisSpawn> = Vec::new();
    let mut under_attack_events: Vec<UnderAttackEvent> = Vec::new();
    let mut structure_destroyed: bool = false;
    // What the death weapon and the bomb set off, after this arm's own effects.
    let mut blast_effects = DeathEffects::default();
    for &dead_id in dead_entities {
        // Native702035 re-enters this branch for an already-zero receiver.
        // Keep unique diagnostic IDs, without suppressing receiver effects.
        if !run.handled_deaths.contains(&dead_id) {
            run.handled_deaths.push(dead_id);
        }
        let dead_info = world.substrate.entities.get(dead_id).map(|e| {
            if e.category() == EntityCategory::Structure {
                structure_destroyed = true;
            }
            let location = crate::sim::movement::ground_pose::object_location(
                e,
                world.resolved_terrain.as_ref(),
            );
            (
                e.type_ref(),
                location,
                e.owner(),
                e.category(),
                e.veterancy.rank(),
            )
        });

        if let Some((type_id, location, owner, _category, veterancy)) = dead_info {
            // Phobos `TechnoClass_ReceiveDamage_AttachEffectExpireWeapon`
            // (`0x702050`): effects with `ExpireWeapon.TriggerOn=death` fire
            // before the native death arm frees slaves and before the death
            // weapon.
            #[cfg(feature = "phobos")]
            if callbacks_enabled(world) {
                world.apply_attach_effect_death_weapons(
                    dead_id,
                    rules,
                    overlay_registry,
                    run.effects(),
                );
            }
            // `0x00702050..0x00702065`: the death arm first frees a master's
            // slaves to the killing hit's source (FreeSlaves, no house).
            if callbacks_enabled(world) {
                let killer = damage_events
                    .iter()
                    .rfind(|event| event.target_id == dead_id)
                    .map(|event| event.attacker_id)
                    .filter(|&attacker| attacker != RAD_NO_ATTACKER);
                world.free_slaves(
                    dead_id,
                    killer,
                    None,
                    rules,
                    overlay_registry,
                    run.effects(),
                );
            }
            // `0x00702112`: the death arm frees a controller's captives before
            // its death sounds (their fate draws precede the debris draws).
            if callbacks_enabled(world) {
                world.free_all_captures(dead_id, rules, overlay_registry, run.effects());
            }
            let type_id_str = world.interner.resolve(type_id);
            if let Some(obj) = rules.object(type_id_str) {
                // TechnoClass::ReceiveDamage70218B/7021FB: each selection is
                // followed by direct7509E0 admission before the next choice,
                // Stun and nested DeathWeapon receivers. House50B6F0 gates
                // VoiceDie; DieSound has no owner gate.
                let voice = if world.house_is_human_player(owner) {
                    obj.voice_die.as_slice()
                } else {
                    &[]
                };
                for choices in [voice, obj.die_sounds.as_slice()] {
                    if let Some(sound) = select_death_sound(choices, &world.main_rng) {
                        if callbacks_enabled(world) {
                            if sound_enabled(world) {
                                world.sound_events.emit(SimSoundEvent::voc_at_world_coord(
                                    sound,
                                    location.into(),
                                ));
                                run.effects().flush_sound(world, rules);
                            }
                        } else {
                            death_sounds.push((sound, location.into()));
                        }
                    }
                }
                // Numeric receiver fixtures retain their explicit packet
                // seam. Production's global cue belongs to441779 below.
                #[cfg(test)]
                if !callbacks_enabled(world)
                    && _category == EntityCategory::Structure
                    && obj.die_sounds.is_empty()
                    && let Some(sound) = rules
                        .general
                        .building_die_sound
                        .as_deref()
                        .filter(|name| !name.is_empty())
                {
                    death_sounds.push((sound.to_owned(), location.into()));
                }
                // 0x00702206..0x00702210: RADIO OVER_OUT to every contact and
                // the Stun, between the death sounds and the debris.
                if callbacks_enabled(world) {
                    world.stun(
                        dead_id,
                        crate::sim::world::UninitContext::new(rules, overlay_registry)
                            .with_effects(run.effects()),
                    );
                }
                // gamemd-derived: the debris block of
                // `TechnoClass::ReceiveDamage @ 0x00701900`
                // (`0x00702281`..`0x0070256C`). It sits BELOW the two death
                // sound draws in the same destruction arm and ABOVE
                // `UnitClass::Death_Explosion`, which `UnitClass::ReceiveDamage
                // @ 0x00737C90` only calls after this function has returned 4 —
                // so it lands here, between the sounds and the `Explosion=` /
                // `DestroyAnim=` draws below.
                // The pieces start from the dying object's GetCoords
                // (`vtable+0x48` at `0x007024FC`): a building's foundation
                // centre (`BuildingClass::GetCoords @ 0x00447AC0`).
                let center = world
                    .substrate
                    .entities
                    .get(dead_id)
                    .map_or((0, 0), |entity| {
                        let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
                        (x, y)
                    });
                // Techno702232..70227B jumps to 0x702672 for a low dropped
                // bomb over water, past the debris, KillPassengers and the
                // death weapon; ordinary water deaths still run them.
                let low_water_bomb = world.dropped_bomb_water_death_location(dead_id).is_some();
                if !low_water_bomb {
                    throw_debris_for_death(
                        world,
                        obj,
                        rules,
                        owner,
                        glam::IVec3::new(center.0, center.1, location.z),
                        &mut voxel_debris,
                        &mut explosion_effects,
                    );
                }
                // An exploding object's passengers die with it
                // (`0x00702603..0x00702667`, FootClass::KillPassengers' loop
                // inlined, credited to the killing hit's source) before its
                // death weapon. A unit that does not explode lets them escape
                // after this arm (`finish_concrete_death`).
                let numbered_weapon = world.substrate.entities.get(dead_id).and_then(|entity| {
                    super::combat_weapon::get_weapon(
                        entity,
                        obj,
                        super::combat_weapon::attacker_facts(entity, obj).current_weapon_number,
                        &world.substrate.entities,
                        rules,
                        &world.interner,
                    )
                });
                let explodes =
                    !low_water_bomb && death_arm_explodes(obj, veterancy, numbered_weapon);
                if explodes && callbacks_enabled(world) {
                    let attacker = killing_attacker(world, damage_events, dead_id);
                    world.kill_passengers(
                        dead_id,
                        attacker,
                        crate::sim::world::UninitContext::with_rules(rules)
                            .with_registry(overlay_registry)
                            .with_effects(run.effects()),
                    );
                }
                // `Fire_Death_Weapon(0)` (`0x0070266D`): its bullet
                // detonates here, inside this ReceiveDamage, through the
                // DetonateAtCoord every bullet takes. An IvanBomb warhead (the
                // Crazy Ivan's own bomber) plants a bomb on the dying object
                // instead of damaging (`0x00469343`), which goes off below.
                if explodes
                    && let Some(detonation) = super::death_weapon_detonation(
                        &mut world.interner,
                        &world.substrate,
                        &world.resolved_terrain,
                        dead_id,
                        rules,
                    )
                {
                    let commit =
                        commit_projectiles(world, run, &[detonation], rules, overlay_registry);
                    under_attack_events.extend(commit.under_attack_events);
                    blast_effects.append(commit.effects);
                }
            }
            // Phobos `TechnoClass_ReceiveDamage_RevengeWeapon` (`0x702672`):
            // after the death weapon, an effect-granted revenge weapon fires
            // at the killing source, if one exists.
            #[cfg(feature = "phobos")]
            if callbacks_enabled(world)
                && let Some(attacker_id) = killing_attacker(world, damage_events, dead_id)
            {
                world.apply_attach_effect_revenge_weapons(
                    dead_id,
                    attacker_id,
                    rules,
                    overlay_registry,
                    run.effects(),
                );
            }
            // `0x0070267F`: after its death weapon, the bomb it carries goes
            // off (`BombClass::Detonate @ 0x00438720`).
            if let Some((effects, pings)) =
                world.bomb_detonate_in_run(run, dead_id, rules, overlay_registry)
            {
                under_attack_events.extend(pings);
                blast_effects.append(effects);
            }

            // The world fatal prelude already owns garrison ejection before
            // the nested death weapon. Callback-disabled receiver fixtures
            // leave the cargo attached for their UnInit assertions.
        }
    }

    let mut effects = DeathEffects {
        structure_destroyed,
        explosion_effects,
        voxel_debris,
        death_sounds,
        under_attack_events,
        ..DeathEffects::default()
    };
    effects.append(blast_effects);

    // Concrete receivers resume after Techno's nested DeathWeapon. Read the
    // retained entity's current state at that boundary.
    for &dead_id in dead_entities {
        // Building442425 returns from Techno701900 before dispatching
        // NowDead4 to4424A2/442511. Its saved-contact RUN_AWAY/C4 loop follows
        // Destroy's BREAK/expiry and Techno's sounds, Stun, debris and nested
        // death weapon; Building DestructionEffects442665 follows it.
        // Techno702035 also forces result4 on an admitted Health0 re-entry. A
        // nested effect that cleared ObjectAlive skips the concrete wrapper
        // at44242C, and PostMortem never reaches this result4 continuation.
        if callbacks_enabled(world)
            && let Some(contacts) = building_contacts
            && world
                .substrate
                .entities
                .get(dead_id)
                .is_some_and(|building| {
                    building.category() == EntityCategory::Structure
                        && building.lifecycle.object_alive()
                })
        {
            let (nested, pings) =
                world.building_now_dead_contacts(dead_id, contacts, rules, overlay_registry, run);
            effects.append(nested);
            effects.under_attack_events.extend(pings);
        }
        finish_concrete_death(
            world,
            dead_id,
            run.selected_units.contains(&dead_id),
            damage_events,
            rules,
            overlay_registry,
            &mut effects,
            run.effects(),
        );
    }

    effects
}

/// Concrete receiver work after shared Techno death effects return.
/// Evidence: Infantry517FA0, Unit737C90 and Building442230 base-call order.
/// `selected_by_player` is the dying unit's local-player selection on
/// receiver entry ([`ReceiverRun::selected_units`]), which the kill's Destroy
/// callback has since cleared.
fn finish_concrete_death(
    world: &mut Simulation,
    dead_id: u64,
    selected_by_player: bool,
    damage_events: &[EntityDamageEvent],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut DeathEffects,
    frame_effects: FrameEffects<'_>,
) {
    let Some(entity) = world.substrate.entities.get(dead_id) else {
        return;
    };
    let category = entity.category();
    // Building442230 returns before its result dispatch when Alive is clear.
    if category == EntityCategory::Structure && !entity.lifecycle.object_alive() {
        return;
    }
    let (type_id, owner, rx, ry, has_animation) = (
        entity.type_ref(),
        entity.owner(),
        entity.position.rx(),
        entity.position.ry(),
        entity.animation.is_some(),
    );
    let mut concrete_smudge_plans = Vec::new();
    if let Some(obj) = world.interner.object_type(type_id, rules) {
        // `TechnoClass::Death_Announcement @ 0x004D98C0` runs at the
        // Aircraft/Infantry/Unit `ReceiveDamage` kill sites (vtable
        // `+0x3B8`; `BuildingClass` has none) and skips `Spawned=`
        // types at `0x004D98DD`. Its owner gate (`0x0050B6F0`) and
        // the `CreateRadarEvent(7)` dedupe (`0x004D98FE`) need the
        // house table and radar queue, which the world owns.
        if let Some(event) = death_announcement_event(obj, category, rx, ry, owner) {
            effects.unit_lost_events.push(event);
        }
    }
    // Look up the warhead that dealt the killing blow for InfDeath
    // selection below. The AnimList anim + smudge are emitted at
    // the per-shot fire site (and at the death-AoE loop), not here.
    let killing_warhead = damage_events
        .iter()
        .rfind(|event| event.target_id == dead_id)
        .and_then(|event| {
            rules
                .warhead(world.interner.resolve(event.warhead_ref))
                .map(|wh| (wh, event.damage))
        });
    // The killing call's concrete receiver booleans: IgnoreDefenses (arg5)
    // becomes the building's NoSurvivor and kills a unit's passengers, arg6
    // gates the vehicle crew.
    let killing_flags = damage_events
        .iter()
        .rfind(|event| event.target_id == dead_id)
        .map(|event| event.receiver_flags);
    let ignore_defenses = killing_flags.is_some_and(|flags| flags.ignore_defenses);
    // Ares's survivor release (`0x100464C0`) reads only IgnoreDefenses.
    #[cfg(not(feature = "ares"))]
    let prevent_crew_escape = killing_flags.is_some_and(|flags| flags.arg6);

    // BuildingClass runs DestructionEffects/SpawnSurvivors only after
    // TechnoClass's synchronous death weapon has returned. Capture the
    // immutable plan now; placement and all RNG stay at that postlude.
    if category == EntityCategory::Structure {
        concrete_smudge_plans.push(ConcreteDeathSmudgePlan::Building);
    }

    // `UnitClass::Death_Explosion @ 0x00738680` unless the unit sinks
    // (`0x00737DE2`), and the Aircraft death arm (`0x0041661F`); the killing
    // detonation's own impact anim follows its receivers. Infantry play none:
    // every `Explosion=` reader is a Unit, Aircraft or Building body
    // (`get_xrefs_to 0x00738680`; the `type+0x73C` operand scan), and
    // infantry death anims come from `InfDeath`/`DeathAnims`.
    match category {
        EntityCategory::Unit => {
            if world.unit_sinks_on_death(rules, dead_id) {
                world.begin_ship_sinking(dead_id, rules, frame_effects);
            } else if !world.unit_dropped_bomb_water_splash(
                rules,
                dead_id,
                &mut effects.explosion_effects,
            ) {
                world.unit_death_explosion(rules, dead_id, &mut effects.explosion_effects);
            }
        }
        EntityCategory::Aircraft => {
            world.aircraft_death_explosion(rules, dead_id, &mut effects.explosion_effects)
        }
        _ => {}
    }
    // `UnitClass::ReceiveDamage` then lifts the dying unit off its cell
    // (vt+0x124 Mark(UP) at `0x00737F7A`) before its passengers and crew
    // leave. The UnInit that follows finds it already unmarked. Mark leaves
    // the AircraftTracker alone: a falling Jumpjet wreck stays in it until its
    // impact (`0x0054D075`), any other dying unit until its UnInit.
    let crashable = category == EntityCategory::Unit
        && world
            .object_type(type_id, rules)
            .is_some_and(|object| object.crashable);
    if category == EntityCategory::Unit && callbacks_enabled(world) {
        world.foot_mark_remove(dead_id, rules, overlay_registry, frame_effects);
        let dying = crate::sim::crew_survival::DyingTransport {
            attacker: killing_attacker(world, damage_events, dead_id),
            ignore_defenses,
            selected_by_player,
        };
        #[cfg(feature = "ares")]
        world.release_ares_vehicle_survivors(
            rules,
            overlay_registry,
            dead_id,
            dying,
            frame_effects,
        );
        #[cfg(not(feature = "ares"))]
        {
            world.release_dying_unit_passengers(
                rules,
                overlay_registry,
                dead_id,
                dying,
                frame_effects,
            );
            world.spawn_vehicle_crew(
                rules,
                overlay_registry,
                dead_id,
                prevent_crew_escape,
                selected_by_player,
                frame_effects,
            );
        }
        world.drop_unit_cargo_crate(dead_id, rules, overlay_registry);
    }

    // InfantryClass::ReceiveDamage reads the killing call's InfDeath off its
    // warhead argument with no null test (`0x00518362..0x00518369`).
    // commit_entities hands every production kill that call's record, whose
    // warhead its receiver resolved (a record it cannot resolve does no
    // damage). Only a receiver fixture without one falls back, to the
    // constructor's InfDeath.
    let inf_death = killing_warhead
        .as_ref()
        .map_or(WarheadType::DEFAULT_INF_DEATH, |(wh, _)| wh.inf_death);
    if category == EntityCategory::Infantry {
        // `InfantryClass::ReceiveDamage` on result 4 (`0x00518077..0x0051808E`):
        // a dying slave leaves its master's node (RemoveSlave).
        Simulation::remove_slave(&world.session, &mut world.substrate, dead_id);
        // The house InfDeath 8 and 9 give their anim (`0x0051887B`,
        // `0x00518A96`): the source's owner (`+0x21C`), else the house
        // argument.
        let anim_house = killing_attacker(world, damage_events, dead_id)
            .and_then(|attacker| world.substrate.entities.get(attacker))
            .map(|attacker| attacker.owner())
            .or_else(|| {
                damage_events
                    .iter()
                    .rfind(|event| event.target_id == dead_id)
                    .and_then(|event| event.source_house)
            });
        let postlude = world.begin_infantry_receiver_death(
            dead_id,
            inf_death,
            anim_house,
            rules,
            overlay_registry,
            frame_effects,
        );
        concrete_smudge_plans.push(ConcreteDeathSmudgePlan::Infantry(postlude));
        effects.despawned_ids.push(dead_id);
    } else if has_animation
        && !world
            .substrate
            .entities
            .get(dead_id)
            .is_some_and(|entity| entity.sinking.is_active())
    {
        // Non-Infantry SHP lifetime remains on its existing, VERA-only path.
        // Natively only InfantryClass::ReceiveDamage reads InfDeath; this
        // corpse borrows the infantry table's Die1/Die2 arms.
        if let Some(entity) = world.substrate.entities.get_mut(dead_id) {
            entity.death.begin();
            if let (Some(sequence), Some(anim)) = (
                crate::sim::world::InfantryDeathSequence::for_inf_death(inf_death),
                entity.animation.as_mut(),
            ) {
                anim.switch_to(sequence.animation());
            }
        }
        // VERA-only death sequence: native's UnInit would Limbo the object
        // here, whose ReReveal (`0x006F6B16`) takes its sight off the maps.
        // An aircraft's corpse defers that UnInit to the sequence's end (a
        // Unit's receiver runs it next), so its sight leaves now.
        world.release_object_sight_with_context(
            dead_id,
            crate::sim::world::UninitContext::new(rules, overlay_registry)
                .with_effects(frame_effects),
        );
        // The corpse stays in the store for its death animation; the death
        // arm's OVER_OUT to every contact (`stun`) already
        // released its dock slots.
        effects.despawned_ids.push(dead_id);
    } else if category == EntityCategory::Aircraft && callbacks_enabled(world) {
        #[cfg(feature = "ares")]
        {
            //41668B ->10048C50 reads the live selected byte here (unlike
            //Unit's entry snapshot), then calls shared survivors100464C0.
            let selected = world
                .substrate
                .entities
                .get(dead_id)
                .is_some_and(|aircraft| {
                    aircraft.selection.is_selected() && world.owner_is_human(aircraft.owner())
                });
            let dying = crate::sim::crew_survival::DyingTransport {
                attacker: killing_attacker(world, damage_events, dead_id),
                ignore_defenses,
                selected_by_player: selected,
            };
            world.release_ares_vehicle_survivors(
                rules,
                overlay_registry,
                dead_id,
                dying,
                frame_effects,
            );
        }
        #[cfg(feature = "ares")]
        let may_crash = world
            .substrate
            .entities
            .get(dead_id)
            .and_then(|aircraft| world.entity_object_type(aircraft, rules))
            .is_none_or(|object| object.aircraft_crashable_override() != Some(false));
        #[cfg(not(feature = "ares"))]
        let may_crash = true;
        //10048CB5..10048CDB skips the original Crash only for an explicit
        //Crashable=no; absence still takes Aircraft416694's original call.
        if !may_crash
            || !world.foot_crash(
                dead_id,
                killing_attacker(world, damage_events, dead_id),
                rules,
                overlay_registry,
                frame_effects,
            )
        {
            // AircraftClass::ReceiveDamage4166A3 UnInits a refused Crash
            // before returning. A later area receiver's passenger placement
            // must see its cell cleared, rather than wait for batch delivery.
            world.uninit_with_context(
                dead_id,
                crate::sim::world::UninitContext::new(rules, overlay_registry)
                    .with_effects(frame_effects),
            );
        }
        // `AircraftClass::ReceiveDamage` (`0x00416694..0x004166A3`): an
        // airborne aircraft crashes instead of its UnInit. It stays alive and
        // represented, with Health 0, until its fall's impact.
        effects.despawned_ids.push(dead_id);
    } else if crashable
        && callbacks_enabled(world)
        && world.foot_crash(dead_id, None, rules, overlay_registry, frame_effects)
    {
        // `UnitClass::ReceiveDamage` (`0x00738457..0x00738475`): an airborne
        // `Crashable=` unit crashes (`Crash(0)`, no attacker) instead of its
        // UnInit, and falls to its impact (a Jumpjet: `jumpjet_crash_impact`).
        effects.despawned_ids.push(dead_id);
    } else if category == EntityCategory::Unit
        && !crashable
        && world
            .substrate
            .entities
            .get(dead_id)
            .is_some_and(|entity| entity.sinking.is_active())
    {
        // Unit738493 checks +3CD after Mark(UP), passenger/crew handling and
        // Crashable; a Crashable type its Crash refused is UnInit even when
        // sinking (`0x0073847F`). A sinking hull retains Alive and its
        // Logic/display slot.
        effects.despawned_ids.push(dead_id);
    } else {
        // A building's UnInit waits for its DestructionEffects below.
        if category != EntityCategory::Structure {
            effects.immediate_uninit_ids.push(dead_id);
        }
        effects.despawned_ids.push(dead_id);
    }
    // The receiver owns the recursion boundary; each concrete owner consumes
    // its captured postlude here without moving constructor/smudge RNG earlier.
    for plan in concrete_smudge_plans {
        match plan {
            ConcreteDeathSmudgePlan::Infantry(postlude) => {
                let uninit = callbacks_enabled(world).then(|| {
                    crate::sim::world::UninitContext::new(rules, overlay_registry)
                        .with_effects(frame_effects)
                });
                postlude.commit(world, rules, effects, uninit);
            }
            ConcreteDeathSmudgePlan::Building => {
                // DestructionEffects (`0x004415F0`): the building's own anims,
                // centre mark, death fuse and NoSurvivor, then SpawnSurvivors
                // (`0x00441F1B`), which interleaves each foundation cell's
                // survivor roll with that cell's own mark.
                let deferred = &mut effects.smudge_spawn_requests;
                world.building_destruction_effects(
                    rules,
                    dead_id,
                    ignore_defenses,
                    &mut effects.explosion_effects,
                    |world, request| {
                        commit_smudges(world, rules, overlay_registry, vec![request], deferred);
                    },
                    frame_effects,
                );
                if callbacks_enabled(world) {
                    let deferred = &mut effects.smudge_spawn_requests;
                    world.spawn_building_survivors(
                        rules,
                        overlay_registry,
                        dead_id,
                        |world, (cell_rx, cell_ry)| {
                            commit_smudges(
                                world,
                                rules,
                                overlay_registry,
                                vec![SmudgeSpawnRequest::BuildingSurvivor { cell_rx, cell_ry }],
                                deferred,
                            );
                        },
                        frame_effects,
                    );
                }
            }
        }
    }
    // Case 4 UnInits a building only while the fuse DestructionEffects armed
    // has time left (`0x0044266B..0x00442690`).
    let frame = world.session.binary_frame() as i32;
    if category == EntityCategory::Structure
        && world
            .substrate
            .entities
            .get(dead_id)
            .is_some_and(|building| building.pending_c4_detonation.has_time_left(frame))
    {
        effects.immediate_uninit_ids.push(dead_id);
    }
}

/// The killing ReceiveDamage call's attacker (its fourth argument), when it
/// names an object.
fn killing_attacker(
    world: &Simulation,
    damage_events: &[EntityDamageEvent],
    dead_id: u64,
) -> Option<u64> {
    damage_events
        .iter()
        .rfind(|event| event.target_id == dead_id)
        .map(|event| event.attacker_id)
        .filter(|&attacker| {
            attacker != crate::sim::combat::RAD_NO_ATTACKER
                && world.substrate.entities.contains(attacker)
        })
}

/// What a special arm of `BulletClass::DetonateAtCoord` reads as its target:
/// the bullet's `+0x10C`, which is an object, a cell (a real one or the dummy
/// cell) or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecialArmTarget {
    Object(u64),
    Cell,
    None,
}

/// The special arm of `BulletClass::DetonateAtCoord @ 0x004690B0`
/// (`0x0046920B..0x00469A3D`) the warhead's flags selected, for both
/// deliveries: visible and `Inviso=` bullets both enter when their AI
/// admits detonation; Inviso placement does not bypass the Arm-gated fuse.
///
/// Whatever the arm does, it claims the impact: every arm, its internal
/// refusals included, leaves by `JMP 0x00469AA4`, and `Apply_area_damage`
/// (`0x00469A83`) and `SpawnShrapnel` are reachable only from the final else
/// at `0x00469A3F`. So the caller runs neither, and runs the shared tail below
/// `LAB_00469AA4` after this returns. `owner` is the bullet's `+0xB0`.
///
/// DirectRocker (`0x0046978E`) remains unported; its admitted arm claims the
/// impact without an effect. Airstrike delegates to the manager lifecycle.
fn run_special_detonation_arm(
    world: &mut Simulation,
    rules: &RuleSet,
    action: SpecialDetonationAction,
    detonation: &ProjectileDetonation,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let owner = detonation.source_id;
    let target = match detonation.target {
        ProjectileTarget::Entity(id) | ProjectileTarget::Terrain(id) => {
            SpecialArmTarget::Object(id)
        }
        ProjectileTarget::Cell { .. } | ProjectileTarget::DummyCell => SpecialArmTarget::Cell,
        ProjectileTarget::None => SpecialArmTarget::None,
    };
    let object = match target {
        SpecialArmTarget::Object(id) => Some(id),
        SpecialArmTarget::Cell | SpecialArmTarget::None => None,
    };
    match action {
        SpecialDetonationAction::OrdinaryDamage => {
            debug_assert!(false, "the ordinary arm is its caller's");
        }
        SpecialDetonationAction::Parasite => {
            // 0x004693DD..0x0046941E: no bullet Owner (+0xB0) -> nothing;
            // otherwise Owner->ParasiteImUsing->AttachTo(Target if FootClass).
            // The impact point is not consulted.
            if world.substrate.entities.contains(owner) {
                let victim = object.filter(|&id| {
                    world.substrate.entities.get(id).is_some_and(|target| {
                        matches!(
                            target.category(),
                            EntityCategory::Unit
                                | EntityCategory::Infantry
                                | EntityCategory::Aircraft
                        )
                    })
                });
                world.parasite_attach(owner, victim, rules, effects);
            }
        }
        SpecialDetonationAction::MindControl => {
            world.mind_control_detonation(*detonation, rules, overlay_registry, effects);
        }
        SpecialDetonationAction::Temporal => {
            let target = match target {
                SpecialArmTarget::Object(id) => {
                    crate::sim::temporal::TemporalShotTarget::Object(id)
                }
                SpecialArmTarget::Cell => crate::sim::temporal::TemporalShotTarget::Cell,
                SpecialArmTarget::None => crate::sim::temporal::TemporalShotTarget::None,
            };
            world.temporal_detonation(owner, target, rules, overlay_registry, effects);
        }
        SpecialDetonationAction::IvanBomb => {
            // 0x00469343..0x00469375: the bullet's owner plants on a Techno.
            Simulation::bomb_attach(
                &world.interner,
                &world.session,
                &mut world.substrate,
                &mut world.sound_events,
                &world.resolved_terrain,
                &mut world.bombs,
                owner,
                object,
                rules,
            );
        }
        SpecialDetonationAction::BombDisarm => {
            // 0x004699C4..0x004699FE: a bombed Object target is defused.
            if let Some(id) = object {
                Simulation::bomb_defuse(&mut world.substrate, &mut world.bombs, id);
            }
        }
        SpecialDetonationAction::NukeMaker => nuke_maker(world, rules, detonation),
        SpecialDetonationAction::MakesDisguise => {
            // 0x00469A03..0x00469A2A: no bullet Owner (+0xB0) -> nothing;
            // otherwise Owner->DisguiseAs(Target) through vt+0x46C.
            if world.substrate.entities.contains(owner) {
                world.disguise_as(
                    owner,
                    object,
                    !matches!(target, SpecialArmTarget::None),
                    rules,
                );
            }
        }
        SpecialDetonationAction::Locomotor => {
            // 0x004694CB..0x00469700: the bullet's strength (+0x6C) and the
            // warhead's Locomotor= class (+0x15C).
            let Some(warhead) = rules.warhead(world.interner.resolve(detonation.payload.warhead))
            else {
                return;
            };
            let target = match target {
                SpecialArmTarget::Object(id) => {
                    crate::sim::locomotor_lift::LocomotorShotTarget::Object(id)
                }
                SpecialArmTarget::Cell => crate::sim::locomotor_lift::LocomotorShotTarget::Cell,
                SpecialArmTarget::None => crate::sim::locomotor_lift::LocomotorShotTarget::None,
            };
            #[cfg(feature = "phobos")]
            let jumpjet_override = warhead.jumpjet_override;
            #[cfg(not(feature = "phobos"))]
            let jumpjet_override = crate::rules::jumpjet_params::JumpjetOverride::default();
            world.locomotor_detonation(
                owner,
                target,
                detonation.payload.base_damage,
                warhead.locomotor,
                &jumpjet_override,
                rules,
                overlay_registry,
                effects,
            );
        }
        SpecialDetonationAction::ElectricAssault => {
            world.substrate.electric_assault_detonation(owner, object);
        }
        SpecialDetonationAction::Airstrike => {
            world.airstrike_detonation(owner, object, rules, effects);
        }
        SpecialDetonationAction::DirectRocker => {
            log::debug!(
                "special detonation {action:?} from {owner} claimed with its body unported; \
                 shrapnel and area damage suppressed, the shared tail still runs"
            );
        }
    }
}

/// `BulletClass::NukeMaker @ 0x0046B310` (Ghidra `SpawnDownwardNuke`), the
/// arm of a `NukeMaker=` warhead (`0x00469A2C`): the nuclear missile's
/// falling warhead. A bullet of the `NukePayload` weapon (the literal at
/// `0x0081AFA0`, `0x0046B371`) is built with that weapon's BulletType,
/// damage, warhead and speed (`BulletClass::Construct @ 0x004664C0`), the
/// detonating bullet's target and owner, bright, and given the weapon
/// (`+0x130`). It falls from the GetCoords of the cell under the target's
/// GetCoords (`0x0046B32E..0x0046B36C`), raised by the detonating weapon's
/// BulletType's `DetonationAltitude=` (`+0x2BC`, `0x0046B459`), straight
/// down ([`missile_launch_velocity`] at -1.0).
///
/// Native dereferences a missing target, NukePayload weapon or warhead; VERA
/// drops nothing then.
///
/// [`missile_launch_velocity`]: crate::sim::projectile::launch::missile_launch_velocity
fn nuke_maker(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    /// `[0x007E4900]` = -1.0.
    const FALL_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0xbff0_0000_0000_0000);
    let terrain = world.resolved_terrain.as_ref();
    let target_coords = match detonation.target {
        ProjectileTarget::Entity(id) => object_get_coords(world, id),
        ProjectileTarget::Terrain(id) => world
            .terrain_objects
            .objects()
            .get(&id)
            .map(|tree| tree.world_coord()),
        ProjectileTarget::Cell { rx, ry } => {
            Some(crate::sim::projectile::cell_ground_coord(terrain, rx, ry))
        }
        ProjectileTarget::DummyCell => Some(crate::sim::projectile::dummy_cell_ground_coord(
            &world.effective_shared_cell_dummy(),
        )),
        ProjectileTarget::None => None,
    };
    let altitude = detonation
        .payload
        .installed_weapon()
        .and_then(|weapon| rules.weapon(world.interner.resolve(weapon)))
        .and_then(|weapon| weapon.projectile.as_deref())
        .and_then(|projectile| rules.projectile(projectile))
        .map_or(0, |projectile| projectile.detonation_altitude);
    let weapon = rules.weapon(NUKE_PAYLOAD_WEAPON);
    let warhead = weapon
        .and_then(|weapon| weapon.warhead.as_deref())
        .and_then(|name| rules.warhead(name));
    let (Some(target_coords), Some(weapon), Some(warhead)) = (target_coords, weapon, warhead)
    else {
        return;
    };
    let (rx, ry) = (
        (target_coords.x / 256) as u16,
        (target_coords.y / 256) as u16,
    );
    let ground = crate::sim::projectile::cell_ground_coord(terrain, rx, ry);
    let origin = ProjectileCoord::new(ground.x, ground.y, ground.z.wrapping_add(altitude));
    let bullet_id = world.substrate.allocate_stable_id();
    let native_unique_id = world.next_native_runtime_id();
    let velocity = crate::sim::projectile::launch::missile_launch_velocity(FALL_SCALE);
    let payload = ProjectilePayload::new(
        weapon.damage,
        world.interner.intern(&warhead.id),
        weapon
            .projectile
            .as_deref()
            .map(|name| world.interner.intern(name)),
        true,
    )
    .with_installed_weapon(Some(world.interner.intern(&weapon.id)));
    if let Some(bullet) = super::FiredBullet::launched(
        world,
        rules,
        native_unique_id,
        detonation.source_id,
        detonation.target,
        payload,
        weapon.speed,
        origin,
        velocity,
    ) {
        super::admit_fired_bullet(world, rules, weapon, bullet_id, bullet);
    }
}

/// The weapon NukeMaker drops, named by the literal `0x0081AFA0`.
const NUKE_PAYLOAD_WEAPON: &str = "NukePayload";

/// Snapshot the native primary-cell wall branch before its receiver collection
/// mutates the overlay. Its return skips the bridge tail even if the wall dies.
pub(crate) fn area_routes_to_wall(
    world: &Simulation,
    overlay_registry: Option<&OverlayTypeRegistry>,
    cell: (u16, u16),
    warhead: &WarheadType,
) -> bool {
    let (Some(grid), Some(registry)) = (world.overlay_grid.as_ref(), overlay_registry) else {
        return false;
    };
    let overlay_id = if let Some(terrain) = world.resolved_terrain.as_ref() {
        //489342 retains the primary CellClass, including fixed512 aliases
        //and the live Dummy. Its overlay, not a rectangular requested key,
        //decides whether this call skips its later bridge continuation.
        let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let identity = query.lookup((cell.0 as i16, cell.1 as i16));
        match identity {
            crate::map::cell_index::NativeCellIdentity::Real(_) => {
                let (rx, ry) = query.coord(identity);
                grid.cell(rx as u16, ry as u16).overlay_id
            }
            crate::map::cell_index::NativeCellIdentity::Dummy => {
                u8::try_from(query.dummy_overlay_identity()).ok()
            }
        }
    } else {
        // Mapless focused hosts have only their supplied rectangular grid.
        grid.cell(cell.0, cell.1).overlay_id
    };
    overlay_id
        .and_then(|id| registry.flags(id))
        .is_some_and(|flags| flags.wall && warhead_damages_wall(warhead, flags))
}

/// Apply_area_damage489B49..48A479 (original gamemd.exe). After synchronous
/// receivers: rocker, bridge, Overlay.Explodes, then ordinary WH.Particle.
/// The original arguments and selected layer survive every nested callback.
fn continue_area_damage(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    context: AreaDamageContinuation<'_>,
    area_result: AreaDamageResult,
) -> (DeathEffects, Vec<UnderAttackEvent>) {
    let mut death = DeathEffects::default();
    let mut pings = Vec::new();
    if !context.entered || area_result == AreaDamageResult::IronCurtain {
        return (death, pings);
    }
    area_rocker::apply(
        world,
        rules,
        context.warhead,
        area_rocker::AreaRockerInput {
            impact: context.impact,
            cell: context.cell,
            damage: context.damage,
            source_id: context.source_id,
            list_layer: context.object_list_layer,
        },
    );
    //489EA4 re-looks up the decoded packed impact Cell, before the bridge
    //callbacks. Retain this allocation, including the shared Dummy; do not
    //replace it with the earlier489342 Cell or a post-bridge lookup.
    let cell = world.resolved_terrain.as_ref().map(|terrain| {
        terrain.native_cell_identity((context.cell.0 as i16, context.cell.1 as i16))
    });
    if !context.routed_wall && context.warhead.wall {
        let event = BridgeDamageEvent {
            rx: context.cell.0,
            ry: context.cell.1,
            damage: context.damage,
            warhead_ref: context.warhead_ref,
            is_ion_cannon: context.warhead_ref == world.rule_handles().ion_cannon,
            impact_z_leptons: context.impact.z,
        };
        death.bridge_state_changed |= crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
            world, rules, std::slice::from_ref(&event), overlay_registry, run.effects(),
        );
    }
    let coord = crate::sim::anim_class::AnimWorldCoord {
        x: context.impact.x,
        y: context.impact.y,
        z: context.impact.z,
    };
    let position = glam::IVec3::new(coord.x, coord.y, coord.z);
    if let (Some(cell), Some(registry)) = (cell, overlay_registry)
        && world.remove_exploding_overlay(cell, rules, registry, run.effects())
    {
        //48A34D: loop1/600h/z0/delay0 at the unchanged impact. The shared
        //nullable constructor preserves identity and constructor RNG order.
        if let Err(error) = world.emit_debris_anim(
            rules,
            rules.general.barrel_explode.as_deref(),
            coord,
            &mut death.explosion_effects,
        ) {
            log::warn!("barrel explosion animation did not construct: {error}");
        }
        //48A371: null source, ORIGINAL outer sourceHouse, C4 and affectTib1.
        //Keep the same receiver run across this recursive gameplay chain.
        if let Some(c4) = rules.warhead(&rules.bridge_warheads.c4_name) {
            let reference = world.interner.intern(&c4.id);
            let (nested, mut nested_pings, _crit_active, _area_result) = apply_area_damage_in_run(
                world,
                run,
                rules,
                overlay_registry,
                context.impact,
                rules.combat_damage.ammo_crate_damage,
                c4,
                true,
                (RAD_NO_ATTACKER, context.source_house, reference),
            );
            death.append(nested);
            pings.append(&mut nested_pings);
        }
        //48A38A..48A3D8: ordered trials stop after the FIRST success. The
        //selected Voxel constructor finishes its RNG before the particle trial.
        for name in &rules.general.barrel_debris {
            if world.scenario_rng.next_range_u32_inclusive(0, 99) >= 15 {
                continue;
            }
            if let Some(type_id) = rules.voxel_anim_type_id_by_name(name) {
                let voxel_type = rules.voxel_anim_type(type_id);
                let inline = callbacks_enabled(world);
                if let Err(error) = world.emit_debris_voxel(
                    type_id,
                    voxel_type,
                    None,
                    position,
                    inline,
                    &mut death.voxel_debris,
                ) {
                    log::warn!("barrel debris did not construct: {error}");
                }
            }
            break;
        }
        //48A3E3..48A41F is separate, even with an empty debris list. The
        //system freezes zero target coords; its explicit child receives the
        //impact for BOTH source and target, not that frozen system target.
        if world.scenario_rng.next_range_u32_inclusive(0, 99) < 25
            && let Some(type_id) = rules
                .general
                .barrel_particle
                .as_deref()
                .and_then(|name| rules.ps_type_id_by_name(name))
            && let Some(system) = world.spawn_particle_system(
                type_id,
                position,
                None,
                None,
                glam::IVec3::ZERO,
                None,
                rules,
            )
        {
            world.spawn_particle_in_system(system, position, position, rules);
        }
    }
    //48A437..48A479 follows both the bridge and Explodes branches. WH.Particle
    //uses the outer sourceHouse and also constructs one immediate child.
    if let Some(type_id) = context
        .warhead
        .particle_system
        .as_deref()
        .and_then(|name| rules.ps_type_id_by_name(name))
        && let Some(system) = world.spawn_particle_system(
            type_id,
            position,
            None,
            None,
            glam::IVec3::ZERO,
            context.source_house,
            rules,
        )
    {
        world.spawn_particle_in_system(system, position, position, rules);
    }
    (death, pings)
}

/// Installed46920B ->100536E0's local extension-effect coordinate. The
/// original receiver still consumes its unchanged impact argument. Kept here
/// with the single pre-effect owner, using the shared5F6440 distance port.
#[cfg(feature = "ares")]
pub(crate) fn ares_warhead_effect_coordinate(
    world: &Simulation,
    detonation: &ProjectileDetonation,
) -> ProjectileCoord {
    //100536E0/10053738..70: the extension effects use a local coordinate.
    //Ares10053660 jumps to the existing planar5F6440 Distance_To, including
    //the target Building's foundation discount. Strict<64 snaps to+48,
    //not Building+A4/TargetCoordOffset. Original damage retains its impact.
    let target = match detonation.target {
        ProjectileTarget::Entity(id) | ProjectileTarget::Terrain(id) => {
            Some(TargetKind::Entity(id))
        }
        ProjectileTarget::Cell { rx, ry } => Some(TargetKind::Cell(rx, ry)),
        ProjectileTarget::None | ProjectileTarget::DummyCell => None,
    };
    target
        .filter(|target| {
            object_distance_to(detonation.projectile_id, target, world)
                .is_some_and(|distance| distance < 64)
        })
        .and_then(|target| {
            let cells = world
                .resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            crate::sim::movement::ground_pose::target_get_coords(target, world, cells.as_ref())
        })
        .map_or(detonation.impact, |coord| {
            ProjectileCoord::new(coord.x, coord.y, coord.z)
        })
}

/// `BulletClass::DetonateAtCoord @ 0x004690B0` up to its receivers: the
/// radiation site, then the special-warhead chain or, in its final else, the
/// shrapnel (`0x00469A51`) and `Apply_area_damage` (`0x00489280`) records,
/// then its shared continuation. Returns the ordinary area's frozen context, or
/// None for a special arm without Apply_area_damage. The receivers and bridge
/// continuation commit before the anim tail
/// ([`emit_detonation_anim`]), as native's area damage returns before
/// `LAB_00469AA4`.
///
/// The area damage's house (`0x00469A5C..0x00469A75`) is the live bullet
/// owner's: a bullet whose firer has expired (`BulletClass::PointerExpired
/// @ 0x004684E0` nulled `+0xB0`) damages with no house.
fn emit_detonation_receivers<'r>(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    detonation: &ProjectileDetonation,
    warhead: &'r WarheadType,
    out: &mut CombatEmit,
) -> (Option<AreaDamageContinuation<'r>>, bool) {
    let effects = run.effects();
    let (impact_rx, impact_ry, ..) = projectile_impact_cell(detonation.impact);

    // Named location: `BulletClass::Detonate @ 0x004690b0`. Radiation is
    // outside and before the exclusive special-effect chain.
    if let Some(weapon) = detonation
        .payload
        .installed_weapon()
        .and_then(|weapon| rules.weapon(world.interner.resolve(weapon)))
        && weapon.rad_level > 0
        && let Some(rad_warhead) = weapon
            .warhead
            .as_deref()
            .and_then(|name| rules.warhead(name))
    {
        out.effects
            .rad_detonations
            .push(crate::sim::radiation::RadDetonation {
                rx: impact_rx,
                ry: impact_ry,
                rad_level: weapon.rad_level,
                // 46917A/469180 read Weapon+AC -> Warhead+124, which
                // need not be the Bullet's independently constructed warhead.
                spread: super::cell_spread::whole_cells(rad_warhead.cell_spread_f64),
                // Phobos `WeaponTypeExt::RadType` binds the field's type.
                rad_type: weapon.rad_type.clone(),
                // Phobos `CreateInstance(owner, invoker)`: the round's house
                // and firer, kept only when the bound type asks for them.
                firing_house: world
                    .substrate
                    .entities
                    .get(detonation.source_id)
                    .map(|entity| entity.owner()),
                invoker: (detonation.source_id != 0).then_some(detonation.source_id),
            });
    }

    // Installed Ares46920B ->100536E0/10052870 runs after radiation,
    // before the exclusive special/ordinary damage arms (including Damage=0).
    #[cfg(feature = "ares")]
    {
        let coordinate = ares_warhead_effect_coordinate(world, detonation);
        // Parent100536E0 captures OwnerHouse before EMP/tag callbacks. Phobos
        // substitutes `FirerHouse` only in its own `WarheadTypeExt::Detonate`.
        let source_house = world
            .substrate
            .entities
            .get(detonation.source_id)
            .map(|source| source.owner());
        world.apply_ares_emp_detonation(
            coordinate,
            Some(detonation.source_id),
            warhead,
            rules,
            effects,
        );
        world.apply_ares_attach_effect_detonation(coordinate, source_house, warhead, rules);
    }

    // `BulletClass::DetonateAtCoord @ 0x0046978e` is the chain's only
    // conditional arm and the only one that consults the target: it reads the
    // bullet's raw `+0x10c` and calls `What_Am_I` through vtable `+0x2c`,
    // claiming the impact only for RTTI 1 (`UnitClass::What_Am_I
    // @ 0x00746e20`). A cell, dummy-cell or null target is never a UnitClass.
    let special_target = SpecialDetonationTarget {
        is_unit: match detonation.target {
            ProjectileTarget::Entity(id) => world
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Unit),
            ProjectileTarget::Cell { .. }
            | ProjectileTarget::Terrain(_)
            | ProjectileTarget::None
            | ProjectileTarget::DummyCell => false,
        },
    };
    let special_action =
        projectile_special_detonation_action(SpecialDetonationFlags::of(warhead), special_target);

    // Native ownership: only the final else at `0x00469a3f` runs
    // `BulletClass::SpawnShrapnel @ 0x0046a310` and
    // `Apply_area_damage @ 0x00489280`, so an arm that claims the impact
    // shadows both — including an arm whose effect body VERA has not ported,
    // because native's own bail-outs (e.g. `0x00469235`) shadow them too.
    // What an arm never shadows is the shared tail below `LAB_00469AA4`: every
    // arm reaches the explosion anim and its smudge. The RESIDUAL list for the
    // unported effect bodies lives on `SpecialDetonationAction`.
    match special_action {
        SpecialDetonationAction::OrdinaryDamage => {
            emit_projectile_shrapnel(
                world,
                rules,
                detonation,
                bullet_location(world, detonation).0,
            );

            let current = world.projectiles.current_detonation(*detonation);
            let detonation = &current;

            // `0x00469A69..0x00469A75`: the bullet's live Owner's house;
            // Phobos keeps `BulletExt::FirerHouse` once the owner is gone.
            let source_house = detonation_source_house(world, detonation);
            let area = collect_area_at(
                world,
                rules,
                overlay_registry,
                detonation.impact,
                detonation.payload.area_damage(),
                warhead,
                true,
                (
                    detonation.source_id,
                    source_house,
                    detonation.payload.warhead,
                ),
                effects,
            );
            #[cfg(test)]
            append_fixture_tiberium(world, &mut out.effects.tiberium_reduction_requests);
            #[cfg(any(test, feature = "test-support"))]
            out.effects.wall_mutations.extend(area.aoe.wall_mutations);

            #[cfg(any(test, feature = "test-support"))]
            out.effects
                .cell_target_detaches
                .extend(area.aoe.cell_target_detaches);
            // Phobos `BulletClass_Detonate @ 0x46920B`: the warhead's
            // per-target pass (AttachEffect lists, crit) runs before the
            // native area damage, so a target it kills is skipped by the
            // dispatch gates below.
            #[cfg(feature = "phobos")]
            let crit_active = super::crit::detonate(
                world,
                run,
                rules,
                overlay_registry,
                warhead,
                detonation.payload.warhead,
                detonation.source_id,
                source_house,
                detonation.impact,
                match detonation.target {
                    ProjectileTarget::Entity(id) => Some(id),
                    _ => None,
                },
                &area.aoe.receivers,
                &mut out.effects,
            );
            #[cfg(not(feature = "phobos"))]
            let crit_active = false;
            out.damage_events.extend(area.aoe.receivers);

            (Some(area.continuation), crit_active)
        }
        claimed => {
            run_special_detonation_arm(
                world,
                rules,
                claimed,
                detonation,
                overlay_registry,
                effects,
            );
            (None, false)
        }
    }
}

/// The detonating bullet's Location (`+0x9C`) and OnBridge, which
/// SpawnShrapnel and the anim tail read. The impact ladder
/// (`ResolveImpactCoordAndDetonate @ 0x00468D80`) hands DetonateAtCoord a
/// resolved coordinate but never moves the bullet. A death weapon's bullet is
/// never placed: Fire_Death_Weapon creates it, Limbos it and detonates it at
/// its firer's GetCoords (`0x0070D756..0x0070D782`), so its Location is still
/// the constructor's zero coordinate (`0x005F3993` copies `0x00AC1380`, which
/// `0x005F38A0` zeroes), off any bridge. A detonation with no live bullet (a
/// test fixture) stands at its own coordinate.
fn bullet_location(
    world: &Simulation,
    detonation: &ProjectileDetonation,
) -> (ProjectileCoord, bool) {
    match world.projectiles.get(detonation.projectile_id) {
        Some(bullet) => (bullet.position(), bullet.on_bridge()),
        None if detonation.reason
            == crate::sim::projectile::ProjectileDetonationReason::DeathWeapon =>
        {
            (ProjectileCoord::new(0, 0, 0), false)
        }
        None => (detonation.impact, false),
    }
}

/// Bullet469D06..469EBA: one budget, then the typed VoxelAnim arm or the
/// MetallicDebris SHP fallback. Both finish constructors synchronously before
/// the outer cluster successor. Stock Smashing/TRexWH use the empty-list arm;
/// an authored Smashing TIRE list uses the same typed selector as Techno deaths.
fn emit_warhead_debris(
    world: &mut Simulation,
    rules: &RuleSet,
    detonation: &ProjectileDetonation,
    warhead: &WarheadType,
    out: &mut CombatEmit,
) {
    use crate::sim::voxel_anim::{
        ShpDebrisSource, debris_budget, throw_shp_debris, throw_typed_debris,
    };

    let budget = debris_budget(
        warhead.min_debris,
        warhead.max_debris,
        &mut world.scenario_rng,
    );
    if budget <= 0 {
        return;
    }
    let mut host = WarheadDebrisHost {
        world,
        rules,
        detonation: *detonation,
        anims: &mut out.effects.explosion_effects,
        voxels: &mut out.effects.voxel_debris,
    };
    let result = if warhead.debris_types.is_empty() {
        throw_shp_debris(
            budget,
            ShpDebrisSource::RulesMetallicDebris,
            rules.general.metallic_debris.len(),
            &mut host,
        )
    } else {
        let types: Vec<_> = warhead
            .debris_types
            .iter()
            .map(|name| {
                rules
                    .voxel_anim_type_id_by_name(name)
                    .map(|id| (id, rules.voxel_anim_type(id)))
            })
            .collect();
        throw_typed_debris(budget, &types, &warhead.debris_maximums, None, &mut host).map(|_| ())
    };
    if let Err(error) = result {
        // Preserve earlier synchronous births on unsupported authored launch
        // arithmetic, as the existing Techno debris entry does.
        log::debug!("warhead debris stopped outside verified numeric domain: {error}");
    }
}

struct WarheadDebrisHost<'w, 'r> {
    world: &'w mut Simulation,
    rules: &'r RuleSet,
    detonation: ProjectileDetonation,
    anims: &'w mut Vec<ExplosionEffect>,
    voxels: &'w mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
}

impl crate::sim::voxel_anim::ShpDebrisHost for WarheadDebrisHost<'_, '_> {
    fn rng(&mut self) -> &mut SimRng {
        &mut self.world.scenario_rng
    }

    fn construct_anim(
        &mut self,
        source: crate::sim::voxel_anim::ShpDebrisSource,
        index: usize,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        debug_assert_eq!(
            source,
            crate::sim::voxel_anim::ShpDebrisSource::RulesMetallicDebris
        );
        //469E50 reads live GetCoords per piece, independently of the
        // impact argument and Inviso-scattered explosion placement.
        let location = bullet_location(self.world, &self.detonation).0;
        let coord = crate::sim::anim_class::AnimWorldCoord {
            x: location.x,
            y: location.y,
            z: location.z.wrapping_add(20),
        };
        self.world.emit_debris_anim(
            self.rules,
            self.rules
                .general
                .metallic_debris
                .get(index)
                .map(String::as_str),
            coord,
            self.anims,
        )
    }
}

impl crate::sim::voxel_anim::VoxelDebrisHost for WarheadDebrisHost<'_, '_> {
    fn voxel_origin(&self) -> glam::IVec3 {
        // 469DC3..469DCF queries the live Bullet per constructor. Unlike
        // SHP469E50 it adds no 20; VoxelAnim74950C owns its own 10-lepton lift.
        let coord = bullet_location(self.world, &self.detonation).0;
        glam::IVec3::new(coord.x, coord.y, coord.z)
    }

    fn construct_voxel(
        &mut self,
        type_id: crate::rules::voxel_anim_type::VoxelAnimTypeId,
        voxel_type: &crate::rules::voxel_anim_type::VoxelAnimType,
        owner_house: Option<InternedId>,
        origin: glam::IVec3,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let admit_inline = callbacks_enabled(self.world);
        self.world.emit_debris_voxel(
            type_id,
            voxel_type,
            owner_house,
            origin,
            admit_inline,
            self.voxels,
        )
    }
}

/// `LAB_00469AA4`, reached by the ordinary arm and every special arm after
/// their receivers: an `Inviso=` bullet first scatters the anim coordinate
/// by one raw Scenario draw (`0x0049F420`, radius 0x20; the damage keeps the
/// impact), then the AnimList anim and its smudge.
fn emit_detonation_anim(
    world: &mut Simulation,
    rules: &RuleSet,
    detonation: &ProjectileDetonation,
    warhead: &WarheadType,
    inviso: bool,
    bright: bool,
    area_result: Option<AreaDamageResult>,
    crit_active: bool,
    out: &mut CombatEmit,
) {
    //469AA4 retains the raw coordinate before49F420; packing first would
    // change negative edges and coordinates beyond the signed cell word.
    // Phobos jumps over this scatter (`DEFINE_JUMP(LJMP, 0x469AC1,
    // 0x469AF0)`): the light and the Iron Curtain anim keep the raw impact,
    // and its `0x469C46` tail scatters each animation itself.
    let (x, y) = if inviso && !cfg!(feature = "phobos") {
        inviso_scatter::random_direction_coord(
            world.cell_grid(),
            &mut world.scenario_rng,
            detonation.impact.x,
            detonation.impact.y,
            0x20,
            inviso_scatter::RandomDirectionSnap::Preserve,
        )
    } else {
        (detonation.impact.x, detonation.impact.y)
    };
    let placement = ProjectileCoord::new(x, y, detonation.impact.z);
    // 469AF0..469BCF reads the still-live Bullet, not the damage/animation
    // coordinate copied at469AA4. The terrain here includes the synchronous
    // bridge continuation, so a collapsed deck can now select SplashList.
    let (selection_coordinate, on_bridge) = bullet_location(world, detonation);
    #[cfg(feature = "phobos")]
    let ignore_water_units = warhead
        .phobos_anim
        .conventional_ignore_units(&rules.phobos_warhead_anim);
    #[cfg(not(feature = "phobos"))]
    let ignore_water_units = false;
    let land = detonation_anim::bullet_land(
        world,
        rules,
        selection_coordinate,
        on_bridge,
        area_result == Some(AreaDamageResult::ReceiverDispatched),
        ignore_water_units,
    );
    // Selection still runs for an isolated Iron Curtain impact, including
    // EMEffect RNG. Only afterward469BEA replaces its constructor type.
    #[cfg(feature = "phobos")]
    let selection = detonation_anim::select_with_context(
        world,
        rules,
        warhead,
        detonation.payload.base_damage,
        land,
        selection_coordinate,
        crit_active,
    );
    #[cfg(not(feature = "phobos"))]
    let selected = detonation_anim::effect(
        world,
        rules,
        warhead,
        detonation.payload.base_damage,
        land,
        selection_coordinate,
        placement,
        crit_active,
    );
    // `0x00469BD6..0x00469C41`: a Bright bullet (`+0xE0`, the weapon's
    // `Bright=`) lights the anim coordinate with its damage (`+0x6C`),
    // force 1, and the warhead's CLDisable channels. AreaDamage result2
    // skips the light and selected ordinary animation after selection RNG.
    if bright && area_result != Some(AreaDamageResult::IronCurtain) {
        let flags = (u32::from(warhead.cl_disable_red) << 1)
            | (u32::from(warhead.cl_disable_green) << 2)
            | (u32::from(warhead.cl_disable_blue) << 3);
        out.effects
            .combat_light_requests
            .push(CombatLightRequest::Impact {
                target_id: None,
                damage: detonation.payload.base_damage,
                warhead_ref: detonation.payload.warhead,
                coord: placement,
                force_create: true,
                flags,
            });
    }
    #[cfg(feature = "phobos")]
    {
        if area_result == Some(AreaDamageResult::IronCurtain) {
            if !rules.general.weapon_nullify_anim.is_empty() {
                let type_id = world.interner.intern(&rules.general.weapon_nullify_anim);
                // 46A2A1..46A301 uses the same placement and constructor row
                // as the ordinary impact, then returns before the ordinary tail.
                let effect = detonation_anim::placed_effect(type_id, placement);
                crate::sim::world::damage_consequences::admit_explosion_effect(
                    world, rules, effect,
                );
            }
        } else if let Some(selection) = selection {
            let invoker_house = detonation_source_house(world, detonation);
            for effect in detonation_anim::bullet_effects(
                world,
                rules,
                warhead,
                detonation.payload.warhead,
                selection,
                inviso,
                placement,
                detonation.source_id,
                crit_active,
                invoker_house,
            ) {
                crate::sim::world::damage_consequences::admit_explosion_effect(
                    world, rules, effect,
                );
            }
        }
    }
    #[cfg(not(feature = "phobos"))]
    {
        let effect = if area_result == Some(AreaDamageResult::IronCurtain) {
            (!rules.general.weapon_nullify_anim.is_empty()).then(|| {
                let type_id = world.interner.intern(&rules.general.weapon_nullify_anim);
                // 46A2A1..46A301 uses the same placement and constructor row as
                // the ordinary impact, then returns before the ordinary tail.
                detonation_anim::placed_effect(type_id, placement)
            })
        } else {
            selected
        };
        if let Some(effect) = effect {
            crate::sim::world::damage_consequences::admit_explosion_effect(world, rules, effect);
        }
    }
}

/// One bullet's detonation, `BulletClass::DetonateAtCoord` then its cluster
/// loop (`0x00469020..0x00469091`): per cluster the receivers commit, the anim
/// tail runs, and every cluster (the last included) draws its successor
/// around the impact, `RandomRanged(0x100, 0x200)` plus one raw draw. An
/// `Airburst=` bullet detonates once and draws nothing, as does a death
/// weapon's bare `DetonateAtCoord` (`ProjectileDetonationReason::DeathWeapon`)
/// and (VERA-internal, never in retail) a weapon that names no BulletType.
///
/// The selected animation constructs synchronously inside the tail, so its
/// common identity and constructor RNG precede the next cluster's draws.
pub(crate) fn commit_projectile_detonations_inline(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    projectile_detonations: &[ProjectileDetonation],
    emit: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    for request in projectile_detonations {
        let current = world.projectiles.current_detonation(*request);
        let detonation = &current;
        let Some(warhead) = rules.warhead(world.interner.resolve(detonation.payload.warhead))
        else {
            log::warn!(
                "Projectile {} dropped: missing serialized warhead {}",
                detonation.projectile_id,
                detonation.payload.warhead
            );
            continue;
        };
        let projectile_type = detonation
            .payload
            .projectile_type()
            .and_then(|projectile| rules.projectile(world.interner.resolve(projectile)));
        // A death weapon and a Phobos instant AttachEffect shot both skip the
        // BulletClass cluster loop and its draws.
        let single_detonation = matches!(
            detonation.reason,
            crate::sim::projectile::ProjectileDetonationReason::DeathWeapon
                | crate::sim::projectile::ProjectileDetonationReason::AttachEffectShot
        );
        // Phobos `BulletClass_Explodes_AirburstCheck1/2` make `Splits` take
        // the airburst arm: one detonation and no outer cluster draws.
        let phobos_splits = cfg!(feature = "phobos") && projectile_type.is_some_and(|p| p.splits());
        let (clusters, cluster_draws) = match projectile_type {
            Some(projectile) if !projectile.airburst && !phobos_splits && !single_detonation => {
                (projectile.cluster.max(0), true)
            }
            _ => (1, false),
        };
        let mut coordinate = detonation.impact;
        // Production impacts retain their Bullet until this transaction ends.
        // Bare death weapons and explicit component fixtures may supply only
        // the temporary Bullet packet; do not invent a second alive flag.
        let represented_bullet = world.projectiles.get(detonation.projectile_id).is_some();
        for _ in 0..clusters {
            // BulletClass::DetonateAtCoord prologue @0x004690B0 reads each
            // axis pair before its special/damage chain (0x004690CE..0x0046912B).
            // The native tests either bound for nonzero, then calls the shared
            // RandomRanged @0x0065C7E0 on g_MainRng @0x00886B88, X before Y.
            let shake_x = (warhead.shake_x_lo != 0 || warhead.shake_x_hi != 0).then(|| {
                world
                    .main_rng
                    .next_range_i32_inclusive(warhead.shake_x_lo, warhead.shake_x_hi)
            });
            let shake_y = (warhead.shake_y_lo != 0 || warhead.shake_y_hi != 0).then(|| {
                world
                    .main_rng
                    .next_range_i32_inclusive(warhead.shake_y_lo, warhead.shake_y_hi)
            });
            if shake_x.is_some() || shake_y.is_some() {
                world
                    .screen_shake_updates
                    .emit(crate::sim::combat::ScreenShakeUpdate {
                        x: shake_x,
                        y: shake_y,
                    });
            }
            let clustered = world.projectiles.current_detonation(ProjectileDetonation {
                impact: coordinate,
                ..*detonation
            });
            let damage_start = emit.damage_events.len();
            let explosion_start = emit.effects.explosion_effects.len();
            let smudge_start = emit.effects.smudge_spawn_requests.len();
            let (area_continuation, crit_active) = emit_detonation_receivers(
                world,
                run,
                rules,
                overlay_registry,
                &clustered,
                warhead,
                emit,
            );
            let outer_explosion_effects = emit.effects.explosion_effects.split_off(explosion_start);
            let outer_anim_requests = emit.effects.smudge_spawn_requests.split_off(smudge_start);
            let (inline_death, mut pings, area_result) = commit_area_with_dispatch(
                world,
                run,
                &emit.damage_events[damage_start..],
                rules,
                overlay_registry,
            );
            emit.effects.append(inline_death);
            if let Some(context) = area_continuation {
                let (tail, mut tail_pings) =
                    continue_area_damage(world, run, rules, overlay_registry, context, area_result);
                emit.effects.append(tail);
                pings.append(&mut tail_pings);
            }
            emit.effects
                .explosion_effects
                .extend(outer_explosion_effects);
            under_attack_events.append(&mut pings);
            //469A88..469A94: ordinary AreaDamage may synchronously remove
            // its Bullet. Return before Inviso/selection/debris; the outer
            //469038 alive gate also skips all remaining cluster draws.
            if area_continuation.is_some()
                && represented_bullet
                && (world.projectiles.get(detonation.projectile_id).is_none()
                    || world
                        .substrate
                        .pending_delete
                        .contains_object(&detonation.projectile_id))
            {
                commit_smudges(
                    world,
                    rules,
                    overlay_registry,
                    outer_anim_requests,
                    &mut emit.effects.smudge_spawn_requests,
                );
                break;
            }
            let anim_start = emit.effects.smudge_spawn_requests.len();
            let live_tail = world.projectiles.current_detonation(clustered);
            let inviso = live_tail
                .payload
                .projectile_type()
                .and_then(|id| rules.projectile(world.interner.resolve(id)))
                .is_some_and(|kind| kind.inviso);
            emit_detonation_anim(
                world,
                rules,
                &live_tail,
                warhead,
                inviso,
                live_tail.payload.bright(),
                area_continuation.map(|_| area_result),
                crit_active,
                emit,
            );
            let mut anim_requests = outer_anim_requests;
            anim_requests.extend(emit.effects.smudge_spawn_requests.split_off(anim_start));
            commit_smudges(
                world,
                rules,
                overlay_registry,
                anim_requests,
                &mut emit.effects.smudge_spawn_requests,
            );
            //46A2A1..46A2FB returns immediately after the nullify Anim.
            // The ordinary outer cluster successor still follows that return.
            if area_continuation.is_none() || area_result != AreaDamageResult::IronCurtain {
                emit_warhead_debris(world, rules, &live_tail, warhead, emit);
                #[cfg(feature = "phobos")]
                super::phobos_airburst::emit(world, rules, &live_tail);
                #[cfg(not(feature = "phobos"))]
                super::airburst::emit(world, rules, &live_tail);
            }
            if cluster_draws {
                coordinate = projectile_next_cluster_coord(
                    world.cell_grid(),
                    detonation.impact,
                    &mut world.scenario_rng,
                );
            }
        }
    }
}

pub(crate) fn commit_projectiles(
    world: &mut Simulation,
    run: &mut ReceiverRun,
    detonations: &[ProjectileDetonation],
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> LogicProjectileCommit {
    let mut emit = CombatEmit::default();
    let mut under_attack_events = Vec::new();
    commit_projectile_detonations_inline(
        world,
        run,
        rules,
        overlay_registry,
        detonations,
        &mut emit,
        &mut under_attack_events,
    );

    debug_assert!(emit.remove_attack.is_empty());
    debug_assert!(emit.fire_events.is_empty());
    debug_assert!(emit.unit_facing.is_empty());
    debug_assert!(emit.spawn_target_updates.is_empty());

    LogicProjectileCommit {
        effects: emit.effects,
        under_attack_events,
    }
}

#[inline]
pub(super) fn callbacks_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if _world.receiver_fixture.is_some() {
        return false;
    }
    true
}

#[inline]
fn receiver_tick(world: &Simulation) -> u64 {
    #[cfg(test)]
    if let Some(fixture) = world.receiver_fixture.as_ref() {
        return fixture.current_tick;
    }
    u64::from(world.session.binary_frame())
}

/// One `StartUncloaking(0)` for a damage receiver: `TechnoClass::ReceiveDamage
/// @ 0x0070281D`'s virtual `+0xFC` (`0x00703850`) and Phobos
/// `ShieldClass::ReceiveDamage` (`ShieldClass.cpp:249..255`) share it.
/// Production combat callers derive `current_tick` from
/// `ScenarioSession::binary_frame`, the clock `CloakingTick` compares against.
/// The native call passes the sound-suppression argument zero, so an accepted
/// transition owns one positional `[AudioVisual] CloakSound`.
fn start_uncloaking_for_damage(
    world: &mut Simulation,
    target_id: u64,
    rules: &RuleSet,
    current_tick: u64,
    sound_enabled: bool,
) {
    let now = current_tick as i32;
    let cloaking_speed = world
        .substrate
        .entities
        .get(target_id)
        .and_then(|target| world.interner.object_type(target.type_ref(), rules))
        .map_or(1, |object| object.cloaking_speed);
    let surfaced = world
        .substrate
        .entities
        .get_mut(target_id)
        .and_then(|target| target.cloak.as_mut())
        .map(|cloak| {
            cloak.start_uncloaking(now, cloaking_speed, rules.general.cloaking_stages, false)
        });
    if surfaced.is_some_and(|result| result.play_sound) && sound_enabled {
        emit_configured_cloak_sound(&world.substrate, &mut world.sound_events, target_id, rules);
    }
}

#[inline]
pub(super) fn sound_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_ref() {
        return fixture.sound_enabled;
    }
    true
}

#[inline]
pub(super) fn fog_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_ref() {
        return fixture.fog_enabled;
    }
    true
}

#[inline]
pub(super) fn radiation_enabled(_world: &Simulation) -> bool {
    #[cfg(test)]
    if let Some(fixture) = _world.receiver_fixture.as_ref() {
        return fixture.radiation_enabled;
    }
    true
}

/// Phobos `BulletExt::FirerHouse` fallback: the House a bullet retained at
/// construction stands in for an Owner Techno that expired before this round
/// detonated (`WarheadType/Hooks.cpp:17`).
fn detonation_source_house(
    world: &Simulation,
    detonation: &ProjectileDetonation,
) -> Option<InternedId> {
    world
        .substrate
        .entities
        .get(detonation.source_id)
        .map(|source| source.owner())
        .or_else(|| {
            #[cfg(feature = "phobos")]
            {
                world
                    .projectiles
                    .get(detonation.projectile_id)
                    .filter(|projectile| projectile.source_id() == detonation.source_id)
                    .and_then(|projectile| projectile.phobos_firer_house())
            }
            #[cfg(not(feature = "phobos"))]
            {
                None
            }
        })
}

#[cfg(test)]
fn append_fixture_tiberium(world: &mut Simulation, out: &mut Vec<TiberiumReductionRequest>) {
    if let Some(fixture) = world.receiver_fixture.as_mut() {
        out.append(&mut fixture.deferred_tiberium);
    }
}

#[cfg(test)]
#[path = "ifv_area_receipt_tests.rs"]
mod ifv_area_receipt_tests;

#[cfg(test)]
#[path = "nuke_maker_tests.rs"]
mod nuke_maker_tests;

#[cfg(all(test, feature = "phobos"))]
#[path = "phobos_bullet_init_tests.rs"]
mod phobos_bullet_init_tests;

fn warhead_damages_wall(
    warhead: &WarheadType,
    wall_flags: &crate::rules::overlay_types::OverlayTypeFlags,
) -> bool {
    warhead.wall || warhead.wall_absolute_destroyer || (warhead.wood && wall_flags.armor_is_wood)
}
