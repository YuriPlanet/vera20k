//! The crate effect arms of `CellClass::PickupCrate @ 0x00481A00` other than
//! Speed ([`super::speed`]): the radius multipliers (Armor `0x00482D56`,
//! Firepower `0x00483125`), Veteran `0x00482972`, HealBase `0x00482B8F`,
//! Money `0x00482463`, Reveal `0x00481F9D`, Darkness `0x00481F6D` and the
//! free Unit `0x00482041`. [`super::pickup`] selects the arm and plays the
//! arm's sound; each arm here reproduces the gameplay writes.
//!
//! The radius arms walk the Ground display layer (`0x008A0394`, the vector
//! the Speed arm's native comparison supplied), measure `Sqrt_Approx` of the
//! squared lepton deltas against `[CrateRules] CrateRadius=` with a strict
//! `<` (`ftol` then `CMP ... JGE`), and skip an object whose multiplier is
//! not exactly 1.0, so no object stacks the same bonus twice.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::Simulation;
use crate::sim::world::display_layers::DisplayLayer;
use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits, distance_3d_leptons};

/// Which `Techno` double a multiplier crate raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Multiplier {
    /// `Techno+0x158`, the Armor arm `0x00482E53..0x00482E79`.
    Armor,
    /// `Techno+0x160`, the Firepower arm `0x00483222..0x00483248`.
    Firepower,
}

/// Ground-layer objects within `radius` of `center`, in layer order, with the
/// native `Sqrt_Approx`/`ftol` distance and strict `<` (`0x00482E38..0x00482E51`).
fn ground_members_in_radius(sim: &Simulation, center: DriveCoord, radius: i32) -> Vec<u64> {
    sim.substrate
        .display
        .members(DisplayLayer::GROUND)
        .iter()
        .copied()
        .filter(|&id| {
            sim.substrate.entities.get(id).is_some_and(|entity| {
                let position = position_world_coord(&entity.position);
                distance_3d_leptons(
                    [center.x, center.y, center.z],
                    [position.x, position.y, position.z],
                ) < radius
            })
        })
        .collect()
}

/// The Armor and Firepower arms: every Techno within the radius whose
/// multiplier is exactly 1.0 takes `multiplier * magnitude` (x87 double
/// multiply, `0x00482E6D..0x00482E79`). Returns the owners of the changed
/// objects, in native visit order; the arm announces when one of them is the
/// local player (`vt+0x3C` then `0x0050B6F0`, `0x00482E7F..0x00482E8D`).
pub(super) fn apply_multiplier_crate(
    sim: &mut Simulation,
    center: DriveCoord,
    radius: i32,
    magnitude: NativeF64Bits,
    which: Multiplier,
) -> Vec<InternedId> {
    let mut changed_owners = Vec::new();
    for id in ground_members_in_radius(sim, center, radius) {
        let Some(entity) = sim.substrate.entities.get_mut(id) else {
            continue;
        };
        let slot = match which {
            Multiplier::Armor => &mut entity.armor_multiplier,
            Multiplier::Firepower => &mut entity.firepower_multiplier,
        };
        if *slot != NativeF64Bits::ONE {
            continue;
        }
        *slot =
            X87::store_f64_masked_chop(X87::mul(X87::load_f64(magnitude), X87::load_f64(*slot)));
        let owner = entity.owner();
        if !changed_owners.contains(&owner) {
            changed_owners.push(owner);
        }
    }
    changed_owners
}

/// `FCOMP a, b` followed by `TEST AH, 1`: C0 set only for an ordered `a < b`.
fn ordered_less(a: f64, b: f64) -> bool {
    a.partial_cmp(&b) == Some(std::cmp::Ordering::Less)
}

