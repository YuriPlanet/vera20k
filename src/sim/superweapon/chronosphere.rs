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
//!
//! The other warp-latch writers, Foot4DF7F0 and Infantry522FE0, share the
//! same Teleport state through `Simulation::foot_chrono_warp_to`; House
//! Action128 uses that owner without running the Super's area/eater path.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimId;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::superweapon::SuperWeapons;
use crate::sim::superweapon::cell_grid::{live_successor, native_cells_3x3, selected_cell_list};
use crate::sim::superweapon::deck_coords;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{SimSoundEvent, Simulation};

/// Leptons case 3, CreateChronoAnim and case 4 each raise their anims
/// (`CoordStruct(0, 0, 5)` at `0x006CC423`, `LEA EBP,[ECX+5]` at
/// `0x006CB43B`, `ebp + 5` at `0x006CC5DA`): the ChronoPlacement loop sits
/// twice this above the cell, the blasts once.
const CHRONO_ANIM_Z_LIFT: i32 = 5;

/// The `[SuperWeaponTypes]` index the Chronosphere's player click selects
/// for the next click (the display's selected Super `0x008809A0` = 4 at
/// `0x006CC46E`), a literal: retail ChronoWarpSpecial.
pub const CHRONO_WARP_SELECTION_INDEX: usize = 4;

impl Simulation {
    /// Every Super's held ChronoPlacement anim with its owner.
    pub fn super_placement_anims(&self) -> Vec<(InternedId, AnimId)> {
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
    pub(crate) fn release_super_anim(
        substrate: &mut ObjectSubstrate,
        super_weapons: &mut SuperWeapons,
        owner: InternedId,
        sw_type: InternedId,
    ) {
        let Some(anim) = super_weapons
            .instance_mut(&owner, &sw_type)
            .and_then(|instance| instance.placement_anim.take())
        else {
            return;
        };
        if let Some(anim) = substrate.anims.get_mut(anim) {
            anim.end_loops();
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
    Simulation::release_super_anim(&mut sim.substrate, &mut sim.super_weapons, owner, sw_type);
    let coords = [x, y, z.wrapping_add(CHRONO_ANIM_Z_LIFT)];
    let anim = super::spawn_super_anim(sim, rules, &rules.general.chrono_placement_anim, coords);
    if let Some(instance) = sim.super_weapons.instance_mut(&owner, &sw_type) {
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
    let Some(instance) = sim.super_weapons.instance_mut(&owner, &sw_type) else {
        return;
    };
    if !instance.is_ready {
        return;
    }
    instance.chrono_cell = cell;
    let mut coords = deck_coords(sim, cell);
    coords[2] = coords[2].wrapping_add(CHRONO_ANIM_Z_LIFT);
    create_chrono_anim(sim, rules, owner, sw_type, coords);
    // This case plays no positional cue: the target cell's centre, with no
    // Map lookup (a lookup off the map would stamp the shared Dummy).
    let world = crate::sim::components::DriveCoord::cell(cell.0, cell.1, 0).into();
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world,
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
    frame_effects: FrameEffects<'_>,
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
        sim.sound_events.emit(SimSoundEvent::RadarEvent {
            radar: RadarEventRequest::new(RadarEventType::ImpactSilent, rx, ry),
        });
    }
    let [dx, dy, dz] = deck_coords(sim, target);
    let [sx, sy, sz] = deck_coords(sim, source);
    Simulation::release_super_anim(&mut sim.substrate, &mut sim.super_weapons, owner, sw_type);
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
            warp_object(sim, rules, &warp, id, overlay_registry, frame_effects);
            next = live_successor(sim, id, (rx, ry), layer);
        }
    }
    // This case plays no positional cue: the target cell's centre, with no
    // Map lookup (a lookup off the map would stamp the shared Dummy).
    let world = crate::sim::components::DriveCoord::cell(target.0, target.1, 0).into();
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world,
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
    frame_effects: FrameEffects<'_>,
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
    let category = entity.category();
    if !matches!(
        category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    ) || in_air(sim, id)
    {
        return;
    }
    if let Some(eater) = entity.parasite.eater()
        && sim.substrate.entities.get(eater).is_some_and(|eater| {
            eater.parasite.class().is_some()
                && sim
                    .object_type(eater.type_ref(), rules)
                    .is_some_and(|object| object.naval)
        })
    {
        sim.parasite_force_release(
            eater,
            crate::sim::combat::parasite::CHRONO_WARP_SUPPRESSION_FRAMES,
            rules,
            frame_effects,
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
        sim.commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects);
        return;
    }
    let frame = sim.session.binary_frame();
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
    arm_chrono_warp(sim, rules, warp, id, overlay_registry, frame_effects);
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
    frame_effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.locomotor.is_none() {
        return;
    }
    let category = entity.category();
    if let Some(bunker) = entity.bunker.link().installed_in()
        && sim
            .substrate
            .entities
            .get(bunker)
            .is_some_and(|building| building.category() == EntityCategory::Structure)
    {
        crate::sim::docking::bunker_link::clear_bunker(sim, bunker, rules, frame_effects);
    }
    let (cx, cy) = warp.destination_cell;
    let [x, y, z] = deck_coords(sim, (cx as u16, cy as u16));
    let mut destination = DriveCoord { x, y, z };
    if category == EntityCategory::Unit {
        sim.locomotor_mark_all_occupation_bits(id, false);
        let _ = sim.force_track(id, -1, destination, rules, overlay_registry, frame_effects);
        Simulation::object_raw_receiver_at(
            &mut sim.substrate,
            &sim.path_grid,
            &sim.resolved_terrain,
            id,
            destination,
            false,
        );
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.foot_occupation_enabled.resume();
        }
    }
    let frame = sim.session.binary_frame();
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
        // Both cells' GetCoords (vt+0x48, `0x00486840`).
        let terrain = sim.resolved_terrain.as_ref();
        let source =
            crate::sim::projectile::cell_ground_coord(terrain, warp.source.0, warp.source.1);
        let target =
            crate::sim::projectile::cell_ground_coord(terrain, warp.target.0, warp.target.1);
        destination = DriveCoord {
            x: target.x.wrapping_add(here.x.wrapping_sub(source.x)),
            y: target.y.wrapping_add(here.y.wrapping_sub(source.y)),
            z: target.z.wrapping_add(here.z.wrapping_sub(source.z)),
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
        frame,
    ));
    crate::sim::radio::broadcast_break(sim, id, rules, frame_effects);
    // `0x006CCC67`: the Super's owner, credited if the warp sinks the object.
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.drop_credit.credit_warp(warp.owner);
    }
    if let Err(error) = sim.assign_destination_represented(
        id,
        Some(NavTargetRef::cell(cx as u16, cy as u16)),
        rules,
        overlay_registry,
        frame_effects,
    ) {
        log::debug!("chrono warp {id} destination: {error:?}");
    }
}

