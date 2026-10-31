//! Shared `SelectAnim @ 0x0048A4F0`, after the caller's land/height decision,
//! and where the chosen animation is placed: the impact's cell
//! ([`projectile_impact_cell`]) and its height byte ([`impact_z_byte`]).
//!
//! Native comparisons: tools/projectile_oracle/ifv_select_anim.{py,json,md}.
//! Selection coordinates and constructor coordinates are separate: Bullet
//! 469BCF supplies its live ObjectClass position even after bridge damage has
//! changed the terrain beneath the copied detonation coordinate.

use super::{ExplosionEffect, ProjectileCoord, RuleSet, WarheadType};
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback_leptons};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

fn cell_at(world: &Simulation, coord: ProjectileCoord) -> CellRef<'_> {
    if world.resolved_terrain.is_some() {
        get_cellclass_fallback_leptons(world.resolved_terrain.as_ref(), coord.x, coord.y)
    } else {
        let cell = world.effective_shared_cell_dummy();
        cell.stamp_coord(coord.x / 256, coord.y / 256);
        CellRef::Dummy { cell }
    }
}

pub(crate) fn land_at(world: &Simulation, coord: ProjectileCoord) -> i32 {
    match cell_at(world, coord) {
        CellRef::Real(cell) => i32::from(cell.land_type),
        CellRef::Dummy { cell } => cell.land_type(),
    }
}

/// Object::GetHeight and the first-ground-object gate at Bullet469AF0..469BCF.
pub(crate) fn bullet_land(
    world: &Simulation,
    rules: &RuleSet,
    coordinate: ProjectileCoord,
    on_bridge: bool,
    receiver_dispatched: bool,
    ignore_units: bool,
) -> i32 {
    let floor = crate::sim::projectile::projectile_ground_z(
        world.resolved_terrain.as_ref(),
        &world.effective_shared_cell_dummy(),
        coordinate,
    );
    let height = coordinate.z.wrapping_sub(floor).wrapping_sub(if on_bridge {
        crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
    } else {
        0
    });
    if height >= 2 * crate::util::lepton::LEPTONS_PER_LEVEL as i32 {
        return -1;
    }
    let land = land_at(world, coordinate);
    if land == 2 && receiver_dispatched && !ignore_units {
        // 469B45..469B87 reads exactly the ground list's first object, not
        // any naval object in the cell and not the bridge deck's list.
        let first = match cell_at(world, coordinate) {
            CellRef::Real(cell) => world
                .substrate
                .occupancy
                .cell_objects(
                    cell.rx,
                    cell.ry,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    world
                        .terrain_objects
                        .cell_index()
                        .get(&(cell.rx, cell.ry))
                        .copied(),
                )
                .next(),
            CellRef::Dummy { .. } => None,
        };
        if let Some(crate::sim::occupancy::CellObjectMember::Entity(id)) = first
            && let Some(unit) = world.substrate.entities.get(id)
            && unit.category() == crate::map::entities::EntityCategory::Unit
            && world
                .interner
                .object_type(unit.type_ref(), rules)
                .is_some_and(|kind| kind.naval && !kind.underwater)
        {
            return -1;
        }
    }
    land
}

fn damage_band<'a>(list: &'a [impl AsRef<str>], damage: i32, step: i32) -> Option<&'a str> {
    // Native uses signed division after an upper clamp. Negative indices read
    // before the vector allocation; Rust deterministically rejects that native
    // invalid-memory case rather than inventing an animation or an RNG draw.
    let last = i32::try_from(list.len())
        .ok()?
        .checked_mul(step)?
        .checked_sub(1)?;
    let index = usize::try_from(damage.min(last) / step).ok()?;
    list.get(index).map(AsRef::as_ref)
}

/// The selected AnimList/SplashList entry and whether the water splash list
/// supplied it. The Bullet tail needs `splashed` to choose the interval,
/// scatter and CreateAll settings Phobos reads in its late hook.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SelectedAnim {
    pub name: InternedId,
    /// Only the Phobos Bullet tail reads this.
    #[cfg_attr(not(feature = "phobos"), allow(dead_code))]
    pub splashed: bool,
}

pub(crate) fn select(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    damage: i32,
    land: i32,
    coordinate: ProjectileCoord,
    crit_active: bool,
) -> Option<InternedId> {
    select_with_context(world, rules, warhead, damage, land, coordinate, crit_active)
        .map(|selection| selection.name)
}

