use super::*;
use crate::ui::main_menu_dialogs::options::{
    HAS_GRAPHICS_API, HAS_HD_MAP_PREVIEWS, HAS_RA2_MODE, HAS_VSYNC,
};
use crate::ui::shell::geom::{SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW};
use crate::ui::shell::sound::{SoundSlider, launcher_volume_rects};
use LauncherCheckboxId as Check;
use LauncherComboId as Combo;
use LauncherTrackbarId as Slider;

pub(super) fn controls(
    category: Category,
    panel: &menu_page::MenuPageLayout,
    px: impl Fn(RectPx) -> RectPx,
) -> Option<Vec<(Control, RectPx)>> {
    use LauncherButton::{Category as Link, Exit};
    use LauncherParentResult::{Back, Keyboard, Mods};

    let mut controls = Vec::new();
    let x = 134;
    let right = 344;
    let back = panel.button_rect(MAIN_MENU_BUTTON)?;
    let mut column = |id, button| {
        if let Some(rect) = panel.button_rect(id) {
            controls.push((Control::Button(button), rect));
        }
    };
    match category {
        Category::LauncherMain => {
            column(KEYBOARD_BUTTON, Exit(Keyboard));
            column(CATEGORY_BUTTONS[0], Link(Category::Enhancements));
            column(CATEGORY_BUTTONS[1], Link(Category::Extensions));
            if HAS_MODS {
                let rail = panel.right_panel;
                let rect =
                    rail.row_cell(rail.tile_count - 2, SDBTNANM_CELL_W_NARROW, SDBTNANM_CELL_H);
                controls.push((Control::Button(Exit(Mods)), rect));
            }
            controls.push((Control::Button(Exit(Back)), back));
        }
        Category::Enhancements | Category::Extensions => {
            controls.push((Control::Button(Link(Category::LauncherMain)), back));
        }
        _ => return None,
    }
    let mut row = |control, rect| controls.push((control, px(rect)));
    match category {
        Category::LauncherMain => {
            section(&mut row, Category::Display, 33);
            slider(&mut row, Slider::Detail, x, 66, 181);
            combo(&mut row, Combo::Zoom, right, 66);
            let vsync_x = 225;
            check_with_width(
                &mut row,
                Check::FullScreen,
                x,
                126,
                if HAS_VSYNC { vsync_x - x } else { 208 },
            );
            if HAS_VSYNC {
                combo(&mut row, Combo::Display, right, 122);
                check(&mut row, Check::VSync, vsync_x, 126);
            }
            if HAS_GRAPHICS_API {
                combo(&mut row, Combo::GraphicsApi, right, 179);
            }
            check(&mut row, Check::LockCursor, x, 150);
            check(&mut row, Check::OriginalShape, x, 174);
            section(&mut row, Category::Gameplay, 201);
            check(&mut row, Check::ShiftQueue, x, 233);
            check(&mut row, Check::BuildingProductionQueue, x, 257);
            if HAS_RA2_MODE {
                check(&mut row, Check::Ra2Mode, x, 281);
            }
            slider(&mut row, Slider::Difficulty, right, 228, 181);
            section(&mut row, Category::Interface, 310);
            check(&mut row, Check::Tooltips, x, 342);
            check(&mut row, Check::TargetLines, x, 366);
            check(&mut row, Check::ShowHidden, x, 390);
            slider(&mut row, Slider::Scroll, right, 344, 181);
            section(&mut row, Category::Audio, 448);
            for (channel, id) in
                SoundSlider::ALL
                    .into_iter()
                    .zip([Slider::Score, Slider::Sound, Slider::Voice])
            {
                let setting = Setting::Slider(id);
                let (caption, track) = launcher_volume_rects(channel);
                row(Control::Label(PageLabel::Caption(setting)), caption);
                row(Control::Setting(setting), track);
            }
        }
        Category::Enhancements => {
            heading(&mut row, "VERA:EnhancementsHeading", 46);
            for (id, xp, y) in [
                (Combo::Language, x, 70),
                (Combo::Sidebar, x, 129),
                (Combo::HealthBars, x, 186),
                (Combo::ColorVision, right, 70),
            ] {
                combo(&mut row, id, xp, y);
            }
            let width = 181;
            slider(&mut row, Slider::SidebarColumns, right, 129, width);
            slider(&mut row, Slider::HouseColorStrength, right, 186, width);
            for (i, (left, right_id)) in [
                (Check::AutoRepair, Check::RightClickOrders),
                (Check::HdRadar, Check::MinimapBeacons),
                (Check::CompactSuperTimers, Check::MouseWheelZoom),
                (Check::DetailedEvaText, Check::YrOriginalStyle),
                (Check::GroupedSuperTimers, Check::ConvenienceHelp),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 252 + i as i32 * 24;
                check(&mut row, left, x, y);
                check(&mut row, right_id, right, y);
            }
            heading(&mut row, "VERA:DebugHeading", 377);
            for (i, id) in [
                Check::SmoothSidebar,
                Check::SmoothOnlineMenus,
                Check::ShowStringKeys,
                Check::PerformanceStats,
            ]
            .into_iter()
            .enumerate()
            {
                check(&mut row, id, x, 405 + i as i32 * 24);
            }
            check(&mut row, Check::SkipIntro, right, 405);
            check(&mut row, Check::VeraIcon, right, 429);
            if HAS_HD_MAP_PREVIEWS {
                check(&mut row, Check::HdMapPreviews, right, 453);
            }
            if HAS_VSYNC {
                check_with_width(&mut row, Check::HighRefreshRateInBattle, right, 477, 255);
            }
            if HAS_CHECK_FOR_UPDATES {
                row(
                    Control::Button(LauncherButton::CheckForUpdates),
                    RectPx::new(right, 510, 140, 24),
                );
            }
        }
        Category::Extensions => {
            heading(&mut row, "VERA:ExtensionsAresHeading", 42);
            row(
                Control::Setting(Setting::List(Combo::Ares)),
                RectPx::new(x, 86, OPTIONS_COMBO_W, COMBO_FACE_H),
            );
            heading(&mut row, "VERA:ExtensionsVeraHeading", 166);
            for (i, id) in [
                Check::CommanderFeedback,
                Check::CustomEndGameBanner,
                Check::CustomEndGameScore,
                Check::CustomEndGameMovie,
            ]
            .into_iter()
            .enumerate()
            {
                check(&mut row, id, x, 193 + i as i32 * 24);
            }
        }
        _ => unreachable!(),
    }
    Some(controls)
}

