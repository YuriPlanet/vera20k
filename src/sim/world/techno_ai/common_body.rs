//! The `TechnoClass::AI_Update` common body the per-category shell runs for
//! each live Techno: veterancy promotion, house and self heal, the target
//! drops and reach checks, the pre/post-mission bracket around the `+0xC4`
//! mission counter, and the passive-target clear on a mission change.

use super::FrameEffects;
use super::ObjectAiCtx;
use super::ObjectAiOutcome;
use super::ObjectSubstrate;
use super::Simulation;
use super::dispatch_foot_mission;
use super::passive_acquire_step;
use super::target_scan;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::houses::Houses;
use crate::sim::intern::StringInterner;
use crate::sim::mission::MissionType;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameChannel;
use crate::sim::world::SimSoundEvent;
use crate::util::forked::Forked;

/// The promotion detector of `TechnoClass::AI_Update @ 0x006FA054..0x006FA145`,
/// run once per live-object visit for every category (the common body is
/// reached from each leaf `AI`).
///
/// gamemd-derived: the rank cache (`+0x13C`) is compared with
/// `GetVeterancyLevel`; a crossing to elite seeds the flash timer (`+0xF0`,
/// `EliteFlashTimer`) for any owner and — for the local human's object — plays
/// `UpgradeEliteSound` positionally plus `EVA_UnitPromoted`; a crossing to
/// veteran plays `UpgradeVeteranSound` plus the EVA. The local-player gate is
/// the app layer's (`HouseClass::IsHumanPlayer @ 0x0050B6F0` is
/// `owner == g_PlayerPtr` in a skirmish). The flash's existing signed dword
/// counter is stepped later by4CC770 at6FAC4D, through flash_step; a
/// newly promoted object's timer therefore loses one on this same visit.
/// Ares replaces this whole block (`Simulation::update_ares_veterancy`).
/// The interner and the sound channel are written only by a promotion
/// (`util::forked`).
fn veterancy_promotion_step(
    interner: &mut Forked<StringInterner>,
    substrate: &mut ObjectSubstrate,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    sound_events: &mut Forked<FrameChannel<SimSoundEvent>>,
    id: u64,
    rules: &RuleSet,
) -> Option<crate::sim::combat::veterancy::Promotion> {
    let entity = substrate
        .entities
        .get_mut_if(id, |entity| entity.veterancy.rank_changed())?;
    let promotion = entity
        .veterancy
        .sample_promotion(&mut entity.flashing, rules.general.elite_flash_timer)?;
    let owner = entity.owner();
    // The promoted object's type is already interned on the entity; carry it
    // out so the accessibility line can name the game's own object without a
    // second lookup or a guessed translation. Transient fact only.
    let type_id = entity.type_ref();
    let world = crate::sim::movement::ground_pose::object_location(entity, terrain).into();
    let sound = match promotion {
        crate::sim::combat::veterancy::Promotion::Elite => {
            rules.general.upgrade_elite_sound.as_deref()
        }
        crate::sim::combat::veterancy::Promotion::Veteran => {
            rules.general.upgrade_veteran_sound.as_deref()
        }
    };
    let sound_id = sound.map(|name| interner.intern(name));
    sound_events.emit(super::SimSoundEvent::UnitPromoted {
        owner,
        sound_id,
        type_id,
        world,
        eva: None,
    });
    Some(promotion)
}

/// The house self-heal pulse of `TechnoClass::AI_Update`, read from the
/// disassembly. It is a second, independent heal beside [`self_heal_step`]'s
/// per-object `+1`, and it has **two arms** in one `AI_Update` body:
///
/// ```text
/// // units: 0x6FA7D2..0x6FA857
/// if (Type->Organic(+0xD97) != 0) goto infantry arm;
/// if (bl != 0) return;
/// if (Frame % Rules->SelfHealUnitFrames(+0x38)) return;
/// if (!House->HasUnitSelfHeal()) return;          // 0x0050D9D0: +0x168 > 0
/// step = House->GetUnitSelfHealStep();            // 0x0050D9F0: (+0x3C) * +0x168
/// Health += min(step, Strength - Health);         // then the smoke tail
///
/// // infantry: 0x6FA8C8..0x6FA93E
/// if (Type->Organic(+0xD97) == 0) return;
/// if (bl != 0) return;
/// if (Frame % Rules->SelfHealInfantryFrames(+0x30)) return;
/// if (!House->HasInfantrySelfHeal()) return;      // 0x0050D9C0: +0x164 > 0
/// step = House->GetInfSelfHealStep();             // 0x0050D9E0: (+0x34) * +0x164
/// Health += min(step, Strength - Health);
/// ```
///
/// `+0xD97` is `Organic=`, which the InfantryType constructor stores
/// (`0x00523911`) and the other classes leave clear, so the first arm serves
/// non-organic Units off the house unit count `UnitsGainSelfHeal` feeds,
/// and the second serves Infantry and organic Units off the infantry count
/// `InfantryGainSelfHeal` feeds. Both run after the per-object pulse and before
/// the cloak tick (`vt+0x410` at `0x006FA946`), so a same-frame heal is visible
/// to the cloak's health branch.
///
/// The unit arm's tail after the add (`0x6FA85A..0x6FA89D`) retires the damage
/// smoke when the object holds one; [`Simulation::retire_damage_smoke_after_self_heal`]
/// is that owner, which [`self_heal_step`] already drives.
pub(super) fn house_self_heal_step(
    interner: &StringInterner,
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    houses: &Houses,
    resolved_terrain: &Option<ResolvedTerrainGrid>,
    id: u64,
    rules: &RuleSet,
) {
    house_heal_arm(
        interner,
        session,
        substrate,
        houses,
        resolved_terrain,
        id,
        rules,
        false,
    );
    house_heal_arm(
        interner,
        session,
        substrate,
        houses,
        resolved_terrain,
        id,
        rules,
        true,
    );
}

/// One arm of [`house_self_heal_step`]. `infantry` selects the hospital arm:
/// its own class admission, frame key, house counter, amount key and predicate.
///
/// Admission, straight from `TechnoClass::AI_Update` (`0x006F9E50`):
///
/// ```text
/// 6FA7B9  cmp eax,1 ; jne 6FA8A2     ; the units arm is Unit-only
/// 6FA7D2  test cl,cl ; jne 6FA8A2    ; and skips an organic type (+0xD97)
/// 6FA7DA  test bl,bl ; jne 6FA8A2    ; and an at/over-strength or zero-Health object
/// 6FA8A9  cmp eax,0Fh ; je 6FA8D2    ; Infantry takes the hospital arm directly ...
/// 6FA8B5  cmp eax,1 ; jne 6FA941     ; ... which otherwise needs a Unit ...
/// 6FA8CE  test cl,cl ; je 6FA941     ; ... whose type is organic
/// 6FA8D2  test bl,bl ; jne 6FA941    ; the same guard as the units arm
/// ```
///
/// Only Infantry and Unit reach either arm — a Structure or an Aircraft
/// house-heals nothing, whatever its type's `Organic` says — and an
/// `Organic=no` Infantry still takes the hospital arm, because the Infantry
/// test at `0x6FA8A9` precedes the `Organic` read at `0x6FA8CE`.
///
/// `bl` is built at `0x006FA79A..0x006FA7B0` (`ebp` is zeroed at `0x006F9E58`):
/// it is set when `Health >= Type+0xA0` (`0x006FA7A2`, `jl` past the set) or when
/// `Health == 0` (`0x006FA7AC`), and either arm leaves before its heal block.
fn house_heal_arm(
    interner: &StringInterner,
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    houses: &Houses,
    resolved_terrain: &Option<ResolvedTerrainGrid>,
    id: u64,
    rules: &RuleSet,
    infantry: bool,
) {
    let Some(entity) = substrate.entities.get(id) else {
        return;
    };
    let Some(object) = interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    let strength = object.strength;
    let Some(count) = house_heal_count(
        houses,
        entity,
        object,
        session.binary_frame(),
        rules,
        infantry,
    ) else {
        return;
    };
    let amount = if infantry {
        rules.general.self_heal_infantry_amount
    } else {
        rules.general.self_heal_unit_amount
    };
    let step = amount.wrapping_mul(count);
    let Some(entity) = substrate.entities.get_mut(id) else {
        return;
    };
    let missing = strength.wrapping_sub(entity.health.current());
    let heal = if step < missing { step } else { missing };
    entity.health.heal(heal);
    // Only the unit arm carries the damage-smoke tail before the cloak tick;
    // the infantry arm rejoins the common flow at 0x006FA941.
    if !infantry {
        Simulation::retire_damage_smoke_after_self_heal(
            interner,
            substrate,
            resolved_terrain,
            id,
            rules,
        );
    }
}

