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
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;
use crate::sim::world::display_layers::DisplayLayer;
use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits, distance_3d_leptons};

/// Which `Techno` double a multiplier crate raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Multiplier {
    /// `Techno+0x158`, the Armor arm `0x00482E53..0x00482E79`.
    Armor,
    /// `Techno+0x160`, the Firepower arm `0x00483222..0x00483248`.
    Firepower,
}

/// The object's crate multipliers, `Techno+0x158` (armor) and `Techno+0x160`
/// (firepower). Under Ares, Ext+88/+80 retain permanent armor/firepower baselines
/// separately from derived Techno+158/+160. The constructor seeds both baselines
/// to 1.0; only the Armor and Firepower arms ([`apply_multiplier_crate`]) write
/// these permanent baselines. Each arm skips a baseline that is not exactly 1.0.
/// The attachment owner separately recomputes effective values. The receiver's
/// damage build reads the armor multiplier;
/// `FireAt`'s damage build folds in the firepower one
/// (`0x006FE33D..0x006FE34D`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CrateMultipliers {
    #[serde(default = "one")]
    armor: NativeF64Bits,
    #[serde(default = "one")]
    firepower: NativeF64Bits,
    /// Ares derived Techno+160; None retains the permanent Ext+80 baseline.
    #[cfg(feature = "ares")]
    #[serde(default)]
    attached_firepower: Option<NativeF64Bits>,
    /// Ares derived Techno+158; None retains the permanent Ext+88 baseline.
    #[cfg(feature = "ares")]
    #[serde(default)]
    attached_armor: Option<NativeF64Bits>,
}

fn one() -> NativeF64Bits {
    NativeF64Bits::ONE
}

impl Default for CrateMultipliers {
    /// The constructor's 1.0 seeds.
    fn default() -> Self {
        Self {
            armor: NativeF64Bits::ONE,
            firepower: NativeF64Bits::ONE,
            #[cfg(feature = "ares")]
            attached_firepower: None,
            #[cfg(feature = "ares")]
            attached_armor: None,
        }
    }
}

impl CrateMultipliers {
    /// `Techno+0x158`.
    pub(crate) const fn armor(self) -> NativeF64Bits {
        #[cfg(feature = "ares")]
        if let Some(value) = self.attached_armor {
            return value;
        }
        self.armor
    }

    /// Permanent armor crate baseline (Ares Ext+88), never an AE product.
    pub(crate) const fn crate_armor(self) -> NativeF64Bits {
        self.armor
    }

    #[cfg(feature = "ares")]
    pub(crate) fn set_attached_armor(&mut self, value: NativeF64Bits) {
        self.attached_armor = (value != self.armor).then_some(value);
    }

    /// Effective Techno+160, read by both FireAt and EstimateDamage.
    pub(crate) const fn firepower(self) -> NativeF64Bits {
        #[cfg(feature = "ares")]
        if let Some(value) = self.attached_firepower {
            return value;
        }
        self.firepower
    }

    /// Permanent firepower crate baseline (Ares Ext+80), never an AE product.
    pub(crate) const fn crate_firepower(self) -> NativeF64Bits {
        self.firepower
    }

    #[cfg(feature = "ares")]
    pub(crate) fn set_attached_firepower(&mut self, value: NativeF64Bits) {
        self.attached_firepower = (value != self.firepower).then_some(value);
    }

    /// Install a fixture's armor multiplier.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_armor_multiplier_for_test(&mut self, value: NativeF64Bits) {
        self.armor = value;
        #[cfg(feature = "ares")]
        {
            self.attached_armor = None;
        }
    }

    /// Install a fixture's firepower multiplier.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_firepower_multiplier_for_test(&mut self, value: NativeF64Bits) {
        self.firepower = value;
        #[cfg(feature = "ares")]
        {
            self.attached_firepower = None;
        }
    }
}

