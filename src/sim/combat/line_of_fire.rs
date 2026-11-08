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
//! Ares builds replace all three (`AresTrajectoryHelper`,
//! `FindFirstImpenetrableObstacle 1006E1F0`, `FindFirstObstacle 1006E2B0`,
//! `GetObstacle 1006E540`): InRange calls them through hooks 0x6F7511 and
//! 0x6F7631 (`1004D120`) with the firer and its target, and the bullet's
//! two callers through 0x4688BD and 0x468BE2 (`rules::ares_bullet_type`).
//! A `SubjectToBuildings=` projectile also stops at a solid building; the
//! hooks at 0x4CC100/0x4CC310/0x4CC360 themselves are reached by no caller
//! then. The Ares walk keeps the integer stepping (`IDIV` at 1006E411..42B),
//! looks the target cell up before the source, and drops the vanilla
//! geometry tail.
//!
//! Depends on: rules (WeaponType, ProjectileType, WarheadType, RuleSet,
//! OverlayTypeRegistry), map (resolved terrain, house alliances), sim
//! (overlay grid, bridge topology; Ares: cell occupancy and entities). Does
//! NOT depend on render/ui/audio/net.

use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::projectile_type::ProjectileType;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::cell_rect::{CellRef, get_cellclass_in_query};
use crate::sim::components::DriveCoord;
use crate::sim::house_diplomacy::HouseAlliances;
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
/// which stock rules set to `no` (and, in Ares builds, under `[CombatDamage]
/// AlliedSolidTransparency` and for `SolidLevel=` penetration).
#[derive(Clone, Copy, Default)]
pub struct LineOfFireInputs<'a> {
    pub overlay_grid: Option<&'a OverlayGrid>,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub alliances: Option<&'a HouseAlliances>,
    /// Ares: the buildings standing in the map's cells, which a
    /// `SubjectToBuildings=` projectile tests. `None` (terrain-only
    /// fixtures) is the same answer as "no building on the line".
    #[cfg(feature = "ares")]
    pub buildings: Option<CellBuildings<'a>>,
}

impl<'a> LineOfFireInputs<'a> {
    /// Terrain-only inputs: cliffs are still evaluated, walls cannot be.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn terrain_only() -> Self {
        Self {
            overlay_grid: None,
            overlay_registry: None,
            alliances: None,
            #[cfg(feature = "ares")]
            buildings: None,
        }
    }
}

/// The cell object lists the Ares building test reads: `CellClass::GetBuilding`
/// (`0x0047C520`, the first building on the ground list) and that building.
#[cfg(feature = "ares")]
#[derive(Clone, Copy)]
pub struct CellBuildings<'a> {
    pub(crate) occupancy: &'a crate::sim::occupancy::OccupancyGrid,
    pub(crate) entities: &'a crate::sim::entity_store::EntityStore,
}

#[cfg(feature = "ares")]
impl<'a> CellBuildings<'a> {
    /// The live world's buildings.
    pub fn of(world: &'a crate::sim::world::Simulation) -> Self {
        Self {
            occupancy: &world.substrate.occupancy,
            entities: &world.substrate.entities,
        }
    }

    /// `CellClass::GetBuilding @ 0x0047C520` of `cell`; the shared Dummy
    /// holds no object.
    fn building(&self, cell: &CellRef<'_>) -> Option<&'a crate::sim::game_entity::GameEntity> {
        let CellRef::Real(cell) = cell else {
            return None;
        };
        let id = self.occupancy.first_building_on_layer(
            cell.rx,
            cell.ry,
            crate::sim::movement::locomotor::MovementLayer::Ground,
        )?;
        self.entities.get(id)
    }
}

/// Who fires along the line: the firing house (the walls' and, in Ares
/// builds, the buildings' alliance argument) and, in Ares builds, the firer
/// and its target, whose own buildings never block (`AresTrajectoryHelper`
/// `pSource`/`pTarget`, `1006E702..1006E710`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LineOfFireParties {
    pub firing_house: Option<InternedId>,
    #[cfg(feature = "ares")]
    pub source_object: Option<u64>,
    #[cfg(feature = "ares")]
    pub target_object: Option<u64>,
}

