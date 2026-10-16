//! The Iron Curtain's launch: `SuperClass::Launch @ 0x006CC390` case 1
//! (`0x006CCE64..0x006CD06F`).
//!
//! A charged Super (`+0x6F`) builds `[General] IronCurtainInvokeAnim=` 5
//! leptons over the cell's deck coordinate (`0x006CCE76..0x006CCF09`), plays
//! `EVA_IronCurtainActivated` (`0x006CCF21`) and raises a type-13 radar event
//! at the cell (`0x006CCF2F`). It then walks the 3x3 block around the cell in
//! the offset table's order ([`native_cells_3x3`]). Each cell gives its bridge
//! list (`+0xE8`) when it carries the bridge flag (`+0x140 & 0x100`), else its
//! ground list (`+0xE4`). Every object on it gets its IronCurtain (vt+0x154)
//! with `[CombatDamage] IronCurtainDuration=` (`Rules+0xFE8`), the Super's
//! house and 0, except a Foot under the Chronosphere's warp latch (`+0x27C`,
//! `0x006CCFF8..0x006CD006`). The next object is read after the call (`+0x30`,
//! `0x006CD025`).
//!
//! The overrides:
//! - InfantryClass's (`0x00522600`) deals the type's Strength as damage.
//! - FootClass's (`0x004DEAE0`, Units and Aircraft) does the same for an
//!   Organic type. For any other, it forces a parasite off, clears the
//!   paralysis timer and curtains.
//! - BuildingClass's (`0x00457C90`) defuses a planted C4, then curtains.
//! - The curtain itself is TechnoClass's (`0x0070E2B0`, [`apply_invulnerability`]).
//! - Terrain objects take ObjectClass's no-op (`0x00426460`, `RET 0xC`). VERA's
//!   object lists hold no terrain.
//!
//! The local player's tail (`0x006CD03B..0x006CD060`) clears the selected
//! Super and drops the queued `EVA_IronCurtainReady` line; the app does both
//! from the launch event.
//!
//! Evidence: `tools/superweapon_oracle.py` sections `iron_curtain_launch` and
//! `curtain_overrides` (Unicorn on gamemd.exe: case 1 and the Infantry and
//! Foot overrides), replayed in `iron_curtain_tests.rs`. BuildingClass's
//! override is read from its instructions, not executed: with a C4 planted
//! (`+0x6DF`) it clears that byte and `+0x540` and restarts the C4 timer
//! (`+0x528`) at the frame with no time; Rust:
//! `world_orders_c4_tests::c4_iron_curtain_application_cancels_pending_detonation`.
//!
//! RESIDUALS:
//! - The EVA line is skipped natively while `0x00A8B538` is set
//!   (`HouseClass::MPlayer_Defeated @ 0x004FC205`), on every client after
//!   the local player's multiplayer defeat; VERA always plays it.
//! - A null `IronCurtainInvokeAnim=` hands AnimClass a null type natively;
//!   VERA constructs nothing. Retail sets `IRONBLST`.
//! - The trigger action IronCurtainAtWP (`TActionClass @ 0x006E36E0`) is not
//!   ported. Trigger: a map trigger with that action. Effect: no anim and no
//!   curtain at the waypoint.
//!
//! Ledger:
//! - Scenario draws: the invoke anim constructor's own, and those of the
//!   receiver for each infantry or Organic hit.
//! - Timer writes: each curtain (`TechnoClass+0x18C`), a Foot's paralysis
//!   timer (`+0x6A0`), an eaten Foot's parasite suppression
//!   (`ParasiteClass+0x2C`) and a planted building's C4 timer (`+0x528`).
//! - Detach calls: ExitUnit (`0x0062A4A0`) of the parasite eating a Foot.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/superweapon/{invulnerability,cell_grid},
//!   sim/combat, sim/radar, sim/game_entity, sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

#[cfg(test)]
#[path = "iron_curtain_tests.rs"]
mod tests;

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
#[cfg(test)]
use crate::sim::combat::EntityDamageEvent;
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::superweapon::cell_grid::{live_successor, native_cells_3x3, selected_cell_list};
use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation};

/// What [`launch`] called, in the oracle's terms (observation only).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
enum Observed {
    /// An object's IronCurtain (vt+0x154), from the list of the cell the walk
    /// asked for.
    Curtain((i16, i16), u64),
    /// An override's ReceiveDamage (vt+0x16C).
    ReceiveDamage(EntityDamageEvent),
    /// FootClass's forced release of the parasite this eater holds.
    ExitUnit(u64),
}

