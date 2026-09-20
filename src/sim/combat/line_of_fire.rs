//! The line-of-fire walk — the last gate in `TechnoClass::InRange`.
//!
//! `InRange` 0x006F7220 ends both of its arms in the same call: the arcing arm
//! at 0x006F7519 pushes its two arguments and jumps to 0x006F763C, the ground
//! arm falls into 0x006F763B, and both land on `CALL 0x004CC310` at
//! **0x006F7642**, whose result is inverted into the return
//! (`TEST EAX,EAX; SETZ AL` at 0x006F7647). A non-null answer is a blocking
//! cell, and the shot is refused — the attacker reports "not in range" rather
//! than firing into a wall or a cliff face.
//!
//! Three native bodies make up the mechanism:
//!
//! | Address | Role |
//! |---|---|
//! | 0x004CC310 | wraps the walk and re-admits a breakable wall |
//! | 0x004CC100 | steps the line source→target one cell at a time |
//! | 0x004CC360 | decides whether one sampled cell blocks |
//!
//! Only projectiles that set `SubjectToCliffs` (`BulletType+0x296`, key string
//! 0x0081B118, read at 0x0046BFF8) or `SubjectToWalls` (`+0x298`, key string
//! 0x0081B0F4, read at 0x0046C02C) enter the walk at all — 0x004CC10B tests
//! both and returns immediately when neither is set.
//!
//! Depends on: rules (WeaponType, ProjectileType, WarheadType, RuleSet,
//! OverlayTypeRegistry), map (resolved terrain, house alliances), sim
//! (overlay grid, bridge topology). Does NOT depend on render/ui/sidebar/
//! audio/net.

use crate::map::houses::{HouseAllianceMap, are_houses_friendly};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::intern::StringInterner;
use crate::sim::map::bridge_topology::CellBridgeView;
use crate::sim::overlay_grid::OverlayGrid;

/// Leptons per cell — the divisor in the `CDQ; AND EDX,0xFF; ADD; SAR 8`
/// idiom the walk uses at 0x004CC12F, 0x004CC28A and 0x004CC371.
const LEPTONS_PER_CELL: i64 = 256;

/// `CellClass::GetEffectiveHeight` difference at or above which a step counts
/// as a cliff — `CMP EBX,0x4; JL` at 0x004CC3DE (and the duplicated
/// `CMP EAX,0x4` at 0x004CC5A7). Units are terrain Levels, not leptons.
const CLIFF_STEP_LEVELS: i32 = 4;

