//! My Information `0x114` (proc `0x00799080`, driver `0x00799D30`): the
//! persona page behind the WOL welcome's My Information button. A family
//! page (`0x0060C540` accepts it) with five owner-draw combos, two
//! checkboxes and the ladder statistics.
//!
//! The model owns the controls and the proc's control logic; the persona
//! file, the credential store and the account globals belong to the app's
//! WOL owner, which runs each returned [`MyInfoAction`] at once (every
//! persona write is LoadRecord, the setter, then SaveProfileToINI).

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::{ComboPopup, ComboPopupInteraction};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window, dlu_rect,
};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::{COMBO_DROPDOWN_ROW_H, COMBO_FACE_H};

pub(crate) const DIALOG: DialogId = DialogId(0x0114);
pub(crate) const BACK: u16 = 0x0686;
pub(crate) const PLAY_BUDDY: u16 = 0x0687;
pub(crate) const CLAN: u16 = 0x0688;
pub(crate) const TOURNAMENT: u16 = 0x07A3;
pub(crate) const FORGET: u16 = 0x06EB;
pub(crate) const MANAGE: u16 = 0x06EC;
pub(crate) const NICKNAME: u16 = 0x0628;
pub(crate) const SERVER: u16 = 0x0629;
pub(crate) const LOCALE: u16 = 0x062A;
pub(crate) const COUNTRY: u16 = 0x06A1;
pub(crate) const COLOR: u16 = 0x06A2;
pub(crate) const AUTO_LOGIN: u16 = 0x06FF;
pub(crate) const MATCH_SAME_RES: u16 = 0x0773;
pub(crate) const HEADING: u16 = 0x06EF;
pub(crate) const WINS: u16 = 0x06F0;
pub(crate) const LOSSES: u16 = 0x06F1;
pub(crate) const DISCONNECTS: u16 = 0x0701;
pub(crate) const RANK: u16 = 0x06F4;
pub(crate) const POINTS: u16 = 0x06F5;
pub(crate) const UPDATE: u16 = 0x0797;

/// The five combos in template order.
pub(crate) const COMBOS: [u16; 5] = [NICKNAME, SERVER, LOCALE, COUNTRY, COLOR];

/// `0x628`'s first row and its item data (`0x00799910..`).
pub(crate) const DEFAULT_KEY: &str = "GUI:Default";
/// `0x7896A0`'s single row with one server record (narrow literal
/// `0x0084B800`, added with `0x4BC`).
pub(crate) const NO_SERVERS: &str = "GUI:NoServersAvailable";
/// `0x799080`'s nickname rows: slots 1..32 (`0x00799A1A CMP EAX,0x20`).
pub(crate) const NICKNAME_SLOTS: std::ops::RangeInclusive<u32> = 1..=32;

/// RT_DIALOG `0x114`: Play Buddy, Clan, Tournament, Forget Account
/// (template-disabled, still counted) and Manage Account stacked by their
/// template tops, Back on the bottom row. Help keys from the executed
/// `0x006040B0`.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:MyInformation",
    stacked: &[
        MenuPageButtonSpec {
            id: PLAY_BUDDY,
            dlu_top: 126,
            csf_key: "GUI:PlayBuddy",
            tooltip_key: "STT:WOLBuddy",
            result: Some(PLAY_BUDDY as i32),
        },
        MenuPageButtonSpec {
            id: CLAN,
            dlu_top: 152,
            csf_key: "GUI:Clan",
            tooltip_key: "STT:WOLClan",
            result: None,
        },
        MenuPageButtonSpec {
            id: TOURNAMENT,
            dlu_top: 178,
            csf_key: "GUI:Tournament",
            tooltip_key: "STT:WOLTournament",
            result: None,
        },
        MenuPageButtonSpec {
            id: FORGET,
            dlu_top: 204,
            csf_key: "GUI:ForgetAccount",
            tooltip_key: "STT:WOLLoginForget",
            result: None,
        },
        MenuPageButtonSpec {
            id: MANAGE,
            dlu_top: 230,
            csf_key: "GUI:ManageAccount",
            tooltip_key: "STT:WOLManageAccount",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 344,
        csf_key: "GUI:Back",
        tooltip_key: "STT:WOLMyInformationBack",
        result: Some(BACK as i32),
    },
};

