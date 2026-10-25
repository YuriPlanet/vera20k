//! Stock Techno cloak producer after the Techno AI's managers and healing.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::veterancy::{has_weapon_ability, rank_from_u16};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::direction::DIRECTION_DELTAS;

use super::{FrameEffects, Simulation, UninitContext};
use crate::sim::world::FrameChannel;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::SimSoundEvent;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SensorCloakReevaluation {
    pub(crate) cloak_transitioned: bool,
    pub(crate) reassigned_targeters: Vec<u64>,
}

fn stock_cloak_tick_facts(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<crate::sim::cloak_disguise::CloakTickFacts> {
    let entity = sim.substrate.entities.get(id)?;
    // Foot vtables (Unit7F5C70, Infantry7EB058, Aircraft7E22A4) all
    // dispatch +410 to CloakingTick6FB740. Buildings use their own +410.
    if !matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    ) || entity.cloak.is_none()
    {
        return None;
    }
    let object = sim.interner.object_type(entity.type_ref(), rules)?;
    let rank_cloak = has_weapon_ability(entity.veterancy.rank_kind(), object, Ability::Cloak);
    let intrinsic_cloak = entity.stealth_ability.active(object.cloakable);
    if !intrinsic_cloak && !rank_cloak {
        return None;
    }
    // vt+0x1D4 / vt+0x1D8 (`0x0070C5B0` / `0x0070C5C0`, reading
    // `TechnoClass+0x270`/`+0x271`): a Chrono teleport or a Temporal warp.
    let chrono_active = entity.is_warped_out() || entity.is_warping_in();
    // `FootClass::IsCloakable @ 0x004DBDA0` (vtable +0x288) =
    // `HasStealthAbility() && !(CloakStop(+0xC93) && locomotor->Is_Moving_Now())`,
    // the locomotor's ILocomotion+0x80 at `0x004DBDDD`, asked only under
    // CloakStop.
    let is_cloakable = intrinsic_cloak
        && !(object.cloak_stop
            && crate::sim::movement::motion_query::is_moving_now(
                entity,
                crate::sim::movement::SpeedRules::new(rules, &sim.interner, &sim.houses),
                sim.session.binary_frame(),
            ));

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
    // Original active-YR producer reachability remains unproven: the
    // dormant TS EMPulse body alone does not establish an original scenario
    // caller. Ares's Bullet hook is a separate, established producer.
    // Ares ordinary EMP supplies this shared native+504 predicate through
    // the entity-owned counter. This does not prove a retail TS producer.
    let emp_active = entity.emp_remaining() > 0;
    let paralyzed = entity.parasite.is_paralyzed(sim.session.binary_frame());
    let state_zero_head_allows =
        is_cloakable && !emp_active && !paralyzed && !chrono_active || rank_cloak;

    // CloakingTick's pre-CanAuto destination exclusion is Contact_With_Whom(0)
    // resolving to a WeaponsFactory building (naval-yard repair contact), not
    // an arbitrary movement destination.
    let destination_is_weapons_factory = entity
        .radio_contacts
        .slot(0)
        .is_some_and(|contact| sim.is_weapons_factory(contact, rules));
    let current_frame = sim.session.binary_frame() as i32;
    let cloak_state = entity.cloak.as_ref().map_or(0, |cloak| cloak.state());
    let cloak_progress = entity.cloak.as_ref().map_or(0, |cloak| cloak.depth());

    // `CellClass::IsVisibleToHouse @ 0x004870B0` is the CloakedByHouses bit,
    // NOT cell visibility — see `FogState::is_cloaked_by_house`. It is only
    // ever set by a `CloakGenerator=yes` building's field, and stock YR has
    // none, so this reads false in every ordinary skirmish. VERA previously
    // substituted `fog.is_cell_visible(owner, ...)` here, which is true for an
    // owner standing on its own cell — inverting both gates below.
    let cloaked_by_own_house = owner_cloak_field_bit(
        sim,
        entity.owner(),
        entity.position.rx(),
        entity.position.ry(),
    );

    // TechnoClass::CanAutoCloak @006FBDC0 reads raw Techno+3D2:
    // the type seed and permanent crate grant share this capability owner.
    // Native also ORs raw +0x3D2 independently of vt+0x288: CloakStop can
    // suppress the moving Foot virtual without removing its retained ability.
    let can_auto_cloak = (is_cloakable || intrinsic_cloak || rank_cloak || cloaked_by_own_house)
        // 2. already fully cloaked.
        && cloak_state != 2
        // 3. ROF rearm countdown at +0x2EC/+0x2F4. Preserve native lazy order:
        // it rejects before the retained-target weapon/range query.
        && entity.rearm_timer.timer().expired(current_frame)
        // 4. `Target(+0x2B4) != 0 && CanFireAtTarget(vt+0x3AC)`.
        //    6FBEF4..6FBF01 calls the canonical selected-weapon/InRange owner
        //    (6F7780), not GetFireError. An ordinary Attack retains a distant
        //    target and must still permit cloak entry.
        //    Consumed original controls: tools/spatial_oracle/cloak_target_gate.
        && !entity.attack_target.target().is_some_and(|target| {
            sim.techno_can_fire_at_target(id, target, rules, registry)
        })
        // CanAutoCloak @006FBDC0 exempts Buildings from this progress gate;
        // all three Foot categories admitted above use the zero-progress gate.
        && cloak_progress == 0
        // 6. CloakDelay countdown at +0x240/+0x248, after the range query.
        && entity
            .cloak
            .as_ref()
            .is_some_and(|cloak| cloak.recloak_delay().expired(current_frame))
        // 7. `+0x2B0 && Foot && +0x6AD` (`0x006FBF57..0x006FBF7D`): an
        //    object a Magnetron holds aloft (`sim::locomotor_lift`). This
        //    producer admits Foot categories only, so the Foot test holds.
        && !(entity.lift.source().is_some() && entity.lift.swap_active())
        // 8. `GetHeight() < 1`.
        && crate::sim::movement::air_movement::current_fly_height(
            entity,
            sim.resolved_terrain.as_ref(),
        ) < 1;

    // `TechnoClass::ShouldUncloak @ 0x006FBC90`:
    //   if ((IsCloakable() || +0x3D2) && !EMP && !vt+0x380 && !WarpIn && !WarpOut)
    //       return 0;
    //   if (rank CLOAK) return 0;
    //   return IsVisibleToHouse(myCell, myOwner) ? 0 : 1;
    // The tail therefore returns 1 in stock YR, so the predicate reduces to
    // "the object can no longer sustain its cloak" — EMP, paralysis, chrono
    // warp, or a lost stealth ability. Neither this body
    // nor CloakingTick's head reads the Unit deployment flags.
    // The `|| +0x3D2` disjunct is the raw stealth byte;
    // Type seed and crate grant both survive moving with CloakStop.
    let should_uncloak =
        if ((is_cloakable || intrinsic_cloak) && !emp_active && !paralyzed && !chrono_active)
            || rank_cloak
        {
            false
        } else {
            !cloaked_by_own_house
        };
    Some(crate::sim::cloak_disguise::CloakTickFacts::new(
        current_frame,
        // CloakingTick6FB7FD..6FB823 applies this contact exclusion before
        // CanAutoCloak in state zero; the sensor callback has no such gate.
        state_zero_head_allows && !destination_is_weapons_factory,
        can_auto_cloak,
        should_uncloak,
        health_strictly_above_condition_red(
            entity.health,
            object.strength,
            rules.general.condition_red,
        ),
        object.cloaking_speed,
        rules.general.cloak_delay_frames,
        rules.general.cloaking_stages,
        object.invisible
            || (entity.category() == EntityCategory::Structure && object.invisible_in_game),
        entity.discovery.owned_by_current_house(),
        entity.category() == EntityCategory::Structure,
    ))
}

