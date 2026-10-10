//! The Chronosphere and the Chrono Warp: `SuperClass::Launch @ 0x006CC390`
//! cases 3 (`0x006CC3B9`) and 4 (`0x006CC4B2`), and the ChronoPlacement anim
//! the Super holds between them (`SuperClass+0x68`).
//!
//! The player clicks the Chronosphere on a source cell: case 3 stores the
//! cell (`+0x62`) and loops `[General] ChronoPlacement=` over it. ClickFire
//! then clears the charge without restarting the recharge (the type is
//! `PreClick=`), so the next Super AI finds the timer run out and readies it
//! again. The click on the destination fires the `[SuperWeaponTypes]` entry
//! at index 4 (`0x006CC46E`), retail ChronoWarpSpecial: `PostClick=` with
//! `PreDependent=ChronoSphere`, so Fire_SW copies the source cell from the
//! Chronosphere's Super, launches case 4 without a charge, then clears the
//! Chronosphere's charge and restarts its recharge (`StopPreclickAnim`).
//! Case 4 hands every Foot in the source's 3x3 block a Teleport locomotor
//! for the warp (`movement::teleport_chrono`, the Teleport's Chronosphere
//! states) and kills the Organic ones that cannot teleport. A Drive or Ship
//! Unit's own locomotor first takes `Force_Track(-1, destination)`; the warp
//! hands it back with head and destination in its own cell, and its next
//! Process asks Find_Path for that cell, which AStar refuses
//! (`0x00429BF3..0x00429C0A`): the destination clears, the head stays and
//! the Unit stays where it landed.
//!
//! Evidence: instruction reading (`0x006CC3B9..0x006CCD3E`, `0x006CB3A0`,
//! `0x006CB830`, `0x004FAE50`); the post-warp Process executed in
//! `tools/spatial_oracle/track_path_continuation`'s post_warp rows.
//!
//! Scenario draws: case 4's C4 kills draw through their damage receivers and
//! the destination setter may draw an Infantry sub-cell through its own;
//! Fire_SW's computer-house alert (`fire.rs`) draws after both clicks. Timer
//! writes: the Teleport's (`movement::teleport_chrono`), a Naval eater's
//! 500-frame suppression, the Chronosphere's recharge (StopPreclickAnim) and
//! the post-warp refusal's PathDelay (Foot+640, `0x004D4016..0x004D4041`).
//! Detach calls: the radio OVER_OUT to every contact, ClearBunker, the
//! eater's ExitUnit and the placement anim's release.
//!
//! RESIDUALS:
//! - An off-map cell of either block reads the shared dummy cell's
//!   coordinates natively; VERA reads the requested cell's.
//! - The warp latch (`TechnoClass+0x27C`) has two more writers,
//!   `FootClass::ChronoWarpTo @ 0x004DF7F0` (`0x004DF9EA`) and
//!   `InfantryClass::ChronoWarpTo @ 0x00522FE0` (`0x005231C1`), reached only
//!   through their vtable slots; they and their callers are not ported.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimId;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::superweapon::cell_grid::{live_successor, native_cells_3x3, selected_cell_list};
use crate::sim::superweapon::deck_coords;
use crate::sim::world::{SimSoundEvent, Simulation};

/// Leptons case 3, CreateChronoAnim and case 4 each raise their anims
/// (`CoordStruct(0, 0, 5)` at `0x006CC423`, `LEA EBP,[ECX+5]` at
/// `0x006CB43B`, `ebp + 5` at `0x006CC5DA`): the ChronoPlacement loop sits
/// twice this above the cell, the blasts once.
const CHRONO_ANIM_Z_LIFT: i32 = 5;

/// The `[SuperWeaponTypes]` index the Chronosphere's player click selects
/// for the next click (the display's selected Super `0x008809A0` = 4 at
/// `0x006CC46E`), a literal: retail ChronoWarpSpecial.
pub(crate) const CHRONO_WARP_SELECTION_INDEX: usize = 4;

impl Simulation {
    /// Every Super's held ChronoPlacement anim with its owner.
    pub(crate) fn super_placement_anims(&self) -> Vec<(InternedId, AnimId)> {
        self.super_weapons
            .iter()
            .flat_map(|(owner, weapons)| {
                weapons
                    .values()
                    .filter_map(|instance| instance.placement_anim().map(|anim| (*owner, anim)))
            })
            .filter(|(_, anim)| self.substrate.anims.get(*anim).is_some())
            .collect()
    }

