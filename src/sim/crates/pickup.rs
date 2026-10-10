//! `CellClass::PickupCrate @ 0x00481A00`: the crate a Foot picks up when its
//! movement commits to a crate cell.
//!
//! Callers (`get_xrefs_to 0x00481A00`): Drive `Process_Movement` at the
//! first fresh candidate (`0x004B405D`) and the finalize tail (`0x004B46E6`),
//! its Ship twin (`0x006A3689`, `0x006A3D15`), Drive/Ship `Force_Track`
//! (`0x004B0D1B`, `0x006A03EB`), Hover `ProcessMovement` (`0x005153E9`),
//! Teleport's arrival (`0x0071972E`), Jumpjet touchdown (`0x0054C9F6`),
//! Drive/Ship `Process_Track` (`0x004B1DBE`, `0x006A1401`), Mech
//! (`0x005B1894`) and Walk (`0x0075C56C`). Every site but Mech is wired
//! (`Simulation::pickup_crate_at`); the Mech locomotor has no Rust host.
//!
//! Body, in native order:
//! 1. `0x00481A12..0x00481A39`: null actor, `Cell+0x44 == -1`, or an
//!    OverlayType without `Crate=` returns true (nothing happened).
//! 2. `0x00481A3F..0x00481A67`: outside game mode 0 a `MultiplayPassive=`
//!    owner returns true.
//! 3. `0x00481A6D..0x00481AC1`: a `CrateTrigger=` overlay springs the actor's
//!    attached tag (event `0x31`) and latches `Scenario+0x34BE` (residual).
//! 4. `0x00481AC8..0x00481B94`: [`select_pickup_outcome`].
//! 5. `0x00481B99..0x00481D81`: outside mode 0, the MCV pre-empt, the
//!    per-powerup eligibility switch and the over-water fallback, each falling
//!    back to Money (slot 0).
//! 6. `0x00481D86..0x00481DB3`: `MapClass` pickup removal (`0x0056C020`),
//!    then, outside mode 0 with the lobby Crates option, one replacement
//!    `MapClass::PlaceCrateAtRandomCell @ 0x0056BD40`.
//! 7. `0x00481DB8..0x00481DE0`: Squad becomes Money; the slot's magnitude is
//!    loaded; slots above 17 skip to the anim; the jump table at
//!    `0x004833C4` selects the arm.
//! 8. `0x004832F5..0x00483389`: the slot's `[Powerups]` anim at the cell
//!    centre 200 leptons above the ground (`AnimClass(type, coord, 0, 1,
//!    0x600, 0, 0)`); return true.
//!
//! Original-code comparisons: `tools/spatial_oracle/crate_pickup.json`
//! (guards, selection, removal and the Speed arm).

use super::effects::{self, Multiplier, UnitCrateOutcome};
use super::runtime::remove_pickup_crate;
use super::{ForcedPostPrecheckFailure, place_one_random_crate, read_crate_mark_fields};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::powerups::{
    POWERUP_ARMOR, POWERUP_COUNT, POWERUP_FIREPOWER, POWERUP_HEAL_BASE, POWERUP_MONEY,
    POWERUP_REVEAL, POWERUP_SPEED, POWERUP_SQUAD, POWERUP_UNIT, POWERUP_VETERAN,
};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::components::{DriveCoord, Position};
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose;
use crate::sim::rng::SimRng;
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF64Bits;

/// `[Powerups]` slots the eligibility switch and dispatcher name beyond the
/// shared constants: the table order is the binary's (`0x007E523C`).
const POWERUP_CLOAK: usize = 3;
const POWERUP_DARKNESS: usize = 7;
/// The last slot the jump table at `0x004833C4` covers (`CMP EBX,0x11`).
const LAST_DISPATCHED_SLOT: usize = 0x11;
/// `[General] CrateRadius=`'s MCV pre-empt credit floor (`0x00481BCC`).
const MCV_PREEMPT_CREDITS: i32 = 0x5DC;
/// The Unit arm's tracked-vehicle ceiling (`0x00481C27`).
const UNIT_CRATE_MAX_VEHICLES: i32 = 0x32;
/// The Squad arm's tracked-infantry ceiling (`0x00481C3B`).
const SQUAD_CRATE_MAX_INFANTRY: i32 = 0x64;
/// Lepton height of the pickup anim above the ground (`0x00483339`).
const PICKUP_ANIM_HEIGHT: i32 = 0xC8;

