//! Stock Techno cloak producer at the Techno AI head.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::intern::InternedId;
use crate::sim::mission::concrete_effects::{
    assign_target_commits, represented_assign_target_admitted,
};
use crate::sim::movement::locomotor::MovementLayer;

use super::Simulation;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SensorCloakReevaluation {
    pub(crate) cloak_transitioned: bool,
    pub(crate) reassigned_targeters: Vec<u64>,
}

fn stock_cloak_tick_facts(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
) -> Option<crate::sim::cloak_disguise::CloakTickFacts> {
    let entity = sim.substrate.entities.get(id)?;
    if entity.category != EntityCategory::Unit || entity.cloak.is_none() {
        return None;
    }
    let object = rules.object(sim.interner.resolve(entity.type_ref()))?;
    let rank_cloak = entity.veterancy >= 100 && object.veteran_cloak
        || entity.veterancy >= 200 && object.elite_cloak;
    if !object.cloakable && !rank_cloak {
        return None;
    }
    let moving =
        crate::sim::movement::drive_locomotor_is_moving(entity) || entity.movement_target.is_some();
    let holds_target = entity.attack_target.is_some();
    // vt+0x1D4 / vt+0x1D8 (`0x0070C5B0` / `0x0070C5C0`, reading
    // `TechnoClass+0x270`/`+0x271`): a Chrono teleport or a Temporal warp.
    let chrono_active = entity.is_warped_out() || entity.is_warping_in();
    // `FootClass::IsCloakable @ 0x004DBDA0` (vtable +0x288) =
    // `HasStealthAbility() && !(CloakStop(+0xC93) && locomotor->IsMoving())`.
    let is_cloakable = object.cloakable && (!object.cloak_stop || !moving);

    // The gate the state-0 head of `CloakingTick @ 0x006FB757..0x006FB7F7`
    // actually applies, read from the disassembly:
    //   IsCloakable(+0x288) && !vt+0x37C && !vt+0x380 && !vt+0x1D4 && !vt+0x1D8,
    //   or, failing that, the current rank's CLOAK ability.
    //
    // CORRECTION: VERA used to put "holds an attack target" in this head gate.
    // Native has no such term here — the target test lives in `CanAutoCloak`
    // step 4 below, as `Target(+0x2B4) != 0 && CanFireAtTarget(vt+0x3AC)`.
    // vt+0x37C is `IsUnderEMP` (Techno `0x0070EFD0` reads `+0x504 > 0`; the
    // Unit override `0x00746C90` ORs in `i32(+0x6D8) != -1`; that field's
    // identity still needs evidence), and
    // vt+0x1D4/+0x1D8 are the chrono warp-in/warp-out flags.
    //
    // RESIDUAL — **EMP decloak is NOT IMPLEMENTED, and this requirement is
    // therefore UNMET, not closed.** VERA has no EMP mechanism anywhere: there
    // is no Rust counterpart to `+0x504`, nothing writes an EMP timer, and the
    // term is hardcoded false at both of its uses in this file. Closing it is
    // not a cloak change — it lands with an EMP weapon/warhead system, and this
    // gate is one of that system's consumers.
    // - Reachability remains unproven. The former stock-source claim was false:
    //   RULESMD's BoomerTorpedo uses APSplash2 and Robogun uses AP; EMPuls is
    //   annotated "disabled in code" and EMPulseSpecial is commented out.
    //   Original EMPulse::Apply4C54E0 writes Techno+504, but its only observed
    //   caller is constructor4C52B0, which has no incoming Ghidra references.
    //   That reference search alone does not establish unreachability. Trace
    //   the actual creation/load path before implementing or closing this arm.
    // - Player effect/frequency: not established for active-retail gameplay.
    // - Downstream risk: none structurally; the predicate is one boolean and
    //   drops into `emp_active` below the day an EMP timer lands.
    // vt+0x380 is FootClass::IsParalyzed `0x004DE770` (Foot+6A0), armed by
    // ParasiteClass: a released owner for 3x its ROF, and bitten victims.
    let emp_active = false;
    let paralyzed = entity.is_paralyzed(sim.session.binary_frame);
    let deploy_pending = entity.deploy_state.is_some();
    let state_zero_head_allows =
        is_cloakable && !emp_active && !paralyzed && !deploy_pending && !chrono_active
            || rank_cloak;

    // CloakingTick's pre-CanAuto destination exclusion is Contact_With_Whom(0)
    // resolving to a WeaponsFactory building (naval-yard repair contact), not
    // an arbitrary movement destination.
    let destination_is_weapons_factory = entity.radio_contacts.slot(0).is_some_and(|contact_id| {
        sim.substrate
            .entities
            .get(contact_id)
            .filter(|contact| contact.category == EntityCategory::Structure)
            .and_then(|contact| rules.object(sim.interner.resolve(contact.type_ref())))
            .is_some_and(|contact_type| contact_type.weapons_factory)
    });
    let current_frame = sim.session.binary_frame as i32;
    let cloak_state = entity.cloak.as_ref().map_or(0, |cloak| cloak.state);
    let cloak_progress = entity.cloak.as_ref().map_or(0, |cloak| cloak.depth);
    let delay_expired = entity
        .cloak
        .as_ref()
        .is_some_and(|cloak| cloak.recloak_delay_expired(current_frame, entity.rearm_timer));

    // `CellClass::IsVisibleToHouse @ 0x004870B0` is the CloakedByHouses bit,
    // NOT cell visibility — see `FogState::is_cloaked_by_house`. It is only
    // ever set by a `CloakGenerator=yes` building's field, and stock YR has
    // none, so this reads false in every ordinary skirmish. VERA previously
    // substituted `fog.is_cell_visible(owner, ...)` here, which is true for an
    // owner standing on its own cell — inverting both gates below.
    let cloaked_by_own_house =
        owner_cloak_field_bit(sim, entity.owner(), entity.position.rx, entity.position.ry);

    // `TechnoClass::CanAutoCloak @ 0x006FBDC0`, in native step order.
    //
    // Step 1's fall-through is `if (!CloakedByHouses(cell, owner) && +0x3D2 ==
    // 0) return false`. `+0x3D2` is the raw stealth-ability byte:
    // `TechnoClass::HasStealthAbility @ 0x0070C5A0` is literally `return
    // +0x3D2 != 0`, and it is seeded at init from `TechnoTypeClass+0xCD0`
    // (`Cloakable=`) — `InfantryClass::InitFromType @ 0x00517D80`,
    // `UnitClass::Constructor @ 0x007355B4`, `TechnoClass::Constructor @
    // 0x006F2F49`. `object.cloakable` below is that seed, so the term is
    // modelled. TWO halves of it are not; both are inert in stock YR:
    //
    // RESIDUAL 1 — the Cloak CRATE also sets `+0x3D2 = 1` permanently, on every
    // object within `[CrateRules] CrateRadius` (`Rules+0x172C`, 3.0 cells) of
    // the picked-up crate: `CrateClass::PickupDispatch @ 0x0048294F`, the arm
    // whose anim comes from the powerup anim table at `0x0081DAD8` (slot 3 =
    // `Cloak`, anim `CLOAK`). VERA has no Cloak-crate producer.
    // - Trigger: a unit picking up a Cloak crate.
    // - Player effect: in gamemd every unit within three cells is permanently
    //   cloakable afterwards; in VERA nothing happens.
    // - Frequency: ZERO in stock, by an AUTHORED zero rather than an absent
    //   row: `ini/rulesmd.ini` `[Powerups]` spells `Cloak=0,CLOAK,yes`. The
    //   weights are the selection authority (`CrateClass::PickupDispatch` sums
    //   the nineteen and draws `RandomRanged(1, total)`), so a 0-weight slot is
    //   never picked — `src/rules/powerups.rs`
    //   `stock_section_parses_the_verified_weight_vector` pins that row's 0 in
    //   the parsed vector (total 110). Only a mod that authors a nonzero
    //   `Cloak=` weight reaches it.
    // - Downstream risk: none; it lands as one extra per-entity flag OR-ed into
    //   `is_cloakable` here and in `should_uncloak` below.
    //
    // RESIDUAL 2 — native ORs the RAW `+0x3D2` here, not vt+0x288, so the two
    // differ exactly when `FootClass::IsCloakable @ 0x004DBDA0` suppresses a
    // stealth unit for `CloakStop=` while it is moving: native still passes
    // step 1, VERA refuses. `grep -c '^CloakStop' ini/rulesmd.ini` is 0, so no
    // stock type can trigger it. Same asymmetry in `should_uncloak` below.
    // - Frequency: zero in stock; mod-only.
    let can_auto_cloak = (is_cloakable || rank_cloak || cloaked_by_own_house)
        // 2. already fully cloaked.
        && cloak_state != 2
        // 3. the ROF rearm countdown at +0x2EC/+0x2F4, and 6. the CloakDelay
        //    countdown at +0x240/+0x248. `recloak_delay_expired` requires both.
        && delay_expired
        // 4. `Target(+0x2B4) != 0 && CanFireAtTarget(vt+0x3AC)`.
        //    SUBSTITUTED: VERA has no cheap `CanFireAtTarget` here and uses the
        //    presence of an attack target instead. The two disagree only for a
        //    unit holding a target no weapon can engage, which VERA's
        //    acquisition and retaliation paths do not install.
        && !holds_target
        // 5. `iVar3 = vt+0x2C(); if (iVar3 != 6 && CloakProgress(+0x224) != 0)
        //    return false` — a BUILDING is exempt from the progress test, every
        //    other category is not. This producer is entered only for
        //    `EntityCategory::Unit` (see the head of this function), so
        //    `WhatAmI != 6` holds for every object that reaches here and the
        //    exemption is structurally unreachable rather than dropped. It is
        //    also unreachable in gamemd data: no stock section pairs
        //    `Cloakable=yes` with a building. This is why the state-3 silent
        //    re-cloak branch is unreachable for units: visual state 1 requires
        //    a nonzero progress.
        && cloak_progress == 0
        // 7. `+0x2B0 && Foot && +0x6AD` (`0x006FBF57..0x006FBF7D`): a
        //    Magnetron-lifted object (`+0x6AD` is written by
        //    `TechnoClass::ImbueLocomotor @ 0x00710352`). VERA has no
        //    Magnetron, so the term is always false and is not evaluated.
        // 8. `GetHeight() < 1`.
        && entity.position.z < 1
        // The pre-CanAutoCloak `Contact_With_Whom(0)` exclusion at
        // 0x006FB7FD..0x006FB823 (`BuildingType+0x16BD` = `WeaponsFactory=`,
        // verified from the key string at 0x0081AA4C).
        && !destination_is_weapons_factory;

    // `TechnoClass::ShouldUncloak @ 0x006FBC90`:
    //   if ((IsCloakable() || +0x3D2) && !EMP && !vt+0x380 && !WarpIn && !WarpOut)
    //       return 0;
    //   if (rank CLOAK) return 0;
    //   return IsVisibleToHouse(myCell, myOwner) ? 0 : 1;
    // The tail therefore returns 1 in stock YR, so the predicate reduces to
    // "the object can no longer sustain its cloak" — EMP, chrono warp, a
    // pending deploy, `CloakStop=` while moving, or a lost stealth ability.
    // The `|| +0x3D2` disjunct is the raw stealth byte; see the two `+0x3D2`
    // residuals on `can_auto_cloak` above — `object.cloakable` inside
    // `is_cloakable` is that byte's stock seed, and the crate-granted and
    // `CloakStop=`-while-moving halves are both unreachable in stock data.
    let should_uncloak =
        if is_cloakable && !emp_active && !paralyzed && !deploy_pending && !chrono_active {
            false
        } else if rank_cloak {
            false
        } else {
            !cloaked_by_own_house
        };
    Some(crate::sim::cloak_disguise::CloakTickFacts {
        current_frame,
        state_zero_head_allows,
        can_auto_cloak,
        should_uncloak,
        health_above_red: health_strictly_above_condition_red(
            entity.health,
            object.strength,
            rules.general.condition_red,
        ),
        cloaking_speed: object.cloaking_speed,
        cloak_delay_frames: rules.general.cloak_delay_frames,
    })
}

