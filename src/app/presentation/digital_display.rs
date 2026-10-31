//! Phobos DigitalDisplay rendering.
//!
//! `DigitalDisplayTypeClass::CanShow` and `::Draw`
//! (`src/New/Type/DigitalDisplayTypeClass.cpp`) decide which display entries
//! a techno shows and where. `TechnoExt::ProcessDigitalDisplays`
//! (`src/Ext/Techno/Body.Visuals.cpp`) runs inside `TechnoClass::DrawHealthBar`
//! at `0x6F65D1` (buildings) and `0x6F683C` (mobiles), so the stock health-bar
//! admission already bounds this feature: hovered or selected. The values
//! themselves come from `sim::digital_display`, which owns every state read.
//!
//! Residual: `HealthBar.Permanent=` is not ported (its `0x6F5E37` hook keeps
//! the disguise checks, adds a shroud test and `HealthBar.Permanent.PipScale`),
//! so an idle techno never reaches the health-bar call and `ShowType=idle`
//! never draws.
//!
//! The native text path is `DSurface::DrawTextA` with
//! `Align | FullShadow | Background?`; VERA emits the same text through its
//! GAME.FNT atlas. The native shape path is `ShapeTextPrinter::PrintShape`
//! with `BlitterFlags::None` (no canvas-half centering).

