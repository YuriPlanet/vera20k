//! Target/action and relationship line overlays.
//!
//! Selected action lines are short-lived app-layer feedback resolved from live
//! simulation movement/attack state. Factory rally lines are app-layer visuals
//! over each factory's rally point, its ArchiveTarget.
//!
//! ## Dependency rules
//! - Part of the app layer - reads sim state but never mutates it.

use crate::app::presentation::frame::ObjectRecord;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::render::surface_line::{StoreGate, SurfaceStroke};
use crate::rules::house_colors::{HouseColorRamps, NO_REMAP};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;

/// How long selected action lines remain visible after a command is issued.
const DURATION_TICKS: i32 = 25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScreenPoint {
    x: f32,
    y: f32,
}

impl From<(f32, f32)> for ScreenPoint {
    fn from((x, y): (f32, f32)) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectedLineKind {
    Move,
    Attack,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SelectedActionLine {
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

/// Build selected unit action-line strokes from live simulation state.
/// The psychic detection line's dashed stroke carries its A gate for the GPU.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_target_line_strokes(
    line_state: &TargetLineState,
    planning_active: bool,
    sim: Option<&Simulation>,
    frame: Option<&crate::app::presentation::frame::PresentationFrame>,
    rules: Option<&RuleSet>,
    palette: Option<&crate::assets::pal_file::Palette>,
    house_colors: &HouseColorMap,
    wall_ms: u32,
    viewport: TacticalViewport,
    mut after_entity: Option<&mut dyn FnMut(u64, &mut Vec<SurfaceStroke>)>,
) -> Vec<SurfaceStroke> {
    let (Some(sim), Some(frame)) = (sim, frame) else {
        return Vec::new();
    };
    // Tactical6D4764..6D478E: a Techno the local player does not control
    // draws its psychic detection line before the planning gate. Skip the
    // per-Techno 43B150 walk when the player has no operational detector.
    // Collect the player's detectors once for the frame's Techno walk.
    // The frame captured the walk (`Place::psychically_detected`).
    let psychic = rules.filter(|_| frame.psychic_detection);
    let project = |p: ScreenPoint| {
        [
            p.x as i32 - viewport.camera[0],
            p.y as i32 - viewport.camera[1] + viewport.clip[1],
        ]
    };
    let psychic_line_for = |entity: &ObjectRecord, strokes: &mut Vec<SurfaceStroke>| {
        let Some(rules) = psychic else {
            return;
        };
        if sim.house_is_human_player(entity.owner())
            || !matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
            || entity.lifecycle.in_limbo()
            || !entity.place.psychically_detected
        {
            return;
        }
        let Some(line) = entity.place.action_line else {
            return;
        };
        let owner = sim.interner().resolve(entity.owner());
        let color = house_colors.get(owner).copied().unwrap_or(NO_REMAP);
        let rgb = crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, color);
        emit_psychic_detection_line(
            strokes,
            project(line.start),
            project(line.end),
            rgb,
            wall_ms,
            viewport,
        );
    };
    // Tactical6D4729 and6D4793 use the same planning gate; the action-line
    //option/timer gates only Foot4DC060, not CaptureManager relationships.
    if planning_active {
        let mut strokes = Vec::new();
        if psychic.is_some() {
            for entity in frame.line_sources() {
                psychic_line_for(entity, &mut strokes);
            }
        }
        return strokes;
    }
    let [left, top, width, height] = viewport.clip;
    let clip_usable = width > 0
        && height > 0
        && left.checked_add(width).is_some()
        && top.checked_add(height).is_some();
    let words = (clip_usable && line_state.is_selected_action_active(sim.session().binary_frame()))
        .then_some(palette)
        .flatten()
        .map(|palette| [action_line_word(palette, 3), action_line_word(palette, 8)]);
    let mut control = rules
        .map(|rules| {
            super::control_links::prepare(sim, frame, rules, house_colors, wall_ms, viewport)
        })
        .unwrap_or_default();
    if words.is_none() && control.is_empty() && after_entity.is_none() && psychic.is_none() {
        return Vec::new();
    }
    let line_for = |entity: &ObjectRecord| {
        selected_action_line_for_entity(entity, sim).map(|line| ProjectedActionLine {
            start: project(line.start),
            end: project(line.end),
            kind: line.kind,
        })
    };
    // Tactical6D4750 walks the construction-ordered Techno array, not the
    // CurrentObjects selection vector or the Logic active-object order.
    // EntityStore's monotonic stable IDs retain that represented order.
    //6D4750/6D478E ->6D47BF/6D47F6 ->6D47FB within EACH Techno visit.
    //Resolving all action lines before relationships would reverse
    //cross-object overlaps. Only the frame's line sources draw anything in
    //this walk, so the others are passed by unread.
    let mut strokes = Vec::new();
    for entity in frame.line_sources() {
        if let Some(words) = words
            && let Some(line) = line_for(entity)
        {
            push_selected_action_line(&mut strokes, line, words, viewport);
        }
        psychic_line_for(entity, &mut strokes);
        if let Some(lines) = control.remove(&entity.stable_id()) {
            strokes.extend(lines);
        }
        if let Some(callback) = after_entity.as_mut() {
            callback(entity.stable_id(), &mut strokes);
        }
    }
    strokes
}

/// Foot4DC340, an enemy Foot's psychic detection line: two clipped 3x3 boxes
/// at point-2 in the owner's colour (House+56F9), then, when 7BC2B0 clips the
/// line in, DSurface4C0750's dashed walk over pattern822540 (the rally
/// pattern) at phase `(-time >> 5) & 15`, storing only where the A-buffer is
/// nonzero (the GPU applies the dashes' [`StoreGate::ANonzero`]; the boxes
/// have none). The colour pulses toward white (6612C0 by `time & 0x3FF`, the
/// low byte mirrored while bit 8 is set) except while bit 9 is set. `time` is
/// timeGetTime: wall-clock milliseconds. No RNG, timer or simulation write.
fn emit_psychic_detection_line(
    strokes: &mut Vec<SurfaceStroke>,
    start: [i32; 2],
    end: [i32; 2],
    rgb: [u8; 3],
    time: u32,
    view: TacticalViewport,
) {
    use crate::render::native_surface_format::{RGB565, interpolate_rgb8};
    use crate::render::surface_line::clip_line;
    push_endpoint_boxes(strokes, [start, end], RGB565.pack_rgb8(rgb), view);
    let (mut start, mut end) = (start, end);
    if !clip_line(&mut start, &mut end, view.clip) {
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
    let phase = ((time.wrapping_neg() as i32) >> 5) & 0xF;
    strokes.push(SurfaceStroke::patterned(
        start,
        end,
        &RALLY_PATTERN,
        phase,
        RGB565.pack_rgb8(color),
        StoreGate::ANonzero,
        view.camera,
    ));
}

/// The integer logical viewport the line producers draw on. Native rally
/// 6DA9D0 and action7049C0 operate in unzoomed surface pixels; wheel zoom
/// scales the finished world surface (`WorldZoomPass`), never their strokes.
///
/// `camera` is the world point at the battlefield's corner, gamemd's tactical
/// position, and `clip` the battlefield on its surface. The match's view
/// (`tactical_world_viewport`) is in the battlefield's own pixels, its clip
/// at 0, 0: wherever a layout puts the battlefield on the frame is in the
/// camera, so the owners' native projections and the strokes' positions,
/// surface pixel plus camera, need no offset of their own.
#[derive(Clone, Copy)]
pub(crate) struct TacticalViewport {
    pub camera: [i32; 2],
    pub clip: [i32; 4],
}

/// Tactical6DA9D0: reverse CurrentObjects (the player's selection vector),
/// live local Building, HasRallyPoint455DA0 and ArchiveTarget. The coordinate
/// owners retain foundation offsets and native ground/slope/bridge semantics.
/// No RNG draws, timer writes, detach calls, Z reads or Z writes occur here.
/// Evidence: tools/procedural_drawing_oracle/rally.{py,json,meta.json}.
pub(crate) fn build_factory_rally_line_strokes(
    sim: Option<&Simulation>,
    frame: Option<&crate::app::presentation::frame::PresentationFrame>,
    rules: Option<&RuleSet>,
    selected_ids: &[u64],
    house_color_map: &HouseColorMap,
    local_owner: Option<&str>,
    viewport: TacticalViewport,
) -> [Vec<SurfaceStroke>; 2] {
    let mut strokes = [Vec::new(), Vec::new()];
    let (Some(sim), Some(frame), Some(rules), Some(local_owner)) = (sim, frame, rules, local_owner)
    else {
        return strokes;
    };
    let project = |coord: crate::sim::components::DriveCoord| {
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        [
            x as i32 - viewport.camera[0] + viewport.clip[0],
            y as i32 - viewport.camera[1] + viewport.clip[1],
        ]
    };
    for &id in selected_ids.iter().rev() {
        let Some(entity) = frame.object(id) else {
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
        let Some([start, target]) = entity.place.rally_line else {
            continue;
        };
        let start = project(start);
        let end = project(target);
        let word = rally_word_for_owner(owner, house_color_map, &rules.house_color_ramps);
        emit_rally_line(
            &mut strokes,
            start,
            end,
            word,
            sim.session().binary_frame(),
            viewport,
        );
    }
    strokes
}

fn selected_action_line_for_entity(
    entity: &ObjectRecord,
    sim: &Simulation,
) -> Option<SelectedActionLine> {
    if !entity.selection.is_selected()
        || entity.category() == EntityCategory::Structure
        || !sim.house_is_human_player(entity.owner())
    {
        return None;
    }
    entity.place.action_line
}

/// A Structure's rally line in world coordinates (`TechnoClass::
/// ArchiveTarget`'s coordinate, its deck-adjusted ground, and the building's
/// GetCoords), captured into its presentation record.
pub(crate) fn rally_line_coords(
    entity: &crate::sim::game_entity::GameEntity,
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
) -> Option<[crate::sim::components::DriveCoord; 2]> {
    use crate::sim::movement::ground_pose::{
        object_get_coords, query_ground_height, target_get_coords,
    };
    if entity.category() != EntityCategory::Structure {
        return None;
    }
    let mut target = target_get_coords(entity.archive_target()?, sim, Some(cells))?;
    let ground = query_ground_height(cells, target).ok()?;
    target.z = ground.wrapping_add(
        if cells.flags(cells.lookup_world(target.x, target.y)) & 0x100 != 0 {
            crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        },
    );
    Some([object_get_coords(entity, Some(cells.terrain())), target])
}

/// The endpoints Foot4DC060 and Foot4DC340 share: with a Target, the +300
/// pivot to 70BCB0's lead coordinate; else, with a NavCom, the Location to
/// the last queued waypoint or the NavCom.
pub(crate) fn action_line_endpoints(
    entity: &crate::sim::game_entity::GameEntity,
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

/// With a Target (+2B4), the +300 pivot (argument 0; an infantryman's gun)
/// to 70BCB0's lead coordinate, projected: Foot4DC0C6/4DC0DF and the target
/// laser's Techno6FF983/6FF9A4. FireAt shares the lead query; no presentation
/// copy.
fn attack_line_points(
    entity: &crate::sim::game_entity::GameEntity,
    sim: &Simulation,
    rules: &RuleSet,
) -> Option<[ScreenPoint; 2]> {
    let attack = entity.attack_target.target()?;
    let start = crate::sim::combat::fire_coord::turret_pivot_coordinate(sim, rules, entity, 0)?;
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

/// The owner's house colour as the rally line's surface word.
fn rally_word_for_owner(
    owner: &str,
    house_color_map: &HouseColorMap,
    ramps: &HouseColorRamps,
) -> u16 {
    // Unknown owner → NO_REMAP, which ramp() resolves to the default scheme
    // (matching the producers' DEFAULT_SCHEME_ENTRY fallback), not entry 0.
    let index = house_color_map.get(owner).copied().unwrap_or(NO_REMAP);
    crate::render::native_surface_format::RGB565
        .pack_rgb8(crate::render::palette_light::house_color_rgb(ramps, index))
}

/// The Move action line's colour (Palette87F6C4 index 3) as presented, for
/// VERA's formation line dots: they mark where a Move will send each unit.
pub(crate) fn move_action_line_color(palette: &crate::assets::pal_file::Palette) -> [f32; 3] {
    use crate::render::native_surface_format::{ACTIVE_RETAIL_RGB565_PRESENTATION, RGB565};
    let [r, g, b] = RGB565.unpack_rgb8(action_line_word(palette, 3));
    let rgba = ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([r, g, b, 255]);
    [rgba[0], rgba[1], rgba[2]].map(|v| f32::from(v) / 255.)
}

/// The packed surface word of Palette87F6C4's entry `index` (its 16-bit
/// table at +174, `+0x10` for index 8): the N53 middle row, read by 4DC280
/// (Move index 3) and 4DC0FA (attack index 8), unpacked and repacked by
/// 7049C0. Raw surface color; the action line samples no A or cell light.
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
    entity: &ObjectRecord,
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
    entity: &ObjectRecord,
    sim: &Simulation,
    rules: &RuleSet,
    palette: impl FnOnce() -> Option<&'a crate::assets::pal_file::Palette>,
    view: crate::render::surface_line::SurfaceLineViewport,
    frame: u32,
) -> Vec<crate::render::surface_line::SurfaceDraw> {
    use crate::render::surface_line::SurfaceDraw;
    let mut draws = Vec::new();
    // Every test only reads, so the cheap ones go first: most units have
    // not fired in the last 15 frames.
    if entity.category() != EntityCategory::Unit
        || (frame as i32).wrapping_sub(entity.last_fire_frame.frame() as i32) >= 15
        || !sim
            .interner()
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.target_laser)
        || !sim.house_is_human_player(entity.owner())
    {
        return draws;
    }
    let points = entity
        .place
        .action_line
        .filter(|line| line.kind == SelectedLineKind::Attack)
        .map(|line| [line.start, line.end]);
    let (Some(points), Some(palette)) = (points, palette()) else {
        return draws;
    };
    // 6D2140 then +886FA4 on y, as Foot4DC060 projects its endpoints. The
    // original's battlefield has no x offset to add; a layout's may.
    let [start, end] = points.map(|p| {
        [
            p.x as i32 - view.camera[0] + view.clip[0],
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
/// by the solid line, in Palette87F6C4 index 3 (Move) or 8 (Attack). The
/// native box origin is point-2 on both axes. Its bottom-right pixel is the
/// endpoint, not the centre of a symmetric box. No A/Z reads or writes, RNG
/// draws, timer mutations or detach calls. Native primitive and overlap
/// evidence: tools/procedural_drawing_oracle/action_lines.
fn push_selected_action_line(
    strokes: &mut Vec<SurfaceStroke>,
    line: ProjectedActionLine,
    words: [u16; 2],
    view: TacticalViewport,
) {
    let word = match line.kind {
        SelectedLineKind::Move => words[0],
        SelectedLineKind::Attack => words[1],
    };
    push_endpoint_boxes(strokes, [line.start, line.end], word, view);
    let (mut start, mut end) = (line.start, line.end);
    if crate::render::surface_line::clip_line(&mut start, &mut end, view.clip) {
        strokes.push(SurfaceStroke::solid(start, end, word, view.camera));
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

/// [`endpoint_boxes`] as rectangle strokes of `word`.
pub(crate) fn push_endpoint_boxes(
    strokes: &mut Vec<SurfaceStroke>,
    points: [[i32; 2]; 2],
    word: u16,
    view: TacticalViewport,
) {
    endpoint_boxes(points, view.clip, |rect| {
        push_rect(strokes, rect, word, view)
    });
}

/// A filled surface rectangle of `word`, already clipped; nothing when empty.
pub(crate) fn push_rect(
    strokes: &mut Vec<SurfaceStroke>,
    rect: [i32; 4],
    word: u16,
    view: TacticalViewport,
) {
    if let Some(stroke) = SurfaceStroke::rect(rect, word, StoreGate::All, view.camera) {
        SurfaceStroke::push(strokes, stroke);
    }
}

/// Original static pattern842930; Foot4DC340's 822540 holds the same
/// bytes. The rally producer phase has period15 even though
/// the leaf repeats16 entries; signed subtraction/remainder survive frame wrap.
const RALLY_PATTERN: [u8; 16] = [1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0];

/// Three dashed rows, black then the owner's `word` twice. Tactical6D4648
/// stores the pixels whose A is nonzero and 6D46CF, later, those whose A is
/// zero: each row's stroke enters both passes, gated for the GPU.
fn emit_rally_line(
    strokes: &mut [Vec<SurfaceStroke>; 2],
    mut from: [i32; 2],
    mut to: [i32; 2],
    word: u16,
    frame: u32,
    viewport: TacticalViewport,
) {
    use crate::render::surface_line::clip_line;
    let phase = i32::MAX.wrapping_sub(frame as i32) % 15;
    from[1] += 2;
    to[1] += 2;
    for word in [0, word, word] {
        // The original mutates and reuses these clipped points, including when
        // one row misses the clip. Independent offset/clip calls differ at edges.
        if clip_line(&mut from, &mut to, viewport.clip) {
            for (pass, gate) in [StoreGate::ANonzero, StoreGate::AZero]
                .into_iter()
                .enumerate()
            {
                strokes[pass].push(SurfaceStroke::patterned(
                    from,
                    to,
                    &RALLY_PATTERN,
                    phase,
                    word,
                    gate,
                    viewport.camera,
                ));
            }
        }
        from[1] -= 1;
        to[1] -= 1;
    }
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
        }
    }

    /// The relationship interleave visits the frame's line sources
    /// ([`crate::app::presentation::frame::PresentationFrame::line_sources`])
    /// with action lines off, and none while planning.
    #[test]
    fn relationship_interleave_visits_technos_when_action_lines_are_disabled_but_not_planning() {
        let sim = sim_with_selected_unit_that_has_attack_and_move();
        let frame = crate::app::presentation::frame::PresentationFrame::of_test(
            &sim,
            crate::rules::ruleset::RuleSet::empty_for_test(),
        );
        let mut state = TargetLineState::default();
        state.set_unit_action_lines_enabled(false);
        let mut visits = Vec::new();
        let mut callback = |id, strokes: &mut Vec<SurfaceStroke>| {
            assert!(strokes.is_empty());
            visits.push(id);
        };
        assert!(
            build_target_line_strokes(
                &state,
                false,
                Some(&sim),
                Some(&frame),
                None,
                None,
                &HouseColorMap::new(),
                0,
                test_viewport(),
                Some(&mut callback),
            )
            .is_empty()
        );
        // The selected unit is a line source; its idle target draws no line,
        // so the walk passes it by.
        let sources: Vec<u64> = frame.line_sources().map(|r| r.stable_id()).collect();
        assert_eq!(sources.len(), 1);
        assert_eq!(visits, sources);
        visits.clear();
        let mut callback = |id, _: &mut Vec<SurfaceStroke>| visits.push(id);
        assert!(
            build_target_line_strokes(
                &state,
                true,
                Some(&sim),
                Some(&frame),
                None,
                None,
                &HouseColorMap::new(),
                0,
                test_viewport(),
                Some(&mut callback),
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