/// Transient selection before multiplayer eligibility checks. Solo money is
/// native's local override (zero means the Money arm still draws its amount).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PickupSelection {
    pub powerup: usize,
    pub solo_money: i32,
}

/// Cell481AC8..481B99: stored selections below19 spend no draw. Otherwise sum
/// all nineteen signed weights and use Scenario's inclusive RandomRanged. Solo
/// image overrides then run in Silver/Wood/Water order, independently: retail
/// aliases Silver and Wood to the same OverlayType, so the later write wins.
///
/// Call only after the null/overlay/passive guards and synchronous crate trigger.
/// Multiplayer eligibility and the post-removal Squad fallback follow later.
/// Original-code comparisons: tools/spatial_oracle/crate_pickup.json.
pub(super) fn select_pickup_outcome(
    rng: &mut SimRng,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    overlay_id: u8,
    stored_selection: u8,
    multiplayer: bool,
) -> PickupSelection {
    let mut powerup = usize::from(stored_selection);
    if powerup >= POWERUP_COUNT {
        let total = rules
            .powerups
            .weights
            .iter()
            .copied()
            .fold(0_i32, i32::wrapping_add);
        let draw = rng.next_range_i32_inclusive(1, total);
        let mut cumulative = 0_i32;
        powerup = rules
            .powerups
            .weights
            .iter()
            .position(|weight| {
                cumulative = cumulative.wrapping_add(*weight);
                draw <= cumulative
            })
            .unwrap_or(POWERUP_COUNT);
    }
    let mut solo_money = 0;
    if !multiplayer && stored_selection == 0 {
        let crate_rules = &rules.crate_rules;
        solo_money = crate_rules.solo_crate_money;
        for (image, selection) in [
            (&crate_rules.crate_img, crate_rules.silver_crate),
            (&crate_rules.wood_crate_img, crate_rules.wood_crate),
            (&crate_rules.water_crate_img, crate_rules.water_crate),
        ] {
            if image.as_deref().and_then(|name| registry.id_for_name(name)) == Some(overlay_id) {
                powerup = selection;
            }
        }
    }
    PickupSelection {
        powerup,
        solo_money,
    }
}

/// The crate cell's centre coordinate on its ground (`0x0041C230` then
/// `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0` with the `(0x80,
/// 0x80)` offset), as every arm and the anim tail build it.
fn crate_center(sim: &Simulation, cell: (i16, i16)) -> DriveCoord {
    let mut center = DriveCoord {
        x: i32::from(cell.0).wrapping_mul(256).wrapping_add(128),
        y: i32::from(cell.1).wrapping_mul(256).wrapping_add(128),
        z: 0,
    };
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        let cells = NativeCellQuery::canonical(terrain);
        center.z = ground_pose::query_ground_height(&cells, center).unwrap_or(0);
    }
    center
}

/// `CellClass+0xEC`, the land type the eligibility tests read
/// (`0x00481C4A`, `0x00481D52`).
fn crate_land_type(sim: &Simulation, cell: (i16, i16)) -> u8 {
    let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
        return LandType::Clear.as_index();
    };
    sim.resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(rx, ry))
        .map_or(LandType::Clear.as_index(), |terrain_cell| {
            terrain_cell.yr_cell_land_type
        })
}

/// `VocClass::PlayAt @ 0x007509E0` of one `[AudioVisual]` crate sound at the
/// crate centre. Every arm but Reveal plays it only when `HouseClass::
/// IsControlledByCurrentPlayer @ 0x0050B6F0` admits the actor's house
/// (`heard_by`): the sim cannot see the local player, so the event carries
/// the house and the app applies that test. Reveal (`0x0048202D..0x0048203C`)
/// plays it for everyone.
fn play_crate_sound(
    sim: &mut Simulation,
    sound: Option<&str>,
    heard_by: Option<InternedId>,
    cell: (i16, i16),
    center: DriveCoord,
) {
    let Some(sound) = sound else {
        return;
    };
    let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
        return;
    };
    let position = Position {
        rx,
        ry,
        z: u8::try_from(center.z / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS).unwrap_or(0),
        exact_z_leptons: Some(center.z),
        sub_x: SimFixed::from_num(128),
        sub_y: SimFixed::from_num(128),
    };
    sim.sound_events.push(SimSoundEvent::voc_at_for(
        sound.to_owned(),
        heard_by.map(|owner| [owner, owner]),
        &position,
    ));
}