/// The house count [`house_heal_arm`]'s `infantry` arm heals `entity` (of
/// type `object`) by at `frame`, or `None` when the arm passes it by. It only
/// reads, so a steady object's visit asks it too (`world::steady_ai`).
fn house_heal_count(
    houses: &Houses,
    entity: &GameEntity,
    object: &crate::rules::object_type::ObjectType,
    frame: u32,
    rules: &RuleSet,
    infantry: bool,
) -> Option<i32> {
    let unit = entity.category() == EntityCategory::Unit;
    let admitted = if infantry {
        entity.category() == EntityCategory::Infantry || (unit && object.organic)
    } else {
        unit && !object.organic
    };
    let health = entity.health.current();
    if !admitted || health >= object.strength || health == 0 {
        return None;
    }
    if !house_heal_arm_frame(frame, rules, infantry) {
        return None;
    }
    let count = houses.get(&entity.owner()).map_or(0, |house| {
        if infantry {
            house.self_heal_infantry()
        } else {
            house.self_heal_units()
        }
    });
    (count > 0).then_some(count)
}

/// Whether `frame` is a heal frame of [`house_heal_arm`]'s `infantry` arm
/// (`Frame % Rules->SelfHeal…Frames`).
fn house_heal_arm_frame(frame: u32, rules: &RuleSet, infantry: bool) -> bool {
    let interval = if infantry {
        rules.general.self_heal_infantry_frames
    } else {
        rules.general.self_heal_unit_frames
    };
    // Original6FA7EE/6FA8E2 divides before testing the House count. Missing
    // keys retain constructor1000. gamemd traps there on an authored period0
    // (and signed MIN/-1); VERA carries on: such a period is never due.
    (frame as i32).checked_rem(interval) == Some(0)
}

/// Whether `frame` is a heal frame of either arm of
/// [`house_self_heal_step`], which reads nothing else.
pub(in crate::sim::world) fn house_heal_frame(frame: u32, rules: &RuleSet) -> bool {
    house_heal_arm_frame(frame, rules, false) || house_heal_arm_frame(frame, rules, true)
}

/// Whether either arm of [`house_self_heal_step`] heals `entity` (of type
/// `object`) at `frame`. It only reads, so a steady object's visit asks it
/// too (`world::steady_ai`).
pub(in crate::sim::world) fn house_heals(
    houses: &Houses,
    entity: &GameEntity,
    object: &crate::rules::object_type::ObjectType,
    frame: u32,
    rules: &RuleSet,
) -> bool {
    [false, true]
        .into_iter()
        .any(|infantry| house_heal_count(houses, entity, object, frame, rules, infantry).is_some())
}

/// The self-heal pulse of `TechnoClass::AI_Update @ 0x006FA743..0x006FA757`:
/// when the eligibility virtual (`vtable+0x294` → `FUN_0070BE80`) holds,
/// `Health += 1` — a raw `INC` on `+0x6C`, no amount key. Ares adds its
/// `SelfHealing.*` amount instead.
///
/// The reached tail marks attached smoke done above ConditionYellow, retaining
/// the pointer until expiry. Building+6E6 and animation slots remain unchanged.
/// Original corpus: building_art_transition.json. After non-Greater health,
/// virtual1C8 is Object GetHeight5F5F40, which also retires smoke below -10
/// leptons relative to the live ground surface and explicit OnBridge offset.
pub(super) fn self_heal_step(
    interner: &StringInterner,
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    resolved_terrain: &Option<ResolvedTerrainGrid>,
    id: u64,
    rules: &RuleSet,
) {
    let Some(entity) = substrate.entities.get(id) else {
        return;
    };
    // As `rules.object(name)` answers; an id the interner never made names
    // nothing.
    let Some(object) = interner
        .try_resolve(entity.type_ref())
        .and_then(|_| interner.object_type(entity.type_ref(), rules))
    else {
        return;
    };
    // Ares 3.0 1004B820 (hook 0x6FA743) adds GetSelfHealAmount (10044E80)
    // in place of the eligibility virtual and its `+1`.
    let amount = if rules.ares() {
        crate::sim::combat::veterancy::ares_self_heal_amount(
            entity,
            object,
            session.binary_frame() as i32,
            rules.general.repair_rate_minutes,
        )
    } else {
        use crate::sim::combat::veterancy;
        // The period is an x87 conversion of a rules value, so only an
        // object past the virtual's ability gate, which answers false
        // without it, works it out (VERA-only order).
        i32::from(
            veterancy::holds_self_heal(entity, object)
                && veterancy::self_heal_eligible(
                    entity,
                    object,
                    session.binary_frame(),
                    veterancy::self_heal_interval_frames(rules.general.repair_rate_minutes),
                ),
        )
    };
    if amount == 0 {
        return;
    }
    if let Some(entity) = substrate.entities.get_mut(id) {
        entity.health.heal(amount);
    }
    Simulation::retire_damage_smoke_after_self_heal(
        interner,
        substrate,
        resolved_terrain,
        id,
        rules,
    );
}

/// `AircraftClass::AI @ 0x00414D2C..0x00414D4B`: a Target whose vt+0x54
/// answers true (IsHighFlying: Object `0x005F6B90`, Aircraft `0x0041B920`,
/// [`is_high_flying`](crate::sim::movement::air_movement::is_high_flying))
/// is dropped every frame through `Assign_Target(NULL)` (vt+0x3C8), and the
/// cloaked-target test below is skipped (`JMP 0x00414DA1`). Returns whether
/// it dropped one. A Cell's slot (`0x00410530`) answers false, and so does a
/// Terrain object on the ground.
pub(super) fn drop_airborne_target_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    effects: &mut FrameEffects<'_>,
) -> bool {
    let Some(crate::sim::combat::TargetKind::Entity(target_id)) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.attack_target.target())
    else {
        return false;
    };
    if !sim.substrate.entities.get(target_id).is_some_and(|target| {
        crate::sim::movement::air_movement::is_high_flying(
            target,
            sim.resolved_terrain.as_ref(),
            Some((rules, &sim.interner)),
        )
    }) {
        return false;
    }
    let _ = sim.assign_target_represented(id, None, rules, effects);
    true
}

/// `AircraftClass::AI @ 0x00414D4D..0x00414DA1`, read from the disassembly:
///
/// ```text
/// T = Target(+0x2B4);
/// if (T && (T->flags & 1) && !IsAlliedWith(myOwner, T)
///       && T->CloakState(+0x220) == 2
///       && !T->vt+0x32C(myOwner))                 // IsSensedByHouse
///     Assign_Target(NULL);
/// ```
///
/// The alliance is the aircraft's house asking (`0x00414D63`,
/// `HouseClass::IsAlliedWith(ObjectClass*) @ 0x004F9AF0`).
///
/// Aircraft alone re-test this every tick, so a Harrier loses its lock the
/// moment its house's sensor coverage of a submerged target lapses — the ground
/// units around it keep theirs until their own scan cadence comes round.
/// (`vt+0x32C` at `0x0070D460` has no function boundary in the database; the
/// body reads the sensor count for the passed house at the object's own cell,
/// and returns false for a null house.)
pub(super) fn drop_unsensed_cloaked_target_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    effects: &mut FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(crate::sim::combat::TargetKind::Entity(target_id)) = entity.attack_target.target()
    else {
        return;
    };
    let owner = entity.owner();
    let Some(target) = sim.substrate.entities.get(target_id) else {
        return;
    };
    if !target
        .cloak
        .as_ref()
        .is_some_and(|cloak| cloak.is_fully_cloaked())
    {
        return;
    }
    let target_cell = (target.position.rx(), target.position.ry());
    if sim
        .house_alliances
        .is_allied(&sim.interner, owner, target.owner())
    {
        return;
    }
    if sim
        .fog
        .has_sensor_for_house(owner, target_cell.0, target_cell.1)
    {
        return;
    }
    let _ = sim.assign_target_represented(id, None, rules, effects);
}