/// Overlay array indices that `CellClass::IsWallConnectableInDirection`
/// 0x00480510 accepts for the "any wall?" query — the form 0x004CC32D uses,
/// pushing `-1` for both the type and the direction. The three ids are hard
/// coded in that body.
///
/// **These are `[OverlayTypes]` DECLARATION indices, not the INI key text.**
/// `RulesClass::Process` reads the section by 0-based entry index — `XOR
/// EBX,EBX` at 0x00668CF3, `PUSH EBX` as the entry selector at 0x00668D0A,
/// `INC EBX; CMP EBX,EBP; JL` at 0x00668D2F-0x00668D32 — and never parses the
/// `N=` on the left of the line. Stock `rulesmd.ini` has 250 entries under keys
/// 1..253 with keys **40, 41 and 183 missing**, so `id == key - 1` holds only
/// below key 42. Resolved by declaration index the three ids are `GAWALL`
/// (key 3), `NAWALL` (key 27) and `GAFWLL` (key 247). `CAKRMW`, the Kremlin
/// wall, is id **240** and is NOT in this set — a wall-breaking shell is
/// refused by it exactly as it is by sandbags. VERA shares that numbering:
/// `rules::overlay_types::is_bridge_overlay_index` pins `BRIDGEB1`/`BRIDGEB2`
/// at 237/238, their declaration indices, against INI keys 241/242.
///
/// Id 243 is LIVE, and all three ids fire in ordinary play. `[GAFWLL]` is the
/// Yuri Citadel Wall: `rulesmd.ini:13561` declares it, `:13571` gives it
/// `Wall=yes`, and it carries `Name=Yuri Citadel Wall`, `TechLevel=2`,
/// `Prerequisite=YABRCK`. Its buildable half is `[BuildingTypes] 312`
/// (`rulesmd.ini:1499`), `artmd.ini:4129`'s `[GAFWLL]` gives it
/// `Cameo=CITWICON` and `ToOverlay=GAFWLL`, and `[AI] ConcreteWalls`
/// (`rulesmd.ini:3077`) names exactly `GAWALL,NAWALL,GAFWLL` — this set. Any
/// Yuri player who walls a base puts overlay 243 on the map.
///
/// It is easy to miss because its header carries a trailing comment —
/// `[GAFWLL];temp wall for yuri[YAWALL]` — which a `^\[NAME\]$` matcher
/// rejects. Neither real reader does. `INIClass::LoadFromStraw` 0x00525A60
/// tests `CMP byte ptr [ESP+0x78],0x5B` at 0x00525DC1, calls
/// `strchr(line, ']')` (`PUSH 0x5D` at 0x00525DCC) and NULs the FIRST `]` at
/// 0x00525DDB before naming the section from `buffer+1`; VERA's
/// `rules::ini_parser` takes the same `line.find(']')` prefix. Nor is this a
/// GAFWLL quirk: 61 stock `rulesmd.ini` sections and 154 in `artmd.ini` have
/// such headers, `[Controller]`, `[Parasite]`, `[FlakWeapon]` and
/// `[JumpjetControls]` among them.
///
/// Deliberately NOT "every `Wall=yes` overlay": stock authors `Wall=yes` on
/// eight overlays — `GASAND` (0), `GAWALL` (2), `NAWALL` (26), `CAFNCB` (203),
/// `CAFNCW` (204), `CAKRMW` (240), `CAFNCP` (241) and `GAFWLL` (243) — of
/// which only three are re-admitted, so a wall-breaking warhead does not get
/// waved through a sandbag or a fence line.
const WALL_CONNECTABLE_OVERLAY_IDS: [u8; 3] = [2, 26, 243];

/// Map state the walk consults beyond the resolved terrain grid.
///
/// `overlay_grid`/`overlay_registry` are `Option` because several sim entry
/// points run against terrain-only fixtures. A `None` there means "this map
/// carries no overlay plane", which is the same answer as "no wall on the
/// line" — it is not a VERA-only early-out. Cliffs need only terrain and are
/// therefore always evaluated.
///
/// `alliances` is consulted only under `[WallModel] AlliedWallTransparency`,
/// which stock rules set to `no`.
#[derive(Clone, Copy, Default)]
pub(crate) struct LineOfFireInputs<'a> {
    pub overlay_grid: Option<&'a OverlayGrid>,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub alliances: Option<&'a HouseAllianceMap>,
}

impl<'a> LineOfFireInputs<'a> {
    /// Terrain-only inputs: cliffs are still evaluated, walls cannot be.
    #[cfg(test)]
    pub(crate) const fn terrain_only() -> Self {
        Self {
            overlay_grid: None,
            overlay_registry: None,
            alliances: None,
        }
    }
}

/// One cell's state as the three native bodies read it.
#[derive(Clone, Copy)]
struct WalkCell {
    rx: u16,
    ry: u16,
    /// `CellClass::GetEffectiveHeight` 0x00487D50 —
    /// `(i8)Level(+0x11B) + ((flags(+0x140) >> 7) & 1) * 4`.
    effective_height: i32,
    /// The raw signed `Level` byte at `+0x11B`. The source-vs-target height
    /// test at 0x004CC444 compares this, NOT the effective height.
    level: i8,
    /// `CellClass+0x44` overlay type index, and whether
    /// `OverlayTypeClass+0x2A8` (`Wall=`, key string 0x0081AC58, read at
    /// 0x005FE7E0) is set on it.
    wall_overlay_id: Option<u8>,
    /// `Houses[CellClass+0x50]` — the wall's owning house, if any.
    wall_owner: Option<crate::sim::intern::InternedId>,
}

impl WalkCell {
    fn has_wall(&self) -> bool {
        self.wall_overlay_id.is_some()
    }
}