/// SW_ChronoSphere::Activate (Ares 3.0 `1006ED30`, hook 6CC390): a charged
/// Super stores the cell (`+0x62`) and, with an anim from GetAnim
/// (`1006EE80`: `SW.Animation`, else `[General] ChronoPlacement=`), holds it
/// over the cell's deck coordinates raised by `SW.AnimationHeight`
/// (Initialize 5) through CreateChronoAnim (`10032470`): the held anim is
/// released, the new one built with `(0, 1, 0x600, 0, 0)` and hidden from
/// the Houses `SW.AnimationVisibility` (Initialize: the owner's team)
/// excludes. The player's click selects [`ares_chrono_warp_for`] for the next
/// click (the client, from the launch event). It reports activation even when
/// uncharged.
#[cfg(feature = "ares")]
pub(super) fn ares_launch_source(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    cell: (u16, u16),
) {
    if !super::is_charged(sim, owner, sw_type) {
        return;
    }
    if let Some(instance) = sim.super_weapons.instance_mut(&owner, &sw_type) {
        instance.chrono_cell = cell;
    }
    let presentation = &sw.presentation;
    let animation = match &presentation.animation {
        Some(animation) => animation.clone(),
        None => Some(rules.general.chrono_placement_anim.clone()),
    };
    let Some(animation) = animation else {
        return;
    };
    let [x, y, z] = deck_coords(sim, cell);
    let coords = [x, y, z.wrapping_add(presentation.height)];
    Simulation::release_super_anim(&mut sim.substrate, &mut sim.super_weapons, owner, sw_type);
    let anim = super::spawn_super_anim(sim, rules, &animation, coords);
    if let Some(anim) = anim {
        super::ares_presentation::hide_from_unaffected_viewers(
            sim,
            anim,
            owner,
            presentation.visibility,
        );
    }
    if let Some(instance) = sim.super_weapons.instance_mut(&owner, &sw_type) {
        instance.placement_anim = anim;
    }
}

