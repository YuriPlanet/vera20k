//! The Genetic Mutator's launch: `SuperClass::Launch @ 0x006CC390` case 9
//! (`0x006CD7E7..0x006CDA62`).
//!
//! A charged Super (`+0x6F`) builds `[General] IonBlast=` 5 leptons over the
//! cell's deck coordinate (`0x006CD7F9..0x006CD8A5`), plays
//! `EVA_GeneticMutatorActivated` (`0x006CD8BD`) and `[AudioVisual]
//! GeneticMutatorActivateSound=` at the deck coordinate (`0x006CD8D3`), and
//! raises a type-13 radar event at the cell (`0x006CD8E0`). Then:
//! - Under `[General] MutateExplosion=` (`Rules+0x17C8`), one Apply_area_damage
//!   at the deck coordinate: 10000 damage, no source, `[SpecialWeapons]
//!   MutateExplosionWarhead=` and the Super's house (`0x006CD8F4..0x006CD90C`).
//! - Otherwise a walk of the 3x3 block around the cell in the offset table's
//!   order ([`native_cells_3x3`]): each cell's bridge list (`+0xE8`) when it
//!   carries the bridge flag (`+0x140 & 0x100`), else its ground list
//!   (`+0xE4`). Each infantryman (WhatAmI 0xF) on it receives its type's
//!   Strength (`+0xA0`) as `MutateWarhead=` damage at distance 0, with no
//!   source, ignoring defenses, from the Super's house (vt+0x16C,
//!   `0x006CD9F3..0x006CDA29`). The walk reads the next object (`+0x30`)
//!   before the call (`0x006CD9E0`).
//!
//! The mutation belongs to the receiver. InfantryClass::ReceiveDamage's
//! InfDeath 9 arm builds `[General] InfantryMutate=` owned by the house
//! (`world::infantry_death`), and that anim's end creates the
//! `AnimToInfantry=` infantryman (AnimClass::AI's MakeInfantry block,
//! `anim_class`). On retail rules both warheads have InfDeath 9, the anim is
//! GENDEATH and the infantryman a BRUTE.
//!
//! The local player's tail (`0x006CDA44..0x006CDA5D`, then the shared
//! `0x006CD51E`) clears the selected Super and drops the queued
//! `EVA_GeneticMutatorReady` line; the app does both from the launch event.
//!
//! Evidence: `tools/superweapon_oracle.py` sections `genetic_launch`,
//! `infantry_mutate_death` and `make_infantry` (Unicorn on gamemd.exe: case 9,
//! the InfDeath 9 arm and the MakeInfantry block), replayed in
//! `genetic_converter_tests.rs`.
//!
//! RESIDUALS:
//! - The EVA line is skipped natively while `0x00A8B538` is set
//!   (`HouseClass::MPlayer_Defeated @ 0x004FC205`), on every client after the
//!   local player's multiplayer defeat; VERA always plays it.
//! - A null `IonBlast=` or warhead reaches AnimClass or the receivers as a
//!   null type natively; VERA builds or damages nothing. Retail sets RING1,
//!   Mutate and MutateExplosion.
//!
//! Ledger:
//! - Scenario draws: the IonBlast anim's constructor, then the receivers':
//!   each victim's PlaceInfantryInCell and InfantryMutate constructor among
//!   them.
//! - Timer writes and detach calls: none of the launch's own; each death's
//!   are its receiver's.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/superweapon/cell_grid, sim/combat,
//!   sim/radar, sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

#[cfg(test)]
#[path = "genetic_converter_tests.rs"]
mod tests;

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
#[cfg(feature = "ares")]
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::combat::RAD_NO_ATTACKER;
use crate::sim::combat::world_receiver::apply_area_damage;
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::superweapon::cell_grid::{live_successor, native_cells_3x3, selected_cell_list};
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation};

/// Case 9's area damage (`MOV EDX, 0x2710`, `0x006CD903`).
const MUTATE_EXPLOSION_DAMAGE: i32 = 10_000;

/// What [`launch`] called, in the oracle's terms (observation only).
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq)]
enum Observed {
    /// Apply_area_damage: the coordinate, the damage and the (source,
    /// house, warhead).
    AreaDamage([i32; 3], i32, (u64, Option<InternedId>, InternedId)),
    /// An infantryman's ReceiveDamage, from the list of the cell the walk
    /// asked for.
    ReceiveDamage((i16, i16), crate::sim::combat::EntityDamageEvent),
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

/// Launch case 9 for `owner`'s Super of type `sw_type` at (target_rx,
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
    let general = &rules.general;
    super::spawn_cell_anim(sim, rules, &general.ion_blast_anim, target_rx, target_ry);
    // The cue (`0x006CD8D3`) and MutateExplosion both use the deck
    // coordinate `0x006CD7F2..0x006CD84C` built.
    let deck = super::deck_coords(sim, (target_rx, target_ry));
    let [x, y, z] = deck;
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world: crate::sim::anim_class::AnimWorldCoord { x, y, z },
    });
    sim.sound_events.emit(SimSoundEvent::RadarEvent {
        radar: RadarEventRequest::new(RadarEventType::ImpactSilent, target_rx, target_ry),
    });

    if general.mutate_explosion {
        let Some(warhead) = rules.warhead(&general.mutate_explosion_warhead) else {
            return true;
        };
        let [x, y, z] = deck;
        let origin = (
            RAD_NO_ATTACKER,
            Some(owner),
            sim.interner.intern(&general.mutate_explosion_warhead),
        );
        #[cfg(any(test, feature = "test-support"))]
        observe(Observed::AreaDamage(
            [x, y, z],
            MUTATE_EXPLOSION_DAMAGE,
            origin,
        ));
        // SuperClass::Launch6CC390 passes affectsTiberium=false at6CD8FE,
        // independently of WH.Tiberium (original gamemd.exe).
        apply_area_damage(
            sim,
            rules,
            overlay_registry,
            crate::sim::projectile::ProjectileCoord::new(x, y, z),
            MUTATE_EXPLOSION_DAMAGE,
            warhead,
            false,
            origin,
            frame_effects,
        );
        return true;
    }

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
            next = live_successor(sim, id, (rx, ry), layer);
            let infantry = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Infantry);
            if !infantry {
                continue;
            }
            if let Some(event) = super::strength_receiver_event(
                &mut sim.interner,
                &sim.substrate,
                rules,
                id,
                &general.mutate_warhead,
                Some(owner),
                true,
            ) {
                #[cfg(any(test, feature = "test-support"))]
                observe(Observed::ReceiveDamage((x, y), event));
                sim.commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects);
            }
        }
    }
    true
}

