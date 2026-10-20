//! `FootClass::Mark` (vt+0x124 @ `0x004D3780`), the one Rust port for the
//! Infantry, Unit and Aircraft vtables (`0x007EB058`, `0x007F5C70`,
//! `0x007E22A4`). Buildings have their own Mark (`0x0043F180`).
//!
//! Mark(UP) and Mark(DOWN) run `TechnoClass::Mark` (`0x006F4A70`), whose
//! `ObjectClass::Mark` (`0x005F5850`) refuses a Limbo object (+0x81) and an
//! object already in the requested state, then writes Cell.Marked (+0x74)
//! before anything else. Only an object whose layer (vt+0x78, the locomotor's
//! `In_Which_Layer`) is Ground reaches `MapClass::Pick_Up` (`0x005687F0`) or
//! `Place_Down` (`0x005683C0`). For the Foot occupy list `{(0,0)}` they
//! address the Cell of the stored Location (vt+0x1B8 = `0x0041BEA0`) when it
//! is a real Cell, unlink it from or prepend it to the OnBridge list
//! (`CellClass::RemoveContent` `0x0047EA90` / `AddContent` `0x0047E8A0`), and
//! run `CellClass::Recalc(-1)` (`0x0047D2B0`) on that Cell. Remove/AddContent
//! skip Infantry's raw receiver; the others write theirs only while the Foot
//! occupation enable (+0x6B6, vt+0xC0 = `0x0041C070`) holds.
//!
//! The accepted Object Mark is followed by Techno's tethered first-contact
//! radio 0xD (`0x006F4A81..0x006F4A91`) before the Foot layer query. Its
//! Object receiver invokes Mark(2), which a Foot accepts immediately without
//! running Techno Mark or changing cell membership (`0x004D3789..0x004D3795`).
//!
//! RESIDUALS:
//! - Building Mark(2)'s clean presentation refresh is not represented. The
//!   executed human GAPILE output witness reaches a dirty building (+0x80=1),
//!   so Object Mark refuses it with no presence, occupation or RNG effects.
//!   The clean arm's palette, z-adjust and slot translucency refresh
//!   (`0x0043F9A6..0x0043FB0B`) remains a presentation residual; it must not
//!   be replaced by PUT/REMOVE or a second cell-list transaction.
//! - AddContent's shared discovery (`0x0047E953..0x0047E9DC`, `DiscoveredBy(Player)`
//!   `0x006F4960`) runs after linking and before the raw receiver. Its history,
//!   House power/radar and House1F4 effects are retained. Attached Tag event4
//!   remains with the larger live Tag/Trigger mechanism.
//! - AddContent skips the insert when the list's second object is already this
//!   one (`0x0047E903..0x0047E906`). Mark's +0x74 gate keeps an unmarked
//!   object out of its lists, and the one direct Place_Down follows its own
//!   Pick_Up, so VERA's lists never reach that state.
//! - Recalc runs only when the caller passes an overlay registry. Reveal and
//!   Limbo pass none, as before this owner existed.
//!
//! Mark's list choice is `occupancy::cell_list_layer_for_entity`. Readers of
//! list membership follow the lists themselves, as list walks
//! (`Simulation::cell_object_list_location`) do. The blocker plane
//! (`bump_crush::blocker_plane_source`) derives it from the object through the
//! same choice and `cell_marked`: for buildings, and for mobiles only in a
//! world without an overlay grid. With one, mobiles reach the plane through
//! the retained Foot+55C counters (`cell_neighbors`), which follow
//! Unlimbo/PerCell/Limbo rather than the lists.
//!
//! RESIDUAL: a Jumpjet's hold and cruise leave its lists alone
//! (`0x0054D0FF..0x0054D134`), so one that sinks below two levels there counts
//! as listed by that derivation before its next Mark lists it, and
//! `Simulation::debug_assert_lifecycle_consistent` reports it. Trigger: a
//! Jumpjet below two levels while it holds or cruises; at a cruise height of
//! 500 that needs ground three levels higher beneath it before it climbs.
//! Effect: in a world without an overlay grid, its cell counts toward the
//! blocker plane before native lists it. Frequency: unmeasured. Risk: a
//! different hierarchy-gate verdict there, no other state.

use super::{ground_pose, locomotor::MovementLayer};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::occupancy::{CellListInsertion, OBJECT_OCCUPATION_BIT};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::FrameEffects;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::world::LifecycleTestEvent;
use crate::sim::world::Simulation;