/// One shot's walk inputs: its projectile's obstacle flags and its parties.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LineOfFireShot {
    pub subject_to_cliffs: bool,
    pub subject_to_walls: bool,
    /// Ares BulletTypeExt `SubjectToBuildings` (`+0x8`).
    #[cfg(feature = "ares")]
    pub subject_to_solid: bool,
    pub parties: LineOfFireParties,
}

impl LineOfFireShot {
    /// `projectile`'s flags (`BulletType+0x296`, `+0x298`; Ares ext `+0x8`).
    pub(crate) fn of(projectile: Option<&ProjectileType>, parties: LineOfFireParties) -> Self {
        Self {
            subject_to_cliffs: projectile.is_some_and(|p| p.subject_to_cliffs),
            subject_to_walls: projectile.is_some_and(|p| p.subject_to_walls),
            #[cfg(feature = "ares")]
            subject_to_solid: projectile.is_some_and(|p| p.bullet_type_ext.subject_to_solid),
            parties,
        }
    }

    /// 0x004CC10B's gate; Ares `SubjectToAnything` adds the buildings.
    fn subject_to_anything(&self) -> bool {
        #[cfg(feature = "ares")]
        let solid = self.subject_to_solid;
        #[cfg(not(feature = "ares"))]
        let solid = false;
        self.subject_to_cliffs || self.subject_to_walls || solid
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
    pub shot: LineOfFireShot,
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

/// The cell holds a `Wall=yes` overlay; its wall owner (`CellClass+0x50`).
fn wall(cell: &CellRef<'_>, los: &LineOfFireInputs<'_>) -> Option<Option<InternedId>> {
    let (id, owner) = overlay(cell, los);
    id.is_some_and(|id| {
        los.overlay_registry
            .and_then(|r| r.flags(id))
            .is_some_and(|f| f.wall)
    })
    .then_some(owner)
}

/// `HouseClass::IsAlliedWith @ 0x004F9A50` of `asker` about `other`, both
/// houses present. Terrain-only fixtures may not bind a house into this
/// interner; such a pair stays unallied, InRange's prior unresolvable-house
/// admission policy.
fn houses_allied(
    asker: Option<InternedId>,
    other: Option<InternedId>,
    interner: &StringInterner,
    los: &LineOfFireInputs<'_>,
) -> bool {
    let (Some(asker), Some(other), Some(alliances)) = (asker, other, los.alliances) else {
        return false;
    };
    interner.try_resolve(asker).is_some()
        && interner.try_resolve(other).is_some()
        && alliances.is_allied(interner, asker, other)
}

/// Native4CC360: one cliff/wall probe shared by range checks and Bullet AI.
/// Its geometry tail4CC489..671 is live: getters/requery can observe and
/// change the same Dummy identity even when a real-cell verdict is unchanged.
/// Ares builds answer with `GetObstacle 1006E540` instead (`ares_obstacle`).
pub(crate) fn cell_blocks_line_of_fire<'a>(
    probe: CellObstructionProbe<'a>,
    interner: &StringInterner,
    rules: &RuleSet,
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
    #[cfg(feature = "ares")]
    {
        ares_obstacle(&probe, cell, interner, rules, los)
    }
    #[cfg(not(feature = "ares"))]
    {
        native_obstacle(probe, cell, packed, interner, rules, los, lookup)
    }
}

/// 0x004CC360's verdict and geometry tail for the sampled `cell`.
#[cfg(not(feature = "ares"))]
fn native_obstacle<'a>(
    probe: CellObstructionProbe<'a>,
    cell: CellRef<'a>,
    packed: (i16, i16),
    interner: &StringInterner,
    rules: &RuleSet,
    los: &LineOfFireInputs<'_>,
    mut lookup: impl FnMut((i16, i16)) -> CellRef<'a>,
) -> Option<CellRef<'a>> {
    let effective = |cell: &CellRef<'_>| {
        crate::sim::map::bridge_topology::CellBridgeView::from_cell_ref(cell).effective_height()
    };
    let blocks = |cell: &CellRef<'_>| {
        if probe.shot.subject_to_cliffs
            && effective(cell).wrapping_sub(effective(&probe.previous)) >= CLIFF_STEP_LEVELS
            && effective(cell) > effective(&probe.source)
        {
            return true;
        }
        if !probe.shot.subject_to_walls
            || cell == &probe.target
            || probe.source.signed_level() > probe.target.signed_level()
        {
            return false;
        }
        let Some(owner) = wall(cell, los) else {
            return false;
        };
        //4CC470 wall-owner House is receiver; firing House is argument.
        !(rules.allied_wall_transparency
            && houses_allied(owner, probe.shot.parties.firing_house, interner, los))
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