/// Tick the common `+0xC4` per-mission AI counter once per object visit
/// (`0x006FA646..0x006FA64F`), then enter `MissionClass::AI @ 0x005B3060`
/// (`0x006FA655`), whose first call is `ObjectClass::AI`'s sound block.
///
/// Returns false only when the fall failed. An object the fall kills (a unit
/// on a destroyed bridge) runs on: `MissionClass::AI` skips its handler on
/// IsAlive and Health (`0x005B306C`, `0x005B30A7`), but `AI_Update` still
/// runs the passive scan (`0x006FA65A..0x006FA6EE`), the bomb tick
/// (`0x006FA6F5..0x006FA712`) and the Slave and Capture managers
/// (`0x006FA717..0x006FA730`) before its IsAlive test at `0x006FA735`.
pub(super) fn mission_counter_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: &mut ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) -> bool {
    sim.substrate.entities.count_mission_ai_visit(id);
    // Ares' `ObjectClass_Update_AlphaLight` hooks ObjectClass::AI's entry.
    if rules.ares() {
        sim.update_alpha_shape(id, crate::sim::world::substrate::ObjectKind::Entity, rules);
    }
    // The sound block (`0x005F3E78..0x005F3F0C`) runs before the fall block
    // (`0x005F3F11`), so a faller its landing kills still plays its sounds.
    sim.object_ai_sounds(id, rules, &mut ctx.effects);
    // A quiet unit is not falling (`world::quiet_units`).
    if sim.quiet(id) {
        return true;
    }
    match sim.object_ai_fall(id, rules, ctx.overlay_registry, &mut ctx.effects) {
        Ok((landed, changed)) => {
            outcome.landed |= landed;
            outcome.bridge_state_changed |= changed;
        }
        Err(error) => {
            outcome.frame_error = Some(error);
            return false;
        }
    }
    true
}

// ===== TechnoClass common-body bracket =====
//
// Per live Unit, gamemd's `TechnoClass::AI_Update` body is one contiguous
// bracket: pre-mission block -> +0xC4/Mission work -> post-mission block, with
// two IsAlive early-returns (after the pre-block, after dispatch). The mission
// work at the dispatch point increments the counter and runs absorbed mission
// handlers. The Unit/Infantry entry Ready/Commence precedes this bracket;
// post-movement promotion stays with the corresponding leaf owner.

/// `ObjectClass+0x90` IsAlive ([`GameEntity::is_object_alive`]), which
/// `TechnoClass::AI_Update` tests after the rocking update when `vt+0x298`
/// answers true (`0x006FA23C`) and again after the bomb and the managers
/// (`0x006FA735`), and `UnitClass::AI` before its fire update (`0x007365BB`).
/// A crashing wreck, an infantry corpse on its death sequence and an authored
/// zero-HP building keep it at Health 0: none may skip its terminal AI by
/// deriving this flag from Health.
/// `MissionClass::AI`'s gate ([`GameEntity::mission_handler_due`]) still
/// requires positive Health.
///
/// [`GameEntity::is_object_alive`]: crate::sim::game_entity::GameEntity::is_object_alive
/// [`GameEntity::mission_handler_due`]: crate::sim::game_entity::GameEntity::mission_handler_due
pub(crate) fn ai_alive(substrate: &ObjectSubstrate, id: u64) -> bool {
    substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.is_object_alive())
}

/// `TechnoClass::AI_Update`'s leading common steps, in native order: the
/// Gattling loop position (`0x006F9EA8`), Iron Curtain tint stage
/// (`0x006F9EAF`, [`iron_tint_step`]), the berserk
/// countdown (`0x006F9F0D`, [`berserk_step`]), the
/// promotion sample (`0x006FA054`) and drain blocks (`0x006FA14B..0x006FA224`),
/// followed by rocking, target validity, charge turret and the Door clock.
/// Returns false when rocking destroyed the object. Managers, self-healing
/// and cloak belong to [`techno_after_acquisition`], after mission dispatch
/// and passive targeting, rather than this pre-mission block.
pub(super) fn techno_common_steps(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
    turret_rotate_sample: Option<bool>,
) -> bool {
    // Ares TechnoClass_Update1004B348..1004B397 converts for the current
    // surface before the same visit updates self-attached effects1004B434.
    // The hook reads the type before converting and keeps it for the
    // PassengerTurret step after.
    if rules.ares() {
        let hook_type = sim
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.type_ref());
        sim.update_ares_surface_conversion(id, rules, effects);
        if let Some(object) =
            hook_type.and_then(|type_ref| sim.interner.object_type(type_ref, rules))
            && let Some(entity) = sim.substrate.entities.get_mut(id)
        {
            entity.update_passenger_turret(object);
        }
        // Ares1004B320 updates a PoweredBy object's power source next. Its
        // TechnoExt::Destroy kills the object; native continues the hook's
        // extension updates on the corpse, which this visit skips instead.
        if !sim.update_ares_powered_by(id, rules, overlay_registry, effects)
            || !ai_alive(&sim.substrate, id)
        {
            return false;
        }
        // Ares1004B434 updates attachments before1004B528 EMP recovery.
        sim.update_ares_attach_effects(id, rules);
    }
    // Phobos `TechnoExt::UpdateAttachEffects` (Body.Update.cpp:895): one AI
    // phase and the animation check for every AttachEffect on this object.
    #[cfg(feature = "phobos")]
    sim.update_phobos_attach_effects_with(id, rules, effects, overlay_registry);
    if rules.ares() {
        sim.update_ares_operator_operability(id, rules, effects);
        // Installed6F9E50 ->1004B320 owns delayed Building reactivation before
        //the stock head. EMP expiry itself must not invent a future-power sweep.
        sim.update_ares_emp_operability(id, rules, effects);
        // Ares1004B320 then stops a deactivated Unit's driving and unjams a
        // deactivated jammer or updates an active one.
        sim.stop_ares_deactivated_unit_driving(id, rules, overlay_registry, effects);
        sim.update_ares_radar_jammer(id, rules);
        sim.update_ares_driver_killed_mission(id, rules, effects);
        // Ares1004B64B..1004B658 follows the extension updates and precedes
        // the original common head, consuming a ready displayed bounty sum.
        sim.flush_bounty_display(id, rules, effects, false);
    }
    // Techno6F9E70..6F9EA8 re-drives +4A4 at Location before mission
    // Stop/Release. A zero gain must stop a live loop before Guard can
    // release it to decay at the previous gain.
    if let Some(entity) = sim.substrate.entities.get(id)
        && sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.is_gattling)
    {
        effects.update_looping_sound(
            sim,
            rules,
            crate::sim::combat::gattling::gattling_sound_owner(id),
            Simulation::movement_sound_world(entity),
        );
    }
    iron_tint_step(&sim.session, &mut sim.scenario_rng, &mut sim.substrate, id);
    // Techno6F9EBB follows the tint head and precedes veterancy/drain work.
    // A queued voice probes its handle only when this actor really reaches
    // this slot; recording final Logic membership would lose early exits.
    sim.record_unit_voice_visit(id);
    berserk_step(sim, id, rules, effects);
    // A quiet unit holds its rank, drains nothing and is not drained, does
    // not rock, recoil or open a door (`world::quiet_units`).
    let quiet = sim.quiet(id);
    // Techno6F9FA9 follows the voice head and precedes promotion6FA054.
    // Building Update froze this sample before Temporal and its common AI.
    if let Some(sample) = turret_rotate_sample {
        sim.visit_turret_rotate_sound(id, sample, rules, effects);
        // Techno6FA027..6FA04F: every Turret-enabled visit re-drives
        // +488 at the live Location after its controls and latch writes.
        // A zero gain must stop it before this visit's later C4 expiry,
        // or another object's hit, can release a still-audible decay.
        if let Some(entity) = sim.substrate.entities.get(id)
            && sim
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.has_turret)
        {
            effects.update_looping_sound(
                sim,
                rules,
                super::turret_rotate_sound::turret_rotate_sound_owner(id),
                Simulation::movement_sound_world(entity),
            );
        }
    }
    if rules.ares() {
        // Ares TechnoClass_Update_Veterancy1004F2E0 (hook6FA054..6FA14B).
        sim.update_ares_veterancy(id, false, true, rules, effects);
    } else if !quiet
        && let Some(promotion) = veterancy_promotion_step(
            &mut sim.interner,
            &mut sim.substrate,
            sim.resolved_terrain.as_ref(),
            &mut sim.sound_events,
            id,
            rules,
        )
    {
        // Wasm module `Promoted`, `value` the new rank (1 veteran, 2 elite).
        let rank = match promotion {
            crate::sim::combat::veterancy::Promotion::Veteran => 1,
            crate::sim::combat::veterancy::Promotion::Elite => 2,
        };
        sim.raise_module_promoted(rules, effects, id, rank);
    }
    if !quiet {
        crate::sim::credit_income::drain_common_step(sim, id, rules);
    }
    // Techno6FA224..6FA244: only a valid loaded main VXL reaches
    // RockingUpdate70B570 and its immediate Alive guard. This precedes
    // locomotor Process and the Foot parasite tail; no later global update
    // may erase a parasite's direct sideways-angle write.
    let rocking_type = sim
        .substrate
        .entities
        .get(id)
        .filter(|_| !quiet)
        .and_then(|entity| {
            let object = sim.interner.type_handle(entity.type_ref(), rules)?;
            rules
                .object_has_loaded_voxel(object)
                .then(|| rules.object_by_handle(object))
        });
    if let Some(object) = rocking_type {
        let needs_self_damage = sim
            .substrate
            .entities
            .get_mut_if(id, crate::sim::rocking::updates)
            .is_some_and(|entity| {
                crate::sim::rocking::update(
                    entity,
                    object,
                    rules.general.fallback_coefficient,
                    sim.session.binary_frame(),
                )
            });
        if needs_self_damage {
            // RockingUpdate70BC23..70BCA3 calls the actual receiver with
            // type Strength, Rules C4Warhead, ignore-defenses and arg6=false.
            // Commit after releasing the entity borrow, before the Alive guard.
            let c4 = sim.interner.intern(&rules.bridge_warheads.c4_name);
            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                object.strength,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                c4,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: false,
                },
            );
            sim.commit_direct_damage_receiver(rules, overlay_registry, event, effects);
        }
        if !ai_alive(&sim.substrate, id) {
            return false;
        }
    }
    behind_indicator_step(sim, id, rules);
    allied_target_drop_step(sim, id, rules);
    illegal_target_drop_step(sim, id, rules);
    // Asked again: the steps since may have written the unit.
    if !sim.quiet(id)
        && let Some(entity) = sim
            .substrate
            .entities
            .get_mut_if(id, GameEntity::voxel_recoil_moves)
    {
        entity.update_voxel_recoil(); // Techno6FA4D1, before ChargeTurret.
    }
    charge_turret_step(&sim.interner, &sim.session, &mut sim.substrate, id, rules);
    //Techno6FA5BE..6FA5D6, before Mission AI: every Techno finishes its own
    //due Door transition here. Gate/Factory mission work observes this change
    //inside the same LogicVector visit; no global phase advances Door clocks.
    // Asked again after ChargeTurret, which may write the unit.
    let frame = sim.session.binary_frame();
    if !sim.quiet(id)
        && let Some(entity) = sim
            .substrate
            .entities
            .get_mut_if(id, |entity| entity.door_advances(frame))
    {
        entity.advance_door(frame);
    }
    true
}

