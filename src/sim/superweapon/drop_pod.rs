//! Ares 3.0 `Type=DropPod` (`src/Misc/SWTypes/DropPod.cpp`) and the gamemd
//! DropPod locomotor that lands its units.
//!
//! - SW_DropPod::Activate (`0x10070E40`, [`launch`]) draws how many units to
//!   drop and, per try, which type, and lands each through
//!   TechnoExt::CreateWithDroppod (`0x10044030`, [`create_with_drop_pod`]).
//! - gamemd's DropPodLocomotionClass has no stock installer (its CLSID's one
//!   installer `0x004DB8A0` has no references), so only this launch reaches
//!   it. Its Move_To (`0x004B6040`, [`Simulation::drop_pod_move_to`]) puts the
//!   Foot in the air; its Process (`0x004B5B70`,
//!   [`Simulation::process_drop_pod_locomotor`]) flies it down and lands it,
//!   with the Ares hooks 0x4B5EB0 (trailer, `1005C8B0`), 0x4B5F9E (report,
//!   `1005C890`) and 0x4B619F (AtmosphereEntry, `1005C870`) installed.
//!
//! The pod piggybacks the Foot's own locomotor (ChangeLocomotorTo's
//! Begin_Piggyback, End_Piggyback when it lands). VERA leaves the Foot's
//! `LocomotorState` alone and keeps the pod as the Foot's [`DropPodFlight`];
//! while it is set, FootClass::AI's Process is the pod's
//! (`world::object_turn`).
//!
//! Ledger: Activate draws Scenario `RandomRanged(min, max)` once, then per
//! try `RandomRanged(0, n - 1)` and, unless the try finished the drop,
//! `RandomRanged(0, 7)`; each CreateObject draws what its constructor draws.
//! Every third frame a flying pod with a `DropPodWeapon=` over a cell whose
//! nearest object is not allied draws one raw Scenario direction
//! (`0x0049F420`) and one Main `Next` (`0x004B5F91`). No timers; the landing
//! Limbo makes its own detach calls.
//!
//! RESIDUALS (all rare or dormant in retail data, which sets no DropPod
//! superweapon):
//! - A negative `DropPod.Minimum`/`.Maximum` count makes native loop about
//!   2^32 tries; VERA makes none.
//! - CreateWithDroppod's SetDestination runs the class setter's prologue
//!   (Unit `0x00741970`, Infantry, Aircraft) before the Foot tail; VERA
//!   publishes NavCom alone (the tail's write) and calls the pod's Move_To
//!   the tail would reach. A `Teleporter=` Unit's setter takes its own arm
//!   natively. Its See(0, 0) repeats the Unlimbo's sight reveal and is not
//!   called again.
//! - While the pod flies, the other ILocomotion answers (Is_Moving, the
//!   layer, Move_To from a mission) are the Foot's own locomotor's in VERA;
//!   natively the pod answers and ignores a later Move_To. A Foot whose
//!   Guard finds a target mid-air can thus keep a Drive destination VERA
//!   would not have. Its draw layer is the presentation's.
//! - A landing that misses `DropPod=` item 0 (or item 1 for an OutOfMap pod)
//!   reads past the list natively; VERA shows no pod.

use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::find_nearby_cell::{
    NearbyArgs, NearbyFootprint, NearbySearchOptions, PassabilityArgs,
};
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{
    FrameEffects, PlacementEvidence, SimSoundEvent, Simulation, UninitContext,
};
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as X, X87Value};

/// DropPodLocomotionClass's own fields while it carries a Foot: Destination
/// (object `+0x20`, the empty coordinate until Move_To) and OutOfMap
/// (`+0x1C`), set when the entry point lies outside the playfield.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct DropPodFlight {
    destination: Option<[i32; 3]>,
    out_of_map: bool,
}

impl DropPodFlight {
    /// The landing coordinate Move_To stored, its Z the ground's.
    pub(crate) fn destination(&self) -> Option<DriveCoord> {
        self.destination.map(|[x, y, z]| DriveCoord { x, y, z })
    }
}