/// Ground-layer objects within `radius` of `center`, in layer order, with the
/// native `Sqrt_Approx`/`ftol` distance and strict `<` (`0x00482E38..0x00482E51`).
fn ground_members_in_radius(
    substrate: &ObjectSubstrate,
    center: DriveCoord,
    radius: i32,
) -> Vec<u64> {
    substrate
        .display
        .members(DisplayLayer::GROUND)
        .iter()
        .copied()
        .filter(|&id| {
            substrate.entities.get(id).is_some_and(|entity| {
                let position = position_world_coord(&entity.position);
                distance_3d_leptons(
                    [center.x, center.y, center.z],
                    [position.x, position.y, position.z],
                ) < radius
            })
        })
        .collect()
}

/// CellClass::PickupCrate @00482840..0048296D: every marked ground-layer
/// Techno strictly inside CrateRadius receives the permanent +3D2 flag.
/// This arm starts no cloak transition, plays no sound and spends no RNG.
pub(crate) fn apply_cloak_crate(
    substrate: &mut ObjectSubstrate,
    rules: &RuleSet,
    interner: &StringInterner,
    center: DriveCoord,
    radius: i32,
) {
    #[cfg(not(feature = "ares"))]
    let _ = (rules, interner);
    for id in ground_members_in_radius(substrate, center, radius) {
        if let Some(entity) = substrate.entities.get_mut(id) {
            entity.stealth_ability.grant();
            // Installed Cloak2@1005F920 writes permanent Ext+98 then aggregates.
            #[cfg(feature = "ares")]
            entity.recompute_ares_attached_stats(rules, interner);
        }
    }
}

/// The Armor and Firepower arms: every Techno within the radius whose
/// multiplier is exactly 1.0 takes `multiplier * magnitude` (x87 double
/// multiply, `0x00482E6D..0x00482E79`). Returns the owners of the changed
/// objects, in native visit order; the arm announces when one of them is the
/// local player (`vt+0x3C` then `0x0050B6F0`, `0x00482E7F..0x00482E8D`).
pub(crate) fn apply_multiplier_crate(
    substrate: &mut ObjectSubstrate,
    rules: &RuleSet,
    interner: &StringInterner,
    center: DriveCoord,
    radius: i32,
    magnitude: NativeF64Bits,
    which: Multiplier,
) -> Vec<InternedId> {
    #[cfg(not(feature = "ares"))]
    let _ = (rules, interner);
    let mut changed_owners = Vec::new();
    for id in ground_members_in_radius(substrate, center, radius) {
        let Some(entity) = substrate.entities.get_mut(id) else {
            continue;
        };
        let slot = match which {
            Multiplier::Armor => &mut entity.crate_multipliers.armor,
            Multiplier::Firepower => &mut entity.crate_multipliers.firepower,
        };
        if *slot != NativeF64Bits::ONE {
            continue;
        }
        *slot =
            X87::store_f64_masked_chop(X87::mul(X87::load_f64(magnitude), X87::load_f64(*slot)));
        // Ares Armor2@1005F870 writes Ext+88 then calls RecalculateStats
        // @1005F89D; Firepower2@1005F997 does the same for Ext+80. Both
        // aggregate before the recipient callback/next actor (Ares Crates.cpp).
        #[cfg(feature = "ares")]
        entity.recompute_ares_attached_stats(rules, interner);
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
    interner: &StringInterner,
    substrate: &mut ObjectSubstrate,
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
    for id in ground_members_in_radius(&*substrate, center, radius) {
        let Some(entity) = substrate.entities.get(id) else {
            continue;
        };
        // `Object+0x74`: ObjectClass::Mark sets it on PUT and clears it on
        // REMOVE (`0x005F58F7`, `jumpjet_cruise` `0x0054D0FF..0x0054D12C`),
        // which is the cell-membership byte `lifecycle.cell_marked` owns.
        if !entity.lifecycle.cell_marked() {
            continue;
        }
        if !interner
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.trainable)
        {
            continue;
        }
        let entity = substrate.entities.get_mut(id).expect("checked above");
        let mut level = 0_i32;
        loop {
            if veterancy::raw_is_veteran(entity.veterancy.raw()) {
                entity.veterancy.set_elite();
            }
            if veterancy::raw_is_rookie(entity.veterancy.raw()) {
                entity.veterancy.set_veteran();
            }
            if veterancy::raw_is_negative(entity.veterancy.raw()) {
                entity.veterancy.set_rookie();
            }
            level += 1;
            if !ordered_less(f64::from(level), magnitude) {
                break;
            }
        }
    }
}