/// Fetch one cell's walk-relevant state.
///
/// Returns `None` when the coordinate is outside the resolved grid. Native's
/// `MapClass::Get_CellClass` 0x005657A0 answers a shared dummy `CellClass`
/// at 0x00ABDC50 instead of failing, and that dummy's persistent `Level` and
/// overlay index are **UNCHECKED** here. It is unreachable on this path in
/// ordinary play: the playfield is convex in cell space, so a straight line
/// between two in-playfield endpoints never leaves it.
fn walk_cell(
    terrain: &ResolvedTerrainGrid,
    los: &LineOfFireInputs<'_>,
    cx: i32,
    cy: i32,
) -> Option<WalkCell> {
    let rx = u16::try_from(cx).ok()?;
    let ry = u16::try_from(cy).ok()?;
    let cell = terrain.cell(rx, ry)?;
    let view = CellBridgeView::from_resolved(cell);

    let mut wall_overlay_id = None;
    let mut wall_owner = None;
    if let (Some(grid), Some(registry)) = (los.overlay_grid, los.overlay_registry) {
        let overlay_cell = grid.cell(rx, ry);
        if let Some(id) = overlay_cell.overlay_id {
            if registry.flags(id).is_some_and(|flags| flags.wall) {
                wall_overlay_id = Some(id);
                wall_owner = overlay_cell.wall_owner;
            }
        }
    }

    Some(WalkCell {
        rx,
        ry,
        effective_height: view.effective_height(),
        level: cell.level as i8,
        wall_overlay_id,
        wall_owner,
    })
}

/// `FUN_004CC360` 0x004CC360 — does the cell at `(x, y)` block the line?
///
/// ```text
/// ECX = source cell, EDX = target cell
/// [ESP+4] = the previous sample's cell, +8/+C/+10 = x/y/z leptons,
/// [ESP+14] = the projectile, [ESP+18] = the firing house
/// ```
///
/// **The tail at 0x004CC489–0x004CC572 is dead and is deliberately not
/// ported.** Every jump into 0x004CC489 (0x004CC3F7 cliff-blocked,
/// 0x004CC466 / 0x004CC475 wall-blocked, and the fallthrough from the
/// not-allied `JNZ` at 0x004CC483) has already concluded "blocked". The tail
/// then measures the sample point against the cell's own coords and, when the
/// perpendicular offset dominates, re-fetches a cell from the packed
/// `CellStruct` at `entry-0x20` — written once at 0x004CC3AD and never
/// touched again, so it re-reads the *same* cell — and re-runs a byte-for-byte
/// duplicate of the tests above it. Both duplicated tests read the identical
/// operands (`CMP EAX,0x4` / `CMP EBX,0x4`; `[ESP+0x1C]` vs `EBP` for the
/// source cell; `[ESP+0x10]` vs `EBX` for the target; `[ESP+0x50]` for the
/// house in both), so every path out of it returns the same cell the first
/// evaluation had already selected. The whole block therefore reduces to
/// `return stepCell`.
fn cell_blocks_line_of_fire(
    src_cell: &WalkCell,
    tgt_cell: &WalkCell,
    prev_cell: &WalkCell,
    x: i64,
    y: i64,
    subject_to_cliffs: bool,
    subject_to_walls: bool,
    firing_house: &str,
    allied_wall_transparency: bool,
    terrain: &ResolvedTerrainGrid,
    interner: &StringInterner,
    los: &LineOfFireInputs<'_>,
) -> Option<(u16, u16)> {
    let step = walk_cell(
        terrain,
        los,
        (x / LEPTONS_PER_CELL) as i32,
        (y / LEPTONS_PER_CELL) as i32,
    )?;

    // Cliff arm, 0x004CC3C0–0x004CC3F7. The step must rise at least four
    // Levels above the cell we stepped FROM *and* stand above the cell the
    // shot started in. A climb that only regains ground already lost does not
    // block.
    if subject_to_cliffs
        && step.effective_height - prev_cell.effective_height >= CLIFF_STEP_LEVELS
        && step.effective_height - src_cell.effective_height > 0
    {
        return Some((step.rx, step.ry));
    }

    // Wall arm, 0x004CC401 onward.
    if !subject_to_walls {
        return None;
    }
    // 0x004CC434 — the target's own cell never blocks; you are allowed to
    // shoot the wall you are aiming at.
    if (step.rx, step.ry) == (tgt_cell.rx, tgt_cell.ry) {
        return None;
    }
    if !step.has_wall() {
        return None;
    }
    // 0x004CC444 — `CMP AL,CL; JG`, a SIGNED byte compare of the raw `Level`
    // at `+0x11B` (not the effective height the cliff arm uses). Firing from
    // higher ground than the target clears the wall.
    if src_cell.level > tgt_cell.level {
        return None;
    }
    // 0x004CC458 — `[WallModel] AlliedWallTransparency`. When it is on, a
    // wall belonging to a house allied with the firer does not block.
    if allied_wall_transparency {
        if let (Some(owner), Some(alliances)) = (step.wall_owner, los.alliances) {
            if are_houses_friendly(alliances, firing_house, interner.resolve(owner)) {
                return None;
            }
        }
    }

    Some((step.rx, step.ry))
}