/// SW_DropPod::Activate (`0x10070E40`) for `owner`'s `sw` at `cell`.
///
/// The types are `DropPod.Types` unless empty, else `[General]
/// DropPodTypes=`; the counts `DropPod.Minimum`/`.Maximum` unless unset,
/// else `DropPodMinimum=`/`DropPodMaximum=`; the veterancy the lower of
/// `DropPod.Veterancy` and `VeteranCap=`. Without types it refuses. It draws
/// the count, then makes three tries per unit: each draws a type (a
/// BuildingType ends the drop), creates it for the owner, raises its
/// veterancy to the value when lower, takes the nearby Wheel cell of the
/// current cell (`MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` with
/// Normal, 1x1, no zone, gate or target) and lands it there
/// ([`create_with_drop_pod`]). Unless that finished the drop, a draw picks
/// the first of the eight neighbours of that cell, walked from it, the
/// object may enter (`IsCellOccupied`, vt+0x1AC, `(-1, -1, NULL, 1)`), which
/// becomes the current cell; an object still in limbo is deleted.
#[allow(clippy::too_many_arguments)]
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(config) = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch().drop_pod().clone())
    else {
        return false;
    };
    let globals = rules.ares_drop_pod();
    let types = if config.types.is_empty() {
        globals.types.clone()
    } else {
        config.types
    };
    let minimum = config.minimum.unwrap_or(globals.minimum);
    let maximum = config.maximum.unwrap_or(globals.maximum);
    // 10070EB8..10070ED4: the Rules cap unless the type's value is not
    // above it.
    let wanted = f64::from_bits(config.veterancy.bits());
    let capped = if wanted <= rules.general.veteran_cap {
        wanted
    } else {
        rules.general.veteran_cap
    };
    let veterancy = capped as f32;
    if types.is_empty() {
        log::warn!(
            "DropPod super weapon \"{}\" could not be launched: no types to drop",
            sw.id
        );
        return false;
    }
    let owner_name = sim.interner.resolve(owner).to_owned();
    let last = i32::try_from(types.len()).map_or(i32::MAX, |count| count - 1);
    let mut target = (cell.0 as i16, cell.1 as i16);
    let mut count = sim.scenario_rng.next_range_i32_inclusive(minimum, maximum);
    for _ in 0..count.wrapping_mul(3).max(0) {
        let index = sim.scenario_rng.next_range_i32_inclusive(0, last);
        let Some(ty) = usize::try_from(index)
            .ok()
            .and_then(|index| types.get(index))
        else {
            break;
        };
        if ty.category == ObjectCategory::Building {
            log::warn!(
                "DropPod super weapon \"{}\" contains building types; aborting",
                sw.id
            );
            break;
        }
        // CreateObject (TechnoType vt+0x8C) for the owner; the reference is
        // a Find of a type the rules hold.
        let Some(object) = rules.object_in_category(ty.category, &ty.id) else {
            continue;
        };
        let Some(id) = sim.construct_resolved_object_limbo_at_height(
            object,
            &owner_name,
            0,
            0,
            0,
            0,
            rules,
            effects,
        ) else {
            continue;
        };
        // 10070F1C..10070F2E: a float compare and store.
        if let Some(entity) = sim.substrate.entities.get_mut(id)
            && f32::from_bits(entity.veterancy.raw().bits()) < veterancy
        {
            entity
                .veterancy
                .inherit_raw(NativeF32Bits::from_bits(veterancy.to_bits()));
        }
        let found = sim
            .nearby_passable_cell(
                (i32::from(target.0), i32::from(target.1)),
                NearbyArgs {
                    passability: PassabilityArgs {
                        speed_type: SpeedType::Wheel,
                        required_zone_id: None,
                        movement_zone: MovementZone::Normal,
                        bridge_aware_zone: false,
                    },
                    footprint: NearbyFootprint::SINGLE,
                    allow_bridge_cells: false,
                    check_height: false,
                    check_occupancy: false,
                    target_cell: None,
                    options: NearbySearchOptions {
                        reject_any_overlay: false,
                    },
                },
            )
            // An empty search answers the empty cell (`0x0056E79A`).
            .map_or((0, 0), |(x, y)| (x as i16, y as i16));
        let landing = DriveCoord {
            x: i32::from(found.0) * 256 + 128,
            y: i32::from(found.1) * 256 + 128,
            z: 0,
        };
        if create_with_drop_pod(sim, rules, id, landing, registry, effects) {
            count -= 1;
            if count == 0 {
                return true;
            }
        }
        let rnd = sim.scenario_rng.next_range_i32_inclusive(0, 7);
        for step in 0..8 {
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[((step + rnd) & 7) as usize];
            let neighbour = (
                found.0.wrapping_add(dx as i16),
                found.1.wrapping_add(dy as i16),
            );
            if can_enter(
                sim,
                rules,
                id,
                neighbour,
                EntryQueryMode::CheckLocomotor,
                registry,
            ) {
                target = neighbour;
                break;
            }
        }
        // 10071098: a Foot still in limbo failed to land (UnInit, vt+0xF8).
        if sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.lifecycle.in_limbo())
        {
            sim.discard_constructed_limbo(id, rules, effects);
        }
    }
    true
}