/// The Veteran arm `0x00482986..0x00482B19`: every marked Techno
/// (`Object+0x74`) within the radius whose type is `Trainable=` climbs one
/// rank per whole unit of `magnitude` while `0.0 < magnitude`
/// (`0x00482A99..0x00482B00`): a veteran becomes elite, a rookie a veteran, a
/// negative accumulator a rookie, each test on the live value in that order.
/// No owner test: allied and enemy objects in the radius are promoted too.
/// The promotion anim and EVA follow from the next `AI_Update` sample, as for
/// a kill promotion.
pub(super) fn apply_veteran_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    center: DriveCoord,
    radius: i32,
    magnitude: NativeF64Bits,
) {
    let magnitude = f64::from_bits(magnitude.bits());
    // `FCOMP` then `TEST AH,1` (`0x00482A99..0x00482AAA`): the C0 "less than"
    // bit alone, so a NaN magnitude promotes nobody.
    if !ordered_less(0.0, magnitude) {
        return;
    }
    for id in ground_members_in_radius(sim, center, radius) {
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        // `Object+0x74`: ObjectClass::Mark sets it on PUT and clears it on
        // REMOVE (`0x005F58F7`, `jumpjet_cruise` `0x0054D0FF..0x0054D12C`),
        // which is the cell-membership byte `lifecycle.cell_marked` owns.
        if !entity.lifecycle.cell_marked {
            continue;
        }
        if !sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.trainable)
        {
            continue;
        }
        let entity = sim.substrate.entities.get_mut(id).expect("checked above");
        let mut level = 0_i32;
        loop {
            if veterancy::raw_is_veteran(entity.veterancy_raw) {
                veterancy::set_elite(entity);
            }
            if veterancy::raw_is_rookie(entity.veterancy_raw) {
                veterancy::set_veteran(entity);
            }
            if veterancy::raw_is_negative(entity.veterancy_raw) {
                veterancy::set_rookie(entity);
            }
            level += 1;
            if !ordered_less(f64::from(level), magnitude) {
                break;
            }
        }
    }
}

/// The HealBase arm's building loop `0x00482C18..0x00482C9C`: every building
/// of the actor's house receives `Health - Type.Strength` (zero or negative)
/// through `ReceiveDamage(&amount, 0, C4Warhead=, NULL, 1, 1, NULL)`, in
/// `BuildingClass::Array` order (construction order here).
pub(super) fn apply_heal_base_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    owner: InternedId,
) {
    let buildings: Vec<(u64, i32)> = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(_, entity)| {
            entity.category == EntityCategory::Structure && entity.owner() == owner
        })
        .filter_map(|(id, entity)| {
            let strength = sim.object_type(entity.type_ref(), rules)?.strength;
            Some((id, entity.health.current.wrapping_sub(strength)))
        })
        .collect();
    let warhead = sim.interner.intern(&rules.bridge_warheads.c4_name);
    for (id, amount) in buildings {
        if !sim.substrate.entities.contains(id) {
            continue;
        }
        sim.commit_direct_damage_receiver(
            rules,
            registry,
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                amount,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
        );
    }
}

/// The Money arm's amount `0x00482479..0x004824A5`: the solo override when
/// set, otherwise `RandomRanged(ftol(magnitude), ftol(magnitude) + 900)` on
/// the Scenario RNG. `HouseClass::Add_Credits @ 0x004F9950` then adds it.
pub(super) fn money_crate_amount(
    sim: &mut Simulation,
    solo_money: i32,
    magnitude: NativeF64Bits,
) -> i32 {
    if solo_money != 0 {
        return solo_money;
    }
    let base =
        crate::util::native_x87::X87Chop53::ftol_f64_low_masked(f64::from_bits(magnitude.bits()));
    sim.scenario_rng
        .next_range_i32_inclusive(base, base.wrapping_add(900))
}