/// `CellClass::IsVisibleToHouse @ 0x004870B0` for the object's own owner — the
/// `CloakedByHouses` bit read by `ShouldUncloak @ 0x006FBDA2`, `CanAutoCloak @
/// 0x006FBE90` and the vt+0x420 hook at `0x006F4F46`. Only a
/// `CloakGenerator=yes` building's field expand/contract writes it
/// (`BuildingClass::UpdateGapGenerator_Tick @ 0x004551B9 / 0x004553B3`), and no
/// stock YR building carries the key — so this is constantly false in an
/// ordinary skirmish, exactly as in gamemd.
fn owner_cloak_field_bit(sim: &Simulation, owner: InternedId, rx: u16, ry: u16) -> bool {
    let Some(index) = sim.base_reservation_house_index(owner) else {
        return false;
    };
    let Ok(index) = u8::try_from(index) else {
        return false;
    };
    sim.fog.is_cloaked_by_house(index, rx, ry)
}

fn sensor_targeters_in_native_dispatch_order(sim: &Simulation, cloaker_id: u64) -> Vec<u64> {
    let Some(cloaker) = sim.substrate.entities.get(cloaker_id) else {
        return Vec::new();
    };
    let cloaker_owner = cloaker.owner();
    let cloaker_cell = (cloaker.position.rx, cloaker.position.ry);

    // TechnoClass+0x420 @ 0x006F4EB0 reverse-scans g_TechnoClass_Array,
    // appends admitted targeters, then reverse-dispatches the saved vector.
    // The two reversals produce forward Techno construction order. VERA's
    // stable object IDs are monotonic construction IDs, so the EntityStore's
    // ordered Techno walk is the same order without copying the native arrays.
    sim.substrate
        .entities
        .iter_sorted()
        .filter_map(|(targeter_id, targeter)| {
            let targets_cloaker = targeter
                .attack_target
                .as_ref()
                .is_some_and(|target| target.target == TargetKind::Entity(cloaker_id));
            let admitted = targeter.owner() == cloaker_owner
                || sim
                    .fog
                    .has_sensor_for_house(targeter.owner(), cloaker_cell.0, cloaker_cell.1);
            (targets_cloaker && admitted).then_some(targeter_id)
        })
        .collect()
}

