//! `OverlayClass::Mark @ 0x005FC570`: the one Rust port of an Overlay's
//! placement on its CellClass.
//!
//! Every Overlay that reveals reaches this body through `ObjectClass::Unlimbo`:
//! `OverlayClass::OverlayClass @ 0x005FC380` for the map reader
//! (`ReadMapOverlayPacks @ 0x005FD4D2`), crate validation (`0x004A198F`),
//! meteor ore (`AnimClass::AI @ 0x0042413B`), `CellClass::PlaceTiberium @
//! 0x0048728C` and the bridge span walkers (`0x005695D1`, `0x00569EF7`), and
//! `0x005FD270` for Building's wall arm. The caller owns its constructor, the
//! base `ObjectClass::Mark @ 0x005F5850` with its tactical dirty, and the
//! lifecycle exit the returned [`OverlayMarkOutcome`] selects. This module owns
//! the derived body: the slope gate at `0x005FC5CD`, the high-bridge setters,
//! the Railroad, wall and low-bridge arms, ordinary admission and writes, the
//! CellAnim branch, and the common Recalc at `0x005FD1FA`.
//!
//! [`OverlayMarkHost`] is the CellClass surface and the owners Mark calls. A
//! host differs only in where those owners live: the load-local cells of the
//! map reader, the live Simulation, the bridge publication, or the batched ore
//! placement. Every decision Mark makes is taken here.
//!
//! Dependency rules: part of map/; depends on rules/ and util/, never on sim/.

use crate::map::bridge_facts::{BridgeStampFamily, high_bridge_stamp_for_overlay};
use crate::map::cell_index::NativeCellIdentity;
use crate::rules::crate_rules::CrateRules;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::overlay_types::{OverlayTypeFlags, OverlayTypeRegistry};
use crate::rules::terrain_rules::LandType;
use crate::rules::tiberium_type::{TiberiumTypeId, TiberiumTypeRegistry};
use crate::util::direction::DIRECTION_DELTAS;

/// `CellClass+0x44` of a cell without an overlay.
pub(crate) const NO_OVERLAY: i32 = -1;

const MARK_MAX_SLOPE: u8 = 4;
/// `0x005FC5DD`: the one identity the slope gate admits on any slope
/// (retail `VEINHOLEDUMMY`).
const MARK_STEEP_SLOPE_EXCEPTION_ID: u8 = 0xB2;
/// `0x005FC65D`: the TS veinhole identity (retail `USELESS`).
const VEINHOLE_ID: u8 = 0xA7;
/// `0x005FC6C5`: the TS veins identity (retail `DUMMYOLD`).
const VEINS_ID: u8 = 0x7E;
/// `0x005FD044`: the identity whose ordinary admission rereads the slope
/// after `0x004834A0` (retail `TIB01`).
const SLOPE_RECHECK_ID: u8 = 0x66;
/// `0x005FD171..0x005FD177`: the CellAnim's offset from the Overlay's
/// Location on both X and Y.
const CELL_ANIM_OFFSET_LEPTONS: i32 = 0x180;

/// `OverlayClass::Mark @ 0x005FC5CD..0x005FC5E3`: the common unsigned slope
/// gate on `CellClass+0x11C`, before every derived arm. Literal `0xB2` is
/// exempt.
const fn overlay_mark_accepts_slope(overlay_id: u8, slope: u8) -> bool {
    slope <= MARK_MAX_SLOPE || overlay_id == MARK_STEEP_SLOPE_EXCEPTION_ID
}

/// The three `RulesClass` crate images ordinary admission compares the
/// Overlay's type with (`0x005FCFF3..0x005FD021`): `+0x100` WaterCrateImg,
/// `+0xFC` CrateImg and `+0xF8` WoodCrateImg, as overlay indices. A pointer
/// comparison of types is an index comparison here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MarkCrateImages {
    pub(crate) water: Option<u8>,
    pub(crate) crate_img: Option<u8>,
    pub(crate) wood: Option<u8>,
}

impl MarkCrateImages {
    pub(crate) fn resolve(rules: &CrateRules, registry: &OverlayTypeRegistry) -> Self {
        let index =
            |name: &Option<String>| name.as_deref().and_then(|name| registry.id_for_name(name));
        Self {
            water: index(&rules.water_crate_img),
            crate_img: index(&rules.crate_img),
            wood: index(&rules.wood_crate_img),
        }
    }
}

/// One Mark of an Overlay its caller constructed and revealed.
#[derive(Clone, Copy)]
pub(crate) struct OverlayMarkRequest<'a> {
    /// `OverlayTypeClass+0x294` (ArrayIndex) of `OverlayClass+0xAC`.
    pub(crate) overlay_id: u8,
    pub(crate) flags: &'a OverlayTypeFlags,
    /// The packed cell `this->vt+0x1B8` reports (`0x005FC5A6`).
    pub(crate) cell: (i16, i16),
    /// `ObjectClass+0x9C..0xA4` Location, set by Unlimbo from the
    /// constructor's center or Building's requested coordinate. Only the
    /// CellAnim branch reads it.
    pub(crate) location: [i32; 3],
    pub(crate) registry: &'a OverlayTypeRegistry,
    /// `None` only for focused crate fixtures that bind no RuleSet: the
    /// Land-5 germination and the CellAnim tint then find no class.
    pub(crate) tiberium_types: Option<&'a TiberiumTypeRegistry>,
    pub(crate) crate_images: MarkCrateImages,
}