/// `AresTrajectoryHelper::GetObstacle @ 1006E540`: the sampled `cell`
/// blocks for a cliff, a wall or a solid building, with no geometry tail.
///
/// - Cliff (`1006E5F5..1006E645`): the unsigned Level plus 4 for the bridge
///   bit `0x100` rises at least 4 over the previous cell and above the
///   source cell.
/// - Wall (`1006E64B..1006E6CE`): a `Wall=yes` overlay other than the
///   target cell's, fired from no higher (unsigned Levels) than the target;
///   `AlliedWallTransparency` lets the wall owner's allies through, and a
///   missing firer is no ally.
/// - Building (`1006E6D4..1006E794`): `SubjectToBuildings=`; the cell's
///   building, unless it is the firer or the target, an ally of the firer
///   under `[CombatDamage] AlliedSolidTransparency`, or an open gate
///   (`BuildingClass::IsTraversable @ 0x004525F0`), whose `SolidHeight=` is
///   set (negative: the type's `Height=`) and whose top, that many levels
///   of 256 leptons over `MapClass::GetCellFloorHeight @ 0x00578080` at the
///   sample, is at or above the sample.
#[cfg(feature = "ares")]
fn ares_obstacle<'a>(
    probe: &CellObstructionProbe<'a>,
    cell: CellRef<'a>,
    interner: &StringInterner,
    rules: &RuleSet,
    los: &LineOfFireInputs<'_>,
) -> Option<CellRef<'a>> {
    let level = |cell: &CellRef<'_>| {
        let bridge = cell.bridge_flags_0x1180() & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        i32::from(cell.signed_level() as u8) + if bridge != 0 { 4 } else { 0 }
    };
    let shot = &probe.shot;
    let cliff = || {
        shot.subject_to_cliffs
            && level(&cell) - level(&probe.previous) >= CLIFF_STEP_LEVELS
            && level(&cell) - level(&probe.source) > 0
    };
    let wall = || {
        if !shot.subject_to_walls
            || cell == probe.target
            || probe.source.signed_level() as u8 > probe.target.signed_level() as u8
        {
            return false;
        }
        let Some(owner) = wall(&cell, los) else {
            return false;
        };
        !(rules.allied_wall_transparency
            && houses_allied(owner, shot.parties.firing_house, interner, los))
    };
    let building = || {
        if !shot.subject_to_solid {
            return false;
        }
        let Some(building) = los
            .buildings
            .and_then(|buildings| buildings.building(&cell))
        else {
            return false;
        };
        let id = Some(building.stable_id());
        if id == shot.parties.source_object || id == shot.parties.target_object {
            return false;
        }
        // RulesExt+0x110; the building's owner is the receiver (1006E728).
        if rules.ares_building_globals().allied_solid_transparency
            && houses_allied(
                Some(building.owner()),
                shot.parties.firing_house,
                interner,
                los,
            )
        {
            return false;
        }
        let Some(kind) = interner.object_type(building.type_ref(), rules) else {
            return false;
        };
        if kind.gate && building.is_open_gate() {
            return false;
        }
        let mut height = kind.building_type_ext().solid_height;
        if height == 0 {
            return false;
        }
        if height < 0 {
            // BuildingType+0xEF4.
            height = rules.building_launch_height(kind);
        }
        let CellRef::Real(real) = &cell else {
            return false;
        };
        let floor = crate::util::lepton::ground_height_leptons(
            real.level,
            real.slope_type,
            probe.sample.x,
            probe.sample.y,
        )
        .expect("Map578080 requires a supported CellClass slope");
        probe.sample.z <= height.wrapping_shl(8).wrapping_add(floor)
    };
    (cliff() || wall() || building()).then_some(cell)
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
///
/// Ares `FindFirstObstacle @ 1006E2B0` walks the same samples, with the same
/// integer division (1006E411..1006E42B), but gates on `SubjectToBuildings`
/// too and looks the target cell up before the source (1006E2DB..1006E3BB).
pub(crate) fn walk_line_for_blocking_cell<'a>(
    src: (i64, i64, i64),
    tgt: (i64, i64, i64),
    shot: LineOfFireShot,
    rules: &RuleSet,
    cells: &NativeCellQuery<'a>,
    interner: &StringInterner,
    los: &LineOfFireInputs<'_>,
) -> Option<CellRef<'a>> {
    if !shot.subject_to_anything() {
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
    #[cfg(not(feature = "ares"))]
    let (source, target) = {
        let source = lookup((sx, sy));
        (source, lookup((tx, ty)))
    };
    #[cfg(feature = "ares")]
    let (source, target) = {
        let target = lookup((tx, ty));
        (lookup((sx, sy)), target)
    };
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
                shot,
            },
            interner,
            rules,
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
/// Ares `FindFirstImpenetrableObstacle @ 1006E1F0` (InRange's hooks
/// 0x6F7511/0x6F7631, `1004D120`, pass the firer and its target as
/// `parties`) re-admits the same wall and, for any other blocking cell, its
/// building of a house the firer is not allied with whose BuildingTypeExt
/// `SolidLevel=` is at most the projectile's (`1006E848..1006E895`).
///
/// Returns the blocking cell, or `None` when the shot is legal.
pub(crate) fn line_of_fire_blocking_cell<'a>(
    src: (i64, i64, i64),
    tgt: (i64, i64, i64),
    weapon: &WeaponType,
    parties: LineOfFireParties,
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

    let hit = walk_line_for_blocking_cell(
        src,
        tgt,
        LineOfFireShot::of(projectile, parties),
        rules,
        cells,
        interner,
        los,
    )?;

    if matches!(
        crate::sim::cell_kernel::wall_connection_step(
            overlay(&hit, los).0.map_or(-1, i32::from),
            -1,
            los.overlay_registry,
        ),
        crate::sim::cell_kernel::WallConnectionStep::Return(true)
    ) {
        // `WeaponType+0xAC`, the warhead, written at 0x00772992; `Wall=` is
        // `WarheadType+0x144`, key string 0x0081AC58, read at 0x0075D508.
        let warhead_breaks_walls = weapon
            .warhead
            .as_deref()
            .and_then(|name| rules.warhead(name))
            .is_some_and(|wh| wh.wall);
        return (!warhead_breaks_walls).then_some(hit);
    }

    #[cfg(feature = "ares")]
    if let Some(building) = los.buildings.and_then(|buildings| buildings.building(&hit)) {
        // The firer's house is the receiver (1006E860..1006E87D).
        let allied = houses_allied(parties.firing_house, Some(building.owner()), interner, los);
        let building_level = interner
            .object_type(building.type_ref(), rules)
            .map_or(1, |kind| kind.building_type_ext().solid_level);
        if !allied && projectile.map_or(0, |p| p.bullet_type_ext.solid_level) >= building_level {
            return None;
        }
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
                ..Default::default()
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
                    LineOfFireShot {
                        subject_to_cliffs: row["cliffs"] == true,
                        subject_to_walls: row["walls"] == true,
                        ..Default::default()
                    },
                    RuleSet::empty_for_test(),
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

    /// Ares `GetObstacle 1006E540` (`1006E5F5..1006E645`) raises a cell by 4
    /// for the bridge bit 0x100 (`(Flags >> 6) & 4`), where 0x004CC360's
    /// `GetEffectiveHeight @ 0x00487D50` adds 4 for the anchor bit 0x80.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_cliff_counts_the_bridge_bit() {
        let blocks = |flags| {
            let mut terrain = crate::map::resolved_terrain::test_grid(
                8,
                8,
                crate::map::resolved_terrain::test_flat_cell,
            );
            terrain
                .cell_mut_for_test(3, 1)
                .unwrap()
                .bridge_facts
                .raw_flags = flags;
            let query = NativeCellQuery::canonical(&terrain);
            walk_line_for_blocking_cell(
                (384, 384, 0),
                (7 * 256 + 128, 384, 0),
                LineOfFireShot {
                    subject_to_cliffs: true,
                    ..Default::default()
                },
                RuleSet::empty_for_test(),
                &query,
                &StringInterner::new(),
                &LineOfFireInputs::default(),
            )
            .is_some()
        };
        assert!(blocks(0x100));
        assert!(!blocks(0x80));
    }

    /// Ares `GetObstacle 1006E540` and `FindFirstImpenetrableObstacle
    /// 1006E1F0`: a `SubjectToBuildings=` shot stops at an enemy building
    /// whose art `SolidHeight=` reaches the line, unless the building is the
    /// shot's own target or the projectile's `SolidLevel=` reaches the
    /// building's; `AlliedSolidTransparency` lets an ally's building through,
    /// and an ally's building is never pierced.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_solid_building_blocks_the_line_until_pierced() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::intern::{test_intern, test_interner};
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};

        let rules_for = |extra: &str| {
            RuleSet::from_ini_with_fixed_art_for_test(
                &IniFile::from_str(&format!(
                    "[BuildingTypes]\n0=BLDG\n[BLDG]\nStrength=100\n\
                     [VehicleTypes]\n0=TANK\n[TANK]\nPrimary=GUN\nSecondary=PIERCE\n\
                     [GUN]\nDamage=1\nRange=8\nProjectile=SHOT\nWarhead=WH\n\
                     [PIERCE]\nDamage=1\nRange=8\nProjectile=PIERCER\nWarhead=WH\n\
                     [SHOT]\nSubjectToBuildings=yes\n\
                     [PIERCER]\nSubjectToBuildings=yes\nSolidLevel=1\n\
                     [WH]\nVerses=100%\n{extra}"
                )),
                &IniFile::from_str("[BLDG]\nSolidHeight=1\n"),
            )
            .unwrap()
        };
        let terrain = crate::map::resolved_terrain::test_grid(
            8,
            8,
            crate::map::resolved_terrain::test_flat_cell,
        );
        let cells = NativeCellQuery::canonical(&terrain);
        let mut entities = crate::sim::entity_store::EntityStore::new();
        entities.insert(GameEntity::test_default_of_category(
            7,
            "BLDG",
            "Owner",
            4,
            1,
            EntityCategory::Structure,
        ));
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            4,
            1,
            7,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let firer = test_intern("Firer");
        let interner = test_interner();
        let allied = HouseAlliances::new(crate::map::houses::HouseAllianceMap::from([
            ("OWNER".into(), ["FIRER".into()].into()),
            ("FIRER".into(), ["OWNER".into()].into()),
        ]));
        let enemies = HouseAlliances::new(Default::default());
        let blocks = |rules: &RuleSet, weapon: &str, target: Option<u64>, alliances| {
            line_of_fire_blocking_cell(
                (256 + 128, 256 + 128, 0),
                (7 * 256 + 128, 256 + 128, 0),
                rules.weapon(weapon).unwrap(),
                LineOfFireParties {
                    firing_house: Some(firer),
                    source_object: Some(1),
                    target_object: target,
                },
                rules,
                &cells,
                &interner,
                &LineOfFireInputs {
                    alliances: Some(alliances),
                    buildings: Some(CellBuildings {
                        occupancy: &occupancy,
                        entities: &entities,
                    }),
                    ..Default::default()
                },
            )
            .map(|cell| match cell {
                CellRef::Real(cell) => (cell.rx, cell.ry),
                CellRef::Dummy { .. } => panic!("the building cell is real"),
            })
        };
        let rules = rules_for("");
        assert_eq!(blocks(&rules, "GUN", None, &enemies), Some((4, 1)));
        assert_eq!(
            blocks(&rules, "GUN", Some(7), &enemies),
            None,
            "its own target"
        );
        assert_eq!(
            blocks(&rules, "PIERCE", None, &enemies),
            None,
            "SolidLevel 1 >= 1"
        );
        assert_eq!(
            blocks(&rules, "PIERCE", None, &allied),
            Some((4, 1)),
            "no ally pierced"
        );
        let transparent = rules_for("[CombatDamage]\nAlliedSolidTransparency=yes\n");
        assert_eq!(blocks(&transparent, "GUN", None, &allied), None);
        assert_eq!(blocks(&transparent, "GUN", None, &enemies), Some((4, 1)));
    }
}
