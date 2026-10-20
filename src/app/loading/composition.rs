//! Pure preparation for the loading-screen composition.
//!
//! This app-level module turns parsed map and resolved launch data into an
//! immutable CPU snapshot, for both selected maps and random-map seed loads.
//! GPU asset decoding, marker remaps, font rasterization, file reads, and
//! presentation remain owned by the loading/render modules.

use std::collections::HashMap;

use crate::assets::csf_file::CsfFile;
use crate::map::map_file::MapFile;
use crate::map::playfield::PlayfieldBounds;
use crate::map::preview::DecodedPreview;
use crate::map::waypoints::Waypoint;
use crate::skirmish_launch::{LaunchCountry, SkirmishLaunchMode, SkirmishLaunchSession};
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale};

const NATIVE_START_LIMIT: u32 = 8;
const PREVIEW_SCALE: i64 = 1_000;
const MARKER_FRACTION_SCALE: i64 = 1_000_000;
const MMPB_OFFSET_X: i64 = -3;
const MMPB_OFFSET_Y: i64 = -2;
const START_INDICATOR_SIZE: i64 = 4;

/// The one screen width that selects the narrow loading art. gamemd compares for
/// equality here, not against a threshold, and the same comparison picks the
/// narrow art size, the narrow text table and the narrow progress-row origin.
pub(crate) const NARROW_LOADING_SCREEN_WIDTH: u32 = 640;
const NARROW_ART_SIZE: [i32; 2] = [640, 480];
const WIDE_ART_SIZE: [i32; 2] = [800, 600];

/// Skirmish mode whose briefing block sits lower on the loading screen.

/// Bitmap the random-map setup dialog writes and the preview source the
/// random-map loading branch consumes.
pub(crate) const RANDOM_MAP_PREVIEW_FILE: &str = "RandMap.img";

/// Loading-art viewport size for the current screen width.
pub(crate) const fn loading_art_viewport_size(render_width: u32) -> [i32; 2] {
    if render_width == NARROW_LOADING_SCREEN_WIDTH {
        NARROW_ART_SIZE
    } else {
        WIDE_ART_SIZE
    }
}

/// The mapping the loading screen paints in. gamemd centers the art viewport in
/// the window and draws it 1:1; a window the shell family fits instead scales the
/// authored dialog to fit it, centred, so every authored rect maps through this
/// scale. [`ShellScale::RETAIL`] leaves every path exactly as gamemd has it.
pub(crate) fn loading_screen_scale(fill_window: bool, render_size: [u32; 2]) -> ShellScale {
    if fill_window {
        ShellScale::from_screen(render_size[0] as i32, render_size[1] as i32)
    } else {
        ShellScale::RETAIL
    }
}

/// The size the loading layers are authored in: the live window at retail, and the
/// dialog a scaled page grows into the window. Every layer's geometry — the
/// narrow/wide art branch, the centered origin and each rect — is computed from
/// this size first, then mapped through the scale, so the two paths cannot drift.
pub(crate) fn loading_authored_size(render_size: [u32; 2], scale: ShellScale) -> [u32; 2] {
    if scale.is_retail() {
        render_size
    } else {
        [SHELL_BASE_W as u32, SHELL_BASE_H as u32]
    }
}

/// Origin that every loading-screen layer hangs off: the top-left corner of the
/// centered art viewport.
///
/// gamemd computes this once when the load-progress manager is set up, and the
/// background art, the progress row and all four text layers read it back from
/// there. Deriving all three from this one helper is what keeps them from
/// drifting apart when the window is not exactly the art size.
pub(crate) const fn loading_base_origin(render_size: [u32; 2]) -> [i32; 2] {
    let [width, height] = render_size;
    let art = loading_art_viewport_size(width);
    [(width as i32 - art[0]) / 2, (height as i32 - art[1]) / 2]
}

/// Native loading-preview destination rectangle, in `(x, y, width, height)` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MmpbRegionRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl MmpbRegionRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The region a stretched page draws its preview into: the native rect grown
    /// by the layout's scale. Identity on the retail mapping.
    pub(crate) fn scaled(self, scale: ShellScale) -> Self {
        let rect = scale.scale_rect(RectPx::new(self.x, self.y, self.width, self.height));
        Self::new(rect.x, rect.y, rect.w, rect.h)
    }
}

/// Select the exact native width-equality branch for the loading preview.
pub(crate) const fn mmpb_region_rect(render_width: u32) -> MmpbRegionRect {
    match render_width {
        800 => MmpbRegionRect::new(499, 379, 216, 166),
        1024 => MmpbRegionRect::new(570, 424, 300, 260),
        _ => MmpbRegionRect::new(385, 270, 200, 200),
    }
}

/// Integer-projected playfield coordinate used by the loading marker helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectedPoint {
    pub x: i32,
    pub y: i32,
}

/// Minimum and extent values in projected loading-marker coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectedPlayfieldBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub extent_x: i32,
    pub extent_y: i32,
}

/// Result of the native scale-1000 aspect fit inside an `mmpb` region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewAspectFit {
    pub scale_1000: i32,
    pub width: i32,
    pub height: i32,
    pub pad_x: i32,
    pub pad_y: i32,
}

/// Colored-marker anchor before ordinary destination-surface clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MmpbMarkerAnchor {
    pub local_x: i32,
    pub local_y: i32,
    pub screen_x: i32,
    pub screen_y: i32,
}

/// App-owned participant identity; deliberately separate from waypoint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LoadingParticipantId {
    Local,
    Human(usize),
    Opponent(usize),
}

/// Final Rust launch ownership for one original start waypoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoadingStartAssignment {
    pub start_index: u32,
    pub participant: LoadingParticipantId,
    /// Launch color priority, resolved later through the existing house-ramp path.
    pub color_priority: u8,
}

/// One `mmpb.shp` record retaining start, participant, and color identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MmpbMarkerRecord {
    pub start_index: u32,
    pub waypoint: Waypoint,
    pub participant: LoadingParticipantId,
    pub color_priority: u8,
    pub anchor: MmpbMarkerAnchor,
}

/// Decorated source preview plus the exact destination fit used by rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedLoadingPreview {
    pub image: DecodedPreview,
    pub region: MmpbRegionRect,
    pub fit: PreviewAspectFit,
}