/// `VoxClass::PlayEVA @ 0x00752700` of a stat-upgrade line when a changed
/// object's owner is the local player; emitted per changed house for the
/// app's local test. Retail `evamd.ini` defines none of the three names, so
/// they resolve to silence there as natively.
fn announce_upgrade(sim: &mut Simulation, owners: &[InternedId], event: &'static str) {
    for &owner in owners {
        sim.sound_events
            .push(SimSoundEvent::HouseEva { owner, event });
    }
}

/// `0x00481C03..0x00481D6B`: the multiplayer eligibility switch on the
/// selected slot and the over-water test. Returns the slot the arm runs.
fn multiplayer_eligible_slot(
    sim: &Simulation,
    rules: &RuleSet,
    actor_id: u64,
    land: u8,
    powerup: usize,
) -> usize {
    let Some(actor) = sim.substrate.entities.get(actor_id) else {
        return powerup;
    };
    let Some(house) = sim.houses.get(&actor.owner()) else {
        return powerup;
    };
    let shore = land == LandType::Water.as_index() || land == LandType::Beach.as_index();
    let object = sim.object_type(actor.type_ref(), rules);
    let eligible = match powerup {
        // `0x00481C1E`: too many vehicles, or a shore cell.
        POWERUP_UNIT => house.tracking.vehicles() <= UNIT_CRATE_MAX_VEHICLES && !shore,
        // `0x00481C32`: too much infantry, or a shore cell.
        POWERUP_SQUAD => house.tracking.infantry() <= SQUAD_CRATE_MAX_INFANTRY && !shore,
        // `0x00481D3A`: an already cloakable actor (`Techno+0x3D2`, seeded
        // from the type's `Cloakable=`; the Cloak crate's permanent write is
        // this module's residual).
        POWERUP_CLOAK => !object.is_some_and(|object| object.cloakable),
        // `0x00481C69`: an actor already armoured.
        POWERUP_ARMOR => actor.armor_multiplier == NativeF64Bits::ONE,
        // `0x00481CDE`: an actor already sped up, or an Aircraft.
        POWERUP_SPEED => {
            actor.foot_speed.crate_multiplier() == NativeF64Bits::ONE
                && actor.category != EntityCategory::Aircraft
        }
        // `0x00481D0B`: an actor already boosted, or unarmed (`Is_Armed`).
        POWERUP_FIREPOWER => {
            actor.firepower_multiplier == NativeF64Bits::ONE
                && object.is_some_and(|object| {
                    crate::sim::combat::combat_weapon::is_armed(actor, object)
                })
        }
        // `0x00481C91`: a Techno actor whose type is not `Trainable=`, or
        // one already elite.
        POWERUP_VETERAN => {
            object.is_some_and(|object| object.trainable)
                && !crate::sim::combat::veterancy::raw_is_elite(actor.veterancy_raw)
        }
        _ => true,
    };
    let mut slot = if eligible { powerup } else { POWERUP_MONEY };
    // `0x00481D52`: on Water the slot needs its over-water flag.
    if land == LandType::Water.as_index()
        && !rules
            .powerups
            .over_water
            .get(slot)
            .copied()
            .unwrap_or(false)
    {
        slot = POWERUP_MONEY;
    }
    slot
}