/// `TechnoClass::AI_Update`'s berserk block (`0x006F9F0D..0x006F9F6E`),
/// after the voice probe: a berserk object counts its time down
/// ([`BerserkState::count_down`](crate::sim::combat::BerserkState::count_down)).
/// Once it calms down its target goes (`vt+0x3C8(0)`), and it queues Guard if
/// its house is a human player's (`IsHuman`, House `+0x1EC`), else Hunt
/// (`vt+0x1E8`). No draws. The Psychedelic arm (`combat::receiver_health`)
/// sets the state.
pub(super) fn berserk_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    effects: &mut FrameEffects<'_>,
) {
    let Some(entity) = sim
        .substrate
        .entities
        .get_mut_if(id, |entity| entity.berserk.is_active())
    else {
        return;
    };
    if !entity.berserk.count_down() {
        return;
    }
    let owner = entity.owner();
    let _ = sim.assign_target_represented(id, None, rules, effects);
    let mission = if sim.houses.get(&owner).is_some_and(|house| house.is_human()) {
        MissionType::Guard
    } else {
        MissionType::Hunt
    };
    let _ = sim.mission_queue_exact(
        id,
        crate::sim::mission::MissionId::from_known(mission),
        0,
        sim.session.binary_frame(),
        &crate::sim::mission::authority::LiveReadyInputProvider { rules },
    );
}

/// `TechnoClass::UpdateIronTint` (`0x006F9EAF`), the first step of
/// `TechnoClass::AI_Update` after the hover flag and the Gattling sound: a
/// curtained object's tint stage, whose stage 2 draws from the Scenario
/// stream ([`InvulnerabilityState::update_tint`]).
///
/// Next, UpdateAirstrikeTint70E920 (call6F9EB6) steps a marked building,
/// consuming its jitter draw after the Iron draw on the same Scenario stream.
/// The stream is written only by a stepped tint (`util::forked`).
///
/// [`InvulnerabilityState::update_tint`]: crate::sim::superweapon::invulnerability::InvulnerabilityState::update_tint
pub(super) fn iron_tint_step(
    session: &ScenarioSession,
    scenario_rng: &mut Forked<SimRng>,
    substrate: &mut ObjectSubstrate,
    id: u64,
) {
    let frame = session.binary_frame() as i32;
    // UpdateAirstrikeTint70E920 admits only a building, and another class's
    // stage never leaves 0 (`AirstrikeTintState::update_tint`), so the step
    // passes it by the category column without reading its record
    // (VERA-only; the same results).
    let building = substrate.entities.category_of(id) == Some(EntityCategory::Structure);
    let airstrike_active = building && substrate.airstrike_tint_active(id);
    // A quiet unit's curtain is gone or spent (`world::quiet_units`).
    if !substrate.entities.is_quiet(id)
        && let Some(curtain) = substrate
            .entities
            .get_mut_if(id, |entity| {
                entity
                    .invulnerability
                    .as_ref()
                    .is_some_and(|curtain| curtain.updates(frame))
            })
            .and_then(|entity| entity.invulnerability.as_mut())
    {
        curtain.update_tint(frame, scenario_rng);
    }
    if building
        && let Some(entity) = substrate
            .entities
            .get_mut_if(id, |entity| entity.airstrike_tint.updates(airstrike_active))
    {
        entity
            .airstrike_tint
            .update_tint(frame, airstrike_active, scenario_rng);
    }
}

/// Techno6FA4FB follows target validity and precedes mission dispatch. The
/// same object's later shot arms its charge duration for the next AI visit.
fn charge_turret_step(
    interner: &StringInterner,
    session: &ScenarioSession,
    substrate: &mut ObjectSubstrate,
    id: u64,
    rules: &RuleSet,
) {
    let Some(object) = substrate
        .entities
        .get(id)
        .and_then(|entity| interner.object_type(entity.type_ref(), rules))
        .filter(|object| GameEntity::charges_turret(object))
    else {
        return;
    };
    if let Some(entity) = substrate.entities.get_mut(id) {
        entity.update_charge_turret(session.binary_frame() as i32, object);
    }
}

/// `TechnoClass::AI_Update @ 0x006FA472..0x006FA4CB`, after the drain
/// blocks: on every sixteenth frame an object with a target asks GetFireError
/// without range (vt+0x3BC, `SelectWeapon(Target)`) and drops the target on
/// ILLEGAL (5) or CANT (6), unless its current mission is Capture (8) or
/// Sabotage (0x11). A Unit's and an Infantry's own fire routines keep an
/// illegal target; this is where they let go of it.
pub(super) fn illegal_target_drop_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::combat::{TargetKind, fire_error::FireError};
    if !illegal_target_drop_frame(sim.session.binary_frame()) {
        return;
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(target) = entity.attack_target.target() else {
        return;
    };
    // A target that no longer resolves never reaches this check natively
    // (`PointerExpired` clears it first); the Attack handler owns VERA's
    // clear of such a stale target.
    if matches!(target, TargetKind::Entity(_))
        && crate::sim::combat::resolve_target_coords(&target, sim).is_none()
    {
        return;
    }
    if matches!(entity.mission.current().raw(), 8 | 0x11) {
        return;
    }
    let weapon = target_scan::select_weapon(sim, rules, id, Some(target));
    // Ares TechnoClass_Update_ZeroOutTarget (10056D80, hook 0x6FA4C6) skips
    // the clear for an Aircraft, resuming at 0x6FA4D1.
    if rules.ares()
        && sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Aircraft)
    {
        return;
    }
    let code = target_scan::fire_error_at(sim, rules, id, Some(target), weapon, false);
    if matches!(code, FireError::Illegal | FireError::Cant) {
        //6FA4CB dispatches class+3C8 before this object's mission handler:
        // Infantry51B1F0 clears68D/requests its action and invalidates5E0.
        sim.assign_target_represented(id, None, rules, &mut FrameEffects::default())
            .expect("the queried target-drop receiver remains present");
    }
}