/// The ChronoWarp type a ChronoSphere's player click selects (Ares
/// `1006ED30`): the `[SuperWeaponTypes]` entry `SW.PostDependent` names when
/// its `Type=` is ChronoWarp, else the first ChronoWarp in list order.
#[cfg(feature = "ares")]
pub fn ares_chrono_warp_for<'r>(
    rules: &'r RuleSet,
    sphere: &crate::rules::superweapon_type::SuperWeaponType,
) -> Option<&'r str> {
    use crate::rules::superweapon_type::SuperWeaponKind;
    let warp = |name: &&String| {
        rules
            .super_weapon(name)
            .is_some_and(|sw| sw.kind == SuperWeaponKind::ChronoWarp)
    };
    let named = rules
        .super_weapon_metadata(&sphere.id)
        .map(|metadata| metadata.launch().post_dependent())
        .filter(|name| !name.is_empty())
        .and_then(|name| rules.super_weapon_index(name))
        .and_then(|index| rules.super_weapon_order.get(index))
        .filter(warp);
    named
        .or_else(|| rules.super_weapon_order.iter().find(warp))
        .map(String::as_str)
}

/// SW_ChronoWarp::Activate (Ares 3.0 `1006FB60`, hook 6CC390) for `owner`'s
/// warp Super `sw_type` at `target`. Its source is the owner's Super at
/// HouseExt SWLastIndex; anything but a ChronoSphere there refuses the
/// activation (false), and Launch then runs the stock case. With the
/// source's SWTypeExt:
/// 1. two type-13 radar events, at the source cell, then at the target, while
///    its `SW.CreateRadarEvent` holds;
/// 2. this Super's own anim is released, then `Chronosphere.BlastSrc` (else
///    `[General] ChronoBlast=`) at the source and `Chronosphere.BlastDest`
///    (else `ChronoBlastDest=`) at the target, each over the cell's deck
///    coordinates raised by `SW.AnimationHeight`;
/// 3. every Techno on its GetRange (`1006EEB0`: `SW.Range`, else a 3x3
///    rectangle) around the source, once and in UniqueID order, goes through
///    [`ares_chronoport`].
#[cfg(feature = "ares")]
pub(super) fn ares_launch_warp(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    target: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    use crate::rules::superweapon_type::SuperWeaponKind;
    let Some((source_name, source_sw)) = super::ares_post_click_source(sim, rules, owner) else {
        return false;
    };
    if source_sw.kind != SuperWeaponKind::ChronoSphere {
        return false;
    }
    let Some(source_type) = sim.interner.get(source_name) else {
        return false;
    };
    let Some(source) = sim
        .super_weapons
        .get(&owner)
        .and_then(|weapons| weapons.get(&source_type))
        .map(super::SuperWeaponInstance::chrono_cell)
    else {
        return false;
    };
    let Some(metadata) = rules.super_weapon_metadata(&source_sw.id) else {
        return false;
    };
    let launch = metadata.launch();
    let targeting = metadata.targeting();
    let chrono = launch.chronosphere().clone();
    if launch.radar_event() {
        for (rx, ry) in [source, target] {
            sim.sound_events.emit(SimSoundEvent::RadarEvent {
                radar: RadarEventRequest::new(RadarEventType::ImpactSilent, rx, ry),
            });
        }
    }
    Simulation::release_super_anim(&mut sim.substrate, &mut sim.super_weapons, owner, sw_type);
    let height = source_sw.presentation.height;
    let blasts = [
        (
            chrono.blast_src.clone(),
            &rules.general.chrono_blast_anim,
            source,
        ),
        (
            chrono.blast_dest.clone(),
            &rules.general.chrono_blast_dest_anim,
            target,
        ),
    ];
    for (custom, fallback, cell) in blasts {
        let Some(animation) = custom.unwrap_or_else(|| Some(fallback.clone())) else {
            continue;
        };
        let [x, y, z] = deck_coords(sim, cell);
        super::spawn_super_anim(sim, rules, &animation, [x, y, z.wrapping_add(height)]);
    }
    let range = match launch.range() {
        (width, height) if width < 0.0 && height < 0 => (3.0, 3),
        range => range,
    };
    let warp = AresChronoWarp {
        owner,
        source,
        target,
    };
    let mut buildings = Vec::new();
    for id in super::protect::area_technos(sim, range, source) {
        ares_chronoport(
            sim,
            rules,
            &warp,
            &chrono,
            targeting,
            id,
            overlay_registry,
            frame_effects,
            &mut buildings,
        );
    }
    // `newStateMachine(ChronoDelay + 1, Coords, pSource, ...)`: the machine
    // reads the source Super's owner and type.
    if !buildings.is_empty() {
        sim.ares_chrono_warps.machines.push(AresChronoWarpMachine {
            start_frame: sim.session.binary_frame(),
            duration: rules.general.chrono_delay.wrapping_add(1),
            owner,
            sw_type: source_type,
            buildings,
        });
    }
    true
}

