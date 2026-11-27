//! Sidebar drawing — chrome, cameos, the observer strip and the texts.
//!
//! The chrome is drawn by its nodes on the match's chrome layer, its
//! buttons by the nodes that take their clicks ([`ChromeHud`]); the observer
//! strip and every text (the cameos' labels, the observer strip's and the
//! money counter) are nodes of retained trees ([`PictureHud`], [`TextHud`]);
//! the cameos, their clocks and strips are built as instance vectors.
//!
//! Split from `presentation::sidebar_render` to keep files under 400 lines.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::frontend::state::FrontendState;
use crate::app::input::state::MatchInputState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::picture_hud::{ChromeHud, ChromePieces, PictureHud, covering};
use crate::app::presentation::sidebar_render::{current_sidebar_chrome, current_sidebar_view};
use crate::app::presentation::text_hud::TextHud;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::{BitFont, Fit};
use crate::render::sidebar_chrome::SidebarChromeEntry;
use crate::ui::sidebar::power_bar_anim::PowerBarAnimState;
use crate::ui::sidebar::{
    ArtKey, PartKind, Rect, SidebarChromeLayoutSpec, SidebarLayout, SidebarPart, SidebarView,
};

#[path = "sidebar_command_bar.rs"]
pub(crate) mod command_bar;

// ---------------------------------------------------------------------------
// Chrome art instances
// ---------------------------------------------------------------------------

/// The chrome's drawings on the match's chrome layer, kept from frame to
/// frame: the sidebar column's and the command bar's.
#[derive(Default)]
pub(crate) struct ChromePaint {
    sidebar: ChromeHud,
    bar: ChromeHud,
}

/// The sidebar column's chrome this frame, drawn by its nodes on the match's
/// chrome layer, the buttons by theirs: none without a view or the atlas.
pub(crate) fn build_sidebar_chrome_instances(
    frontend: &FrontendState,
    match_state: &mut MatchState,
) -> Vec<SpriteInstance> {
    if current_sidebar_view(&match_state.match_presentation).is_none()
        || current_sidebar_chrome(frontend, match_state).is_none()
    {
        return Vec::new();
    }
    let Some(nodes) =
        crate::app::input::chrome_input::paint_nodes(&mut match_state.match_presentation)
    else {
        return Vec::new();
    };
    let presentation = &mut match_state.match_presentation;
    let (order, drawn) =
        sidebar_paint_order(&nodes.sidebar_parts, presentation.sidebar_layout_spec);
    let mut hud = std::mem::take(&mut presentation.chrome_paint.sidebar);
    let parts = &nodes.sidebar_parts;
    let mut frame = hud.frame(presentation.dialog_ui.gui_mut(), nodes.tree, parts, parts);
    if let (Some(view), Some(atlas)) = (
        current_sidebar_view(&match_state.match_presentation),
        current_sidebar_chrome(frontend, match_state),
    ) {
        let presentation = &match_state.match_presentation;
        let spec = presentation.sidebar_layout_spec;
        draw_sidebar_chrome(
            &mut frame,
            &|part: SidebarPart, key: ArtKey, index: usize| atlas.art(part, key, index),
            spec,
            view,
            &presentation.power_bar_anim,
            [match_state.input.camera_x, match_state.input.camera_y],
            presentation.radar_anim.is_none(),
        );
    }
    let presentation = &mut match_state.match_presentation;
    let instances = hud
        .finish(presentation.dialog_ui.gui_mut(), &order[..drawn])
        .to_vec();
    presentation.chrome_paint.sidebar = hud;
    instances
}

/// The sidebar's part nodes `parts` (by `SidebarPart as usize`) in the
/// order they draw in, the layout's (`spec.parts`), and how many: a part the
/// layout leaves out draws nothing, its button taking the pointer still.
pub(crate) fn sidebar_paint_order(
    parts: &[vera20k_gui::NodeId; 19],
    spec: SidebarChromeLayoutSpec,
) -> ([vera20k_gui::NodeId; 19], usize) {
    let mut order = *parts;
    let mut drawn = 0;
    for part in spec.parts_in_order() {
        order[drawn] = parts[part as usize];
        drawn += 1;
    }
    (order, drawn)
}

/// The bottom command bar, which shares the chrome atlas but draws into a
/// surface of its own (`presentation::sidebar_surface`), drawn by its nodes
/// on the match's chrome layer, its buttons by theirs.
pub(crate) fn build_command_bar_instances(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
) -> Vec<SpriteInstance> {
    if current_sidebar_chrome(frontend, match_state).is_none() {
        return Vec::new();
    }
    let bar = command_bar::layout_and_slots(platform, renderer, frontend, match_state, persistence);
    let Some(nodes) =
        crate::app::input::chrome_input::paint_nodes(&mut match_state.match_presentation)
    else {
        return Vec::new();
    };
    let slots = bar.as_ref().map_or(&[][..], |(_, slots)| slots.as_slice());
    let (order, drawn) = command_bar::paint_order(&nodes, slots);
    let presentation = &mut match_state.match_presentation;
    let mut hud = std::mem::take(&mut presentation.chrome_paint.bar);
    let mut frame = hud.frame(
        presentation.dialog_ui.gui_mut(),
        nodes.tree,
        &nodes.bar_parts,
        &nodes.bar_buttons,
    );
    if let (Some((layout, slots)), Some(atlas)) =
        (&bar, current_sidebar_chrome(frontend, match_state))
    {
        let camera = [match_state.input.camera_x, match_state.input.camera_y];
        command_bar::draw(
            &mut frame,
            &atlas.command_bar,
            *layout,
            slots,
            &match_state.match_presentation.sidebar_gadget_state,
            camera,
        );
    }
    let presentation = &mut match_state.match_presentation;
    let instances = hud
        .finish(presentation.dialog_ui.gui_mut(), &order[..drawn])
        .to_vec();
    presentation.chrome_paint.bar = hud;
    instances
}

/// The column's chrome into `inst`, in drawing order: each part of the
/// layout's list, its pictures from `art` (the atlas's `part`, `key`,
/// frame); a button part is one button, drawn or not.
pub(crate) fn draw_sidebar_chrome(
    inst: &mut impl ChromePieces,
    art: &impl Fn(SidebarPart, ArtKey, usize) -> Option<SidebarChromeEntry>,
    spec: SidebarChromeLayoutSpec,
    view: &SidebarView,
    power_bar_anim: &PowerBarAnimState,
    camera_offset: [f32; 2],
    static_radar_fallback: bool,
) {
    let layout = &view.layout;
    let d = 0.00048;
    let td = d - 0.00001;
    let btn_depth = d - 0.00002;
    // One part after another in the layout's order, so a later part lies over
    // an earlier one; a part the layout leaves out draws nothing.
    for part in spec.parts_in_order() {
        // Each part a named node of the chrome, its pieces its children.
        inst.part(part as usize);
        match part.kind() {
            // DrawCreditsSHPBackground 6D0E60, RadarClass::Draw 653100 and
            // SIDE1: the first frame at the part's corner.
            PartKind::Image => {
                if let Some(entry) = art(part, ArtKey::Art, 0) {
                    let [x, y] = layout.origin(&spec, part);
                    push_chrome(inst, entry, x, y, d, camera_offset);
                }
            }
            // 653010/653100: draw and hit-test use the same loaded SHP
            // canvas. `shape_button_frame` (ShapeButtonClass::Draw
            // 0x0069DEB0) picks the frame through SidebarGadgetState; the
            // scroll pair's 3-frame convention (0 = idle, 1 = pressed,
            // 2 = disabled) comes from SidebarGadgetState::scroll_*_frame.
            // Every button but the top two falls back to its first frame when
            // the MIX lacks the one asked for.
            PartKind::Button | PartKind::Tab => {
                let piece = view.button(part).and_then(|(rect, frame)| {
                    let frame = usize::from(frame);
                    let top = matches!(part, SidebarPart::Diplomacy | SidebarPart::Options);
                    let entry = art(part, ArtKey::Art, frame)
                        .or_else(|| art(part, ArtKey::Art, 0).filter(|_| !top))?;
                    let depth = if part.kind() == PartKind::Tab {
                        td
                    } else {
                        btn_depth
                    };
                    Some(chrome_piece(
                        entry,
                        rect.x,
                        rect.y,
                        entry.pixel_size,
                        depth,
                        camera_offset,
                    ))
                });
                // The button draws itself, or nothing this frame.
                inst.button(part as usize, piece);
            }
            PartKind::Radar => {
                let radar = art(part, ArtKey::Art, 0);
                if let Some(radar) = radar.filter(|_| static_radar_fallback) {
                    let [x, y] = layout.origin(&spec, part);
                    push_chrome(inst, radar, x, y, d, camera_offset);
                }
            }
            PartKind::CameoGrid => {
                draw_cameo_rows(inst, art, spec, layout, part, camera_offset, d);
            }
            // powerp.shp strips stacked from the top.
            PartKind::PowerBar => {
                render_power_bar(inst, art, spec, layout, power_bar_anim, camera_offset, d)
            }
            // The credits number is text, drawn with the sidebar's labels;
            // the map picture is drawn with the radar's.
            PartKind::Text | PartKind::Minimap | PartKind::Panel => {}
        }
    }
}