/// Static CSF keys selected from the resolved local country.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoadingCountryTextKeys {
    pub country_name: &'static str,
    pub special_unit: &'static str,
    pub load_brief: &'static str,
}

const COUNTRY_TEXT_KEYS: [LoadingCountryTextKeys; 10] = [
    LoadingCountryTextKeys {
        country_name: "Name:Americans",
        special_unit: "Name:Para",
        load_brief: "LoadBrief:USA",
    },
    LoadingCountryTextKeys {
        country_name: "Name:Alliance",
        special_unit: "Name:BEAGLE",
        load_brief: "LoadBrief:Korea",
    },
    LoadingCountryTextKeys {
        country_name: "Name:French",
        special_unit: "Name:GTGCAN",
        load_brief: "LoadBrief:French",
    },
    LoadingCountryTextKeys {
        country_name: "Name:Germans",
        special_unit: "Name:TNKD",
        load_brief: "LoadBrief:Germans",
    },
    LoadingCountryTextKeys {
        country_name: "Name:British",
        special_unit: "Name:SNIPE",
        load_brief: "LoadBrief:British",
    },
    LoadingCountryTextKeys {
        country_name: "Name:Africans",
        special_unit: "Name:DTRUCK",
        load_brief: "LoadBrief:Lybia",
    },
    LoadingCountryTextKeys {
        country_name: "Name:Arabs",
        special_unit: "Name:DESO",
        load_brief: "LoadBrief:Iraq",
    },
    LoadingCountryTextKeys {
        country_name: "Name:Confederation",
        special_unit: "Name:TERROR",
        load_brief: "LoadBrief:Cuba",
    },
    LoadingCountryTextKeys {
        country_name: "Name:Russians",
        special_unit: "Name:TTNK",
        load_brief: "LoadBrief:Russia",
    },
    LoadingCountryTextKeys {
        country_name: "Name:YuriCountry",
        special_unit: "Name:YURI",
        load_brief: "LoadBrief:YuriCountry",
    },
];

pub(crate) const LOADING_TEXT_KEY: &str = "GUI:LoadingEx";

/// Resolve the verified country-dependent key row without inventing fallback text.
pub(crate) fn loading_country_text_keys(country: LaunchCountry) -> Option<LoadingCountryTextKeys> {
    COUNTRY_TEXT_KEYS
        .get(usize::from(country.raw_index()))
        .copied()
}

/// Localized strings retained by the immutable loading composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalizedLoadingTextSnapshot {
    pub keys: Option<LoadingCountryTextKeys>,
    pub country_name: Option<String>,
    pub special_unit: Option<String>,
    pub load_brief: Option<String>,
    pub loading: Option<String>,
}

impl LocalizedLoadingTextSnapshot {
    fn missing(country: LaunchCountry) -> Self {
        Self {
            keys: loading_country_text_keys(country),
            country_name: None,
            special_unit: None,
            load_brief: None,
            loading: None,
        }
    }
}

/// Resolve the four mode-5 loading strings, omitting each independently when absent.
pub(crate) fn localize_loading_text(
    csf: &CsfFile,
    country: LaunchCountry,
) -> LocalizedLoadingTextSnapshot {
    let Some(keys) = loading_country_text_keys(country) else {
        // DrawLoading55381E/553B39/553E52 supplies NULL outside0..9;
        // 553B44 then copies NULL. Safely omit these unsupported stock labels
        // rather than reproducing that crash or changing the canonical country.
        return LocalizedLoadingTextSnapshot {
            loading: Some(csf.text(LOADING_TEXT_KEY).into_owned()),
            ..LocalizedLoadingTextSnapshot::missing(country)
        };
    };
    LocalizedLoadingTextSnapshot {
        keys: Some(keys),
        country_name: Some(csf.text(keys.country_name).into_owned()),
        special_unit: Some(native_uppercase_special_unit(
            csf.text(keys.special_unit).as_ref(),
        )),
        load_brief: Some(csf.text(keys.load_brief).into_owned()),
        loading: Some(csf.text(LOADING_TEXT_KEY).into_owned()),
    }
}

/// Logical rectangles for the four verified post-marker text layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoadingTextRects {
    pub country_name: RectPx,
    pub special_unit: RectPx,
    pub load_brief: RectPx,
    pub loading: RectPx,
}

/// Compute the native 640 and 800 text tables, offset by the shared base origin.
///
/// The Cooperative briefing Y only exists in the 800 table: gamemd's Cooperative
/// branch collapses to the same values as the plain branch at exactly 640 wide.
/// The tables are authored numbers, so a stretched page maps them through its
/// scale after the offset, keeping the copy on the art it labels.
pub(crate) fn loading_text_rects(
    render_size: [u32; 2],
    mode: &SkirmishLaunchMode,
    scale: ShellScale,
) -> LoadingTextRects {
    let authored = loading_authored_size(render_size, scale);
    let table = if authored[0] == NARROW_LOADING_SCREEN_WIDTH {
        LoadingTextRects {
            country_name: RectPx::new(385, 436, 200, 20),
            special_unit: RectPx::new(16, 72, 200, 20),
            load_brief: RectPx::new(16, 126, 318, 104),
            loading: RectPx::new(16, 235, 200, 20),
        }
    } else {
        let cooperative = mode.class == crate::skirmish_modes::MpModeClass::Cooperative;
        LoadingTextRects {
            country_name: RectPx::new(540, 310, 200, 20),
            special_unit: RectPx::new(20, 90, 200, 20),
            load_brief: RectPx::new(20, if cooperative { 380 } else { 158 }, 398, 130),
            loading: RectPx::new(20, 300, 200, 20),
        }
    };

    let [dx, dy] = loading_base_origin(authored);
    let place = |rect: RectPx| scale.scale_rect(rect.translate(dx, dy));
    LoadingTextRects {
        country_name: place(table.country_name),
        special_unit: place(table.special_unit),
        load_brief: place(table.load_brief),
        loading: place(table.loading),
    }
}

/// Explicit player-visible ordering shared by first-frame and progress repaints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadingCompositionLayer {
    Background,
    PreviewWithBlackStartIndicators,
    AssignedMmpbMarkers,
    CountryBacking,
    CountryText,
    SpecialUnitText,
    BriefingBacking,
    BriefingText,
    LoadingBacking,
    LoadingText,
    ProgressBacking,
    ProgressBar,
    ProgressSideIcon,
    ProgressLabel,
}