/// The right-aligned captions of the combos and the statistics, in
/// template order.
pub(crate) const LABELS: [(&str, [i32; 4]); 10] = [
    ("GUI:LoginName", [47, 49, 118, 13]),
    ("GUI:PreferredServer", [47, 71, 118, 13]),
    ("GUI:Locale", [47, 93, 118, 13]),
    ("GUI:PreferredCountry", [47, 115, 118, 13]),
    ("GUI:PreferredColor", [47, 136, 118, 13]),
    ("GUI:LadderWins", [87, 222, 115, 13]),
    ("GUI:LadderLosses", [87, 240, 115, 13]),
    ("GUI:Disconnects", [87, 258, 115, 13]),
    ("GUI:LadderRank", [87, 276, 115, 13]),
    ("GUI:LadderPoints", [87, 294, 115, 13]),
];

/// The statistics' value statics (left aligned), in template order.
pub(crate) const STATS: [u16; 5] = [WINS, LOSSES, DISCONNECTS, RANK, POINTS];

pub(crate) const AUTO_LOGIN_KEY: &str = "GUI:AutoLogin";
pub(crate) const MATCH_SAME_RES_KEY: &str = "GUI:MatchSameRes";
pub(crate) const UNKNOWN_STATS_KEY: &str = "GUI:UnknownStats";

/// Status help of `0x114`'s children (executed `0x006040B0`).
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    Some(match control {
        NICKNAME => "STT:WOLMyInformationName",
        SERVER => "STT:WOLMyInformationServer",
        LOCALE => "STT:WOLMyInformationLocale",
        COUNTRY => "STT:WOLMyInformationCountry",
        COLOR => "STT:WOLMyInformationColor",
        AUTO_LOGIN => "STT:WOLMyInformationAutoLogin",
        MATCH_SAME_RES => "STT:MatchSameRes",
        WINS => "STT:WOLMyInformationWins",
        LOSSES => "STT:WOLMyInformationLosses",
        DISCONNECTS => "STT:WOLMyInformationDisconnects",
        RANK => "STT:WOLMyInformationRank",
        POINTS => "STT:WOLMyInformationPoints",
        _ => return PAGE.button(control).map(|spec| spec.tooltip_key),
    })
}

/// One combo row: its text, item data and colour swatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MyInfoRow {
    pub(crate) text: String,
    pub(crate) data: i32,
    pub(crate) swatch: Option<usize>,
}

impl MyInfoRow {
    pub(crate) fn text(text: impl Into<String>, data: i32) -> Self {
        Self {
            text: text.into(),
            data,
            swatch: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MyInfoCombo {
    pub(crate) rows: Vec<MyInfoRow>,
    pub(crate) selected: Option<usize>,
    pub(crate) enabled: bool,
}

impl MyInfoCombo {
    fn new(rows: Vec<MyInfoRow>) -> Self {
        Self {
            rows,
            selected: None,
            enabled: true,
        }
    }

    /// The selection's item data, or `none` without one.
    pub(crate) fn data_or(&self, none: i32) -> i32 {
        self.selected
            .and_then(|row| self.rows.get(row))
            .map_or(none, |row| row.data)
    }

    /// `0x77A9F0`'s item-data search: the first row holding `data`, else the
    /// selection stays.
    fn select_data(&mut self, data: i32) {
        if let Some(row) = self.rows.iter().position(|row| row.data == data) {
            self.selected = Some(row);
        }
    }
}

/// What `0x114`'s init and refreshes read from a persona record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MyInfoPersona {
    pub(crate) server: String,
    pub(crate) locale: i32,
    pub(crate) country: i32,
    pub(crate) color: i32,
    pub(crate) match_by_res: bool,
}

/// The persona write or account change a control asks for; the app owner
/// runs it at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MyInfoAction {
    /// `0x628` CBN_SELCHANGE (`0x007991C7`): the slot's nickname (none for
    /// -1) becomes `[0xB73E40]` and the page refreshes from its persona.
    Nickname { slot: i32 },
    /// `0x629` (`0x00799368`): `0x77A4A0` with the face text.
    Server(String),
    /// `0x62A` (`0x007993DA`): `0x77A700(value, 3)`.
    Locale(i32),
    /// `0x6A1` (`0x00799450`): `0x77A780`.
    Country(i32),
    /// `0x6A2` (`0x007994CA`): `0x77A7A0`.
    Color(i32),
    /// `0x6FF` (`0x0079957A`): checked stores the nickname as AutoLogin;
    /// unchecked clears an AutoLogin naming it.
    AutoLogin(bool),
    /// `0x773` (`0x00799620`): `0x77A760`.
    MatchSameRes(bool),
    /// `0x6EB` (`0x00799692`) with a selected row: its narrowed text and item
    /// data. The model has already removed a stored row and reset itself.
    Forget { name: String, slot: i32 },
    /// Back (`0x007991A4`) or Play Buddy (`0x00799161`) closes the page.
    Close(u16),
    /// Clan, Tournament and Manage open `0x773E90`'s embedded browser with
    /// this registry key.
    Browser(&'static str),
}

/// Dialog `0x114` while it exists.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolMyInfoPage {
    /// [`COMBOS`] order.
    pub(crate) combos: [MyInfoCombo; 5],
    pub(crate) auto_login: bool,
    pub(crate) auto_login_enabled: bool,
    pub(crate) match_same_res: bool,
    pub(crate) forget_enabled: bool,
    pub(crate) play_buddy_enabled: bool,
    /// The heading, the five statistics and the update line by control id
    /// (`0x77A9F0`).
    pub(crate) texts: Vec<(u16, String)>,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) popup: ComboPopupInteraction<u16>,
    pub(crate) hovered: Option<u16>,
}