pub(crate) fn select_with_context(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    damage: i32,
    land: i32,
    coordinate: ProjectileCoord,
    crit_active: bool,
) -> Option<SelectedAnim> {
    // Phobos `WarheadTypeExt::CreateAnimsOnZeroDamage` (`0x48A4F3`): a zero
    // packet normally returns no animation; the key (or its `[General]`
    // fallback) lets selection continue with damage 0.
    #[cfg(feature = "phobos")]
    let create_zero_damage = warhead
        .phobos_anim
        .create_anims_on_zero_damage(&rules.phobos_warhead_anim);
    #[cfg(not(feature = "phobos"))]
    let create_zero_damage = false;
    if damage == 0 && !create_zero_damage {
        return None;
    }
    // Phobos `SelectDamageAnimation_NegativeZeroDamage` (`0x48A4F3`): a
    // negative packet selects by its magnitude.
    #[cfg(feature = "phobos")]
    let damage = if damage < 0 {
        damage.wrapping_neg()
    } else {
        damage
    };
    // 48A50D..48A592: water's Conventional branch precedes the weather
    // override, and the cell's structural flag suppresses splash selection.
    if land == 2
        && warhead.conventional
        && cell_at(world, coordinate).bridge_flags_0x1180()
            & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
            == 0
    {
        let floor = crate::sim::projectile::projectile_ground_z(
            world.resolved_terrain.as_ref(),
            &world.effective_shared_cell_dummy(),
            coordinate,
        );
        if coordinate.z < floor.wrapping_add(2 * crate::util::lepton::LEPTONS_PER_LEVEL as i32) {
            // Phobos `WarheadTypeClass_AnimList_SplashList` (`0x48A551`): a
            // present per-warhead list replaces the global list; a present
            // empty list stays empty and selects nothing.
            #[cfg(feature = "phobos")]
            let splash_names = warhead.phobos_anim.splash_list_names(rules);
            #[cfg(not(feature = "phobos"))]
            let splash_names: Vec<&str> = rules
                .combat_damage
                .splash_list
                .iter()
                .map(String::as_str)
                .collect();
            #[cfg(feature = "phobos")]
            let pick_random = warhead.phobos_anim.splash_list_pick_random;
            #[cfg(not(feature = "phobos"))]
            let pick_random = false;
            // Phobos evaluates the PickRandom range before its `size() > 0`
            // check, so an empty list still consumes the native
            // `RandomRanged(0, -1)` (one swapped-bounds draw).
            let random_index = pick_random.then(|| {
                world.scenario_rng.next_range_i32_inclusive(
                    0,
                    i32::try_from(splash_names.len()).expect("native SplashList count fits i32")
                        - 1,
                )
            });
            if splash_names.is_empty() {
                return None;
            }
            let name = match random_index {
                Some(index) => splash_names[index as usize],
                None => damage_band(&splash_names, damage, 35)?,
            };
            return Some(SelectedAnim {
                name: world.interner.intern(name),
                splashed: true,
            });
        }
    }
    // 48A594..48A5A6 compares the retained LightningWarhead pointer, then
    // returns WeatherConBoltExplosion (including its null default).
    if !rules.general.lightning_warhead.is_empty() && warhead.id == rules.general.lightning_warhead
    {
        if rules.general.weather_con_bolt_explosion.is_empty() {
            return None;
        }
        return Some(SelectedAnim {
            name: world
                .interner
                .intern(&rules.general.weather_con_bolt_explosion),
            splashed: false,
        });
    }
    // Phobos `SelectDamageAnimation_CritAnim` (`0x48A5B3`, ahead of the
    // AnimList count load and empty test at `0x48A5BB`): an active crit
    // replaces the damage animation with a `Crit.AnimList` entry while that
    // list stays off the affected targets, even when `AnimList=` is empty.
    #[cfg(feature = "phobos")]
    if crit_active
        && !warhead.crit.anim_list().is_empty()
        && !warhead.crit.anim_on_affected_targets(rules.crit_globals())
    {
        let list = warhead.crit.anim_list();
        // `Crit.AnimList.PickRandom.Get(AnimList.PickRandom)`.
        let crit_pick_random = warhead
            .crit
            .anim_list_pick_random
            .unwrap_or(warhead.phobos_anim.anim_list_pick_random);
        let index = if warhead.em_effect || crit_pick_random {
            world.scenario_rng.next_range_i32_inclusive(
                0,
                i32::try_from(list.len() - 1).expect("native AnimList count fits i32"),
            ) as usize
        } else {
            // `std::min(size * 25 - 1, (size_t)nDamage) / 25`: the unsigned
            // cast sends a negative packet to the last entry.
            (list.len() * 25 - 1).min(damage as u32 as usize) / 25
        };
        return Some(SelectedAnim {
            name: world.interner.intern(&list[index]),
            splashed: false,
        });
    }
    #[cfg(not(feature = "phobos"))]
    let _ = crit_active;
    if warhead.anim_list.is_empty() {
        return None;
    }
    #[cfg(feature = "phobos")]
    let pick_random = warhead.em_effect || warhead.phobos_anim.anim_list_pick_random;
    #[cfg(not(feature = "phobos"))]
    let pick_random = warhead.em_effect;
    let name = if pick_random {
        let index = world.scenario_rng.next_range_i32_inclusive(
            0,
            i32::try_from(warhead.anim_list.len() - 1).expect("native AnimList count fits i32"),
        );
        &warhead.anim_list[index as usize]
    } else {
        damage_band(&warhead.anim_list, damage, 25)?
    };
    Some(SelectedAnim {
        name: world.interner.intern(name),
        splashed: false,
    })
}

/// Build the selected animation's sole delivery packet. The caller decides
/// when to construct it relative to its own damage and RNG operations.
pub(crate) fn effect(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    damage: i32,
    land: i32,
    selection_coordinate: ProjectileCoord,
    placement: ProjectileCoord,
    crit_active: bool,
) -> Option<ExplosionEffect> {
    let shp_name = select(
        world,
        rules,
        warhead,
        damage,
        land,
        selection_coordinate,
        crit_active,
    )?;
    Some(placed_effect(shp_name, placement))
}

/// `AnimExt::SetAnimOwnerHouseKind(pAnim, pHouse, nullptr, false, true)` for
/// this tail: a `MakeInfantry=` type uses `MakeInfantryOwner`, a
/// `CreateUnit=` type overrides it with `CreateUnit.Owner`, and anything else
/// keeps the invoker House. `Random` draws one House in native array order.
#[cfg(feature = "phobos")]
fn anim_owner_house(
    world: &mut Simulation,
    rules: &RuleSet,
    type_id: InternedId,
    invoker: Option<InternedId>,
) -> Option<InternedId> {
    use crate::rules::ares_warhead::HouseSelector;
    let (make_owner, create_unit) = {
        let name = world.interner.resolve(type_id);
        rules
            .art()
            .anim_runtime_config(name)
            .map_or((None, None), |config| {
                (
                    (config.make_infantry > -1).then_some(config.phobos_make_infantry_owner),
                    config.phobos_create_unit,
                )
            })
    };
    let kind = create_unit
        .map(|(owner, _)| owner)
        .or(make_owner)
        .unwrap_or(HouseSelector::Default);
    let random_house = if kind == HouseSelector::Random {
        let order = world.houses.order();
        if order.is_empty() {
            None
        } else {
            let index = world
                .scenario_rng
                .next_range_i32_inclusive(0, order.len() as i32 - 1)
                as usize;
            order.get(index).copied()
        }
    } else {
        None
    };
    world.house_kind(kind, invoker, invoker, invoker, None, rules, random_house)
}