/// The HealBase arm's live Logic loop `0x00482C18..0x00482C9C`: each
/// same-house Techno receives `Health - Type.Strength` through
/// `ReceiveDamage(&amount, 0, C4Warhead=, NULL, 1, 1, NULL)`. Each receiver
/// finishes before the next live slot, collector owner, type and health read.
pub(super) fn apply_heal_base_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    collector: u64,
    frame_effects: FrameEffects<'_>,
) {
    let warhead = sim.interner.intern(&rules.bridge_warheads.c4_name);
    let mut index = 0;
    while let Some(&id) = sim.logic_order().get(index) {
        index += 1;
        // Non-Techno Logic members have their own stores. The native loop
        // tests the collector's Techno bit, not the recipient's category.
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        let Some(collector) = sim.substrate.entities.get(collector) else {
            break;
        };
        if entity.owner() != collector.owner() {
            continue;
        }
        let Some(object_type) = sim.object_type(entity.type_ref(), rules) else {
            continue;
        };
        let amount = entity.health.current().wrapping_sub(object_type.strength);
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
            frame_effects,
        );
    }
}

/// The Money arm's amount `0x00482479..0x004824A5`: the solo override when
/// set, otherwise `RandomRanged(ftol(magnitude), ftol(magnitude) + 900)` on
/// the Scenario RNG. `HouseClass::Add_Credits @ 0x004F9950` then adds it.
pub(super) fn money_crate_amount(
    scenario_rng: &mut SimRng,
    solo_money: i32,
    magnitude: NativeF64Bits,
) -> i32 {
    if solo_money != 0 {
        return solo_money;
    }
    let base = magnitude_amount(magnitude);
    scenario_rng.next_range_i32_inclusive(base, base.wrapping_add(900))
}

/// `Math__ftol` of one `[Powerups]` magnitude double, the damage base the Gas
/// (`fld` at `0x00481E08`), Napalm (`fld` at `0x004827E8`, `call` at
/// `0x004827EC`) and Explosion (`fld` at `0x00482579`) arms convert out of the
/// shared `local_170` magnitude load at `0x00481DC3`.
fn magnitude_amount(magnitude: NativeF64Bits) -> i32 {
    crate::util::native_x87::X87Chop53::ftol_f64_low_masked(f64::from_bits(magnitude.bits()))
}

/// The Gas arm `0x00481DE7..0x00481E99`: the named "GAS" warhead on the crate
/// cell and each `0x0089F688` neighbour (centre first, then N, NE, E, SE, S,
/// SW, W, NW), each one `Apply_area_damage(cellCentre, amount, 0, warhead, 0,
/// 0)` whose arg3 source of 0 walks no per-cell spread. No RNG and no sound:
/// the arm jumps to the shared anim tail past `0x004832F0`.
pub(super) fn apply_gas_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    cell: (i16, i16),
    magnitude: NativeF64Bits,
    frame_effects: FrameEffects<'_>,
) {
    // `0x00481DE7..0x00481E06` walks the 0x8874C4 WarheadType array for
    // "GAS" through the `strcasecmp` finder 0x75E4A0; the WarheadType
    // factory 0x75E3B0 (which reads `FlameDamage=`) scans and allocates into
    // the same array. `rules.warhead` is the same case-insensitive name
    // search over the parsed warheads.
    let Some(warhead) = rules.warhead("GAS") else {
        // 0x75E4A0 returns NULL for a missing name (0x75E4D3), and
        // Apply_area_damage returns on a null warhead (0x004892D7), so all
        // nine calls are inert; the anim tail still runs.
        return;
    };
    let warhead_ref = sim.interner.intern(&warhead.id);
    let amount = magnitude_amount(magnitude);
    // The nine-cell centres go through the same retained-Cell centre owner as
    // the dispatcher (`crate_center`); the direction adds are native WORD adds.
    let mut targets = Vec::with_capacity(9);
    targets.push(cell);
    for (dx, dy) in crate::util::direction::DIRECTION_DELTAS {
        targets.push((
            cell.0.wrapping_add(dx as i16),
            cell.1.wrapping_add(dy as i16),
        ));
    }
    for target in targets {
        let receiver = sim.resolved_terrain.as_ref().map(|terrain| {
            crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(target)
        });
        let centre = super::pickup::crate_center(sim, target, receiver);
        // RESIDUAL: native passes affectsTiberium=0 (0x00481E17, 0x00481E71);
        // apply_area_damage hard-codes 1. Dormant: retail [Gas] sets no Tiberium=.
        crate::sim::combat::world_receiver::apply_area_damage(
            sim,
            rules,
            registry,
            crate::sim::projectile::ProjectileCoord::new(centre.x, centre.y, centre.z),
            amount,
            warhead,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            frame_effects,
        );
    }
}