/// Whether [`illegal_target_drop_step`] asks at `frame`: every sixteenth
/// frame (`0x006FA472`).
pub(in crate::sim::world) fn illegal_target_drop_frame(frame: u32) -> bool {
    frame.is_multiple_of(16)
}

/// `TechnoClass::AI_Update @ 0x006FAAEF..0x006FABB8`, after the spawn
/// manager: a Foot outside a team that holds a target it cannot reach, or an
/// open-topped transport's rider (`+0x82`), approaches the target, then lets
/// go of one it still cannot hit unless that left it a destination.
///
/// - The block skips an aircraft (vt+0x2C == 2), an object without a target,
///   a team member (vt+0x290 = `0x0041C050`, Foot `+0x5D4`) and a Foot with a
///   NavCom (`+0x5A4`). A building answers reachable (vt+0x2C4 =
///   `0x00459D90`) and is no rider, so it never acts here.
/// - A Foot asks IsInSameZoneAs (vt+0x2C4,
///   [`Simulation::foot_is_in_same_zone_as`]). A reachable target held by an
///   object outside a transport ends the block (`0x006FAB52`).
/// - Otherwise the Foot runs Approach_Target(0) (vt+0x53C at `0x006FAB74`,
///   [`Simulation::approach_target`]). With a NavCom then, the block ends;
///   without, the target it then holds is measured with CanFireAt (vt+0x3A8)
///   and the weapon SelectWeapon picks for it (vt+0x2E4), and a miss drops it
///   (vt+0x3C8(NULL), `0x006FABB2`). CanFireAt answers yes for no target
///   (`0x006F77BB`).
///
/// A rider never chases (`0x004D5782`) and is refused any destination
/// (`0x004D94D7`): out of range on its Guard (every rider:
/// `SetInOpenTransport` puts it there), Approach drops the target and the
/// destination (`0x004D571F..0x004D5744`); on Attack, a human's Area Guard or
/// a computer's Hunt, Approach goes on to its search, and the block's
/// CanFireAt drops the target. In range, Approach returns at `0x004D5A34`.
///
/// The block draws nothing itself. Approach draws through PlaceInfantryInCell
/// (`0x00481180`): a `CloseRange=` infantryman's spot requests, and the
/// destination setter of a Teleport or Jumpjet infantryman, which picks his
/// spot in the cell it is given. The block has no Health gate: a Die1/Die2
/// corpse, still IsAlive, runs it too.
///
/// RESIDUAL: a unit rider runs the head of `UnitClass::Approach_Target @
/// 0x007414E0` ([`Simulation::unit_approach_head`]), whose crush and
/// BalloonHover arms take the target as the destination through
/// `UnitClass::Set_Destination @ 0x00741970`, unreviewed for a rider ahead of
/// the Foot refusal. Trigger: a modded unit rider with `Crusher=`,
/// `OmniCrusher=` or `BalloonHover=`; retail's only unit riders, the Terror
/// and Chaos Drones, have none. Effect: none in retail.
pub(super) fn target_reach_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) {
    use crate::sim::combat::TargetKind;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(target) = entity.attack_target.target() else {
        return;
    };
    if !target_reach_asks(sim, id, entity) {
        return;
    }
    // A target that no longer resolves is cleared natively (`PointerExpired`)
    // before this block; the Attack handler owns VERA's clear of it.
    if matches!(target, TargetKind::Entity(_))
        && crate::sim::combat::resolve_target_coords(&target, sim).is_none()
    {
        return;
    }
    let rider = entity.passenger_role().in_open_transport();
    // A rider asks too (`0x006FAB48`), though only `+0x82` decides for it.
    // The query needs the map's cells, zones, bounds and size and a Foot's
    // locomotor, which a loaded scenario always has; a fixture without them
    // cannot ask and answers reachable.
    let reachable = match sim.foot_is_in_same_zone_as(id, target, rules) {
        Ok(reachable) => reachable,
        Err(cause) => {
            log::debug!("object {id} cannot ask IsInSameZoneAs: {cause}");
            true
        }
    };
    if reachable && !rider {
        return;
    }
    sim.approach_target(id, rules, overlay_registry, effects);
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.navigation.nav_com().is_some() {
        return;
    }
    let Some(target) = entity.attack_target.target() else {
        return;
    };
    let weapon = target_scan::select_weapon(sim, rules, id, Some(target));
    if target_scan::can_fire_at(sim, rules, id, target, weapon) {
        return;
    }
    let _ = sim.assign_target_represented(id, None, rules, effects);
}

/// Whether [`target_reach_step`] asks about the target `entity` (`id`)
/// holds: a Unit or Infantry outside a team with no NavCom. A steady
/// object's visit asks it too (`world::steady_ai`).
pub(in crate::sim::world) fn target_reach_asks(
    sim: &Simulation,
    id: u64,
    entity: &GameEntity,
) -> bool {
    matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry
    ) && sim.team_script_vm.team_of(id).is_none()
        && entity.navigation.nav_com().is_none()
}

/// `TechnoClass::AI_Update @ 0x006FA30C..0x006FA46C`, every frame after the
/// drain blocks: a computer house's object lets go of a target its house is
/// allied with (`HouseClass::Is_Ally_ByObject @ 0x004F9AF0`). The deciding
/// house is the controller's when the object rides a mind-controlled
/// `OpenTopped=` transport (`+0x11C`, `+0x5E4`, `+0x2C0`), else its own; a
/// human house never drops (`0x0050B730`). It keeps the target when
/// - it is an infantryman and the target a building that would admit it as an
///   occupant (`0x00457CE0`);
/// - it is an `Engineer=` infantryman (`+0xEC3`);
/// - its weapon 1's warhead is `ElectricAssault=` (`+0x158`) and the target a
///   building whose type is `Overpowerable=` (`+0x1575`);
/// - it is berserk (`+0x298`).
///
/// Otherwise Assign_Target(NULL). No draw, no retarget.
pub(super) fn allied_target_drop_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    if !allied_target_drops(sim, id, rules) {
        return;
    }
    //6FA46C precedes the subsequent live target re-read and illegal test.
    // Preserve class refusal and synchronous action/path effects.
    sim.assign_target_represented(id, None, rules, &mut FrameEffects::default())
        .expect("the queried allied-target receiver remains present");
}

