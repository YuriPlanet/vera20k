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
//! TAction109 (`0x006E36E0`) shares the receiver overrides below, but its
//! ground-only cell-spread walk, null house and unraised invoke animation
//! differ from SuperClass::Launch; see [`at_waypoint`].
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
#[cfg(any(test, feature = "test-support"))]
use crate::sim::combat::EntityDamageEvent;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::superweapon::cell_grid::{live_successor, native_cells_3x3, selected_cell_list};
use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation};

/// What [`launch`] called, in the oracle's terms (observation only).
#[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Observation only: what [`launch`] called on this thread while a test
    /// holds `Some`.
    static OBSERVED: std::cell::RefCell<Option<Vec<Observed>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
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
    // This case plays no positional cue: the target cell's centre, with no
    // Map lookup (a lookup off the map would stamp the shared Dummy).
    let world = crate::sim::components::DriveCoord::cell(target_rx, target_ry, 0).into();
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world,
    });
    sim.sound_events.emit(SimSoundEvent::RadarEvent {
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
                #[cfg(any(test, feature = "test-support"))]
                observe(Observed::Curtain((x, y), id));
                curtain_object(
                    sim,
                    rules,
                    overlay_registry,
                    id,
                    category,
                    Some(owner),
                    frame_effects,
                );
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

/// Execute109 -> IronCurtainAtWP6E36E0..6E37E4 (Steam gamemd).
/// GetWaypoint68BCC0, Cell486840 GetCoords +5, then the first nine entries
/// of CellSpread ABD490 (count7ED3D4). Unlike the Super, it reads only +E4,
/// passes (duration, NULL, 0), and does not test a Foot's +27C warp latch.
/// Each +30 successor and Rules+FE8 duration is read after prior callbacks.
/// No Super admission/charge/EVA/radar event and no action-value/House use.
/// RNG, timers and detach calls remain in the shared anim/receiver owners.
pub(crate) fn at_waypoint(
    sim: &mut Simulation,
    rules: &RuleSet,
    waypoint: i32,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let Some(cell) = sim.session.waypoint_cell(waypoint) else {
        return false;
    };
    if sim.resolved_terrain.is_none() {
        return false;
    }
    let coord = crate::sim::cell_rect::get_cellclass_fallback(
        sim.resolved_terrain.as_ref(),
        i32::from(cell.0),
        i32::from(cell.1),
    )
    .center_coordinate()
    .expect("IronCurtain waypoint requires a supported Cell slope");
    super::spawn_super_anim(
        sim,
        rules,
        &rules.general.iron_curtain_invoke_anim,
        [coord.x, coord.y, coord.z.wrapping_add(5)],
    );
    for &(dx, dy) in crate::sim::combat::cell_spread::sweep(1) {
        let (x, y) = (cell.0.wrapping_add(dx), cell.1.wrapping_add(dy));
        let crate::sim::cell_rect::CellRef::Real(real) =
            crate::sim::cell_rect::get_cellclass_fallback(
                sim.resolved_terrain.as_ref(),
                i32::from(x),
                i32::from(y),
            )
        else {
            continue;
        };
        let cell = (real.rx, real.ry);
        let layer = MovementLayer::Ground;
        let mut next = sim
            .substrate
            .occupancy
            .get(cell.0, cell.1)
            .and_then(|cell| cell.first_on_layer(layer));
        while let Some(id) = next {
            if let Some(category) = sim
                .substrate
                .entities
                .get(id)
                .map(|entity| entity.category())
            {
                #[cfg(any(test, feature = "test-support"))]
                observe(Observed::Curtain((x, y), id));
                curtain_object(
                    sim,
                    rules,
                    overlay_registry,
                    id,
                    category,
                    None,
                    frame_effects,
                );
            }
            next = live_successor(sim, id, cell, layer);
        }
    }
    true
}

/// One dispatch of ObjectClass vt+154. Both Super6CCFF8 and Action6E37C9
/// enter the same Infantry522600 / Foot4DEAE0 / Building457C90 overrides.
fn curtain_object(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    category: EntityCategory,
    house: Option<InternedId>,
    frame_effects: FrameEffects<'_>,
) {
    let duration = rules.general.iron_curtain_duration;
    match category {
        EntityCategory::Infantry => {
            // Phobos redirects `InfantryClass::IronCurtain` to the Foot path
            // and lets `IronCurtain.Effect` decide: kill, curtain or nothing.
            #[cfg(feature = "phobos")]
            {
                let Some(object) = sim
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules))
                else {
                    return;
                };
                match organic_curtain_outcome(rules, object, InvulnKind::IronCurtain) {
                    (crate::rules::invulnerability_rules::IronCurtainEffect::Ignore, _) => {}
                    (crate::rules::invulnerability_rules::IronCurtainEffect::Invulnerable, _) => {
                        let frame = sim.session.binary_frame();
                        if let Some(entity) = sim.substrate.entities.get_mut(id) {
                            apply_invulnerability(entity, frame, duration, InvulnKind::IronCurtain);
                        }
                    }
                    (crate::rules::invulnerability_rules::IronCurtainEffect::Kill, warhead) => {
                        receive_strength_as_c4(
                            sim,
                            rules,
                            overlay_registry,
                            id,
                            warhead.as_deref(),
                            house,
                            true,
                            frame_effects,
                        )
                    }
                }
            }
            #[cfg(not(feature = "phobos"))]
            receive_strength_as_c4(
                sim,
                rules,
                overlay_registry,
                id,
                None,
                house,
                true,
                frame_effects,
            )
        }
        EntityCategory::Unit | EntityCategory::Aircraft => {
            foot_iron_curtain(sim, rules, overlay_registry, id, duration, frame_effects)
        }
        _ => {
            let frame = sim.session.binary_frame();
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                apply_invulnerability(entity, frame, duration, InvulnKind::IronCurtain);
            }
        }
    }
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
        #[cfg(feature = "phobos")]
        {
            let Some(object) = sim
                .substrate
                .entities
                .get(id)
                .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules))
            else {
                return;
            };
            match organic_curtain_outcome(rules, object, InvulnKind::IronCurtain) {
                (crate::rules::invulnerability_rules::IronCurtainEffect::Ignore, _) => return,
                (crate::rules::invulnerability_rules::IronCurtainEffect::Kill, warhead) => {
                    receive_strength_as_c4(
                        sim,
                        rules,
                        overlay_registry,
                        id,
                        warhead.as_deref(),
                        None,
                        false,
                        frame_effects,
                    );
                    return;
                }
                // Invulnerable falls through to the parasite/paralysis/curtain
                // block below, as the hook's MakeInvulnerable arm does.
                (crate::rules::invulnerability_rules::IronCurtainEffect::Invulnerable, _) => {}
            }
        }
        #[cfg(not(feature = "phobos"))]
        {
            receive_strength_as_c4(
                sim,
                rules,
                overlay_registry,
                id,
                None,
                None,
                false,
                frame_effects,
            );
            return;
        }
    }
    if let Some(eater) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.parasite.eater())
    {
        #[cfg(any(test, feature = "test-support"))]
        observe(Observed::ExitUnit(eater));
        sim.parasite_force_release(
            eater,
            crate::sim::combat::parasite::FORCED_RELEASE_SUPPRESSION_FRAMES,
            rules,
            frame_effects,
        );
    }
    let frame = sim.session.binary_frame();
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.parasite.end_paralysis(frame);
        apply_invulnerability(entity, frame, duration, InvulnKind::IronCurtain);
    }
}

