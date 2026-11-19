//! An authored room uses the LAN window family with one taller row per seat
//! and a chat column between its roster and native sidebar.

use super::*;

pub(crate) const SCENARIO_ROWS_PER_PAGE: usize = 6;
pub(crate) const SCENARIO_BRIEFING: u16 = 0x7F1C;
pub(crate) const SCENARIO_DESCRIPTION_PREVIOUS: u16 = 0x7F26;
pub(crate) const SCENARIO_DESCRIPTION_NEXT: u16 = 0x7F27;
pub(crate) const SCENARIO_DIFFICULTY_TRACK_W: i32 = 362;
const SCENARIO_DETAILS: u16 = 0x7F20;

pub(crate) fn scenario_details(row: usize) -> u16 {
    SCENARIO_DETAILS + row as u16
}

pub(crate) fn scenario_details_row(id: u16) -> Option<usize> {
    let row = usize::from(id.checked_sub(SCENARIO_DETAILS)?);
    (row < SCENARIO_ROWS_PER_PAGE).then_some(row)
}

const BRIEFING_BUTTON: MenuPageButtonSpec = MenuPageButtonSpec {
    id: SCENARIO_BRIEFING,
    dlu_top: HOST_PAGE.stacked[1].dlu_top,
    csf_key: "Mission briefing",
    tooltip_key: "",
    result: None,
};
const SCENARIO_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[
        HOST_PAGE.stacked[0],
        BRIEFING_BUTTON,
        MenuPageButtonSpec {
            dlu_top: 2 * HOST_PAGE.stacked[1].dlu_top - HOST_PAGE.stacked[0].dlu_top,
            ..HOST_PAGE.stacked[1]
        },
    ],
    ..HOST_PAGE
};
const SCENARIO_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[GUEST_PAGE.stacked[0], BRIEFING_BUTTON],
    ..GUEST_PAGE
};

pub(super) fn page(host: bool) -> &'static MenuPageSpec {
    if host {
        &SCENARIO_HOST_PAGE
    } else {
        &SCENARIO_GUEST_PAGE
    }
}