/// `TechnoClass` vtable `+0x420 @ 0x006F4EB0`, the callback every sensor
/// deposit runs over the residents of each covered cell (`AddSensorsAt @
/// 0x004DE7B0` and its three siblings).
///
/// The function has two arms, read from the disassembly:
/// * `0x006F4F05..0x006F4F3A` — if the object is fully cloaked, is not the
///   local player's, and the local player has no sensor on its cell, call
///   `ObjectClass::Deselect` (vt+0x150). Local-player UI only; not sim state.
/// * `0x006F4F3A..0x006F5085` — the **cloak-field entry hook**: if
///   `CellClass::IsVisibleToHouse(myCell, myOwner)` (the `CloakedByHouses` bit,
///   `0x004870B0`) AND `CanAutoCloak()`, snapshot the admitted targeters,
///   `StartCloaking(0)` (vt+0x460), then re-`Assign_Target` each saved one.
///
/// Nothing here ever uncloaks, and the second arm is dormant in stock YR
/// because no building sets `CloakGenerator=`.
///
/// **DRIFT corrected here.** VERA gated the second arm on
/// `fog.is_cell_visible(owner, ...)`, which is true for any owner standing on
/// its own revealed cell — so every sensor add/remove touching a cell force-
/// cloaked an eligible unit there and played `CloakSound`. With six stock
/// `SensorsSight=` types re-depositing on every cell they move through, that
/// fired continuously in naval play. The gate is now the real
/// `CloakedByHouses` bit, so the arm is dormant exactly as in gamemd; the
/// structure stays modelled for a mod that does ship a cloak generator.
pub(crate) fn sensor_reevaluate_stock_cloak(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
) -> SensorCloakReevaluation {
    let Some(facts) = stock_cloak_tick_facts(sim, id, rules) else {
        return SensorCloakReevaluation::default();
    };
    let inside_friendly_cloak_field = sim.substrate.entities.get(id).is_some_and(|entity| {
        owner_cloak_field_bit(sim, entity.owner(), entity.position.rx, entity.position.ry)
    });
    if !inside_friendly_cloak_field || !facts.can_auto_cloak {
        return SensorCloakReevaluation::default();
    }

    // Snapshot before StartCloaking, its positional sound, or any targeter
    // mutation. This is the DynamicVector transaction in 0x006F4EB0.
    let reassigned_targeters = sensor_targeters_in_native_dispatch_order(sim, id);
    let start = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.cloak.as_mut())
        .map(|cloak| cloak.start_cloaking_from_sensor(facts.current_frame, facts.cloaking_speed));
    if start.is_some_and(|start| start.play_sound) {
        emit_configured_cloak_sound(sim, id, rules);
    }
    let commits = assign_target_commits(&sim.substrate.entities, Some(TargetKind::Entity(id)));
    for &targeter_id in &reassigned_targeters {
        let targeter = sim
            .substrate
            .entities
            .get_mut(targeter_id)
            .expect("saved Techno targeter remains registered during sensor callback");
        represented_assign_target_admitted(targeter, Some(TargetKind::Entity(id)), commits);
    }
    SensorCloakReevaluation {
        cloak_transitioned: start.is_some_and(|start| start.transitioned),
        reassigned_targeters,
    }
}