fn combo_index(control: u16) -> Option<usize> {
    COMBOS.iter().position(|id| *id == control)
}

/// The rows of one init (`0x00799910..0x00799B41`).
#[derive(Debug, Clone, Default)]
pub(crate) struct MyInfoRows {
    /// Stored nicknames with their slots, in slot order.
    pub(crate) nicknames: Vec<MyInfoRow>,
    pub(crate) default_text: String,
    /// `None`: one server record, so `0x629` holds the disabled
    /// [`NO_SERVERS`] row.
    pub(crate) servers: Option<Vec<String>>,
    pub(crate) locales: Vec<MyInfoRow>,
    pub(crate) countries: Vec<MyInfoRow>,
    pub(crate) colors: Vec<MyInfoRow>,
}

impl WolMyInfoPage {
    /// Init `0x497`: fill the combos (nickname, locale, country, colour,
    /// then server), then `0x77A9F0` with the nickname fill and the enables
    /// by slot alone. The colour and country start at row 0 (`0x14E` 0).
    pub(crate) fn open(
        rows: MyInfoRows,
        nickname: &str,
        persona: &MyInfoPersona,
        texts: Vec<(u16, String)>,
        auto_login_names_it: bool,
    ) -> Self {
        let mut nicknames = vec![MyInfoRow::text(rows.default_text, -1)];
        nicknames.extend(rows.nicknames);
        let mut nickname_combo = MyInfoCombo::new(nicknames);
        nickname_combo.selected = Some(0);
        let server = match rows.servers {
            Some(names) => MyInfoCombo::new(
                names
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| MyInfoRow::text(name, index as i32 + 1))
                    .collect(),
            ),
            None => MyInfoCombo {
                rows: vec![MyInfoRow::text(NO_SERVERS, 0)],
                selected: None,
                enabled: false,
            },
        };
        let mut country = MyInfoCombo::new(rows.countries);
        country.selected = Some(0);
        let mut color = MyInfoCombo::new(rows.colors);
        color.selected = Some(0);
        let mut page = Self {
            combos: [
                nickname_combo,
                server,
                MyInfoCombo::new(rows.locales),
                country,
                color,
            ],
            ..Self::default()
        };
        // 0x7896A0 selects the persona's server or row 0 before 0x77A9F0
        // looks again.
        let server = &mut page.combos[1];
        if server.enabled {
            server.selected = Some(
                server
                    .rows
                    .iter()
                    .position(|row| row.text.eq_ignore_ascii_case(&persona.server))
                    .unwrap_or(0),
            );
        }
        page.select_nickname(nickname);
        page.refresh(persona, texts);
        let slot = page.slot();
        page.forget_enabled = slot != -1;
        page.play_buddy_enabled = slot != -1;
        page.auto_login_enabled = slot != -1;
        page.auto_login = auto_login_names_it;
        page
    }

    /// `0x77A9F0`'s nickname part: the stored row, else `GUI:Default`.
    fn select_nickname(&mut self, nickname: &str) {
        let combo = &mut self.combos[0];
        let found = (!nickname.is_empty())
            .then(|| {
                combo
                    .rows
                    .iter()
                    .skip(1)
                    .position(|row| row.text.eq_ignore_ascii_case(nickname))
                    .map(|row| row + 1)
            })
            .flatten();
        combo.selected = Some(found.unwrap_or(0));
    }

    /// `0x77A9F0` without the nickname: the server by name (none when
    /// absent), the locale, country and colour by item data, MatchSameRes
    /// and the texts.
    pub(crate) fn refresh(&mut self, persona: &MyInfoPersona, texts: Vec<(u16, String)>) {
        let server = &mut self.combos[1];
        server.selected = server
            .rows
            .iter()
            .position(|row| row.text.eq_ignore_ascii_case(&persona.server));
        self.combos[2].select_data(persona.locale);
        self.combos[3].select_data(persona.country);
        self.combos[4].select_data(persona.color);
        self.match_same_res = persona.match_by_res;
        self.texts = texts;
    }

    /// The nickname combo's item data, -1 without a stored row.
    pub(crate) fn slot(&self) -> i32 {
        self.combos[0].data_or(-1)
    }

    pub(crate) fn combo(&self, control: u16) -> Option<&MyInfoCombo> {
        combo_index(control).map(|index| &self.combos[index])
    }

    pub(crate) fn enabled(&self, control: u16) -> bool {
        match control {
            FORGET => self.forget_enabled,
            PLAY_BUDDY => self.play_buddy_enabled,
            AUTO_LOGIN => self.auto_login_enabled,
            control => self.combo(control).is_none_or(|combo| combo.enabled),
        }
    }

    /// `0x628`'s handler after the app changed `[0xB73E40]` and loaded its
    /// persona: AutoLogin by slot, then Forget and Play Buddy by slot and a
    /// nonempty nickname. The combo itself is not refilled.
    pub(crate) fn nickname_changed(
        &mut self,
        persona: &MyInfoPersona,
        texts: Vec<(u16, String)>,
        nickname_set: bool,
        auto_login_names_it: bool,
    ) {
        let slot = self.slot();
        self.auto_login_enabled = slot != -1;
        self.auto_login = auto_login_names_it;
        self.refresh(persona, texts);
        self.forget_enabled = slot != -1 && nickname_set;
        self.play_buddy_enabled = slot != -1 && nickname_set;
    }

    /// A drop-list row was picked: CBN_SELCHANGE.
    pub(crate) fn select(&mut self, control: u16, row: usize) -> Option<MyInfoAction> {
        let combo = &mut self.combos[combo_index(control)?];
        if !combo.enabled || row >= combo.rows.len() {
            return None;
        }
        combo.selected = Some(row);
        let data = combo.rows[row].data;
        Some(match control {
            NICKNAME => MyInfoAction::Nickname { slot: data },
            SERVER => MyInfoAction::Server(combo.rows[row].text.clone()),
            LOCALE => MyInfoAction::Locale(data),
            COUNTRY => MyInfoAction::Country(data),
            _ => MyInfoAction::Color(data),
        })
    }

    /// A click on a checkbox's icon: BS_AUTOCHECKBOX flips it.
    pub(crate) fn toggle(&mut self, control: u16) -> Option<MyInfoAction> {
        match control {
            AUTO_LOGIN if self.auto_login_enabled => {
                self.auto_login = !self.auto_login;
                Some(MyInfoAction::AutoLogin(self.auto_login))
            }
            MATCH_SAME_RES => {
                self.match_same_res = !self.match_same_res;
                Some(MyInfoAction::MatchSameRes(self.match_same_res))
            }
            _ => None,
        }
    }

    /// A released button.
    pub(crate) fn activate(&mut self, control: u16) -> Option<MyInfoAction> {
        if !self.enabled(control) {
            return None;
        }
        match control {
            BACK | PLAY_BUDDY => Some(MyInfoAction::Close(control)),
            CLAN => Some(MyInfoAction::Browser("Battleclans")),
            TOURNAMENT => Some(MyInfoAction::Browser("Ladder")),
            MANAGE => Some(MyInfoAction::Browser("Manage")),
            FORGET => self.forget(),
            _ => None,
        }
    }

    /// Forget Account (`0x00799692..0x007998CC`): a stored row is deleted;
    /// then the selection returns to `GUI:Default` without a notification,
    /// Forget and AutoLogin go off. `[0xB73E40]` and Play Buddy stay.
    fn forget(&mut self) -> Option<MyInfoAction> {
        let combo = &mut self.combos[0];
        let action = combo.selected.and_then(|row| {
            let stored = combo.rows.get(row)?.clone();
            if stored.data != -1 {
                combo.rows.remove(row);
            }
            Some(MyInfoAction::Forget {
                name: stored.text,
                slot: stored.data,
            })
        });
        combo.selected = Some(0);
        self.forget_enabled = false;
        self.auto_login = false;
        self.auto_login_enabled = false;
        action
    }

    /// The drop list of the open combo.
    pub(crate) fn popup_geometry(&self, layout: &WolMyInfoLayout) -> Option<ComboPopup> {
        let control = self.popup.control?;
        let combo = self.combo(control).filter(|combo| combo.enabled)?;
        Some(ComboPopup::new(
            layout.face(control)?,
            layout.page.scale,
            combo.rows.len(),
            visible_rows(control),
            self.popup.top_index,
        ))
    }
}