/// Presentation derived from the admitted stage and live room.
#[derive(Debug, Clone)]
pub(crate) struct ScenarioRoomView {
    pub(crate) rows: Vec<ScenarioRoomRow>,
    pub(crate) caption: &'static str,
    pub(crate) players_heading: &'static str,
    pub(crate) chat_heading: &'static str,
    pub(crate) rules_heading: &'static str,
    pub(crate) rules: Vec<String>,
    pub(crate) waiting: Option<String>,
    pub(crate) unavailable_preview: &'static str,
    pub(crate) briefing_label: &'static str,
    pub(crate) briefing: String,
    pub(crate) details_label: &'static str,
    pub(crate) accept_label: &'static str,
    pub(crate) choose_label: &'static str,
    pub(crate) leave_label: &'static str,
    pub(crate) presentation:
        Option<std::sync::Arc<crate::map::scenario_menu::LocalScenarioPresentation>>,
    pub(crate) role_colors: Vec<Option<[u8; 3]>>,
    pub(crate) description_page: usize,
    pub(crate) description_pages: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ScenarioRoomRow {
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) flag: Option<String>,
    pub(crate) color: Option<[u8; 3]>,
    pub(crate) details: Option<ScenarioRoleDetails>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScenarioRoleDetails {
    pub(crate) title: String,
    pub(crate) body: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DescriptionLayout {
    pub(crate) dialog: RectPx,
    pub(crate) title: RectPx,
    pub(crate) body: RectPx,
    pub(crate) page: RectPx,
    pub(crate) previous: RectPx,
    pub(crate) next: RectPx,
    pub(crate) ok: RectPx,
}

#[derive(Debug, Clone)]
pub(crate) struct ScenarioRoomLayout {
    pub(crate) players_heading: RectPx,
    pub(crate) chat_heading: RectPx,
    pub(crate) rules_heading: RectPx,
    pub(crate) waiting: RectPx,
    pub(crate) rules: RectPx,
    pub(crate) seats: [RectPx; SCENARIO_ROWS_PER_PAGE],
    pub(crate) names: [RectPx; SCENARIO_ROWS_PER_PAGE],
    pub(crate) statuses: [RectPx; SCENARIO_ROWS_PER_PAGE],
    pub(crate) flags: [RectPx; SCENARIO_ROWS_PER_PAGE],
    pub(crate) colors: [RectPx; SCENARIO_ROWS_PER_PAGE],
    pub(crate) details: [RectPx; SCENARIO_ROWS_PER_PAGE],
    pub(crate) description: DescriptionLayout,
}

impl MultiplayerSetupLayout {
    pub(super) fn place_scenario_room(&mut self) {
        let scale = self.page.scale;
        let window = scale.base_rect(scale.window());
        let rect = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        // The stocked sidebar uses roughly 22% at the approved 4:3 surface.
        // The body leaves half the window for names, roles and room rules.
        let body_left = window.x + 10;
        let chat_left = window.x + window.w / 2;
        let side_left = window.x + window.w * 78 / 100;
        let left_w = chat_left - body_left - 10;
        let chat_w = side_left - chat_left - 8;

        let row_y = |row: usize| 41 + row as i32 * 46;
        // Use the original 249 x 24 owner-drawn combo face and its arrow.
        for page_row in 0..SCENARIO_ROWS_PER_PAGE {
            let y = row_y(page_row);
            self.combos[page_row][SetupColumn::Player as usize] =
                rect(body_left + 7, y + 1, left_w - 126, 20);
            self.combos[page_row][SetupColumn::Team as usize] = rect(
                body_left + 56,
                y + 21,
                249,
                crate::ui::skirmish_shell::COMBO_FACE_H,
            );
        }
        self.labels.clear();
        self.chat = rect(chat_left, 532, chat_w, 23);
        self.chat_log = rect(chat_left, 42, chat_w, 480);
        self.roster_navigation = [
            rect(body_left + 6, 322, 111, 23),
            rect(body_left + left_w - 117, 322, 111, 23),
        ];
        self.roster_page_label = rect(body_left + 118, 322, left_w - 236, 23);
        self.map_preview = rect(side_left + 10, 40, 800 - side_left - 21, 109);
        let panel = scale.base_rect(self.page.right_panel.top);
        self.mode_label = rect(panel.x + 24, 172, panel.w - 48, 20);
        self.map_label = rect(panel.x + 24, 192, panel.w - 48, 20);
        self.checkboxes[0] = rect(body_left + 8, 438, left_w / 2 - 12, 20);
        self.checkboxes[2] = rect(body_left + left_w / 2, 438, left_w / 2 - 7, 20);
        if let Some((_, bar)) = self.trackbars.iter_mut().find(|(id, _)| *id == DIFFICULTY) {
            *bar = rect(body_left + 9, 406, SCENARIO_DIFFICULTY_TRACK_W, 22);
        }
        self.emote = None;
        let text_box = crate::ui::shell::modal::menu_box(SHELL_BASE_W, SHELL_BASE_H)
            .translate(window.x / 2, 0);
        self.scenario = Some(ScenarioRoomLayout {
            players_heading: rect(body_left + 2, 15, left_w - 4, 22),
            chat_heading: rect(chat_left + 1, 15, chat_w - 2, 22),
            rules_heading: rect(body_left + 7, 362, left_w - 14, 22),
            waiting: rect(chat_left + 4, 38, chat_w - 8, 28),
            rules: rect(body_left + 9, 463, left_w - 18, 76),
            seats: std::array::from_fn(|row| rect(body_left, row_y(row), left_w, 44)),
            names: std::array::from_fn(|row| rect(body_left + 7, row_y(row) + 1, left_w - 132, 19)),
            statuses: std::array::from_fn(|row| {
                rect(body_left + left_w - 119, row_y(row) + 1, 112, 19)
            }),
            flags: std::array::from_fn(|row| rect(body_left + 7, row_y(row) + 22, 46, 22)),
            colors: std::array::from_fn(|row| {
                rect(body_left + left_w - 72, row_y(row) + 27, 15, 12)
            }),
            details: std::array::from_fn(|row| {
                rect(body_left + left_w - 56, row_y(row) + 24, 51, 18)
            }),
            description: DescriptionLayout {
                dialog: scale.scale_rect(text_box),
                title: rect(text_box.x + 24, text_box.y + 14, text_box.w - 48, 24),
                body: rect(
                    text_box.x + 24,
                    text_box.y + 48,
                    text_box.w - 48,
                    text_box.h - 109,
                ),
                page: rect(
                    text_box.x + 24,
                    text_box.y + text_box.h - 60,
                    text_box.w - 48,
                    18,
                ),
                previous: rect(
                    text_box.x + 24,
                    text_box.y + text_box.h - 37,
                    ROSTER_BUTTON_W,
                    24,
                ),
                next: rect(
                    text_box.x + text_box.w - 24 - ROSTER_BUTTON_W,
                    text_box.y + text_box.h - 37,
                    ROSTER_BUTTON_W,
                    24,
                ),
                ok: rect(
                    text_box.x + (text_box.w - ROSTER_BUTTON_W) / 2,
                    text_box.y + text_box.h - 37,
                    ROSTER_BUTTON_W,
                    24,
                ),
            },
        });
    }
}