/// `CellClass::IsVisibleToHouse @ 0x004870B0` for the object's own owner — the
/// `CloakedByHouses` bit read by `ShouldUncloak @ 0x006FBDA2`, `CanAutoCloak @
/// 0x006FBE90` and the vt+0x420 hook at `0x006F4F46`. Only a
/// `CloakGenerator=yes` building's field expand/contract writes it
/// (`BuildingClass::UpdateGapGenerator_Tick @ 0x004551B9 / 0x004553B3`), and no
/// stock YR building carries the key — so this is constantly false in an
/// ordinary skirmish, exactly as in gamemd.
///
/// The bit is the owner's House array index (`HouseClass+0x30`). The AI asks
/// it of every object, so an owner the House array lacks (a test fixture's)
/// answers false rather than tripping the base-reservation lookup's assertion.
fn owner_cloak_field_bit(sim: &Simulation, owner: InternedId, rx: u16, ry: u16) -> bool {
    sim.houses
        .index_of(owner)
        .and_then(|index| u8::try_from(index).ok())
        .is_some_and(|index| sim.fog.is_cloaked_by_house(index, rx, ry))
}

fn sensor_targeters_in_native_dispatch_order(sim: &Simulation, cloaker_id: u64) -> Vec<u64> {
    let Some(cloaker) = sim.substrate.entities.get(cloaker_id) else {
        return Vec::new();
    };
    let cloaker_owner = cloaker.owner();
    let cloaker_cell = (cloaker.position.rx(), cloaker.position.ry());

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
                .target()
                .is_some_and(|target| target == TargetKind::Entity(cloaker_id));
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
/// 0x004DE7B0` and its three siblings), that the object's cell change runs
/// (`TechnoClass::PerCellTail @ 0x006F50FA`), and that the Techno AI runs
/// for an object on the wrong side of its owner's cloak field
/// ([`cloak_field_ai_step`]).
///
/// Both arms read the Cell of the object's GetCoords (vt+0x48,
/// `0x006F4EC0..0x006F4F03`):
/// * `0x006F4F05..0x006F4F3A` — a fully cloaked object (state 2) whose owner
///   is not the player's (`PlayerPtr`, `0x00A83D4C`, which must be set) is
///   deselected (vt+0x150) unless the player holds a sensor count on that
///   cell (`CellClass::SensorCountForHouse @ 0x004870D0`). So a selected
///   enemy submarine drops out of the player's selection when it finishes
///   diving out of the player's sonar, or when that sonar leaves.
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
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> SensorCloakReevaluation {
    let Some((owner, cell, fully_cloaked)) = sim.substrate.entities.get(id).and_then(|entity| {
        Some((
            entity.owner(),
            super::lifecycle::object_get_coords_cell(entity)?,
            entity
                .cloak
                .as_ref()
                .is_some_and(|cloak| cloak.state() == 2),
        ))
    }) else {
        return SensorCloakReevaluation::default();
    };
    let player = sim.session.current_house();
    if fully_cloaked
        && player.is_some_and(|player| player != owner)
        && !sensor_visible_to_player(sim, id)
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        entity.selection.deselect();
    }
    // 6F4F46 checks the field before invoking CanAutoCloak and its range query.
    if !owner_cloak_field_bit(sim, owner, cell.0, cell.1) {
        return SensorCloakReevaluation::default();
    }
    let Some(facts) = stock_cloak_tick_facts(sim, id, rules, registry) else {
        return SensorCloakReevaluation::default();
    };
    if !facts.can_auto_cloak {
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
        // Virtual `StartCloaking +0x460 @ 0x00703770` from the active
        // sensor-count resident callback `0x006F4EB0`, argument zero.
        .map(|cloak| cloak.start_cloaking(facts.current_frame, facts.cloaking_speed, false));
    if start.is_some_and(|start| start.transitioned) {
        start_cloaking_detach(sim, id, rules, registry, FrameEffects::default());
    }
    if start.is_some_and(|start| start.play_sound) {
        emit_configured_cloak_sound(&sim.substrate, &mut sim.sound_events, id, rules);
    }
    for &targeter_id in &reassigned_targeters {
        let _ = sim.assign_target_represented(
            targeter_id,
            Some(TargetKind::Entity(id)),
            rules,
            registry,
            FrameEffects::default(),
        );
    }
    SensorCloakReevaluation {
        cloak_transitioned: start.is_some_and(|start| start.transitioned),
        reassigned_targeters,
    }
}

/// `TechnoClass::IsSensorVisibleToPlayer @ 0x0070D420` (vt+0x328): the
/// player's house (`PlayerPtr`, `0x00A83D4C`) holds a sensor count
/// (`CellClass::SensorCountForHouse @ 0x004870D0`) on the Cell of the
/// object's GetCoords (vt+0x48). False without a player house.
pub(crate) fn sensor_visible_to_player(sim: &Simulation, id: u64) -> bool {
    let Some(player) = sim.session.current_house() else {
        return false;
    };
    sim.substrate
        .entities
        .get(id)
        .and_then(super::lifecycle::object_get_coords_cell)
        .is_some_and(|(rx, ry)| sim.fog.has_sensor_for_house(player, rx, ry))
}

/// `TechnoClass::AI_Update @ 0x006FA95B..0x006FA9D2`, after the spawn
/// manager: an uncloaked object (state 0) inside its owner's cloak field, or
/// a fully cloaked one (state 2) outside it, runs the sensor callback
/// ([`sensor_reevaluate_stock_cloak`]). Both read the `CloakedByHouses` bit
/// (`0x004870B0`) of the Cell of the object's GetCoords for its owner.
///
/// No retail building raises a cloak field, so in skirmish this is every
/// fully cloaked object on every AI visit: its sensor callback deselects it
/// unless the player owns it or senses its cell.
pub(super) fn cloak_field_ai_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) {
    // An object with no cloak runtime is in state 0 and cannot pass
    // CanAutoCloak (`stock_cloak_tick_facts`), so the callback would do
    // nothing for it; skipping it keeps the field lookup out of the AI loop.
    let Some((owner, cell, state)) = sim.substrate.entities.get(id).and_then(|entity| {
        Some((
            entity.owner(),
            super::lifecycle::object_get_coords_cell(entity)?,
            entity.cloak.as_ref()?.state(),
        ))
    }) else {
        return;
    };
    let in_field = || owner_cloak_field_bit(sim, owner, cell.0, cell.1);
    if (state == 0 && in_field()) || (state == 2 && !in_field()) {
        sensor_reevaluate_stock_cloak(sim, id, rules, registry);
    }
}