/// `ObjectClass::Detach_All(false)` (vtable `+0xDC`, `0x005F5280`; the
/// `FootClass` override is `0x004D9720`) → `DispatchPointerExpiredCleanup @
/// 0x007258D0` with control 0 → `TechnoClass::PointerExpired @ 0x007077C0` on
/// every registered object, which `StartCloaking @ 0x00703770` runs before its
/// own state writes.
///
/// This is the SAME native body the UnInit broadcast runs, only with a
/// different control value, so it is routed through the one Rust model of it
/// (`Simulation::detach_all_pointer_expired`) rather than a cloak-local copy.
/// A miniature copy would have cleared the receiver's Target and nothing else —
/// no `RandomRanged(4, 8)` re-arm of the `+0x180/+0x188` targeting timer, no
/// NavCom/cargo/manager slot clears, and none of it for the non-targeters
/// `Detach_All` also visits.
///
/// The clause that matters most for cloak: the receiver's `Target(+0x2B4)` is
/// cleared through `Assign_Target(NULL)` unless
///
/// * `allowClear` was cancelled — the receiver's OWN house holds a sensor count
///   on the expiring object's cell (`0x00707994 CALL 0x004870D0`); or
/// * the expiring object has the same owner (`expired->vt+0x3C == my +0x21C`,
///   `0x007079B7..0x007079CB`).
///
/// `FootClass::PointerExpired @ 0x004D9960` then recomputes the SAME Boolean
/// from its own `0x004D9A57 CALL 0x004870D0` and gates the `+0x5A0`/`+0x5A4`
/// NavCom pair on it, so the sensing house keeps its destination too. Together
/// those mean a destroyer whose house covers the diving submarine keeps both
/// firing at it and closing on it, while everyone else loses target and chase
/// the instant the dive begins. Before this landed, VERA kept every attacker
/// locked on until the next passive-scan cadence (~28 frames) re-evaluated.
///
/// `RadioClass::PointerExpired @ 0x0065AAC0` is the counter-example: its slot
/// clear is control-1 only (`0065aaf0 TEST BL,BL / JZ`), so a dive breaks no
/// radio contact — the diving sub keeps its naval-yard repair and transport
/// links.
///
/// Running this after `CloakRuntime::tick` has written the new state instead of
/// before it is output-equivalent: the admission test reads only the cloaker's
/// cell, its owner and each receiver's house — never the cloak state.
fn detach_targeters_on_cloak(sim: &mut Simulation, cloaker_id: u64, rules: &RuleSet) {
    sim.detach_all_pointer_expired(cloaker_id, rules);
}

