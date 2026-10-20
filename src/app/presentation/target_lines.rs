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
///
/// `alpha_at` samples the shroud A-buffer at a surface point, which the
/// psychic detection line's dashed leaf reads.
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn build_target_line_instances(
    line_state: &TargetLineState,
    planning_active: bool,
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    palette_bytes: Option<&[u8]>,
    house_colors: &HouseColorMap,
    wall_ms: u32,
    viewport: TacticalViewport,
    after_entity: Option<&mut dyn FnMut(u64, &mut Vec<SpriteInstance>)>,
    alpha_at: &mut dyn FnMut([i32; 2]) -> u8,
) -> Vec<SpriteInstance> {
    build_target_line_instances_sampled(
        line_state,
        planning_active,
        sim,
        rules,
        palette_bytes,
        house_colors,
        wall_ms,
        viewport,
        after_entity,
        alpha_at,
        None,
    )
}

pub(crate) fn build_target_line_instances_sampled(
    line_state: &TargetLineState,
    planning_active: bool,
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    palette_bytes: Option<&[u8]>,
    house_colors: &HouseColorMap,
    wall_ms: u32,
    viewport: TacticalViewport,
    mut after_entity: Option<&mut dyn FnMut(u64, &mut Vec<SpriteInstance>)>,
    alpha_at: &mut dyn FnMut([i32; 2]) -> u8,
    interpolation: Option<&super::interpolation::RenderInterpolation>,
) -> Vec<SpriteInstance> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    // Tactical6D4764..6D478E: a Techno the local player does not control
    // draws its psychic detection line before the planning gate. Skip the
    // per-Techno 43B150 walk when the player has no operational detector.
    // Collect the player's detectors once for the frame's Techno walk.
    let psychic = rules.and_then(|rules| {
        let viewer = sim.session().current_house()?;
        let sites: Vec<_> = sim.psychic_detection_sites(viewer, rules).collect();
        (!sites.is_empty()).then_some((rules, viewer, sites))
    });
    let cells = sim.resolved_terrain().map(NativeCellQuery::isolated);
    let project = |p: ScreenPoint| {
        [
            p.x as i32 - viewport.camera[0],
            p.y as i32 - viewport.camera[1] + viewport.clip[1],
        ]
    };
    let mut psychic_line_for = |entity: &GameEntity, instances: &mut Vec<SpriteInstance>| {
        let Some((rules, viewer, sites)) = psychic.as_ref() else {
            return;
        };
        if sim.house_is_human_player(entity.owner())
            || !matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
            || entity.lifecycle.in_limbo()
            || !sim.techno_intent_psychically_detected(
                entity.stable_id(),
                *viewer,
                sites,
                cells.as_ref(),
            )
        {
            return;
        }
        let Some(line) = action_line_endpoints(entity, sim, Some(*rules), cells.as_ref()) else {
            return;
        };
        let line = sample_action_line(line, entity, sim, interpolation);
        let owner = sim.interner().resolve(entity.owner());
        let color = house_colors.get(owner).copied().unwrap_or(NO_REMAP);
        let rgb = crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, color);
        emit_psychic_detection_line(
            project(line.start),
            project(line.end),
            rgb,
            wall_ms,
            viewport.clip,
            &mut *alpha_at,
            |rect, word| push_surface_word(instances, rect, word, viewport),
        );
    };
    // Tactical6D4729 and6D4793 use the same planning gate; the action-line
    //option/timer gates only Foot4DC060, not CaptureManager relationships.
    if planning_active {
        let mut instances = Vec::new();
        if psychic.is_some() {
            for entity in sim.entities().values() {
                psychic_line_for(entity, &mut instances);
            }
        }
        return instances;
    }
    let colors = line_state
        .is_selected_action_active(sim.session().binary_frame())
        .then(|| {
            palette_bytes.and_then(|bytes| crate::assets::pal_file::Palette::from_bytes(bytes).ok())
        })
        .flatten()
        .map(|palette| {
            [
                action_line_color(&palette, 3),
                action_line_color(&palette, 8),
            ]
        });
    let mut control = rules
        .map(|rules| {
            super::control_links::prepare(
                sim,
                rules,
                house_colors,
                wall_ms,
                viewport,
                interpolation,
            )
        })
        .unwrap_or_default();
    if colors.is_none() && control.is_empty() && after_entity.is_none() && psychic.is_none() {
        return Vec::new();
    }
    let line_for = |entity: &GameEntity| {
        selected_action_line_for_entity(entity, sim, rules, cells.as_ref()).map(|line| {
            let line = sample_action_line(line, entity, sim, interpolation);
            ProjectedActionLine {
                start: project(line.start),
                end: project(line.end),
                kind: line.kind,
            }
        })
    };
    // Tactical6D4750 walks the construction-ordered Techno array, not the
    // CurrentObjects selection vector or the Logic active-object order.
    // EntityStore's monotonic stable IDs retain that represented order.
    if !control.is_empty() || after_entity.is_some() || psychic.is_some() {
        //6D4750/6D478E ->6D47BF/6D47F6 ->6D47FB within EACH Techno visit.
        //Resolving all action lines before relationships would reverse
        //cross-object overlaps.
        let mut instances = Vec::new();
        for entity in sim.entities().values() {
            if let Some(colors) = colors
                && let Some(line) = line_for(entity)
            {
                instances.extend(build_selected_action_line_instances(
                    &[line],
                    colors,
                    viewport,
                ));
            }
            psychic_line_for(entity, &mut instances);
            if let Some(lines) = control.remove(&entity.stable_id()) {
                instances.extend(lines);
            }
            if let Some(callback) = after_entity.as_mut() {
                callback(entity.stable_id(), &mut instances);
            }
        }
        return instances;
    }
    let Some(colors) = colors else {
        return Vec::new();
    };
    let lines = sim
        .entities()
        .values()
        .filter_map(line_for)
        .collect::<Vec<_>>();
    build_selected_action_line_instances(&lines, colors, viewport)
}

