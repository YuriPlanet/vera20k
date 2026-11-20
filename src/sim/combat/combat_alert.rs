//! Phobos combat alert (`Ext/Techno/Hooks.ReceiveDamage.cpp:86-140`), the
//! block inside `TechnoClass_ReceiveDamage_Shield @ 0x701900` that runs after
//! the damage multipliers and before the shield stage.
//!
//! The block tests the globals first (`RulesExt::Global()->CombatAlert` and
//! `damage > 1`), then the victim House: it must be the current player's and
//! its `CombatAlertTimer` must have expired. A warhead whose
//! `CombatAlert.Suppress` resolves true is skipped, as is a type whose
//! `CombatAlert` resolves false or an object outside the playfield. An
//! ordinary building is skipped under `CombatAlert.IgnoreBuilding` unless its
//! type is vehicle-like or its own `CombatAlert.NotBuilding` says otherwise.
//! An accepted hit starts the House timer for `CombatAlert.Interval`, creates
//! `RadarEventType::Combat` at `Coord2Cell(GetCoords())` and plays at most one
//! line: `VoiceFeedback[0]`, else `VoiceAttack[0]`, else the `CombatAlert.EVA`
//! Vox entry.
//!
//! The `CombatAlert.SuppressIfInScreen` gate reads the live tactical view and
//! its DSurface; a headless simulation has neither, so it is the recorded
//! residual ([`docs/phobos-combat-alert.md`]).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and sim/ only; never audio/ or ui/.
//!   The chosen voice is a name the client resolves.

use crate::map::entities::EntityCategory;
use crate::rules::phobos_combat_alert::{CombatAlertEva, DEFAULT_COMBAT_ALERT_EVA};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::receiver_call::EntityDamageEvent;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::{CombatAlertVoice, SimSoundEvent, Simulation};

/// One received hit's chance to raise the victim House's combat alert.
///
/// `damage` is the receiver body's current value at the native hook point
/// (after [`super::damage_multiplier`], which owns the Phobos
/// `Damage*Multiplier` block at `Ext/Techno/Hooks.ReceiveDamage.cpp:48-82`)
/// and `frame` is the current binary frame.
pub(crate) fn raise_combat_alert(
    world: &mut Simulation,
    rules: &RuleSet,
    event: &EntityDamageEvent,
    damage: i32,
    frame: i32,
) {
    let globals = &rules.phobos_globals;
    // `if (RulesExt::Global()->CombatAlert && damage > 1)`
    // (`Ext/Techno/Hooks.ReceiveDamage.cpp:86`).
    if !globals.combat_alert || damage <= 1 {
        return;
    }
    let Some(target) = world.substrate.entities.get(event.target_id) else {
        return;
    };
    let owner = target.owner();
    let category = target.category();
    let in_playfield = target.in_playfield();
    let type_ref = target.type_ref();
    // `pThis->GetCoords()` (`:109`), read once for `Coord2Cell`.
    let coords = crate::sim::movement::ground_pose::object_get_coords(
        target,
        world.resolved_terrain.as_ref(),
    );
    // `pTargetHouse` (`:90,95,121`).
    let Some(house) = world.houses.get(&owner) else {
        return;
    };
    // `!pTargetHouse->IsControlledByCurrentPlayer()` (`:90`). The simulation
    // has no local player; "a human controls the House" is the recorded
    // approximation, and the app keeps only its own player's event.
    if !house.is_controlled_by_human(world.session.rule_policy()) {
        return;
    }
    // `pHouseExt->CombatAlertTimer.HasTimeLeft()` (`:95`): the native timer
    // throttles every further alert for the House.
    // `CDTimerClass::HasTimeLeft()` is `GetTimeLeft() > 0`: a timer whose
    // countdown already went negative also has no time left.
    if house.combat_alert_timer().remaining(frame) > 0 {
        return;
    }
    // `pSourceHouse = pAttacker ? pAttacker->Owner : args->SourceHouse`
    // (`:43`); the receiver event's source House and attacker owner are the
    // same two inputs the later armor block reads (`world_receiver.rs`).
    let source_house = event.source_house.or_else(|| {
        world
            .substrate
            .entities
            .get(event.attacker_id)
            .map(|attacker| attacker.owner())
    });
    // `CombatAlert_SuppressIfAllyDamage && pTargetHouse->IsAlliedWith(
    // pSourceHouse)` (`:90`).
    if globals.combat_alert_suppress_if_ally_damage
        && source_house.is_some_and(|source| {
            world
                .house_alliances
                .is_allied(&world.interner, owner, source)
        })
    {
        return;
    }
    // `pWHExt->CombatAlert_Suppress.Get(!pWHExt->Malicious ||
    // pWHExt->Nonprovocative)` (`:95`). `Malicious` is the one existing
    // owner ([`super::world_receiver::warhead_is_malicious`]): Ares' warhead
    // extension under `ares`, otherwise the constructor's true.
    let warhead_suppressed = rules
        .warhead(world.interner.resolve(event.warhead_ref))
        .is_some_and(|warhead| {
            warhead.combat_alert_suppress.unwrap_or_else(|| {
                !super::world_receiver::warhead_is_malicious(rules, &world.interner, event)
                    || warhead.nonprovocative
            })
        });
    if warhead_suppressed {
        return;
    }
    let Some(object) = world.interner.object_type(type_ref, rules) else {
        return;
    };
    let type_keys = &object.phobos_combat_alert;
    // `pTypeExt->CombatAlert.Get(RulesExt::Global()->CombatAlert_Default.Get(
    // !pType->Insignificant && !pType->Spawned))` (`:101`).
    let type_default = globals
        .combat_alert_default
        .unwrap_or(!object.insignificant && !object.spawned);
    if !type_keys.alert.unwrap_or(type_default) || !in_playfield {
        return;
    }
    // `CombatAlert_IgnoreBuilding && pBuilding && (CombatAlert_NotBuilding
    // .isset() ? !CombatAlert_NotBuilding.Get() : !pBuilding->Type
    // ->IsVehicle())` (`:106`). `IsVehicle` is Techno vt+0x80
    // (`0x00457620` -> `BuildingTypeClass__Is1x1WithUndeploy @ 0x00465D40`),
    // the shared [`super::vehicle_like`] answer.
    if globals.combat_alert_ignore_building
        && category == EntityCategory::Structure
        && type_keys.not_building.map_or_else(
            || !super::vehicle_like(category, object.non_vehicle, object.is_vehicle(), || false),
            |not_building| !not_building,
        )
    {
        return;
    }
    // `RadarEventClass::Create(RadarEventType::Combat,
    // CellClass::Coord2Cell(coordInMap))` (`:122`): `Coord2Cell` divides
    // toward zero and narrows to the 16-bit cell words.
    let radar = RadarEventRequest::new(
        RadarEventType::Combat,
        (coords.x / 256) as u16,
        (coords.y / 256) as u16,
    );
    let voice = choose_voice(globals, type_keys, object);
    // `pHouseExt->CombatAlertTimer.Start(CombatAlert_Interval)` (`:121`)
    // precedes the radar event and the voice.
    if let Some(house) = world.houses.get_mut(&owner) {
        house.start_combat_alert(frame, globals.combat_alert_interval);
    }
    world.sound_events.emit(SimSoundEvent::CombatAlert {
        owner,
        radar,
        voice,
    });
}