/// Immutable CPU-side input for every selected-map loading presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoadingCompositionSnapshot {
    /// The mapping [`Self::text_rects`] and the preview region were placed in, so
    /// the paint path scales the art it draws beside them the same way.
    pub scale: ShellScale,
    pub preview: Option<PreparedLoadingPreview>,
    pub markers: Vec<MmpbMarkerRecord>,
    pub text: LocalizedLoadingTextSnapshot,
    pub text_rects: LoadingTextRects,
    pub layers: Vec<LoadingCompositionLayer>,
}

/// Build a complete selected-map snapshot after the map's initial parse.
pub(crate) fn build_loading_composition(
    map: &MapFile,
    session: &SkirmishLaunchSession,
    csf: Option<&CsfFile>,
    render_size: [u32; 2],
    scale: ShellScale,
    assignments: &[LoadingStartAssignment],
) -> LoadingCompositionSnapshot {
    let region = mmpb_region_rect(loading_authored_size(render_size, scale)[0]).scaled(scale);
    let prefix = native_loading_waypoint_prefix(&map.waypoints);
    let bounds = projected_playfield_bounds(map);

    let mut markers = Vec::new();
    let preview = map
        .preview
        .decoded
        .as_ref()
        .filter(|preview| valid_preview_buffer(preview))
        .and_then(|preview| {
            let fit = aspect_fit_preview(region, preview.width, preview.height)?;
            let mut image = preview.clone();
            if let Some(bounds) = bounds {
                burn_black_start_indicators(&mut image, &prefix, bounds);
                markers = build_mmpb_marker_records(&prefix, assignments, bounds, region, fit);
            }
            Some(PreparedLoadingPreview { image, region, fit })
        });

    finish_loading_composition(session, csf, render_size, scale, preview, markers)
}

/// Build the snapshot for a random-map seed load.
///
/// gamemd branches the *preview holder* on the scenario's random-map flag and
/// nothing else: the preview comes from the `RandMap.img` bitmap the random-map
/// setup dialog wrote — that dialog is where the map is generated — rather than
/// from the scenario. The four text layers sit after that branch and are drawn
/// exactly as they are for a selected map.
///
/// gamemd provenance: DrawLoadingScreen 0x00552D60 loads `RandMap.img` at
/// 0x00553592/0x00553599, then unconditionally calls compositor 0x00640A40 at
/// 0x00553687 with the active Scenario waypoint array and resolved assignments.
/// For an accepted RMG launch, Full Init copied setup staging into that array
/// before `.SED` regeneration; the regenerated map supplies current playfield
/// bounds, but its waypoint table does not replace that earlier active copy.
/// Rust's optional setup preview below is presentation fallback only; it never
/// enters map loading or start assignment.
pub(crate) fn build_random_map_loading_composition(
    session: &SkirmishLaunchSession,
    csf: Option<&CsfFile>,
    render_size: [u32; 2],
    scale: ShellScale,
    preview_image: Option<DecodedPreview>,
    launch_map: &MapFile,
    active_scenario_waypoints: &HashMap<u32, Waypoint>,
    assignments: &[LoadingStartAssignment],
) -> LoadingCompositionSnapshot {
    let region = mmpb_region_rect(loading_authored_size(render_size, scale)[0]).scaled(scale);
    let mut markers = Vec::new();
    let preview = preview_image
        .filter(valid_preview_buffer)
        .and_then(|mut image| {
            let fit = aspect_fit_preview(region, image.width, image.height)?;
            let prefix = native_loading_waypoint_prefix(active_scenario_waypoints);
            if let Some(bounds) = projected_playfield_bounds(launch_map) {
                burn_black_start_indicators(&mut image, &prefix, bounds);
                markers = build_mmpb_marker_records(&prefix, assignments, bounds, region, fit);
            }
            Some(PreparedLoadingPreview { image, region, fit })
        });

    finish_loading_composition(session, csf, render_size, scale, preview, markers)
}

/// Attach the map-independent layers — the four localized text strings and their
/// rectangles — to whichever preview and marker set the caller resolved.
fn finish_loading_composition(
    session: &SkirmishLaunchSession,
    csf: Option<&CsfFile>,
    render_size: [u32; 2],
    scale: ShellScale,
    preview: Option<PreparedLoadingPreview>,
    markers: Vec<MmpbMarkerRecord>,
) -> LoadingCompositionSnapshot {
    let country = session.local_slot().country;
    let text = csf.map_or_else(
        || LocalizedLoadingTextSnapshot::missing(country),
        |csf| localize_loading_text(csf, country),
    );
    let text_rects = loading_text_rects(render_size, &session.mode, scale);
    let layers = composition_layers(preview.is_some(), !markers.is_empty(), &text);
    LoadingCompositionSnapshot {
        scale,
        preview,
        markers,
        text,
        text_rects,
        layers,
    }
}

/// Project one signed-16-bit map cell through the verified loading coordinate path.
pub(crate) fn project_waypoint(waypoint: Waypoint) -> ProjectedPoint {
    project_cell(waypoint.rx, waypoint.ry)
}

fn project_cell(rx: u16, ry: u16) -> ProjectedPoint {
    let lepton_x = i64::from(rx as i16) * 256 + 128;
    let lepton_y = i64::from(ry as i16) * 256 + 128;
    let raw_x = (lepton_x * 60) / 2 + (lepton_y * -60) / 2;
    let raw_y = (lepton_x * 30) / 2 + (lepton_y * 30) / 2;
    let screen_x = raw_x / 256 + 15_360;
    let screen_y = raw_y / 256;
    ProjectedPoint {
        x: (screen_x / 60) as i32,
        y: (screen_y / 30) as i32,
    }
}

/// Derive projected min/extents from mode-zero playfield cells.
///
/// gamemd-derived: loading compositor `FUN_00640A40` gates every decoded
/// cell through `MapClass::IsCellInPlayfield @ 0x00578460` with mode `0`
/// before updating its projected bounds. Size-diamond filler therefore cannot
/// move ordinary or retained-RMG loading markers.
pub(crate) fn projected_playfield_bounds(map: &MapFile) -> Option<ProjectedPlayfieldBounds> {
    let playfield = PlayfieldBounds::from_map_header(&map.header);
    let mut points = map
        .cells
        .iter()
        .filter(|cell| playfield.contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry)))
        .map(|cell| project_cell(cell.rx, cell.ry));
    let first = points.next()?;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
    for point in points {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    Some(ProjectedPlayfieldBounds {
        min_x,
        min_y,
        extent_x: max_x - min_x,
        extent_y: max_y - min_y,
    })
}