/// The cameo strip's SIDE2 rows, then SIDE3 and ADDON under the last.
///
/// 6A6C30: row count is native capacity; pixel advance is SIDE2's loaded
/// canvas height. SIDE3 and ADDON follow the last actual tile.
fn draw_cameo_rows(
    inst: &mut impl ChromePieces,
    art: &impl Fn(SidebarPart, ArtKey, usize) -> Option<SidebarChromeEntry>,
    spec: SidebarChromeLayoutSpec,
    layout: &SidebarLayout,
    part: SidebarPart,
    camera_offset: [f32; 2],
    d: f32,
) {
    let cx = layout.origin(&spec, part)[0];
    let row_art = art(part, ArtKey::RowArt, 0);
    if let Some(side2) = row_art.filter(|_| spec.columns() <= 2) {
        for row in 0..layout.side2_tile_count {
            let y = layout.cameo_grid_top + row as f32 * side2.pixel_size[1];
            push_chrome(inst, side2, cx, y, d, camera_offset);
        }
    } else if let Some(side2) = row_art {
        // VERA's columns: SIDE2 paints the original's two button frames, so
        // a row of three to five repeats one of them per cell, scaled with
        // the cameo it holds, instead of stretching the painted pair out of
        // shape. The panel's own edges keep their width, so the border runs
        // unbroken down the strip.
        let art = side2.pixel_size;
        let row_h = spec.cameo_row_height.max(1.0);
        let inset = spec.cameo_inset_x;
        let pitch = spec.cameo_column_pitch.max(1.0);
        let cell = spec
            .cameo_cell_source_width
            .min((art[0] - inset).max(1.0))
            .max(1.0);
        let rail = (art[0] - inset - 2.0 * cell).max(0.0);
        let cell_art = side2.columns(inset, cell);
        let left_edge = side2.columns(0.0, inset);
        let right_edge = side2.columns(art[0] - rail, rail);
        for row in 0..layout.side2_tile_count {
            let y = layout.cameo_grid_top + row as f32 * row_h;
            for col in 0..spec.columns() {
                let x = cx + inset + col as f32 * pitch;
                push_chrome_sized(inst, cell_art, x, y, [pitch, row_h], d, camera_offset);
            }
            push_chrome_sized(inst, left_edge, cx, y, [inset, row_h], d, camera_offset);
            if rail > 0.0 {
                let x = cx + art[0] - rail;
                push_chrome_sized(inst, right_edge, x, y, [rail, row_h], d, camera_offset);
            }
        }
    }
    let mut y = layout.side3_y;
    for key in [ArtKey::EndArt, ArtKey::FootArt] {
        if let Some(entry) = art(part, key, 0) {
            push_chrome(inst, entry, cx, y, d, camera_offset);
            y += entry.pixel_size[1];
        }
    }
}

/// Render the vertical power bar meter by stacking powerp.shp strip tiles.
///
/// Draws segments from top to bottom as PowerClass::Draw 63FB20 does
/// ([`PowerBarAnimState::visit_draw_segments`]): empty space (frame 0), the
/// blink (frame 4) in place of the first lit segment, then the surplus over
/// 100 (frame 1, +152C), the spare up to 100 (frame 2, +1530) and the drain
/// (frame 3, +1534).
///
/// Segment counts come from `PowerBarAnimState`, which moves them one at a
/// time toward their targets.
fn render_power_bar(
    inst: &mut impl ChromePieces,
    art: &impl Fn(SidebarPart, ArtKey, usize) -> Option<SidebarChromeEntry>,
    spec: SidebarChromeLayoutSpec,
    layout: &SidebarLayout,
    anim: &PowerBarAnimState,
    camera_offset: [f32; 2],
    base_depth: f32,
) {
    let [bar_x, bar_top] = layout.origin(&spec, SidebarPart::Power);
    let tile_h: f32 = spec.power_bar_tile_height;

    if tile_h <= 0.0 || anim.max_segments() <= 0 {
        return;
    }

    let fill_depth: f32 = base_depth - 0.00002;

    anim.visit_draw_segments(|frame, row| {
        if let Some(entry) = art(SidebarPart::Power, ArtKey::Art, usize::from(frame)) {
            push_chrome_sized(
                inst,
                entry,
                bar_x,
                bar_top + row as f32 * tile_h,
                entry.pixel_size,
                fill_depth,
                camera_offset,
            );
        }
    });
}

/// Ordinary gamemd chrome draws one render pixel per asset pixel;
/// 6A5090/6A5130 change row capacity on resize, never artwork scale.
fn push_chrome(
    pictures: &mut impl ChromePieces,
    entry: SidebarChromeEntry,
    x: f32,
    y: f32,
    depth: f32,
    camera_offset: [f32; 2],
) {
    push_chrome_sized(
        pictures,
        entry,
        x,
        y,
        entry.pixel_size,
        depth,
        camera_offset,
    );
}

fn push_chrome_sized(
    pictures: &mut impl ChromePieces,
    entry: SidebarChromeEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
    camera_offset: [f32; 2],
) {
    let (rect, instance) = chrome_piece(entry, x, y, size, depth, camera_offset);
    pictures.picture(rect, instance);
}

/// One piece over `size` at `(x, y)` of the layout space, and the whole
/// pixels it covers there, its node's rect.
fn chrome_piece(
    entry: SidebarChromeEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
    camera_offset: [f32; 2],
) -> (vera20k_gui::Rect, SpriteInstance) {
    (
        covering(x, y, size[0], size[1]),
        SpriteInstance {
            position: [x + camera_offset[0], y + camera_offset[1]],
            size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        },
    )
}

// ---------------------------------------------------------------------------
// Cameo icon instances
// ---------------------------------------------------------------------------
/// Horizontal padding around the ready text (each side, in native pixels).
pub(crate) const READY_PAD_X: f32 = 2.0;
/// Vertical padding around the ready text (each side, in native pixels).
pub(crate) const READY_PAD_Y: f32 = 1.0;

/// VERA's own: the [`Fit`] that keeps a cameo's status label on one line on
/// its cameo, `slot_w` less the strip's padding and less any queue `badge`
/// sharing its row, so that a longer translation squeezes instead of running
/// off the cameo or under the badge. Retail's labels fit at [`Fit::NONE`].
/// The label's strip and its text take the same fit.
pub(crate) fn status_label_fit(
    font: &BitFont,
    text: &str,
    slot_w: f32,
    badge: Option<&str>,
) -> Fit {
    let badge_w = badge.map_or(0.0, |badge| {
        font.text_width(badge) as f32 + QUEUE_COUNT_PAD_X
    });
    font.fit_line(text, (slot_w - READY_PAD_X * 2.0 - badge_w).max(1.0) as u32)
}

/// Horizontal padding for queue count badge (native pixels, matches ComputeTextRect x_pad=2).
const QUEUE_COUNT_PAD_X: f32 = 2.0;
/// Vertical padding for queue count badge (native pixels, matches ComputeTextRect y_pad=1).
const QUEUE_COUNT_PAD_Y: f32 = 1.0;

/// The CSF labels shared by the cameo's single status strip and its glyphs.
#[derive(Clone, Copy)]
pub(crate) struct CameoStatusLabels<'a> {
    pub ready: &'a str,
    pub hold: &'a str,
    pub charging: &'a str,
    pub active: &'a str,
    /// Ares: each type's localized `Text.*` labels ([`ares_status_texts`]);
    /// `None` unless Ares plays in the game (`RuleSet::ares`).
    pub ares: Option<&'a AresStatusTexts>,
}

/// Each SuperWeaponType's localized NameReadiness texts, keyed by its
/// upper-case section: Preparing, Hold, Ready, Charging, Active; an empty
/// label resolves to no text.
pub(crate) type AresStatusTexts = std::collections::HashMap<String, [String; 5]>;