/// Whether [`allied_target_drop_step`] lets `id`'s target go. It only reads,
/// so a steady object's visit asks it too (`world::steady_ai`).
pub(in crate::sim::world) fn allied_target_drops(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
) -> bool {
    use crate::sim::combat::{TargetKind, combat_weapon};
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    // The target is read before the house only to spare an object without
    // one the house lookups; neither test writes (VERA-only order).
    let Some(target) = entity.attack_target.target() else {
        return false;
    };
    let house = combat_weapon::controlled_transport_house(
        &sim.substrate.entities,
        rules,
        &sim.interner,
        entity,
    )
    .unwrap_or(entity.owner());
    if sim.owner_is_human(house) {
        return false;
    }
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    // `0x004F9AF0` tests the target's Object flag first: a cell is never
    // allied.
    let target = match target {
        TargetKind::Entity(target_id) => sim.substrate.entities.get(target_id),
        _ => None,
    };
    let allied = target.is_some_and(|target| {
        combat_weapon::is_ally_by_object(
            Some(&sim.house_alliances),
            &sim.interner,
            house,
            target.owner(),
        )
    });
    // Ares 3.0 TechnoClass_Update_LoseTarget (1004E4C0, hook 0x6FA361): a
    // healer (`0x006F3970(-1) < 0`) keeps an allied target and takes a
    // non-allied one, a cell included, to the drop below (0x006FA39D);
    // anything else keeps the stock test.
    let drops = if rules.ares() {
        allied
            != (combat_weapon::weapon_damage_value(
                entity,
                obj,
                &sim.substrate.entities,
                rules,
                &sim.interner,
            ) < 0)
    } else {
        allied
    };
    if !drops {
        return false;
    }
    let target_obj = target.and_then(|target| sim.interner.object_type(target.type_ref(), rules));
    if target.is_some() && target_obj.is_none() {
        return false;
    }
    let infantry = entity.category() == EntityCategory::Infantry;
    let target_building = target
        .and_then(|target| target_obj.map(|target_obj| (target, target_obj)))
        .filter(|(target, _)| target.category() == EntityCategory::Structure);
    let occupiable = infantry
        && target_building.is_some_and(|(target, target_obj)| {
            crate::sim::passenger::can_dock_occupier_garrison(
                entity,
                target,
                obj,
                target_obj,
                target.passenger_role().occupants(),
                rules,
                sim,
            )
        });
    let engineer = infantry && obj.engineer;
    let overpowers = target_building.is_some_and(|(_, target_obj)| target_obj.overpowerable)
        && target_scan::weapon_at_index(sim, rules, id, 1)
            .and_then(|weapon| rules.weapon_warhead(weapon))
            .is_some_and(|warhead| warhead.electric_assault);
    !(engineer || overpowers || occupiable || entity.berserk.is_active())
}

/// The shared post-acquisition sequence of `TechnoClass::AI_Update`:
/// Bomb6FA6F5, Slave6FA717, Capture6FA726, IsAlive6FA735, self-heal6FA743,
/// House healing6FA79A, Cloak6FA941, Spawn6FA94C, the cloak-field arms
/// 6FA95B and the healer drops 6FA9D8. Original instructions
/// and executable controls: `tools/spatial_oracle/capture_overload_update.py
/// --techno-order` and its documented callback/stock-MIND boundaries.
///
/// Re-read every owner after the previous callback: a fatal bomb frees the
/// captives before CaptureManager is queried, so the stock manager emits no
/// overload pulse. An overload still constructs its sparks, but a dead
/// controller never heals, cloaks or spawns. Mission and scan draws precede
/// overload draws on the same Scenario stream. Unit,
/// Infantry, Building and Aircraft use this one sequence in their Logic visit.
pub(super) fn techno_after_acquisition(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: &mut ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) -> bool {
    // A quiet unit carries no bomb and no manager, is at full strength and
    // cannot cloak (`world::quiet_units`); each step below would find so.
    // The mission and scan before may have written it: asked here.
    let quiet = sim.quiet(id);
    if !quiet {
        outcome.bridge_state_changed |=
            sim.bomb_fuse_step(id, rules, ctx.overlay_registry, &mut ctx.effects);
        if let Err(error) = sim.slave_manager_ai(id, rules, ctx.overlay_registry, &mut ctx.effects)
        {
            outcome.frame_error = Some(super::FrameAdvanceError::new(
                sim.session.tick(),
                sim.session.binary_frame(),
                id,
                error.to_string(),
            ));
            return false;
        }
        sim.capture_manager_update(id, rules, ctx.overlay_registry, &mut ctx.effects);
        if !ai_alive(&sim.substrate, id) {
            return false;
        }
    }
    // Ares 3.0 1004B820 (hook 0x6FA743): a crashing or sinking techno skips
    // both pulses, resuming at the cloak tick (0x6FA941).
    let heals = if rules.ares() {
        sim.substrate
            .entities
            .get(id)
            .is_some_and(|entity| !entity.crash.is_crashing() && !entity.sinking.is_active())
    } else {
        !quiet
    };
    if heals {
        self_heal_step(
            &sim.interner,
            &sim.session,
            &mut sim.substrate,
            &sim.resolved_terrain,
            id,
            rules,
        );
        house_self_heal_step(
            &sim.interner,
            &sim.session,
            &mut sim.substrate,
            &sim.houses,
            &sim.resolved_terrain,
            id,
            rules,
        );
    }
    if !quiet {
        super::techno_ai_cloak::tick_stock_cloak_producer(
            sim,
            id,
            rules,
            ctx.overlay_registry,
            &mut ctx.effects,
        );
        // A target the Unit's later Fire_At_Target hands its manager waits
        // until the next visit (Unit73647B/7365E1).
        crate::sim::spawn_manager::spawn_manager_ai(sim, rules, id, &mut ctx.effects);
        super::techno_ai_cloak::cloak_field_ai_step(sim, id, rules, ctx.overlay_registry);
    }
    healer_target_drop_step(sim, id, rules);
    true
}

/// The GetCoords of `entity` (`id`, of type `object`) whose Cell
/// [`behind_indicator_step`] asks about ([`behind_indicator_cell`]), or
/// `None` when the step asks none: for a type without `CanBeHidden=`, an
/// Aircraft, or while no Cell hides anything. A steady object's mark asks
/// it too (`world::steady_ai`).
pub(in crate::sim::world) fn behind_indicator_coord(
    sim: &Simulation,
    id: u64,
    entity: &GameEntity,
    object: &crate::rules::object_type::ObjectType,
) -> Option<crate::sim::anim_class::AnimWorldCoord> {
    // With no hidden-occupation entry no Cell hides anything; skip the
    // per-object coordinate and Cell lookups (VERA-only, result unchanged).
    if !object.can_be_hidden
        || entity.category() == EntityCategory::Aircraft
        || sim.substrate.hidden_occupation.entry_count() == 0
    {
        return None;
    }
    sim.anim_owner_coords(id)
}

/// The Cell holding `coord`, which [`behind_indicator_step`] asks whether it
/// hides objects; `None` off the map's coordinate range.
pub(in crate::sim::world) fn behind_indicator_cell(
    coord: crate::sim::anim_class::AnimWorldCoord,
) -> Option<(u16, u16)> {
    u16::try_from(coord.x / 256)
        .ok()
        .zip(u16::try_from(coord.y / 256).ok())
}

/// `TechnoClass::AI_Update @ 0x006FA24A..0x006FA30C`, before the allied
/// target drop: an object whose type has `CanBeHidden=` (TechnoType `+0x724`)
/// and that is not an Aircraft (vt+0x2C 2), standing on a Cell that hides it
/// (`cell_hides_objects`, `0x00487E00`, the Cell of its GetCoords vt+0x48),
/// takes the Behind indicator (`UpdateBehindIndicator @ 0x0070F1D0`); any
/// other object UnInits a retained one (vt+0xF8) and clears `+0x12C`.
///
/// UpdateBehindIndicator acts only when GetVisualState(0, NULL) (vt+0x68,
/// [`screen_visual_character`]) answers 0 and `+0x12C` is empty: it
/// constructs `[General] Behind=` (`Rules+0xB8`) at the object's GetCoords
/// with delay 0, loop 1, flags 0x600, ZAdjust 0 (`0x0070F20D..0x0070F24C`),
/// stores it at `+0x12C` and attaches it (SetOwnerObject `0x00424B50`). The
/// Anim's hidden byte (`+0x19D`) follows the local Show Hidden option
/// (`Options+0x1F`, `0x0070F26B`, AnimClass::AI `0x00423B62..0x00423B88`); the
/// presentation reads the option in its place.
///
/// RESIDUAL: with no `Behind=` type (retail sets BEHIND), native draws a
/// blinking bracket at the caller's screen point instead
/// (`0x0070F290..0x0070F659`); VERA draws nothing.
///
/// GetVisualState(0, NULL) depends on the current house only for an
/// `Invisible=`/`InvisibleInGame=` object (the retail lamp posts) on a hiding
/// Cell: natively 0 for its owner and 5 for anyone else, so peers disagreed
/// on the anim and the IDs after it. A network game asks with no local owner
/// ([`Simulation::shared_owned_by_current_house`], which records the owner's
/// residual): no peer builds one.
///
/// RESIDUAL: when BEHIND's art never bound, the spawn takes a native unique
/// ID and fails each frame; native instead constructs a default-type Anim
/// that expires and is rebuilt. Neither draws. No RNG draws or timer writes.
pub(super) fn behind_indicator_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::components::AnimClassSpawnDescriptor;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    let coord = behind_indicator_coord(sim, id, entity, object);
    let hidden = coord
        .and_then(behind_indicator_cell)
        .is_some_and(|(rx, ry)| sim.substrate.cell_hides_objects(rx, ry));
    if !hidden {
        // A quiet unit holds no Behind anim (`world::quiet_units`).
        if !sim.quiet(id)
            && let Some(anim) = sim
                .substrate
                .entities
                .get_mut_if(id, |entity| entity.behind_anim().is_some())
                .and_then(|entity| entity.take_behind_anim())
        {
            sim.destroy_anim(anim, rules);
        }
        return;
    }
    if screen_visual_character(sim, entity, rules) != 0 || entity.behind_anim().is_some() {
        return;
    }
    let (Some(name), Some(coord)) = (rules.general.behind.as_deref(), coord) else {
        return;
    };
    let type_name = sim.interner.intern(name);
    let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
    let descriptor = AnimClassSpawnDescriptor {
        draw_flags: 0x600,
        owner: Some(id),
        ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
    };
    if let Ok(anim) = sim.spawn_anim_at_world(rules, descriptor, coord)
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        entity.retain_behind_anim(anim);
    }
}