/// The world half of `TechnoClass::StartCloaking @ 0x00703770`, after
/// [`CloakRuntime::start_cloaking`](crate::sim::cloak_disguise::CloakRuntime)
/// accepted the transition:
/// * `Detach_All(false)` (vtable `+0xDC`, [`Simulation::detach_all`]), which
///   native runs before its state writes;
/// * `0x00703819..0x0070383C`: a selected object whose owner is not the
///   player's (`PlayerPtr`) is deselected (vt+0x150). The player's sensors
///   do not keep this one, unlike Detach_All's.
///
/// Detach_All(false)'s announcement reaches `TechnoClass::PointerExpired @
/// 0x007077C0` on every registered object with control 0. It is the SAME
/// native body the UnInit broadcast runs, only with a different control
/// value. A miniature copy would have cleared the receiver's Target and
/// nothing else — no `RandomRanged(4, 8)` re-arm of the `+0x180/+0x188`
/// targeting timer, no NavCom/cargo/manager slot clears, and none of it for
/// the non-targeters `Detach_All` also visits.
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
/// clear is control-1 only (`0065aaf0 TEST BL,BL / JZ`), so the announcement
/// breaks no radio contact. Only the Foot prelude's OVER_OUT to a contact 0
/// the owner is not allied with does; the diving sub keeps its naval-yard
/// repair and transport links.
///
/// Running this after `CloakRuntime` has written the new state instead of
/// before it is output-equivalent: Detach_All reads the cloaker's cell, its
/// owner, its contacts' owners, the player's sensors and each receiver's
/// house — never the cloak state.
fn start_cloaking_detach(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    sim.detach_all(
        id,
        false,
        UninitContext::new(rules, registry).with_effects(effects),
    );
    let player = sim.session.current_house();
    if let Some(entity) = sim.substrate.entities.get_mut(id)
        && player != Some(entity.owner())
    {
        entity.selection.deselect();
    }
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
///     if (o && !o->Owner->Is_Ally_ByObject(this)
///           && (oType->Sensors(+0xC9D) || o->HasWeaponAbility(0xC)))
///     { vt+0xFC(); break; }
///   }
/// ```
///
/// The neighbour's House asks about the mover (`0x004D87E8..0x004D87EF`).
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
    let (rx, ry) = (
        i32::from(mover.position.rx()),
        i32::from(mover.position.ry()),
    );

    let mut triggered = false;
    for (dx, dy) in DIRECTION_DELTAS {
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
        let Some(nearest) = sim.nearest_cell_object((nx, ny), MovementLayer::Ground, None) else {
            continue;
        };
        let Some(other) = sim.substrate.entities.get(nearest) else {
            continue;
        };
        if crate::sim::combat::combat_weapon::is_ally_by_object(
            Some(sim.house_alliances.as_map()),
            &sim.interner,
            other.owner(),
            owner,
        ) {
            continue;
        }
        let detects = sim
            .interner
            .object_type(other.type_ref(), rules)
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
        .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules))
        .map_or(1, |object| object.cloaking_speed);
    let now = sim.session.binary_frame() as i32;
    let surfaced = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.cloak.as_mut())
        // `FootClass::PerCellProcess @ 0x004D8829` invokes the `+0xFC`
        // `StartUncloaking(0)` wrapper (`0x00703850`) when a fully cloaked
        // mover enters a cell one of whose eight neighbours holds a
        // non-allied `Sensors=yes` object.
        .map(|cloak| {
            cloak.start_uncloaking(now, cloaking_speed, rules.general.cloaking_stages, false)
        });
    if surfaced.is_some_and(|result| result.play_sound) {
        emit_configured_cloak_sound(&sim.substrate, &mut sim.sound_events, id, rules);
    }
    surfaced.is_some_and(|result| result.transitioned)
}