fn sample_action_line(
    mut line: SelectedActionLine,
    entity: &GameEntity,
    sim: &Simulation,
    interpolation: Option<&super::interpolation::RenderInterpolation>,
) -> SelectedActionLine {
    let Some(cache) = interpolation else {
        return line;
    };
    let offset = cache.entity_offset(entity, sim.session().binary_frame());
    line.start.x += offset[0];
    line.start.y += offset[1];
    let target = match line.kind {
        SelectedLineKind::Attack => entity.attack_target.target(),
        SelectedLineKind::Move => entity
            .navigation
            .nav_queue()
            .last()
            .copied()
            .or(entity.navigation.nav_com())
            .map(TargetKind::from),
    };
    if let Some(TargetKind::Entity(id)) = target
        && let Some(target) = sim.entities().get(id)
    {
        let offset = cache.entity_offset(target, sim.session().binary_frame());
        line.end.x += offset[0];
        line.end.y += offset[1];
    }
    line
}

/// Foot4DC340, an enemy Foot's psychic detection line: two clipped 3x3 boxes
/// at point-2 in the owner's colour (House+56F9), then, when 7BC2B0 clips the
/// line in, DSurface4C0750's dashed walk over pattern822540 (the rally
/// pattern) at phase `(-time >> 5) & 15`, storing only where the A-buffer is
/// nonzero. The colour pulses toward white (6612C0 by `time & 0x3FF`, the
/// low byte mirrored while bit 8 is set) except while bit 9 is set. `time` is
/// timeGetTime: wall-clock milliseconds. No RNG, timer or simulation write.
fn emit_psychic_detection_line(
    start: [i32; 2],
    end: [i32; 2],
    rgb: [u8; 3],
    time: u32,
    clip: [i32; 4],
    alpha_at: &mut dyn FnMut([i32; 2]) -> u8,
    mut draw: impl FnMut([i32; 4], u16),
) {
    use crate::render::native_surface_format::{RGB565, interpolate_rgb8};
    use crate::render::surface_line::{clip_line, patterned_line};
    let base = RGB565.pack_rgb8(rgb);
    endpoint_boxes([start, end], clip, |rect| draw(rect, base));
    let (mut start, mut end) = (start, end);
    if !clip_line(&mut start, &mut end, clip) {
        return;
    }
    let mut color = rgb;
    if time & 0x200 == 0 {
        let mut amount = time & 0x3FF;
        if time & 0x100 != 0 {
            amount ^= 0xFF;
        }
        color = interpolate_rgb8(rgb, [255; 3], amount & 0xFF);
    }
    let word = RGB565.pack_rgb8(color);
    let phase = ((time.wrapping_neg() as i32) >> 5) & 0xF;
    patterned_line(start, end, &RALLY_PATTERN, phase, |point| {
        if alpha_at(point) != 0 {
            draw([point[0], point[1], 1, 1], word);
        }
    });
}

