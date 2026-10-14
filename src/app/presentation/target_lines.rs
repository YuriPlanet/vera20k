//! Target/action and relationship line overlays.
//!
//! Selected action lines are short-lived app-layer feedback resolved from live
//! simulation movement/attack state. Factory rally lines are app-layer visuals
//! over each factory's rally point, its ArchiveTarget.
//!
//! ## Dependency rules
//! - Part of the app layer - reads sim state but never mutates it.

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::render::batch::SpriteInstance;
use crate::rules::house_colors::{HouseColorRamps, NO_REMAP};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;

/// How long selected action lines remain visible after a command is issued.
const DURATION_TICKS: i32 = 25;

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScreenPoint {
    x: f32,
    y: f32,
}

impl From<(f32, f32)> for ScreenPoint {
    fn from((x, y): (f32, f32)) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedLineKind {
    Move,
    Attack,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SelectedActionLine {
    start: ScreenPoint,
    end: ScreenPoint,
    kind: SelectedLineKind,
}

#[derive(Debug, Clone, Copy)]
struct ProjectedActionLine {
    start: [i32; 2],
    end: [i32; 2],
    kind: SelectedLineKind,
}

/// Global selected action line state stored on `AppState`.
#[derive(Debug, Clone)]
pub(crate) struct TargetLineState {
    timer: CdTimer,
    unit_action_lines_enabled: bool,
}

impl Default for TargetLineState {
    fn default() -> Self {
        Self {
            timer: CdTimer::default(),
            unit_action_lines_enabled: true,
        }
    }
}

impl TargetLineState {
    /// Foot4DC089..4DC0AD uses signed wrapping elapsed and a positive
    /// remainder, including the native paused sentinel. It is not !expired.
    pub(crate) fn is_selected_action_active(&self, binary_frame: u32) -> bool {
        self.unit_action_lines_enabled && self.remaining_frames(binary_frame) > 0
    }

    /// Countdown observation for sealed input/capture receipts, independent
    /// of the option gate. Reading it cannot advance or restart the timer.
    pub(crate) fn remaining_frames(&self, binary_frame: u32) -> i32 {
        self.timer.remaining(binary_frame as i32)
    }

    pub(crate) fn set_unit_action_lines_enabled(&mut self, enabled: bool) {
        self.unit_action_lines_enabled = enabled;
    }

    #[cfg(test)]
    pub(crate) fn unit_action_lines_enabled(&self) -> bool {
        self.unit_action_lines_enabled
    }

    /// Open the action-line window.
    ///
    /// The original's start-timer helper has four unconditional callers and
    /// three of them are selection events — band-box release, click-select and
    /// control-group recall — with order dispatch the fourth. So selecting a
    /// group flashes what it is already doing; the lines are not an order-only
    /// cue.
    pub(crate) fn start_timer(&mut self, binary_frame: u32) {
        // Original70D150. No RNG draw or simulation mutation.
        self.timer.start(binary_frame as i32, DURATION_TICKS);
    }

    /// Successful load685167..6851A1 preserves the remaining countdown at
    /// the restored frame, then reanchors it. It never restarts25 here.
    pub(crate) fn reanchor_after_load(&mut self, binary_frame: u32) {
        let frame = binary_frame as i32;
        self.timer.start(frame, self.timer.remaining(frame));
    }
}

/// Build selected unit action-line instances from live simulation state.
pub(crate) fn build_target_line_instances(
    line_state: &TargetLineState,
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    palette_bytes: Option<&[u8]>,
    viewport: TacticalViewport,
) -> Vec<SpriteInstance> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    if !line_state.is_selected_action_active(sim.session.binary_frame) {
        return Vec::new();
    }
    let Some(palette) =
        palette_bytes.and_then(|bytes| crate::assets::pal_file::Palette::from_bytes(bytes).ok())
    else {
        return Vec::new();
    };
    let colors = [
        action_line_color(&palette, 3),
        action_line_color(&palette, 8),
    ];

    let cells = sim.resolved_terrain.as_ref().map(NativeCellQuery::isolated);
    let mut lines = Vec::new();
    let project = |p: ScreenPoint| {
        [
            p.x as i32 - viewport.camera[0],
            p.y as i32 - viewport.camera[1] + viewport.clip[1],
        ]
    };
    // Tactical6D4750 walks the construction-ordered Techno array, not the
    // CurrentObjects selection vector or the Logic active-object order.
    // EntityStore's monotonic stable IDs retain that represented order.
    for entity in sim.entities().values() {
        let Some(line) = selected_action_line_for_entity(entity, sim, rules, cells.as_ref()) else {
            continue;
        };
        lines.push(ProjectedActionLine {
            start: project(line.start),
            end: project(line.end),
            kind: line.kind,
        });
    }
    build_selected_action_line_instances(&lines, colors, viewport)
}

/// Compose this call's already projected lines in Tactical6D4750 Techno order.
/// Native7049C0 writes a constant opaque color without reading A or Z, so after
/// resolving overlaps its disjoint final spans may be emitted in row order.
/// The pixel grid and dirty ranges are derived only from these lines/clip and
/// are dropped before returning; no presentation cache or simulation state is
/// retained. Both routes pass the same logical pixels through the half-open
/// pixel-center scaling in `push_surface_rect` after resolving opaque stores.
/// Native primitive/overlap evidence: tools/procedural_drawing_oracle/action_lines.
fn build_selected_action_line_instances(
    lines: &[ProjectedActionLine],
    colors: [[f32; 3]; 2],
    viewport: TacticalViewport,
) -> Vec<SpriteInstance> {
    let [left, top, width, height] = viewport.clip;
    let mut instances = Vec::new();
    // Storage/index safety only; geometry clipping remains in the native raster.
    if width <= 0
        || height <= 0
        || left.checked_add(width).is_none()
        || top.checked_add(height).is_none()
    {
        return instances;
    }
    let color_index = |kind| match kind {
        SelectedLineKind::Move => 1_u8,
        SelectedLineKind::Attack => 2_u8,
    };
    let width = width as usize;
    let height = height as usize;
    // Small selections keep the direct route. Checked/fallible allocations
    // preserve that same route if a dense viewport cannot be represented.
    let dense = (lines.len() > 32)
        .then(|| {
            let pixel_count = width.checked_mul(height)?;
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(pixel_count).ok()?;
            pixels.resize(pixel_count, 0_u8); // 0 is untouched, not a black pixel.
            let mut dirty_rows = Vec::new();
            dirty_rows.try_reserve_exact(height).ok()?;
            dirty_rows.resize(height, [width, 0]); // [first, end), empty initially.
            Some((pixels, dirty_rows))
        })
        .flatten();
    let Some((mut pixels, mut dirty_rows)) = dense else {
        for line in lines {
            let tint = colors[usize::from(color_index(line.kind) - 1)];
            emit_selected_action_line(line.start, line.end, viewport.clip, |rect| {
                push_surface_rect(&mut instances, rect, tint, viewport);
            });
        }
        return instances;
    };
    for line in lines {
        let color = color_index(line.kind);
        emit_selected_action_line(line.start, line.end, viewport.clip, |rect| {
            if rect[2] <= 0 || rect[3] <= 0 {
                return;
            }
            // Rectangles are native-clipped. Widened/clamped offsets also
            // keep signed extreme coordinates from escaping this allocation.
            let x0 = (i64::from(rect[0]) - i64::from(left)).clamp(0, width as i64) as usize;
            let x1 = (i64::from(rect[0]) + i64::from(rect[2]) - i64::from(left))
                .clamp(0, width as i64) as usize;
            let y0 = (i64::from(rect[1]) - i64::from(top)).clamp(0, height as i64) as usize;
            let y1 = (i64::from(rect[1]) + i64::from(rect[3]) - i64::from(top))
                .clamp(0, height as i64) as usize;
            if x0 >= x1 || y0 >= y1 {
                return;
            }
            for (y, dirty) in dirty_rows.iter_mut().enumerate().take(y1).skip(y0) {
                let row = y * width;
                pixels[row + x0..row + x1].fill(color);
                dirty[0] = dirty[0].min(x0);
                dirty[1] = dirty[1].max(x1);
            }
        });
    }
    for (y, [first, end]) in dirty_rows.into_iter().enumerate() {
        let row = &pixels[y * width..(y + 1) * width];
        let mut x = first;
        while x < end {
            let color = row[x];
            if color == 0 {
                x += 1;
                continue;
            }
            let start = x;
            x += 1;
            while x < end && row[x] == color {
                x += 1;
            }
            push_surface_rect(
                &mut instances,
                [left + start as i32, top + y as i32, (x - start) as i32, 1],
                colors[usize::from(color - 1)],
                viewport,
            );
        }
    }
    instances
}

/// Integer logical viewport plus VERA's presentation-only zoom. Native rally
/// 6DA9D0 and action7049C0 operate in unzoomed surface pixels. Scaling happens
/// after rasterization, independently of their native clipping/projection.
#[derive(Clone, Copy)]
pub(crate) struct TacticalViewport {
    pub camera: [i32; 2],
    pub clip: [i32; 4],
    pub zoom: f32,
}

/// Tactical6DA9D0: reverse CurrentObjects (the player's selection vector),
/// live local Building, HasRallyPoint455DA0 and ArchiveTarget. The coordinate
/// owners retain foundation offsets and native ground/slope/bridge semantics.
/// No RNG draws, timer writes, detach calls, Z reads or Z writes occur here.
/// Evidence: tools/procedural_drawing_oracle/rally.{py,json,meta.json}.
pub(crate) fn build_factory_rally_line_instances(
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    selected_ids: &[u64],
    house_color_map: &HouseColorMap,
    local_owner: Option<&str>,
    viewport: TacticalViewport,
    mut alpha_at: impl FnMut([i32; 2]) -> u8,
) -> [Vec<SpriteInstance>; 2] {
    use crate::map::resolved_terrain::NativeCellQuery;
    use crate::sim::movement::ground_pose::{
        object_get_coords, query_ground_height, target_get_coords,
    };
    let mut instances = [Vec::new(), Vec::new()];
    let (Some(sim), Some(rules), Some(local_owner)) = (sim, rules, local_owner) else {
        return instances;
    };
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return instances;
    };
    // Presentation must not stamp the simulation's shared fallback CellClass.
    let cells = NativeCellQuery::isolated(terrain);
    let project = |coord: crate::sim::components::DriveCoord| {
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        [
            x as i32 - viewport.camera[0] + viewport.clip[0],
            y as i32 - viewport.camera[1] + viewport.clip[1],
        ]
    };
    for &id in selected_ids.iter().rev() {
        let Some(entity) = sim.entities().get(id) else {
            continue;
        };
        if !entity.selected
            || !entity.is_object_alive()
            || entity.category() != EntityCategory::Structure
        {
            continue;
        }
        let owner = sim.interner.resolve(entity.owner());
        if owner != local_owner {
            continue;
        }
        let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
            continue;
        };
        if !obj.has_rally_line() {
            continue;
        }
        let Some(mut target) = entity
            .archive_target()
            .and_then(|target| target_get_coords(target, sim.entities(), Some(&cells)))
        else {
            continue;
        };
        let Ok(ground) = query_ground_height(&cells, target) else {
            continue;
        };
        target.z = ground.wrapping_add(
            if cells.flags(cells.lookup_world(target.x, target.y)) & 0x100 != 0 {
                crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
            } else {
                0
            },
        );
        let start = project(object_get_coords(entity, Some(terrain)));
        let end = project(target);
        let tint = rally_tint_for_owner(owner, house_color_map, &rules.house_color_ramps);
        emit_rally_line(
            &mut instances,
            start,
            end,
            tint,
            sim.session.binary_frame,
            viewport,
            &mut alpha_at,
        );
    }
    instances
}