/// `0x00481BB8..0x00481BFF`: with the lobby Bases option, a house with no
/// tracked building, more than 1500 available credits and no tracked first
/// ownable `BaseUnit=` is forced onto the Unit slot.
fn mcv_preempt(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> bool {
    use crate::sim::ai_buildable::{first_owner_compatible, house_country_bit};
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    if house.tracking.buildings() != 0 {
        return false;
    }
    if house.economy.available_money() <= MCV_PREEMPT_CREDITS {
        return false;
    }
    let country_bit = house_country_bit(rules, sim.interner.resolve(house.house_type_id()));
    let mcv_tracked = first_owner_compatible(
        rules,
        &rules.general.base_unit_types,
        crate::rules::object_type::ObjectCategory::Vehicle,
        country_bit,
    )
    .and_then(|mcv| sim.interner.get(&mcv.id))
    .map_or(0, |type_id| {
        house.tracking.owned_count(EntityCategory::Unit, type_id)
    });
    mcv_tracked == 0 && sim.session.game_options.bases
}

/// `CellClass::PickupCrate @ 0x00481A00` for `actor_id` committing to
/// `cell`. Returns native AL: false only when the sprung trigger killed the
/// actor or the Unit arm placed its vehicle (`0x00482449`); the movement
/// hosts then drop the head they were installing.
pub(crate) fn pickup_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    cell: (i16, i16),
    actor_id: u64,
) -> bool {
    let Some(actor) = sim.substrate.entities.get(actor_id) else {
        return true;
    };
    let owner = actor.owner();
    let (overlay_id, stored_selection) = read_crate_mark_fields(sim, cell);
    let Some(overlay_id) = overlay_id else {
        return true;
    };
    let Some(flags) = registry.flags(overlay_id) else {
        return true;
    };
    if !flags.crate_type {
        return true;
    }
    let multiplayer = sim.session.game_mode_nonzero;
    if multiplayer
        && sim
            .houses
            .get(&owner)
            .is_some_and(|house| house.multiplay_passive)
    {
        return true;
    }
    // `0x00481A6D..0x00481AC1`: a `CrateTrigger=` overlay (both stock crate
    // images) springs the actor's attached TagClass with event 0x31 and
    // latches `Scenario+0x34BE`. RESIDUAL: VERA attaches no Tags to objects
    // and keeps no owner for that latch. Trigger: a tagged object (map
    // scripting) picking up a crate. Effect: the map trigger's "picks up
    // crate" event never fires and a trigger that killed the actor cannot
    // make this return false. Frequency: authored maps only. Risk: none in
    // skirmish.
    let selection = select_pickup_outcome(
        &mut sim.scenario_rng,
        rules,
        registry,
        overlay_id,
        stored_selection,
        multiplayer,
    );
    let mut powerup = selection.powerup;
    let mut preempted = false;
    if multiplayer {
        if mcv_preempt(sim, rules, owner) {
            powerup = POWERUP_UNIT;
            preempted = true;
        }
        let land = crate_land_type(sim, cell);
        powerup = multiplayer_eligible_slot(sim, rules, actor_id, land, powerup);
        // `0x00481D6B..0x00481D81`: game mode 4 counts the slot in the house's
        // statistics block (`0x00749020`). VERA runs no mode-4 session.
    }
    // `0x00481D86`: removal, then the replacement crate.
    let _ = remove_pickup_crate(sim, rules, registry, cell);
    if multiplayer && sim.session.game_options.crates {
        let lighting = sim.scenario_normal_lighting;
        let _ = place_one_random_crate(
            sim,
            rules,
            registry,
            lighting,
            ForcedPostPrecheckFailure::None,
        );
    }
    if powerup == POWERUP_SQUAD {
        powerup = POWERUP_MONEY;
    }
    // A draw past every cumulative weight (negative weights only) leaves slot
    // 19 natively, which reads beyond the magnitude table; nothing applies.
    if powerup >= POWERUP_COUNT {
        return true;
    }
    let magnitude = rules.powerups.magnitudes[powerup];
    let center = crate_center(sim, cell);
    let radius = rules.crate_rules.radius;
    let general = &rules.general;
    // The slot whose anim the tail spawns: the Unit arm's failure rewrites it
    // to Money (`0x0048245D`).
    let mut anim_slot = powerup;
    if powerup <= LAST_DISPATCHED_SLOT {
        let mut arm = powerup;
        if arm == POWERUP_UNIT {
            let chosen = effects::choose_unit_crate_type(sim, rules, owner, preempted)
                .map(|object| object.id.clone());
            let Some(type_id) = chosen else {
                return spawn_pickup_anim(sim, rules, anim_slot, cell, center);
            };
            match effects::place_unit_crate(sim, rules, registry, owner, &type_id, cell, center) {
                UnitCrateOutcome::Placed => {
                    play_crate_sound(
                        sim,
                        general.crate_unit_sound.as_deref(),
                        Some(owner),
                        cell,
                        center,
                    );
                    return false;
                }
                UnitCrateOutcome::NoObject => {
                    return spawn_pickup_anim(sim, rules, anim_slot, cell, center);
                }
                UnitCrateOutcome::FellBackToMoney => {
                    // `0x0048245D`: only the slot copy is rewritten; the
                    // magnitude stays the Unit slot's (`local_170` is not
                    // reloaded), so the Money draw's base is `ftol(Unit)`.
                    arm = POWERUP_MONEY;
                    anim_slot = POWERUP_MONEY;
                }
            }
        }
        match arm {
            POWERUP_MONEY => {
                let amount = effects::money_crate_amount(sim, selection.solo_money, magnitude);
                // `0x004824B0..0x004824D4`: in game mode 0 a player-controlled
                // actor pays `PlayerPtr`; the sim cannot see that house and
                // pays the actor's owner. Identical whenever the controlled
                // house is the player's own, the campaign norm.
                crate::sim::credit_income::add_credits(sim, owner, amount);
                play_crate_sound(
                    sim,
                    general.crate_money_sound.as_deref(),
                    Some(owner),
                    cell,
                    center,
                );
            }
            POWERUP_HEAL_BASE => {
                play_crate_sound(
                    sim,
                    rules.crate_rules.heal_crate_sound.as_deref(),
                    Some(owner),
                    cell,
                    center,
                );
                effects::apply_heal_base_crate(sim, rules, Some(registry), owner);
            }
            POWERUP_DARKNESS => {
                // `MapClass::Reset_Shroud @ 0x00577AB0` for the actor's house;
                // natively only `PlayerPtr`'s shroud changes.
                sim.fog.reset_explored_for_owner(owner);
            }
            POWERUP_REVEAL => {
                // `MapClass::Reveal @ 0x00577D90` for the actor's house.
                sim.reveal_whole_map_for_owner(owner);
                play_crate_sound(
                    sim,
                    general.crate_reveal_sound.as_deref(),
                    None,
                    cell,
                    center,
                );
            }
            POWERUP_ARMOR => {
                let owners = effects::apply_multiplier_crate(
                    sim,
                    center,
                    radius,
                    magnitude,
                    Multiplier::Armor,
                );
                announce_upgrade(sim, &owners, "EVA_UnitArmorUpgraded");
                play_crate_sound(
                    sim,
                    general.crate_armour_sound.as_deref(),
                    Some(owner),
                    cell,
                    center,
                );
            }
            POWERUP_SPEED => {
                if super::speed::apply_speed_crate(sim, center, radius, magnitude) {
                    // `0x00483072`: any recipient whose house has
                    // `PlayerControl` announces.
                    let owners: Vec<InternedId> = sim
                        .houses
                        .iter()
                        .filter(|(_, house)| house.player_control)
                        .map(|(&id, _)| id)
                        .collect();
                    announce_upgrade(sim, &owners, "EVA_UnitSpeedUpgraded");
                }
                play_crate_sound(
                    sim,
                    general.crate_speed_sound.as_deref(),
                    Some(owner),
                    cell,
                    center,
                );
            }
            POWERUP_FIREPOWER => {
                let owners = effects::apply_multiplier_crate(
                    sim,
                    center,
                    radius,
                    magnitude,
                    Multiplier::Firepower,
                );
                announce_upgrade(sim, &owners, "EVA_UnitFirePowerUpgraded");
                play_crate_sound(
                    sim,
                    general.crate_fire_sound.as_deref(),
                    Some(owner),
                    cell,
                    center,
                );
            }
            POWERUP_VETERAN => {
                effects::apply_veteran_crate(sim, rules, center, radius, magnitude);
                play_crate_sound(
                    sim,
                    general.crate_promote_sound.as_deref(),
                    Some(owner),
                    cell,
                    center,
                );
            }
            // RESIDUAL arms, each a zero weight in retail `[Powerups]` so no
            // random pickup reaches them (a map's stored selection or a mod
            // can): Cloak (3, `0x00482840`: `Techno+0x3D2 = 1` on every Techno
            // in the radius; VERA keeps no instance cloakable flag),
            // Explosion (4, `0x00482565`: C4 damage on the actor and five
            // random-offset blasts with two draws each), Napalm (5,
            // `0x0048271E`: a fire anim and warhead damage), ICBM (12,
            // `0x00482CA1`: grant the first ICBM super), Gas (16,
            // `0x00481DE7`: poison gas particle systems on the nine cells)
            // and Tiberium (17, `0x00481E99`: a random ore type and 10..20
            // random placements, two draws each). Effect when reached: the
            // crate is consumed with its anim and no effect, and the
            // Explosion/Tiberium draws are not spent. Invulnerability (13),
            // IonStorm (15) and Pod (18) have no native effect arm either.
            _ => {}
        }
    }
    spawn_pickup_anim(sim, rules, anim_slot, cell, center)
}