/// Phobos' `BulletClass_Logics_DamageAnimSelected` (`0x469C46`) late tail:
/// place the animation against the cell floor, expand CreateAll lists, gate
/// on the shared creation interval, and scatter each constructed animation
/// independently.
#[cfg(feature = "phobos")]
pub(crate) fn bullet_effects(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    selection: SelectedAnim,
    inviso: bool,
    placement: ProjectileCoord,
    source_id: u64,
    crit_active: bool,
    invoker_house: Option<InternedId>,
) -> Vec<ExplosionEffect> {
    // `MapClass::GetCellFloorHeight 0x00578080` runs before the interval
    // queue. `PlayAnimAboveSurface` lifts the animation to the floor;
    // `PlayAnimUnderground=no` then drops it when the floor is still above
    // the result. Each constructed animation keeps `AnimZAdjust`.
    let cell_height = crate::sim::projectile::projectile_ground_z(
        world.resolved_terrain.as_ref(),
        &world.effective_shared_cell_dummy(),
        placement,
    );
    let base_z = if warhead.phobos_anim.play_anim_above_surface {
        placement.z.max(cell_height)
    } else {
        placement.z
    };
    if cell_height > base_z && !warhead.phobos_anim.play_anim_underground {
        return Vec::new();
    }
    let placement = ProjectileCoord::new(placement.x, placement.y, base_z);
    let z_adjust = warhead
        .phobos_anim
        .anim_z_adjust(&rules.phobos_warhead_anim);
    // Phobos' tail assigns `pOwner ? pOwner->Owner : BulletExt::FirerHouse`
    // to each constructed animation; the caller supplies that house.

    let interval = if selection.splashed {
        warhead.phobos_anim.splash_list_creation_interval
    } else {
        warhead.phobos_anim.anim_list_creation_interval
    };
    // Phobos uses the Techno's shared counter only for a positive interval
    // with a live owner; otherwise the warhead type owns it. The gate runs
    // before any list expansion so a skipped detonation interns nothing.
    let allowed = if interval > 0
        && let Some(source) = world.substrate.entities.get_mut(source_id)
    {
        source
            .wh_anim_remaining_creation_interval
            .interval_allows(interval)
    } else {
        world
            .warhead_anim_runtime
            .interval_allows(warhead_ref, interval)
    };
    if !allowed {
        return Vec::new();
    }

    // Phobos replaces its single selected type only when the relevant
    // CreateAll key is on; an expansion whose list is empty then creates
    // nothing (e.g. the lightning override with an empty AnimList) instead of
    // falling back to the selected type.
    let create_all_types = if selection.splashed {
        warhead.phobos_anim.splash_list_create_all.then(|| {
            warhead
                .phobos_anim
                .splash_list_names(rules)
                .into_iter()
                .map(|name| world.interner.intern(name))
                .collect::<Vec<_>>()
        })
    } else {
        let crit_replaces = crit_active
            && !warhead.crit.anim_list().is_empty()
            && !warhead.crit.anim_on_affected_targets(rules.crit_globals());
        if crit_replaces {
            warhead
                .crit
                .anim_list_create_all
                .unwrap_or(warhead.phobos_anim.anim_list_create_all)
                .then(|| {
                    warhead
                        .crit
                        .anim_list()
                        .iter()
                        .map(|name| world.interner.intern(name))
                        .collect::<Vec<_>>()
                })
        } else {
            warhead.phobos_anim.anim_list_create_all.then(|| {
                warhead
                    .anim_list
                    .iter()
                    .map(|name| world.interner.intern(name))
                    .collect::<Vec<_>>()
            })
        }
    };
    let types = create_all_types.unwrap_or_else(|| vec![selection.name]);

    let (scatter_min, scatter_max) = if selection.splashed {
        (
            warhead.phobos_anim.splash_list_scatter_min,
            warhead.phobos_anim.splash_list_scatter_max,
        )
    } else {
        (
            warhead.phobos_anim.anim_list_scatter_min,
            warhead.phobos_anim.anim_list_scatter_max,
        )
    };
    let allow_scatter = scatter_max >= 0 || scatter_min >= 0 || inviso;
    let mut effects = Vec::with_capacity(types.len());
    for type_id in types {
        let coordinate = if allow_scatter {
            let distance = if scatter_max >= 0 || scatter_min >= 0 {
                world
                    .scenario_rng
                    .next_range_i32_inclusive(scatter_min, scatter_max)
            } else {
                32
            };
            let (x, y) = super::inviso_scatter::random_direction_coord(
                world.cell_grid(),
                &mut world.scenario_rng,
                placement.x,
                placement.y,
                distance,
                super::inviso_scatter::RandomDirectionSnap::Preserve,
            );
            ProjectileCoord::new(x, y, placement.z)
        } else {
            placement
        };
        let mut effect = placed_effect_with_z_adjust(type_id, coordinate, z_adjust);
        effect.owner_house = anim_owner_house(world, rules, type_id, invoker_house);
        effects.push(effect);
    }
    effects
}

pub(crate) fn placed_effect(shp_name: InternedId, placement: ProjectileCoord) -> ExplosionEffect {
    placed_effect_with_z_adjust(
        shp_name,
        placement,
        crate::sim::anim_class::COMBAT_EXPLOSION_Z_ADJUST,
    )
}

pub(crate) fn placed_effect_with_z_adjust(
    shp_name: InternedId,
    placement: ProjectileCoord,
    z_adjust: i32,
) -> ExplosionEffect {
    let (rx, ry, sub_x, sub_y, world_z) = projectile_impact_cell(placement);
    ExplosionEffect {
        shp_name,
        rx,
        ry,
        sub_x,
        sub_y,
        z: impact_z_byte(world_z.div_euclid(crate::util::lepton::LEPTONS_PER_LEVEL as i32)),
        world_z,
        z_adjust,
        owner_house: None,
        death: None,
    }
}

/// Narrow an impact z into the byte the presentation path carries it in.
///
/// The projection that turns that byte into a pixel decodes it with `as i8`
/// (`util::lepton::lepton_to_screen`), so the byte is a *signed* level count
/// and the only correct saturation is into `i8` range: clamping into `u8`
/// range instead would let 200 through, which decodes as -56 levels and throws
/// the sprite most of a screen away.
///
/// Known mismatch, outside this file: the sprite depth key reads the same byte
/// as unsigned. The two readings agree over 0..=127, which covers every map
/// height, so it is latent — but a negative impact z would sort by one rule
/// and draw by the other.
pub(crate) fn impact_z_byte(impact_z: i32) -> u8 {
    impact_z.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8 as u8
}