/// Resolve every type's Ares `Text.*` labels through the shared CSF owner,
/// once per match (`install_world_presentation`).
pub(crate) fn ares_status_texts(
    rules: &crate::rules::ruleset::RuleSet,
    text: impl Fn(&str) -> String,
) -> AresStatusTexts {
    rules
        .super_weapons
        .values()
        .map(|sw| {
            let texts = sw.ares_texts();
            let labels = [
                texts.preparing(),
                texts.hold(),
                texts.ready(),
                texts.charging(),
                texts.active(),
            ];
            (
                sw.id.to_ascii_uppercase(),
                labels.map(|label| {
                    if label.is_empty() {
                        String::new()
                    } else {
                        text(label)
                    }
                }),
            )
        })
        .collect()
}

/// `SuperClass::NameReadiness6CC2B0` status label; `None` while it has no
/// status text. Shared by the right cameo strip and the left sidebar button.
/// When Ares plays in the game (`RuleSet::ares`, carried by `labels.ares`),
/// Ares's replacement (hook 6CC2B0, Ares.dll 100356A0) returns the type's
/// `Text.*` label, `Text.Preparing` without a status, and nothing for an
/// empty label.
pub(crate) fn superweapon_status_label<'a>(
    section: Option<&str>,
    display: crate::sim::superweapon::SuperWeaponDisplay,
    labels: CameoStatusLabels<'a>,
) -> Option<&'a str> {
    use crate::sim::superweapon::SuperWeaponStatus;
    if let (Some(texts), Some(section)) = (labels.ares, section)
        && let Some(texts) = texts.get(&section.to_ascii_uppercase())
    {
        let text = &texts[match display.status {
            None => 0,
            Some(SuperWeaponStatus::Hold) => 1,
            Some(SuperWeaponStatus::Ready) => 2,
            Some(SuperWeaponStatus::Charging) => 3,
            Some(SuperWeaponStatus::Active) => 4,
        }];
        return (!text.is_empty()).then_some(text.as_str());
    }
    display.status.map(|status| match status {
        SuperWeaponStatus::Hold => labels.hold,
        SuperWeaponStatus::Ready => labels.ready,
        SuperWeaponStatus::Charging => labels.charging,
        SuperWeaponStatus::Active => labels.active,
    })
}

/// The same status selection sizes the dark strip and draws its text.
fn cameo_status_text<'a>(
    item: &crate::ui::sidebar::SidebarItem,
    labels: CameoStatusLabels<'a>,
) -> Option<&'a str> {
    if let Some(display) = item.super_weapon_display {
        return superweapon_status_label(item.super_weapon_section.as_deref(), display, labels);
    }
    if item.is_ready {
        Some(labels.ready)
    } else if item.is_on_hold {
        Some(labels.hold)
    } else {
        None
    }
}

/// Whether the status label is the `[esp+0x40]` text drawn before the clock.
///
/// `StripClass::Draw` draws that slot, production `TXT_READY` and every
/// `SuperClass::NameReadiness` label, at `0x006A9D94..0x006A9E36` and only
/// then the clock at `0x006A9E97`, so a held or charge-drain superweapon's
/// clock covers its label. The production Hold label is resolved after the
/// clock (`0x006A9ECC..0x006AA014`) and draws over it.
fn status_text_precedes_clock(item: &crate::ui::sidebar::SidebarItem) -> bool {
    item.super_weapon_display.is_some() || item.is_ready
}

/// Whether a Building or Defense cameo may draw a queue count at all.
///
/// gamemd's badge path resolves the requested type's House Factory with a
/// hard-coded `BuildCat` 0 (`StripClass::Draw`, push at `0x006A9C52`, then
/// Fetch_Factory `0x00500510`): every structure gets the Buildings Factory. A
/// Building entry finds its own, which holds one build at a time, so its count
/// is at most the held copy; a Defense entry never counts. The de-hard-coded
/// lookup (Phobos' `StripClass_DrawStrip_FindFactoryDehardCode @ 0x006A9C54`,
/// `src/Ext/Building/Hooks.cpp`) uses the building type's own Factory. It is
/// unconditional under the `phobos` feature; a default build takes it with
/// `[General] BuildingProductionQueue=`, the switch that lets structure strips
/// stack copies, so gamemd's single-count badge stays without it.
fn structure_queue_badges(frontend: &FrontendState, match_state: &MatchState) -> bool {
    cfg!(feature = "phobos")
        || crate::app::state::active_rules(frontend, match_state)
            .is_some_and(|rules| rules.general.building_production_queue)
}

fn shows_queue_badge(item: &crate::ui::sidebar::SidebarItem, structure_badges: bool) -> bool {
    // VERA's Superweapon Limit: a superweapon cameo counts its charged shots
    // once more than one waits. gamemd's has no count.
    if item.is_superweapon {
        return item.queued_count > 1;
    }
    let counts = !matches!(
        item.queue_category,
        crate::sim::production::ProductionCategory::Building
            | crate::sim::production::ProductionCategory::Defense
    ) || structure_badges;
    // `Factory::CountType @ 0x004CA670` counts the held product as one when it
    // is this type, so a count of one is a queued copy only while the held
    // product is something else (`0x006A9C99..0x006A9CC3`).
    counts && (item.queued_count > 1 || (item.queued_count > 0 && !item.is_held_this_type))
}

/// The top-right badge text: the queue count, or VERA's endless-build mark
/// (`build_repeat`, Ctrl + left click) after it. The mark is not native.
fn queue_badge_text(
    input: &MatchInputState,
    item: &crate::ui::sidebar::SidebarItem,
    has_queue_badge: bool,
) -> Option<String> {
    if !item.is_superweapon && input.build_repeat.is_repeating(&item.type_id) {
        return Some(format!("{}{}", item.queued_count, ENDLESS_BUILD_MARK));
    }
    has_queue_badge.then(|| item.queued_count.to_string())
}

/// Shown after the queue count of a type that builds endlessly.
const ENDLESS_BUILD_MARK: &str = "+";

/// Whether a status label sits at the cameo's left edge instead of centered.
///
/// gamemd keeps two sidebar text slots. `[esp+0x40]` carries `TXT_READY`
/// (`push 0xd53` at `0x006A982F`, taken only when `IsComplete 0x004CA130`
/// answers) and every `SuperClass::NameReadiness 0x006CC2B0` label, including a
/// held superweapon's; its draw at `0x006A9DAE..0x006A9E17` is always
/// `cameo_x + 30` with flags `0x142`. Only the production Hold label, resolved
/// separately at `0x006A9ECC`, reads the count-badge flag at `0x006A9EE4` and
/// moves to `cameo_x + 2` with `0x42` (`0x006A9EF8..0x006A9EFE`).
fn status_text_shifts_left(item: &crate::ui::sidebar::SidebarItem, has_queue_badge: bool) -> bool {
    has_queue_badge && item.is_on_hold && !item.is_ready && item.super_weapon_display.is_none()
}

/// Whether a cameo draws the build clock. gamemd's strip draw sets its clock
/// flag whenever the cameo's factory exists (`StripClass::Draw`,
/// `0x006A9812..0x006A981D`) and draws frame `progress + 1`, so a build shows
/// its clock from the frame it starts, before its first step.
fn shows_build_clock(item: &crate::ui::sidebar::SidebarItem) -> bool {
    // SuperClass::ShouldDrawProgress6CDE90 draws every drain phase, even
    // with raw Ready set or a stage-zero charge. Factory policy is separate.
    if let Some(display) = item.super_weapon_display {
        return display.draw_progress;
    }
    !item.is_ready && (item.is_building_this_type || item.is_on_hold || item.progress > 0.0)
}

/// Map an alpha-cropped source rectangle through its original canvas into a
/// sidebar slot. Rounding both crop edges from the shared canvas transform
/// keeps the base art and full-canvas overlays on the same pixel boundaries.
/// The camera offset is added after screen-space rounding so the shader can
/// subtract it without making fixed UI geometry depend on fractional panning.
pub(crate) fn place_canvas_crop_in_slot(
    slot: Rect,
    canvas_size: [f32; 2],
    crop_origin: [f32; 2],
    crop_size: [f32; 2],
    camera_offset: [f32; 2],
) -> Option<Rect> {
    let [canvas_w, canvas_h] = canvas_size;
    let [crop_w, crop_h] = crop_size;
    if canvas_w <= 0.0 || canvas_h <= 0.0 || crop_w <= 0.0 || crop_h <= 0.0 {
        return None;
    }

    let scale = (slot.w / canvas_w).min(slot.h / canvas_h);
    let canvas_x = slot.x + (slot.w - canvas_w * scale) * 0.5;
    let canvas_y = slot.y + (slot.h - canvas_h * scale) * 0.5;
    let left = (canvas_x + crop_origin[0] * scale).round();
    let top = (canvas_y + crop_origin[1] * scale).round();
    let right = (canvas_x + (crop_origin[0] + crop_w) * scale).round();
    let bottom = (canvas_y + (crop_origin[1] + crop_h) * scale).round();

    Some(Rect {
        x: left + camera_offset[0],
        y: top + camera_offset[1],
        w: right - left,
        h: bottom - top,
    })
}