/// `0x004832F5..0x00483389`: the slot's `[Powerups]` anim 200 leptons above
/// the crate centre, `AnimClass(type, coord, 0, 1, 0x600, 0, 0)`; a slot whose
/// anim name resolved to no AnimType (`-1`) spawns nothing. Always true.
fn spawn_pickup_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    slot: usize,
    cell: (i16, i16),
    center: DriveCoord,
) -> bool {
    let Some(anim) = rules.powerups.anims.get(slot).and_then(Option::as_deref) else {
        return true;
    };
    let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
        return true;
    };
    let type_name = sim.interner.intern(anim);
    let mut descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
        type_name,
        rx,
        ry,
        SimFixed::from_num(0),
        SimFixed::from_num(0),
        0,
    );
    descriptor.delay = 0;
    descriptor.loop_count = 1;
    descriptor.draw_flags = 0x600;
    descriptor.z_adjust = 0;
    descriptor.reverse = false;
    // An unregistered anim name is native's `-1` index: no anim. Every other
    // failure is a loader gap (`effect_asset_catalog::anim_class_roots` binds
    // the `[Powerups]` names), worth a warning rather than silence.
    if let Err(error) = sim.spawn_anim_at_world(
        rules,
        descriptor,
        crate::sim::anim_class::AnimWorldCoord {
            x: center.x,
            y: center.y,
            z: center.z.wrapping_add(PICKUP_ANIM_HEIGHT),
        },
    ) && !matches!(
        error,
        crate::sim::anim_class::AnimSpawnError::MissingType(_)
    ) {
        log::warn!("crate pickup anim [{anim}] did not spawn: {error}");
    }
    true
}