/// What the Unit arm `0x00482041..0x00482461` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnitCrateOutcome {
    /// The vehicle stands on the map; the pickup returns false
    /// (`0x00482449`) without the anim tail.
    Placed,
    /// `CreateObject` returned null or no type was chosen: the anim tail with
    /// the Unit slot (`0x004832F5`).
    NoObject,
    /// Both Unlimbo attempts failed: the object is deleted (`vt+0x20(1)`)
    /// and the Money arm runs with slot 0 (`0x00482454..0x00482461`).
    FellBackToMoney,
}

/// The Unit arm's type choice `0x00482055..0x004821F7`. `preempted` is the
/// MCV pre-empt flag; `bases` the lobby Bases byte (`0x00A8B258`).
///
/// A pre-empted pickup asks for the first ownable `BaseUnit=`. Otherwise, a
/// house with one of the first two `[AI] BuildConst=` types tracked and
/// neither of the first two `HarvesterUnit=` types tracked gets its first
/// ownable `HarvesterUnit=`. `[CrateRules] UnitCrateType=` overrides both.
/// With no type yet, draw `RandomRanged(0, UnitTypes.Count - 1)` until a
/// `CrateGoodie=` type passes: with Bases on, a `BaseUnit=` type is accepted
/// for a human house or a pre-empted pickup; with Bases off, never.
///
/// gamemd's retry at `0x00482166..0x004821E9` never terminates when no type
/// passes all of those gates. This returns `None` without a draw instead
/// (a rules-data-only divergence), including when every goodie is a forbidden
/// base unit. When a candidate exists, retain the original full-array draws.
pub(super) fn choose_unit_crate_type<'r>(
    sim: &mut Simulation,
    rules: &'r RuleSet,
    owner: InternedId,
    preempted: bool,
) -> Option<&'r crate::rules::object_type::ObjectType> {
    use crate::sim::ai_buildable::{first_owner_compatible, house_country_bit};
    let house = sim.houses.get(&owner)?;
    let country_bit = house_country_bit(rules, sim.interner.resolve(house.house_type_id()));
    let mut chosen = None;
    if preempted {
        chosen = first_owner_compatible(
            rules,
            &rules.general.base_unit_types,
            ObjectCategory::Vehicle,
            country_bit,
        );
    }
    if chosen.is_none() {
        let tracked = |category: EntityCategory, type_id: &str| -> i32 {
            sim.interner
                .get(type_id)
                .map_or(0, |type_id| house.tracking.owned_count(category, type_id))
        };
        let has_conyard = rules
            .build_const_types
            .iter()
            .take(2)
            .any(|type_id| tracked(EntityCategory::Structure, type_id) > 0);
        let has_harvester = rules
            .harvester_unit_types
            .iter()
            .take(2)
            .any(|type_id| tracked(EntityCategory::Unit, type_id) != 0);
        if has_conyard && !has_harvester {
            chosen = first_owner_compatible(
                rules,
                &rules.harvester_unit_types,
                ObjectCategory::Vehicle,
                country_bit,
            );
        }
    }
    if let Some(named) = rules
        .crate_rules
        .unit_crate_type
        .as_deref()
        .and_then(|name| rules.object_in_category(ObjectCategory::Vehicle, name))
    {
        chosen = Some(named);
    }
    if chosen.is_some() {
        return chosen;
    }
    let human = house.is_controlled_by_human(sim.session.game_mode_nonzero);
    let bases = sim.session.game_options.bases;
    let ids = rules.type_array_ids(ObjectCategory::Vehicle);
    let eligible = |candidate: &crate::rules::object_type::ObjectType| {
        let is_base_unit = rules
            .general
            .base_unit_types
            .iter()
            .any(|base| base.eq_ignore_ascii_case(&candidate.id));
        candidate.crate_goodie && (!is_base_unit || (bases && (human || preempted)))
    };
    let any_eligible = ids.iter().any(|id| {
        rules
            .object_in_category(ObjectCategory::Vehicle, id)
            .is_some_and(eligible)
    });
    if !any_eligible {
        return None;
    }
    let count = i32::try_from(ids.len()).unwrap_or(i32::MAX);
    loop {
        let index = sim
            .scenario_rng
            .next_range_i32_inclusive(0, count.wrapping_sub(1));
        let Some(candidate) = usize::try_from(index)
            .ok()
            .and_then(|index| ids.get(index))
            .and_then(|id| rules.object_in_category(ObjectCategory::Vehicle, id))
        else {
            continue;
        };
        if eligible(candidate) {
            return Some(candidate);
        }
    }
}