/// Clockwise-from-north neighbour offsets, native `g_DirectionOffsets`
/// (0x0089F688), indices 0..7 — the exact order `FootClass::PerCellProcess`
/// walks its eight neighbours.
const NEIGHBOUR_OFFSETS: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// `CellClass::Find_Nearest_Object @ 0x0047C3D0`, read from the disassembly for
/// the one callsite that matters here (`0x004D87DD`, which passes offset
/// `{0, 0}`, `alt = 0` and `exclude = NULL`):
///
/// ```text
/// dx7 = offset.X * 7;  dy7 = offset.Y * 7;
/// for (o = cell->FirstObject(+0xE4); o; o = o->NextObject(+0x30)) {
///   if (!(o->AbstractFlags(+0x14) & 1)) continue;   // Techno only
///   if (o == exclude) continue;
///   c  = o->GetCoords(vt+0x48);
///   d  = ftol(Sqrt_Approx(sq((c.X & 0xFF) - dx7) + sq((c.Y & 0xFF) - dy7)));
///   if (best == NULL || d < bestDistance) { best = o; bestDistance = d; }
/// }
/// ```
///
/// Three things this fixes over the previous "lowest stable id whose
/// `position` matches" scan:
///
/// * **The candidate set is the cell's object list, not every entity parked on
///   the coordinate.** `techno_limbo` never clears `entity.position`, so a
///   limboed object keeps a map position for the rest of the match; native
///   walks `CellClass::FirstObject`, which a limboed object is unlinked from.
///   `substrate.occupancy` is VERA's model of that list — the same authority
///   `sensor_lifecycle::sensor_residents_in_native_order` uses — and the
///   `MovementLayer::Ground` list is the `+0xE4` chain (`+0xE8`, the `alt`
///   chain, is selected only when `param_3 != 0`, and this callsite passes 0).
/// * **Selection is nearest-to-the-cell-origin, not lowest id.** With a
///   `{0, 0}` offset the distance is the object's own sub-cell lepton offset
///   from the cell's NW corner, through the retail `Sqrt_Approx` LUT and a
///   truncating `ftol` — so two objects whose distances truncate to the same
///   integer are decided by list order, and `<` keeps the earlier one.
/// * **The `+0x14` bit-0 test is the `AbstractFlags` *Techno identity* bit**
///   (`TechnoClass__Constructor @ 0x006F3228` ORs in 1;
///   `ObjectClass__Constructor @ 0x005F3B34` ORs in 2), not an on-map flag.
///   Every `GameEntity` is a Techno, so it needs no counterpart here.
pub(super) fn find_nearest_object_in_cell(sim: &Simulation, cell: (u16, u16)) -> Option<u64> {
    let occupancy = sim.substrate.occupancy.get(cell.0, cell.1)?;
    let mut best: Option<(u64, i32)> = None;
    for occupant in occupancy.iter_layer(MovementLayer::Ground) {
        let Some(other) = sim.substrate.entities.get(occupant.entity_id) else {
            continue;
        };
        let distance = crate::sim::cell_kernel::native_xy_distance(
            other.position.sub_x.to_num::<i32>(),
            other.position.sub_y.to_num::<i32>(),
        );
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((occupant.entity_id, distance));
        }
    }
    best.map(|(entity_id, _)| entity_id)
}