#[cfg(test)]
#[path = "pickup_tests.rs"]
mod pickup_tests;

#[cfg(test)]
mod tests {
    use super::super::tests::{crate_registry, crate_ruleset};
    use super::*;

    /// Entire Scenario RNG state, not merely the chosen result: rejection draws
    /// change subsequent production RNG even when two selections agree.
    #[test]
    fn selection_and_rng_match_original_pickup_prefix() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/crate_pickup.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in rows {
            if row["prepared"].as_array().unwrap().is_empty() {
                continue;
            }
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut rules = crate_ruleset("");
            rules.powerups.weights = [
                20, 20, 10, 0, 0, 0, 0, 0, 10, 10, 10, 10, 0, 0, 20, 0, 0, 0, 0,
            ];
            if let Some(weights) = input["weights"].as_array() {
                rules.powerups.weights =
                    std::array::from_fn(|i| weights[i].as_i64().unwrap() as i32);
            }
            let choices = input["solo_choices"].as_array().map_or([2, 10, 0], |v| {
                std::array::from_fn(|i| v[i].as_u64().unwrap() as usize)
            });
            rules.crate_rules.silver_crate = choices[0];
            rules.crate_rules.wood_crate = choices[1];
            rules.crate_rules.water_crate = choices[2];
            rules.crate_rules.solo_crate_money =
                input["solo_money"].as_i64().unwrap_or(2000) as i32;
            if input["different_image"] == true {
                rules.crate_rules.wood_crate_img = Some("SILVER".into());
            }
            if input["same_images"] == true {
                rules.crate_rules.crate_img = Some("WOOD".into());
                rules.crate_rules.water_crate_img = Some("WOOD".into());
            }
            let registry = crate_registry();
            let mut rng = SimRng::new(input["seed"].as_u64().unwrap_or(31));
            assert_eq!(
                rng.native_state_hex(),
                row["rng_before"].as_str().unwrap(),
                "{name}"
            );
            let selection = select_pickup_outcome(
                &mut rng,
                &rules,
                &registry,
                registry.id_for_name("WOOD").unwrap(),
                input["selection"].as_u64().unwrap_or(10) as u8,
                input["mode"].as_i64() != Some(0),
            );
            assert_eq!(
                serde_json::json!([[selection.powerup, selection.solo_money]]),
                row["prepared"],
                "{name}"
            );
            assert_eq!(
                rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{name}"
            );
            compared += 1;
        }
        assert_eq!(compared, 24);
    }
}