/// The Tiberium arm `0x00481E99..0x00481F68`: one `IncreaseTiberium(type, 1)`
/// on the crate cell, then `RandomRanged(10, 20)` further ones at coordinates
/// the shared `0x0049F420` random-direction helper (snap 1) moves up to
/// `RandomRanged(0, 0x300)` leptons away — two draws per loop pass, with no
/// validity retry, so a refused cell still spends its pair.
pub(super) fn apply_tiberium_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    cell: (i16, i16),
    base: DriveCoord,
) {
    // `0x00481EAD..0x00481ED1`: `RandomRanged(0, Tiberiums.Count - 1)` picks
    // the ore type, and a drawn 1 clamps it to 0 (`cmp edi,1`/`jne`/`xor
    // edi,edi` at `0x00481ECC..0x00481ED1`) — both paths fall into the same
    // `IncreaseTiberium(type, 1)` at `0x00481ED3..0x00481ED8`, so the arm
    // never abandons on the draw.
    let tiberium_count = rules.tiberium_types.len();
    if tiberium_count == 0 {
        // Unpinned defensive edge: with no registered ore type native would
        // draw `RandomRanged(0, -1)` and index an empty array; VERA leaves
        // the arm (and its RNG) alone instead.
        return;
    }
    let drawn = sim
        .scenario_rng
        .next_range_i32_inclusive(0, tiberium_count as i32 - 1);
    // Unpinned: past 256 ore types `u8::try_from` fails where native keeps
    // indexing the type array with the raw value; VERA drops the arm there.
    let Ok(type_index) = u8::try_from(if drawn == 1 { 0 } else { drawn }) else {
        return;
    };
    let type_id = crate::rules::tiberium_type::TiberiumTypeId(type_index);
    let (Ok(pickup_x), Ok(pickup_y)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
        return;
    };
    // `CellClass::IncreaseTiberium @ 0x00487190` runs on the live map: the
    // shared placement owner already consumes the exact native draw order —
    // the twelve-image flat variant pick, the growth-queue priority draw and
    // the spread feeds — so the arm drives it with the scenario RNG and never
    // draws around it (see `place_tiberium`).
    let binary_frame = sim.session.binary_frame();
    let growth_enabled = sim.ore_growth_config.grows();
    let spread_enabled = sim.ore_growth_config.spreads();
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        // Native always increases tiberium through the map owner; a mapless
        // fixture has no Cell to write and stops after its Draw1.
        return;
    };
    let live_objects = crate::sim::tiberium::TiberiumPlacementObjectContext::new(
        &sim.substrate.entities,
        &sim.substrate.occupancy,
        &sim.substrate.raw_cell_occupation,
        rules,
        &sim.interner,
        sim.terrain_objects.cell_index(),
    );
    let admission = crate::sim::tiberium::NewTiberiumAdmission::runtime(terrain, live_objects);
    let Some(grid) = sim.overlay_grid.as_mut() else {
        return;
    };
    let mut context = crate::sim::tiberium::PlaceTiberiumContext {
        overlay_grid: grid,
        ore_growth_state: &mut sim.ore_growth_state,
        overlay_registry: registry,
        tiberium_types: &rules.tiberium_types,
        resolved_terrain: Some(terrain),
        source_object_cells: sim.terrain_objects.spawning_cells(),
        new_cell_admission: Some(admission),
        live_objects: Some(admission.live_objects().object_view()),
        rng: &mut sim.scenario_rng,
        binary_frame,
        growth_enabled,
        spread_enabled,
        dirty_cells: Some(&mut sim.dirty_cells),
    };
    let mut receipts = Vec::new();
    let mut placed_cells = Vec::new();
    if let Some(placement) =
        crate::sim::tiberium::place_tiberium(&mut context, (pickup_x, pickup_y), type_id, 1)
    {
        placed_cells.push(placement.cell());
        receipts.extend(placement.into_overlay_construction());
    }
    // `0x00481EDD..0x00481F68`: the loop count, then per pass one
    // `RandomRanged(0, 0x300)` distance and the one raw draw inside 0x49F420
    // (the snap-1 CellCentre mask applies after the 512-cell fallback).
    let count = context.rng.next_range_i32_inclusive(10, 20);
    for _ in 0..count.max(0) {
        let distance = context.rng.next_range_i32_inclusive(0, 0x300);
        let (x, y) = crate::sim::combat::inviso_scatter::random_direction_coord(
            context.rng,
            base.x,
            base.y,
            distance,
            crate::sim::combat::inviso_scatter::RandomDirectionSnap::CellCenter,
        );
        let target = (
            crate::util::lepton::lepton_to_cell(x)
                .try_into()
                .unwrap_or(u16::MAX),
            crate::util::lepton::lepton_to_cell(y)
                .try_into()
                .unwrap_or(u16::MAX),
        );
        if let Some(placement) =
            crate::sim::tiberium::place_tiberium(&mut context, target, type_id, 1)
        {
            placed_cells.push(placement.cell());
            receipts.extend(placement.into_overlay_construction());
        }
    }
    drop(context);
    // Place487190's overlay constructor joins the runtime registries and its
    // Mark republishes the cell's land, as the ore drivers publish theirs.
    Simulation::publish_overlay_constructions(
        &mut sim.native_unique_ids,
        &mut sim.load_objects,
        &mut sim.substrate,
        receipts,
    );
    sim.publish_tiberium_cells(rules, registry, &placed_cells);
}