/// How Mark returned; each caller maps it onto its Overlay's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayMarkOutcome {
    /// `0x005FC5DD..0x005FC5E3`: false before any write. The Overlay stays
    /// revealed and registered; Mark calls no UnInit.
    SlopeRefused,
    /// `0x005FC77C..0x005FC787`: `Is_Clear_To_Build` refused the wall. Mark
    /// calls `vt+0xF8` (UnInit) and returns false.
    WallRefused,
    /// `0x005FD1FA..0x005FD225`: the common tail ran Recalc, cleared
    /// `+0x74`, set `+0x81` and called `vt+0xF8`; Mark returns true.
    Marked,
}

/// The CellAnim `OverlayClass::Mark @ 0x005FD112..0x005FD1F4` constructs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OverlayCellAnim<'a> {
    /// `OverlayTypeClass+0x29C` CellAnim.
    pub(crate) anim: &'a str,
    /// The Overlay's Location; the host samples `MapClass @ 0x00578080`'s
    /// ground height there for the constructor's Z.
    pub(crate) location: [i32; 3],
    /// The constructor's X/Y: Location plus `0x180` (`0x005FD171..0x005FD177`).
    pub(crate) anim_xy: [i32; 2],
    /// The CellClass Mark resolved; the tint copies its `+0x10A`.
    pub(crate) cell: NativeCellIdentity,
    /// `CellClass::GetTiberiumType @ 0x00485010` on that cell after Mark's
    /// writes. `Some` enters the tint block at `0x005FD1C5..0x005FD1F4`.
    pub(crate) tiberium: Option<TiberiumTypeId>,
}

/// The CellClass surface `OverlayClass::Mark` reads and writes, and the
/// owners it calls. Real cells and the shared dummy keep their own identity,
/// data and slope; a write to the dummy is a write to the dummy.
pub(crate) trait OverlayMarkHost {
    type Error;