use crate::app::presentation::ui_overlays::{
    BuildingBarGeometry, StatusTextOverlayInstances, StatusViewport, foot_select_bracket,
    health_bar_drawable,
};
use crate::map::entities::EntityCategory;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::selection_overlay::SelectionOverlay;
use crate::rules::building_select_bracket::BuildingSelectBracketPosition;
use crate::rules::digital_display::{
    DigitalDisplayType, DigitalInfoType, DigitalTextAlign, DigitalVerticalAnchor,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;

/// Text height `DigitalDisplayTypeClass::DisplayText` uses for its Top anchor.
const TEXT_HEIGHT: f32 = 12.0;
/// Shield-bar pips height added to the Top anchor.
const PIPS_HEIGHT: f32 = 4.0;
/// `ShapeTextPrinter::SignSequence` (`ShapeTextPrinter.h:28`).
const SIGN_SEQUENCE: &[u8] = b"/%$,.!?|";

pub(crate) struct DigitalDisplayInstances {
    /// One backing+glyph set per text display, in list order. Keeping them
    /// separate reproduces Phobos's per-entry "background then text" order
    /// when several displays share an anchor.
    pub texts: Vec<StatusTextOverlayInstances>,
    /// `(catalog index, instance)` — each entry must be drawn with its own
    /// custom SHP texture.
    pub shapes: Vec<(u32, SpriteInstance)>,
}

impl DigitalDisplayInstances {
    fn empty() -> Self {
        Self {
            texts: Vec::new(),
            shapes: Vec::new(),
        }
    }
}

/// Build every digital display this parent shows. `enabled` is the
/// `[Phobos] DigitalDisplay.Enable` client switch; the `phobos` build itself
/// is the capability gate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn digital_display_instances(
    sim: &Simulation,
    entity: &GameEntity,
    rules: &RuleSet,
    overlay: &SelectionOverlay,
    font: &BitFont,
    hovered_id: Option<u64>,
    local_owner: Option<&str>,
    viewport: StatusViewport,
    enabled: bool,
) -> DigitalDisplayInstances {
    let mut out = DigitalDisplayInstances::empty();
    if !enabled {
        return out;
    }
    let Some(object) = sim.interner().object_type(entity.type_ref(), rules) else {
        return out;
    };
    if object.digital_display_disable() {
        return out;
    }
    let mut indices = rules.digital_display_indices(object).peekable();
    if indices.peek().is_none() {
        return out;
    }
    // `TechnoExt::ProcessDigitalDisplays` is called from DrawHealthBar, so the
    // stock health-bar admission is the only lifecycle gate here.
    if !health_bar_drawable(sim, entity, hovered_id, local_owner, Some(rules)) {
        return out;
    }
    let is_building = entity.category() == EntityCategory::Structure;
    let is_infantry = entity.category() == EntityCategory::Infantry;
    let viewer = local_owner.and_then(|name| sim.interner().get(name));
    let observer = viewer.is_none();
    let selected = entity.selection.is_selected();
    let hovering = hovered_id == Some(entity.stable_id());
    let frame = sim.session().binary_frame() as i32;
    // Phobos `TemporalTargetingMe || IsIronCurtained()`. `IsIronCurtained`
    // (`0x0041BF40`) only tests the timer, so a Force Shield counts too.
    let special_state = entity.temporal.is_warped()
        || crate::sim::superweapon::invulnerability::is_invulnerable(
            entity.invulnerability.as_ref(),
            frame as u32,
        );
    let owner = entity.owner();
    let (sx, sy) = screen_point(sim, entity);

    for index in indices {
        let display = &rules.digital_display_types().types()[index];
        // `DigitalDisplayTypeClass::CanShow`.
        if observer {
            if !display.visible_to_houses_observer {
                continue;
            }
        } else {
            let viewer = viewer.expect("non-observer viewer");
            // `CanTargetHouse(flags, Owner, CurrentPlayer)` tests the viewer.
            if !crate::sim::combat::can_target_house(
                display.visible_to_houses,
                owner,
                viewer,
                sim.houses(),
                sim.house_alliances(),
                sim.interner(),
            ) {
                continue;
            }
        }
        if !display.visible_in_special_state && special_state {
            continue;
        }
        let show = display.show_type;
        if show != crate::rules::digital_display::DigitalShowType::ALL {
            let current = if hovering { 0x1 } else { 0 } | if selected { 0x2 } else { 0 };
            if current != 0 {
                if current & show.bits() == 0 {
                    continue;
                }
            } else if !show.contains(crate::rules::digital_display::DigitalShowType::IDLE) {
                continue;
            }
        }

        let Some((mut value, mut maximum)) = crate::sim::digital_display::values_for_display(
            sim,
            entity,
            object,
            rules,
            display.info_type,
            display.info_index,
            viewer,
        ) else {
            continue;
        };
        if value < 0 || maximum <= 0 {
            continue;
        }
        let divisor = display.divisor();
        if divisor > 1 {
            value = (value / divisor).max(if value != 0 { 1 } else { 0 });
            maximum = (maximum / divisor).max(1);
        }

        let has_shield = entity
            .shield()
            .zip(
                entity
                    .shield()
                    .and_then(|s| rules.shield_type(s.type_name())),
            )
            .is_some_and(|(shield, ty)| !shield.broken_non_respawning(ty));
        let mut position = if is_building {
            building_bracket(sim, entity, object, rules, display.anchor_building)
        } else {
            foot_bracket(sx, sy, is_infantry, display)
        };
        position.1 += object.pixel_selection_bracket_delta as f32;
        if display.info_type == DigitalInfoType::Shield
            && let Some(delta) = entity
                .shield()
                .and_then(|shield| rules.shield_type(shield.type_name()))
        {
            position.1 += delta.bracket_delta() as f32;
        }
        position.0 += display.offset[0] as f32;
        position.1 += display.offset[1] as f32;
        if has_shield {
            if let Some(delta) = display.offset_shield_delta {
                position.0 += delta[0] as f32;
                position.1 += delta[1] as f32;
            } else if display.info_type == DigitalInfoType::Shield {
                position.1 -= 10.0;
            }
        }
        if is_building {
            match display.anchor_building {
                BuildingSelectBracketPosition::Top => position.1 -= 4.0,
                BuildingSelectBracketPosition::LeftTop
                | BuildingSelectBracketPosition::LeftBottom => {
                    position.0 -= 8.0;
                }
                _ => {}
            }
        }

        let ratio = f64::from(value) / f64::from(maximum);
        let shape = display
            .shape
            .as_ref()
            .and_then(|_| overlay.digital_shape(index));
        if let Some(shape) = shape {
            display_shape(
                display,
                shape,
                &mut out,
                index,
                position,
                value,
                maximum,
                ratio,
                rules,
                is_building,
                is_infantry,
                has_shield,
                viewport,
            );
        } else {
            display_text(
                display,
                font,
                &mut out,
                position,
                value,
                maximum,
                ratio,
                rules,
                is_infantry,
                has_shield,
                viewport,
            );
        }
    }
    out
}