fn section(row: &mut impl FnMut(Control, RectPx), category: Category, y: i32) {
    row(
        Control::Label(PageLabel::Section(category)),
        RectPx::new(36, y, 240, 17),
    );
}

fn heading(row: &mut impl FnMut(Control, RectPx), key: &'static str, y: i32) {
    row(
        Control::Label(PageLabel::Localized(key)),
        RectPx::new(46, y, 240, 17),
    );
}

fn check(row: &mut impl FnMut(Control, RectPx), id: Check, x: i32, y: i32) {
    check_with_width(row, id, x, y, 208);
}

fn check_with_width(row: &mut impl FnMut(Control, RectPx), id: Check, x: i32, y: i32, width: i32) {
    row(
        Control::Setting(Setting::Checkbox(id)),
        RectPx::new(x + 7, y, width, 17),
    );
}

fn combo(row: &mut impl FnMut(Control, RectPx), id: Combo, x: i32, y: i32) {
    let setting = Setting::List(id);
    row(
        Control::Label(PageLabel::Caption(setting)),
        RectPx::new(x, y, 180, 17),
    );
    row(
        Control::Setting(setting),
        RectPx::new(x, y + 21, OPTIONS_COMBO_W, COMBO_FACE_H),
    );
}

pub(super) fn shows_value(id: Slider) -> bool {
    id == Slider::HouseColorStrength || id.plaque_reserve() != 0
}

fn slider(row: &mut impl FnMut(Control, RectPx), id: Slider, x: i32, y: i32, width: i32) {
    let setting = Setting::Slider(id);
    let label_w = if shows_value(id) { width } else { width - 62 };
    row(
        Control::Label(PageLabel::Caption(setting)),
        RectPx::new(x, y, label_w, 17),
    );
    if !shows_value(id) {
        row(
            Control::Label(PageLabel::Step(id)),
            RectPx::new(x + label_w, y, 62, 17),
        );
    }
    row(Control::Setting(setting), RectPx::new(x, y + 21, width, 22));
}