/// `ComputeScenarioHeaderFromMap 0x0058B830` over a generated map: the
/// leading defined start waypoints 0..7 (the loop stops at the first
/// undefined one, `0x0058B858`) give the staged pairs through the same
/// `0x006D62E0` projection divided by 60 and 30, and the playfield cells give
/// the minimum and span (`0x0058B91F..0x0058BA14`). The cells are the start
/// waypoints by slot, as the `'W'` sender reads them (`0x005E7F38..`).
/// RESIDUAL: native gates the bounds with `IsCellInPlayfield` mode 1
/// (`0x0058B953`, level and slope aware); this reuses the compositor's mode-0
/// bounds, which can differ by a unit at the playfield's edge.
pub(crate) fn scenario_start_staging(
    map: &MapFile,
    start_waypoints: &[(u8, u16, u16)],
) -> crate::map::preview::ScenarioStartStaging {
    use crate::map::preview::{START_STAGING_SLOTS, ScenarioStartStaging};
    let mut staging = ScenarioStartStaging::default();
    let cell = |slot: usize| {
        start_waypoints
            .iter()
            .find(|(index, _, _)| usize::from(*index) == slot)
            .map(|&(_, rx, ry)| (rx, ry))
    };
    for slot in 0..START_STAGING_SLOTS {
        if let Some((rx, ry)) = cell(slot) {
            staging.cells[slot] = (rx as i16, ry as i16);
        }
    }
    for slot in 0..START_STAGING_SLOTS {
        let Some((rx, ry)) = cell(slot) else {
            break;
        };
        let point = project_cell(rx, ry);
        staging.pairs[slot] = (point.x, point.y);
        staging.count += 1;
    }
    // The minimum starts at 10000 and the maximum at 0 (0x0058B929).
    let bounds = projected_playfield_bounds(map).unwrap_or(ProjectedPlayfieldBounds {
        min_x: 10_000,
        min_y: 10_000,
        extent_x: -10_000,
        extent_y: -10_000,
    });
    staging.start_x = bounds.min_x;
    staging.start_y = bounds.min_y;
    staging.width = bounds.extent_x;
    staging.height = bounds.extent_y;
    staging
}

/// Reproduce the valid-count-then-numeric-prefix behavior over waypoint indices 0..7.
pub(crate) fn native_loading_waypoint_prefix(waypoints: &HashMap<u32, Waypoint>) -> Vec<Waypoint> {
    let valid_count = (0..NATIVE_START_LIMIT)
        .filter(|index| waypoints.contains_key(index))
        .count() as u32;
    (0..valid_count)
        .filter_map(|index| waypoints.get(&index).copied())
        .collect()
}

/// Compute the native scale-1000 fit, rejecting invalid/zero fitted dimensions.
pub(crate) fn aspect_fit_preview(
    region: MmpbRegionRect,
    source_width: u32,
    source_height: u32,
) -> Option<PreviewAspectFit> {
    if region.width <= 0 || region.height <= 0 || source_width == 0 || source_height == 0 {
        return None;
    }
    let source_width = i64::from(source_width);
    let source_height = i64::from(source_height);
    let region_width = i64::from(region.width);
    let region_height = i64::from(region.height);
    let scale_1000 = (region_height * PREVIEW_SCALE / source_height)
        .min(region_width * PREVIEW_SCALE / source_width);
    let width = source_width * scale_1000 / PREVIEW_SCALE;
    let height = source_height * scale_1000 / PREVIEW_SCALE;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(PreviewAspectFit {
        scale_1000: i32::try_from(scale_1000).ok()?,
        width: i32::try_from(width).ok()?,
        height: i32::try_from(height).ok()?,
        pad_x: i32::try_from((region_width - width) / 2).ok()?,
        pad_y: i32::try_from((region_height - height) / 2).ok()?,
    })
}

/// Preserve both integer-normalization stages and native `(-3,-2)` marker offset.
pub(crate) fn mmpb_marker_anchor(
    point: ProjectedPoint,
    bounds: ProjectedPlayfieldBounds,
    region: MmpbRegionRect,
    fit: PreviewAspectFit,
) -> Option<MmpbMarkerAnchor> {
    if bounds.extent_x <= 0 || bounds.extent_y <= 0 {
        return None;
    }
    let fraction_x = (i64::from(point.x) - i64::from(bounds.min_x)) * MARKER_FRACTION_SCALE
        / i64::from(bounds.extent_x);
    let fraction_y = (i64::from(point.y) - i64::from(bounds.min_y)) * MARKER_FRACTION_SCALE
        / i64::from(bounds.extent_y);
    let local_x = i64::from(fit.pad_x)
        + fraction_x * i64::from(fit.width) / MARKER_FRACTION_SCALE
        + MMPB_OFFSET_X;
    let local_y = i64::from(fit.pad_y)
        + fraction_y * i64::from(fit.height) / MARKER_FRACTION_SCALE
        + MMPB_OFFSET_Y;
    Some(MmpbMarkerAnchor {
        local_x: i32::try_from(local_x).ok()?,
        local_y: i32::try_from(local_y).ok()?,
        screen_x: i32::try_from(i64::from(region.x) + local_x).ok()?,
        screen_y: i32::try_from(i64::from(region.y) + local_y).ok()?,
    })
}

/// Build markers by start lookup, never by zipping participant and waypoint lists.
pub(crate) fn build_mmpb_marker_records(
    prefix: &[Waypoint],
    assignments: &[LoadingStartAssignment],
    bounds: ProjectedPlayfieldBounds,
    region: MmpbRegionRect,
    fit: PreviewAspectFit,
) -> Vec<MmpbMarkerRecord> {
    prefix
        .iter()
        .filter_map(|waypoint| {
            let assignment = assignments
                .iter()
                .find(|assignment| assignment.start_index == waypoint.index)?;
            let anchor = mmpb_marker_anchor(project_waypoint(*waypoint), bounds, region, fit)?;
            Some(MmpbMarkerRecord {
                start_index: waypoint.index,
                waypoint: *waypoint,
                participant: assignment.participant,
                color_priority: assignment.color_priority,
                anchor,
            })
        })
        .collect()
}

