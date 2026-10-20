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
//! (overlay grid, bridge topology). Does NOT depend on render/ui/
//! audio/net.

use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::cell_rect::{CellRef, get_cellclass_in_query};
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::overlay_grid::OverlayGrid;

/// Leptons per cell — the divisor in the `CDQ; AND EDX,0xFF; ADD; SAR 8`
/// idiom the walk uses at 0x004CC12F, 0x004CC28A and 0x004CC371.
const LEPTONS_PER_CELL: i32 = 256;

/// `CellClass::GetEffectiveHeight` difference at or above which a step counts
/// as a cliff — `CMP EBX,0x4; JL` at 0x004CC3DE (and the duplicated
/// `CMP EAX,0x4` at 0x004CC5A7). Units are terrain Levels, not leptons.
const CLIFF_STEP_LEVELS: i32 = 4;

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
pub struct LineOfFireInputs<'a> {
    pub overlay_grid: Option<&'a OverlayGrid>,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub alliances: Option<&'a HouseAllianceMap>,
}

impl<'a> LineOfFireInputs<'a> {
    /// Terrain-only inputs: cliffs are still evaluated, walls cannot be.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn terrain_only() -> Self {
        Self {
            overlay_grid: None,
            overlay_registry: None,
            alliances: None,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static LINE_SAMPLES: std::cell::RefCell<Option<Vec<DriveCoord>>> = const { std::cell::RefCell::new(None) };
}

/// Read-only production-entry observations for the original4CC100 corpus.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn trace_line_samples<T>(run: impl FnOnce() -> T) -> (T, Vec<DriveCoord>) {
    LINE_SAMPLES.with_borrow_mut(|trace| {
        assert!(trace.is_none(), "nested line sample trace");
        *trace = Some(Vec::new());
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            LINE_SAMPLES.with_borrow_mut(|trace| *trace = None);
        }
    }
    let reset = Reset;
    let result = run();
    let samples = LINE_SAMPLES.with_borrow_mut(|trace| trace.take().unwrap());
    drop(reset);
    (result, samples)
}

/// Retained inputs to native4CC360. Cell identities remain live through
/// nested lookups, which can restamp an aliased shared Dummy.
pub(crate) struct CellObstructionProbe<'a> {
    pub source: CellRef<'a>,
    pub target: CellRef<'a>,
    pub previous: CellRef<'a>,
    pub sample: DriveCoord,
    pub subject_to_cliffs: bool,
    pub subject_to_walls: bool,
    pub firing_house: Option<InternedId>,
}

fn overlay(cell: &CellRef<'_>, los: &LineOfFireInputs<'_>) -> (Option<u8>, Option<InternedId>) {
    match cell {
        CellRef::Real(cell) => los.overlay_grid.map_or((None, None), |grid| {
            let entry = grid.cell(cell.rx, cell.ry);
            (entry.overlay_id, entry.wall_owner)
        }),
        CellRef::Dummy { cell } => (cell.overlay_fields().0, None),
    }
}