/// What SW_ChronoWarp::Activate's lambda reads of its launch: the Super's
/// owner, the source's cell (`+0x62`) and the clicked target.
#[cfg(feature = "ares")]
struct AresChronoWarp {
    owner: InternedId,
    source: (u16, u16),
    target: (u16, u16),
}

/// SW_ChronoWarp::Activate's lambda (Ares 3.0 `1006F4D0`) for one Techno:
/// - skipped when the source's `SW.AffectsHouse`/`SW.AffectsTarget` exclude
///   it, or its TechnoTypeExt `Chronoshift.Allow` is off;
/// - a Building: a `BridgeRepairHut=` one is skipped, and so is one that
///   neither counts as a vehicle (`Chronoshift.IsVehicle` with
///   `Chronosphere.ReconsiderBuildings`, or an `UndeploysInto=` one with
///   `Chronosphere.AffectsUndeployable`) nor is allowed by
///   `Chronosphere.AffectsBuildings`, or a vehicle one without the Units bit;
/// - skipped while immobilized (the warp latch), in the air, warping out or
///   in, `Warpable=no` without `Chronosphere.AffectsUnwarpable`, curtained
///   without `Chronosphere.AffectsIronCurtain`, or a Unit in a war factory
///   that is not Naval (TechnoExt::GetWarFactoryOf 10045580, excludeNaval);
/// - an `Organic=` one with `Chronosphere.KillOrganic`, unless a
///   `Teleporter=` without `Chronosphere.KillTeleporters`, takes its Strength
///   as `C4Warhead=` damage, ignoring defenses, from the owner;
/// - otherwise a `Naval=` Foot's eater lets go (suppression 500, ExitUnit), a
///   Foot in a Tank Bunker leaves it (ClearBunker), `Chronosphere.KillCargo`
///   kills its passengers (TechnoExt::KillCargo 100455E0, no killer), and the
///   Foot is armed for the warp ([`ares_arm_chrono_warp`]);
/// - a Building instead empties its Tank Bunker (UnloadBunker `0x004593A0`,
///   EmptyBunker `0x004595C0`), sends OVER_OUT to each contact, has its
///   passengers killed under `KillCargo` and starts its chronoshift
///   ([`ares_begin_building_chronoshift`]), joining `buildings`.
#[cfg(feature = "ares")]
#[allow(clippy::too_many_arguments)]
fn ares_chronoport(
    sim: &mut Simulation,
    rules: &RuleSet,
    warp: &AresChronoWarp,
    chrono: &crate::rules::superweapon_type::AresChronosphereRules,
    targeting: crate::rules::superweapon_targeting::AresSuperWeaponTargetingState,
    id: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
    buildings: &mut Vec<AresChronoWarpBuilding>,
) {
    use crate::rules::superweapon_targeting::TargetMask;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let relation = sim.ares_house_relation(warp.owner, entity.owner());
    if !targeting.affects_house().intersects(relation)
        || !super::protect::techno_affected(sim, entity, targeting.affects_target())
    {
        return;
    }
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let chronoshift = object.ares_chronoshift();
    if !chronoshift.allow {
        return;
    }
    let category = entity.category();
    let mut is_vehicle = false;
    if category == EntityCategory::Structure {
        if object.bridge_repair_hut {
            return;
        }
        let undeployer = chrono.affect_undeployable && object.undeploys_into.is_some();
        is_vehicle = undeployer || (chrono.reconsider_buildings && chronoshift.is_vehicle);
        if !undeployer
            && (!is_vehicle && !targeting.chrono_affect_buildings()
                || is_vehicle && !targeting.affects_target().intersects(TargetMask::UNIT))
        {
            return;
        }
    }
    let (organic, teleporter, strength, warpable, naval) = (
        object.organic,
        object.teleporter,
        object.strength,
        object.warpable,
        object.naval,
    );
    let frame = sim.session.binary_frame();
    let in_air = crate::sim::movement::air_movement::is_high_flying(
        entity,
        sim.resolved_terrain.as_ref(),
        Some((rules, &sim.interner)),
    );
    if entity.chrono_warp_latch()
        || in_air
        || entity.is_warped_out()
        || entity.is_warping_in()
        || !warpable && !chrono.affect_unwarpable
        || crate::sim::superweapon::invulnerability::is_invulnerable(
            entity.invulnerability.as_ref(),
            frame,
        ) && !chrono.affect_iron_curtain
    {
        return;
    }
    let naval_factory = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(id)
            .and_then(|unit| unit.radio_contacts.slot(0))
            .and_then(|contact| sim.substrate.entities.get(contact))
            .and_then(|building| sim.object_type(building.type_ref(), rules))
            .is_some_and(|building| building.naval)
    };
    if category == EntityCategory::Unit
        && sim.unit_in_contact_war_factory(id, rules)
        && !naval_factory(sim)
    {
        return;
    }
    if organic && chrono.kill_organic && (!teleporter || chrono.kill_teleporters) {
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
        sim.commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects);
        return;
    }
    if naval
        && let Some(eater) = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.parasite.eater())
        && sim
            .substrate
            .entities
            .get(eater)
            .is_some_and(|eater| eater.parasite.class().is_some())
    {
        sim.parasite_force_release(
            eater,
            crate::sim::combat::parasite::CHRONO_WARP_SUPPRESSION_FRAMES,
            rules,
            frame_effects,
        );
    }
    if let Some(bunker) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.bunker.link().installed_in())
        && sim
            .substrate
            .entities
            .get(bunker)
            .is_some_and(|building| building.category() == EntityCategory::Structure)
    {
        crate::sim::docking::bunker_link::clear_bunker(sim, bunker, rules, frame_effects);
    } else if category == EntityCategory::Structure
        && sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|building| building.bunker.occupant().is_some())
    {
        // `pBunker->UnloadBunker(); pBunker->EmptyBunker();`
        crate::sim::docking::bunker_link::release_sell_destroy(sim, id, rules, frame_effects);
        crate::sim::docking::bunker_link::release_normal(sim, id, rules, frame_effects);
    }
    if category == EntityCategory::Structure {
        // SendToEachLink(RequestRedraw) only redraws; SendToEachLink
        // (NotifyUnlink) is the OVER_OUT to every contact.
        crate::sim::radio::broadcast_break(sim, id, rules, frame_effects);
    }
    if chrono.kill_cargo {
        let context = crate::sim::world::UninitContext::new(rules, overlay_registry)
            .with_effects(frame_effects);
        sim.ares_kill_cargo(id, context);
    }
    if category == EntityCategory::Structure {
        if let Some(building) = ares_begin_building_chronoshift(
            sim,
            rules,
            warp,
            id,
            is_vehicle,
            overlay_registry,
            frame_effects,
        ) {
            buildings.push(building);
        }
        return;
    }
    ares_arm_chrono_warp(sim, rules, warp, id, overlay_registry, frame_effects);
}