/// The object's IsCellOccupied (vt+0x1AC) at `cell` with `(-1, -1, NULL)`
/// answers 0 (Move::OK).
fn can_enter(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    cell: (i16, i16),
    mode: EntryQueryMode,
    registry: Option<&OverlayTypeRegistry>,
) -> bool {
    sim.mover_can_enter(id, cell, InfantryEntryArgs::REPAIR, mode, rules, registry)
        .is_ok_and(|code| code == 0)
}

/// TechnoExt::CreateWithDroppod (`0x10044030`): the Foot lands at `coord`
/// from a drop pod. Its cell must be open to it (IsCellOccupied with
/// `(-1, -1, NULL, 0)`). Then the pod piggybacks its locomotor, the Foot
/// moves to the coordinate at Z 0, takes the cell as its destination (the
/// Foot tail publishes NavCom and calls the pod's Move_To, which puts the
/// Foot in the air), the pod is sent there again (ignored once it has a
/// destination) and the Foot faces north. Out of limbo it queues Guard and
/// commences it, and the landing counts.
pub(crate) fn create_with_drop_pod(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    coord: DriveCoord,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    // GetCellAt(CoordStruct), toward zero.
    let cell = ((coord.x / 256) as i16, (coord.y / 256) as i16);
    if !can_enter(
        sim,
        rules,
        id,
        cell,
        EntryQueryMode::SkipLocomotor,
        registry,
    ) {
        return false;
    }
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    entity.begin_ares_drop_pod();
    sim.foot_set_location_marked(
        id,
        DriveCoord {
            x: coord.x,
            y: coord.y,
            z: 0,
        },
        rules,
        registry,
        effects,
    );
    // SetDestination(cell, true) (vt+0x480), through the Foot tail
    // (`0x004D94B0`) to the active locomotor's Move_To: the pod's.
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        crate::sim::movement::navcom::publish_nav_com(
            entity,
            NavTargetRef::cell(cell.0 as u16, cell.1 as u16),
        );
    }
    sim.drop_pod_move_to(id, coord, rules, registry, effects);
    // Locomotor->Move_To(XYZ) (`0x100440C9`).
    sim.drop_pod_move_to(id, coord, rules, registry, effects);
    // Facing.set(DirStruct()) (`0x100440D9..0x100440F3`).
    let frame = sim.session.binary_frame();
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    entity.body_facing.snap(0, frame);
    if entity.lifecycle.in_limbo() {
        return false;
    }
    crate::sim::world::queue_and_commence(sim, id, MissionType::Guard, rules);
    true
}

/// `DropPodHeight / tan(DropPodAngle)` (`0x004B60CD..0x004B60E4`): FILD,
/// `Math::TanFromTable4096 @ 0x004CAD50`, FDIVR. `None` for an angle the x87
/// cannot load or a zero tangent.
fn entry_run(height: i32, angle: crate::util::native_x87::NativeF64Bits) -> Option<X87Value> {
    let tangent = crate::util::native_trig::native_tan_from_table(X::load_f64(angle).ok()?);
    X::div(X::load_i32(height), tangent).ok()
}

/// AnimTypeClass::Find's answer for `name` (case-insensitive) is a type.
fn anim_type_exists(rules: &RuleSet, name: &str) -> bool {
    (0..)
        .map_while(|index| rules.anim_type_name_by_index(index))
        .any(|anim| anim.eq_ignore_ascii_case(name))
}