/// Burn clipped opaque-black `4x4` source rectangles before preview aspect fitting.
///
/// Returns the number of indicators that wrote at least one in-bounds pixel.
pub(crate) fn burn_black_start_indicators(
    preview: &mut DecodedPreview,
    prefix: &[Waypoint],
    bounds: ProjectedPlayfieldBounds,
) -> usize {
    if !valid_preview_buffer(preview) {
        return 0;
    }
    let mut written = 0;
    for waypoint in prefix {
        let point = project_waypoint(*waypoint);
        let x = (i64::from(point.x) - i64::from(bounds.min_x)) * 2 - 1;
        let y = i64::from(point.y) - i64::from(bounds.min_y) - 1;
        if fill_black_rect_clipped(preview, x, y) {
            written += 1;
        }
    }
    written
}

fn valid_preview_buffer(preview: &DecodedPreview) -> bool {
    let Some(pixel_count) = usize::try_from(preview.width).ok().and_then(|width| {
        usize::try_from(preview.height)
            .ok()
            .and_then(|height| width.checked_mul(height))
    }) else {
        return false;
    };
    pixel_count
        .checked_mul(4)
        .is_some_and(|byte_count| byte_count == preview.rgba.len())
        && preview.width != 0
        && preview.height != 0
}

fn fill_black_rect_clipped(preview: &mut DecodedPreview, x: i64, y: i64) -> bool {
    let width = i64::from(preview.width);
    let height = i64::from(preview.height);
    let mut wrote = false;
    for offset_y in 0..START_INDICATOR_SIZE {
        let pixel_y = y + offset_y;
        if !(0..height).contains(&pixel_y) {
            continue;
        }
        for offset_x in 0..START_INDICATOR_SIZE {
            let pixel_x = x + offset_x;
            if !(0..width).contains(&pixel_x) {
                continue;
            }
            let index = ((pixel_y as usize * preview.width as usize) + pixel_x as usize) * 4;
            if let Some(pixel) = preview.rgba.get_mut(index..index + 4) {
                pixel.copy_from_slice(&[0, 0, 0, 255]);
                wrote = true;
            }
        }
    }
    wrote
}