/// One native RGB565 store as presented, through the shared surface rect.
pub(crate) fn push_surface_word(
    instances: &mut Vec<SpriteInstance>,
    rect: [i32; 4],
    word: u16,
    view: TacticalViewport,
) {
    let rgb = crate::render::native_surface_format::RGB565.unpack_rgb8(word);
    let rgba = crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
        .quantize_rgba8([rgb[0], rgb[1], rgb[2], 255]);
    push_surface_rect(
        instances,
        rect,
        [rgba[0], rgba[1], rgba[2]].map(|v| f32::from(v) / 255.),
        view,
    );
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
#[cfg(test)]
pub(crate) fn build_factory_rally_line_instances(
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    selected_ids: &[u64],
    house_color_map: &HouseColorMap,
    local_owner: Option<&str>,
    viewport: TacticalViewport,
    mut alpha_at: impl FnMut([i32; 2]) -> u8,
) -> [Vec<SpriteInstance>; 2] {
    build_factory_rally_line_instances_sampled(
        sim,
        rules,
        selected_ids,
        house_color_map,
        local_owner,
        viewport,
        &mut alpha_at,
        None,
    )
}

pub(crate) fn build_factory_rally_line_instances_sampled(
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    selected_ids: &[u64],
    house_color_map: &HouseColorMap,
    local_owner: Option<&str>,
    viewport: TacticalViewport,
    mut alpha_at: impl FnMut([i32; 2]) -> u8,
    interpolation: Option<&super::interpolation::RenderInterpolation>,
) -> [Vec<SpriteInstance>; 2] {
    use crate::map::resolved_terrain::NativeCellQuery;
    use crate::sim::movement::ground_pose::{
        object_get_coords, query_ground_height, target_get_coords,
    };
    let mut instances = [Vec::new(), Vec::new()];
    let (Some(sim), Some(rules), Some(local_owner)) = (sim, rules, local_owner) else {
        return instances;
    };
    let Some(terrain) = sim.resolved_terrain() else {
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
        if !entity.selection.is_selected()
            || !entity.is_object_alive()
            || entity.category() != EntityCategory::Structure
        {
            continue;
        }
        let owner = sim.interner().resolve(entity.owner());
        if owner != local_owner {
            continue;
        }
        let Some(obj) = sim.interner().object_type(entity.type_ref(), rules) else {
            continue;
        };
        if !obj.has_rally_line() {
            continue;
        }
        let Some(mut target) = entity
            .archive_target()
            .and_then(|target| target_get_coords(target, sim, Some(&cells)))
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
        if let Some(cache) = interpolation
            && let Some(TargetKind::Entity(id)) = entity.archive_target()
            && let Some(entity) = sim.entities().get(id)
        {
            let sampled = cache.entity_coord(
                entity,
                sim.session().binary_frame(),
                [target.x, target.y, target.z],
            );
            target.x = sampled[0];
            target.y = sampled[1];
        }
        let start = project(object_get_coords(entity, Some(terrain)));
        let end = project(target);
        let tint = rally_tint_for_owner(owner, house_color_map, &rules.house_color_ramps);
        emit_rally_line(
            &mut instances,
            start,
            end,
            tint,
            sim.session().binary_frame(),
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
    if !entity.selection.is_selected()
        || entity.category() == EntityCategory::Structure
        || !sim.house_is_human_player(entity.owner())
    {
        return None;
    }
    action_line_endpoints(entity, sim, rules, cells)
}

/// The endpoints Foot4DC060 and Foot4DC340 share: with a Target, the +300
/// pivot to 70BCB0's lead coordinate; else, with a NavCom, the Location to
/// the last queued waypoint or the NavCom.
fn action_line_endpoints(
    entity: &GameEntity,
    sim: &Simulation,
    rules: Option<&RuleSet>,
    cells: Option<&NativeCellQuery<'_>>,
) -> Option<SelectedActionLine> {
    let project = |x, y, z| crate::util::lepton::absolute_leptons_to_screen(x, y, z).into();
    if entity.attack_target.target().is_some() {
        let [start, end] = attack_line_points(entity, sim, rules?)?;
        return Some(SelectedActionLine {
            start,
            end,
            kind: SelectedLineKind::Attack,
        });
    }

    let _nav_com = entity.navigation.nav_com()?;
    let nav_target = entity
        .navigation
        .nav_queue()
        .last()
        .copied()
        .or(entity.navigation.nav_com())?;
    use crate::sim::movement::ground_pose::{
        object_location, query_ground_height, target_get_coords,
    };
    let terrain = sim.resolved_terrain();
    let mut end = target_get_coords(TargetKind::from(nav_target), sim, cells)?;
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

/// With a Target (+2B4), the +300 pivot (argument 0) to 70BCB0's lead
/// coordinate, projected: Foot4DC0C6/4DC0DF and the target laser's
/// Techno6FF983/6FF9A4. FireAt shares the lead query; no presentation copy.
fn attack_line_points(
    entity: &GameEntity,
    sim: &Simulation,
    rules: &RuleSet,
) -> Option<[ScreenPoint; 2]> {
    let attack = entity.attack_target.target()?;
    let start = crate::sim::combat::fire_coord::turret_pivot_coordinate(sim, rules, entity)?;
    let end = crate::sim::combat::aim_coord::led_target_coordinate(
        sim,
        rules,
        entity.stable_id(),
        Some(attack),
    );
    Some(
        [start, end].map(|p| crate::util::lepton::absolute_leptons_to_screen(p.x, p.y, p.z).into()),
    )
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
    let [r, g, b] = RGB565.unpack_rgb8(action_line_word(palette, index));
    let rgba = ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([r, g, b, 255]);
    [rgba[0], rgba[1], rgba[2]].map(|v| f32::from(v) / 255.)
}

/// The packed surface word of Palette87F6C4's entry `index` (its 16-bit
/// table at +174, `+0x10` for index 8).
fn action_line_word(palette: &crate::assets::pal_file::Palette, index: u8) -> u16 {
    let color = palette.color(index);
    crate::render::palette_light::PaletteLight::plain(53, 1000).rgb565(
        [color.r, color.g, color.b],
        index,
        127,
    )
}

/// The target laser's dash, static pattern 84310C.
const TARGET_LASER_PATTERN: [u8; 16] = [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0];

/// `UnitClass::Draw_It`'s last call (`0x0073D415..0x0073D43A`): while the
/// firer's +3BC timer runs, Techno6FF960 draws its target laser in the Unit's
/// own Display slot, after its body. A human player's successful FireAt
/// starts that 15-frame timer (`0x006FF48D..0x006FF4C9`) on the frame and
/// path that store +120 (`0x006FF743`), so it runs while `frame - LastFireFrame
/// < 15`. Only Units draw it; any class may start it. 6FF960 needs a Target
/// (+2B4). The frame is A8ED84 at the draw, `sim`'s committed frame: Main_Tick
/// renders (`0x0055DBBE`) before its logic (`0x0055DC9E`) and increment
/// (`0x0055DE81`), so the first draw after a shot sees elapsed 1 and the
/// laser shows for 14 frames.
///
/// Residual: IsHumanPlayer is read now, not at the shot, so an owner change
/// within the 15 frames after a shot shows or hides the rest of that laser.
pub(crate) fn target_laser_draws<'a>(
    entity: &GameEntity,
    sim: &Simulation,
    rules: &RuleSet,
    palette: impl FnOnce() -> Option<&'a crate::assets::pal_file::Palette>,
    view: crate::render::surface_line::SurfaceLineViewport,
) -> Vec<crate::render::surface_line::SurfaceDraw> {
    let frame = sim.session().binary_frame();
    target_laser_draws_at(entity, sim, rules, palette, view, frame)
}

/// [`target_laser_draws`] at native frame `frame`.
fn target_laser_draws_at<'a>(
    entity: &GameEntity,
    sim: &Simulation,
    rules: &RuleSet,
    palette: impl FnOnce() -> Option<&'a crate::assets::pal_file::Palette>,
    view: crate::render::surface_line::SurfaceLineViewport,
    frame: u32,
) -> Vec<crate::render::surface_line::SurfaceDraw> {
    use crate::render::surface_line::SurfaceDraw;
    let mut draws = Vec::new();
    if entity.category() != EntityCategory::Unit
        || !rules
            .object(sim.interner().resolve(entity.type_ref()))
            .is_some_and(|object| object.target_laser)
        || (frame as i32).wrapping_sub(entity.last_fire_frame.frame() as i32) >= 15
        || !sim.house_is_human_player(entity.owner())
    {
        return draws;
    }
    let (Some(points), Some(palette)) = (attack_line_points(entity, sim, rules), palette()) else {
        return draws;
    };
    // 6D2140 then +886FA4 on y, as Foot4DC060 projects its endpoints.
    let [start, end] = points.map(|p| {
        [
            p.x as i32 - view.camera[0],
            p.y as i32 - view.camera[1] + view.clip[1],
        ]
    });
    emit_target_laser(
        start,
        end,
        action_line_word(palette, 8),
        frame,
        view.clip,
        |rect, word| draws.push(SurfaceDraw::OpaqueRect { rect, word }),
    );
    draws
}

/// Techno6FF960 after its projection: the clipped 3x3 FillRect (vt+14) boxes
/// at both endpoints, then, when 7BC2B0 clips the line in, XSurface7BA8C0's
/// dashed walk (DSurface vt+48) over pattern 84310C at phase
/// `(frame * 7) % 16`, all in Palette87F6C4 index 8. Opaque stores with no
/// A or Z; no RNG draw, timer write or detach call.
fn emit_target_laser(
    start: [i32; 2],
    end: [i32; 2],
    word: u16,
    frame: u32,
    clip: [i32; 4],
    mut draw: impl FnMut([i32; 4], u16),
) {
    use crate::render::surface_line::{clip_line, patterned_line};
    endpoint_boxes([start, end], clip, |rect| draw(rect, word));
    let (mut start, mut end) = (start, end);
    if !clip_line(&mut start, &mut end, clip) {
        return;
    }
    let phase = (frame as i32).wrapping_mul(7) % 16;
    patterned_line(start, end, &TARGET_LASER_PATTERN, phase, |point| {
        draw([point[0], point[1], 1, 1], word)
    });
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
    endpoint_boxes([start, end], clip, &mut emit);
    if clip_line(&mut start, &mut end, clip) {
        solid_line(start, end, emit);
    }
}

/// The clipped 3x3 box 7049C0, 4DC340 and 704E40 fill at each endpoint before
/// their line: origin point-2 on both axes, so its bottom-right pixel is the
/// endpoint.
pub(crate) fn endpoint_boxes(
    points: [[i32; 2]; 2],
    clip: [i32; 4],
    mut emit: impl FnMut([i32; 4]),
) {
    for point in points {
        emit(crate::util::rect::clip_rect(
            [point[0].wrapping_sub(2), point[1].wrapping_sub(2), 3, 3],
            clip,
        ));
    }
}

/// Original static pattern842930; Foot4DC340's 822540 holds the same
/// bytes. The rally producer phase has period15 even though
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
pub(crate) fn push_surface_rect(
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
    use super::*;
    use crate::sim::combat::TargetKind;
    use crate::sim::components::NavTargetRef;
    use crate::sim::game_entity::GameEntity;

    fn sim_with_selected_unit_that_has_attack_and_move() -> Simulation {
        let mut sim = Simulation::new();
        let mut unit = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        unit.set_owner_for_test(sim.intern("Americans"));
        unit.set_type_ref_for_test(sim.intern("MTNK"));
        unit.selection.set_selected_for_test(true);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(unit.owner()));
        unit.navigation
            .set_nav_com_for_test(Some(NavTargetRef::cell(25, 25)));
        unit.attack_target.set_for_test(Some(TargetKind::Entity(2)));
        let mut target = GameEntity::test_default(2, "HTNK", "Soviet", 14, 10);
        target.set_owner_for_test(sim.intern("Soviet"));
        target.set_type_ref_for_test(sim.intern("HTNK"));
        sim.entities_mut_for_test().insert(unit);
        sim.entities_mut_for_test().insert(target);
        sim
    }

    fn test_viewport() -> TacticalViewport {
        TacticalViewport {
            camera: [-320, 0],
            clip: [0, 0, 1000, 1000],
            zoom: 1.,
        }
    }

    #[test]
    fn relationship_interleave_visits_technos_when_action_lines_are_disabled_but_not_planning() {
        let sim = sim_with_selected_unit_that_has_attack_and_move();
        let mut state = TargetLineState::default();
        state.set_unit_action_lines_enabled(false);
        let mut visits = Vec::new();
        let mut callback = |id, instances: &mut Vec<SpriteInstance>| {
            assert!(instances.is_empty());
            visits.push(id);
        };
        assert!(
            build_target_line_instances(
                &state,
                false,
                Some(&sim),
                None,
                None,
                &HouseColorMap::new(),
                0,
                test_viewport(),
                Some(&mut callback),
                &mut |_| 127,
            )
            .is_empty()
        );
        assert_eq!(
            visits,
            sim.entities()
                .iter_sorted()
                .map(|(id, _)| id)
                .collect::<Vec<_>>()
        );
        visits.clear();
        let mut callback = |id, _: &mut Vec<SpriteInstance>| visits.push(id);
        assert!(
            build_target_line_instances(
                &state,
                true,
                Some(&sim),
                None,
                None,
                &HouseColorMap::new(),
                0,
                test_viewport(),
                Some(&mut callback),
                &mut |_| 127,
            )
            .is_empty()
        );
        assert!(visits.is_empty());
    }

    /// Selection alone opens the window — no command required.
    #[test]
    fn start_timer_opens_the_window_without_a_command() {
        let mut state = TargetLineState::default();
        assert!(!state.is_selected_action_active(100));
        state.start_timer(100);
        // The original admitted planning click reaches70D150; its rejected
        // capacity control skips that suffix. Consume the executed duration
        // here; production input capture covers whether dispatch reaches it.
        let native = crate::test_fixture::json("tools/spatial_oracle/building_manual_primary.json");
        assert_eq!(
            state.remaining_frames(100),
            native["planning_admission"]["controls"][0]["phases"][0]["state"]["planning"]
                ["action_line_timer"][2]
                .as_i64()
                .unwrap() as i32
        );
        assert!(state.is_selected_action_active(100));
        assert!(state.is_selected_action_active(124));
        assert!(!state.is_selected_action_active(125));
    }
}

#[cfg(test)]
#[path = "rally_line_tests.rs"]
mod rally_tests;

#[cfg(test)]
#[path = "action_line_tests.rs"]
mod action_tests;

#[cfg(test)]
#[path = "target_laser_tests.rs"]
mod target_laser_tests;

#[cfg(test)]
#[path = "psychic_line_tests.rs"]
mod psychic_tests;