/// The spot an infantryman holds in its cell (0–4), which Mark puts into the
/// cell's lists and the occupancy index projects. gamemd keeps no such field:
/// `CellClass::PlaceInfantryInCell` (`0x00481180`) picks the spot as it
/// places the coordinate, and the coordinate's in-cell offset carries it from
/// then on. VERA caches the spot at each placement: Unlimbo, the walk
/// boundary, and the crew, passenger, paradrop, parasite, tube and unload
/// placements. `None` on every non-infantry object, and on an infantryman
/// built in limbo until its Unlimbo places it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct InfantrySpot(Option<u8>);

impl InfantrySpot {
    /// The spot an object starts with, before any placement.
    pub(crate) const fn new(spot: Option<u8>) -> Self {
        Self(spot)
    }

    /// The held spot.
    pub(crate) const fn get(self) -> Option<u8> {
        self.0
    }

    /// Hold `spot`, the one a placement chose: PlaceInfantryInCell's search or
    /// preference (`0x00481180`), or a map entry's authored spot.
    pub(crate) fn hold(&mut self, spot: u8) {
        self.0 = Some(spot);
    }

    /// Hold the spot of the in-cell offset `(sub_x, sub_y)` that an
    /// infantryman was put at without a search: the offset's quadrant, as
    /// PlaceInfantryInCell's priority placement returns it
    /// ([`crate::sim::cell_kernel::infantry_preferred_spot`]).
    pub(crate) fn hold_offset(
        &mut self,
        sub_x: crate::util::fixed_math::SimFixed,
        sub_y: crate::util::fixed_math::SimFixed,
    ) {
        self.0 = Some(crate::sim::cell_kernel::infantry_preferred_spot(
            crate::sim::cell_kernel::CellQueryPoint {
                x: sub_x.to_num::<i32>(),
                y: sub_y.to_num::<i32>(),
            },
        ));
    }

    /// Hold no spot: a non-infantry object, or an infantryman built in limbo
    /// whose Unlimbo has not placed it yet.
    pub(crate) fn release(&mut self) {
        self.0 = None;
    }
}

/// The Foot occupation enable (`Foot+0x6B6`, read through vt+0xC0
/// `0x0041C070`): while it holds, Mark's Remove/AddContent write the object's
/// raw occupation. The Foot constructor sets it (`0x004D344A`). A mover
/// suspends it while it carries the object off its cell and resumes it on
/// arrival; a Chrono warp, a superweapon's launch and a new locomotor resume
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct OccupationEnable(bool);

impl OccupationEnable {
    /// The Foot constructor's enable (`0x004D344A`).
    pub(crate) const fn constructed() -> Self {
        Self(true)
    }

    /// Whether Mark writes the object's raw occupation.
    pub(crate) const fn get(self) -> bool {
        self.0
    }

    /// Stop Mark writing the raw occupation while a mover carries the object
    /// off its cell: a Drive track's first paid point (`0x004B161A`, Ship
    /// `0x006A0CDA`) and Hover's first translating frame (`0x005147D5`).
    pub(crate) fn suspend(&mut self) {
        self.0 = false;
    }

    /// Let Mark write the raw occupation again: a Drive track's terminal point
    /// (`0x004B1FEF`, Ship `0x006A1632`), Hover's arrival (`0x0051451E`),
    /// `FootClass::ChronoWarpTo` (`0x004DF93F`) and Infantry's (`0x0052307C`),
    /// `SuperClass::Launch` (`0x006CCAB4`) and
    /// `TechnoClass::ImbueLocomotor` (`0x007101CF`).
    pub(crate) fn resume(&mut self) {
        self.0 = true;
    }
}

/// The raw occupation plane an object receiver writes at the stored
/// Location `coord` (Unit `0x007441B0`/`0x00744210`, Infantry
/// `0x005217C0`/`0x00521850`, Object `0x005F60A0`/`0x005F6120`): its Cell,
/// and whether the deck plane is selected. The deck needs Z at or above the
/// Cell's ground (`0x00578080`) plus the 416-lepton deck and, when
/// `needs_structural`, the Cell's structural bridge bit (`+0x140 & 0x100`).
/// Every PUT and the Object clear test that bit; the Unit and Infantry
/// clears test height alone.
pub(super) fn raw_occupation_plane(
    coord: DriveCoord,
    needs_structural: bool,
    terrain: Option<&ResolvedTerrainGrid>,
    path_grid: Option<&PathGrid>,
) -> ((u16, u16), MovementLayer) {
    let at = ((coord.x / 256) as u16, (coord.y / 256) as u16);
    let ground = ground_pose::ground_surface_z_at([coord.x, coord.y], false, terrain, path_grid)
        .unwrap_or(coord.z);
    let deck = coord.z >= ground.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS)
        && (!needs_structural
            || terrain
                .and_then(|terrain| terrain.cell(at.0, at.1))
                .is_some_and(|cell| cell.bridge_facts.has_structural_bridge()));
    (
        at,
        if deck {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        },
    )
}