/// GetVisualState(0, NULL) (`0x00703860`) as the AI asks it: the screen
/// query for the current house (`0x00A83D4C`, the persisted
/// `session.current_house`), its sensor on the object's Cell and both
/// alliance directions.
fn screen_visual_character(
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    rules: &RuleSet,
) -> u8 {
    screen_visual_character_for(
        sim,
        entity,
        rules,
        sim.session.current_house(),
        sim.shared_owned_by_current_house(entity),
    )
}

/// [`screen_visual_character`] for `viewer`, with `owned_by_viewer` as the
/// `+0x41A` byte that house would hold.
fn screen_visual_character_for(
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    rules: &RuleSet,
    viewer: Option<crate::sim::intern::InternedId>,
    owned_by_viewer: bool,
) -> u8 {
    use crate::sim::combat::combat_weapon::is_ally_by_object;
    let owner = entity.owner();
    let allied = |a, b| is_ally_by_object(Some(&sim.house_alliances), &sim.interner, a, b);
    let object = sim.interner.object_type(entity.type_ref(), rules);
    let invisible = object.is_some_and(|object| {
        object.invisible
            || entity.category() == EntityCategory::Structure && object.invisible_in_game
    });
    entity.visual_character(
        object.map_or(rules.general.cloaking_stages, |object| {
            object.cloaking_stages(rules.general.cloaking_stages)
        }),
        invisible,
        owned_by_viewer,
        crate::sim::cloak_disguise::VisualCharacterQuery::screen(
            true,
            viewer.is_some(),
            viewer.is_some_and(|viewer| sim.fog.senses_object_for_house(viewer, entity)),
            viewer.is_some_and(|viewer| allied(viewer, owner) && allied(owner, viewer)),
            !sim.session.game_mode_nonzero(),
            false,
        ),
    )
}

impl Simulation {
    /// Whether `viewer` sees the object at this moment as that house's
    /// screen would show it (RFC 0001 `Visible`): alive and out of limbo;
    /// the viewer's own or an ally's, or on a cell the viewer's fog shows
    /// objects on ([`FogState::shows_objects_to`], current sight under
    /// FogOfWar, as the translucent fog draws); and not Hidden (5) to the
    /// screen GetVisualState (`0x00703860`) asked for the viewer, which
    /// covers cloaking, sensors, `Invisible=` and the tunnel ripple.
    ///
    /// [`FogState::shows_objects_to`]: crate::sim::vision::FogState::shows_objects_to
    pub(crate) fn object_seen_by(
        &self,
        rules: &RuleSet,
        entity: &crate::sim::game_entity::GameEntity,
        viewer: crate::sim::intern::InternedId,
    ) -> bool {
        if !entity.lifecycle.object_alive() || entity.lifecycle.in_limbo() {
            return false;
        }
        let owner = entity.owner();
        let own = owner == viewer;
        let shown = own
            || self
                .house_alliances
                .is_allied(&self.interner, viewer, owner)
            || self.fog.shows_objects_to(
                viewer,
                entity.position.rx(),
                entity.position.ry(),
                self.session.game_options().fog_of_war,
            );
        shown && screen_visual_character_for(self, entity, rules, Some(viewer), own) != 5
    }
}

/// `TechnoClass::AI_Update @ 0x006FA9D8..0x006FAAEF`, after the spawn manager
/// (`0x006FA958`) and the cloak-field arms: a healer (CombatDamage
/// `0x006F3970(-1) < 0`) lets go of its target (vt+0x3C8(NULL)) in three
/// arms, each re-reading the target:
/// - GameMode 0 (`0x00A8B238`): a house that is not the player's
///   (`0x0050B6F0`) drops a target it is not allied with
///   (`0x004F9AF0`, `0x006FA9D8..0x006FAA1B`);
/// - any other GameMode: a human house (`+0x1EC`, which IsControlledByHuman
///   `0x0050B730` reads there) drops a target it is not allied with
///   (`0x006FAA21..0x006FAA69`);
/// - a Unit drops an Aircraft target that is airborne (GetHeight vt+0x1C8
///   above 0) or stands on a building's Cell (`0x0047C520` at its Location,
///   `0x006FAA6F..0x006FAAE9`).
///
/// IsAlliedWith on an object asks its owner; a Cell or an ownerless object is
/// never allied. Both alliance arms ask the object's own house (+0x21C).
pub(super) fn healer_target_drop_step(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    use crate::sim::combat::{TargetKind, combat_weapon};
    let healer = |sim: &Simulation| {
        sim.substrate.entities.get(id).is_some_and(|entity| {
            sim.interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|kind| combat_damage_heals(sim, entity, kind, rules))
        })
    };
    let target = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(id)
            .and_then(|entity| entity.attack_target.target())
    };
    let allied = |sim: &Simulation, target: TargetKind| {
        let (Some(entity), TargetKind::Entity(target_id)) =
            (sim.substrate.entities.get(id), target)
        else {
            return false;
        };
        sim.substrate.entities.get(target_id).is_some_and(|object| {
            combat_weapon::is_ally_by_object(
                Some(&sim.house_alliances),
                &sim.interner,
                entity.owner(),
                object.owner(),
            )
        })
    };
    let drop = |sim: &mut Simulation| {
        sim.assign_target_represented(id, None, rules, &mut FrameEffects::default())
            .expect("the queried healer receiver remains present");
    };
    let Some(owner) = sim.substrate.entities.get(id).map(|entity| entity.owner()) else {
        return;
    };
    if !sim.session.game_mode_nonzero()
        && !sim.house_is_human_player(owner)
        && let Some(current) = target(sim)
        && !allied(sim, current)
        && healer(sim)
    {
        drop(sim);
    }
    if let Some(current) = target(sim)
        && sim.session.game_mode_nonzero()
        && sim.owner_is_human(owner)
        && healer(sim)
        && !allied(sim, current)
    {
        drop(sim);
    }
    let Some(TargetKind::Entity(target_id)) = target(sim) else {
        return;
    };
    let Some(aircraft) = sim
        .substrate
        .entities
        .get(target_id)
        .filter(|object| object.category() == EntityCategory::Aircraft)
    else {
        return;
    };
    let unit = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.category() == EntityCategory::Unit);
    if !unit || !healer(sim) {
        return;
    }
    let airborne = crate::sim::movement::air_movement::current_fly_height(
        aircraft,
        sim.resolved_terrain.as_ref(),
    ) > 0;
    let padded = sim
        .substrate
        .occupancy
        .first_building_on_layer(
            aircraft.position.rx(),
            aircraft.position.ry(),
            crate::sim::movement::locomotor::MovementLayer::Ground,
        )
        .is_some();
    if airborne || padded {
        drop(sim);
    }
}

/// Whether `entity` (of type `object`) is the healer every arm of
/// [`healer_target_drop_step`] asks for: its CombatDamage
/// (`0x006F3970(-1)`) is negative. A Unit's or Infantry's weapon is its own,
/// so a steady object's mark asks it (`world::steady_ai`).
pub(in crate::sim::world) fn combat_damage_heals(
    sim: &Simulation,
    entity: &GameEntity,
    object: &crate::rules::object_type::ObjectType,
    rules: &RuleSet,
) -> bool {
    crate::sim::combat::combat_weapon::weapon_damage_value(
        entity,
        object,
        &sim.substrate.entities,
        rules,
        &sim.interner,
    ) < 0
}