/// The Explosion arm `0x00482565..0x0048271E`: the picker first takes
/// `ReceiveDamage(&amount, 0, C4Warhead, NULL, 1, 0, NULL)`, then five
/// blasts — each `RandomRanged(0, 0x200)` plus the raw draw inside the
/// unsnapped `0x0049F420`, `Apply_area_damage` at the scattered coordinate,
/// the `SelectAnim @ 0x0048A4F0` animation (`AnimClass(type, &coord, 0, 1,
/// 0x2600, -15, 0)`) and the `0x0048A620` flash.
pub(super) fn apply_explosion_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    actor_id: u64,
    center: DriveCoord,
    magnitude: NativeF64Bits,
    frame_effects: FrameEffects<'_>,
) {
    // `0x00482572`: Rules+0xFA8. A null C4Warhead leaves ReceiveDamage
    // (701BF6), SelectAnim (48A4FF), Flash (48A652) and Apply_area_damage
    // (4892D7) each inert natively, but the five passes still spend their
    // ten draws.
    let c4 = rules
        .warhead(&rules.bridge_warheads.c4_name)
        .map(|warhead| (warhead, sim.interner.intern(&warhead.id)));
    let amount = magnitude_amount(magnitude);
    // `0x0048258D..0x004825A2`. The blasts read the pre-receive copy at
    // `[esp+0x1C]` (`0x004825A1`, `0x0048263E`), never the receiver's
    // in/out amount.
    if let Some((_, warhead_ref)) = c4 {
        sim.commit_direct_damage_receiver(
            rules,
            registry,
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                actor_id,
                amount,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                warhead_ref,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: false,
                },
            ),
            frame_effects,
        );
    }
    for _ in 0..5 {
        // `0x004825AA..0x004825C2` and the 0x49F420 copy: one distance draw,
        // one raw direction draw, no cell snap.
        let distance = sim.scenario_rng.next_range_i32_inclusive(0, 0x200);
        let (x, y) = crate::sim::combat::inviso_scatter::random_direction_coord(
            &mut sim.scenario_rng,
            center.x,
            center.y,
            distance,
            crate::sim::combat::inviso_scatter::RandomDirectionSnap::Preserve,
        );
        let Some((warhead, warhead_ref)) = c4 else {
            continue;
        };
        let scattered = crate::sim::projectile::ProjectileCoord::new(x, y, center.z);
        crate::sim::combat::world_receiver::apply_area_damage(
            sim,
            rules,
            registry,
            scattered,
            amount,
            warhead,
            (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
            frame_effects,
        );
        // `0x0048A4F0` selection feeding the combat constructor shape
        // `spawn_combat_explosion_anim` owns (`0x2600`/`-15`, Start at
        // construction). The same owners serve TAction42's DoExplosionAt.
        // `0x0048267D` passes the constant LandType 0 (Clear), so the
        // water-splash branch never fires here.
        if let Some(effect) = crate::sim::combat::detonation_anim::effect(
            sim, rules, warhead, amount, 0, scattered, scattered,
        ) {
            crate::sim::world::damage_consequences::admit_explosion_effect(sim, rules, effect);
        }
        sim.combat_light_requests
            .emit(crate::sim::combat::CombatLightRequest::Impact {
                target_id: None,
                damage: amount,
                warhead_ref,
                coord: scattered,
                force_create: false,
                flags: 0,
            });
    }
}