impl Simulation {
    /// Save/clear/restore Object+74 without changing cell lists. Locomotors
    /// use this around setters (Jumpjet54D0FF/54D6B4 and54C189/54C1A3, and
    /// the Drive/Ship track's SetLocation and SetHeight(0) in
    /// `track_host`).
    /// This is distinct from Mark4D3780, whose pickup/place callbacks run
    /// only when the native caller actually invokes Mark.
    pub(crate) fn replace_foot_marked_byte(&mut self, id: u64, marked: bool) -> bool {
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("marked Foot exists");
        entity.lifecycle.replace_cell_marked(marked)
    }

    /// The object's Mark(2) virtual dispatch reached by Object radio 0xD
    /// (`0x005F5374`). Foot4D3789 returns true before any lifecycle gate or
    /// Techno Mark, so it cannot recursively transmit 0xD.
    ///
    /// Bounded Building coverage: the original human GAPILE output witness's
    /// twelve dirty Mark(2) calls refuse at Object5F586B..73 with no effects.
    /// Clean Building Mark(2)'s presentation tail is the residual above;
    /// VERA does not retain Object+0x80 or invent a dirty-byte predicate.
    /// Evidence: human-infantry-output-downstream-research/mark-radio.native.json,
    /// SHA256 270a1321e3a5509df0eb58500883737f69af2172726da216f7ca107962391127;
    /// pinned active-retail gamemd SHA256
    /// 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
    pub(crate) fn mark_entity_refresh(&mut self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() != EntityCategory::Structure)
    }

    /// Techno Mark6F4A81..97 after an accepted Object Mark: read the live
    /// tether and first contact, transmit literal13 synchronously, then return
    /// to the class-specific Mark tail. No entity borrow crosses the receiver.
    pub(crate) fn techno_mark_after_object(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        frame_effects: FrameEffects<'_>,
    ) {
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.tether.partner().is_some())
        {
            crate::sim::radio::transmit_to_contact(
                self,
                id,
                crate::sim::radio::RadioMessage::AnimStop,
                rules,
                frame_effects,
            );
        }
    }

    /// Mark(UP). +0x74 is cleared (`0x005F5913`) before the layer query, so
    /// a Jumpjet answers as an unmarked object. The list unlink and the raw
    /// clear use the Cell and OnBridge the object holds now.
    pub(crate) fn foot_mark_remove(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if !entity.lifecycle.admit_mark_up() {
            return false;
        }
        self.techno_mark_after_object(id, rules, frame_effects);
        self.foot_pick_up(id, rules, registry);
        true
    }

    /// `MapClass::Pick_Up` (`0x005687F0`) of a Foot: at the Cell and on the
    /// OnBridge list [`Self::foot_mark_cell`] picks, RemoveContent
    /// (`0x0047EA90`) unlinks it and runs its raw receiver tail, then Recalc.
    /// Mark(UP) runs it after clearing +0x74; the Aircraft Unload ejection
    /// calls it directly (`0x0041552E`).
    pub(crate) fn foot_pick_up(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some((cell, layer)) = self.foot_mark_cell(id) else {
            return;
        };
        self.substrate
            .occupancy
            .remove_on_layer(cell.0, cell.1, id, layer);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListUnlinked);
        if self.foot_mark_raw(id, false) {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationCleared);
        }
        self.recalculate_track_cell(cell, rules, registry);
    }

    pub(crate) fn foot_mark_put(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        self.foot_mark_put_observed(id, rules, registry, &mut |_, _| {}, frame_effects)
    }

    /// Mark(DOWN). `receive` runs after the list link, where AddContent's
    /// discovery sits. `ObjectClass::Mark` (`0x005F5850`) refuses a Limbo
    /// (`+0x81`) or marked (`+0x74`) object, then stores `+0x74` (`0x005F58F7`).
    pub(super) fn foot_mark_put_observed(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        receive: &mut impl FnMut(&mut Simulation, u64),
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if !entity.lifecycle.admit_mark_down() {
            return false;
        }
        self.techno_mark_after_object(id, rules, frame_effects);
        self.foot_place_down(id, rules, registry, receive);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::CellMarked);
        true
    }

    /// `MapClass::Place_Down` (`0x005683C0`) of a Foot: AddContent
    /// (`0x0047E8A0`) prepends it to the list [`Self::foot_mark_cell`] picks,
    /// `receive` runs where its discovery sits, then the raw receiver tail and
    /// Recalc. Both the Foot enable and the Location are read again after
    /// `receive`; Recalc still addresses the Cell it linked. Mark(DOWN) runs it
    /// after setting +0x74; the Aircraft Unload ejection calls it directly
    /// (`0x00415565`).
    pub(crate) fn foot_place_down(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        receive: &mut impl FnMut(&mut Simulation, u64),
    ) {
        let Some((cell, layer)) = self.foot_mark_cell(id) else {
            return;
        };
        let (sub_cell, insertion) = {
            let entity = self.substrate.entities.get(id).expect("placed Foot");
            (
                entity.sub_cell(),
                CellListInsertion::from_category(entity.category()),
            )
        };
        self.substrate
            .occupancy
            .add(cell.0, cell.1, id, layer, sub_cell, insertion);
        self.discover_cell_put_object(
            id,
            cell,
            crate::sim::world::UninitContext::new(rules, registry),
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListLinked);
        receive(self, id);
        if self.foot_mark_raw(id, true) {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
        }
        self.recalculate_track_cell(cell, rules, registry);
    }

    /// The Cell Pick_Up/Place_Down address and the OnBridge list, or `None`
    /// when the layer query (`0x004D37A6`) is not Ground or the Location's
    /// Cell is not a real one (`0x00568471..0x0056848E`).
    pub(crate) fn foot_mark_cell(&self, id: u64) -> Option<((u16, u16), MovementLayer)> {
        let entity = self.substrate.entities.get(id)?;
        let layer = crate::sim::occupancy::cell_list_layer_for_entity(
            entity,
            self.resolved_terrain.as_ref(),
        )?;
        let cell = (entity.position.rx(), entity.position.ry());
        if self.resolved_terrain.as_ref().is_some_and(|terrain| {
            terrain
                .native_fixed_cell_index(cell.0 as i16, cell.1 as i16)
                .is_none()
        }) {
            return None;
        }
        Some((cell, layer))
    }

    /// Remove/AddContent's receiver tail: Infantry returns first
    /// (`0x0047EAFE` / `0x0047E9EA`), then the Foot enable gates the object's
    /// vt+0xF4/+0xF0 on its stored Location. Whether a receiver ran.
    fn foot_mark_raw(&mut self, id: u64, put: bool) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if !entity.foot_occupation_enabled() || entity.category() == EntityCategory::Infantry {
            return false;
        }
        let coord = ground_pose::position_world_coord(&entity.position);
        self.object_raw_receiver_at(id, coord, put)
    }

    /// The object's own raw occupation receiver on `coord`, vt+0xF0 (PUT) or
    /// vt+0xF4 (REMOVE): Unit `0x007441B0`/`0x00744210` (0x20), Infantry
    /// `0x005217C0`/`0x00521850` (its sub-cell) and, for Aircraft, ObjectClass
    /// `0x005F60A0`/`0x005F6120` (0x40). Mark reaches it through
    /// [`Self::foot_mark_raw`]; a direct call (the Jumpjet grounded reset,
    /// `0x0054D42F`) has no Foot enable gate. Whether a receiver ran.
    pub(crate) fn object_raw_receiver_at(&mut self, id: u64, coord: DriveCoord, put: bool) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        match entity.category() {
            EntityCategory::Unit => {
                self.track_raw_mark_at(coord, put);
            }
            EntityCategory::Infantry => {
                let owner = entity.owner();
                super::walk_head::raw_at(
                    &mut self.substrate.raw_cell_occupation,
                    owner,
                    coord,
                    put,
                    self.resolved_terrain.as_ref(),
                    self.path_grid.as_deref(),
                );
            }
            EntityCategory::Aircraft => self.object_raw_mark_at(coord, put),
            _ => return false,
        }
        true
    }

    /// Aircraft vt+0xF0/+0xF4, the ObjectClass receivers `0x005F60A0` /
    /// `0x005F6120`: bit 0x40 on the plane [`raw_occupation_plane`] selects,
    /// testing the structural bridge bit on PUT and REMOVE alike.
    fn object_raw_mark_at(&mut self, coord: DriveCoord, put: bool) {
        let (at, layer) = raw_occupation_plane(
            coord,
            true,
            self.resolved_terrain.as_ref(),
            self.path_grid.as_deref(),
        );
        let raw = &mut self.substrate.raw_cell_occupation;
        match (put, layer) {
            (true, MovementLayer::Bridge) => raw.mark_deck(at.0, at.1, OBJECT_OCCUPATION_BIT),
            (true, _) => raw.mark_ground(at.0, at.1, OBJECT_OCCUPATION_BIT),
            (false, MovementLayer::Bridge) => raw.clear_deck(at.0, at.1, OBJECT_OCCUPATION_BIT),
            (false, _) => raw.clear_ground(at.0, at.1, OBJECT_OCCUPATION_BIT),
        }
    }
}

#[cfg(test)]
#[path = "foot_mark_tests.rs"]
mod tests;