    /// The Super's anim release, as `CreateChronoAnim`
    /// (`0x006CB3A6..0x006CB410`), `StopPreclickAnim` (`0x006CB839..`),
    /// ClickFire (`0x006CBBF6..`) and case 4 (`0x006CC56B..`) each inline
    /// it: the anim's remaining-loops byte (`+0x195`) drops to 0, so it plays
    /// out to its last frame and leaves, and the Super forgets it. The notify
    /// list `0x00B0F5B8` and its `+0x6C` byte only track the pointer; VERA's
    /// anim ids are never reused, so a dead anim's id finds nothing.
    pub(crate) fn release_super_anim(&mut self, owner: InternedId, sw_type: InternedId) {
        let Some(anim) = self
            .super_weapons
            .get_mut(&owner)
            .and_then(|weapons| weapons.get_mut(&sw_type))
            .and_then(|instance| instance.placement_anim.take())
        else {
            return;
        };
        if let Some(anim) = self.substrate.anims.get_mut(anim) {
            anim.runtime.loop_remaining = 0;
        }
    }
}

/// `SuperClass::CreateChronoAnim @ 0x006CB3A0`: release the held anim, then
/// construct `[General] ChronoPlacement=` at `coords` raised by
/// [`CHRONO_ANIM_Z_LIFT`] (`0x006CB43B`) with the row `(delay 0, loopCount 1,
/// drawFlags 0x600, zAdjust 0, reverse 0)` (`0x006CB431..0x006CB467`) and
/// hold it.
fn create_chrono_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    [x, y, z]: [i32; 3],
) {
    sim.release_super_anim(owner, sw_type);
    let coords = [x, y, z.wrapping_add(CHRONO_ANIM_Z_LIFT)];
    let anim = super::spawn_super_anim(sim, rules, &rules.general.chrono_placement_anim, coords);
    if let Some(instance) = sim
        .super_weapons
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&sw_type))
    {
        instance.placement_anim = anim;
    }
}

/// Case 3 (`0x006CC3B9..0x006CC4B1`): a charged Chronosphere (`+0x6F`)
/// stores the clicked cell (`+0x62`) and loops ChronoPlacement over it, 5
/// leptons above [`deck_coords`] before CreateChronoAnim adds its own 5. The
/// rest is presentation: the local player's
/// click selects the Chrono Warp for the next click (`0x006CC46E`), and
/// anyone else's anim is hidden with its sound stopped (`0x006CC485..`);
/// the app does both from the launch event.
pub(super) fn launch_source(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    cell: (u16, u16),
) {
    let Some(instance) = sim
        .super_weapons
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&sw_type))
    else {
        return;
    };
    if !instance.is_ready {
        return;
    }
    instance.chrono_cell = cell;
    let mut coords = deck_coords(sim, cell);
    coords[2] = coords[2].wrapping_add(CHRONO_ANIM_Z_LIFT);
    create_chrono_anim(sim, rules, owner, sw_type, coords);
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: cell.0,
        ry: cell.1,
    });
}