/// Native4CC360: one cliff/wall probe shared by range checks and Bullet AI.
/// Its geometry tail4CC489..671 is live: getters/requery can observe and
/// change the same Dummy identity even when a real-cell verdict is unchanged.
pub(crate) fn cell_blocks_line_of_fire<'a>(
    probe: CellObstructionProbe<'a>,
    interner: &StringInterner,
    allied_wall_transparency: bool,
    los: &LineOfFireInputs<'_>,
    mut lookup: impl FnMut((i16, i16)) -> CellRef<'a>,
) -> Option<CellRef<'a>> {
    #[cfg(any(test, feature = "test-support"))]
    LINE_SAMPLES.with_borrow_mut(|trace| {
        if let Some(samples) = trace {
            samples.push(probe.sample);
        }
    });
    let packed = (
        (probe.sample.x / LEPTONS_PER_CELL) as i16,
        (probe.sample.y / LEPTONS_PER_CELL) as i16,
    );
    let cell = lookup(packed);
    let effective = |cell: &CellRef<'_>| {
        crate::sim::map::bridge_topology::CellBridgeView::from_cell_ref(cell).effective_height()
    };
    let blocks = |cell: &CellRef<'_>| {
        if probe.subject_to_cliffs
            && effective(cell).wrapping_sub(effective(&probe.previous)) >= CLIFF_STEP_LEVELS
            && effective(cell) > effective(&probe.source)
        {
            return true;
        }
        if !probe.subject_to_walls
            || cell == &probe.target
            || probe.source.signed_level() > probe.target.signed_level()
        {
            return false;
        }
        let (id, owner) = overlay(cell, los);
        if !id.is_some_and(|id| {
            los.overlay_registry
                .and_then(|r| r.flags(id))
                .is_some_and(|f| f.wall)
        }) {
            return false;
        }
        //4CC470 wall-owner House is receiver; firing House is argument.
        if allied_wall_transparency
            && let (Some(owner), Some(firer), Some(alliances)) =
                (owner, probe.firing_house, los.alliances)
            // Terrain-only fixtures may not bind the owner into this interner.
            // Preserve InRange's prior unresolvable-house admission policy.
            && let (Some(owner), Some(firer)) = (interner.try_resolve(owner), interner.try_resolve(firer))
            && is_allied_with(alliances, owner, firer)
        {
            return false;
        }
        true
    };
    if !blocks(&cell) {
        return None;
    }
    let center = cell.center_coordinate().expect("Cell486840 slope");
    if crate::util::native_x87::distance_3d_leptons(
        [probe.sample.x, probe.sample.z, probe.sample.y],
        [center.x, center.z, center.y],
    ) <= 85
    {
        return Some(cell);
    }
    let source = probe
        .source
        .center_coordinate()
        .expect("source Cell486840 slope");
    let target = probe
        .target
        .center_coordinate()
        .expect("target Cell486840 slope");
    let major_x = source.x.wrapping_sub(target.x).wrapping_abs();
    let major_y = source.y.wrapping_sub(target.y).wrapping_abs();
    let off_x = probe.sample.x.wrapping_sub(center.x).wrapping_abs();
    let off_y = probe.sample.y.wrapping_sub(center.y).wrapping_abs();
    if (major_x > major_y && off_y > off_x) || (major_y > major_x && off_x > off_y) {
        let second = lookup(packed);
        if !blocks(&second) {
            return None;
        }
    }
    Some(cell)
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
fn walk_line_for_blocking_cell<'a>(
    src: (i64, i64, i64),
    tgt: (i64, i64, i64),
    subject_to_cliffs: bool,
    subject_to_walls: bool,
    firing_house: Option<InternedId>,
    allied_wall_transparency: bool,
    cells: &NativeCellQuery<'a>,
    interner: &StringInterner,
    los: &LineOfFireInputs<'_>,
) -> Option<CellRef<'a>> {
    if !subject_to_cliffs && !subject_to_walls {
        return None;
    }
    let src = [src.0 as i32, src.1 as i32, src.2 as i32];
    let tgt = [tgt.0 as i32, tgt.1 as i32, tgt.2 as i32];
    let packed = |p: [i32; 3]| {
        (
            (p[0] / LEPTONS_PER_CELL) as i16,
            (p[1] / LEPTONS_PER_CELL) as i16,
        )
    };
    let (sx, sy) = packed(src);
    let (tx, ty) = packed(tgt);
    let steps = (i32::from(sx) - i32::from(tx))
        .abs()
        .max((i32::from(sy) - i32::from(ty)).abs());
    //4CC1E7/1EE/1F9 signed IDIV; zero steps never enter the walk.
    let delta = if steps == 0 {
        [0; 3]
    } else {
        std::array::from_fn(|i| tgt[i].wrapping_sub(src[i]) / steps)
    };
    let lookup = |(x, y): (i16, i16)| {
        get_cellclass_in_query(
            Some(cells.terrain()),
            i32::from(x),
            i32::from(y),
            Some(cells),
        )
    };
    //4CC20D/222 happen even for a same-cell shot, retaining their Dummy writes.
    let source = lookup((sx, sy));
    let target = lookup((tx, ty));
    let mut previous = source.clone();
    let mut offset = [0i32; 3];
    for _ in 0..steps {
        let sample = std::array::from_fn::<_, 3, _>(|i| src[i].wrapping_add(offset[i]));
        if let Some(hit) = cell_blocks_line_of_fire(
            CellObstructionProbe {
                source: source.clone(),
                target: target.clone(),
                previous: previous.clone(),
                sample: DriveCoord {
                    x: sample[0],
                    y: sample[1],
                    z: sample[2],
                },
                subject_to_cliffs,
                subject_to_walls,
                firing_house,
            },
            interner,
            allied_wall_transparency,
            los,
            lookup,
        ) {
            return Some(hit);
        }
        //4CC2C2 reads previous before XYZ accumulators advance.
        previous = lookup(packed(sample));
        for i in 0..3 {
            offset[i] = offset[i].wrapping_add(delta[i]);
        }
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
pub(crate) fn line_of_fire_blocking_cell<'a>(
    src: (i64, i64, i64),
    tgt: (i64, i64, i64),
    weapon: &WeaponType,
    firing_house: Option<InternedId>,
    rules: &RuleSet,
    cells: &NativeCellQuery<'a>,
    interner: &StringInterner,
    los: &LineOfFireInputs<'_>,
) -> Option<CellRef<'a>> {
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
        cells,
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
    if warhead_breaks_walls
        && matches!(
            crate::sim::cell_kernel::wall_connection_step(
                overlay(&hit, los).0.map_or(-1, i32::from),
                -1,
            ),
            crate::sim::cell_kernel::WallConnectionStep::Return(true)
        )
    {
        return None;
    }

    Some(hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rules::ini_parser::IniFile;

    use crate::sim::intern::StringInterner;
    use crate::sim::overlay_grid::OverlayGrid;

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

    fn overlay_registry(extra: &str) -> OverlayTypeRegistry {
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&rules_ini(extra)), None)
    }

    #[test]
    fn original_line_walk_signed_xyz_and_obstruction_controls() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/line_walk_vectors.json",
        ))
        .unwrap();
        assert_eq!(
            native["sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        for row in native["cases"].as_array().unwrap() {
            let name = row["name"].as_str().unwrap();
            let mut terrain = crate::map::resolved_terrain::test_grid(
                8,
                8,
                crate::map::resolved_terrain::test_flat_cell,
            );
            let mut overlays = OverlayGrid::new(8, 8);
            for y in 0..8u16 {
                for x in 0..8u16 {
                    let values = &row["cells"][format!("{x},{y}")];
                    let cell = terrain.cell_mut_for_test(x, y).unwrap();
                    cell.level = values["level"].as_i64().unwrap_or(0) as u8;
                    cell.slope_type = values["slope"].as_u64().unwrap_or(0) as u8;
                    cell.bridge_facts.raw_flags = values["flags"].as_u64().unwrap_or(0) as u32;
                    if values["wall"] == true {
                        overlays.place_overlay(x, y, 0, 0);
                    }
                }
            }
            let query = NativeCellQuery::canonical(&terrain);
            let registry = overlay_registry("");
            let interner = StringInterner::new();
            let los = LineOfFireInputs {
                overlay_grid: Some(&overlays),
                overlay_registry: Some(&registry),
                alliances: None,
            };
            let point = |field: &str| {
                (
                    row[field][0].as_i64().unwrap(),
                    row[field][1].as_i64().unwrap(),
                    row[field][2].as_i64().unwrap(),
                )
            };
            let (result, samples) = trace_line_samples(|| {
                walk_line_for_blocking_cell(
                    point("source_coord"),
                    point("target_coord"),
                    row["cliffs"] == true,
                    row["walls"] == true,
                    None,
                    false,
                    &query,
                    &interner,
                    &los,
                )
            });
            let actual_samples: Vec<_> = samples.iter().map(|p| [p.x, p.y, p.z]).collect();
            assert_eq!(
                serde_json::json!(actual_samples),
                row["samples"],
                "{name}: original4CC360 entry XYZ"
            );
            let hit = result.map(|cell| match cell {
                CellRef::Real(cell) => [i32::from(cell.rx), i32::from(cell.ry)],
                CellRef::Dummy { cell } => {
                    let (x, y) = cell.snapshot().coord;
                    [x, y]
                }
            });
            assert_eq!(
                serde_json::json!(hit),
                row["blocking_cell"],
                "{name}: original4CC100 result"
            );
            let (x, y) = query.dummy().snapshot().coord;
            assert_eq!(
                serde_json::json!([x, y]),
                row["dummy_coord"],
                "{name}: retained Dummy writes"
            );
        }
    }
}