    /// `MapClass::Get_CellClass @ 0x005657A0` on a packed cell. A miss stamps
    /// the shared dummy's coordinate and returns the dummy.
    fn cell(&mut self, coord: (i16, i16)) -> NativeCellIdentity;
    /// `CellClass+0x24`, the packed coordinate `Adjacent_Cell @ 0x00481810`
    /// steps from; the dummy reports its latest stamp.
    fn coord(&self, cell: NativeCellIdentity) -> (i16, i16);
    /// `MapClass::In_Bounds @ 0x00568300`.
    fn in_bounds(&mut self, coord: (i16, i16)) -> Result<bool, Self::Error>;
    /// `CellClass+0x11C`.
    fn slope(&self, cell: NativeCellIdentity) -> u8;
    /// `CellClass+0x44`, [`NO_OVERLAY`] for none.
    fn identity(&self, cell: NativeCellIdentity) -> i32;
    /// `CellClass+0x11E`.
    fn data(&self, cell: NativeCellIdentity) -> u8;
    /// The literal `CellClass+0x44` store, without Recalc.
    fn write_identity(
        &mut self,
        cell: NativeCellIdentity,
        overlay_id: u8,
    ) -> Result<(), Self::Error>;
    /// The literal `CellClass+0x11E` store, with or without an identity.
    fn write_data(&mut self, cell: NativeCellIdentity, data: u8) -> Result<(), Self::Error>;
    /// `[0x00A8E7AC] != 0`: the ScenarioInit bracket map loading keeps raised.
    fn scenario_init(&self) -> bool;
    /// One raw draw of `ScenarioClass+0x218` (`0x0065C780`).
    fn next_scenario_raw(&mut self) -> u32;
    /// `CellClass::CheckCellPassability @ 0x004834A0` with Mark's fixed zone
    /// -1, MovementZone 0, level -1 and isBridge 1. `ignore_occupants` is the
    /// pair of ignore flags, which Mark always passes equal.
    fn passable(
        &mut self,
        cell: NativeCellIdentity,
        speed: SpeedType,
        ignore_occupants: bool,
    ) -> Result<bool, Self::Error>;
    /// `CellClass::SetBridgeDirection_NESW @ 0x0047E040` or `_NWSE @
    /// 0x0047E470` with `(direction, 1)` on the anchor, including its data
    /// writes on the anchor, both forward cells and the opposite cell.
    fn set_bridge_direction(
        &mut self,
        anchor: NativeCellIdentity,
        family: BridgeStampFamily,
        direction: u8,
    ) -> Result<(), Self::Error>;
    /// `CellClass::Is_Clear_To_Build @ 0x0047C620` with `(Track, NULL,
    /// NULL)`; it admits while the ScenarioInit bracket is raised.
    fn wall_clear_to_build(&mut self, anchor: NativeCellIdentity) -> Result<bool, Self::Error>;
    /// `CellClass::PostDestructionWallCleanup @ 0x00480630` with argument 1.
    fn wall_cleanup(&mut self, anchor: NativeCellIdentity) -> Result<(), Self::Error>;
    /// The anchor's zone merges `0x0056D5A0` then `0x00584550`.
    fn merge_wall_zones(&mut self, anchor: NativeCellIdentity) -> Result<(), Self::Error>;
    /// One wrapping `INC byte [CellClass+0x122]` (`0x005FC762..0x005FC773`).
    fn increment_wall_neighbor_count(
        &mut self,
        cell: NativeCellIdentity,
    ) -> Result<(), Self::Error>;
    /// `AnimClass::AnimClass @ 0x00421EA0` with `(CellAnim, coord, 0, 1,
    /// 0x600, 0, 0)`, then the tint block when the cell resolved a class.
    fn spawn_cell_anim(&mut self, request: OverlayCellAnim<'_>) -> Result<(), Self::Error>;
    /// `CellClass::RecalcAttributes @ 0x0047D2B0` with -1.
    fn recalc(&mut self, cell: NativeCellIdentity) -> Result<(), Self::Error>;
}

/// Run `OverlayClass::Mark @ 0x005FC570` after the base Mark and the
/// `MARK_DOWN` gate (`0x005FC57D..0x005FC597`).
pub(crate) fn mark_overlay<H: OverlayMarkHost>(
    host: &mut H,
    request: &OverlayMarkRequest<'_>,
) -> Result<OverlayMarkOutcome, H::Error> {
    let id = request.overlay_id;
    let flags = request.flags;
    // 0x005FC5BC: the Overlay's own CellClass, retained through the tail.
    let anchor = host.cell(request.cell);
    if !overlay_mark_accepts_slope(id, host.slope(anchor)) {
        return Ok(OverlayMarkOutcome::SlopeRefused);
    }

    let high = high_bridge_stamp_for_overlay(id);
    if let Some((family, direction)) = high {
        // 0x005FC5EE..0x005FC62C: the four high-bridge identities stamp
        // their span first, then continue at the Railroad test.
        host.set_bridge_direction(anchor, family, direction)?;
    } else if id == VEINHOLE_ID || (id == VEINS_ID && !host.scenario_init()) {
        // 0x005FC65D..0x005FC6E1: the TS veinhole and GrowVeins
        // (`0x00485AF0`) arms, not ported (GSI-18.01): Mark goes straight to
        // its common tail. On a cell `0x0074D670` admits (any cell during
        // ScenarioInit) native writes 0xA7 on the anchor and 0xB2 on its eight
        // neighbours and constructs a VeinholeMonster. Dormant in retail: no
        // runtime constructor places 0xA7, and the map reader
        // (`ReadMapOverlayPacks @ 0x005FD2E0`) constructs only identities with
        // an SHP or a CellAnim, which `USELESS` has in no theater (no rules or
        // art section, and neither `USELESS.SHP` nor `UGELESS.SHP` is in a
        // retail archive). No retail map places 0xA7 either.
        return finish(host, anchor);
    }

    // 0x005FC631..0x005FC658: Land=Railroad stores data before identity.
    if flags.land == LandType::Railroad {
        host.write_data(anchor, 0)?;
        host.write_identity(anchor, id)?;
        return finish(host, anchor);
    }
    if flags.wall {
        return mark_wall(host, anchor, id);
    }
    if let Some((tables, trigger)) = LowBridgeTables::for_trigger(id) {
        mark_low_bridge(host, request.cell, tables, trigger)?;
        return finish(host, anchor);
    }

    // 0x005FCFE7..0x005FD077: ScenarioInit admits without asking the cell.
    let scenario_init = host.scenario_init();
    let passable = if scenario_init {
        true
    } else if Some(id) == request.crate_images.water {
        host.passable(anchor, SpeedType::Float, false)?
    } else if Some(id) == request.crate_images.crate_img || Some(id) == request.crate_images.wood {
        host.passable(anchor, SpeedType::Track, false)?
    } else if high.is_some() {
        true
    } else {
        host.passable(anchor, SpeedType::Track, true)?
            && !(id == SLOPE_RECHECK_ID && host.slope(anchor) > MARK_MAX_SLOPE)
    };
    // 0x005FD079..0x005FD099: an existing Overrides=yes overlay keeps the
    // cell, outside the bracket only.
    let overridden = !scenario_init
        && u8::try_from(host.identity(anchor))
            .ok()
            .and_then(|existing| request.registry.flags(existing))
            .is_some_and(|existing| existing.overrides);
    if passable && !overridden {
        // 0x005FD09F..0x005FD0CC: the high bridges keep their setter's data.
        host.write_identity(anchor, id)?;
        if high.is_none() {
            host.write_data(anchor, 0)?;
        }
        // 0x005FD0D9: OverlayTypeClass+0x298 (Land) == 5, Tiberium.
        if flags.land == LandType::Tiberium {
            host.write_data(anchor, 1)?;
            germinate(host, request, anchor)?;
        }
        // 0x005FD0FB..0x005FD105
        if flags.crate_type {
            host.write_data(anchor, u8::MAX)?;
        }
    }

    // 0x005FD112..0x005FD1F4: reached written or not.
    if let Some(anim) = flags.cell_anim.as_deref() {
        let tiberium = request.tiberium_types.and_then(|tiberium_types| {
            u8::try_from(host.identity(anchor))
                .ok()
                .and_then(|identity| {
                    request
                        .registry
                        .tiberium_type_for_overlay(tiberium_types, identity)
                })
        });
        host.spawn_cell_anim(OverlayCellAnim {
            anim,
            location: request.location,
            anim_xy: [
                request.location[0].wrapping_add(CELL_ANIM_OFFSET_LEPTONS),
                request.location[1].wrapping_add(CELL_ANIM_OFFSET_LEPTONS),
            ],
            cell: anchor,
            tiberium,
        })?;
    }
    finish(host, anchor)
}

/// `0x005FD1FA`: the common Recalc of the retained CellClass. The lifecycle
/// half of the tail belongs to the caller.
fn finish<H: OverlayMarkHost>(
    host: &mut H,
    anchor: NativeCellIdentity,
) -> Result<OverlayMarkOutcome, H::Error> {
    host.recalc(anchor)?;
    Ok(OverlayMarkOutcome::Marked)
}

/// `CellClass::SpreadCellGerminate @ 0x004818E0` with argument 0, called at
/// `0x005FD0EC`: the shared kernel counts the receiver's same-class
/// neighbours through Get_CellClass and Mark stores the density.
fn germinate<H: OverlayMarkHost>(
    host: &mut H,
    request: &OverlayMarkRequest<'_>,
    anchor: NativeCellIdentity,
) -> Result<(), H::Error> {
    let Some(tiberium_types) = request.tiberium_types else {
        return Ok(());
    };
    let receiver = u8::try_from(host.identity(anchor)).ok();
    let origin = host.coord(anchor);
    let germinated = crate::map::tiberium_cell::spread_cell_germinate_without_randomization(
        tiberium_types,
        request.registry,
        receiver,
        origin,
        |coord| {
            let neighbor = host.cell(coord);
            (
                u8::try_from(host.identity(neighbor)).ok(),
                host.data(neighbor),
            )
        },
    );
    match germinated {
        Some(germinated) => host.write_data(anchor, germinated.density),
        None => Ok(()),
    }
}

/// `0x005FC6E6..0x005FC777`: the `Wall=yes` arm.
fn mark_wall<H: OverlayMarkHost>(
    host: &mut H,
    anchor: NativeCellIdentity,
    id: u8,
) -> Result<OverlayMarkOutcome, H::Error> {
    if !host.wall_clear_to_build(anchor)? {
        return Ok(OverlayMarkOutcome::WallRefused);
    }
    // 0x005FC711..0x005FC71E: data before identity.
    host.write_data(anchor, 0)?;
    host.write_identity(anchor, id)?;
    host.wall_cleanup(anchor)?;
    if !host.scenario_init() {
        host.merge_wall_zones(anchor)?;
    }
    // 0x005FC747..0x005FC755 copies a house into CellClass+0x50 when the
    // constructor's third argument (0x008333BC) is not -1. Every ported
    // constructor passes -1 (0x004A1983, 0x005FD4C3, 0x00487275, 0x0042411F,
    // 0x005695C8, 0x00569EEB, 0x005FE584); only the unported Building Mark
    // (0x0043F62E) passes a house.
    // 0x005FC758..0x005FC775: Adjacent_Cell rereads the anchor's coordinate
    // for each of the eight directions.
    for (dx, dy) in DIRECTION_DELTAS {
        let base = host.coord(anchor);
        let neighbor = host.cell((
            base.0.wrapping_add(dx as i16),
            base.1.wrapping_add(dy as i16),
        ));
        host.increment_wall_neighbor_count(neighbor)?;
    }
    finish(host, anchor)
}

/// The literal tables of one low-bridge family, indexed by trigger
/// (`id - first`) or by cross row.
struct LowBridgeTables {
    first: u8,
    /// `0x008333C0` / `0x00833408`: the fixed row's direction.
    row_direction: [u8; 4],
    /// `0x008333D0` / `0x00833418`: the fixed row's identity.
    fixed: [u8; 4],
    /// `0x008333E0` / `0x00833428`: the direction the search walks.
    join_direction: [u8; 4],
    /// `0x008333F0` / `0x00833438`: the body base per cross row.
    body_base: [u8; 2],
    /// `0x008333F8` / `0x00833440`: the opposite end, by join direction / 2.
    opposite: [u8; 4],
}

/// Wood bridges, `0x005FC790..0x005FCBB4`.
const WOOD_LOW_BRIDGE: LowBridgeTables = LowBridgeTables {
    first: 0x7A,
    row_direction: [4, 4, 2, 2],
    fixed: [0x5C, 0x5E, 0x60, 0x62],
    join_direction: [6, 2, 4, 0],
    body_base: [0x53, 0x4A],
    opposite: [0x60, 0x5C, 0x62, 0x5E],
};

/// Concrete bridges, `0x005FCBB9..0x005FCFE2`.
const CONCRETE_LOW_BRIDGE: LowBridgeTables = LowBridgeTables {
    first: 0xE9,
    row_direction: [4, 4, 2, 2],
    fixed: [0xDF, 0xE1, 0xE3, 0xE5],
    join_direction: [6, 2, 4, 0],
    body_base: [0xD6, 0xCD],
    opposite: [0xE3, 0xDF, 0xE5, 0xE1],
};

/// The function-static start offsets both families initialize once
/// (`0x00AC154C`, `0x00AC155C`), by trigger.
const LOW_BRIDGE_START_OFFSETS: [(i16, i16); 4] = [(0, -1), (0, -1), (-1, 0), (-1, 0)];

/// The function-static cross rows (`0x00AC1588`, `0x00AC15A0`), indexed by
/// `(reverse % 4) / 2`.
const LOW_BRIDGE_CROSS_ROWS: [[(i16, i16); 3]; 2] =
    [[(-1, 0), (0, 0), (1, 0)], [(0, -1), (0, 0), (0, 1)]];

impl LowBridgeTables {
    fn for_trigger(id: u8) -> Option<(&'static Self, usize)> {
        [&WOOD_LOW_BRIDGE, &CONCRETE_LOW_BRIDGE]
            .into_iter()
            .find(|tables| (tables.first..=tables.first + 3).contains(&id))
            .map(|tables| (tables, usize::from(id - tables.first)))
    }
}

fn step(coord: (i16, i16), direction: u8) -> (i16, i16) {
    let (dx, dy) = DIRECTION_DELTAS[usize::from(direction & 7)];
    (
        coord.0.wrapping_add(dx as i16),
        coord.1.wrapping_add(dy as i16),
    )
}

fn offset(coord: (i16, i16), delta: (i16, i16)) -> (i16, i16) {
    (coord.0.wrapping_add(delta.0), coord.1.wrapping_add(delta.1))
}

/// The low-bridge arms, `0x005FC790..0x005FCFE2`: three fixed probes, the
/// fixed row, the search for the opposite end, and the body rows with one
/// raw Scenario draw per cell. Every exit reaches the common tail.
fn mark_low_bridge<H: OverlayMarkHost>(
    host: &mut H,
    origin: (i16, i16),
    tables: &LowBridgeTables,
    trigger: usize,
) -> Result<(), H::Error> {
    let start = offset(origin, LOW_BRIDGE_START_OFFSETS[trigger]);
    let row_direction = tables.row_direction[trigger];
    // 0x005FC8DA..0x005FC956: all three probes are visited.
    let mut probe = start;
    let mut clear = true;
    for _ in 0..3 {
        let cell = host.cell(probe);
        if host.identity(cell) != NO_OVERLAY {
            clear = false;
        }
        probe = step(probe, row_direction);
    }
    if !clear {
        return Ok(());
    }
    // 0x005FC95C..0x005FC9AD: identity, then data, then Recalc per cell.
    let mut target = start;
    for data in 0..3u8 {
        let cell = host.cell(target);
        host.write_identity(cell, tables.fixed[trigger])?;
        host.write_data(cell, data)?;
        host.recalc(cell)?;
        target = step(target, row_direction);
    }

    // 0x005FC9AF..0x005FCA7C
    let join = tables.join_direction[trigger];
    let opposite = tables.opposite[usize::from(join / 2)];
    let mut search = step(origin, join);
    if !host.in_bounds(search)? {
        return Ok(());
    }
    let found = loop {
        let cell = host.cell(search);
        if host.identity(cell) == i32::from(opposite) && host.data(cell) == 1 {
            break search;
        }
        search = step(search, join);
        if !host.in_bounds(search)? {
            return Ok(());
        }
    };

    // 0x005FCA82..0x005FCBAE: the body runs back from the found end to the
    // fixed row; the signed word distance is the row count.
    let reverse = join.wrapping_sub(4) & 7;
    let mut work = step(found, reverse);
    let rows = (i32::from(work.1) - i32::from(start.1))
        .abs()
        .max((i32::from(work.0) - i32::from(start.0)).abs());
    let cross = usize::from((reverse % 4) / 2);
    for _ in 0..rows {
        for (data, delta) in LOW_BRIDGE_CROSS_ROWS[cross].into_iter().enumerate() {
            let cell = host.cell(offset(work, delta));
            let variant = (host.next_scenario_raw() & 3) as u8;
            host.write_identity(cell, tables.body_base[cross].wrapping_add(variant))?;
            host.write_data(cell, data as u8)?;
            host.recalc(cell)?;
        }
        work = step(work, reverse);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Call {
        Identity((i16, i16), u8),
        Data((i16, i16), u8),
        Draw,
        Passable(SpeedType, bool),
        Bridge(BridgeStampFamily, u8),
        ClearToBuild,
        Cleanup,
        MergeZones,
        Increment((i16, i16)),
        Anim([i32; 2], Option<TiberiumTypeId>),
        Recalc((i16, i16)),
    }

    /// A `width` x `height` CellClass array; every other coordinate resolves
    /// to one shared dummy. Each callback is logged in call order.
    struct MockHost {
        width: i16,
        height: i16,
        identity: Vec<i32>,
        data: Vec<u8>,
        slope: Vec<u8>,
        dummy: (i32, u8),
        dummy_coord: (i16, i16),
        scenario_init: bool,
        passable: bool,
        clear_to_build: bool,
        fail_recalc_at: Option<usize>,
        recalcs: usize,
        calls: Vec<Call>,
    }

    impl MockHost {
        fn new(width: i16, height: i16) -> Self {
            let cells = usize::try_from(width * height).unwrap();
            Self {
                width,
                height,
                identity: vec![NO_OVERLAY; cells],
                data: vec![0; cells],
                slope: vec![0; cells],
                dummy: (NO_OVERLAY, 0),
                dummy_coord: (0, 0),
                scenario_init: false,
                passable: true,
                clear_to_build: true,
                fail_recalc_at: None,
                recalcs: 0,
                calls: Vec::new(),
            }
        }

        fn index(&self, (x, y): (i16, i16)) -> Option<usize> {
            ((0..self.width).contains(&x) && (0..self.height).contains(&y))
                .then(|| usize::try_from(y * self.width + x).unwrap())
        }
    }

    impl OverlayMarkHost for MockHost {
        type Error = &'static str;

        fn cell(&mut self, coord: (i16, i16)) -> NativeCellIdentity {
            match self.index(coord) {
                Some(index) => NativeCellIdentity::Real(index),
                None => {
                    self.dummy_coord = coord;
                    NativeCellIdentity::Dummy
                }
            }
        }

        fn coord(&self, cell: NativeCellIdentity) -> (i16, i16) {
            match cell {
                NativeCellIdentity::Real(index) => {
                    let width = usize::try_from(self.width).unwrap();
                    ((index % width) as i16, (index / width) as i16)
                }
                NativeCellIdentity::Dummy => self.dummy_coord,
            }
        }

        fn in_bounds(&mut self, coord: (i16, i16)) -> Result<bool, Self::Error> {
            Ok(self.index(coord).is_some())
        }

        fn slope(&self, cell: NativeCellIdentity) -> u8 {
            match cell {
                NativeCellIdentity::Real(index) => self.slope[index],
                NativeCellIdentity::Dummy => 0,
            }
        }

        fn identity(&self, cell: NativeCellIdentity) -> i32 {
            match cell {
                NativeCellIdentity::Real(index) => self.identity[index],
                NativeCellIdentity::Dummy => self.dummy.0,
            }
        }

        fn data(&self, cell: NativeCellIdentity) -> u8 {
            match cell {
                NativeCellIdentity::Real(index) => self.data[index],
                NativeCellIdentity::Dummy => self.dummy.1,
            }
        }

        fn write_identity(
            &mut self,
            cell: NativeCellIdentity,
            overlay_id: u8,
        ) -> Result<(), Self::Error> {
            self.calls
                .push(Call::Identity(self.coord(cell), overlay_id));
            match cell {
                NativeCellIdentity::Real(index) => self.identity[index] = i32::from(overlay_id),
                NativeCellIdentity::Dummy => self.dummy.0 = i32::from(overlay_id),
            }
            Ok(())
        }

        fn write_data(&mut self, cell: NativeCellIdentity, data: u8) -> Result<(), Self::Error> {
            self.calls.push(Call::Data(self.coord(cell), data));
            match cell {
                NativeCellIdentity::Real(index) => self.data[index] = data,
                NativeCellIdentity::Dummy => self.dummy.1 = data,
            }
            Ok(())
        }

        fn scenario_init(&self) -> bool {
            self.scenario_init
        }

        fn next_scenario_raw(&mut self) -> u32 {
            self.calls.push(Call::Draw);
            0
        }

        fn passable(
            &mut self,
            _cell: NativeCellIdentity,
            speed: SpeedType,
            ignore_occupants: bool,
        ) -> Result<bool, Self::Error> {
            self.calls.push(Call::Passable(speed, ignore_occupants));
            Ok(self.passable)
        }

        fn set_bridge_direction(
            &mut self,
            _anchor: NativeCellIdentity,
            family: BridgeStampFamily,
            direction: u8,
        ) -> Result<(), Self::Error> {
            self.calls.push(Call::Bridge(family, direction));
            Ok(())
        }

        fn wall_clear_to_build(
            &mut self,
            _anchor: NativeCellIdentity,
        ) -> Result<bool, Self::Error> {
            self.calls.push(Call::ClearToBuild);
            Ok(self.clear_to_build)
        }

        fn wall_cleanup(&mut self, _anchor: NativeCellIdentity) -> Result<(), Self::Error> {
            self.calls.push(Call::Cleanup);
            Ok(())
        }

        fn merge_wall_zones(&mut self, _anchor: NativeCellIdentity) -> Result<(), Self::Error> {
            self.calls.push(Call::MergeZones);
            Ok(())
        }

        fn increment_wall_neighbor_count(
            &mut self,
            cell: NativeCellIdentity,
        ) -> Result<(), Self::Error> {
            self.calls.push(Call::Increment(self.coord(cell)));
            Ok(())
        }

        fn spawn_cell_anim(&mut self, request: OverlayCellAnim<'_>) -> Result<(), Self::Error> {
            self.calls
                .push(Call::Anim(request.anim_xy, request.tiberium));
            Ok(())
        }

        fn recalc(&mut self, cell: NativeCellIdentity) -> Result<(), Self::Error> {
            self.recalcs += 1;
            if Some(self.recalcs) == self.fail_recalc_at {
                return Err("injected Recalc failure");
            }
            self.calls.push(Call::Recalc(self.coord(cell)));
            Ok(())
        }
    }

    /// `count` types `OV0..`, with `sections` appended to the rules text.
    fn dense(count: usize, sections: &str) -> (OverlayTypeRegistry, TiberiumTypeRegistry) {
        let mut text = String::from("[Animations]\n0=SPARK\n[OverlayTypes]\n");
        for index in 0..count {
            text.push_str(&format!("{index}=OV{index}\n"));
        }
        text.push_str(sections);
        let ini = IniFile::from_str(&text);
        (
            OverlayTypeRegistry::from_ini(&ini, None),
            TiberiumTypeRegistry::from_ini(&ini),
        )
    }

    fn mark(
        host: &mut MockHost,
        (registry, tiberium_types): &(OverlayTypeRegistry, TiberiumTypeRegistry),
        crate_images: MarkCrateImages,
        cell: (i16, i16),
        overlay_id: u8,
    ) -> Result<OverlayMarkOutcome, &'static str> {
        let request = OverlayMarkRequest {
            overlay_id,
            flags: registry.flags(overlay_id).expect("registered type"),
            cell,
            location: [
                i32::from(cell.0) * 256 + 128,
                i32::from(cell.1) * 256 + 128,
                0,
            ],
            registry,
            tiberium_types: Some(tiberium_types),
            crate_images,
        };
        mark_overlay(host, &request)
    }

    #[test]
    fn slope_gate_refuses_before_any_callback_except_b2() {
        let types = dense(0xB3, "");
        let mut host = MockHost::new(4, 4);
        host.slope[5] = 5;
        assert_eq!(
            mark(&mut host, &types, MarkCrateImages::default(), (1, 1), 0),
            Ok(OverlayMarkOutcome::SlopeRefused)
        );
        assert!(host.calls.is_empty());
        assert_eq!(
            mark(&mut host, &types, MarkCrateImages::default(), (1, 1), 0xB2),
            Ok(OverlayMarkOutcome::Marked)
        );
        assert_eq!(host.identity[5], 0xB2);
    }

    #[test]
    fn ordinary_admission_passes_native_passability_arguments() {
        // 0x005FCFE7..0x005FD077: the crate images ask without the ignores,
        // Float for the water image; any other type asks Track with both
        // ignores; the high bridges ask nothing.
        let types = dense(0x19, "");
        let images = MarkCrateImages {
            water: Some(1),
            crate_img: Some(2),
            wood: Some(3),
        };
        for (id, expected) in [
            (1, Call::Passable(SpeedType::Float, false)),
            (2, Call::Passable(SpeedType::Track, false)),
            (3, Call::Passable(SpeedType::Track, false)),
            (0, Call::Passable(SpeedType::Track, true)),
            (0x18, Call::Bridge(BridgeStampFamily::Nesw, 0)),
        ] {
            let mut host = MockHost::new(4, 4);
            mark(&mut host, &types, images, (1, 1), id).unwrap();
            let asked: Vec<_> = host
                .calls
                .iter()
                .copied()
                .filter(|call| matches!(call, Call::Passable(..) | Call::Bridge(..)))
                .collect();
            assert_eq!(asked, vec![expected], "overlay {id:#04X}");
        }

        // 0x005FD079..0x005FD099: an existing Overrides=yes overlay keeps the
        // cell outside ScenarioInit; the raised bracket asks nothing.
        let types = dense(2, "[OV1]\nOverrides=yes\n");
        let mut live = MockHost::new(4, 4);
        live.identity[5] = 1;
        assert_eq!(
            mark(&mut live, &types, MarkCrateImages::default(), (1, 1), 0),
            Ok(OverlayMarkOutcome::Marked)
        );
        assert_eq!(
            live.calls,
            vec![Call::Passable(SpeedType::Track, true), Call::Recalc((1, 1))]
        );
        let mut init = MockHost::new(4, 4);
        init.identity[5] = 1;
        init.scenario_init = true;
        mark(&mut init, &types, MarkCrateImages::default(), (1, 1), 0).unwrap();
        assert_eq!(
            init.calls,
            vec![
                Call::Identity((1, 1), 0),
                Call::Data((1, 1), 0),
                Call::Recalc((1, 1)),
            ]
        );
    }

    #[test]
    fn cell_anim_follows_a_written_or_refused_mark_and_reads_the_final_cell() {
        // 0x005FD112..0x005FD1F4: reached whether or not the identity was
        // written, at Location + 0x180, tinted by the cell's TiberiumClass
        // after the writes; then the common Recalc and a true return.
        let types = dense(
            1,
            "[OV0]\nTiberium=yes\nLand=Tiberium\nCellAnim=SPARK\n\
             [Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\n",
        );
        let anim_xy = [256 + 128 + 0x180, 256 + 128 + 0x180];
        let mut refused = MockHost::new(4, 4);
        refused.passable = false;
        assert_eq!(
            mark(&mut refused, &types, MarkCrateImages::default(), (1, 1), 0),
            Ok(OverlayMarkOutcome::Marked)
        );
        assert_eq!(
            refused.calls,
            vec![
                Call::Passable(SpeedType::Track, true),
                Call::Anim(anim_xy, None),
                Call::Recalc((1, 1)),
            ]
        );

        let mut written = MockHost::new(4, 4);
        mark(&mut written, &types, MarkCrateImages::default(), (1, 1), 0).unwrap();
        assert_eq!(
            &written.calls[written.calls.len() - 2..],
            &[
                Call::Anim(anim_xy, Some(TiberiumTypeId(0))),
                Call::Recalc((1, 1)),
            ]
        );
    }

    #[test]
    fn wall_arm_runs_in_native_order_and_refusal_writes_nothing() {
        // 0x005FC6E6..0x005FC777, then the common Recalc.
        let types = dense(1, "[OV0]\nWall=yes\n");
        let mut host = MockHost::new(4, 4);
        assert_eq!(
            mark(&mut host, &types, MarkCrateImages::default(), (1, 1), 0),
            Ok(OverlayMarkOutcome::Marked)
        );
        assert_eq!(
            host.calls,
            vec![
                Call::ClearToBuild,
                Call::Data((1, 1), 0),
                Call::Identity((1, 1), 0),
                Call::Cleanup,
                Call::MergeZones,
                Call::Increment((1, 0)),
                Call::Increment((2, 0)),
                Call::Increment((2, 1)),
                Call::Increment((2, 2)),
                Call::Increment((1, 2)),
                Call::Increment((0, 2)),
                Call::Increment((0, 1)),
                Call::Increment((0, 0)),
                Call::Recalc((1, 1)),
            ]
        );

        let mut init = MockHost::new(4, 4);
        init.scenario_init = true;
        mark(&mut init, &types, MarkCrateImages::default(), (1, 1), 0).unwrap();
        assert!(!init.calls.contains(&Call::MergeZones));

        let mut refused = MockHost::new(4, 4);
        refused.clear_to_build = false;
        assert_eq!(
            mark(&mut refused, &types, MarkCrateImages::default(), (1, 1), 0),
            Ok(OverlayMarkOutcome::WallRefused)
        );
        assert_eq!(refused.calls, vec![Call::ClearToBuild]);
    }

    #[test]
    fn railroad_stores_data_first_and_skips_the_cell_anim() {
        // 0x005FC631..0x005FC658 jumps to the common tail.
        let types = dense(1, "[OV0]\nLand=Railroad\nCellAnim=SPARK\n");
        let mut host = MockHost::new(4, 4);
        mark(&mut host, &types, MarkCrateImages::default(), (1, 1), 0).unwrap();
        assert_eq!(
            host.calls,
            vec![
                Call::Data((1, 1), 0),
                Call::Identity((1, 1), 0),
                Call::Recalc((1, 1)),
            ]
        );
    }

    #[test]
    fn low_bridge_host_error_stops_before_later_writes_or_draws() {
        let types = dense(0x7C, "");
        let mut host = MockHost::new(20, 20);
        host.fail_recalc_at = Some(2);
        assert_eq!(
            mark(&mut host, &types, MarkCrateImages::default(), (8, 8), 0x7B),
            Err("injected Recalc failure")
        );
        assert_eq!(
            host.calls,
            vec![
                Call::Identity((8, 7), 0x5E),
                Call::Data((8, 7), 0),
                Call::Recalc((8, 7)),
                Call::Identity((8, 8), 0x5E),
                Call::Data((8, 8), 1),
            ]
        );
    }
}