#[cfg(test)]
thread_local! {
    /// Observation only: what [`launch`] called on this thread while a test
    /// holds `Some`.
    static OBSERVED: std::cell::RefCell<Option<Vec<Observed>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn observe(call: Observed) {
    OBSERVED.with(|log| {
        if let Some(log) = log.borrow_mut().as_mut() {
            log.push(call);
        }
    });
}

/// Launch case 1 for `owner`'s Super of type `sw_type` at (target_rx,
/// target_ry): see the module doc. Returns whether the Super was charged.
pub(crate) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    sw_type: InternedId,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    if !super::is_charged(sim, owner, sw_type) {
        return false;
    }
    super::spawn_cell_anim(
        sim,
        rules,
        &rules.general.iron_curtain_invoke_anim,
        target_rx,
        target_ry,
    );
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: target_rx,
        ry: target_ry,
    });
    sim.sound_events.push(SimSoundEvent::RadarEvent {
        radar: RadarEventRequest::new(RadarEventType::ImpactSilent, target_rx, target_ry),
    });

    let mut curtained = 0;
    for (x, y) in native_cells_3x3(target_rx, target_ry) {
        let Some(((rx, ry), layer)) = selected_cell_list(sim, x, y) else {
            continue;
        };
        let mut next = sim
            .substrate
            .occupancy
            .get(rx, ry)
            .and_then(|cell| cell.first_on_layer(layer));
        while let Some(id) = next {
            if let Some(category) = sim
                .substrate
                .entities
                .get(id)
                .filter(|entity| !entity.chrono_warp_latch())
                .map(|entity| entity.category())
            {
                #[cfg(test)]
                observe(Observed::Curtain((x, y), id));
                let duration = rules.general.iron_curtain_duration;
                match category {
                    EntityCategory::Infantry => {
                        receive_strength_as_c4(
                            sim,
                            rules,
                            overlay_registry,
                            id,
                            Some(owner),
                            true,
                            frame_effects,
                        );
                    }
                    EntityCategory::Unit | EntityCategory::Aircraft => {
                        foot_iron_curtain(
                            sim,
                            rules,
                            overlay_registry,
                            id,
                            duration,
                            frame_effects,
                        );
                    }
                    _ => {
                        let frame = sim.session.binary_frame;
                        if let Some(entity) = sim.substrate.entities.get_mut(id) {
                            apply_invulnerability(entity, frame, duration, InvulnKind::IronCurtain);
                        }
                    }
                }
                curtained += 1;
            }
            next = live_successor(sim, id, (rx, ry), layer);
        }
    }

    log::info!(
        "IronCurtain launched at ({}, {}) by '{}', {} objects called",
        target_rx,
        target_ry,
        sim.interner.resolve(owner),
        curtained
    );
    true
}

/// FootClass::IronCurtain `0x004DEAE0`, the Unit and Aircraft override
/// (`vtable__UnitClass`/`vtable__AircraftClass` +0x154). An Organic type
/// (`+0xD97`, `0x004DEAEE`; retail DLPH and SQD) receives its Strength
/// instead of the curtain, before the parasite test. Otherwise a parasite
/// eating the Foot (`+0x694`) is forced off (`0x004DEB38..0x004DEB6E`): its
/// suppression timer runs 50 frames, then ExitUnit `0x0062A4A0` deletes a
/// drone or releases a squid. Then the paralysis timer (`+0x6A0`) restarts
/// at the frame with no time (`0x004DEB73..0x004DEB8D`), and
/// TechnoClass::IronCurtain `0x0070E2B0` applies (`0x004DEB9D`).
fn foot_iron_curtain(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    duration: i32,
    frame_effects: FrameEffects<'_>,
) {
    let Some(organic) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules))
        .map(|object| object.organic)
    else {
        return;
    };
    if organic {
        receive_strength_as_c4(sim, rules, overlay_registry, id, None, false, frame_effects);
        return;
    }
    if let Some(eater) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.parasite.eater())
    {
        #[cfg(test)]
        observe(Observed::ExitUnit(eater));
        sim.parasite_force_release(
            eater,
            crate::sim::combat::parasite::FORCED_RELEASE_SUPPRESSION_FRAMES,
            rules,
            frame_effects,
        );
    }
    let frame = sim.session.binary_frame;
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.parasite.end_paralysis(frame);
        apply_invulnerability(entity, frame, duration, InvulnKind::IronCurtain);
    }
}

/// The overrides' ReceiveDamage (vt+0x16C): the type's Strength as
/// `[CombatDamage] C4Warhead=` (`Rules+0xFA8`)
/// ([`super::strength_receiver_event`]). InfantryClass::IronCurtain
/// (`0x00522600..0x00522632`) ignores defenses and names the launching house;
/// FootClass's Organic arm (`0x004DEAF8..0x004DEB2B`) does neither. The
/// shared receiver owns the death, its attribution and its announcement.
fn receive_strength_as_c4(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    house: Option<InternedId>,
    ignore_defenses: bool,
    frame_effects: FrameEffects<'_>,
) {
    let Some(event) = super::strength_receiver_event(
        sim,
        rules,
        id,
        &rules.bridge_warheads.c4_name,
        house,
        ignore_defenses,
    ) else {
        return;
    };
    #[cfg(test)]
    observe(Observed::ReceiveDamage(event));
    sim.commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects);
}
