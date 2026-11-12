//! Accepted custom-handler presentation from SWTypeExt::Launch10032ED0.
//! The animation owner retains a viewer-independent snapshot of Anim+19D;
//! local presentation never writes authoritative simulation state.
//! UnitDelivery100A8E70, GenericWarhead100A8ECC and HunterSeeker100A9280 share GetAnim
//! 1006D0C0 (null fallback), GetSound1006D1C0 (-1 fallback), and Flags
//! 10003650 (zero); none suppresses the common animation/sound/Ready cleanup.
//! Battery shares them too; Protect's GetAnim (10074A90) falls back to
//! `IronCurtainInvokeAnim=`. The ChronoSphere's Flags (1006EE70) skip the
//! anim, EVA, money, radar event, message and cleanup; the ChronoWarp's
//! (10070040) the anim and radar event, its other steps reading the source
//! Super's type.

use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_targeting::HouseMask;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};

pub(super) fn on_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    typ: InternedId,
    cell: (u16, u16),
) {
    use crate::rules::superweapon_type::AresSuperWeaponFlags as F;
    let Some(fired) = rules.super_weapon(sim.interner.resolve(typ)) else {
        return;
    };
    // The fired type's handler answers the Flags; the PostClick switch
    // (10032F2B) hands the rest the source Super's type.
    let flags = fired.kind.ares_flags();
    let data = super::ares_launch_data_type(sim, rules, owner, typ);
    let Some(sw) = rules.super_weapon(sim.interner.resolve(data)) else {
        return;
    };
    let presentation = &sw.presentation;
    let mut coord = super::deck_coords(sim, cell);
    // Launch10033068..100330F9: GetAnim (base Nullable fallback nullptr;
    // SW_Protect::GetAnim10074A90 falls back to IronCurtainInvokeAnim), add
    // the signed height, construct regardless of visibility, then write
    // Hidden. No OwnerHouse store is made by this producer.
    let animation = match &presentation.animation {
        _ if flags.has(F::NO_ANIM) => None,
        Some(animation) => animation.clone(),
        None if sw.kind == crate::rules::superweapon_type::SuperWeaponKind::Protect => {
            Some(rules.general.iron_curtain_invoke_anim.clone())
        }
        // SW_GeneticMutator::GetAnim10071F30: `[General] IonBlast=`.
        None if sw.kind == crate::rules::superweapon_type::SuperWeaponKind::GeneticConverter => {
            Some(rules.general.ion_blast_anim.clone())
        }
        None => None,
    };
    if let Some(name) = &animation {
        coord[2] = coord[2].wrapping_add(presentation.height);
        if let Some(anim) = super::spawn_super_anim(sim, rules, name, coord) {
            hide_from_unaffected_viewers(sim, anim, owner, presentation.visibility);
        }
    }
    // Launch10033106..1003314E, GetSound1006D1C0: play only the resolved
    // nonnegative cue. Bridge10033BB0 calls VocClass::PlayAtCoord750E20,
    // using the signed coordinates the animation just raised, with flags0.
    // SW_GeneticMutator::GetSound10072000 falls back to `[AudioVisual]
    // GeneticMutatorActivateSound=`.
    let sound = match &presentation.sound {
        Some(sound) => sound.clone(),
        None if sw.kind == crate::rules::superweapon_type::SuperWeaponKind::GeneticConverter => {
            rules.general.genetic_mutator_activate_sound.clone()
        }
        None => None,
    };
    if !flags.has(F::NO_SOUND)
        && let Some(sound) = &sound
    {
        sim.sound_events.emit(SimSoundEvent::VocAtCoord {
            sound_id: sound.clone(),
            coordinate: DriveCoord {
                x: coord[0],
                y: coord[1],
                z: coord[2],
            },
            flags: 0,
        });
    }
    // Launch10033153..10033167: a type-13 radar event at the launch cell
    // while SW.CreateRadarEvent holds, unless the Flags hold NoEvent.
    if !flags.has(F::NO_EVENT)
        && rules
            .super_weapon_metadata(&sw.id)
            .is_some_and(|metadata| metadata.launch().radar_event())
    {
        sim.sound_events.emit(SimSoundEvent::RadarEvent {
            radar: crate::sim::radar::RadarEventRequest::new(
                crate::sim::radar::RadarEventType::ImpactSilent,
                cell.0,
                cell.1,
            ),
        });
    }
    // SWTypeExt::Launch10033201..10033242 removes Ready only after the
    // common sound and Message.Launch (printed by every client from this
    // fact unless NoMessage). The client supplies Fire_SW's local-player
    // predicate. No ported handler holds NoCleanup without NoMessage
    // (ChronoSphere holds both, LightningStorm only NoMessage), so one fact
    // carries both steps.
    if !(flags.has(F::NO_MESSAGE) && flags.has(F::NO_CLEANUP)) {
        sim.sound_events
            .emit(SimSoundEvent::AresSuperWeaponFinished {
                owner,
                sw_type: data,
                message: !flags.has(F::NO_MESSAGE),
            });
    }
}

/// SWTypeExt::ExtData::IsTimerVisible (Ares.dll 10032E80), asked by
/// TacticalClass_Draw_TimerVisibility (10035A80, hook `0x6D49D1`) before the
/// stock hold test of each Super on the timer list: whether the local player
/// (HouseClass::Player) sees `view`'s timer line. An observer sees what an
/// ally sees; otherwise the owner's relation to the player (10021440: the
/// owner, an ally, else an enemy, also when there is no player) picks the
/// type's `SW.TimerVisibility` bit. A hidden Super takes no line (`0x6D4A71`).
pub fn ares_timer_visible(sim: &Simulation, rules: &RuleSet, view: super::SuperTimerView) -> bool {
    let Some(sw) = rules.super_weapon(sim.interner.resolve(view.sw_type())) else {
        return true;
    };
    let relation = match sim.session.current_house() {
        Some(player) if sim.session.is_observer_house(player) => HouseMask::ALLIES,
        Some(player) => sim.ares_house_relation(view.owner(), player),
        None => HouseMask::ENEMIES,
    };
    sw.presentation.timer_visibility.intersects(relation)
}

/// `AnimClass+0x19D` (Invisible) per viewer: hidden from each House the
/// type's `SW.AnimationVisibility` excludes (IsAnimVisible, inlined in
/// CreateChronoAnim 10032470 and Launch 10033060), from the owner's view.
pub(super) fn hide_from_unaffected_viewers(
    sim: &mut Simulation,
    anim: crate::sim::anim_class::AnimId,
    owner: InternedId,
    mask: HouseMask,
) {
    if mask == HouseMask::ALL || mask == HouseMask::NONE {
        sim.substrate.set_anim_hidden(anim, mask == HouseMask::NONE);
        return;
    }
    let viewers = sim
        .houses
        .order()
        .iter()
        .copied()
        .map(|viewer| {
            (
                viewer,
                !mask.intersects(sim.ares_house_relation(owner, viewer)),
            )
        })
        .collect();
    sim.substrate
        .set_anim_viewer_hidden(anim, viewers, !mask.intersects(HouseMask::ENEMIES));
}