/// `FUN_004CC100` 0x004CC100 — walk the line and return the first blocking
/// cell.
///
/// Step count is the Chebyshev distance in CELLS between the source and target
/// cells (0x004CC191–0x004CC1B5); the per-step increments are the lepton
/// deltas divided by that count with truncating integer division
/// (`IDIV EBP` at 0x004CC1E7/0x004CC1EE/0x004CC1F9). Sample `i` sits at
/// `src + i * delta`, so the walk covers the source position and everything up
/// to but not including the target position.
///
/// The "previous cell" argument each step hands to the blocking predicate is
/// the cell of sample `i-1` — it starts as the source cell (0x004CC22F) and is
/// re-fetched from the sample just tested at 0x004CC2C2, *before* the
/// accumulators advance.
#[allow(clippy::too_many_arguments)]
fn walk_line_for_blocking_cell(
    src: (i64, i64),
    tgt: (i64, i64),
    subject_to_cliffs: bool,
    subject_to_walls: bool,
    firing_house: &str,
    allied_wall_transparency: bool,
    terrain: &ResolvedTerrainGrid,
    interner: &StringInterner,
    los: &LineOfFireInputs<'_>,
) -> Option<(u16, u16)> {
    // 0x004CC10B — neither key set, no walk at all.
    if !subject_to_cliffs && !subject_to_walls {
        return None;
    }

    let src_cx = (src.0 / LEPTONS_PER_CELL) as i32;
    let src_cy = (src.1 / LEPTONS_PER_CELL) as i32;
    let tgt_cx = (tgt.0 / LEPTONS_PER_CELL) as i32;
    let tgt_cy = (tgt.1 / LEPTONS_PER_CELL) as i32;

    let steps = (src_cx - tgt_cx).abs().max((src_cy - tgt_cy).abs());
    // 0x004CC22D — `CMP EBP,ECX; JLE`, so a same-cell shot never enters the
    // loop and can never be blocked.
    if steps <= 0 {
        return None;
    }

    // Both endpoint cells are read once, before the loop (0x004CC20D /
    // 0x004CC222), and reused for every sample.
    let src_cell = walk_cell(terrain, los, src_cx, src_cy)?;
    let tgt_cell = walk_cell(terrain, los, tgt_cx, tgt_cy)?;

    let dx = (tgt.0 - src.0) / i64::from(steps);
    let dy = (tgt.1 - src.1) / i64::from(steps);

    let mut prev_cell = src_cell;
    for i in 0..i64::from(steps) {
        let x = src.0 + dx * i;
        let y = src.1 + dy * i;
        if let Some(hit) = cell_blocks_line_of_fire(
            &src_cell,
            &tgt_cell,
            &prev_cell,
            x,
            y,
            subject_to_cliffs,
            subject_to_walls,
            firing_house,
            allied_wall_transparency,
            terrain,
            interner,
            los,
        ) {
            return Some(hit);
        }
        let Some(next) = walk_cell(
            terrain,
            los,
            (x / LEPTONS_PER_CELL) as i32,
            (y / LEPTONS_PER_CELL) as i32,
        ) else {
            return None;
        };
        prev_cell = next;
    }
    None
}