fn native_uppercase_special_unit(value: &str) -> String {
    let units: Vec<u16> = value
        .encode_utf16()
        .map(|unit| {
            if (0x61..=0x7A).contains(&unit) || (0xE0..=0xFE).contains(&unit) {
                unit - 0x20
            } else {
                unit
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

fn composition_layers(
    has_preview: bool,
    has_markers: bool,
    text: &LocalizedLoadingTextSnapshot,
) -> Vec<LoadingCompositionLayer> {
    let mut layers = Vec::with_capacity(13);
    layers.push(LoadingCompositionLayer::Background);
    if has_preview {
        layers.push(LoadingCompositionLayer::PreviewWithBlackStartIndicators);
    }
    if has_markers {
        layers.push(LoadingCompositionLayer::AssignedMmpbMarkers);
    }
    if text.country_name.is_some() {
        layers.push(LoadingCompositionLayer::CountryBacking);
        layers.push(LoadingCompositionLayer::CountryText);
    }
    if text.special_unit.is_some() {
        layers.push(LoadingCompositionLayer::SpecialUnitText);
    }
    if text.load_brief.is_some() {
        layers.push(LoadingCompositionLayer::BriefingBacking);
        layers.push(LoadingCompositionLayer::BriefingText);
    }
    if text.loading.is_some() {
        layers.push(LoadingCompositionLayer::LoadingBacking);
        layers.push(LoadingCompositionLayer::LoadingText);
    }
    layers.push(LoadingCompositionLayer::ProgressBacking);
    layers.push(LoadingCompositionLayer::ProgressBar);
    layers.push(LoadingCompositionLayer::ProgressSideIcon);
    layers.push(LoadingCompositionLayer::ProgressLabel);
    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(override_file: &str) -> SkirmishLaunchMode {
        SkirmishLaunchMode {
            class: if override_file.eq_ignore_ascii_case("MPCoopMD.ini") {
                crate::skirmish_modes::MpModeClass::Cooperative
            } else {
                crate::skirmish_modes::MpModeClass::Battle
            },
            id: 0,
            ui_name_key: String::new(),
            tooltip_key: String::new(),
            override_file: override_file.to_owned(),
            map_filter: String::new(),
            random_maps_allowed: false,
            allies_allowed: false,
            must_ally: false,
        }
    }

    fn waypoint(index: u32, x: i16, y: i16) -> Waypoint {
        Waypoint {
            index,
            rx: x as u16,
            ry: y as u16,
        }
    }

    #[test]
    fn marker_region_uses_exact_width_equality() {
        assert_eq!(
            mmpb_region_rect(640),
            MmpbRegionRect::new(385, 270, 200, 200)
        );
        assert_eq!(
            mmpb_region_rect(800),
            MmpbRegionRect::new(499, 379, 216, 166)
        );
        assert_eq!(
            mmpb_region_rect(1024),
            MmpbRegionRect::new(570, 424, 300, 260)
        );
        assert_eq!(
            mmpb_region_rect(801),
            MmpbRegionRect::new(385, 270, 200, 200)
        );
        assert_eq!(
            mmpb_region_rect(1920),
            MmpbRegionRect::new(385, 270, 200, 200)
        );
    }

    #[test]
    fn waypoint_projection_sign_extends_and_truncates_toward_zero() {
        assert_eq!(
            project_waypoint(waypoint(0, -1, -1)),
            ProjectedPoint { x: 256, y: 0 }
        );
        assert_eq!(
            project_waypoint(waypoint(0, -1, 0)),
            ProjectedPoint { x: 255, y: 0 }
        );
    }

    #[test]
    fn aspect_fit_and_two_stage_marker_fixture_match_verified_values() {
        let region = mmpb_region_rect(800);
        let fit = aspect_fit_preview(region, 200, 80).expect("valid fit");
        assert_eq!(
            fit,
            PreviewAspectFit {
                scale_1000: 1080,
                width: 216,
                height: 86,
                pad_x: 0,
                pad_y: 40,
            }
        );
        let anchor = mmpb_marker_anchor(
            ProjectedPoint { x: 35, y: 30 },
            ProjectedPlayfieldBounds {
                min_x: 10,
                min_y: 10,
                extent_x: 100,
                extent_y: 80,
            },
            region,
            fit,
        )
        .expect("positive projected extents");
        assert_eq!(
            anchor,
            MmpbMarkerAnchor {
                local_x: 51,
                local_y: 59,
                screen_x: 550,
                screen_y: 438,
            }
        );
    }

    #[test]
    fn sparse_waypoints_use_valid_count_then_numeric_prefix() {
        let waypoints = HashMap::from([
            (0, waypoint(0, 0, 0)),
            (1, waypoint(1, 1, 1)),
            (4, waypoint(4, 4, 4)),
            (5, waypoint(5, 5, 5)),
        ]);
        let prefix = native_loading_waypoint_prefix(&waypoints);
        assert_eq!(
            prefix
                .iter()
                .map(|waypoint| waypoint.index)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn black_source_indicator_clips_a_four_by_four_rect() {
        let mut preview = DecodedPreview {
            width: 3,
            height: 3,
            rgba: vec![255; 3 * 3 * 4],
        };
        let start = waypoint(0, 0, 0);
        let point = project_waypoint(start);
        let written = burn_black_start_indicators(
            &mut preview,
            &[start],
            ProjectedPlayfieldBounds {
                min_x: point.x,
                min_y: point.y,
                extent_x: 1,
                extent_y: 1,
            },
        );
        assert_eq!(written, 1);
        assert_eq!(preview.rgba, [0, 0, 0, 255].repeat(9));
    }

    #[test]
    fn allocated_country_has_no_stock_text_identity() {
        let csf = test_csf(&[
            ("Name:YuriCountry", "Yuri"),
            ("Name:YURI", "Yuri Clone"),
            ("LoadBrief:YuriCountry", "A stock briefing"),
            ("GUI:LoadingEx", "Loading..."),
        ]);
        for index in [10, 13, 14] {
            let country = LaunchCountry::from_country_index(index);
            let text = localize_loading_text(&csf, country);
            assert!(text.keys.is_none());
            assert!(text.country_name.is_none());
            assert!(text.special_unit.is_none());
            assert!(text.load_brief.is_none());
            assert_eq!(text.loading.as_deref(), Some("Loading..."));
        }
    }

    #[test]
    fn country_key_table_contains_every_verified_load_brief_key() {
        let countries = [
            LaunchCountry::America,
            LaunchCountry::Korea,
            LaunchCountry::France,
            LaunchCountry::Germany,
            LaunchCountry::GreatBritain,
            LaunchCountry::Libya,
            LaunchCountry::Iraq,
            LaunchCountry::Cuba,
            LaunchCountry::Russia,
            LaunchCountry::Yuri,
        ];
        let briefs = countries
            .map(loading_country_text_keys)
            .map(|keys| keys.unwrap().load_brief);
        assert_eq!(
            briefs,
            [
                "LoadBrief:USA",
                "LoadBrief:Korea",
                "LoadBrief:French",
                "LoadBrief:Germans",
                "LoadBrief:British",
                "LoadBrief:Lybia",
                "LoadBrief:Iraq",
                "LoadBrief:Cuba",
                "LoadBrief:Russia",
                "LoadBrief:YuriCountry",
            ]
        );
    }

    #[test]
    fn localized_snapshot_uses_csf_and_uppercases_only_special_unit() {
        let csf = test_csf(&[
            ("Name:Americans", "America"),
            ("Name:Para", "paradrop"),
            ("LoadBrief:USA", "A briefing."),
            ("GUI:LoadingEx", "Loading..."),
        ]);
        let text = localize_loading_text(&csf, LaunchCountry::America);
        assert_eq!(text.country_name.as_deref(), Some("America"));
        assert_eq!(text.special_unit.as_deref(), Some("PARADROP"));
        assert_eq!(text.load_brief.as_deref(), Some("A briefing."));
        assert_eq!(text.loading.as_deref(), Some("Loading..."));
    }

    fn test_launch_session() -> SkirmishLaunchSession {
        use crate::skirmish_launch::{
            LaunchStartPosition, LaunchTeam, SkirmishLaunchOptions, SkirmishLocalSlot,
        };
        SkirmishLaunchSession {
            mode: mode("MPBattleMD.ini"),
            selected_map_file: Some(RANDMAP_SED_FIXTURE.to_owned()),
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".to_owned(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(0),
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 0,
            opponents: Vec::new(),
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(0),
            options: SkirmishLaunchOptions::default(),
        }
    }

    const RANDMAP_SED_FIXTURE: &str = "RandMap.Sed";

    #[test]
    fn base_origin_centers_the_art_viewport_for_each_native_width_branch() {
        // Exactly 640 selects the 640x480 art; every other width uses 800x600.
        assert_eq!(loading_base_origin([640, 480]), [0, 0]);
        assert_eq!(loading_base_origin([800, 600]), [0, 0]);
        assert_eq!(loading_base_origin([1024, 768]), [112, 84]);
        assert_eq!(loading_base_origin([1920, 1080]), [560, 240]);
        assert_eq!(loading_base_origin([640, 800]), [0, 160]);
    }

    #[test]
    fn gsi_03_09_random_preview_aspect_fits_without_invented_starts() {
        let image = DecodedPreview {
            width: 200,
            height: 80,
            rgba: vec![255; 200 * 80 * 4],
        };
        let map =
            crate::map::rmg::emit::empty_map_file(&crate::map::rmg::RmgOptions::default(), 32, 32);
        let composition = build_random_map_loading_composition(
            &test_launch_session(),
            None,
            [800, 600],
            ShellScale::RETAIL,
            Some(image),
            &map,
            &map.waypoints,
            &[],
        );

        let preview = composition.preview.expect("valid bitmap fits the region");
        assert_eq!(preview.region, mmpb_region_rect(800));
        assert_eq!(
            preview.fit,
            aspect_fit_preview(mmpb_region_rect(800), 200, 80).expect("valid fit")
        );
        // Without optional preview map data, the loader never invents starts.
        assert!(preview.image.rgba.iter().all(|byte| *byte == 255));
        assert!(composition.markers.is_empty());
    }

    #[test]
    fn start_staging_counts_the_leading_starts_and_keeps_every_cell() {
        use crate::map::map_file::MapCell;

        let mut map = crate::map::rmg::emit::empty_map_file(
            &crate::map::rmg::RmgOptions::default(),
            100,
            100,
        );
        let empty = scenario_start_staging(&map, &[(0, 70, 70)]);
        assert_eq!(
            (empty.start_x, empty.start_y, empty.width, empty.height),
            (10_000, 10_000, -10_000, -10_000)
        );
        let cell = |rx, ry| MapCell {
            rx,
            ry,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        };
        map.cells = vec![cell(60, 60), cell(60, 140), cell(140, 60)];
        // Slot 2 is undefined: the count and pairs stop there, the cells do not.
        let staging = scenario_start_staging(&map, &[(3, 100, 100), (0, 70, 70), (1, 90, 80)]);
        assert_eq!(staging.count, 2);
        assert_eq!(staging.pairs[0], (256, 70));
        assert_eq!(staging.pairs[2], (0, 0));
        assert_eq!(staging.pairs[3], (0, 0));
        assert_eq!(staging.cells[..4], [(70, 70), (90, 80), (0, 0), (100, 100)]);
        let bounds = projected_playfield_bounds(&map).expect("inside cells");
        assert_eq!(
            (
                staging.start_x,
                staging.start_y,
                staging.width,
                staging.height
            ),
            (bounds.min_x, bounds.min_y, bounds.extent_x, bounds.extent_y)
        );
    }

    #[test]
    fn loading_composition_ignores_size_filler_outside_normalized_local_size() {
        use crate::map::map_file::MapCell;

        let mut map = crate::map::rmg::emit::empty_map_file(
            &crate::map::rmg::RmgOptions::default(),
            100,
            100,
        );
        let cell = |rx, ry| MapCell {
            rx,
            ry,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        };
        map.cells = vec![cell(60, 60), cell(60, 140), cell(140, 60)];
        map.waypoints = HashMap::from([(0, waypoint(0, 70, 70))]);
        map.preview.decoded = Some(DecodedPreview {
            width: 300,
            height: 100,
            rgba: vec![255; 300 * 100 * 4],
        });
        let assignments = [LoadingStartAssignment {
            start_index: 0,
            participant: LoadingParticipantId::Local,
            color_priority: 3,
        }];
        let session = test_launch_session();

        let baseline_bounds =
            projected_playfield_bounds(&map).expect("inside cells establish bounds");
        let baseline = build_loading_composition(
            &map,
            &session,
            None,
            [800, 600],
            ShellScale::RETAIL,
            &assignments,
        );

        // (55,55) is inside the RMG Size diamond (104 < x+y) but below the
        // normalized LocalSize near edge (114 < x+y), so native mode zero skips it.
        map.cells.push(cell(55, 55));
        assert_eq!(projected_playfield_bounds(&map), Some(baseline_bounds));
        let with_filler = build_loading_composition(
            &map,
            &session,
            None,
            [800, 600],
            ShellScale::RETAIL,
            &assignments,
        );
        assert_eq!(with_filler.markers, baseline.markers);

        // A genuinely in-playfield cell still extends both projected bounds and
        // moves the same loading marker after native normalization.
        map.cells.push(cell(150, 60));
        let extended_bounds = projected_playfield_bounds(&map).expect("inside extender");
        assert_ne!(extended_bounds, baseline_bounds);
        let with_inside = build_loading_composition(
            &map,
            &session,
            None,
            [800, 600],
            ShellScale::RETAIL,
            &assignments,
        );
        assert_ne!(with_inside.markers[0].anchor, baseline.markers[0].anchor);
    }

    #[test]
    fn gsi_03_09_retained_random_first_frame_has_black_starts_markers_text_and_name() {
        use crate::map::map_file::MapCell;

        let mut map = crate::map::rmg::emit::empty_map_file(
            &crate::map::rmg::RmgOptions::default(),
            100,
            100,
        );
        map.cells = vec![
            MapCell {
                rx: 60,
                ry: 60,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 60,
                ry: 140,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 140,
                ry: 60,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
        ];
        map.waypoints = HashMap::from([
            (0, waypoint(0, 70, 70)),
            (1, waypoint(1, 100, 70)),
            (2, waypoint(2, 70, 100)),
        ]);
        let assignments = [
            LoadingStartAssignment {
                start_index: 0,
                participant: LoadingParticipantId::Local,
                color_priority: 3,
            },
            LoadingStartAssignment {
                start_index: 1,
                participant: LoadingParticipantId::Opponent(0),
                color_priority: 6,
            },
        ];
        let csf = test_csf(&[
            ("Name:Americans", "America"),
            ("Name:Para", "paradrop"),
            ("LoadBrief:USA", "A briefing."),
            ("GUI:LoadingEx", "Loading..."),
        ]);
        let image = DecodedPreview {
            width: 300,
            height: 100,
            rgba: vec![255; 300 * 100 * 4],
        };
        let session = test_launch_session();
        let composition = build_random_map_loading_composition(
            &session,
            Some(&csf),
            [800, 600],
            ShellScale::RETAIL,
            Some(image),
            &map,
            &map.waypoints,
            &assignments,
        );

        let preview = composition.preview.expect("retained preview");
        assert_eq!(
            preview
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| **pixel == [0, 0, 0, 255])
                .count(),
            3 * 4 * 4,
            "both assigned starts and the unassigned start are burned black"
        );
        assert_eq!(composition.markers.len(), 2);
        assert_eq!(
            composition.markers[0],
            MmpbMarkerRecord {
                anchor: composition.markers[0].anchor,
                start_index: 0,
                waypoint: waypoint(0, 70, 70),
                participant: LoadingParticipantId::Local,
                color_priority: 3,
            }
        );
        assert_eq!(composition.markers[1].start_index, 1);
        assert_eq!(
            composition.markers[1].participant,
            LoadingParticipantId::Opponent(0)
        );
        assert_eq!(composition.markers[1].color_priority, 6);
        assert!(
            composition
                .markers
                .iter()
                .all(|marker| marker.start_index != 2)
        );
        assert_eq!(composition.text.country_name.as_deref(), Some("America"));
        assert_eq!(composition.text.special_unit.as_deref(), Some("PARADROP"));
        assert_eq!(composition.text.load_brief.as_deref(), Some("A briefing."));
        assert_eq!(composition.text.loading.as_deref(), Some("Loading..."));
        assert!(
            composition
                .layers
                .contains(&LoadingCompositionLayer::PreviewWithBlackStartIndicators)
        );
        assert!(
            composition
                .layers
                .contains(&LoadingCompositionLayer::AssignedMmpbMarkers)
        );
        assert_eq!(
            crate::app::loading::progress_row::LoadingProgressRowSnapshot::from_launch_session(
                &session
            )
            .local_label(),
            "Player"
        );
    }

    #[test]
    fn text_rects_cover_640_centered_800_plus_and_cooperative() {
        let battle = mode("MPBattleMD.ini");
        let cooperative = mode("MPCoopMD.ini");
        assert_eq!(
            loading_text_rects([640, 480], &cooperative, ShellScale::RETAIL).load_brief,
            RectPx::new(16, 126, 318, 104)
        );
        assert_eq!(
            loading_text_rects([800, 600], &battle, ShellScale::RETAIL),
            LoadingTextRects {
                country_name: RectPx::new(540, 310, 200, 20),
                special_unit: RectPx::new(20, 90, 200, 20),
                load_brief: RectPx::new(20, 158, 398, 130),
                loading: RectPx::new(20, 300, 200, 20),
            }
        );
        assert_eq!(
            loading_text_rects([1024, 768], &cooperative, ShellScale::RETAIL).load_brief,
            RectPx::new(132, 464, 398, 130)
        );
        assert_eq!(
            loading_text_rects([799, 600], &battle, ShellScale::RETAIL).country_name,
            RectPx::new(540, 310, 200, 20)
        );
    }

    #[test]
    fn a_stretched_loading_screen_grows_one_authored_dialog_into_the_window() {
        use crate::app::loading::campaign_presentation::CampaignLoadingLayout;
        use crate::app::loading::progress_row::{
            LoadingProgressRowPlacement, layout_loading_progress_row,
        };

        let battle = mode("MPBattleMD.ini");
        let scale = ShellScale::from_screen(1600, 1200);
        let authored = loading_text_rects([800, 600], &battle, ShellScale::RETAIL);

        // Every layer places itself in the 800x600 dialog, then maps through the
        // page scale, so the copy stays on the art it labels at any window size.
        assert_eq!(
            loading_text_rects([1600, 1200], &battle, scale),
            LoadingTextRects {
                country_name: RectPx::new(1080, 620, 400, 40),
                special_unit: RectPx::new(40, 180, 400, 40),
                load_brief: RectPx::new(40, 316, 796, 260),
                loading: RectPx::new(40, 600, 400, 40),
            }
        );
        assert_eq!(
            mmpb_region_rect(800).scaled(scale),
            MmpbRegionRect::new(998, 758, 432, 332)
        );
        let row = layout_loading_progress_row(
            LoadingProgressRowPlacement::StandardSkirmish([1600, 1200]),
            [80, 5],
            Some([47, 23]),
            [0, 12],
            scale,
            0,
        );
        assert_eq!(row.bar_origin, [48, 664]);
        assert_eq!(row.icon_origin, Some([234, 646]));
        assert_eq!(row.label_rect, RectPx::new(348, 656, 490, 24));
        let authored_chrome = CampaignLoadingLayout::for_authored_size([800, 600]);
        let chrome = authored_chrome.scaled(scale);
        assert_eq!(chrome.title, RectPx::new(0, 0, 1600, 80));
        assert_eq!(chrome.body, RectPx::new(0, 80, 1600, 1040));
        assert_eq!(chrome.bar, RectPx::new(0, 1120, 1600, 80));
        assert_eq!(chrome.progress_point, [328, 1134]);

        // gamemd's own mapping is untouched by the fit: the same dialog is
        // centered at 1:1 and the identity scale leaves each layer where it is.
        let centered = loading_text_rects([1600, 1200], &battle, ShellScale::RETAIL);
        assert_eq!(
            centered.country_name,
            authored.country_name.translate(400, 300)
        );
        assert_eq!(centered.load_brief, authored.load_brief.translate(400, 300));
        assert_eq!(
            CampaignLoadingLayout::for_authored_size([1600, 1200]).title,
            RectPx::new(400, 300, 800, 40)
        );
    }

    #[test]
    fn ordered_layers_keep_text_between_markers_and_progress() {
        let text = LocalizedLoadingTextSnapshot {
            keys: loading_country_text_keys(LaunchCountry::America),
            country_name: Some("America".to_owned()),
            special_unit: Some("PARADROP".to_owned()),
            load_brief: Some("Brief".to_owned()),
            loading: Some("Loading...".to_owned()),
        };
        assert_eq!(
            composition_layers(true, true, &text),
            vec![
                LoadingCompositionLayer::Background,
                LoadingCompositionLayer::PreviewWithBlackStartIndicators,
                LoadingCompositionLayer::AssignedMmpbMarkers,
                LoadingCompositionLayer::CountryBacking,
                LoadingCompositionLayer::CountryText,
                LoadingCompositionLayer::SpecialUnitText,
                LoadingCompositionLayer::BriefingBacking,
                LoadingCompositionLayer::BriefingText,
                LoadingCompositionLayer::LoadingBacking,
                LoadingCompositionLayer::LoadingText,
                LoadingCompositionLayer::ProgressBacking,
                LoadingCompositionLayer::ProgressBar,
                LoadingCompositionLayer::ProgressSideIcon,
                LoadingCompositionLayer::ProgressLabel,
            ]
        );
    }

    fn test_csf(entries: &[(&str, &str)]) -> CsfFile {
        const HEADER_MAGIC: u32 = 0x4353_4620;
        const LABEL_MAGIC: u32 = 0x4C42_4C20;
        const STRING_MAGIC: u32 = 0x5354_5220;

        let mut data = Vec::new();
        data.extend_from_slice(&HEADER_MAGIC.to_le_bytes());
        data.extend_from_slice(&3_u32.to_le_bytes());
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        data.extend_from_slice(&0_u16.to_le_bytes());
        data.extend_from_slice(&[0_u8; 6]);
        for (key, value) in entries {
            data.extend_from_slice(&LABEL_MAGIC.to_le_bytes());
            data.extend_from_slice(&1_u32.to_le_bytes());
            data.extend_from_slice(&(key.len() as u32).to_le_bytes());
            data.extend_from_slice(key.as_bytes());
            data.extend_from_slice(&STRING_MAGIC.to_le_bytes());
            data.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
            data.extend(
                value
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .map(|byte| !byte),
            );
        }
        CsfFile::from_bytes(&data).expect("valid test CSF")
    }
}
