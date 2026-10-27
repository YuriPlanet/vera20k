//! Accepted custom-handler presentation from SWTypeExt::Launch10032ED0.
//! The animation owner retains a viewer-independent snapshot of Anim+19D;
//! local presentation never writes authoritative simulation state.
//! UnitDelivery100A8E70, GenericWarhead100A8ECC and HunterSeeker100A9280 share GetAnim
//! 1006D0C0 (null fallback), GetSound1006D1C0 (-1 fallback), and Flags
//! 10003650 (zero); none suppresses the common animation/sound/Ready cleanup.

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
    let Some(sw) = rules.super_weapon(sim.interner.resolve(typ)) else {
        return;
    };
    let presentation = &super::financial_type(rules, sw).presentation;
    let mut coord = super::deck_coords(sim, cell);
    // Launch10033068..100330F9: GetAnim (base Nullable fallback nullptr),
    // add the signed height, construct regardless of visibility, then write
    // Hidden. No OwnerHouse store is made by this producer.
    if let Some(Some(name)) = &presentation.animation {
        coord[2] = coord[2].wrapping_add(presentation.height);
        if let Some(anim) = super::spawn_super_anim(sim, rules, name, coord) {
            let mask = presentation.visibility;
            if mask == HouseMask::ALL || mask == HouseMask::NONE {
                sim.substrate.set_anim_hidden(anim, mask == HouseMask::NONE);
            } else {
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
                sim.substrate.set_anim_viewer_hidden(
                    anim,
                    viewers,
                    !mask.intersects(HouseMask::ENEMIES),
                );
            }
        }
    }
    // Launch10033106..1003314E, GetSound1006D1C0: play only the resolved
    // nonnegative cue. Bridge10033BB0 calls VocClass::PlayAtCoord750E20,
    // using the signed coordinates the animation just raised, with flags0.
    if let Some(Some(sound)) = &presentation.sound {
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
    // SWTypeExt::Launch10033201..10033242 removes Ready only after the
    // common sound. The client supplies Fire_SW's local-player predicate.
    sim.sound_events
        .emit(SimSoundEvent::AresSuperWeaponFinished {
            owner,
            sw_type: typ,
        });
}