/// `FootClass::PerCellProcess @ 0x004D85D0`, the cell-enter (`param_2 == 2`)
/// arm at `0x004D8802..0x004D8829` — the ONLY consumer of `Sensors=`
/// (`TechnoTypeClass+0xC9D`) that is live in stock YR:
///
/// ```text
/// if (CloakState(+0x220) == 2)
///   for dir in 0..8 {
///     n = myCell + g_DirectionOffsets[dir];
///     if (!Is_Cell_In_Playfield(n, 1)) continue;
///     o = CellClass::Find_Nearest_Object(n, coord(0,0), 0);
///     if (o && !Is_Ally_ByObject(this, o)
///           && (oType->Sensors(+0xC9D) || o->HasWeaponAbility(0xC)))
///     { vt+0xFC(); break; }
///   }
/// ```
///
/// This is the "drive a Destroyer next to a moving submarine and it surfaces"
/// rule. Note what it is NOT: the stationary case is not covered — a Destroyer
/// parking beside a motionless submerged sub never forces it up; only the sub's
/// own cell entry can. What the Destroyer's `SensorsSight=` deposit does instead
/// is make the sub legal to target (the acquisition gate above) without touching
/// its cloak state.
///
/// One substitution, recorded rather than hidden: `HasWeaponAbility(0xC)` is
/// the SENSORS veteran/elite ability. No stock type lists it
/// (`grep 'Abilities=.*SENSORS' ini/rulesmd.ini` is empty), so only the
/// `Sensors=` type flag is consulted here.
pub(crate) fn uncloak_on_sensor_neighbour_after_cell_entry(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
) -> bool {
    let Some(mover) = sim.substrate.entities.get(id) else {
        return false;
    };
    if !mover
        .cloak
        .as_ref()
        .is_some_and(|cloak| cloak.is_fully_cloaked())
    {
        return false;
    }
    let owner = mover.owner();
    let owner_str = sim.interner.resolve(owner).to_owned();
    let (rx, ry) = (i32::from(mover.position.rx), i32::from(mover.position.ry));

    let mut triggered = false;
    for (dx, dy) in NEIGHBOUR_OFFSETS {
        let (nx, ny) = (rx + dx, ry + dy);
        if nx < 0 || ny < 0 {
            continue;
        }
        let (nx, ny) = (nx as u16, ny as u16);
        // Native passes mode 1 — the height-aware `MapClass::IsCellInPlayfield`
        // seam.
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(nx), i32::from(ny)),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
        ) {
            continue;
        }
        let Some(nearest) = find_nearest_object_in_cell(sim, (nx, ny)) else {
            continue;
        };
        let Some(other) = sim.substrate.entities.get(nearest) else {
            continue;
        };
        let other_owner_str = sim.interner.resolve(other.owner());
        if sim.fog.is_friendly(&owner_str, other_owner_str) || other.owner() == owner {
            continue;
        }
        let detects = rules
            .object(sim.interner.resolve(other.type_ref()))
            .is_some_and(|object| object.sensors);
        if !detects {
            continue;
        }
        triggered = true;
        break;
    }
    if !triggered {
        return false;
    }
    let cloaking_speed = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| rules.object(sim.interner.resolve(entity.type_ref())))
        .map_or(1, |object| object.cloaking_speed);
    let now = sim.session.binary_frame as i32;
    let surfaced = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.cloak.as_mut())
        .map(|cloak| cloak.start_uncloaking_from_sensor_neighbour(now, cloaking_speed));
    if surfaced.is_some_and(|result| result.play_sound) {
        emit_configured_cloak_sound(sim, id, rules);
    }
    surfaced.is_some_and(|result| result.transitioned)
}