/// The Unit arm's construction and placement `0x004821FA..0x00482461`:
/// `CreateObject(house)`, `Unlimbo` at the crate cell's centre on its ground
/// (`0x0047B3A0`), else at the nearest passable cell for the type's SpeedType
/// (`Find_Nearby_Passable_Cell @ 0x0056DC20`, z = 0), else delete.
pub(super) fn place_unit_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    owner: InternedId,
    type_id: &str,
    cell: (i16, i16),
    center: DriveCoord,
) -> UnitCrateOutcome {
    use crate::sim::find_nearby_cell::{
        NearbyAnchorGate, NearbyFootprint, NearbyQuery, NearbySearchOptions, PassabilityArgs,
        RADIUS_HARD_CAP, find_nearby_passable_cell_with_options,
    };
    use crate::sim::world::PlacementEvidence;
    let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
        return UnitCrateOutcome::NoObject;
    };
    let owner_name = sim.interner.resolve(owner).to_owned();
    let level =
        u8::try_from(center.z / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS).unwrap_or(0);
    let Some(id) = sim.spawn_object_limbo_at_height(type_id, &owner_name, rx, ry, 0, level, rules)
    else {
        return UnitCrateOutcome::NoObject;
    };
    if sim
        .reveal_constructed_object_at_coord_with_overlay_context(
            id,
            center,
            0,
            PlacementEvidence::EvaluateMark,
            rules,
            Some(registry),
        )
        .is_some()
    {
        return UnitCrateOutcome::Placed;
    }
    let nearby = rules
        .object_in_category(ObjectCategory::Vehicle, type_id)
        .and_then(|object| {
            // `(cell, SpeedType, -1, 0, 0, 1, 1, 0, 0, 0, 1, &0, 0, 0)`: the
            // free-unit query shape, bridge cells refused, the level gate on.
            let query = NearbyQuery {
                native_cells: None,
                raw_occupation: None,
                passability: PassabilityArgs {
                    speed_type: object.speed_type,
                    required_zone_id: None,
                    movement_zone: object.movement_zone,
                    bridge_aware_zone: false,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::UnverifiedCompatibilityBypass,
                allow_bridge_cells: false,
                check_height: true,
                check_occupancy: false,
                radius_cap: RADIUS_HARD_CAP,
                target_cell: None,
                path_grid: sim.path_grid(),
                resolved_terrain: sim.resolved_terrain.as_ref(),
                overlay_grid: sim.overlay_grid.as_ref(),
                occupancy: Some(&sim.substrate.occupancy),
                entities: Some(&sim.substrate.entities),
                zone_grid: sim.zone_grid.as_ref(),
                playfield_bounds: sim.playfield_bounds,
            };
            find_nearby_passable_cell_with_options(
                (i32::from(rx), i32::from(ry)),
                &query,
                NearbySearchOptions::default(),
                sim.session.binary_frame,
            )
        });
    if let Some((nx, ny)) = nearby {
        let coord = DriveCoord {
            x: i32::from(nx).wrapping_mul(256).wrapping_add(128),
            y: i32::from(ny).wrapping_mul(256).wrapping_add(128),
            z: 0,
        };
        if sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                coord,
                0,
                PlacementEvidence::EvaluateMark,
                rules,
                Some(registry),
            )
            .is_some()
        {
            return UnitCrateOutcome::Placed;
        }
    }
    sim.discard_constructed_limbo(id, Some(rules));
    UnitCrateOutcome::FellBackToMoney
}