/// `:125-135`, in native order: no line when `MakeAVoice` is false; else
/// `VoiceFeedback[0]` when its switch resolves true and the list is
/// non-empty; else `VoiceAttack[0]` under the same shape; else the EVA line
/// when its switch resolves true, where a set `-1` index (a `none` read) or
/// an unset key with no `EVA_UnitsInCombat` entry plays nothing.
fn choose_voice(
    globals: &crate::rules::phobos_globals::PhobosGlobals,
    type_keys: &crate::rules::phobos_combat_alert::PhobosCombatAlert,
    object: &crate::rules::object_type::ObjectType,
) -> CombatAlertVoice {
    if !globals.combat_alert_make_a_voice {
        return CombatAlertVoice::None;
    }
    if type_keys
        .use_feedback_voice
        .unwrap_or(globals.combat_alert_use_feedback_voice)
        && let Some(sound_id) = object.voice_feedback.first()
    {
        return CombatAlertVoice::Feedback {
            sound_id: sound_id.clone(),
        };
    }
    if type_keys
        .use_attack_voice
        .unwrap_or(globals.combat_alert_use_attack_voice)
        && let Some(sound_id) = object.voice_attack().first()
    {
        return CombatAlertVoice::Attack {
            sound_id: sound_id.clone(),
        };
    }
    if type_keys.use_eva.unwrap_or(globals.combat_alert_use_eva) {
        // `CombatAlert_EVA.Get(VoxClass::FindIndex("EVA_UnitsInCombat"))`:
        // a Vox name the process catalog does not define is a client-side
        // silence; the `None` state is native's explicit -1 index.
        return match &type_keys.eva {
            CombatAlertEva::Default => CombatAlertVoice::Eva {
                event: DEFAULT_COMBAT_ALERT_EVA.to_owned(),
            },
            CombatAlertEva::None => CombatAlertVoice::None,
            CombatAlertEva::Name(name) => CombatAlertVoice::Eva {
                event: name.clone(),
            },
        };
    }
    CombatAlertVoice::None
}

#[cfg(all(test, tests_sim_combat_a_c))]
#[path = "phobos_combat_alert_tests.rs"]
mod tests;