/// Phobos `FootClass_IronCurtain_Organics` (`0x4DEAEE`): the TechnoType's
/// `IronCurtain.Effect`/`ForceShield.Effect` override, else the
/// `[CombatDamage]` default, and the kill warhead the type/global pair
/// supplies (else `C4Warhead=`).
#[cfg(feature = "phobos")]
fn organic_curtain_outcome(
    rules: &RuleSet,
    object: &crate::rules::object_type::ObjectType,
    kind: InvulnKind,
) -> (
    crate::rules::invulnerability_rules::IronCurtainEffect,
    Option<String>,
) {
    let globals = rules.invulnerability_globals();
    let (options, curtain) = match kind {
        InvulnKind::IronCurtain => (
            &object.phobos_invulnerability.iron_curtain,
            &globals.iron_curtain,
        ),
        InvulnKind::ForceShield => (
            &object.phobos_invulnerability.force_shield,
            &globals.force_shield,
        ),
    };
    (
        options
            .effect_on_organics
            .unwrap_or(curtain.effect_on_organics),
        options
            .kill_warhead
            .clone()
            .or_else(|| curtain.kill_organics_warhead.clone()),
    )
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
    warhead: Option<&str>,
    house: Option<InternedId>,
    ignore_defenses: bool,
    frame_effects: FrameEffects<'_>,
) {
    let warhead = warhead.unwrap_or(&rules.bridge_warheads.c4_name);
    let Some(event) = super::strength_receiver_event(
        &mut sim.interner,
        &sim.substrate,
        rules,
        id,
        warhead,
        house,
        ignore_defenses,
    ) else {
        return;
    };
    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::ReceiveDamage(event));
    sim.commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects);
}