/// SW_GeneticMutator::GetWarhead (Ares 3.0 `10072030`): the type's non-null
/// `SW.Warhead`, else `[SpecialWeapons] MutateExplosionWarhead=` under its
/// `Mutate.Explosion` (unset: `[General] MutateExplosion=`), else
/// `MutateWarhead=`.
#[cfg(feature = "ares")]
fn ares_warhead<'r>(rules: &'r RuleSet, sw: &SuperWeaponType) -> Option<&'r str> {
    let launch = rules.super_weapon_metadata(&sw.id)?.launch();
    if let Some(warhead) = launch.warhead().flatten() {
        return Some(warhead);
    }
    let general = &rules.general;
    Some(
        if launch
            .mutate()
            .explosion
            .unwrap_or(general.mutate_explosion)
        {
            &general.mutate_explosion_warhead
        } else {
            &general.mutate_warhead
        },
    )
}

/// SW_GeneticMutator::Activate (Ares 3.0 `10071C60`, hook 6CC390) for
/// `owner`'s Super of `sw` at `cell`, after GetWarhead (`10072030`). Charged:
/// - with `Mutate.Explosion` (else `[General] MutateExplosion=`), one
///   DamageArea at the cell's deck coordinates with GetDamage (`10071F60`:
///   `SW.Damage`, else 10000), no source object, not affecting Tiberium,
///   from the owner;
/// - otherwise each infantryman on GetRange (`10071F80`: `SW.Range`, else a
///   3x3 rectangle, or radius 5 for an explosion), once and in UniqueID order,
///   that `SW.AffectsHouse`/`SW.AffectsTarget` admit, unless a `Cyborg=` one
///   with `Mutate.IgnoreCyborg` or a `NotHuman=` one with
///   `Mutate.IgnoreNotHuman`, takes its Strength as damage ignoring
///   defenses from the owner: `C4Warhead=` for a `Natural=` one with
///   `Mutate.KillNatural`, else the warhead.
///
/// The IonBlast anim, the sound and the radar event are the common Launch's
/// (GetAnim `10071F30`, GetSound `10072000`). It reports activation even
/// when uncharged.
#[cfg(feature = "ares")]
pub(super) fn ares_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if !super::is_charged(sim, owner, sw_type) {
        return;
    }
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return;
    };
    let launch = metadata.launch();
    let mutate = launch.mutate();
    let explosion = mutate.explosion.unwrap_or(rules.general.mutate_explosion);
    let warhead_name = ares_warhead(rules, sw).unwrap_or_default().to_owned();
    if explosion {
        let Some(warhead) = rules.warhead(&warhead_name) else {
            return;
        };
        let [x, y, z] = super::deck_coords(sim, cell);
        let origin = (
            RAD_NO_ATTACKER,
            Some(owner),
            sim.interner.intern(&warhead_name),
        );
        apply_area_damage(
            sim,
            rules,
            overlay_registry,
            crate::sim::projectile::ProjectileCoord::new(x, y, z),
            launch.damage().unwrap_or(10000),
            warhead,
            false,
            origin,
            frame_effects,
        );
        return;
    }
    let range = match launch.range() {
        (width, height) if width < 0.0 && height < 0 => (3.0, 3),
        range => range,
    };
    let targeting = metadata.targeting();
    for id in super::protect::area_technos(sim, range, cell) {
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        if entity.category() != EntityCategory::Infantry
            || !targeting
                .affects_house()
                .intersects(sim.ares_house_relation(owner, entity.owner()))
            || !super::protect::techno_affected(sim, entity, targeting.affects_target())
        {
            continue;
        }
        let Some(object) = sim.object_type(entity.type_ref(), rules) else {
            continue;
        };
        if object.cyborg && mutate.ignore_cyborg || object.not_human && mutate.ignore_not_human {
            continue;
        }
        let warhead = if object.natural && mutate.kill_natural {
            rules.bridge_warheads.c4_name.clone()
        } else {
            warhead_name.clone()
        };
        if let Some(event) = super::strength_receiver_event(
            &mut sim.interner,
            &sim.substrate,
            rules,
            id,
            &warhead,
            Some(owner),
            true,
        ) {
            sim.commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects);
        }
    }
}