/// Select and place the GCLOCK2 frame for one in-progress cameo.
pub(crate) fn build_gclock_instance(
    gclock_frames: &[SidebarChromeEntry],
    progress: f32,
    slot: Rect,
    camera_offset: [f32; 2],
) -> Option<SpriteInstance> {
    // Factory stage projection retains its existing clamp and rounding.
    let progress = progress.clamp(0.0, 1.0);
    let stage = (progress * f32::from(crate::sim::production::PRODUCTION_STEPS)).round() as i32;
    build_gclock_stage_instance(gclock_frames, stage, slot, camera_offset)
}

fn build_cameo_clock_instance(
    gclock_frames: &[SidebarChromeEntry],
    item: &crate::ui::sidebar::SidebarItem,
    slot: Rect,
    camera_offset: [f32; 2],
) -> Option<SpriteInstance> {
    if let Some(display) = item.super_weapon_display {
        build_gclock_stage_instance(gclock_frames, display.stage, slot, camera_offset)
    } else {
        build_gclock_instance(gclock_frames, item.progress, slot, camera_offset)
    }
}

pub(crate) fn build_gclock_stage_instance(
    gclock_frames: &[SidebarChromeEntry],
    stage: i32,
    slot: Rect,
    camera_offset: [f32; 2],
) -> Option<SpriteInstance> {
    let last_frame = gclock_frames.len().checked_sub(1)?;
    // StripClass::Draw6A9E97 adds one before SHP signed-frame admission.
    // Stage-1 selects frame0; lower frames refuse. Short art still clamps.
    let frame_index = usize::try_from(stage.wrapping_add(1)).ok()?.min(last_frame);
    let gclock_entry = &gclock_frames[frame_index];
    let gclock_rect = place_canvas_crop_in_slot(
        slot,
        gclock_entry.pixel_size,
        [0.0, 0.0],
        gclock_entry.pixel_size,
        camera_offset,
    )?;

    Some(SpriteInstance {
        position: [gclock_rect.x, gclock_rect.y],
        size: [gclock_rect.w, gclock_rect.h],
        uv_origin: gclock_entry.uv_origin,
        uv_size: gclock_entry.uv_size,
        depth: 0.00043,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        draw_state: crate::render::draw_state::DrawState::from_native_selector(0, 4),
        ..Default::default()
    })
}

#[derive(Default)]
pub(crate) struct SidebarCameoInstances {
    pub cameo: Vec<SpriteInstance>,
    /// The hovered entry's outline, between the icon and the unavailable mask.
    pub hover: Vec<SpriteInstance>,
    pub unavailable: Vec<SpriteInstance>,
    pub flash: Vec<SpriteInstance>,
    pub gclock: Vec<SpriteInstance>,
    pub queue_overlay: Vec<SpriteInstance>,
    /// Strips behind the labels drawn before the clock.
    pub label_overlay: Vec<SpriteInstance>,
    /// Strips behind the production Hold label, drawn after the clock.
    pub overlay: Vec<SpriteInstance>,
}

/// The visible slot whose select button the mouse is over. Mouse_Enter6AB990
/// and Mouse_Leave6AB9E0 keep that button's +0x34 flag; StripDraw6A972D
/// reads it only for a filled slot and ignores it while input is locked
/// (`[0xA8ED9C]`, 6A9734).
fn hovered_cameo_slot(
    hovered: Option<usize>,
    entry_count: usize,
    input_locked: bool,
) -> Option<usize> {
    let slot = hovered?;
    (slot < entry_count && !input_locked).then_some(slot)
}

/// `XSurface::DrawRectEx @ 0x007BADC0`: a one-pixel outline whose last row
/// and column lie inside the rectangle. StripDraw6A9AF8 draws the hovered
/// 60x48 cameo slot with it; Phobos `SWButtonClass::Draw` uses the same rect.
pub(crate) fn push_rect_outline(out: &mut Vec<SpriteInstance>, rect: Rect, tint: [f32; 3]) {
    for [x, y, w, h] in rect_outline_edges(rect) {
        out.push(SpriteInstance {
            position: [x, y],
            size: [w, h],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: OUTLINE_DEPTH,
            tint,
            alpha: 1.0,
            ..Default::default()
        });
    }
}

/// The hover outline's four one-pixel edges as `x, y, width, height`: top,
/// bottom, left and right ([`push_rect_outline`]).
pub(crate) fn rect_outline_edges(rect: Rect) -> [[f32; 4]; 4] {
    let border = 1.0;
    [
        [rect.x, rect.y, rect.w, border],
        [rect.x, rect.y + rect.h - border, rect.w, border],
        [rect.x, rect.y, border, rect.h],
        [rect.x + rect.w - border, rect.y, border, rect.h],
    ]
}

/// The hover outline's depth.
pub(crate) const OUTLINE_DEPTH: f32 = 0.00044;

/// Original shape point is the cameo top-left; the caller clips at +60/+48
/// (StripDraw6A99C3..6A99E0). Do not fit the stock75x52 DARKEN canvas into
/// the cameo. Preserve frame origin, zero indices and the unscaled source UV.
pub(crate) fn build_darken_instance(
    mask: &crate::render::sidebar_chrome::SidebarDarkenMask,
    slot: Rect,
    camera_offset: [f32; 2],
    selector: u8,
) -> Option<SpriteInstance> {
    let [x, y] = mask.frame_origin;
    let width = mask.texture.width as f32;
    let height = mask.texture.height as f32;
    let size = [width.min(slot.w - x), height.min(slot.h - y)];
    if size[0] <= 0.0 || size[1] <= 0.0 {
        return None;
    }
    Some(SpriteInstance {
        position: [slot.x + x + camera_offset[0], slot.y + y + camera_offset[1]],
        size,
        uv_size: [size[0] / width, size[1] / height],
        tint: [1.0; 3],
        alpha: 1.0,
        draw_state: crate::render::draw_state::DrawState::from_native_selector(0, selector),
        ..Default::default()
    })
}