fn emit_configured_cloak_sound(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(sound_name) = rules.general.cloak_sound.as_deref() else {
        return;
    };
    let Some(position) = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.position.clone())
    else {
        return;
    };
    sim.sound_events
        .push(crate::sim::world::SimSoundEvent::cloak_sound(
            sound_name.to_owned(),
            &position,
        ));
}

fn health_strictly_above_condition_red(
    health: crate::sim::components::Health,
    strength: i32,
    condition_red: f64,
) -> bool {
    health.compare_ratio(strength, condition_red)
        == crate::util::native_x87::MaskedX87Ordering::Greater
}

#[cfg(test)]
#[test]
fn original_health_ratio_corpus_populates_cloak_tick_facts() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    for row in crate::sim::health_ratio_fixture::rows() {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=SUB\n[SUB]\nStrength={}\nCloakable=yes\n",
            row.input.strength
        )))
        .unwrap();
        rules.general.condition_red = row.input.red();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("A");
        let type_ref = sim.interner.intern("SUB");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            owner,
            Health {
                current: row.input.current,
            },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.cloak = Some(crate::sim::cloak_disguise::CloakRuntime::new(0, 9));
        sim.substrate.entities.insert(entity);
        let facts = stock_cloak_tick_facts(&sim, 1, &rules).expect("cloakable unit facts");
        assert_eq!(
            facts.health_above_red, row.output.cloak_above_red,
            "{row:?}"
        );
    }
}