/// Case 4 (`0x006CC4B2..0x006CCD3E`), the Chrono Warp from the source cell
/// Fire_SW copied into this Super (`+0x62`) to `target`:
/// 1. `CreateRadarEvent(13)` at the source, then at the target
///    (`0x006CC4BE`, `0x006CC4D2`), on every client;
/// 2. this Super's own anim is released (`0x006CC56B`; none in retail, case
///    3 held the Chronosphere's), then `[General] ChronoBlastDest=` at the
///    target and `ChronoBlast=` at the source, each 5 leptons above its
///    [`deck_coords`] (`0x006CC5C5..0x006CC674`);
/// 3. every object of the source's 3x3 block ([`native_cells_3x3`], the bridge
///    list where the cell has one) goes through [`warp_object`], reading its
///    successor after (`0x006CCCCA`);
/// 4. the launch event: the app plays `EVA_ChronosphereActivated`
///    (`0x006CCD03`) and, for the local player, drops the selection and the
///    queued `EVA_ChronosphereReady` (`0x006CCD17..0x006CCD2D`).
pub(super) fn launch_warp(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    target: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    let Some(source) = sim
        .super_weapons
        .get(&owner)
        .and_then(|weapons| weapons.get(&sw_type))
        .map(super::SuperWeaponInstance::chrono_cell)
    else {
        return;
    };
    for (rx, ry) in [source, target] {
        sim.sound_events.push(SimSoundEvent::SuperWeaponRadarEvent {
            radar: RadarEventRequest::new(RadarEventType::ImpactSilent, rx, ry),
        });
    }
    let [dx, dy, dz] = deck_coords(sim, target);
    let [sx, sy, sz] = deck_coords(sim, source);
    sim.release_super_anim(owner, sw_type);
    super::spawn_super_anim(
        sim,
        rules,
        &rules.general.chrono_blast_dest_anim,
        [dx, dy, dz.wrapping_add(CHRONO_ANIM_Z_LIFT)],
    );
    super::spawn_super_anim(
        sim,
        rules,
        &rules.general.chrono_blast_anim,
        [sx, sy, sz.wrapping_add(CHRONO_ANIM_Z_LIFT)],
    );
    let blocks = native_cells_3x3(source.0, source.1).zip(native_cells_3x3(target.0, target.1));
    for ((x, y), destination_cell) in blocks {
        let Some(((rx, ry), layer)) = selected_cell_list(sim, x, y) else {
            continue;
        };
        let mut next = sim
            .substrate
            .occupancy
            .get(rx, ry)
            .and_then(|cell| cell.first_on_layer(layer));
        while let Some(id) = next {
            let warp = ChronoWarpLaunch {
                owner,
                source,
                target,
                destination_cell,
            };
            warp_object(sim, rules, &warp, id, overlay_registry);
            next = live_successor(sim, id, (rx, ry), layer);
        }
    }
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: target.0,
        ry: target.1,
    });
}

/// What case 4 hands each object of the source block.
struct ChronoWarpLaunch {
    /// The Super's owner (`+0x2C`): the damage source house and Techno
    /// `+0x42C`.
    owner: InternedId,
    /// The source cell (`+0x62`).
    source: (u16, u16),
    /// The clicked target cell.
    target: (u16, u16),
    /// The target cell plus this source cell's offset.
    destination_cell: (i16, i16),
}

/// One object of case 4's walk (`0x006CC742..0x006CCCC7`):
/// - only a Foot (AbstractFlags `0x4`) that is not in the air (vt+0x54);
/// - a Naval eater holding it (retail SQD) lets go: suppression 500, then
///   ExitUnit (`0x006CC763..0x006CC7B7`);
/// - an `Organic=` type that is no `Teleporter=` takes its authored Strength
///   as `C4Warhead=` damage, no attacker, ignoreDefenses, credited to the
///   Super's owner (`0x006CC85A..0x006CC8C6`);
/// - otherwise one not under the Iron Curtain (vt+0x160), not latched
///   (`+0x27C`), not warping out or in (vt+0x1D4/+0x1D8) and not a Unit still
///   standing in the war factory its radio contact names
///   ([`Simulation::unit_in_contact_war_factory`]) is armed
///   ([`arm_chrono_warp`]).
fn warp_object(
    sim: &mut Simulation,
    rules: &RuleSet,
    warp: &ChronoWarpLaunch,
    id: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    let in_air = |sim: &Simulation, id: u64| {
        sim.substrate.entities.get(id).is_some_and(|entity| {
            crate::sim::movement::air_movement::is_high_flying(
                entity,
                sim.resolved_terrain.as_ref(),
                Some((rules, &sim.interner)),
            )
        })
    };
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let category = entity.category;
    if !matches!(
        category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    ) || in_air(sim, id)
    {
        return;
    }
    if let Some(eater) = entity.parasite_eating_me
        && sim.substrate.entities.get(eater).is_some_and(|eater| {
            eater.parasite.is_some()
                && sim
                    .object_type(eater.type_ref(), rules)
                    .is_some_and(|object| object.naval)
        })
    {
        sim.parasite_force_release(
            eater,
            crate::sim::combat::parasite::CHRONO_WARP_SUPPRESSION_FRAMES,
            rules,
        );
    }
    let on_factory = category == EntityCategory::Unit && sim.unit_in_contact_war_factory(id, rules);
    let Some(object) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
    else {
        return;
    };
    let (organic, teleporter, strength) = (object.organic, object.teleporter, object.strength);
    if organic && !teleporter && !in_air(sim, id) {
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            id,
            strength,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            Some(warp.owner),
            sim.interner.intern(&rules.bridge_warheads.c4_name),
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        sim.commit_direct_damage_receiver(rules, overlay_registry, event);
        return;
    }
    let frame = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if crate::sim::superweapon::invulnerability::is_invulnerable(
        entity.invulnerability.as_ref(),
        frame,
    ) || entity.chrono_warp_latch()
        || entity.is_warped_out()
        || entity.is_warping_in()
        || on_factory
    {
        return;
    }
    arm_chrono_warp(sim, rules, warp, id, overlay_registry);
}