/// The `CloakSound` of `StartCloaking @ 0x00703770` and `StartUncloaking @
/// 0x007036C0` when their suppression argument is zero: `[AudioVisual]
/// CloakSound=` (`Rules+0x6A0`) through `VocClass::PlayAt @ 0x007509E0` at
/// the object's coordinate. Every cloak transition's sound goes through here.
pub(crate) fn emit_configured_cloak_sound(
    substrate: &ObjectSubstrate,
    sound_events: &mut FrameChannel<SimSoundEvent>,
    id: u64,
    rules: &RuleSet,
) {
    let Some(sound_name) = rules.general.cloak_sound.as_deref() else {
        return;
    };
    let Some(position) = substrate.entities.get(id).map(|entity| entity.position) else {
        return;
    };
    sound_events.emit(crate::sim::world::SimSoundEvent::cloak_sound(
        sound_name.to_owned(),
        &position,
    ));
}

fn health_strictly_above_condition_red(
    health: crate::sim::combat::Health,
    strength: i32,
    condition_red: f64,
) -> bool {
    health.compare_ratio(strength, condition_red)
        == crate::util::native_x87::MaskedX87Ordering::Greater
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn original_health_ratio_corpus_populates_cloak_tick_facts() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
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
            Health::new(row.input.current),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.cloak = Some(crate::sim::cloak_disguise::CloakRuntime::new(0));
        sim.substrate.entities.insert(entity);
        let facts = stock_cloak_tick_facts(&sim, 1, &rules, None).expect("cloakable unit facts");
        assert_eq!(
            facts.health_above_red, row.output.cloak_above_red,
            "{row:?}"
        );
    }
}