fn selected_action_line_for_entity(
    entity: &GameEntity,
    sim: &Simulation,
    rules: Option<&RuleSet>,
    cells: Option<&NativeCellQuery<'_>>,
) -> Option<SelectedActionLine> {
    if !entity.selected
        || entity.category() == EntityCategory::Structure
        || !sim.house_is_human_player(entity.owner())
    {
        return None;
    }
    let project = |x, y, z| crate::util::lepton::absolute_leptons_to_screen(x, y, z).into();
    if let Some(attack) = &entity.attack_target {
        let rules = rules?;
        let start = crate::sim::combat::fire_coord::turret_pivot_coordinate(sim, rules, entity)?;
        let end = crate::sim::combat::aim_coord::led_target_coordinate(
            sim,
            rules,
            entity.stable_id(),
            Some(attack.target),
        );
        return Some(SelectedActionLine {
            // Foot4DC0C6/4DC0DF: +300 pivot and70BCB0's own TarCom+58.
            // FireAt shares the latter query; no presentation copy of lead.
            start: project(start.x, start.y, start.z),
            end: project(end.x, end.y, end.z),
            kind: SelectedLineKind::Attack,
        });
    }

    let _nav_com = entity.navigation.nav_com?;
    let nav_target = entity
        .navigation
        .nav_queue
        .last()
        .copied()
        .or(entity.navigation.nav_com)?;
    use crate::sim::movement::ground_pose::{
        object_location, query_ground_height, target_get_coords,
    };
    let terrain = sim.resolved_terrain.as_ref();
    let target = match nav_target {
        NavTargetRef::Cell { rx, ry } => TargetKind::Cell(rx, ry),
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => TargetKind::Entity(id),
    };
    let mut end = target_get_coords(target, sim.entities(), cells)?;
    let end_cell = (
        crate::util::lepton::lepton_to_cell_packed(end.x),
        crate::util::lepton::lepton_to_cell_packed(end.y),
    );
    // Foot4DC205..4DC27A: retain target+48 Z except for an in-Size cell
    // with flag100, which replaces it with ground578080 + Foot's deck height.
    if sim.map_cell_in_bounds(end_cell)
        && let Some(cells) = cells
        && cells.flags(cells.lookup_world(end.x, end.y)) & 0x100 != 0
    {
        end.z = query_ground_height(cells, end)
            .ok()?
            .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    let start = object_location(entity, terrain);
    Some(SelectedActionLine {
        start: project(start.x, start.y, start.z),
        end: project(end.x, end.y, end.z),
        kind: SelectedLineKind::Move,
    })
}

fn rally_tint_for_owner(
    owner: &str,
    house_color_map: &HouseColorMap,
    ramps: &HouseColorRamps,
) -> [f32; 3] {
    // Unknown owner → NO_REMAP, which ramp() resolves to the default scheme
    // (matching the producers' DEFAULT_SCHEME_ENTRY fallback), not entry 0.
    let index = house_color_map.get(owner).copied().unwrap_or(NO_REMAP);
    crate::render::palette_light::house_color_rgb(ramps, index)
        .map(|channel| f32::from(channel) / 255.0)
}

/// Palette87F6C4's N53 middle row, read by4DC280 (Move index3) and
/// 4DC0FA (attack index8), unpacked and repacked by7049C0. This is raw
/// surface color; the action line does not sample A or use cell lighting.
fn action_line_color(palette: &crate::assets::pal_file::Palette, index: u8) -> [f32; 3] {
    use crate::render::native_surface_format::{ACTIVE_RETAIL_RGB565_PRESENTATION, RGB565};
    let color = palette.colors[usize::from(index)];
    let word = crate::render::palette_light::PaletteLight::plain(53, 1000).rgb565(
        [color.r, color.g, color.b],
        index,
        127,
    );
    let [r, g, b] = RGB565.unpack_rgb8(word);
    let rgba = ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([r, g, b, 255]);
    [rgba[0], rgba[1], rgba[2]].map(|v| f32::from(v) / 255.)
}

/// Ordinary7049C0(source,target,color,0,0): two clipped3x3 fills followed
/// by the solid line. The native box origin is point-2 on both axes. Its
/// bottom-right pixel is the endpoint, not the centre of a symmetric box.
/// No A/Z reads or writes, RNG draws, timer mutations or detach calls.
fn emit_selected_action_line(
    mut start: [i32; 2],
    mut end: [i32; 2],
    clip: [i32; 4],
    mut emit: impl FnMut([i32; 4]),
) {
    use crate::render::surface_line::{clip_line, solid_line};
    for point in [start, end] {
        let rect = crate::util::rect::clip_rect(
            [point[0].wrapping_sub(2), point[1].wrapping_sub(2), 3, 3],
            clip,
        );
        emit(rect);
    }
    if clip_line(&mut start, &mut end, clip) {
        solid_line(start, end, emit);
    }
}

/// Original static pattern842930. The producer phase has period15 even though
/// the leaf repeats16 entries; signed subtraction/remainder survive frame wrap.
const RALLY_PATTERN: [u8; 16] = [1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0];

fn emit_rally_line(
    instances: &mut [Vec<SpriteInstance>; 2],
    mut from: [i32; 2],
    mut to: [i32; 2],
    tint: [f32; 3],
    frame: u32,
    viewport: TacticalViewport,
    mut alpha_at: impl FnMut([i32; 2]) -> u8,
) {
    use crate::render::surface_line::{clip_line, patterned_line};
    let phase = i32::MAX.wrapping_sub(frame as i32) % 15;
    let rgb = tint.map(|v| (v * 255.).round() as u8);
    let rgba = crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
        .quantize_rgba8([rgb[0], rgb[1], rgb[2], 255]);
    let tint = [rgba[0], rgba[1], rgba[2]].map(|v| f32::from(v) / 255.);
    from[1] += 2;
    to[1] += 2;
    for color in [[0.; 3], tint, tint] {
        // The original mutates and reuses these clipped points, including when
        // one row misses the clip. Independent offset/clip calls differ at edges.
        if clip_line(&mut from, &mut to, viewport.clip) {
            patterned_line(from, to, &RALLY_PATTERN, phase, |point| {
                let pass = usize::from(alpha_at(point) == 0);
                push_surface_rect(
                    &mut instances[pass],
                    [point[0], point[1], 1, 1],
                    color,
                    viewport,
                );
            });
        }
        from[1] -= 1;
        to[1] -= 1;
    }
}

/// Nearest sampling of the native logical pixel grid. Use the existing UI
/// passthrough pipeline so zoom never pads adjacent 1px quads or writes Z.
/// Adjacent stores of the same row/color become one horizontal span.
/// WGSL fragment positions sample pixel centers at integer coordinates + 0.5:
/// https://www.w3.org/TR/WGSL/#builtin-values-position
/// Thus each half-open scaled edge maps to ceil(edge * zoom - 0.5). Resolving
/// opaque overlaps in logical pixels before this mapping preserves coverage.
fn push_surface_rect(
    instances: &mut Vec<SpriteInstance>,
    [left, top, width, height]: [i32; 4],
    tint: [f32; 3],
    view: TacticalViewport,
) {
    let edge = |v: i32| (v as f32 * view.zoom - 0.5).ceil();
    let x = edge(left);
    let y = edge(top);
    let size = [edge(left + width) - x, edge(top + height) - y];
    if size[0] <= 0. || size[1] <= 0. {
        return;
    }
    let position = [x + view.camera[0] as f32, y + view.camera[1] as f32];
    if let Some(last) = instances.last_mut()
        && last.tint == tint
        && last.position[1] == position[1]
        && last.size[1] == size[1]
        && last.position[0] + last.size[0] == position[0]
    {
        last.size[0] += size[0];
        return;
    }
    instances.push(SpriteInstance {
        position,
        size,
        uv_size: [1.; 2],
        tint,
        alpha: 1.,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::rally_tests::{alpha_fixture, assert_native_passes, rally_native};
    use super::*;
    use crate::rules::house_colors::HouseColorIndex;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::AttackTarget;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn rally_pattern_matches_original_producer_pixels() {
        let row = &rally_native()["producer_cases"][0];
        let view = TacticalViewport {
            camera: [0, 0],
            clip: [0, 0, 160, 120],
            zoom: 1.,
        };
        let mut actual = [Vec::new(), Vec::new()];
        emit_rally_line(
            &mut actual,
            [20, 25],
            [140, 85],
            [248. / 255., 40. / 255., 8. / 255.],
            0,
            view,
            |p| alpha_fixture(p, "mixed"),
        );
        assert_native_passes(&actual, row, view);
    }

    fn active_line_state_for_tick(frame: u32) -> TargetLineState {
        TargetLineState {
            timer: CdTimer::started(frame as i32, DURATION_TICKS),
            unit_action_lines_enabled: true,
        }
    }

    fn rules_with_factory_and_non_factory() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GAWEAP\n\
             1=GAPOWR\n\
             [GAWEAP]\nFactory=UnitType\nStrength=1000\n\
             [GAPOWR]\nStrength=750\n",
        );
        RuleSet::from_ini(&ini).expect("rules")
    }

    fn sim_with_selected_unit_that_has_attack_and_move() -> Simulation {
        let mut sim = Simulation::new();
        let mut unit = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        unit.set_owner_for_test(sim.interner.intern("Americans"));
        unit.set_type_ref_for_test(sim.interner.intern("MTNK"));
        unit.selected = true;
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(unit.owner());
        unit.navigation.nav_com = Some(NavTargetRef::cell(25, 25));
        unit.attack_target = Some(AttackTarget::new(2));
        let mut target = GameEntity::test_default(2, "HTNK", "Soviet", 14, 10);
        target.set_owner_for_test(sim.interner.intern("Soviet"));
        target.set_type_ref_for_test(sim.interner.intern("HTNK"));
        sim.entities_mut().insert(unit);
        sim.entities_mut().insert(target);
        sim
    }

    fn sim_with_selected_factory_and_non_factory() -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let factory_type = sim.interner.intern("GAWEAP");
        let power_type = sim.interner.intern("GAPOWR");
        let mut factory = GameEntity::new_at_frame_zero_for_test(
            10,
            10,
            10,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(1000),
            factory_type,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        factory.selected = true;
        factory.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(16, 10)));
        let mut power = factory.clone();
        power.set_stable_id_for_test(11);
        power.set_type_ref_for_test(power_type);
        power.position.rx = 12;
        power.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(16, 10)));
        sim.entities_mut().insert(factory);
        sim.entities_mut().insert(power);
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(
            40,
            40,
            (0..40)
                .flat_map(|y| (0..40).map(move |x| ResolvedTerrainCell::clear_for_test(x, y)))
                .collect(),
        ));
        sim
    }

    fn test_viewport() -> TacticalViewport {
        TacticalViewport {
            camera: [-320, 0],
            clip: [0, 0, 1000, 1000],
            zoom: 1.,
        }
    }

    fn test_house_colors() -> HouseColorMap {
        let mut colors = HouseColorMap::new();
        colors.insert("Americans".to_string(), HouseColorIndex(1));
        colors
    }

    #[test]
    fn selected_action_timer_expires_at_25_ticks() {
        let state = active_line_state_for_tick(100);
        assert!(state.is_selected_action_active(124));
        assert!(!state.is_selected_action_active(125));
    }

    #[test]
    fn unit_action_lines_option_disables_selected_timer() {
        let mut state = active_line_state_for_tick(100);
        state.set_unit_action_lines_enabled(false);
        assert!(!state.is_selected_action_active(101));
    }

    /// Selection alone opens the window — no command required.
    #[test]
    fn start_timer_opens_the_window_without_a_command() {
        let mut state = TargetLineState::default();
        assert!(!state.is_selected_action_active(100));
        state.start_timer(100);
        assert!(state.is_selected_action_active(100));
        assert!(state.is_selected_action_active(124));
        assert!(!state.is_selected_action_active(125));
    }

    #[test]
    fn selected_action_attack_target_wins_over_movement() {
        let sim = sim_with_selected_unit_that_has_attack_and_move();
        let actor = sim.entities().get(1).unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\n[HTNK]\nStrength=400\n",
        ))
        .unwrap();
        let line = selected_action_line_for_entity(actor, &sim, Some(&rules), None).unwrap();
        assert_eq!(line.kind, SelectedLineKind::Attack);
    }

    #[test]
    fn factory_rally_builder_emits_only_selected_local_eligible_structures() {
        let sim = sim_with_selected_factory_and_non_factory();
        let rules = rules_with_factory_and_non_factory();
        let lines = build_factory_rally_line_instances(
            Some(&sim),
            Some(&rules),
            &[10, 11],
            &test_house_colors(),
            Some("Americans"),
            test_viewport(),
            |_| 127,
        );
        assert!(!lines[0].is_empty());
    }

    #[test]
    fn disabling_unit_action_lines_does_not_disable_rally_lines() {
        let mut state = active_line_state_for_tick(0);
        state.set_unit_action_lines_enabled(false);
        let sim = sim_with_selected_factory_and_non_factory();
        let rules = rules_with_factory_and_non_factory();

        assert!(
            build_target_line_instances(&state, Some(&sim), None, None, test_viewport()).is_empty()
        );
        assert!(
            !build_factory_rally_line_instances(
                Some(&sim),
                Some(&rules),
                &[10, 11],
                &test_house_colors(),
                Some("Americans"),
                test_viewport(),
                |_| 127,
            )[0]
            .is_empty()
        );
    }

    #[test]
    fn line_builders_skip_when_sim_or_rules_missing() {
        assert!(
            build_target_line_instances(
                &TargetLineState::default(),
                None,
                None,
                None,
                test_viewport()
            )
            .is_empty()
        );
        assert!(
            build_factory_rally_line_instances(
                None,
                None,
                &[],
                &HouseColorMap::new(),
                Some("Americans"),
                test_viewport(),
                |_| 127,
            )[0]
            .is_empty()
        );
    }
}

#[cfg(test)]
#[path = "rally_line_tests.rs"]
mod rally_tests;

#[cfg(test)]
#[path = "action_line_tests.rs"]
mod action_tests;