/// Base icons, native unavailable/flash/clock destination edits, then status
/// strip quads. Native status/badge text strips retain their separate owner.
pub(crate) fn build_sidebar_cameo_instances(
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    view: &SidebarView,
    labels: CameoStatusLabels<'_>,
) -> SidebarCameoInstances {
    let Some(atlas) = match_state.match_presentation.sidebar_cameo_atlas.as_ref() else {
        return SidebarCameoInstances::default();
    };
    let mut instances = Vec::new();
    let mut gclock_instances = Vec::new();
    let mut overlay_instances = Vec::new();
    let mut queue_overlay = Vec::new();
    let mut label_overlay = Vec::new();
    let mut unavailable = Vec::new();
    let mut flash = Vec::new();
    let co = [match_state.input.camera_x, match_state.input.camera_y];
    let chrome =
        crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state);
    let gclock_frames: &[SidebarChromeEntry] =
        chrome.map(|a| a.gclock_frames.as_slice()).unwrap_or(&[]);
    let mut hover = Vec::new();
    let hovered_slot = hovered_cameo_slot(
        crate::app::input::chrome_input::hovered_cameo(&match_state.match_presentation),
        view.items.len(),
        crate::app::input::campaign_input::is_locked(match_state),
    );
    // SetSidebarTextColor72F440 writes B0FA1C, which Draw packs at 6A9A4F.
    // The layout's `[Cameos] LabelColor=`, by default the side text colour.
    let hover_tint = crate::render::sidebar_text::native_outline_from(
        match_state
            .match_presentation
            .sidebar_layout_spec
            .label_text,
    );
    for (slot_index, item) in view.items.iter().enumerate() {
        let entry = if item.is_superweapon {
            atlas.superweapon(&item.type_id)
        } else {
            atlas.cameo(&item.type_id, item.alt_cameo)
        };
        let slot = item.cameo_rect();
        // StripDraw6A99F3 skips only the shape when an entry has no cameo;
        // its outline, masks, clock and text still draw.
        if let Some(entry) = entry
            && let Some(cameo_rect) = place_canvas_crop_in_slot(
                slot,
                entry.canvas_size,
                entry.crop_origin,
                entry.pixel_size,
                co,
            )
        {
            instances.push(SpriteInstance {
                position: [cameo_rect.x, cameo_rect.y],
                size: [cameo_rect.w, cameo_rect.h],
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                depth: 0.00044,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
        }
        if hovered_slot == Some(slot_index) {
            push_rect_outline(
                &mut hover,
                Rect {
                    x: slot.x + co[0],
                    y: slot.y + co[1],
                    ..slot
                },
                hover_tint,
            );
        }
        if shows_build_clock(item)
            && let Some(instance) = build_cameo_clock_instance(gclock_frames, item, slot, co)
        {
            gclock_instances.push(instance);
        }

        if let Some(mask) = chrome.and_then(|atlas| atlas.darken.as_ref()) {
            // View owns admission: linked E2 clears BL even when its last
            // producer is Selling, while unlinked FLAKT can remain shaded.
            if !item.enabled
                && let Some(instance) = build_darken_instance(mask, slot, co, 1)
            {
                unavailable.push(instance);
            }
            if item.flash_darkened
                && let Some(instance) = build_darken_instance(mask, slot, co, 4)
            {
                flash.push(instance);
            }
        }

        let has_queue_badge =
            shows_queue_badge(item, structure_queue_badges(frontend, match_state));

        // Dark strip overlay behind the cameo status text (alpha 0xAF).
        // It follows the label it backs, so only the Hold strip shifts.
        if let Some(status_text) = cameo_status_text(item, labels)
            && renderer.bit_font.darken_texture().is_some()
        {
            let font = &renderer.bit_font;
            let shifts_left = status_text_shifts_left(item, has_queue_badge);
            let badge = shifts_left
                .then(|| queue_badge_text(&match_state.input, item, has_queue_badge))
                .flatten();
            let fit = status_label_fit(font, status_text, slot.w, badge.as_deref());
            let text_w = font.fitted(fit, |font| font.text_width(status_text)) as f32;
            let strip_w = text_w + READY_PAD_X * 2.0;
            // gamemd `ComputeTextRect` uses `cell_height + 2*y_pad` for the
            // strip height (cell_height includes the 1 px inter-line gap that
            // gamemd extends below the glyphs).
            let strip_h = font.cell_height() + READY_PAD_Y * 2.0;
            let strip_x = if shifts_left {
                slot.x + co[0]
            } else {
                slot.x + (slot.w - strip_w) * 0.5 + co[0]
            };
            let strips = if status_text_precedes_clock(item) {
                &mut label_overlay
            } else {
                &mut overlay_instances
            };
            strips.push(SpriteInstance {
                position: [strip_x, slot.y + co[1]],
                size: [strip_w, strip_h.min(slot.h)],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: 0.00043,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
        }

        // Dark strip overlay behind queue count badge (top-right, same alpha as Ready strip).
        // Original: ComputeTextRect(cameo_x+60, cameo_y+1, 0x242, x_pad=2, y_pad=1)
        // The dark rect extends x_pad (2px) past the cameo right edge.
        if let Some(count_str) = queue_badge_text(&match_state.input, item, has_queue_badge)
            && renderer.bit_font.darken_texture().is_some()
        {
            let text_w = renderer.bit_font.text_width(&count_str) as f32;
            let strip_w = text_w + QUEUE_COUNT_PAD_X * 2.0;
            let strip_h = renderer.bit_font.cell_height() + QUEUE_COUNT_PAD_Y * 2.0;
            // Right-align anchor at cameo right edge; strip extends x_pad past it.
            let strip_x = slot.x + slot.w - text_w - QUEUE_COUNT_PAD_X;
            queue_overlay.push(SpriteInstance {
                position: [strip_x + co[0], slot.y + co[1]],
                size: [strip_w, strip_h.min(slot.h)],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: 0.00043,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
        }
    }
    // StripDraw's empty-slot path (6A99BE -> 6A9B4B) paints the same flash
    // darkening on a lit empty record.
    if let Some(mask) = chrome.and_then(|atlas| atlas.darken.as_ref()) {
        flash.extend(
            view.flash_empty_slots
                .iter()
                .filter_map(|&slot| build_darken_instance(mask, slot, co, 4)),
        );
    }
    SidebarCameoInstances {
        cameo: instances,
        hover,
        unavailable,
        flash,
        gclock: gclock_instances,
        queue_overlay,
        label_overlay,
        overlay: overlay_instances,
    }
}

/// The observer strip's pictures, from the chrome atlas, as the part
/// `Observer` of `pictures`: each listed House's side background at its row
/// pair, its flag 70 pixels right and down (`0x006AA0F1..0x006AA154`,
/// `0x006AA1E3..0x006AA2C9`). Begins and finishes the tree's frame.
pub(crate) fn draw_observer_strip<'a>(
    frontend: &FrontendState,
    match_state: &MatchState,
    view: Option<&SidebarView>,
    pictures: &'a mut PictureHud,
) -> &'a [SpriteInstance] {
    pictures.begin();
    let houses = view.and_then(|view| view.observer.as_deref());
    if let (Some(houses), Some(atlas)) = (houses, current_sidebar_chrome(frontend, match_state)) {
        let co = [match_state.input.camera_x, match_state.input.camera_y];
        let art = &atlas.observer_art;
        pictures.part(OBSERVER);
        for house in houses {
            let [x, y] = house.origin;
            if let Some(&background) = art.background(house.side) {
                let size = background.pixel_size;
                let (rect, instance) =
                    chrome_piece(background, x, y, size, OBSERVER_BACKGROUND_DEPTH, co);
                pictures.picture(rect, instance);
            }
            if let Some(&flag) = art.flag(house.country) {
                let (x, y) = (x + 70.0, y + 70.0);
                let (rect, instance) =
                    chrome_piece(flag, x, y, flag.pixel_size, OBSERVER_FLAG_DEPTH, co);
                pictures.picture(rect, instance);
            }
        }
    }
    pictures.finish()
}

/// The observer strip's part name in the sidebar's trees.
const OBSERVER: &str = "Observer";

/// Over the strip's SIDE2 tiles, under its text.
const OBSERVER_BACKGROUND_DEPTH: f32 = 0.00045;
const OBSERVER_FLAG_DEPTH: f32 = 0.000449;

/// The observer strip's texts in each House's colour (Fancy_Text_Print
/// `0x004A61C0`, style `0x42`), as the frame's next texts of `hud` in the
/// part `Observer`: the name at 8, 4 into the row pair, each line 17 pixels
/// below the last (`0x006AA373..0x006AA583`).
pub(crate) fn draw_observer_strip_text(
    hud: &mut TextHud,
    font: &BitFont,
    houses: &[crate::ui::sidebar::ObserverEntry],
    co: [f32; 2],
) {
    hud.part(OBSERVER);
    let height = font.cell_height();
    for house in houses {
        let tint = house.rgb.map(|c| f32::from(c) / 255.0);
        let [x, y] = [house.origin[0] + 8.0, house.origin[1] + 4.0];
        for (line, text) in std::iter::once(&house.name).chain(&house.lines).enumerate() {
            let y = y + line as f32 * 17.0;
            // The line is not measured, to measure nothing the strip did not:
            // its node marks where it starts.
            hud.fitted_text(
                covering(x, y, 0.0, height),
                text,
                [x + co[0], y + co[1]],
                tint,
                crate::render::sidebar_text::CREDITS_DEPTH,
                Fit::NONE,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Text label instances
// ---------------------------------------------------------------------------

/// The sidebar's pieces besides its chrome as drawing trees, kept from frame
/// to frame, one for each lane they draw in.
#[derive(Default)]
pub(crate) struct SidebarHuds {
    /// The observer strip's pictures, over the chrome.
    pub(crate) observer: PictureHud,
    /// The texts drawn after the clock: the production Hold labels, the
    /// observer strip's and the money counter.
    pub(crate) status: TextHud,
    /// The cameos' queue counts, over their strips.
    pub(crate) queue: TextHud,
    /// The Ready and superweapon labels, drawn before the clock.
    pub(crate) label: TextHud,
    /// Phobos's superweapon column.
    #[cfg(feature = "phobos")]
    pub(crate) sw: crate::app::presentation::sw_sidebar_build::SwSidebarHuds,
}

/// The cameos' labels as the frame's next texts of `huds`, each tree in the
/// part `Cameos`: the Ready and superweapon labels into `label`, drawn before
/// the clock, the production Hold label into `status`, drawn after it, and
/// the queue counts into `queue`. The caller begins and finishes the trees,
/// and draws nothing of this without the retail FNT.
pub(crate) fn draw_cameo_labels(
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    view: &SidebarView,
    labels: CameoStatusLabels<'_>,
    ready_tint: [f32; 3],
    huds: &mut SidebarHuds,
) {
    let co = [match_state.input.camera_x, match_state.input.camera_y];
    let font = bit_font;
    let height = font.cell_height();
    let cameos = SidebarPart::Cameos.name();
    for hud in [&mut huds.status, &mut huds.queue, &mut huds.label] {
        hud.part(cameos);
    }

    for item in &view.items {
        let slot = item.rect;

        let has_queue_badge =
            shows_queue_badge(item, structure_queue_badges(frontend, match_state));

        // Cameo status text ("Ready" / "On Hold") — at the top of the cameo.
        // Only the Hold label moves to the left edge, and only beside a queue
        // badge; see `status_text_shifts_left`.
        if let Some(status_text) = cameo_status_text(item, labels) {
            let shifts_left = status_text_shifts_left(item, has_queue_badge);
            let badge = shifts_left
                .then(|| queue_badge_text(&match_state.input, item, has_queue_badge))
                .flatten();
            let fit = status_label_fit(font, status_text, slot.w, badge.as_deref());
            let text_w = font.fitted(fit, |font| font.text_width(status_text)) as f32;
            // Residual: native truncates this halving; the float half
            // places odd widths up to 1 px apart, which a player cannot see.
            let text_x = if shifts_left {
                slot.x + READY_PAD_X
            } else {
                slot.x + (slot.w - text_w) * 0.5
            };
            // gamemd anchors text at `cameo_y + y_pad`; the strip extends
            // y_pad above and (cell_height - glyph_height + y_pad) below.
            let text_y = slot.y + READY_PAD_Y;
            let hud = if status_text_precedes_clock(item) {
                &mut huds.label
            } else {
                &mut huds.status
            };
            hud.fitted_text(
                covering(text_x, text_y, text_w, height),
                status_text,
                [text_x + co[0], text_y + co[1]],
                ready_tint,
                CAMEO_TEXT_DEPTH,
                fit,
            );
        }

        // Queue count badge — right-aligned at top-right of cameo.
        // Original: ComputeTextRect(cameo_x+60, cameo_y+1, 0x242, 2, 1)
        // 0x242 = right-align. Uses same side-dependent color as Ready text.
        if let Some(count_str) = queue_badge_text(&match_state.input, item, has_queue_badge) {
            let text_w = font.text_width(&count_str) as f32;
            // Right-align: text right edge at cameo right edge (anchor = cameo_x + 60).
            let text_x = slot.x + slot.w - text_w;
            let text_y = slot.y + QUEUE_COUNT_PAD_Y;
            huds.queue.fitted_text(
                covering(text_x, text_y, text_w, height),
                &count_str,
                [text_x + co[0], text_y + co[1]],
                ready_tint,
                CAMEO_TEXT_DEPTH,
                Fit::NONE,
            );
        }
    }
}

/// The cameos' labels' depth, the sidebar text layer's.
const CAMEO_TEXT_DEPTH: f32 = 0.00042;

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::{
        build_gclock_instance, hovered_cameo_slot, place_canvas_crop_in_slot, push_rect_outline,
    };
    use crate::render::sidebar_chrome::SidebarChromeEntry;
    use crate::ui::sidebar::Rect;

    #[test]
    fn test_canvas_crop_uses_shared_rounded_edges_with_camera_cancellation() {
        let slot = Rect {
            x: 100.1,
            y: 50.3,
            w: 75.0,
            h: 60.0,
        };
        let without_camera =
            place_canvas_crop_in_slot(slot, [60.0, 48.0], [3.0, 4.0], [54.0, 40.0], [0.0, 0.0])
                .unwrap();
        let with_camera =
            place_canvas_crop_in_slot(slot, [60.0, 48.0], [3.0, 4.0], [54.0, 40.0], [13.25, -7.5])
                .unwrap();

        assert_eq!(
            without_camera,
            Rect {
                x: 104.0,
                y: 55.0,
                w: 67.0,
                h: 50.0
            }
        );
        assert_eq!(with_camera.w, without_camera.w);
        assert_eq!(with_camera.h, without_camera.h);
        assert_eq!(with_camera.x - 13.25, without_camera.x);
        assert_eq!(with_camera.y + 7.5, without_camera.y);
        // 54 * 1.25 is 67.5; deriving size from the shared rounded edges
        // intentionally produces 67 rather than independently rounding to 68.
        assert_eq!(with_camera.w, 67.0);
    }

    #[test]
    fn hover_outline_marks_only_a_filled_slot_while_input_is_unlocked() {
        // The chrome hovers the second cameo of the pool.
        assert_eq!(hovered_cameo_slot(Some(1), 2, false), Some(1));
        // 6A9727: a pooled select button past EntryCount draws nothing.
        assert_eq!(hovered_cameo_slot(Some(1), 1, false), None);
        // 6A9734: the input lock hides the outline but keeps the hover.
        assert_eq!(hovered_cameo_slot(Some(1), 2, true), None);
        // A hover on no cameo marks none.
        assert_eq!(hovered_cameo_slot(None, 3, false), None);
    }

    #[test]
    fn rect_outline_includes_the_last_row_and_column_like_draw_rect_ex() {
        let mut out = Vec::new();
        let slot = Rect {
            x: 878.0,
            y: 228.0,
            w: 60.0,
            h: 48.0,
        };
        push_rect_outline(&mut out, slot, [1.0, 0.0, 0.0]);
        let edges: Vec<_> = out.iter().map(|i| (i.position, i.size)).collect();
        // 7BADC0 strokes x..x+w-1 and y..y+h-1.
        assert_eq!(
            edges,
            [
                ([878.0, 228.0], [60.0, 1.0]),
                ([878.0, 275.0], [60.0, 1.0]),
                ([878.0, 228.0], [1.0, 48.0]),
                ([937.0, 228.0], [1.0, 48.0]),
            ]
        );
    }

    #[test]
    fn gclock_selects_executed_native_frames_and_short_art_clamps() {
        let frames: Vec<_> = (0..55)
            .map(|frame| SidebarChromeEntry {
                uv_origin: [frame as f32, 0.0],
                uv_size: [1.0; 2],
                pixel_size: [60.0, 48.0],
            })
            .collect();
        let slot = Rect {
            x: 0.0,
            y: 0.0,
            w: 60.0,
            h: 48.0,
        };
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/factory_sale_sidebar.json",
        ))
        .unwrap();
        let mut compared = 0;
        for control in native["controls"].as_array().unwrap() {
            for command in control["draw_commands"].as_array().into_iter().flatten() {
                if command["kind"] != "clock" {
                    continue;
                }
                let stage = control["prepared"]["factory"]["stage"].as_u64().unwrap() as u16;
                let progress =
                    f32::from(stage) / f32::from(crate::sim::production::PRODUCTION_STEPS);
                let instance = build_gclock_instance(&frames, progress, slot, [0.0; 2]).unwrap();
                let frame = command["frame"].as_u64().unwrap() as usize;
                assert_eq!(
                    instance.uv_origin, frames[frame].uv_origin,
                    "{}",
                    control["name"]
                );
                assert_eq!(
                    0x400 | instance.draw_state.native_selector_bits(),
                    command["flags"].as_u64().unwrap() as u32,
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 4);
        let instance = build_gclock_instance(&frames[..3], 53.0 / 54.0, slot, [0.0; 2]).unwrap();
        assert_eq!(instance.uv_origin, frames[2].uv_origin);
    }

    /// The UI receives the simulation's derived native display, independently
    /// of raw Ready: drain phase2 still draws a clock and its own status.
    #[test]
    fn super_cameo_preserves_native_display_through_status_and_signed_clock() {
        use crate::sim::intern::StringInterner;
        use crate::sim::superweapon::{SuperWeaponDisplay, SuperWeaponStatus, SuperWeaponView};
        use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
        use crate::ui::sidebar::{self, SidebarChromeLayoutSpec, SidebarTab};
        let mut interner = StringInterner::new();
        let id = interner.intern("Drain");
        // Supplied display output at the client boundary. Core histories
        // cover the actual Grant/AI/click arithmetic that produces it.
        let project = |display, online| {
            let supers = [SuperWeaponView {
                type_id: id,
                use_ai_targeting: false,
                display_name: "Drain".into(),
                progress: 1.0,
                is_ready: true,
                is_online: online,
                should_flash_tab: false,
                can_fire: online,
                can_afford: true,
                constraints_blocked: false,
                ares_refusal: None,
                ready_shots: 0,
                ares_show_cameo: true,
                display,
            }];
            let cameos = sidebar::cameo_order::CameoStrips::layout_fixture(&[], &[], &supers);
            sidebar::build_sidebar_view_with_spec(
                SidebarChromeLayoutSpec::stock(),
                sidebar::SidebarFrame::native(800, 600),
                SidebarTab::Defense,
                0,
                0,
                0,
                None,
                &[],
                &[],
                &[],
                None,
                Some(&interner),
                &supers,
                &SidebarGadgetState::default(),
                None,
                None,
                None,
                None,
                [None; 2],
                &cameos,
            )
            .items
            .pop()
            .unwrap()
        };
        let labels = super::CameoStatusLabels {
            ready: "Ready",
            hold: "On Hold",
            charging: "Charging",
            active: "Firestorm On",
            ares: None,
        };
        let display = SuperWeaponDisplay {
            stage: 0,
            draw_progress: true,
            status: Some(SuperWeaponStatus::Active),
        };
        let mut item = project(display, true);
        assert_eq!(item.super_weapon_display, Some(display));
        assert!(item.is_ready && super::shows_build_clock(&item));
        assert_eq!(
            super::cameo_status_text(&item, labels),
            Some("Firestorm On")
        );
        let frames: Vec<_> = (0..55)
            .map(|frame| SidebarChromeEntry {
                uv_origin: [frame as f32, 0.0],
                uv_size: [1.0; 2],
                pixel_size: [60.0, 48.0],
            })
            .collect();
        let slot = item.cameo_rect();
        let draw = |item: &sidebar::SidebarItem| {
            super::build_cameo_clock_instance(&frames, item, slot, [0.0; 2])
        };
        assert_eq!(
            draw(&item).unwrap().uv_origin,
            frames[1].uv_origin,
            "native integer stage wins over raw Ready and float progress"
        );
        item = project(
            SuperWeaponDisplay {
                status: Some(SuperWeaponStatus::Hold),
                ..display
            },
            false,
        );
        assert!(item.is_ready && item.is_on_hold);
        assert_eq!(super::cameo_status_text(&item, labels), Some("On Hold"));
        item = project(
            SuperWeaponDisplay {
                stage: -1,
                status: Some(SuperWeaponStatus::Charging),
                ..display
            },
            true,
        );
        assert_eq!(super::cameo_status_text(&item, labels), Some("Charging"));
        assert_eq!(draw(&item).unwrap().uv_origin, frames[0].uv_origin);
        item = project(
            SuperWeaponDisplay {
                stage: -2,
                status: None,
                ..display
            },
            true,
        );
        assert!(draw(&item).is_none());
        assert_eq!(super::cameo_status_text(&item, labels), None);
    }

    /// A status label wider than its cameo squeezes onto it, also clear of a
    /// queue badge beside it; one that fits keeps its width.
    #[test]
    fn a_long_status_label_squeezes_onto_its_cameo_beside_the_badge() {
        use super::{QUEUE_COUNT_PAD_X, READY_PAD_X, status_label_fit};
        use crate::render::bit_font::Fit;
        let font = crate::render::bit_font::tests::make_vector_test_font();
        let text = "[Ôñ Ĥöļð ~~~~~~~~]";
        let natural = font.text_width(text) as f32;
        let fits = natural + READY_PAD_X * 2.0;
        assert_eq!(status_label_fit(&font, text, fits, None), Fit::NONE);
        // The narrowest letters are as far as a label squeezes; past them it is cut.
        let tightest = font.fitted(Fit::TIGHTEST, |font| font.text_width(text)) as f32;
        assert!(tightest < natural);
        for badge in [None, Some("3")] {
            let badge_w = badge.map_or(0.0, |badge| {
                font.text_width(badge) as f32 + QUEUE_COUNT_PAD_X
            });
            let slot_w = tightest + READY_PAD_X * 2.0 + badge_w;
            let fit = status_label_fit(&font, text, slot_w, badge);
            let width = font.fitted(fit, |font| font.text_width(text)) as f32;
            assert!(fit > Fit::NONE, "{badge:?}");
            assert!(
                width <= slot_w - READY_PAD_X * 2.0 - badge_w,
                "{badge:?}: {width}"
            );
        }
    }

    /// The cameo status slot carries `TXT_HOLD` for suspended production, the
    /// same way it carries `TXT_READY` for a placement-ready building. Before
    /// this, a held or unaffordable item looked identical to an idle one.
    /// Ares NameReadiness (hook 6CC2B0, 100356A0): per-type `Text.*`,
    /// Text.Preparing without a status, nothing for an empty label.
    #[test]
    fn ares_status_label_reads_the_type_texts() {
        use super::{CameoStatusLabels, ares_status_texts, superweapon_status_label};
        use crate::sim::superweapon::{SuperWeaponDisplay, SuperWeaponStatus as S};
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[SuperWeaponTypes]\n0=Sonar\n[Sonar]\nText.Preparing=TXT_WAIT\n\
                 Text.Hold=\nText.Ready=TXT_GO\n",
            ))
            .unwrap();
        let texts = ares_status_texts(&rules, |label| format!("<{label}>"));
        let labels = CameoStatusLabels {
            ready: "Ready",
            hold: "On Hold",
            charging: "Charging",
            active: "Active",
            ares: Some(&texts),
        };
        let label = |section, status| {
            let display = SuperWeaponDisplay {
                stage: 0,
                draw_progress: true,
                status,
            };
            superweapon_status_label(section, display, labels)
        };
        assert_eq!(label(Some("sonar"), None), Some("<TXT_WAIT>"));
        assert_eq!(label(Some("Sonar"), Some(S::Ready)), Some("<TXT_GO>"));
        assert_eq!(
            label(Some("Sonar"), Some(S::Hold)),
            Some("<TXT_HOLD>"),
            "an empty read keeps the constructor label"
        );
        assert_eq!(
            label(Some("Sonar"), Some(S::Active)),
            Some("<TXT_FIRESTORM_ON>")
        );
        assert_eq!(label(Some("Other"), Some(S::Ready)), Some("Ready"));
    }

    #[test]
    fn cameo_status_text_covers_ready_and_hold() {
        use super::{CameoStatusLabels, cameo_status_text};
        let labels = CameoStatusLabels {
            ready: "Ready",
            hold: "On Hold",
            charging: "Charging",
            active: "Active",
            ares: None,
        };

        let mut item = crate::ui::sidebar::SidebarItem {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 60.0,
                h: 48.0,
            },
            type_id: "GAPOWR".to_string(),
            display_name: "GAPOWR".to_string(),
            cost: Some(600),
            has_cameo_art: true,
            alt_cameo: false,
            flash_darkened: false,
            queue_category: crate::sim::production::ProductionCategory::Building,
            enabled: true,
            at_build_limit: false,
            progress: 0.5,
            queued_count: 1,
            is_building_this_type: false,
            is_held_this_type: false,
            is_ready: false,
            is_on_hold: false,
            is_armed: false,
            is_superweapon: false,
            super_weapon_section: None,
            super_weapon_display: None,
        };
        assert_eq!(cameo_status_text(&item, labels), None);

        item.is_on_hold = true;
        assert_eq!(cameo_status_text(&item, labels), Some("On Hold"));

        // Ready wins when a slot somehow reports both.
        item.is_ready = true;
        assert_eq!(cameo_status_text(&item, labels), Some("Ready"));
    }

    /// A build shows its clock from its start frame, at progress 0, and while
    /// it is on hold; a queued-only or ready cameo does not.
    #[test]
    fn build_clock_shows_from_the_start_of_a_build() {
        use super::shows_build_clock;

        let mut item = crate::ui::sidebar::SidebarItem {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 60.0,
                h: 48.0,
            },
            type_id: "MTNK".to_string(),
            display_name: "MTNK".to_string(),
            cost: Some(700),
            has_cameo_art: true,
            alt_cameo: false,
            flash_darkened: false,
            queue_category: crate::sim::production::ProductionCategory::Vehicle,
            enabled: true,
            at_build_limit: false,
            progress: 0.0,
            queued_count: 1,
            is_building_this_type: false,
            is_held_this_type: false,
            is_ready: false,
            is_on_hold: false,
            is_armed: false,
            is_superweapon: false,
            super_weapon_section: None,
            super_weapon_display: None,
        };
        assert!(!shows_build_clock(&item), "queued behind another build");
        item.is_building_this_type = true;
        assert!(shows_build_clock(&item), "started, no step yet");
        item.is_building_this_type = false;
        item.is_on_hold = true;
        assert!(shows_build_clock(&item), "on hold at progress 0");
        item.is_on_hold = false;
        item.is_ready = true;
        item.progress = 1.0;
        assert!(!shows_build_clock(&item), "ready");
    }

    /// 0x006A9D94 draws Ready and every Super label before the 0x006A9E97
    /// clock; the production Hold label waits for 0x006AA014.
    #[test]
    fn super_labels_precede_their_clock_and_production_hold_follows_it() {
        use super::{shows_build_clock, status_text_precedes_clock};
        use crate::sim::superweapon::{SuperWeaponDisplay, SuperWeaponStatus};
        let mut held = badge_item(
            crate::sim::production::ProductionCategory::Vehicle,
            0,
            false,
            false,
        );
        held.is_on_hold = true;
        assert!(shows_build_clock(&held) && !status_text_precedes_clock(&held));
        let mut ready = held.clone();
        ready.is_on_hold = false;
        ready.is_ready = true;
        assert!(status_text_precedes_clock(&ready));
        let mut super_held = held.clone();
        super_held.is_on_hold = false;
        super_held.super_weapon_display = Some(SuperWeaponDisplay {
            stage: 20,
            draw_progress: true,
            status: Some(SuperWeaponStatus::Hold),
        });
        assert!(shows_build_clock(&super_held) && status_text_precedes_clock(&super_held));
    }

    /// The `StripClass::Draw` badge inputs for one cameo.
    fn badge_item(
        category: crate::sim::production::ProductionCategory,
        queued_count: usize,
        is_building: bool,
        is_held: bool,
    ) -> crate::ui::sidebar::SidebarItem {
        crate::ui::sidebar::SidebarItem {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 60.0,
                h: 48.0,
            },
            type_id: "GAPOWR".to_string(),
            display_name: "GAPOWR".to_string(),
            cost: Some(600),
            has_cameo_art: true,
            alt_cameo: false,
            flash_darkened: false,
            queue_category: category,
            enabled: true,
            at_build_limit: false,
            progress: 0.0,
            queued_count,
            is_building_this_type: is_building,
            is_held_this_type: is_held,
            is_ready: false,
            is_on_hold: false,
            is_armed: false,
            is_superweapon: false,
            super_weapon_section: None,
            super_weapon_display: None,
        }
    }

    /// A structure cameo counts queued copies only where the badge can resolve
    /// its Factory: gamemd keeps a hard-coded `BuildCat` there, and Phobos
    /// de-hard-codes it at `0x006A9C54` for every Phobos build.
    #[test]
    fn structure_queue_badge_needs_the_phobos_factory_lookup() {
        use super::shows_queue_badge;
        use crate::sim::production::ProductionCategory::{Building, Defense, Vehicle};

        let stacked = badge_item(Building, 2, true, true);
        assert!(
            !shows_queue_badge(&stacked, false),
            "a default build never stacks structures"
        );
        assert!(shows_queue_badge(&stacked, true));
        assert!(shows_queue_badge(&badge_item(Defense, 3, true, true), true));

        let mobile = badge_item(Vehicle, 2, true, true);
        assert!(shows_queue_badge(&mobile, false));
        assert!(shows_queue_badge(&mobile, true));
    }

    /// gamemd shifts only the production Hold label. Ready and every
    /// `SuperClass::NameReadiness` label — including a held superweapon's —
    /// live in the other slot, which never reads the count badge.
    #[test]
    fn only_the_production_hold_label_shifts_left_beside_a_badge() {
        use super::status_text_shifts_left;
        let mut item = badge_item(
            crate::sim::production::ProductionCategory::Vehicle,
            2,
            true,
            true,
        );
        assert!(
            !status_text_shifts_left(&item, true),
            "not held, not shifted"
        );
        assert!(!status_text_shifts_left(&item, false));
        item.is_on_hold = true;
        assert!(status_text_shifts_left(&item, true));
        assert!(
            !status_text_shifts_left(&item, false),
            "the count badge is half the rule"
        );
        item.is_ready = true;
        assert!(
            !status_text_shifts_left(&item, true),
            "Ready takes the centered slot"
        );
        item.is_ready = false;
        item.super_weapon_display = Some(crate::sim::superweapon::SuperWeaponDisplay {
            status: Some(crate::sim::superweapon::SuperWeaponStatus::Hold),
            ..Default::default()
        });
        assert!(
            !status_text_shifts_left(&item, true),
            "a held superweapon is the other slot"
        );
    }

    /// A count of one is the held product rather than a queued copy whenever the
    /// primary Factory holds this entry's type, whether that product is finished
    /// or on hold: gamemd compares `Factory+0x58`'s type with the requested one
    /// at `0x006A9CBF..0x006A9CC3` and draws nothing. Reading the building flag
    /// instead left a bare `1` on a placement-ready structure.
    #[test]
    fn queue_badge_of_one_waits_for_a_copy_that_is_not_the_held_product() {
        use super::shows_queue_badge;
        use crate::sim::production::ProductionCategory::{Building, Vehicle};

        assert!(!shows_queue_badge(
            &badge_item(Building, 1, false, true),
            true
        ));
        assert!(!shows_queue_badge(
            &badge_item(Vehicle, 1, false, true),
            false
        ));
        assert!(shows_queue_badge(
            &badge_item(Vehicle, 1, false, false),
            false
        ));
    }

    /// The column's chrome drawn by its nodes on the chrome layer, its
    /// buttons by the nodes that take their clicks, draws the instances of
    /// its drawing in the same order and at the same depths, frame after
    /// frame, as the layout reorders its parts and leaves one out.
    #[test]
    fn the_chromes_nodes_draw_the_sidebars_pieces_in_drawing_order() {
        use super::{SpriteInstance, draw_sidebar_chrome, sidebar_paint_order};
        use crate::app::presentation::picture_hud::ChromeHud;
        use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
        use crate::ui::sidebar::{
            ArtKey, PowerBarAnimState, SidebarChromeLayoutSpec, SidebarFrame, SidebarPart,
            SidebarTab, build_sidebar_view_with_spec,
        };
        // Every picture of every part, told apart by where it is in the atlas.
        let art = |part: SidebarPart, key: ArtKey, frame: usize| {
            Some(SidebarChromeEntry {
                uv_origin: [
                    part as usize as f32 / 32.0,
                    key as usize as f32 / 8.0 + frame as f32 / 64.0,
                ],
                uv_size: [1.0 / 32.0, 1.0 / 64.0],
                pixel_size: [40.0, 20.0],
            })
        };
        let view = |spec: SidebarChromeLayoutSpec| {
            build_sidebar_view_with_spec(
                spec,
                SidebarFrame::for_layout(1024, 768, 100, &spec),
                SidebarTab::Building,
                0,
                0,
                0,
                Some([32.0, 28.0]),
                &[],
                &[],
                &[],
                None,
                None,
                &[],
                &SidebarGadgetState::new(),
                Some([52.0, 32.0]),
                Some([52.0, 32.0]),
                Some([46.0, 25.0]),
                Some([46.0, 25.0]),
                [Some([72.0, 18.0]); 2],
                &crate::ui::sidebar::cameo_order::CameoStrips::default(),
            )
        };
        let stock = SidebarChromeLayoutSpec::stock();
        // The power bar first, and no up arrow.
        let mut moved = stock;
        let mut order = std::iter::once(SidebarPart::Power).chain(
            SidebarPart::STOCK_ORDER
                .into_iter()
                .filter(|part| !matches!(part, SidebarPart::Power | SidebarPart::ScrollUp)),
        );
        moved.parts = std::array::from_fn(|_| order.next());
        let mut anim = PowerBarAnimState::new();
        anim.set_max_segments(120);
        let camera = [2.5, -1.0];
        let (mut gui, nodes) = crate::app::input::chrome_input::test_layer();
        let chrome = crate::app::input::chrome_input::paint_nodes_of(&nodes);
        let mut hud = ChromeHud::default();
        for spec in [stock, stock, moved, moved, stock] {
            let view = view(spec);
            let mut expected: Vec<SpriteInstance> = Vec::new();
            draw_sidebar_chrome(&mut expected, &art, spec, &view, &anim, camera, true);
            {
                let parts = &chrome.sidebar_parts;
                let mut pieces = hud.frame(&mut gui, chrome.tree, parts, parts);
                draw_sidebar_chrome(&mut pieces, &art, spec, &view, &anim, camera, true);
            }
            let (order, count) = sidebar_paint_order(&chrome.sidebar_parts, spec);
            let drawn = hud.finish(&mut gui, &order[..count]).to_vec();
            assert!(drawn.len() > 10, "the panels, the rows and the ten buttons");
            assert_eq!(
                bytemuck::cast_slice::<SpriteInstance, u8>(&drawn),
                bytemuck::cast_slice::<SpriteInstance, u8>(&expected),
            );
        }
    }
}