/// Produce the world-dependent virtual results consumed by
/// `TechnoClass::CloakingTick @ 0x006FB740` for Foot objects;
/// the shared caller runs this after mission/targeting, Bomb, SlaveManager,
/// CaptureManager, IsAlive and healing (Techno6FA941..6FA946), before Spawn.
pub(super) fn tick_stock_cloak_producer(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some((category, type_ref, veterancy, stealth_ability)) =
        sim.substrate.entities.get(id).map(|entity| {
            (
                entity.category(),
                entity.type_ref(),
                entity.veterancy.rank(),
                entity.stealth_ability,
            )
        })
    else {
        return;
    };
    if !matches!(
        category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    ) {
        return;
    }
    let Some(object) = sim.interner.object_type(type_ref, rules) else {
        return;
    };
    let rank_cloak = has_weapon_ability(rank_from_u16(veterancy), object, Ability::Cloak);
    if !stealth_ability.active(object.cloakable) && !rank_cloak {
        return;
    }

    let frame = sim.session.binary_frame() as i32;
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        crate::sim::cloak_disguise::CloakRuntime::install_if_absent(&mut entity.cloak, frame);
    }

    let Some(facts) = stock_cloak_tick_facts(sim, id, rules, registry) else {
        return;
    };
    let result = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.cloak.as_mut())
        .map(|cloak| cloak.tick(facts, &mut sim.scenario_rng));
    if result.is_some_and(|result| result.began_cloaking) {
        start_cloaking_detach(sim, id, rules, registry, effects);
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
        sim.detach_all(
            id,
            false,
            UninitContext::new(rules, registry).with_effects(effects),
        );
        for targeter_id in retained {
            let _ = sim.assign_target_represented(
                targeter_id,
                Some(TargetKind::Entity(id)),
                rules,
                registry,
                effects,
            );
        }
        // `0x006FBC0B..0x006FBC32`: both completion paths then make a Unit
        // whose owner is not human-controlled (`HouseClass::IsControlledByHuman
        // @ 0x0050B730`) Scatter(null coordinate, forced, not no-kidding), so
        // a computer's freshly cloaked sub, dolphin or squid drifts off.
        let owner = sim.substrate.entities.get(id).map(|entity| entity.owner());
        if owner.is_some_and(|owner| !sim.owner_is_human(owner)) {
            sim.scatter(
                id,
                None,
                crate::sim::movement::ScatterFlags::new(true, false),
                rules,
                registry,
                effects,
            )
            .unwrap_or_else(|cause| panic!("cloak completion Scatter: {cause}"));
        }
    }
    if result.is_some_and(|result| result.play_cloak_sound) {
        emit_configured_cloak_sound(&sim.substrate, &mut sim.sound_events, id, rules);
    }
}

#[cfg(test)]
#[path = "techno_ai_cloak_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "techno_cloak_range_tests.rs"]
mod range_tests;

#[cfg(test)]
#[path = "techno_cloak_range_oracle_tests.rs"]
mod range_oracle_tests;