/// The drop list's rows: country `0x4DE(0, 7)` (`0x004E3A36`), colour
/// `0x4DE(0, 9)` (`0x004E45E5`); the others as many as the template's
/// 143-DLU window holds below the face.
pub(crate) fn visible_rows(control: u16) -> usize {
    match control {
        COUNTRY => 7,
        COLOR => 9,
        _ => {
            let base = child_window(177, 49, 156, 143);
            ((base.h - COMBO_FACE_H - 1) / COMBO_DROPDOWN_ROW_H).max(1) as usize
        }
    }
}

/// `0x114` at the current screen: the family right panel and status line,
/// the left children at their template places (`0x0060B950`).
#[derive(Debug, Clone)]
pub(crate) struct WolMyInfoLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) status_help: RectPx,
    /// [`LABELS`] order.
    pub(crate) labels: [RectPx; 10],
    /// [`COMBOS`] order: the whole window (checked against the oracle) and
    /// its closed face.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) combos: [RectPx; 5],
    pub(crate) faces: [RectPx; 5],
    pub(crate) auto_login: RectPx,
    pub(crate) match_same_res: RectPx,
    pub(crate) heading: RectPx,
    pub(crate) group_box: RectPx,
    /// [`STATS`] order.
    pub(crate) stats: [RectPx; 5],
    pub(crate) update: RectPx,
}