/// Produce the world-dependent virtual results consumed by
/// `TechnoClass::CloakingTick @ 0x006FB740`. Stock cloakable objects are Units;
/// the caller keeps this at the Unit Techno bracket head to preserve Scenario
/// RNG ordering relative to the rest of that object's AI visit.
pub(super) fn tick_stock_cloak_producer(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some((category, type_ref, veterancy)) = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| (entity.category, entity.type_ref(), entity.veterancy))
    else {
        return;
    };
    if category != EntityCategory::Unit {
        return;
    }
    let Some(object) = rules.object(sim.interner.resolve(type_ref)) else {
        return;
    };
    let rank_cloak =
        veterancy >= 100 && object.veteran_cloak || veterancy >= 200 && object.elite_cloak;
    if !object.cloakable && !rank_cloak {
        return;
    }

    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.cloak.is_none())
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        entity.cloak = Some(crate::sim::cloak_disguise::CloakRuntime::new(
            sim.session.binary_frame as i32,
            rules.general.cloaking_stages,
        ));
    }

    let Some(facts) = stock_cloak_tick_facts(sim, id, rules) else {
        return;
    };
    let result = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.cloak.as_mut())
        .map(|cloak| cloak.tick(facts, &mut sim.scenario_rng));
    if result.is_some_and(|result| result.began_cloaking) {
        // `StartCloaking @ 0x00703770` opens with `Detach_All(false)`.
        detach_targeters_on_cloak(sim, id, rules);
    }
    if result.is_some_and(|result| result.completed_cloak) {
        // The 1 → 2 completion at `0x006FBA98` snapshots the still-admitted
        // targeters (sensed-or-same-owner), runs `Detach_All(false)` again, then
        // re-`Assign_Target`s each saved one in reverse-of-reverse order. The
        // re-assign is a no-op for the pointer itself — `Assign_Target @
        // 0x006FCDB0` returns early on an unchanged target — but it still clears
        // each receiver's passive-acquire provenance byte `+0x50C` first, which
        // `represented_assign_target_admitted` reproduces.
        let retained = sensor_targeters_in_native_dispatch_order(sim, id);
        detach_targeters_on_cloak(sim, id, rules);
        let commits = assign_target_commits(&sim.substrate.entities, Some(TargetKind::Entity(id)));
        for targeter_id in retained {
            if let Some(targeter) = sim.substrate.entities.get_mut(targeter_id) {
                represented_assign_target_admitted(targeter, Some(TargetKind::Entity(id)), commits);
            }
        }
    }
    if result.is_some_and(|result| result.play_cloak_sound) {
        emit_configured_cloak_sound(sim, id, rules);
    }
}

#[cfg(test)]
#[path = "techno_ai_cloak_tests.rs"]
mod tests;