/// The Napalm arm `0x0048271E..0x00482840`: `AnimTypeArray8B4154[0]` at the
/// componentwise midpoint of the crate centre and the picker coordinate, the
/// picker's `ReceiveDamage(&buf, 0, FlameDamage, NULL, 1, 0, NULL)`, then one
/// `Apply_area_damage(mid, ftol(magnitude), 0, FlameDamage, 1, 0)`. No RNG
/// and no sound.
pub(super) fn apply_napalm_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    actor_id: u64,
    cell_center: DriveCoord,
    magnitude: NativeF64Bits,
    frame_effects: FrameEffects<'_>,
) {
    let mid = sim.substrate.entities.get(actor_id).map(|entity| {
        let picker = position_world_coord(&entity.position);
        DriveCoord {
            x: cell_center.x.wrapping_add(picker.x) / 2,
            y: cell_center.y.wrapping_add(picker.y) / 2,
            z: cell_center.z.wrapping_add(picker.z) / 2,
        }
    });
    let Some(mid) = mid else {
        // A picker the synchronous crate trigger already deleted has no
        // modeled coordinate; native would read its lingering object. Unpinned.
        return;
    };
    if let Some(fire_anim) = rules.anim_type_name_by_index(0) {
        // `0x004827C9..0x004827E3`: AnimClass(type, &mid, 0, 1, 0x600, 0, 0).
        sim.spawn_named_anim(
            rules,
            fire_anim,
            crate::sim::anim_class::AnimWorldCoord {
                x: mid.x,
                y: mid.y,
                z: mid.z,
            },
            0,
            0x600,
            0,
        );
    }
    // `0x00482803`/`0x00482827`: Rules+0xF84. A null warhead drops the
    // ReceiveDamage (701BF6) and returns from Apply_area_damage (4892D7).
    let Some(warhead) = rules
        .combat_damage
        .flame_damage()
        .and_then(|name| rules.warhead(name))
    else {
        return;
    };
    let warhead_ref = sim.interner.intern(&warhead.id);
    let amount = magnitude_amount(magnitude);
    sim.commit_direct_damage_receiver(
        rules,
        registry,
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            actor_id,
            amount,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            warhead_ref,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        frame_effects,
    );
    crate::sim::combat::world_receiver::apply_area_damage(
        sim,
        rules,
        registry,
        crate::sim::projectile::ProjectileCoord::new(mid.x, mid.y, mid.z),
        amount,
        warhead,
        (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
        frame_effects,
    );
}

/// The ICBM arm `0x00482CA1..0x00482D56`. `FindFirstOfAction(20)` at
/// `0x00482CB8` names the first type whose `Action=` is Nuke (retail
/// `NukeSpecial`). A scan of the picker House's Supers for one whose type's
/// `Type=` (`+0xB4`) is 0, MultiMissile, gates the grant
/// (`0x00482CC9..0x00482D07`; the matched index is discarded), then
/// `Grant(1, 0, 0)` runs on the found type's own Super (`+0x98`,
/// `0x00482D0D..0x00482D20`). No RNG and no sound.
pub(super) fn apply_icbm_crate(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    // The House constructor holds one Super per registered type, so a
    // registered MultiMissile type is the scan's admission.
    if crate::sim::superweapon::super_types_with_type(rules, 0)
        .next()
        .is_none()
    {
        return;
    }
    let Some(type_name) = crate::sim::superweapon::first_super_type_with_action(rules, "Nuke")
    else {
        // Unpinned: native dereferences the null type here (`0x00482D0D`).
        return;
    };
    let Some(type_id) = sim.interner.get(type_name) else {
        return;
    };
    crate::sim::superweapon::trigger_grant_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.houses,
        &mut sim.super_weapons,
        rules,
        owner,
        type_id,
        true,
    );
    // `0x00482D2D..0x00482D4C`: a successful grant adds the current player's
    // cameo (`AddCameo(0x1F, ..)`); the sidebar projects active Supers.
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
    interner: &StringInterner,
    session: &ScenarioSession,
    scenario_rng: &mut SimRng,
    houses: &Houses,
    rules: &'r RuleSet,
    owner: InternedId,
    preempted: bool,
) -> Option<&'r crate::rules::object_type::ObjectType> {
    use crate::sim::ai_buildable::first_owner_compatible;
    let house = houses.get(&owner)?;
    let country_bit = rules.country_owner_mask(interner.resolve(house.house_type_id()));
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
            interner
                .get(type_id)
                .map_or(0, |type_id| house.tracking().owned_count(category, type_id))
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
    let human = house.is_controlled_by_human(session.game_mode_nonzero());
    let bases = session.game_options().bases;
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
        let index = scenario_rng.next_range_i32_inclusive(0, count.wrapping_sub(1));
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
    frame_effects: FrameEffects<'_>,
) -> UnitCrateOutcome {
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
    use crate::sim::world::PlacementEvidence;
    let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
        return UnitCrateOutcome::NoObject;
    };
    let owner_name = sim.interner.resolve(owner).to_owned();
    let level =
        u8::try_from(center.z / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS).unwrap_or(0);
    let Some(id) = sim.construct_object_limbo_at_height(
        type_id,
        &owner_name,
        rx,
        ry,
        0,
        level,
        rules,
        frame_effects,
    ) else {
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
            frame_effects,
        )
        .is_some()
    {
        return UnitCrateOutcome::Placed;
    }
    let nearby = rules
        .object_in_category(ObjectCategory::Vehicle, type_id)
        .and_then(|object| {
            // `0x0048231E..0x00482366` push `(cell, Type+0x67C, -1, 0, 0, 1,
            // 1, 0, 0, 0, 1, &0, 0, 0)`: the type's SpeedType, zone -1 and
            // MovementZone Normal in the NearbyLocation shape.
            sim.nearby_passable_cell(
                (i32::from(rx), i32::from(ry)),
                NearbyArgs::plain(
                    PassabilityArgs {
                        speed_type: object.speed_type,
                        required_zone_id: None,
                        movement_zone: MovementZone::Normal,
                        bridge_aware_zone: false,
                    },
                    NearbyFootprint::SINGLE,
                ),
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
                frame_effects,
            )
            .is_some()
        {
            return UnitCrateOutcome::Placed;
        }
    }
    sim.discard_constructed_limbo(id, rules, frame_effects);
    UnitCrateOutcome::FellBackToMoney
}