/// `FUN_004CC310` 0x004CC310 — the function `InRange` actually calls, and the
/// one that decides the gate.
///
/// ```text
/// 004cc322  CALL 0x004cc100                    ; the walk
/// 004cc32b  JZ   ...                           ; nothing blocked -> pass
/// 004cc333  CALL 0x00480510                    ; IsWallConnectableInDirection(-1, -1)
/// 004cc342  MOV  CL,[EAX+0x144]                ; weapon->Warhead->Wall
/// 004cc34d  XOR  EAX,EAX                       ; both true -> report NOT blocked
/// ```
///
/// The re-admission is the reason a Grizzly does not refuse to fire at a
/// target behind a wall it can knock down: `105mm`'s warhead carries
/// `Wall=yes`, so the shot is legal and the wall eats it. A warhead without
/// `Wall=` — small arms, most of `InvisibleLow`'s users — stays refused.
///
/// Returns the blocking cell, or `None` when the shot is legal.
pub(crate) fn line_of_fire_blocking_cell(
    src: (i64, i64),
    tgt: (i64, i64),
    weapon: &WeaponType,
    firing_house: &str,
    rules: &RuleSet,
    terrain: &ResolvedTerrainGrid,
    interner: &StringInterner,
    los: &LineOfFireInputs<'_>,
) -> Option<(u16, u16)> {
    // `WeaponType+0xA0`, the projectile, written at 0x007729AA.
    let projectile = weapon
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name));
    let subject_to_cliffs = projectile.is_some_and(|p| p.subject_to_cliffs);
    let subject_to_walls = projectile.is_some_and(|p| p.subject_to_walls);

    let hit = walk_line_for_blocking_cell(
        src,
        tgt,
        subject_to_cliffs,
        subject_to_walls,
        firing_house,
        rules.allied_wall_transparency,
        terrain,
        interner,
        los,
    )?;

    // `WeaponType+0xAC`, the warhead, written at 0x00772992; `Wall=` is
    // `WarheadType+0x144`, key string 0x0081AC58, read at 0x0075D508.
    let warhead_breaks_walls = weapon
        .warhead
        .as_deref()
        .and_then(|name| rules.warhead(name))
        .is_some_and(|wh| wh.wall);
    if warhead_breaks_walls && wall_is_connectable(terrain, los, hit) {
        return None;
    }

    Some(hit)
}