impl WolMyInfoLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let status_template = RectPx::new(2, 355, 316, 12);
        let (mut page, status_help) = if scale.is_retail() {
            (
                menu_page::compute_layout(&PAGE, width as u32, height as u32),
                status_line_rect(status_template, width, height),
            )
        } else {
            (
                menu_page::compute_responsive_layout(&PAGE, width as u32, height as u32),
                scale.scale_rect(status_line_rect(
                    status_template,
                    SHELL_BASE_W,
                    SHELL_BASE_H,
                )),
            )
        };
        // Heading 0x694 is 108x11 DLU, one row taller than the family's.
        let mut title =
            menu_page::compute_layout(&PAGE, SHELL_BASE_W as u32, SHELL_BASE_H as u32).title;
        title.h = dlu_rect(424, 4, 108, 11).h + 1;
        if scale.is_retail() {
            page.title.h = title.h;
        } else {
            page.title = scale.scale_rect(title);
        }
        let px = |[x, y, w, h]: [i32; 4]| scale.scale_rect(child_window(x, y, w, h));
        let combo_tops = [49, 71, 93, 115, 137];
        let combos = combo_tops.map(|top| child_window(177, top, 156, 143));
        Self {
            page,
            status_help,
            labels: LABELS.map(|(_, rect)| px(rect)),
            combos: combos.map(|rect| scale.scale_rect(rect)),
            faces: combos
                .map(|rect| scale.scale_rect(RectPx::new(rect.x, rect.y, rect.w, COMBO_FACE_H))),
            auto_login: px([139, 160, 187, 10]),
            match_same_res: px([139, 175, 187, 10]),
            heading: px([78, 196, 255, 16]),
            group_box: px([78, 209, 255, 115]),
            stats: [222, 240, 258, 276, 294].map(|top| px([214, top, 110, 13])),
            update: px([86, 310, 237, 8]),
        }
    }

    pub(crate) fn face(&self, control: u16) -> Option<RectPx> {
        combo_index(control).map(|index| self.faces[index])
    }

    /// The child under `(x, y)`: buttons first, then the template's other
    /// children with status help.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        COMBOS
            .iter()
            .copied()
            .zip(self.faces)
            .chain([
                (AUTO_LOGIN, self.auto_login),
                (MATCH_SAME_RES, self.match_same_res),
            ])
            .chain(STATS.iter().copied().zip(self.stats))
            .find_map(|(id, rect)| rect.contains(x, y).then_some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(value: &serde_json::Value) -> RectPx {
        let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
        RectPx::new(a[0], a[1], a[2], a[3])
    }

    #[test]
    fn layout_and_help_follow_the_executed_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x114"]["sizes"].as_object().unwrap() {
            let (width, height) = size.split_once('x').unwrap();
            let layout = WolMyInfoLayout::new(width.parse().unwrap(), height.parse().unwrap());
            for (control, child) in case["controls"].as_object().unwrap() {
                let actual = match control.as_str() {
                    "0x71C" => layout.page.warning_monitor,
                    "0x694" => layout.page.title,
                    "0x695" => layout.status_help,
                    "0x6EF" => layout.heading,
                    "0x797" => layout.update,
                    "0x6FF" => layout.auto_login,
                    "0x773" => layout.match_same_res,
                    "0xFFFF#10" => layout.group_box,
                    key if key.starts_with("0xFFFF") => {
                        // Keys sort as text: #1, #10 (the box), #2 .. #9.
                        let index = key
                            .strip_prefix("0xFFFF#")
                            .map_or(0, |n| n.parse::<usize>().unwrap());
                        assert_eq!(
                            child["caption"].as_str(),
                            Some(LABELS[index].0),
                            "{size} {key}"
                        );
                        layout.labels[index]
                    }
                    key => {
                        let id = u16::from_str_radix(&key[2..], 16).unwrap();
                        if let Some(index) = combo_index(id) {
                            layout.combos[index]
                        } else if let Some(index) = STATS.iter().position(|s| *s == id) {
                            layout.stats[index]
                        } else {
                            layout
                                .page
                                .button_rect(id)
                                .unwrap_or_else(|| panic!("{size} {key}"))
                        }
                    }
                };
                assert_eq!(actual, rect(&child["rect"]), "{size} {control}");
                checked += 1;
            }
        }
        assert_eq!(checked, 3 * 34);
        for (control, help) in corpus["help"]["0x114"].as_object().unwrap() {
            let id = u16::from_str_radix(&control[2..], 16).unwrap();
            assert_eq!(tooltip_key(id), help.as_str(), "{control}");
        }
        let record = &corpus["slides"]["0x114"];
        let slide = crate::ui::shell::slide::slide_spec_for(DIALOG).unwrap();
        assert_eq!(
            slide.top_buttons as usize,
            record["top"].as_array().unwrap().len()
        );
        assert_eq!(
            slide.bottom_button,
            !record["bottom"].as_array().unwrap().is_empty()
        );
        assert_eq!(
            (slide.top_panel, slide.map_button),
            (record["d5"] != 0, record["d6"] != 0)
        );
    }

    fn persona() -> MyInfoPersona {
        MyInfoPersona {
            server: "Test Server".into(),
            locale: 5,
            country: 3,
            color: 2,
            match_by_res: false,
        }
    }

    fn rows() -> MyInfoRows {
        MyInfoRows {
            nicknames: vec![MyInfoRow::text("tester", 1), MyInfoRow::text("Other", 32)],
            default_text: "Default".into(),
            servers: Some(vec!["Ladder".into(), "Test Server".into()]),
            locales: vec![
                MyInfoRow::text("Unknown", 0),
                MyInfoRow::text("Five", 5),
                MyInfoRow::text("Six", 6),
            ],
            countries: (-2..5).map(|v| MyInfoRow::text(v.to_string(), v)).collect(),
            colors: (-2..8)
                .filter(|v| *v != -1)
                .map(|v| MyInfoRow::text("", v))
                .collect(),
        }
    }

    fn selected_data(page: &WolMyInfoPage) -> Vec<i32> {
        page.combos.iter().map(|combo| combo.data_or(-99)).collect()
    }

    #[test]
    fn init_fills_from_the_persona_and_enables_by_slot() {
        let page = WolMyInfoPage::open(rows(), "Tester", &persona(), Vec::new(), true);
        assert_eq!(selected_data(&page), [1, 2, 5, 3, 2]);
        assert!(page.forget_enabled && page.play_buddy_enabled && page.auto_login_enabled);
        assert!(page.auto_login && !page.match_same_res);

        // An empty or unlisted nickname shows Default; the server combo
        // loses its selection when the persona names none listed.
        let default = MyInfoPersona {
            server: String::new(),
            ..persona()
        };
        let mut page = WolMyInfoPage::open(rows(), "", &default, Vec::new(), false);
        assert_eq!(page.slot(), -1);
        assert_eq!(page.combos[1].selected, None);
        assert!(!page.forget_enabled && !page.play_buddy_enabled && !page.auto_login_enabled);
        assert!(!page.enabled(FORGET) && page.activate(FORGET).is_none());

        // One server record: the disabled placeholder.
        let mut single = rows();
        single.servers = None;
        let page = WolMyInfoPage::open(single, "tester", &persona(), Vec::new(), false);
        assert!(!page.enabled(SERVER));
        assert_eq!(page.combos[1].rows[0].text, NO_SERVERS);
    }

    #[test]
    fn handlers_follow_the_proc_table() {
        let mut page = WolMyInfoPage::open(rows(), "tester", &persona(), Vec::new(), false);
        assert_eq!(
            page.select(SERVER, 0),
            Some(MyInfoAction::Server("Ladder".into()))
        );
        assert_eq!(page.select(LOCALE, 2), Some(MyInfoAction::Locale(6)));
        assert_eq!(page.select(COUNTRY, 0), Some(MyInfoAction::Country(-2)));
        assert_eq!(page.select(COLOR, 1), Some(MyInfoAction::Color(0)));
        assert_eq!(page.select(COLOR, 99), None);
        assert_eq!(
            page.toggle(MATCH_SAME_RES),
            Some(MyInfoAction::MatchSameRes(true))
        );
        assert_eq!(page.toggle(AUTO_LOGIN), Some(MyInfoAction::AutoLogin(true)));
        assert_eq!(page.activate(BACK), Some(MyInfoAction::Close(BACK)));
        assert_eq!(
            page.activate(PLAY_BUDDY),
            Some(MyInfoAction::Close(PLAY_BUDDY))
        );
        assert_eq!(
            page.activate(CLAN),
            Some(MyInfoAction::Browser("Battleclans"))
        );
        assert_eq!(
            page.activate(TOURNAMENT),
            Some(MyInfoAction::Browser("Ladder"))
        );
        assert_eq!(page.activate(MANAGE), Some(MyInfoAction::Browser("Manage")));

        // The nickname change: Default clears the enables, a stored row with
        // a nickname sets them; AutoLogin follows the slot alone.
        assert_eq!(
            page.select(NICKNAME, 0),
            Some(MyInfoAction::Nickname { slot: -1 })
        );
        page.nickname_changed(&persona(), Vec::new(), false, false);
        assert!(!page.forget_enabled && !page.play_buddy_enabled && !page.auto_login_enabled);
        assert_eq!(
            page.select(NICKNAME, 2),
            Some(MyInfoAction::Nickname { slot: 32 })
        );
        page.nickname_changed(&persona(), Vec::new(), true, true);
        assert!(page.forget_enabled && page.play_buddy_enabled && page.auto_login);
    }

    #[test]
    fn forget_removes_the_row_and_keeps_play_buddy() {
        let mut page = WolMyInfoPage::open(rows(), "Other", &persona(), Vec::new(), true);
        assert_eq!(
            page.activate(FORGET),
            Some(MyInfoAction::Forget {
                name: "Other".into(),
                slot: 32
            })
        );
        let names: Vec<&str> = page.combos[0]
            .rows
            .iter()
            .map(|row| row.text.as_str())
            .collect();
        assert_eq!(names, ["Default", "tester"]);
        assert_eq!(page.slot(), -1);
        assert!(!page.forget_enabled && !page.auto_login && !page.auto_login_enabled);
        assert!(page.play_buddy_enabled);
    }

    #[test]
    fn drop_lists_hold_the_native_row_counts() {
        assert_eq!(visible_rows(COUNTRY), 7);
        assert_eq!(visible_rows(COLOR), 9);
        assert_eq!(visible_rows(NICKNAME), 9);
    }
}