fn screen_point(sim: &Simulation, entity: &GameEntity) -> (f32, f32) {
    let coord =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain());
    crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z)
}

/// `TechnoExt::GetFootSelectBracketPosition`: the shared foot bracket, then
/// the configured anchor.
fn foot_bracket(sx: f32, sy: f32, is_infantry: bool, display: &DigitalDisplayType) -> (f32, f32) {
    let rect = foot_select_bracket(sx, sy, is_infantry);
    (
        rect[0] + rect[2] * display.anchor_horizontal.offset(),
        rect[1] + rect[3] * display.anchor_vertical.offset(),
    )
}

/// `TechnoExt::GetBuildingSelectBracketPosition`: project `Dimension2`
/// (foundation edge plus ART Height), then apply the hexagon vertex.
pub(crate) fn building_bracket(
    sim: &Simulation,
    entity: &GameEntity,
    object: &crate::rules::object_type::ObjectType,
    rules: &RuleSet,
    anchor: BuildingSelectBracketPosition,
) -> (f32, f32) {
    let (sx, sy) = screen_point(sim, entity);
    let geometry = BuildingBarGeometry::new(rules, object);
    let (foundation_width, foundation_height) = object.foundation.dimensions();
    let length_w = i32::from(foundation_width) * 7 + i32::from(foundation_width) / 2;
    let length_h = i32::from(foundation_height) * 7 + i32::from(foundation_height) / 2;
    let art_height = geometry.height_leptons / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
    let height_px = art_height * 12;
    let (mut x, mut y) = geometry.first_pip(sx, sy);
    match anchor {
        BuildingSelectBracketPosition::Top => {}
        BuildingSelectBracketPosition::LeftTop => {
            x -= (length_h * 4) as f32;
            y += (length_h * 2) as f32;
        }
        BuildingSelectBracketPosition::LeftBottom => {
            x -= (length_h * 4) as f32;
            y += (length_h * 2 + height_px) as f32;
        }
        BuildingSelectBracketPosition::Bottom => {
            y += (length_w * 2 + length_h * 2 + height_px) as f32;
        }
        BuildingSelectBracketPosition::RightBottom => {
            x += (length_w * 4) as f32;
            y += (length_w * 2 + height_px) as f32;
        }
        BuildingSelectBracketPosition::RightTop => {
            x += (length_w * 4) as f32;
            y += (length_w * 2) as f32;
        }
    }
    (x, y)
}

#[allow(clippy::too_many_arguments)]
fn display_text(
    display: &DigitalDisplayType,
    font: &BitFont,
    out: &mut DigitalDisplayInstances,
    mut position: (f32, f32),
    value: i32,
    maximum: i32,
    ratio: f64,
    rules: &RuleSet,
    is_infantry: bool,
    has_shield: bool,
    viewport: StatusViewport,
) {
    let text = value_text(display, value, maximum, ratio, is_infantry);
    if display.anchor_vertical == DigitalVerticalAnchor::Top {
        position.1 -= TEXT_HEIGHT + if has_shield { PIPS_HEIGHT } else { 0.0 };
    }
    let width = font.text_width(&text) as f32;
    let height = font.glyph_height();
    let x = match display.align {
        DigitalTextAlign::Left => position.0,
        DigitalTextAlign::Center => position.0 - width / 2.0,
        DigitalTextAlign::Right => position.0 - width,
    };
    let (x, y) = (x, position.1);
    if !crate::app::presentation::instances::in_view(
        x,
        y,
        width,
        height,
        viewport.camera[0],
        viewport.camera[1],
        viewport.size[0],
        viewport.size[1],
        48.0,
    ) {
        return;
    }
    let [red, green, blue] = display.text_color_for(
        ratio,
        rules.general.condition_yellow,
        rules.general.condition_red,
    );
    let tint = [red, green, blue].map(|channel| f32::from(channel) / 255.0);
    let mut text_out = StatusTextOverlayInstances::default();
    if display.text_background {
        text_out.rectangles.push(SpriteInstance {
            position: [x, y],
            size: [width, font.cell_height()],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            tint: [0.0, 0.0, 0.0],
            alpha: 1.0,
            ..Default::default()
        });
    }
    // `TextPrintType::FullShadow`: one black copy behind the coloured glyphs.
    text_out
        .text
        .extend(font.build_text(&text, x + 1.0, y + 1.0, 1.0, 0.0, [0.0; 3], [0.0; 2]));
    text_out
        .text
        .extend(font.build_text(&text, x, y, 1.0, 0.0, tint, [0.0; 2]));
    out.texts.push(text_out);
}