/// One building a ChronoWarpStateMachine moves (Ares
/// `ChronoWarpContainer`): the building, its target cell (its cell moved by
/// the target's offset from the source), its Location when the warp began
/// and whether it places like a vehicle.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresChronoWarpBuilding {
    building: u64,
    target: (i16, i16),
    origin: [i32; 3],
    is_vehicle: bool,
}

/// An Ares `ChronoWarpStateMachine`: its clock (SWStateMachine `Clock`,
/// started at the launch for `ChronoDelay + 1` frames), the source Super's
/// owner and type, and its buildings.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresChronoWarpMachine {
    start_frame: u32,
    duration: i32,
    owner: InternedId,
    sw_type: InternedId,
    buildings: Vec<AresChronoWarpBuilding>,
}

/// The running ChronoWarpStateMachines (Ares `SWStateMachine::Array`'s
/// ChronoWarp entries), in registration order. Saved and hashed.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresChronoWarps {
    machines: Vec<AresChronoWarpMachine>,
}

#[cfg(feature = "ares")]
impl AresChronoWarps {
    /// BuildingExt `AboutToChronoshift` (+0xD): set when the lambda
    /// registers a building (1006F4D0) and cleared when its machine brings it
    /// back (10070230), which also empties the machine's list.
    pub(crate) fn about_to_chronoshift(&self, building: u64) -> bool {
        self.machines.iter().any(|machine| {
            machine
                .buildings
                .iter()
                .any(|entry| entry.building == building)
        })
    }

    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if !self.machines.is_empty() {
            b"ares-chrono-warp-machines-v1".hash(hasher);
            self.machines.hash(hasher);
        }
    }
}