pub(crate) fn projectile_impact_cell(
    impact: ProjectileCoord,
) -> (u16, u16, SimFixed, SimFixed, i32) {
    let (rx, sub_x) = crate::util::lepton::split_lepton_axis(impact.x);
    let (ry, sub_y) = crate::util::lepton::split_lepton_axis(impact.y);
    (rx, ry, sub_x, sub_y, impact.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::SimRng;
    use serde_json::Value;

    #[test]
    fn projectile_impact_cell_preserves_signed_native_cell_and_subcell() {
        // AreaDamage489309..489330 truncates each signed Coord component
        // toward zero and stores AX. Keep the signed remainder so its exact
        // distance/layer point survives that packed-cell representation.
        for (point, expected_cell, expected_subcell) in [
            ((-257, -511), (-1_i16, -1_i16), (-1, -255)),
            ((-1, -255), (0, 0), (-1, -255)),
            ((-128_128, 5_248), (-500, 20), (-128, 128)),
            ((2_694, 5_253), (10, 20), (134, 133)),
        ] {
            let (rx, ry, sub_x, sub_y, z) =
                projectile_impact_cell(ProjectileCoord::new(point.0, point.1, 209));
            assert_eq!((rx as i16, ry as i16), expected_cell, "{point:?}");
            assert_eq!(
                (sub_x.to_num::<i32>(), sub_y.to_num::<i32>()),
                expected_subcell,
                "{point:?}"
            );
            assert_eq!(
                (
                    i32::from(rx as i16) * 256 + sub_x.to_num::<i32>(),
                    i32::from(ry as i16) * 256 + sub_y.to_num::<i32>()
                ),
                point
            );
            assert_eq!(z, 209);
        }
    }

    #[test]
    fn dummy_land_survives_misses_and_resets_at_native_resize() {
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_select_anim.json",
        ))
        .unwrap();
        let mut world = Simulation::new();
        let dummy = world.effective_shared_cell_dummy();
        let rows = native["dummy_land_controls"].as_array().unwrap();
        assert_eq!(dummy.land_type(), rows[0]["land"].as_i64().unwrap() as i32);
        let clean_hash = world.state_hash();
        for row in &rows[1..4] {
            dummy.test_set_land_type(row["supplied_land"].as_i64().unwrap() as i32);
            assert_eq!(
                land_at(&world, ProjectileCoord::new(1_000_000, 0, 0)),
                row["land"].as_i64().unwrap() as i32
            );
            assert_eq!(serde_json::json!(dummy.snapshot().coord), row["coord"]);
            assert_eq!(dummy.fork_query_identity().land_type(), dummy.land_type());
            let prepared = crate::map::resolved_terrain::SharedCellDummy::fresh();
            prepared.adopt_prepared_load_state(&dummy);
            assert_eq!(prepared.land_type(), dummy.land_type());
        }
        assert_ne!(
            world.state_hash(),
            clean_hash,
            "live fallback land affects subsequent impact choice"
        );
        world.reconstruct_cellclass_dummy_for_map_resize();
        assert_eq!(dummy.land_type(), rows[4]["land"].as_i64().unwrap() as i32);
        assert_eq!(world.state_hash(), clean_hash);
    }

    #[test]
    fn first_ground_occupant_and_area_receipt_match_native_naval_selection() {
        use crate::map::entities::EntityCategory;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::{CellListInsertion, CellObjectMember};

        let Some((base, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_select_anim.json",
        ))
        .unwrap();
        let rows = native["rows"].as_array().unwrap();
        let controls: Vec<_> = rows
            .iter()
            .filter(|row| row["input"]["building"] == true)
            .collect();
        assert_eq!(controls.len(), 5);
        for row in controls {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let naval = row["unit_naval"].as_bool().unwrap();
            let underwater = row["unit_underwater"].as_bool().unwrap();
            let mut layers = RulesLayerStack::new(base.clone());
            // Same yes/no overrides as the original FV UnitType reader in
            // the corpus. The legacy input label "building" means an object
            // was supplied; only nonbuilding=true selects its Building vtable.
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&format!(
                    "[FV]\nNaval={}\nUnderwater={}\n",
                    if naval { "yes" } else { "no" },
                    if underwater { "yes" } else { "no" },
                )),
            );
            let processed = layers.process_with_fixed_art(&art).unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            let kind = rules.object("FV").unwrap();
            assert_eq!((kind.naval, kind.underwater), (naval, underwater));
            let mut world = Simulation::with_seed(31);
            for id in ["Americans", "FV", "DEST"] {
                crate::sim::intern::test_intern(id);
            }
            world.interner = crate::sim::intern::test_interner();
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
            let cell = terrain.cell_mut_for_test(10, 20).unwrap();
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.land_type = input["land"].as_i64().unwrap() as u8;
            cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
            world.install_resolved_terrain_for_new_map(terrain);
            let mut entity = GameEntity::test_default(1, "FV", "Americans", 10, 20);
            entity.set_category_for_test(if input["nonbuilding"] == true {
                EntityCategory::Structure
            } else {
                EntityCategory::Unit
            });
            let insertion = CellListInsertion::from_category(entity.category());
            world.substrate.entities.insert(entity);
            // Original fixture supplies Cell+E4 and the object/type pointer;
            // this uses the production retained-list owner at that boundary.
            // It does not claim native Unlimbo/Mark or placement admission.
            world
                .substrate
                .occupancy
                .add(10, 20, 1, MovementLayer::Ground, None, insertion);
            assert_eq!(
                world
                    .substrate
                    .occupancy
                    .cell_objects(10, 20, MovementLayer::Ground, None)
                    .next(),
                Some(CellObjectMember::Entity(1))
            );
            let coordinate = ProjectileCoord::new(2688, 5248, input["z"].as_i64().unwrap() as i32);
            let on_bridge = input["on_bridge"].as_bool().unwrap();
            let dispatched = input["direct"] == 0;
            let land = bullet_land(&world, &rules, coordinate, on_bridge, dispatched, false);
            assert_eq!(
                land,
                row["passed"]["land"].as_i64().unwrap() as i32,
                "{name}"
            );
            let before = world.scenario_rng.logical_state();
            let selected = select(
                &mut world,
                &rules,
                rules.warhead("HE").unwrap(),
                25,
                land,
                coordinate,
                false,
            );
            assert_eq!(
                selected.map(|id| world.interner.resolve(id)),
                row["selected"].as_str(),
                "{name}"
            );
            assert_eq!(
                world.scenario_rng.logical_state(),
                before,
                "{name} native RNG unchanged"
            );
            assert_eq!(row["rng_unchanged"], true);

            // Additional list-owner invariants from469B45..469B87, not new
            // native executable rows: neither a later object nor deck list
            // membership may replace the supplied first Ground object.
            if !naval {
                let naval_kind = rules.object("DEST").unwrap();
                assert!(naval_kind.naval && !naval_kind.underwater);
                world.substrate.entities.insert(GameEntity::test_default(
                    2,
                    "DEST",
                    "Americans",
                    10,
                    20,
                ));
                world.substrate.occupancy.add(
                    10,
                    20,
                    2,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
                // Reinsert the original nonnaval head in front of the new unit.
                world.substrate.occupancy.remove(10, 20, 1);
                world
                    .substrate
                    .occupancy
                    .add(10, 20, 1, MovementLayer::Ground, None, insertion);
                assert_eq!(
                    bullet_land(&world, &rules, coordinate, on_bridge, dispatched, false),
                    land,
                    "later naval member"
                );
                world.substrate.occupancy.remove(10, 20, 2);
                world.substrate.occupancy.add(
                    10,
                    20,
                    2,
                    MovementLayer::Bridge,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
                assert_eq!(
                    bullet_land(&world, &rules, coordinate, on_bridge, dispatched, false),
                    land,
                    "naval deck member"
                );
                for height in [207, 208, 209] {
                    let height_name = format!("caller_flags0_height{height}");
                    let expected = rows
                        .iter()
                        .find(|r| r["input"]["name"] == height_name)
                        .unwrap();
                    let probe = ProjectileCoord::new(
                        2688,
                        5248,
                        expected["input"]["z"].as_i64().unwrap() as i32,
                    );
                    assert_eq!(
                        bullet_land(&world, &rules, probe, false, true, false),
                        expected["passed"]["land"].as_i64().unwrap() as i32,
                        "{height_name} with nonnaval head"
                    );
                }
            }
        }
    }

    #[test]
    fn selection_and_bullet_height_match_original_executed_controls() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_select_anim.json",
        ))
        .unwrap();
        assert_eq!(native["rows"].as_array().unwrap().len(), 48);
        for row in native["rows"].as_array().unwrap() {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut world = Simulation::with_seed(31);
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
            let cell = terrain.cell_mut_for_test(10, 20).unwrap();
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.land_type = input["land"].as_i64().unwrap() as u8;
            cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
            world.install_resolved_terrain_for_new_map(terrain);
            let coordinate = ProjectileCoord::new(2688, 5248, input["z"].as_i64().unwrap() as i32);
            // Unit occupancy/type bytes are independently supplied in the
            // caller corpus. Here selection consumes the actual passed land;
            // the plain height rows also exercise our production caller probe.
            let land = row["passed"]["land"].as_i64().unwrap() as i32;
            if input["route"] == "caller"
                && !input.get("unit").is_some_and(|v| v == true)
                && !name.contains("unit")
            {
                assert_eq!(
                    bullet_land(
                        &world,
                        &rules,
                        coordinate,
                        input["on_bridge"].as_bool().unwrap(),
                        input["direct"] == 0,
                        false
                    ),
                    land,
                    "{name} caller land"
                );
            }
            let mut warhead = rules.warhead("HE").unwrap().clone();
            // Reader controls are tested by select_anim_inputs; these bytes
            // are the original retained inputs to this executable comparison.
            warhead.conventional = row["conventional"].as_bool().unwrap();
            warhead.em_effect = row["em_effect"].as_bool().unwrap();
            let actual = if input["null_warhead"] == true {
                None
            } else {
                select(
                    &mut world,
                    &rules,
                    &warhead,
                    input["damage"].as_i64().unwrap() as i32,
                    land,
                    coordinate,
                    false,
                )
            };
            assert_eq!(
                actual.map(|id| world.interner.resolve(id)),
                row["selected"].as_str(),
                "{name}"
            );
            let hex = world.scenario_rng.native_state_hex();
            let bytes: Vec<u8> = hex
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|digits| u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(
                crate::util::sha256::sha256_hex(&bytes),
                row["rng_after_sha256"].as_str().unwrap(),
                "{name} full RNG"
            );
            assert_eq!(
                world.scenario_rng.state() == SimRng::new(31).state(),
                row["rng_unchanged"].as_bool().unwrap(),
                "{name}"
            );
        }
    }
}