impl Simulation {
    /// DropPodLocomotionClass::Move_To (`0x004B6040`) for the pod carrying
    /// `id`. Only the first call acts: the destination is `to` on its ground
    /// (`0x00578080`), and the Foot enters `DropPodHeight=` above it, run
    /// back along `DropPodAngle=` toward +X, or toward -X with OutOfMap when
    /// that point is outside the playfield (`0x005785F0`). The Foot is put
    /// there and Unlimbo'd (vt+0xD8, facing 0x80); a refused Unlimbo is
    /// tried once more and does nothing else. A placed Foot is put there
    /// again, snaps its body to 0x8000 (`0x004C9300`) and shows
    /// `AtmosphereEntry=`, which Ares 1005C870 (hook 0x4B619F) skips when it
    /// is unset.
    pub(crate) fn drop_pod_move_to(
        &mut self,
        id: u64,
        to: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(pod) = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.ares_drop_pod())
        else {
            return;
        };
        if pod.destination.is_some() {
            return;
        }
        let locomotor = &rules.ares_drop_pod().locomotor;
        let ground = self.resolved_terrain.as_ref().map_or(0, |terrain| {
            crate::sim::movement::ground_pose::query_ground_height(
                &NativeCellQuery::canonical(terrain),
                to,
            )
            .unwrap_or(0)
        });
        let destination = DriveCoord { z: ground, ..to };
        let Some(run) = entry_run(locomotor.height, locomotor.angle) else {
            return;
        };
        let mut start = DriveCoord {
            x: X::ftol_i32_low_masked(X::add(run, X::load_i32(destination.x))),
            y: destination.y,
            z: destination.z.wrapping_add(locomotor.height),
        };
        let out_of_map = !crate::sim::cell_rect::cell_is_in_playfield_leptons(
            (start.x, start.y, start.z),
            self.playfield_bounds(),
            self.resolved_terrain.as_ref(),
        );
        if out_of_map {
            start.x = X::ftol_i32_low_masked(X::sub(X::load_i32(destination.x), run));
        }
        if let Some(pod) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.ares_drop_pod_mut())
        {
            pod.destination = Some([destination.x, destination.y, destination.z]);
            pod.out_of_map = out_of_map;
        }
        self.foot_set_location_marked(id, start, rules, registry, effects);
        let placed = self
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                start,
                0x80,
                PlacementEvidence::EvaluateMark,
                rules,
                registry,
                effects,
            )
            .is_some();
        if !placed {
            // 0x004B61DE: the second Unlimbo's answer is not read.
            let _ = self.reveal_constructed_object_at_coord_with_overlay_context(
                id,
                start,
                0x80,
                PlacementEvidence::EvaluateMark,
                rules,
                registry,
                effects,
            );
            return;
        }
        self.foot_set_location_marked(id, start, rules, registry, effects);
        let frame = self.session.binary_frame();
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.body_facing.snap(0x8000, frame);
        }
        if let Some(anim) = locomotor.atmosphere_entry.as_deref() {
            super::spawn_super_anim(self, rules, anim, [start.x, start.y, start.z]);
        }
    }

    /// DropPodLocomotionClass::Process (`0x004B5B70`) for the pod carrying
    /// `id`, from FootClass::AI.
    ///
    /// In the air (GetHeight, vt+0x1C8, above 0) the Foot moves
    /// `max(height / 10 + 2, DropPodSpeed=)` along `DropPodAngle=`
    /// (`Math::CosFromTable`/`SinFromTable`, ftol): down, and toward -X, or
    /// +X for an OutOfMap pod. Ares 1005C8B0 (hook 0x4B5EB0) then leaves
    /// `[General] DropPodTrailer=` (unset: `SMOKEY`) at the old Location
    /// every sixth frame, with or without a weapon. With a `DropPodWeapon=`,
    /// every third frame, unless the nearest object of the destination's
    /// cell (`0x0047C3D0`) is an ally's (`0x004F9A90`, none is not), it
    /// strafes a random point 0x55 leptons from the destination
    /// ([`Self::drop_pod_strafe`]).
    ///
    /// On the ground it lands (`0x004B5C8D..0x004B5E92`): SetHeight(0),
    /// Limbo, End_Piggyback, Unlimbo at the Location facing 0. A refused
    /// Unlimbo leaves the Foot in limbo and blows 100 `C4Warhead=` there;
    /// otherwise `DropPodPuff=` and `DropPod=` item 0 (item 1 when OutOfMap)
    /// play at the Location, and the Foot runs Mark(DOWN), SetHeight(0),
    /// Enter_Idle_Mode(0, 1), Commence and Scatter(NULL, 0, 0).
    pub(crate) fn process_drop_pod_locomotor(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(pod) = entity.ares_drop_pod() else {
            return;
        };
        let terrain = self.resolved_terrain.as_ref();
        let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
        let height = crate::sim::movement::air_movement::current_fly_height(entity, terrain);
        let locomotor = &rules.ares_drop_pod().locomotor;
        if height <= 0 {
            self.land_drop_pod(id, pod, rules, registry, effects);
            return;
        }
        let Ok(angle) = X::load_f64(locomotor.angle) else {
            return;
        };
        let speed = X::load_i32((height / 10 + 2).max(locomotor.speed));
        let trig = crate::util::retail_trig::TrigTable::embedded();
        let run = X::mul(trig.cos_from_table(angle), speed);
        let x = if pod.out_of_map {
            X::add(run, X::load_i32(location.x))
        } else {
            X::sub(X::load_i32(location.x), run)
        };
        let drop = X::mul(trig.sin_from_table(angle), speed);
        let moved = DriveCoord {
            x: X::ftol_i32_low_masked(x),
            y: location.y,
            z: X::ftol_i32_low_masked(X::sub(X::load_i32(location.z), drop)),
        };
        self.foot_set_location_marked(id, moved, rules, registry, effects);
        let frame = self.session.binary_frame();
        if frame % 6 == 0 {
            // An unset trailer takes AnimTypeClass::Find("SMOKEY"), null
            // when no such type exists.
            let trailer = match &rules.ares_drop_pod().trailer {
                Some(trailer) => trailer.as_deref(),
                None => anim_type_exists(rules, "SMOKEY").then_some("SMOKEY"),
            };
            if let Some(trailer) = trailer {
                super::spawn_super_anim(self, rules, trailer, [location.x, location.y, location.z]);
            }
        }
        let (Some(weapon), Some(destination)) = (locomotor.weapon.as_deref(), pod.destination())
        else {
            return;
        };
        if frame % 3 == 0 {
            self.drop_pod_strafe(id, weapon, destination, moved, rules, registry, effects);
        }
    }

    /// `0x004B5F29..0x004B602D`: the pod's `DropPodWeapon=` strafe. A
    /// nearest object of the destination's ground list that is an ally's
    /// spares the frame. Otherwise the hit is `0x0049F420(destination, 0x55,
    /// 0)`; a Main `Next` picks `Report=` (Ares 1005C890, hook 0x4B5F9E:
    /// none without sounds) played at the pod's new Location;
    /// `Apply_area_damage @ 0x00489280` deals twice the weapon's Damage with
    /// its Warhead from the Foot, and the Warhead's explosion for that damage
    /// on Clear plays there (`0x0048A4F0`, `0x2600`).
    #[allow(clippy::too_many_arguments)]
    fn drop_pod_strafe(
        &mut self,
        id: u64,
        weapon_name: &str,
        destination: DriveCoord,
        location: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(owner) = self.substrate.entities.get(id).map(|entity| entity.owner()) else {
            return;
        };
        let nearest_owner =
            Self::map_coord_cell(&self.resolved_terrain, destination.x, destination.y)
                .and_then(|cell| {
                    self.nearest_cell_object(
                        cell,
                        crate::sim::movement::locomotor::MovementLayer::Ground,
                        None,
                    )
                })
                .and_then(|object| self.substrate.entities.get(object))
                .map(|object| object.owner());
        if nearest_owner.is_some_and(|other| {
            crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                owner,
                other,
            )
        }) {
            return;
        }
        let (x, y) = crate::sim::combat::inviso_scatter::random_direction_coord(
            self.cell_grid(),
            &mut self.scenario_rng,
            destination.x,
            destination.y,
            0x55,
            crate::sim::combat::inviso_scatter::RandomDirectionSnap::Preserve,
        );
        let hit = ProjectileCoord::new(x, y, destination.z);
        let weapon = rules.weapon(weapon_name);
        let draw = self.main_rng.next_u32();
        if let Some(sound) = weapon.and_then(|weapon| {
            let count = weapon.report.len();
            (count != 0).then(|| weapon.report[draw as usize % count].clone())
        }) {
            self.sound_events.emit(SimSoundEvent::voc_at_world_coord(
                sound,
                crate::sim::anim_class::AnimWorldCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
            ));
        }
        let damage = weapon.map_or(0, |weapon| weapon.damage) << 1;
        // A weapon without a Warhead leaves Apply_area_damage (`0x004892D7`)
        // and the selection (`0x0048A4FF`) inert.
        let Some(warhead) = weapon
            .and_then(|weapon| weapon.warhead.as_deref())
            .and_then(|name| rules.warhead(name))
        else {
            return;
        };
        self.drop_pod_blast(id, hit, damage, warhead, rules, registry, effects);
    }

    /// `Apply_area_damage(coord, damage, Foot, warhead, 1, NULL)` and the
    /// warhead's explosion for that damage on Clear at the coordinate
    /// (`0x0048A4F0`, flags `0x2600`, the `0x0048ACE0` Z adjust): the strafe
    /// and a refused landing (100 `C4Warhead=`).
    #[allow(clippy::too_many_arguments)]
    fn drop_pod_blast(
        &mut self,
        id: u64,
        coord: ProjectileCoord,
        damage: i32,
        warhead: &crate::rules::warhead_type::WarheadType,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let warhead_ref = self.interner.intern(&warhead.id);
        let receipt = crate::sim::combat::world_receiver::apply_area_damage(
            self,
            rules,
            registry,
            coord,
            damage,
            warhead,
            true,
            (id, None, warhead_ref),
            effects,
        );
        if let Some(effect) = crate::sim::combat::detonation_anim::effect(
            self,
            rules,
            warhead,
            damage,
            0,
            coord,
            coord,
            receipt.crit_active,
        ) {
            crate::sim::world::damage_consequences::admit_explosion_effect(self, rules, effect);
        }
    }

    /// The landing arm of [`Self::process_drop_pod_locomotor`].
    fn land_drop_pod(
        &mut self,
        id: u64,
        pod: DropPodFlight,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.set_object_height(id, 0, rules, registry, effects);
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let _ = self.techno_limbo_with_context(
            id,
            UninitContext::new(rules, registry).with_effects(effects),
        );
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.end_ares_drop_pod();
        }
        let placed = self
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                location,
                0,
                PlacementEvidence::EvaluateMark,
                rules,
                registry,
                effects,
            )
            .is_some();
        let at = [location.x, location.y, location.z];
        if !placed {
            if let Some(c4) = rules.warhead(&rules.bridge_warheads.c4_name) {
                let coord = ProjectileCoord::new(at[0], at[1], at[2]);
                self.drop_pod_blast(id, coord, 100, c4, rules, registry, effects);
            }
            return;
        }
        let locomotor = &rules.ares_drop_pod().locomotor;
        if let Some(puff) = locomotor.puff.as_deref() {
            super::spawn_super_anim(self, rules, puff, at);
        }
        if let Some(anim) = locomotor.anims.get(usize::from(pod.out_of_map)) {
            super::spawn_super_anim(self, rules, anim, at);
        }
        // Mark(DOWN) refuses the Foot the Unlimbo marked.
        let _ = self.foot_mark_put(id, rules, registry, effects);
        self.set_object_height(id, 0, rules, registry, effects);
        crate::sim::world::enter_idle_mode(self, id, rules, registry, effects);
        let now = self.session.binary_frame();
        let _ = self.substrate.mission_commence_exact(id, now);
        if let Err(error) = self.scatter(
            id,
            None,
            crate::sim::movement::ScatterFlags::new(false, false),
            rules,
            registry,
            effects,
        ) {
            log::debug!("landed drop pod {id} Scatter failed: {error}");
        }
    }
}