/// The lambda's Building arm (Ares 3.0 `1006F4D0`): the building becomes
/// untargetable (the detach sweep `0x0070D4A0`), every Bullet aimed at it
/// loses its target (`0x00468430`), it counts as warped out (`+0x270`, with
/// BuildingExt `AboutToChronoshift`), its owner rechecks tech tree and
/// power, and its temporal goes offline (`0x004521C0`: the online latch
/// and slot anims). The redraw (`UpdatePlacement(Redraw)`) is presentation.
/// Its target is its cell moved by the target's offset from the source.
#[cfg(feature = "ares")]
fn ares_begin_building_chronoshift(
    sim: &mut Simulation,
    rules: &RuleSet,
    warp: &AresChronoWarp,
    id: u64,
    is_vehicle: bool,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> Option<AresChronoWarpBuilding> {
    sim.stop_all_targeting_on_detach(id, rules, overlay_registry, frame_effects);
    sim.lose_projectile_targets(id, rules);
    let entity = sim.substrate.entities.get_mut(id)?;
    entity.begin_ares_chronoshift();
    let owner = entity.owner();
    let (rx, ry) = (entity.position.rx() as i16, entity.position.ry() as i16);
    sim.houses.request_house_tech_tree_recheck(owner);
    Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, owner, false);
    sim.substrate.set_building_online_for_temporal(id, false);
    let entity = sim.substrate.entities.get(id)?;
    let origin =
        crate::sim::movement::ground_pose::object_location(entity, sim.resolved_terrain.as_ref());
    let target = (
        rx.wrapping_sub(warp.source.0 as i16)
            .wrapping_add(warp.target.0 as i16),
        ry.wrapping_sub(warp.source.1 as i16)
            .wrapping_add(warp.target.1 as i16),
    );
    Some(AresChronoWarpBuilding {
        building: id,
        target,
        origin: [origin.x, origin.y, origin.z],
        is_vehicle,
    })
}

/// SWStateMachine::UpdateAll's ChronoWarp entries (inlined in the
/// LogicClass::Update hook 0x55AFB3, Ares 3.0 `1006E140`): each machine runs
/// [`ares_chrono_warp_update`], then leaves once its clock has run out.
#[cfg(feature = "ares")]
pub(super) fn ares_tick_chrono_warps(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    let machines = std::mem::take(&mut sim.ares_chrono_warps.machines);
    let mut kept = Vec::with_capacity(machines.len());
    for mut machine in machines {
        ares_chrono_warp_update(sim, rules, &mut machine, overlay_registry, frame_effects);
        let passed = sim.session.binary_frame().wrapping_sub(machine.start_frame) as i32;
        if passed < machine.duration {
            kept.push(machine);
        }
    }
    kept.append(&mut sim.ares_chrono_warps.machines);
    sim.ares_chrono_warps.machines = kept;
}