/// The common Techno post-mission stage and the stock-dormant Cyborg Spark
/// producer. The retained system and both native producers share particles::spawn.
pub(super) fn techno_common_post(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    effects: &mut FrameEffects<'_>,
) {
    // Native6FABBC..C2 skips ONLY the generic Stage block to6FAC31; the
    //shared flash/particle/EMP tail still precedes Building's Gattling code.
    //Do not reset Stage.changed or flash twice when migrating this caller.
    stage_step(sim, id);
    flash_step(&mut sim.substrate, id, sim.session.binary_frame());

    sim.update_damage_spark_after_ai(id, rules);
    // Techno6FAF01..6FAF07 invokes virtual+4A0(0), after damage sparks and
    // before EMP. Its simulation discovery prelude is independent of radar
    // presentation, viewport admission and rendering cadence.
    sim.update_techno_discovery_from_shroud(id);
    // Installed Ares6FAF0D ->10053DF0, after the ordinary Techno tail.
    // No global frame sweep: limbo/skipped/dead visits never reach this slot.
    if rules.ares() {
        sim.tick_ares_emp(id, rules, effects);
    }
}

/// The common stage clock (Techno6FABC4), after Mission, passive acquisition
/// and the reach slot. Buildings step at4509DE in their existing
/// UpdateAnimation owner. A quiet unit's clock stands still
/// (`world::quiet_units`); a steady object's counted visit runs this step
/// alone (`world::steady_ai`).
pub(in crate::sim::world) fn stage_step(sim: &mut Simulation, id: u64) {
    let now = sim.session.binary_frame() as i32;
    if !sim.quiet(id)
        && let Some(entity) = sim.substrate.entities.get_mut_if(id, |entity| {
            entity.category() != EntityCategory::Structure && entity.native_stage().advances(now)
        })
    {
        entity.tick_native_stage(now);
    }
}

/// Shared late Techno6FAC31..6FAC79 slot for every represented category.
/// The countdown owns no RNG or CdTimer. Native redraw virtual+124 is a
/// presentation dependency; the existing +F0 supplies its phase bits.
/// A building whose countdown ran and flipped its flash phase relights its
/// slot anims (`0x006FAC62..0x006FACC8`).
fn flash_step(substrate: &mut ObjectSubstrate, id: u64, frame: u32) {
    if !ai_alive(substrate, id) {
        return;
    }
    let Some(entity) = substrate
        .entities
        .get_mut_if(id, |entity| entity.flashing.counting())
    else {
        return;
    };
    let was_flashed = entity.flashing.flashed();
    let flipped = entity.flashing.tick() && entity.flashing.flashed() != was_flashed;
    if flipped && entity.category() == EntityCategory::Structure {
        substrate.relight_building_anims(id, frame);
    }
}

/// Outcome of the common bracket for one Unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BracketReach {
    /// Died after the pre-block (health 0); no mission work ran.
    DiedInPre,
    /// Live Unit: ran the `+0xC4` counter and admitted mission handlers at the
    /// dispatch point.
    Dispatched,
}

/// Per-Unit TechnoClass common bracket. Runs the contiguous
/// `pre -> [IsAlive B] -> +0xC4/dispatch -> [IsAlive E] -> post` structure at
/// the gamemd-faithful per-object AI point (pre-movement, LogicVector order).
/// UnitClass::AI's first Ready→Commence at736473 precedes this bracket in
/// `techno_ai_shell`; the later checkpoint has its post-movement owner.
pub(super) fn unit_techno_bracket(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: &mut ObjectAiCtx<'_>,
    outcome: &mut ObjectAiOutcome,
) -> BracketReach {
    if !techno_common_steps(sim, id, rules, ctx.overlay_registry, &mut ctx.effects, None) {
        return BracketReach::DiedInPre;
    }
    // Guard B (IsAlive, `0x006FA23C`). A crashing wreck is alive at Health 0,
    // so it runs on: the counter, the passive block (on Move,
    // Guard or Harvest the gate and the scan with its Scenario draws, which
    // can acquire a target its fire update then shoots), the bomb and the
    // managers, and the post block; only the handlers are skipped.
    if !ai_alive(&sim.substrate, id) {
        return BracketReach::DiedInPre;
    }
    // The off-mission passive-target clear runs BEFORE the +0xC4 counter.
    clear_passive_target_off_mission(sim, id, rules);
    if !mission_counter_step(sim, id, rules, ctx, outcome) {
        return BracketReach::Dispatched;
    }
    // Mission_Dispatch position: the absorbed handler bodies run here,
    // timer-gated, ending with the verified post-handler epilogue write
    // (start = current frame, delay = handler return). One committed-selector
    // match routes Harvest, Attack and the other represented Foot handlers.
    match dispatch_foot_mission(sim, id, rules, ctx) {
        Ok(changed) => outcome.bridge_state_changed |= changed,
        Err(error) => {
            outcome.frame_error = Some(super::FrameAdvanceError::new(
                sim.session.tick(),
                sim.session.binary_frame(),
                id,
                error.to_string(),
            ));
            return BracketReach::Dispatched;
        }
    }
    // Passive / opportunity target acquisition sits between mission dispatch
    // and the second IsAlive guard, before the object's own locomotion.
    passive_acquire_step(sim, id, rules, ctx);
    // Guard E (IsAlive, `0x006FA735`): the dispatched handler, or the bomb it
    // carried, may have destroyed the Unit; a dead Unit runs no post-mission
    // block.
    if !techno_after_acquisition(sim, id, rules, ctx, outcome) {
        return BracketReach::Dispatched;
    }
    target_reach_step(sim, id, rules, ctx.overlay_registry, &mut ctx.effects);
    techno_common_post(sim, id, rules, &mut ctx.effects);
    BracketReach::Dispatched
}

// ===== Passive / opportunity target acquisition =====
//
// What makes an idle Grizzly shoot a tank that drives past: the passive block
// of TechnoClass::AI_Update (`0x006FA65A`) asks the gate and runs
// Retaliate_And_Scan. Both live in `target_scan`; this section keeps the
// mission-change drop of a scanner-installed target.

/// Missions on which a passively-acquired target is dropped, before the AI
/// counter runs. Meaning: the moment an object takes a job that should not be
/// shooting, a target it picked up on its own goes away. The mission *numbers*
/// are verified ({0, 7, 13, 14, 16, 18, 19, 20, 22, 23, 24, 28}); the names
/// below are this project's mission table for those indices.
const PASSIVE_TARGET_CLEAR_MISSIONS: [MissionType; 12] = [
    MissionType::Sleep,
    MissionType::Enter,
    MissionType::Stop,
    MissionType::Ambush,
    MissionType::Unload,
    MissionType::Construction,
    MissionType::Selling,
    MissionType::Repair,
    MissionType::Missile,
    MissionType::Harmless,
    MissionType::Open,
    MissionType::Deliberate,
];

/// The off-mission clear, which runs before the AI counter: a passively
/// acquired target is dropped the moment the object takes a job that should not
/// be shooting (see [`PASSIVE_TARGET_CLEAR_MISSIONS`]).
/// `TechnoClass::AI_Update @ 0x006F9E50` at `0x006FA5E8..0x006FA63B`: TarCom `+0x2B4`,
/// the mark `+0x50C`, then the mission `+0xAC`.
pub(super) fn clear_passive_target_off_mission(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    if !sim
        .substrate
        .entities
        .get(id)
        .is_some_and(off_mission_clear_drops)
    {
        return;
    }
    let _ = sim.assign_target_represented(id, None, rules, &mut FrameEffects::default());
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.passive_scan.clear_acquired();
    }
}

/// Whether [`clear_passive_target_off_mission`] drops `entity`'s target. It
/// reads the object alone, so a steady object's mark asks it
/// (`world::steady_ai`).
pub(in crate::sim::world) fn off_mission_clear_drops(entity: &GameEntity) -> bool {
    entity.attack_target.target().is_some()
        && entity.passive_scan.acquired()
        && PASSIVE_TARGET_CLEAR_MISSIONS.contains(&entity.passive_acquire_mission())
}