/// `DigitalDisplayTypeClass::DisplayText`'s plain/percentage/timer strings.
fn value_text(
    display: &DigitalDisplayType,
    value: i32,
    maximum: i32,
    ratio: f64,
    is_infantry: bool,
) -> String {
    if display.value_as_timer {
        timer_text(value)
    } else if display.percentage {
        format!("{}%", (ratio * 100.0) as i32)
    } else if display.hide_max_value(is_infantry) {
        format!("{value}")
    } else {
        format!("{value}/{maximum}")
    }
}

/// `DisplayShape`'s frame group: green above `ConditionYellow`, yellow above
/// `ConditionRed`, red otherwise.
fn number_base(ratio: f64, condition_yellow: f64, condition_red: f64) -> i32 {
    if ratio > condition_yellow {
        0
    } else if ratio > condition_red {
        10
    } else {
        20
    }
}

fn timer_text(value: i32) -> String {
    let minute = value / 60;
    let hour = minute / 60;
    if hour != 0 {
        format!("{}:{:02}:{:02}", hour, minute % 60, value % 60)
    } else {
        format!("{}:{:02}", minute % 60, value % 60)
    }
}

/// `DigitalDisplayTypeClass::DisplayShape` + `ShapeTextPrinter::PrintShape`.
#[allow(clippy::too_many_arguments)]
fn display_shape(
    display: &DigitalDisplayType,
    shape: &crate::render::selection_overlay::CenteredShapeStrip,
    out: &mut DigitalDisplayInstances,
    index: usize,
    mut position: (f32, f32),
    value: i32,
    maximum: i32,
    ratio: f64,
    rules: &RuleSet,
    is_building: bool,
    is_infantry: bool,
    has_shield: bool,
    viewport: StatusViewport,
) {
    let mut spacing = display.shape_spacing(is_building);
    let text = if display.shape_percentage_frame {
        String::new()
    } else if display.value_as_timer {
        let minute = value / 60;
        let hour = minute / 60;
        let mut text = String::new();
        if hour != 0 {
            text.push_str(&hour.to_string());
            text.push('%');
        }
        let min = minute % 60;
        if min / 10 == 0 && hour != 0 {
            text.push('0');
        }
        text.push_str(&min.to_string());
        text.push('%');
        let sec = value % 60;
        if sec / 10 == 0 {
            text.push('0');
        }
        text.push_str(&sec.to_string());
        text
    } else if display.percentage {
        format!("{}%", (ratio * 100.0) as i32)
    } else if display.hide_max_value(is_infantry) {
        format!("{value}")
    } else {
        format!("{value}/{maximum}")
    };
    if display.anchor_vertical == DigitalVerticalAnchor::Top {
        position.1 -= shape.height() as f32 + if has_shield { PIPS_HEIGHT } else { 0.0 };
    }
    let len = text.len() as i32;
    match display.align {
        DigitalTextAlign::Left => {}
        DigitalTextAlign::Center => {
            if display.shape_percentage_frame {
                position.0 -= (shape.width() as i32 / 2) as f32;
            } else {
                position.0 -= (len * spacing[0] / 2) as f32;
                position.1 += (len * spacing[1] / 2) as f32;
            }
        }
        DigitalTextAlign::Right => {
            if display.shape_percentage_frame {
                position.0 -= shape.width() as f32;
            } else {
                position.0 -= spacing[0] as f32;
            }
        }
    }
    if !crate::app::presentation::instances::in_view(
        position.0,
        position.1,
        shape.width() as f32,
        shape.height() as f32,
        viewport.camera[0],
        viewport.camera[1],
        viewport.size[0],
        viewport.size[1],
        48.0,
    ) {
        return;
    }
    let number_base = number_base(
        ratio,
        rules.general.condition_yellow,
        rules.general.condition_red,
    );
    if display.shape_percentage_frame {
        let frames = shape.frame_count();
        if frames == 0 {
            return;
        }
        let frame = (ratio.clamp(0.0, 1.0) * (frames - 1) as f64 + 0.5) as usize;
        if let Some(instance) = shape.raw_instance(frame, position, 0.0006) {
            out.shapes.push((index as u32, instance));
        }
        return;
    }
    let extra_base = match number_base {
        10 => 32,
        20 => 34,
        _ => 30,
    };
    let mut chars: Vec<char> = text.chars().collect();
    if display.align == DigitalTextAlign::Right {
        chars.reverse();
        spacing[0] = -spacing[0];
    }
    for ch in chars {
        let frame = if ch.is_ascii_digit() {
            number_base + (ch as u8 - b'0') as i32
        } else if let Some(sign) = SIGN_SEQUENCE
            .iter()
            .position(|candidate| *candidate == ch as u8)
        {
            extra_base + sign as i32
        } else {
            continue;
        };
        let frame = usize::try_from(frame).unwrap_or(usize::MAX);
        if let Some(instance) = shape.raw_instance(frame, position, 0.0006) {
            out.shapes.push((index as u32, instance));
        }
        position.0 += spacing[0] as f32;
        position.1 -= spacing[1] as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::digital_display::DigitalHorizontalAnchor;

    fn display() -> DigitalDisplayType {
        crate::rules::digital_display::DigitalDisplayType {
            name: "TEST".to_owned(),
            text_color: [[0, 255, 0], [255, 255, 0], [255, 0, 0]],
            text_background: false,
            offset: [0, 0],
            offset_shield_delta: None,
            align: DigitalTextAlign::Right,
            anchor_horizontal: DigitalHorizontalAnchor::Right,
            anchor_vertical: DigitalVerticalAnchor::Top,
            anchor_building: BuildingSelectBracketPosition::Top,
            shape: None,
            palette: None,
            shape_spacing: None,
            shape_percentage_frame: false,
            percentage: false,
            hide_max_value: None,
            visible_to_houses: crate::rules::phobos_shields::AffectedHouse::ALL,
            visible_to_houses_observer: true,
            visible_in_special_state: true,
            info_type: DigitalInfoType::Health,
            info_index: 0,
            value_scale_divisor: None,
            value_as_timer: false,
            show_type: crate::rules::digital_display::DigitalShowType::SELECT,
        }
    }

    #[test]
    fn foot_bracket_uses_the_eight_or_seventeen_pip_rectangle() {
        let display = display();
        assert_eq!(foot_bracket(100.0, 100.0, false, &display), (118.0, 72.0));
        assert_eq!(foot_bracket(100.0, 100.0, true, &display), (110.0, 73.0));
    }

    #[test]
    fn text_formats_follow_phobos_order() {
        let mut display = display();
        assert_eq!(value_text(&display, 30, 120, 0.25, false), "30/120");
        display.percentage = true;
        assert_eq!(value_text(&display, 30, 120, 0.25, false), "25%");
        display.percentage = false;
        display.hide_max_value = Some(true);
        assert_eq!(value_text(&display, 30, 120, 0.25, false), "30");
        display.hide_max_value = None;
        display.value_as_timer = true;
        assert_eq!(timer_text(30), "0:30");
        assert_eq!(timer_text(3661), "1:01:01");
    }

    #[test]
    fn shape_frame_groups_follow_the_health_thresholds() {
        assert_eq!(number_base(0.75, 0.5, 0.25), 0);
        assert_eq!(number_base(0.5, 0.5, 0.25), 10);
        assert_eq!(number_base(0.25, 0.5, 0.25), 20);
        assert_eq!(number_base(0.26, 0.5, 0.25), 10);
    }
}