/// ChronoWarpStateMachine::Update (Ares 3.0 `10070230`), by the frames
/// since its launch:
/// - 1: every building redraws (presentation);
/// - its duration less one: every building leaves the map (Remove, and
///   `ActuallyPlacedOnMap` +0x6E4 drops), then each one not under a
///   Temporal erase comes back facing its current facing rounded to eight
///   bits: on the first cell of the CellSpread (radius 10 for a vehicle,
///   else 0) around its target where `CanPlaceAt` (`0x00464AC0`) admits it
///   and Put (Unlimbo) succeeds, at the cell's coordinates with the bridge
///   height; else, inside the `0xA8E7AC` bracket, back at its origin with
///   Place(false) (Grand_Opening). It is no longer warped out, its owner
///   rechecks power and tech tree and its temporal comes back online
///   (`0x00452210`). An unplaced vehicle, or any unplaced building under
///   the source's `Chronosphere.BlowUnplaceable`, takes its Strength as
///   `C4Warhead=` damage, ignoring defenses, credited to the Super's owner;
/// - its duration: the owner rechecks power, radar and tech tree.
#[cfg(feature = "ares")]
fn ares_chrono_warp_update(
    sim: &mut Simulation,
    rules: &RuleSet,
    machine: &mut AresChronoWarpMachine,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    let passed = sim.session.binary_frame().wrapping_sub(machine.start_frame) as i32;
    if passed == 1 {
        return;
    }
    if passed == machine.duration {
        sim.houses.request_house_tech_tree_recheck(machine.owner);
        Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, machine.owner, true);
        return;
    }
    if passed != machine.duration.wrapping_sub(1) {
        return;
    }
    // InvalidatePointer drops a building that leaves the game.
    let buildings: Vec<_> = std::mem::take(&mut machine.buildings)
        .into_iter()
        .filter(|item| {
            sim.substrate
                .entities
                .get(item.building)
                .is_some_and(|entity| entity.lifecycle.object_alive())
        })
        .collect();
    let context =
        crate::sim::world::UninitContext::new(rules, overlay_registry).with_effects(frame_effects);
    for item in &buildings {
        sim.techno_limbo_with_context(item.building, context);
        if let Some(entity) = sim.substrate.entities.get_mut(item.building) {
            entity.building_art.clear_actually_placed();
        }
    }
    let blow_unplaceable = rules
        .super_weapon(sim.interner.resolve(machine.sw_type))
        .and_then(|sw| rules.super_weapon_metadata(&sw.id))
        .is_some_and(|metadata| metadata.launch().chronosphere().blow_unplaceable);
    for item in &buildings {
        ares_chrono_warp_put_back(
            sim,
            rules,
            machine.owner,
            item,
            blow_unplaceable,
            overlay_registry,
            frame_effects,
        );
    }
}

/// One building's return in [`ares_chrono_warp_update`].
#[cfg(feature = "ares")]
fn ares_chrono_warp_put_back(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    item: &AresChronoWarpBuilding,
    blow_unplaceable: bool,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    use crate::sim::world::PlacementEvidence;
    let id = item.building;
    let frame = sim.session.binary_frame();
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    // `pBld->TemporalTargetingMe` (+0x278) skips the return.
    if entity.temporal.is_warped() {
        return;
    }
    let dir = crate::util::direction_tables::quantize::round_facing16_to_8(
        entity.body_facing_current(frame),
    );
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let mut success = false;
    let spread = if item.is_vehicle { 10 } else { 0 };
    for (dx, dy) in crate::sim::cell_rect::cell_spread_offsets(spread) {
        let cell = (
            item.target.0.wrapping_add(dx),
            item.target.1.wrapping_add(dy),
        );
        if cell.0 < 0
            || cell.1 < 0
            || !crate::sim::build_site::can_place_building_at(
                sim,
                rules,
                overlay_registry,
                object,
                cell,
                None,
                None,
                None,
            )
        {
            continue;
        }
        let [x, y, z] = deck_coords(sim, (cell.0 as u16, cell.1 as u16));
        if sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                DriveCoord { x, y, z },
                dir,
                PlacementEvidence::EvaluateMark,
                rules,
                overlay_registry,
                frame_effects,
            )
            .is_some()
        {
            success = true;
            break;
        }
    }
    if !success {
        let [x, y, z] = item.origin;
        sim.with_object_placement_scope(|sim| {
            let _ = sim.reveal_constructed_object_at_coord_with_overlay_context(
                id,
                DriveCoord { x, y, z },
                dir,
                PlacementEvidence::EvaluateMark,
                rules,
                overlay_registry,
                frame_effects,
            );
            let scenario_initialization = sim.object_placement_scope_active();
            sim.grand_opening(
                id,
                false,
                scenario_initialization,
                rules,
                overlay_registry,
                frame_effects,
            );
        });
    }
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    entity.end_ares_chronoshift();
    let building_owner = entity.owner();
    Simulation::invalidate_house_power(&sim.houses, &mut sim.power_states, building_owner, false);
    sim.houses.request_house_tech_tree_recheck(building_owner);
    sim.substrate.set_building_online_for_temporal(id, true);
    if !success && (item.is_vehicle || blow_unplaceable) {
        let Some(strength) = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| sim.object_type(entity.type_ref(), rules))
            .map(|object| object.strength)
        else {
            return;
        };
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            id,
            strength,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            Some(owner),
            sim.interner.intern(&rules.bridge_warheads.c4_name),
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: true,
            },
        );
        sim.commit_direct_damage_receiver(rules, overlay_registry, event, frame_effects);
    }
}