/// `CellClass::IsWallConnectableInDirection(cell, -1, -1)` 0x00480510 as
/// 0x004CC333 calls it: with the type argument `-1` the body reduces to a
/// membership test on the cell's own overlay index against three hard-coded
/// ids, and the direction argument is never read.
fn wall_is_connectable(
    terrain: &ResolvedTerrainGrid,
    los: &LineOfFireInputs<'_>,
    (rx, ry): (u16, u16),
) -> bool {
    let Some(grid) = los.overlay_grid else {
        return false;
    };
    // Bounds are the terrain grid's, matching the walk's own lookups.
    if terrain.cell(rx, ry).is_none() {
        return false;
    }
    grid.cell(rx, ry)
        .overlay_id
        .is_some_and(|id| WALL_CONNECTABLE_OVERLAY_IDS.contains(&id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::intern::StringInterner;
    use crate::sim::overlay_grid::OverlayGrid;

    const WALL_GAWALL_ID: u8 = 2;
    const SANDBAG_GASAND_ID: u8 = 0;
    const GRID: u16 = 16;

    /// Overlay ids follow `[OverlayTypes]` DECLARATION order (0x00668CF3 /
    /// 0x00668D0A read entries by 0-based index, never by key text), so the
    /// stock numbering that puts `GAWALL` at 2 and `NAWALL` at 26 has to be
    /// reproduced by the fixture list, not asserted from names.
    fn rules_ini(extra: &str) -> String {
        format!(
            "[OverlayTypes]\n1=GASAND\n2=CYCL\n3=GAWALL\n\n\
             [GASAND]\nWall=yes\n\n[CYCL]\n\n[GAWALL]\nWall=yes\n\n\
             [VehicleTypes]\n0=TANK\n\n\
             [TANK]\nStrength=100\nArmor=heavy\nPrimary=GUN\nSecondary=GUNWALL\n\n\
             [GUN]\nDamage=20\nROF=10\nRange=8\nSpeed=30\nProjectile=SHELL\nWarhead=WH\n\n\
             [GUNWALL]\nDamage=20\nROF=10\nRange=8\nSpeed=30\nProjectile=SHELL\nWarhead=WHWALL\n\n\
             {extra}\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
             [WHWALL]\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
        )
    }

    fn rules_with(extra: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(&rules_ini(extra))).expect("rules parse")
    }

    fn overlay_registry(extra: &str) -> OverlayTypeRegistry {
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&rules_ini(extra)), None)
    }

    fn cell_at(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: true,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: Default::default(),
            speed_costs: Default::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            accepts_smudge: true,
            allows_tiberium: false,
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn flat_terrain() -> ResolvedTerrainGrid {
        let cells: Vec<ResolvedTerrainCell> = (0..GRID)
            .flat_map(|ry| (0..GRID).map(move |rx| cell_at(rx, ry)))
            .collect();
        ResolvedTerrainGrid::from_cells(GRID, GRID, cells)
    }

    fn set_level(terrain: &mut ResolvedTerrainGrid, rx: u16, ry: u16, level: u8) {
        terrain.cell_mut(rx, ry).expect("cell in grid").level = level;
    }

    fn overlay_grid_with(cells: &[(u16, u16, u8)]) -> OverlayGrid {
        let mut grid = OverlayGrid::new(GRID, GRID);
        for &(rx, ry, id) in cells {
            grid.cell_mut(rx, ry).overlay_id = Some(id);
        }
        grid
    }

    fn centre(cx: i64, cy: i64) -> (i64, i64) {
        (cx * 256 + 128, cy * 256 + 128)
    }

    /// The headline case: `[Cannon]` is `SubjectToWalls=yes`, so a tank whose
    /// warhead cannot break walls is refused a shot through one.
    #[test]
    fn a_wall_between_attacker_and_target_blocks_the_shot() {
        let extra = "[SHELL]\nSubjectToWalls=yes\n";
        let rules = rules_with(extra);
        let registry = overlay_registry(extra);
        let terrain = flat_terrain();
        let overlays = overlay_grid_with(&[(4, 2, WALL_GAWALL_ID)]);
        let interner = StringInterner::new();
        let los = LineOfFireInputs {
            overlay_grid: Some(&overlays),
            overlay_registry: Some(&registry),
            alliances: None,
        };
        let weapon = rules.weapon("GUN").expect("GUN");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                centre(6, 2),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            Some((4, 2)),
        );
    }

    /// 0x004CC10B — a projectile with neither key never walks, so the same
    /// wall is invisible to it.
    #[test]
    fn a_projectile_with_neither_key_never_walks() {
        let extra = "[SHELL]\nROT=0\n";
        let rules = rules_with(extra);
        let registry = overlay_registry(extra);
        let terrain = flat_terrain();
        let overlays = overlay_grid_with(&[(4, 2, WALL_GAWALL_ID)]);
        let interner = StringInterner::new();
        let los = LineOfFireInputs {
            overlay_grid: Some(&overlays),
            overlay_registry: Some(&registry),
            alliances: None,
        };
        let weapon = rules.weapon("GUN").expect("GUN");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                centre(6, 2),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            None,
        );
    }

    /// 0x004CC333 + 0x004CC342 — a `Wall=yes` warhead is waved through a
    /// GAWALL, which is one of the three ids `IsWallConnectableInDirection`
    /// accepts.
    #[test]
    fn a_wall_breaking_warhead_is_readmitted_through_a_gawall() {
        let extra = "[SHELL]\nSubjectToWalls=yes\n";
        let rules = rules_with(extra);
        let registry = overlay_registry(extra);
        let terrain = flat_terrain();
        let overlays = overlay_grid_with(&[(4, 2, WALL_GAWALL_ID)]);
        let interner = StringInterner::new();
        let los = LineOfFireInputs {
            overlay_grid: Some(&overlays),
            overlay_registry: Some(&registry),
            alliances: None,
        };
        // `GUNWALL` carries `Warhead=WHWALL`, whose `Wall=yes` is the
        // `WarheadType+0x144` byte 0x004CC342 reads.
        let weapon = rules.weapon("GUNWALL").expect("GUNWALL");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                centre(6, 2),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            None,
        );
    }

    /// The re-admission is narrower than `Wall=yes`: `GASAND` (overlay id 0)
    /// carries `Wall=yes` and still blocks, because 0x00480510's `-1` arm
    /// accepts only ids 2, 26 and 243.
    #[test]
    fn a_wall_breaking_warhead_is_still_blocked_by_a_sandbag_wall() {
        let extra = "[SHELL]\nSubjectToWalls=yes\n";
        let rules = rules_with(extra);
        let registry = overlay_registry(extra);
        let terrain = flat_terrain();
        let overlays = overlay_grid_with(&[(4, 2, SANDBAG_GASAND_ID)]);
        let interner = StringInterner::new();
        let los = LineOfFireInputs {
            overlay_grid: Some(&overlays),
            overlay_registry: Some(&registry),
            alliances: None,
        };
        // `GUNWALL` carries `Warhead=WHWALL`, whose `Wall=yes` is the
        // `WarheadType+0x144` byte 0x004CC342 reads.
        let weapon = rules.weapon("GUNWALL").expect("GUNWALL");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                centre(6, 2),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            Some((4, 2)),
        );
    }

    /// 0x004CC434 — the wall standing on the target's own cell is the wall you
    /// are shooting at, and never blocks.
    #[test]
    fn a_wall_on_the_targets_own_cell_does_not_block() {
        let extra = "[SHELL]\nSubjectToWalls=yes\n";
        let rules = rules_with(extra);
        let registry = overlay_registry(extra);
        let terrain = flat_terrain();
        let overlays = overlay_grid_with(&[(6, 2, WALL_GAWALL_ID)]);
        let interner = StringInterner::new();
        let los = LineOfFireInputs {
            overlay_grid: Some(&overlays),
            overlay_registry: Some(&registry),
            alliances: None,
        };
        let weapon = rules.weapon("GUN").expect("GUN");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                centre(6, 2),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            None,
        );
    }

    /// 0x004CC444 — firing from higher raw `Level` than the target clears the
    /// wall. The compare is on `+0x11B`, so a bridge anchor's `+4` effective
    /// height must NOT enter it.
    #[test]
    fn firing_downhill_clears_the_wall() {
        let extra = "[SHELL]\nSubjectToWalls=yes\n";
        let rules = rules_with(extra);
        let registry = overlay_registry(extra);
        let mut terrain = flat_terrain();
        set_level(&mut terrain, 2, 2, 3);
        let overlays = overlay_grid_with(&[(4, 2, WALL_GAWALL_ID)]);
        let interner = StringInterner::new();
        let los = LineOfFireInputs {
            overlay_grid: Some(&overlays),
            overlay_registry: Some(&registry),
            alliances: None,
        };
        let weapon = rules.weapon("GUN").expect("GUN");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                centre(6, 2),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            None,
            "source Level 3 > target Level 0 clears the wall (0x004CC444)"
        );
    }

    /// 0x004CC3DE / 0x004CC3F7 — a four-Level rise above BOTH the previous
    /// cell and the source cell blocks a `SubjectToCliffs` shot. Three Levels
    /// does not: the boundary is `>= 4`.
    #[test]
    fn a_four_level_cliff_blocks_and_three_levels_does_not() {
        let extra = "[SHELL]\nSubjectToCliffs=yes\n";
        let rules = rules_with(extra);
        let interner = StringInterner::new();
        let los = LineOfFireInputs::terrain_only();
        let weapon = rules.weapon("GUN").expect("GUN");

        for (rise, expect) in [(4u8, Some((4u16, 2u16))), (3u8, None)] {
            let mut terrain = flat_terrain();
            for ry in 0..GRID {
                set_level(&mut terrain, 4, ry, rise);
            }
            assert_eq!(
                line_of_fire_blocking_cell(
                    centre(2, 2),
                    centre(6, 2),
                    weapon,
                    "Americans",
                    &rules,
                    &terrain,
                    &interner,
                    &los,
                ),
                expect,
                "a {rise}-Level step verdict"
            );
        }
    }

    /// The second half of 0x004CC3F7: the step must also stand above the
    /// SOURCE cell. Climbing four Levels out of a pit back to the source's own
    /// height is not a cliff.
    #[test]
    fn a_step_that_only_regains_the_source_height_is_not_a_cliff() {
        let extra = "[SHELL]\nSubjectToCliffs=yes\n";
        let rules = rules_with(extra);
        let mut terrain = flat_terrain();
        for ry in 0..GRID {
            set_level(&mut terrain, 2, ry, 4);
            set_level(&mut terrain, 3, ry, 0);
            set_level(&mut terrain, 4, ry, 4);
        }
        let interner = StringInterner::new();
        let los = LineOfFireInputs::terrain_only();
        let weapon = rules.weapon("GUN").expect("GUN");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                centre(6, 2),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            None,
        );
    }

    /// 0x004CC22D — a same-cell shot has zero steps and is never blocked, even
    /// standing on a wall.
    #[test]
    fn a_same_cell_shot_is_never_blocked() {
        let extra = "[SHELL]\nSubjectToWalls=yes\n";
        let rules = rules_with(extra);
        let registry = overlay_registry(extra);
        let terrain = flat_terrain();
        let overlays = overlay_grid_with(&[(2, 2, WALL_GAWALL_ID)]);
        let interner = StringInterner::new();
        let los = LineOfFireInputs {
            overlay_grid: Some(&overlays),
            overlay_registry: Some(&registry),
            alliances: None,
        };
        let weapon = rules.weapon("GUN").expect("GUN");

        assert_eq!(
            line_of_fire_blocking_cell(
                centre(2, 2),
                (2 * 256 + 200, 2 * 256 + 40),
                weapon,
                "Americans",
                &rules,
                &terrain,
                &interner,
                &los,
            ),
            None,
        );
    }

    /// 0x004CC458 — with `[WallModel] AlliedWallTransparency=yes` an allied
    /// wall stops blocking; with the stock `no` it still blocks.
    #[test]
    fn allied_wall_transparency_gates_the_owner_exemption() {
        use std::collections::{BTreeMap, BTreeSet};

        let terrain = flat_terrain();
        let mut alliances: HouseAllianceMap = BTreeMap::new();
        // `normalize_house_name` upper-cases both sides, so the fixture map
        // has to be keyed the same way.
        alliances.insert(
            "AMERICANS".to_string(),
            BTreeSet::from(["FRENCH".to_string()]),
        );

        for (transparency, expect) in [("yes", None), ("no", Some((4u16, 2u16)))] {
            let extra = format!(
                "[SHELL]\nSubjectToWalls=yes\n\n[WallModel]\nAlliedWallTransparency={transparency}\n"
            );
            let rules = rules_with(&extra);
            let registry = overlay_registry(&extra);
            let mut interner = StringInterner::new();
            let mut overlays = overlay_grid_with(&[(4, 2, WALL_GAWALL_ID)]);
            overlays.cell_mut(4, 2).wall_owner = Some(interner.intern("French"));
            let los = LineOfFireInputs {
                overlay_grid: Some(&overlays),
                overlay_registry: Some(&registry),
                alliances: Some(&alliances),
            };
            let weapon = rules.weapon("GUN").expect("GUN");

            assert_eq!(
                line_of_fire_blocking_cell(
                    centre(2, 2),
                    centre(6, 2),
                    weapon,
                    "Americans",
                    &rules,
                    &terrain,
                    &interner,
                    &los,
                ),
                expect,
                "AlliedWallTransparency={transparency}"
            );
        }
    }
}