/// Case 4's arming of one Foot (`0x006CC91D..0x006CCCA8`):
/// 1. a Unit in a Tank Bunker leaves it (`BuildingClass::ClearBunker`);
/// 2. the destination: the cell at the target plus this cell's offset, its
///    [`deck_coords`] (`0x006CC9AF..0x006CCA48`);
/// 3. a Unit first releases its locomotor's occupation (`+0x9C(0)`), forces
///    its track to the destination (`Force_Track(-1, ..)`), clears its raw
///    occupation there (vt+0xF4) and raises Foot `+0x6B6`
///    (`0x006CCA4C..0x006CCAB4`);
/// 4. a fresh Teleport links the Foot and piggybacks its locomotor
///    (`0x006CC989`, `0x006CCAD5`, `0x006CCB4A..0x006CCB59`);
/// 5. any other Foot lands at the target cell's coordinates plus its own
///    offset from the source cell's, on the deck where that cell has a bridge
///    (`0x006CCB6A..0x006CCC29`);
/// 6. the latch, the destination (`+0x288`) and the owner (`+0x42C`), around
///    the radio OVER_OUT to every contact (vt+0x280(3)), then the class
///    setter's destination at that cell (vt+0x480(cell, 1)).
fn arm_chrono_warp(
    sim: &mut Simulation,
    rules: &RuleSet,
    warp: &ChronoWarpLaunch,
    id: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.locomotor.is_none() {
        return;
    }
    let category = entity.category;
    if let Some(bunker) = entity.bunker_link.installed_in()
        && sim
            .substrate
            .entities
            .get(bunker)
            .is_some_and(|building| building.category == EntityCategory::Structure)
    {
        crate::sim::docking::bunker_link::clear_bunker(sim, bunker, rules);
    }
    let (cx, cy) = warp.destination_cell;
    let [x, y, z] = deck_coords(sim, (cx as u16, cy as u16));
    let mut destination = DriveCoord { x, y, z };
    if category == EntityCategory::Unit {
        sim.locomotor_mark_all_occupation_bits_up(id);
        let _ = sim.force_track(id, -1, destination, Some(rules), overlay_registry);
        sim.object_raw_receiver_at(id, destination, false);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.foot_occupation_enabled = true;
        }
    }
    let frame = sim.session.binary_frame;
    let Some(locomotor) = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.locomotor.as_mut())
    else {
        return;
    };
    // Whatever is active goes into the fresh Teleport's slot, a driving
    // Chrono Miner's Drive with the miner's Teleport inside it; the result
    // goes unread (`0x006CCB4A`).
    locomotor.begin_piggyback(LocomotorKind::Teleport, frame);
    if category != EntityCategory::Unit {
        let Some(entity) = sim.substrate.entities.get(id) else {
            return;
        };
        // The object's GetCoords (vt+0x48, `0x006CCB99`).
        let here = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let [ox, oy, oz] = super::fire::cell_coords(sim, warp.source);
        let [tx, ty, tz] = super::fire::cell_coords(sim, warp.target);
        destination = DriveCoord {
            x: tx.wrapping_add(here.x.wrapping_sub(ox)),
            y: ty.wrapping_add(here.y.wrapping_sub(oy)),
            z: tz.wrapping_add(here.z.wrapping_sub(oz)),
        };
        // `MapClass::GetCellAt(CoordStruct) @ 0x00565730`.
        if super::cell_grid::cell_has_bridge_flag(
            sim,
            (destination.x / 256) as i16,
            (destination.y / 256) as i16,
        ) {
            destination.z = destination
                .z
                .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        }
    }
    let Some(runtime) = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.locomotor.as_mut())
        .and_then(|locomotor| locomotor.teleport_runtime_mut())
    else {
        return;
    };
    runtime.arm_chrono(crate::sim::movement::teleport_movement::ChronoWarp::new(
        destination,
        warp.owner,
        frame,
    ));
    crate::sim::radio::broadcast_break(sim, id, Some(rules));
    if let Err(error) = sim.assign_destination_represented(
        id,
        Some(NavTargetRef::cell(cx as u16, cy as u16)),
        Some(rules),
        overlay_registry,
    ) {
        log::debug!("chrono warp {id} destination: {error:?}");
    }
}