/// The lambda's Foot arm (Ares 3.0 `1006F4D0`): from the Foot's GetCoords
/// (vt+0x48) and its cell, the destination cell is that cell moved by the
/// target's offset from the source, the destination its coordinates moved
/// by that offset in leptons, raised by the bridge height where that cell
/// has a bridge (CellClass::FixHeight `10070010`). Every Foot's locomotor
/// releases its occupation (`+0x9C(0)`) and forces its track to the Foot's
/// own coordinates (`Force_Track(-1, ..)`), the Foot clears its raw occupation
/// there (vt+0xF4) and raises `+0x6B6`; a fresh Teleport piggybacks the
/// locomotor, then the latch, the destination (`+0x288`), the radio OVER_OUT
/// to every contact, the warping House (`+0x42C`) and the destination setter
/// at the destination cell (vt+0x480(cell, 1)).
///
/// Unlike case 4's, this Force_Track leaves a Drive or Ship heading for its
/// source: Force_Track (`0x004B0C40`) stores the coordinates as head and
/// destination, nothing in the Teleport states (`0x007192F0`), their
/// Update_Position, PostWarpValidation and Stop_Moving, Teleport's END
/// (`0x00719EE0`) or the Foot AI tail resets the stashed locomotor, and its
/// next Process (`0x004B0500`, Process_Movement `0x004B2630`) asks Find_Path
/// for the source cell: the Unit drives back unless it cannot reach it.
#[cfg(feature = "ares")]
fn ares_arm_chrono_warp(
    sim: &mut Simulation,
    rules: &RuleSet,
    warp: &AresChronoWarp,
    id: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.locomotor.is_none() {
        return;
    }
    let here =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    let (dx, dy) = (
        i32::from(warp.target.0 as i16) - i32::from(warp.source.0 as i16),
        i32::from(warp.target.1 as i16) - i32::from(warp.source.1 as i16),
    );
    let (cx, cy) = (
        (i32::from(entity.position.rx() as i16) + dx) as i16,
        (i32::from(entity.position.ry() as i16) + dy) as i16,
    );
    let mut destination = DriveCoord {
        x: here.x.wrapping_add(dx.wrapping_mul(256)),
        y: here.y.wrapping_add(dy.wrapping_mul(256)),
        z: here.z,
    };
    if super::cell_grid::cell_has_bridge_flag(sim, cx, cy) {
        destination.z = destination
            .z
            .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    sim.locomotor_mark_all_occupation_bits(id, false);
    let _ = sim.force_track(id, -1, here, rules, overlay_registry, frame_effects);
    Simulation::object_raw_receiver_at(
        &mut sim.substrate,
        &sim.path_grid,
        &sim.resolved_terrain,
        id,
        here,
        false,
    );
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.foot_occupation_enabled.resume();
    }
    let frame = sim.session.binary_frame();
    let Some(locomotor) = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.locomotor.as_mut())
    else {
        return;
    };
    locomotor.begin_piggyback(LocomotorKind::Teleport, frame);
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
        frame,
    ));
    crate::sim::radio::broadcast_break(sim, id, rules, frame_effects);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.drop_credit.credit_warp(warp.owner);
    }
    if let Err(error) = sim.assign_destination_represented(
        id,
        Some(NavTargetRef::cell(cx as u16, cy as u16)),
        rules,
        overlay_registry,
        frame_effects,
    ) {
        log::debug!("ares chrono warp {id} destination: {error:?}");
    }
}