#[cfg(all(test, feature = "phobos"))]
mod phobos_enhancement_tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellListInsertion;

    fn animation_rules(keys: &str, splash_global: &str, general: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\n{general}\n\
             [CombatDamage]\nSplashList={splash_global}\n\
             [Warheads]\n0=WH\n\
             [Animations]\n0=A\n1=B\n2=C\n3=D\n4=G1\n5=G2\n\
             [WH]\nAnimList=A,B\nConventional=yes\n{keys}\n"
        ));
        RuleSet::from_ini(&ini).unwrap()
    }

    fn name(world: &Simulation, id: InternedId) -> String {
        world.interner.resolve(id).to_owned()
    }

    fn effect_abs(effect: &ExplosionEffect) -> (i32, i32) {
        (
            i32::from(effect.rx as i16) * 256 + effect.sub_x.to_num::<i32>(),
            i32::from(effect.ry as i16) * 256 + effect.sub_y.to_num::<i32>(),
        )
    }

    #[test]
    fn zero_damage_pick_random_and_create_all() {
        let plain = animation_rules("", "", "");
        let mut world = Simulation::with_seed(7);
        assert!(
            select_with_context(
                &mut world,
                &plain,
                plain.warhead("WH").unwrap(),
                0,
                0,
                ProjectileCoord::new(0, 0, 0),
                false,
            )
            .is_none(),
            "zero damage is silent without CreateAnimsOnZeroDamage"
        );

        let zero = animation_rules("CreateAnimsOnZeroDamage=yes\n", "", "");
        let mut world = Simulation::with_seed(7);
        let selected = select_with_context(
            &mut world,
            &zero,
            zero.warhead("WH").unwrap(),
            0,
            0,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .expect("zero damage creates the first AnimList entry");
        assert_eq!(name(&world, selected.name), "A");

        let random = animation_rules(
            "CreateAnimsOnZeroDamage=yes\nAnimList.PickRandom=yes\n",
            "",
            "",
        );
        let mut world = Simulation::with_seed(7);
        let before = world.scenario_rng.native_state_hex();
        let selected = select_with_context(
            &mut world,
            &random,
            random.warhead("WH").unwrap(),
            0,
            0,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .unwrap();
        assert_ne!(world.scenario_rng.native_state_hex(), before);
        assert!(matches!(name(&world, selected.name).as_str(), "A" | "B"));

        let all = animation_rules("AnimList.CreateAll=yes\n", "", "");
        let mut world = Simulation::with_seed(7);
        let selection = select_with_context(
            &mut world,
            &all,
            all.warhead("WH").unwrap(),
            50,
            0,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .unwrap();
        let warhead_ref = world.interner.intern("WH");
        let effects = bullet_effects(
            &mut world,
            &all,
            all.warhead("WH").unwrap(),
            warhead_ref,
            selection,
            false,
            ProjectileCoord::new(2_560, 5_120, 0),
            0,
            false,
            None,
        );
        assert_eq!(
            effects
                .iter()
                .map(|effect| name(&world, effect.shp_name))
                .collect::<Vec<_>>(),
            vec!["A", "B"]
        );
    }

    #[test]
    fn splash_list_overrides_global_and_create_all() {
        let global = animation_rules("", "G1,G2", "");
        let mut world = Simulation::with_seed(7);
        let selected = select_with_context(
            &mut world,
            &global,
            global.warhead("WH").unwrap(),
            50,
            2,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .unwrap();
        assert!(selected.splashed);
        assert_eq!(name(&world, selected.name), "G2");

        let override_list =
            animation_rules("SplashList=C,D\nSplashList.CreateAll=yes\n", "G1,G2", "");
        let mut world = Simulation::with_seed(7);
        let selected = select_with_context(
            &mut world,
            &override_list,
            override_list.warhead("WH").unwrap(),
            50,
            2,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .unwrap();
        let warhead_ref = world.interner.intern("WH");
        let effects = bullet_effects(
            &mut world,
            &override_list,
            override_list.warhead("WH").unwrap(),
            warhead_ref,
            selected,
            false,
            ProjectileCoord::new(2_560, 5_120, 0),
            0,
            false,
            None,
        );
        assert_eq!(
            effects
                .iter()
                .map(|effect| name(&world, effect.shp_name))
                .collect::<Vec<_>>(),
            vec!["C", "D"]
        );
    }

    #[test]
    fn splash_list_pick_random_consumes_a_draw() {
        let deterministic = animation_rules("SplashList=C,D\n", "G1,G2", "");
        let random = animation_rules("SplashList=C,D\nSplashList.PickRandom=yes\n", "G1,G2", "");
        let mut first = Simulation::with_seed(7);
        let selected = select_with_context(
            &mut first,
            &deterministic,
            deterministic.warhead("WH").unwrap(),
            50,
            2,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .unwrap();
        let before = first.scenario_rng.native_state_hex();
        assert_eq!(name(&first, selected.name), "D");
        assert_eq!(first.scenario_rng.native_state_hex(), before);

        let mut second = Simulation::with_seed(7);
        let before = second.scenario_rng.native_state_hex();
        let selected = select_with_context(
            &mut second,
            &random,
            random.warhead("WH").unwrap(),
            50,
            2,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .unwrap();
        assert_ne!(second.scenario_rng.native_state_hex(), before);
        assert!(matches!(name(&second, selected.name).as_str(), "C" | "D"));

        // `SplashList=none` is a present list that resolves to nothing;
        // Phobos evaluates `RandomRanged(0, -1)` before its `size() > 0`
        // check, so the native stream still advances.
        let empty = animation_rules("SplashList=none\nSplashList.PickRandom=yes\n", "G1,G2", "");
        let mut third = Simulation::with_seed(7);
        let before = third.scenario_rng.native_state_hex();
        assert!(
            select_with_context(
                &mut third,
                &empty,
                empty.warhead("WH").unwrap(),
                50,
                2,
                ProjectileCoord::new(0, 0, 0),
                false,
            )
            .is_none(),
            "a present list that resolves to no animation selects nothing"
        );
        assert_ne!(
            third.scenario_rng.native_state_hex(),
            before,
            "native RandomRanged(0, -1) still consumes a draw"
        );
    }

    #[test]
    fn splash_interval_and_scatter_use_the_splash_keys() {
        let rules = animation_rules(
            "SplashList=C,D\nSplashList.CreationInterval=2\n\
             SplashList.ScatterMin=1\nSplashList.ScatterMax=1\n",
            "",
            "",
        );
        let mut world = Simulation::with_seed(7);
        let placement = ProjectileCoord::new(2_560, 5_120, 0);
        let select = |world: &mut Simulation| {
            select_with_context(
                world,
                &rules,
                rules.warhead("WH").unwrap(),
                50,
                2,
                placement,
                false,
            )
            .unwrap()
        };
        let warhead_ref = world.interner.intern("WH");
        let first = select(&mut world);
        assert!(first.splashed);
        let effects = bullet_effects(
            &mut world,
            &rules,
            rules.warhead("WH").unwrap(),
            warhead_ref,
            first,
            false,
            placement,
            0,
            false,
            None,
        );
        assert_eq!(effects.len(), 1);
        let (x, y) = effect_abs(&effects[0]);
        let distance = f64::from((x - placement.x).pow(2) + (y - placement.y).pow(2)).sqrt();
        assert!((250.0..=260.0).contains(&distance), "{distance}");
        let second = select(&mut world);
        assert!(
            bullet_effects(
                &mut world,
                &rules,
                rules.warhead("WH").unwrap(),
                warhead_ref,
                second,
                false,
                placement,
                0,
                false,
                None,
            )
            .is_empty(),
            "the shared counter suppresses the next splash detonation"
        );
    }

    #[test]
    fn play_anim_floor_gate_and_z_adjust_follow_the_warhead() {
        let below_floor = ProjectileCoord::new(2_560, 5_120, -100);
        let run = |keys: &str, general: &str| {
            let rules = animation_rules(keys, "", general);
            let mut world = Simulation::with_seed(7);
            let warhead = rules.warhead("WH").unwrap();
            let selection =
                select_with_context(&mut world, &rules, warhead, 50, 0, below_floor, false)
                    .unwrap();
            let warhead_ref = world.interner.intern("WH");
            let effects = bullet_effects(
                &mut world,
                &rules,
                warhead,
                warhead_ref,
                selection,
                false,
                below_floor,
                0,
                false,
                None,
            );
            (world, effects)
        };

        let (_, effects) = run("", "");
        assert_eq!(effects.len(), 1, "PlayAnimUnderground defaults to yes");
        assert_eq!(effects[0].world_z, -100);
        assert_eq!(
            effects[0].z_adjust,
            crate::sim::anim_class::COMBAT_EXPLOSION_Z_ADJUST
        );

        let (_, effects) = run("PlayAnimUnderground=no\n", "");
        assert!(
            effects.is_empty(),
            "PlayAnimUnderground=no drops a below-floor animation"
        );

        let (_, effects) = run("PlayAnimAboveSurface=yes\n", "");
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0].world_z, 0,
            "PlayAnimAboveSurface lifts the animation to the cell floor"
        );

        let (world, effects) = run("AnimZAdjust=7\n", "");
        assert_eq!(effects[0].z_adjust, 7);
        assert_eq!(world.interner.resolve(effects[0].shp_name), "B");
    }

    #[test]
    fn anim_z_adjust_inherits_the_audio_visual_global() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[AudioVisual]\nWarheadAnimZAdjust=-9\n\
             [Warheads]\n0=WH\n[WH]\nAnimList=A\n\
             [Animations]\n0=A\n",
        ))
        .unwrap();
        let mut world = Simulation::with_seed(7);
        let warhead = rules.warhead("WH").unwrap();
        let placement = ProjectileCoord::new(2_560, 5_120, 0);
        let selection =
            select_with_context(&mut world, &rules, warhead, 50, 0, placement, false).unwrap();
        let warhead_ref = world.interner.intern("WH");
        let effects = bullet_effects(
            &mut world,
            &rules,
            warhead,
            warhead_ref,
            selection,
            false,
            placement,
            0,
            false,
            None,
        );
        assert_eq!(effects[0].z_adjust, -9);
    }

    #[test]
    fn bullet_effects_assign_the_invoker_house() {
        let rules = animation_rules("", "", "");
        let mut world = Simulation::with_seed(7);
        world
            .substrate
            .entities
            .insert(GameEntity::test_default(1, "E1", "Americans", 5, 5));
        let warhead = rules.warhead("WH").unwrap();
        let placement = ProjectileCoord::new(2_560, 5_120, 0);
        let selection =
            select_with_context(&mut world, &rules, warhead, 50, 0, placement, false).unwrap();
        let warhead_ref = world.interner.intern("WH");
        let invoker = world.substrate.entities.get(1).unwrap().owner();
        let effects = bullet_effects(
            &mut world,
            &rules,
            warhead,
            warhead_ref,
            selection,
            false,
            placement,
            1,
            false,
            Some(invoker),
        );
        assert_eq!(
            effects[0].owner_house,
            Some(invoker),
            "pAnim->Owner = pInvoker"
        );
    }

    #[test]
    fn make_infantry_and_create_unit_types_take_their_owner_kinds() {
        use crate::sim::anim_class::AnimRemap;
        let rules_text = "[Countries]
0=Americans
1=Neutral
2=Special
             [Americans]
[Neutral]
[Special]
             [Sides]
Allied=Americans
Civilian=Neutral
             [General]
DamageFireTypes=
             [AudioVisual]
ConditionYellow=50%
ConditionRed=25%
             [Animations]
0=A
1=B
             [CombatDamage]
             [Warheads]
0=WH
             [WH]
AnimList=A,B
AnimList.CreateAll=yes
Conventional=yes
";
        let art_text = "[A]
MakeInfantry=0
MakeInfantryOwner=civilian
             [B]
CreateUnit=MTNK
CreateUnit.Owner=special
";
        let art_ini = IniFile::from_str(art_text);
        let mut rules =
            RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(rules_text), &art_ini)
                .unwrap();
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("A", 1);
        art.bind_anim_frame_count_for_test("B", 1);
        rules.install_art_fixture(art);

        let mut world = Simulation::with_seed(7);
        let register = |world: &mut Simulation, rules: &RuleSet, name: &str| {
            let side = crate::sim::house_state::resolve_house_side_index(
                rules,
                Some(name),
                Some("Civilian"),
                0,
            );
            let id = world.interner.intern(name);
            let country = world.interner.intern(name);
            world
                .houses
                .register(crate::sim::house_state::HouseState::new(
                    id,
                    side,
                    Some(country),
                    false,
                    0,
                    10,
                ));
            id
        };
        let _americans = register(&mut world, &rules, "Americans");
        let neutral = register(&mut world, &rules, "Neutral");
        let special = register(&mut world, &rules, "Special");
        world
            .substrate
            .entities
            .insert(GameEntity::test_default(1, "E1", "Americans", 5, 5));

        let warhead = rules.warhead("WH").unwrap();
        let placement = ProjectileCoord::new(2_560, 5_120, 0);
        let selection =
            select_with_context(&mut world, &rules, warhead, 50, 0, placement, false).unwrap();
        let warhead_ref = world.interner.intern("WH");
        let invoker_house = world.interner.intern("Americans");
        let effects = bullet_effects(
            &mut world,
            &rules,
            warhead,
            warhead_ref,
            selection,
            false,
            placement,
            1,
            false,
            Some(invoker_house),
        );
        assert_eq!(effects.len(), 2, "CreateAll expands both list entries");
        assert_eq!(
            effects[0].owner_house,
            Some(neutral),
            "MakeInfantryOwner=civilian overrides the invoker House"
        );
        assert_eq!(
            effects[1].owner_house,
            Some(special),
            "CreateUnit.Owner=special overrides MakeInfantryOwner"
        );

        // SetAnimOwnerHouseKind's LightConvert gate: MakeInfantry types always
        // remap; CreateUnit types only with CreateUnit.RemapAnim.
        let spawn = |world: &mut Simulation, effect: &ExplosionEffect| {
            world
                .spawn_combat_explosion_anim_with_z_adjust(
                    &rules,
                    effect.shp_name,
                    effect.rx,
                    effect.ry,
                    effect.sub_x,
                    effect.sub_y,
                    effect.z,
                    effect.world_z,
                    effect.z_adjust,
                    effect.owner_house,
                )
                .unwrap()
        };
        let make_infantry = spawn(&mut world, &effects[0]);
        assert_eq!(
            world.anim(make_infantry).unwrap().remap(),
            Some(AnimRemap::House(neutral))
        );
        let create_unit = spawn(&mut world, &effects[1]);
        assert_eq!(
            world.anim(create_unit).unwrap().remap(),
            None,
            "CreateUnit.RemapAnim defaults to no"
        );
    }

    #[test]
    fn create_all_with_an_empty_anim_list_creates_nothing() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nLightningWarhead=WH\nWeatherConBoltExplosion=LIGHT\n\
             [Warheads]\n0=WH\n[WH]\nAnimList.CreateAll=yes\n\
             [Animations]\n0=LIGHT\n",
        ))
        .unwrap();
        let mut world = Simulation::with_seed(7);
        let selected = select_with_context(
            &mut world,
            &rules,
            rules.warhead("WH").unwrap(),
            10,
            0,
            ProjectileCoord::new(0, 0, 0),
            false,
        )
        .unwrap();
        assert_eq!(name(&world, selected.name), "LIGHT");
        let warhead_ref = world.interner.intern("WH");
        assert!(
            bullet_effects(
                &mut world,
                &rules,
                rules.warhead("WH").unwrap(),
                warhead_ref,
                selected,
                false,
                ProjectileCoord::new(0, 0, 0),
                0,
                false,
                None,
            )
            .is_empty(),
            "Phobos replaces the selected type with the empty AnimList"
        );
    }

    #[test]
    fn anim_scatter_uses_explicit_cells_and_inviso_default() {
        let scattered = animation_rules("AnimList.ScatterMin=1\nAnimList.ScatterMax=1\n", "", "");
        let mut world = Simulation::with_seed(7);
        let placement = ProjectileCoord::new(2_560, 5_120, 0);
        let selected = select_with_context(
            &mut world,
            &scattered,
            scattered.warhead("WH").unwrap(),
            50,
            0,
            placement,
            false,
        )
        .unwrap();
        let warhead_ref = world.interner.intern("WH");
        let effects = bullet_effects(
            &mut world,
            &scattered,
            scattered.warhead("WH").unwrap(),
            warhead_ref,
            selected,
            false,
            placement,
            0,
            false,
            None,
        );
        let (x, y) = effect_abs(&effects[0]);
        let distance = f64::from((x - placement.x).pow(2) + (y - placement.y).pow(2)).sqrt();
        assert!((250.0..=260.0).contains(&distance), "{distance}");

        let default = animation_rules("", "", "");
        let mut world = Simulation::with_seed(7);
        let selected = select_with_context(
            &mut world,
            &default,
            default.warhead("WH").unwrap(),
            50,
            0,
            placement,
            false,
        )
        .unwrap();
        let warhead_ref = world.interner.intern("WH");
        let effects = bullet_effects(
            &mut world,
            &default,
            default.warhead("WH").unwrap(),
            warhead_ref,
            selected,
            true,
            placement,
            0,
            false,
            None,
        );
        let (x, y) = effect_abs(&effects[0]);
        let distance = f64::from((x - placement.x).pow(2) + (y - placement.y).pow(2)).sqrt();
        assert!((28.0..=36.0).contains(&distance), "{distance}");
    }

    #[test]
    fn creation_interval_is_shared_per_warhead_and_per_techno() {
        let rules = animation_rules("AnimList.CreationInterval=2\n", "", "");
        let warhead = rules.warhead("WH").unwrap();
        let mut world = Simulation::with_seed(7);
        let selection = SelectedAnim {
            name: world.interner.intern("A"),
            splashed: false,
        };
        let warhead_ref = world.interner.intern("WH");
        let mut counts = Vec::new();
        for _ in 0..4 {
            counts.push(
                bullet_effects(
                    &mut world,
                    &rules,
                    warhead,
                    warhead_ref,
                    selection,
                    false,
                    ProjectileCoord::new(2_560, 5_120, 0),
                    0,
                    false,
                    None,
                )
                .len(),
            );
        }
        assert_eq!(counts, vec![1, 0, 0, 1], "per-warhead counter");

        let mut world = Simulation::with_seed(7);
        world
            .substrate
            .entities
            .insert(GameEntity::test_default(1, "E1", "Americans", 5, 5));
        let warhead_ref = world.interner.intern("WH");
        let mut counts = Vec::new();
        for _ in 0..4 {
            counts.push(
                bullet_effects(
                    &mut world,
                    &rules,
                    warhead,
                    warhead_ref,
                    selection,
                    false,
                    ProjectileCoord::new(2_560, 5_120, 0),
                    1,
                    false,
                    None,
                )
                .len(),
            );
        }
        assert_eq!(counts, vec![1, 0, 0, 1], "per-Techno counter");
        assert_eq!(
            world
                .substrate
                .entities
                .get(1)
                .unwrap()
                .wh_anim_remaining_creation_interval
                .remaining(),
            2
        );
    }

    #[test]
    fn conventional_ignore_units_keeps_the_water_land_type() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=SHIP\n[SHIP]\nNaval=yes\nUnderwater=no\n\
             [CombatDamage]\nSplashList=G\n\
             [Warheads]\n0=WH\n[WH]\nConventional=yes\nAnimList=A\n\
             [Animations]\n0=A\n1=G\n",
        ))
        .unwrap();
        let mut world = Simulation::with_seed(7);
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(16);
        terrain.cell_mut_for_test(5, 5).unwrap().land_type = 2;
        world.install_resolved_terrain_for_new_map(terrain);
        let mut entity = GameEntity::test_default(1, "SHIP", "Americans", 5, 5);
        entity.set_type_ref_for_test(world.interner.intern("SHIP"));
        entity.set_category_for_test(EntityCategory::Unit);
        world.substrate.entities.insert(entity);
        world.substrate.occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let point = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0);
        let warhead = rules.warhead("WH").unwrap();
        let over_unit = bullet_land(&world, &rules, point, false, true, false);
        let ignored = bullet_land(&world, &rules, point, false, true, true);
        assert_eq!(over_unit, -1);
        assert_eq!(ignored, 2);
        let anim = select_with_context(&mut world, &rules, warhead, 10, over_unit, point, false)
            .expect("the land type keeps the AnimList branch");
        assert!(!anim.splashed);
        assert_eq!(name(&world, anim.name), "A");
        let splash = select_with_context(&mut world, &rules, warhead, 10, ignored, point, false)
            .expect("IgnoreUnits selects the water splash list");
        assert!(splash.splashed);
        assert_eq!(name(&world, splash.name), "G");
    }
}
