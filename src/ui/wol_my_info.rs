//! My Information `0x114` (proc `0x00799080`, driver `0x00799D30`): the
//! persona page behind the WOL welcome's My Information button. A family
//! page (`0x0060C540` accepts it) with five owner-draw combos, two
//! checkboxes and the ladder statistics.
//!
//! The model owns the controls and the proc's control logic; the persona
//! file, the credential store and the account globals belong to the app's
//! WOL owner, which runs each returned [`MyInfoAction`] at once (every
//! persona write is LoadRecord, the setter, then SaveProfileToINI).

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Rgba, Value};

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{RectPx, ShellScale, child_window};
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page;
use crate::ui::skirmish_shell::{COMBO_DROPDOWN_ROW_H, COMBO_FACE_H};

pub(crate) const DIALOG: DialogResourceId = DialogResourceId(0x0114);
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
        MenuPageButtonSpec::new(PLAY_BUDDY, 126, "GUI:PlayBuddy", "STT:WOLBuddy")
            .returning(PLAY_BUDDY as i32),
        MenuPageButtonSpec::new(CLAN, 152, "GUI:Clan", "STT:WOLClan"),
        MenuPageButtonSpec::new(TOURNAMENT, 178, "GUI:Tournament", "STT:WOLTournament"),
        MenuPageButtonSpec::new(FORGET, 204, "GUI:ForgetAccount", "STT:WOLLoginForget"),
        MenuPageButtonSpec::new(MANAGE, 230, "GUI:ManageAccount", "STT:WOLManageAccount"),
    ],
    back: MenuPageButtonSpec::new(BACK, 344, "GUI:Back", "STT:WOLMyInformationBack")
        .returning(BACK as i32),
};

/// The right-aligned captions of the combos and the statistics, in
/// template order (ids `0xFF00..`; their rects are `shell_layout.ini`'s).
pub(crate) const LABELS: [&str; 10] = [
    "GUI:LoginName",
    "GUI:PreferredServer",
    "GUI:Locale",
    "GUI:PreferredCountry",
    "GUI:PreferredColor",
    "GUI:LadderWins",
    "GUI:LadderLosses",
    "GUI:Disconnects",
    "GUI:LadderRank",
    "GUI:LadderPoints",
];

/// The statistics' group box (template static `0xFFFF#10`).
pub(crate) const GROUP_BOX: u16 = 0xFF0A;

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

/// Dialog `0x114` while it exists: its combos' rows and its texts. Its
/// widgets hold the selections, the enables and the boxes.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolMyInfoPage {
    /// [`COMBOS`] order.
    pub(crate) combos: [Vec<MyInfoRow>; 5],
    /// The heading, the five statistics and the update line by control id
    /// (`0x77A9F0`).
    pub(crate) texts: Vec<(u16, String)>,
    /// A text never written keeps its template `GUI:UnknownStats`.
    unknown: String,
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

fn set_flag(gui: &mut Gui, root: NodeId, ids: &[u16], prop: Prop, on: bool) {
    for id in ids {
        page::set(gui, root, *id, prop, Value::Bool(on));
    }
}

/// Combo `control`'s selected row.
pub(crate) fn selected(gui: &Gui, root: NodeId, control: u16) -> Option<usize> {
    usize::try_from(page::int(gui, root, control, Prop::Selected)?).ok()
}

impl WolMyInfoPage {
    /// Init `0x497` on the page's widgets: fill the combos (nickname,
    /// locale, country, colour, then server), then `0x77A9F0` with the
    /// nickname fill and the enables by slot alone. The colour and country
    /// start at row 0 (`0x14E` 0); `0x7896A0`'s server pick is redone by
    /// `0x77A9F0`. `unknown` is `GUI:UnknownStats`.
    pub(crate) fn open(
        gui: &mut Gui,
        root: NodeId,
        rows: MyInfoRows,
        nickname: &str,
        persona: &MyInfoPersona,
        texts: Vec<(u16, String)>,
        auto_login_names_it: bool,
        unknown: &str,
    ) -> Self {
        let mut nicknames = vec![MyInfoRow::text(rows.default_text, -1)];
        nicknames.extend(rows.nicknames);
        let servers = match &rows.servers {
            Some(names) => (names.iter().enumerate())
                .map(|(index, name)| MyInfoRow::text(name.clone(), index as i32 + 1))
                .collect(),
            None => vec![MyInfoRow::text(NO_SERVERS, 0)],
        };
        let mut page = Self {
            combos: [
                nicknames,
                servers,
                rows.locales,
                rows.countries,
                rows.colors,
            ],
            texts: Vec::new(),
            unknown: unknown.to_owned(),
        };
        for control in COMBOS {
            page.fill(gui, root, control);
            page::set(gui, root, control, Prop::Selected, Value::Int(-1));
        }
        set_flag(gui, root, &[SERVER], Prop::Enabled, rows.servers.is_some());
        for control in [COUNTRY, COLOR] {
            page::set(gui, root, control, Prop::Selected, Value::Int(0));
        }
        // 0x77A9F0's nickname part: the stored row, else `GUI:Default`.
        let found = (!nickname.is_empty())
            .then(|| {
                (page.combos[0].iter().skip(1))
                    .position(|row| row.text.eq_ignore_ascii_case(nickname))
                    .map(|row| row as i32 + 1)
            })
            .flatten();
        page::set(
            gui,
            root,
            NICKNAME,
            Prop::Selected,
            Value::Int(found.unwrap_or(0)),
        );
        page.refresh(gui, root, persona, texts);
        let stored = page.slot(gui, root) != -1;
        set_flag(
            gui,
            root,
            &[FORGET, PLAY_BUDDY, AUTO_LOGIN],
            Prop::Enabled,
            stored,
        );
        set_flag(gui, root, &[AUTO_LOGIN], Prop::Checked, auto_login_names_it);
        page
    }

    /// Combo `control`'s rows into its widget.
    fn fill(&self, gui: &mut Gui, root: NodeId, control: u16) {
        let Some(rows) = combo_index(control).map(|index| &self.combos[index]) else {
            return;
        };
        let items = |f: fn(&MyInfoRow) -> String| Value::Items(rows.iter().map(f).collect());
        page::set(
            gui,
            root,
            control,
            Prop::Items,
            items(|row| row.text.clone()),
        );
        let swatches = items(|row| row.swatch.map(|s| s.to_string()).unwrap_or_default());
        page::set(gui, root, control, Prop::Swatches, swatches);
    }

    /// `0x77A9F0`'s item-data search: the first row holding `data`, else the
    /// selection stays.
    fn select_data(&self, gui: &mut Gui, root: NodeId, control: u16, data: i32) {
        let rows = combo_index(control).map_or(&[][..], |index| &self.combos[index]);
        if let Some(row) = rows.iter().position(|row| row.data == data) {
            page::set(gui, root, control, Prop::Selected, Value::Int(row as i32));
        }
    }

    /// `0x77A9F0` without the nickname: the server by name (none when
    /// absent), the locale, country and colour by item data, MatchSameRes
    /// and the texts.
    pub(crate) fn refresh(
        &mut self,
        gui: &mut Gui,
        root: NodeId,
        persona: &MyInfoPersona,
        texts: Vec<(u16, String)>,
    ) {
        let server =
            (self.combos[1].iter()).position(|row| row.text.eq_ignore_ascii_case(&persona.server));
        let server = Value::Int(server.map_or(-1, |row| row as i32));
        page::set(gui, root, SERVER, Prop::Selected, server);
        self.select_data(gui, root, LOCALE, persona.locale);
        self.select_data(gui, root, COUNTRY, persona.country);
        self.select_data(gui, root, COLOR, persona.color);
        set_flag(
            gui,
            root,
            &[MATCH_SAME_RES],
            Prop::Checked,
            persona.match_by_res,
        );
        self.texts = texts;
        for id in [HEADING, UPDATE].into_iter().chain(STATS) {
            let text = self.texts.iter().find(|(control, _)| *control == id);
            let text = text.map_or(&self.unknown, |(_, text)| text);
            page::set(gui, root, id, Prop::Text, Value::Text(text.clone()));
        }
    }

    /// API+0xD0's count fills `0x62A` (`0x00798F50`) and selects `locale`.
    pub(crate) fn fill_locales(
        &mut self,
        gui: &mut Gui,
        root: NodeId,
        rows: Vec<MyInfoRow>,
        locale: i32,
    ) {
        self.combos[2] = rows;
        self.fill(gui, root, LOCALE);
        page::set(gui, root, LOCALE, Prop::Selected, Value::Int(-1));
        self.select_data(gui, root, LOCALE, locale);
    }

    /// Combo `control`'s selected row's item data, or `none` without one.
    pub(crate) fn data_or(&self, gui: &Gui, root: NodeId, control: u16, none: i32) -> i32 {
        let rows = combo_index(control).map_or(&[][..], |index| &self.combos[index]);
        selected(gui, root, control)
            .and_then(|row| rows.get(row))
            .map_or(none, |row| row.data)
    }

    /// The nickname combo's item data, -1 without a stored row.
    pub(crate) fn slot(&self, gui: &Gui, root: NodeId) -> i32 {
        self.data_or(gui, root, NICKNAME, -1)
    }

    /// `0x628`'s handler after the app changed `[0xB73E40]` and loaded its
    /// persona: AutoLogin by slot, then Forget and Play Buddy by slot and a
    /// nonempty nickname. The combo itself is not refilled.
    pub(crate) fn nickname_changed(
        &mut self,
        gui: &mut Gui,
        root: NodeId,
        persona: &MyInfoPersona,
        texts: Vec<(u16, String)>,
        nickname_set: bool,
        auto_login_names_it: bool,
    ) {
        let stored = self.slot(gui, root) != -1;
        set_flag(gui, root, &[AUTO_LOGIN], Prop::Enabled, stored);
        set_flag(gui, root, &[AUTO_LOGIN], Prop::Checked, auto_login_names_it);
        self.refresh(gui, root, persona, texts);
        let on = stored && nickname_set;
        set_flag(gui, root, &[FORGET, PLAY_BUDDY], Prop::Enabled, on);
    }

    /// A drop-list row was picked: CBN_SELCHANGE.
    pub(crate) fn select(&self, control: u16, row: usize) -> Option<MyInfoAction> {
        let picked = self.combos[combo_index(control)?].get(row)?;
        let data = picked.data;
        Some(match control {
            NICKNAME => MyInfoAction::Nickname { slot: data },
            SERVER => MyInfoAction::Server(picked.text.clone()),
            LOCALE => MyInfoAction::Locale(data),
            COUNTRY => MyInfoAction::Country(data),
            _ => MyInfoAction::Color(data),
        })
    }

    /// A box flipped (BS_AUTOCHECKBOX).
    pub(crate) fn toggle(control: u16, checked: bool) -> Option<MyInfoAction> {
        match control {
            AUTO_LOGIN => Some(MyInfoAction::AutoLogin(checked)),
            MATCH_SAME_RES => Some(MyInfoAction::MatchSameRes(checked)),
            _ => None,
        }
    }

    /// A released button.
    pub(crate) fn activate(
        &mut self,
        gui: &mut Gui,
        root: NodeId,
        control: u16,
    ) -> Option<MyInfoAction> {
        match control {
            BACK | PLAY_BUDDY => Some(MyInfoAction::Close(control)),
            CLAN => Some(MyInfoAction::Browser("Battleclans")),
            TOURNAMENT => Some(MyInfoAction::Browser("Ladder")),
            MANAGE => Some(MyInfoAction::Browser("Manage")),
            FORGET => self.forget(gui, root),
            _ => None,
        }
    }

    /// Forget Account (`0x00799692..0x007998CC`): a stored row is deleted;
    /// then the selection returns to `GUI:Default` without a notification,
    /// Forget and AutoLogin go off. `[0xB73E40]` and Play Buddy stay.
    fn forget(&mut self, gui: &mut Gui, root: NodeId) -> Option<MyInfoAction> {
        let action = selected(gui, root, NICKNAME).and_then(|row| {
            let stored = self.combos[0].get(row)?.clone();
            if stored.data != -1 {
                self.combos[0].remove(row);
            }
            Some(MyInfoAction::Forget {
                name: stored.text,
                slot: stored.data,
            })
        });
        self.fill(gui, root, NICKNAME);
        page::set(gui, root, NICKNAME, Prop::Selected, Value::Int(0));
        set_flag(gui, root, &[FORGET, AUTO_LOGIN], Prop::Enabled, false);
        set_flag(gui, root, &[AUTO_LOGIN], Prop::Checked, false);
        action
    }
}

/// `0x114`'s widgets: the column's buttons, the right-aligned captions, the
/// five combos (their drop lists of [`visible_rows`]), the two boxes (each
/// toggled on its icon's press), and the heading, update line and
/// statistics (the statistics with status help).
pub(crate) fn spec() -> NodeSpec {
    let align = |a: ShellAlign| Value::Int(i32::from(a.0));
    let help = |node: NodeSpec, id: u16| match tooltip_key(id) {
        Some(key) => node.id(id.into()).tooltip(key),
        None => node.id(id.into()),
    };
    let labels = LABELS.iter().enumerate().map(|(index, key)| {
        let id = 0xFF00 + index as u16;
        NodeSpec::new(NodeKind::Text)
            .id(id.into())
            .label(key)
            .with(Prop::Align, align(ShellAlign::H_RIGHT))
    });
    let combos = COMBOS.map(|id| {
        help(NodeSpec::new(NodeKind::Combo), id)
            .with(Prop::VisibleRows, Value::Int(visible_rows(id) as i32))
    });
    let boxes = [
        (AUTO_LOGIN, AUTO_LOGIN_KEY),
        (MATCH_SAME_RES, MATCH_SAME_RES_KEY),
    ]
    .map(|(id, key)| {
        // The caption greys as a disabled button's does.
        help(NodeSpec::new(NodeKind::Checkbox), id)
            .label(key)
            .with(Prop::Color, Value::Rgba(Rgba(0xFF, 0xFF, 0, 0xFF)))
            .with(Prop::DisabledColor, Value::Rgba(Rgba(0x48, 0, 0, 0xFF)))
    });
    let texts = [
        (HEADING, ShellAlign::H_CENTER),
        (UPDATE, ShellAlign::H_CENTER),
    ]
    .into_iter()
    .chain(STATS.map(|id| (id, ShellAlign::NONE)))
    .map(|(id, a)| help(NodeSpec::new(NodeKind::Text), id).with(Prop::Align, align(a)));
    NodeSpec::new(NodeKind::Dialog)
        .children(page::menu_buttons(&PAGE))
        .children(labels)
        .children(combos)
        .children(boxes)
        .children(texts)
}

/// The drop list's rows: country `0x4DE(0, 7)` (`0x004E3A36`), colour
/// `0x4DE(0, 9)` (`0x004E45E5`); the others as many as the template's
/// 143-DLU window holds below the face.
pub(crate) fn visible_rows(control: u16) -> usize {
    match control {
        COUNTRY => 7,
        COLOR => 9,
        _ => {
            let base = layout_data::entry_of("0x114", control)
                .map_or(0, |e| child_window(0, 0, 0, e.rect.h).h);
            ((base - COMBO_FACE_H - 1) / COMBO_DROPDOWN_ROW_H).max(1) as usize
        }
    }
}

/// `0x114` at the current screen: the family right panel and status line,
/// the left children at their template places (`0x0060B950`).
#[derive(Debug, Clone)]
pub(crate) struct WolMyInfoLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) rects: RectTable,
}

impl WolMyInfoLayout {
    pub(crate) fn new(width: i32, height: i32, scale: ShellScale) -> Self {
        let (page, rects) = layout_data::menu_page(&PAGE, "0x114", scale, width, height);
        Self { page, rects }
    }

    /// Where control `id` sits (a combo: its closed face).
    pub(crate) fn rect_of(&self, id: u16) -> Option<RectPx> {
        self.rects.get(id).or_else(|| self.page.button_rect(id))
    }
}

#[cfg(all(test, tests_ui))]
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
            let layout = WolMyInfoLayout::new(
                width.parse().unwrap(),
                height.parse().unwrap(),
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            for (control, child) in case["controls"].as_object().unwrap() {
                let actual = match control.as_str() {
                    "0x71C" => layout.page.warning_monitor,
                    "0x694" => layout.page.title,
                    "0x695" => layout.page.status_help,
                    key if key.starts_with("0xFFFF") => {
                        // Keys sort as text: #1, #10 (the box), #2 .. #9.
                        let index = key
                            .strip_prefix("0xFFFF#")
                            .map_or(0, |n| n.parse::<u16>().unwrap());
                        if let Some(caption) = LABELS.get(usize::from(index)) {
                            assert_eq!(child["caption"].as_str(), Some(*caption), "{size} {key}");
                        }
                        layout.rect_of(0xFF00 + index).unwrap()
                    }
                    key => {
                        let id = u16::from_str_radix(&key[2..], 16).unwrap();
                        let rect = layout.rect_of(id).unwrap_or_else(|| panic!("{size} {key}"));
                        if combo_index(id).is_some() {
                            // The oracle holds the whole window; the face is its top.
                            assert_eq!(rect.h, COMBO_FACE_H);
                            let entry = layout_data::entry_of("0x114", id).unwrap().rect;
                            let h = child_window(entry.x, entry.y, entry.w, entry.h).h;
                            RectPx { h, ..rect }
                        } else {
                            rect
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

    /// `0x114`'s widgets with the page opened on them.
    struct Page {
        gui: Gui,
        root: NodeId,
        page: WolMyInfoPage,
    }

    impl Page {
        fn open(rows: MyInfoRows, nickname: &str, persona: &MyInfoPersona, auto: bool) -> Self {
            let mut gui = Gui::new(64);
            let root = gui.create(NodeKind::Dialog, None).unwrap();
            page::ensure_built(&mut gui, root, spec);
            let page = WolMyInfoPage::open(
                &mut gui,
                root,
                rows,
                nickname,
                persona,
                Vec::new(),
                auto,
                "?",
            );
            Self { gui, root, page }
        }

        fn data(&self) -> Vec<i32> {
            (COMBOS.iter())
                .map(|id| self.page.data_or(&self.gui, self.root, *id, -99))
                .collect()
        }

        fn flag(&self, id: u16, prop: Prop) -> bool {
            page::get(&self.gui, self.root, id, prop) == Some(&Value::Bool(true))
        }

        fn enabled(&self, ids: &[u16]) -> Vec<bool> {
            ids.iter().map(|id| self.flag(*id, Prop::Enabled)).collect()
        }
    }

    #[test]
    fn init_fills_from_the_persona_and_enables_by_slot() {
        let page = Page::open(rows(), "Tester", &persona(), true);
        assert_eq!(page.data(), [1, 2, 5, 3, 2]);
        assert_eq!(page.enabled(&[FORGET, PLAY_BUDDY, AUTO_LOGIN]), [true; 3]);
        assert!(page.flag(AUTO_LOGIN, Prop::Checked) && !page.flag(MATCH_SAME_RES, Prop::Checked));

        // An empty or unlisted nickname shows Default; the server combo
        // loses its selection when the persona names none listed.
        let default = MyInfoPersona {
            server: String::new(),
            ..persona()
        };
        let page = Page::open(rows(), "", &default, false);
        assert_eq!(page.page.slot(&page.gui, page.root), -1);
        assert_eq!(selected(&page.gui, page.root, SERVER), None);
        assert_eq!(page.enabled(&[FORGET, PLAY_BUDDY, AUTO_LOGIN]), [false; 3]);

        // One server record: the disabled placeholder.
        let mut single = rows();
        single.servers = None;
        let page = Page::open(single, "tester", &persona(), false);
        assert_eq!(page.enabled(&[SERVER]), [false]);
        assert_eq!(page.page.combos[1][0].text, NO_SERVERS);
    }

    #[test]
    fn handlers_follow_the_proc_table() {
        let mut p = Page::open(rows(), "tester", &persona(), false);
        let page = &mut p.page;
        assert_eq!(
            page.select(SERVER, 0),
            Some(MyInfoAction::Server("Ladder".into()))
        );
        assert_eq!(page.select(LOCALE, 2), Some(MyInfoAction::Locale(6)));
        assert_eq!(page.select(COUNTRY, 0), Some(MyInfoAction::Country(-2)));
        assert_eq!(page.select(COLOR, 1), Some(MyInfoAction::Color(0)));
        assert_eq!(page.select(COLOR, 99), None);
        assert_eq!(
            WolMyInfoPage::toggle(MATCH_SAME_RES, true),
            Some(MyInfoAction::MatchSameRes(true))
        );
        assert_eq!(
            WolMyInfoPage::toggle(AUTO_LOGIN, true),
            Some(MyInfoAction::AutoLogin(true))
        );
        let (gui, root) = (&mut p.gui, p.root);
        for (control, action) in [
            (BACK, MyInfoAction::Close(BACK)),
            (PLAY_BUDDY, MyInfoAction::Close(PLAY_BUDDY)),
            (CLAN, MyInfoAction::Browser("Battleclans")),
            (TOURNAMENT, MyInfoAction::Browser("Ladder")),
            (MANAGE, MyInfoAction::Browser("Manage")),
        ] {
            assert_eq!(page.activate(gui, root, control), Some(action));
        }

        // The nickname change: Default clears the enables, a stored row with
        // a nickname sets them; AutoLogin follows the slot alone.
        assert_eq!(
            page.select(NICKNAME, 0),
            Some(MyInfoAction::Nickname { slot: -1 })
        );
        page::set(gui, root, NICKNAME, Prop::Selected, Value::Int(0));
        page.nickname_changed(gui, root, &persona(), Vec::new(), false, false);
        assert_eq!(p.enabled(&[FORGET, PLAY_BUDDY, AUTO_LOGIN]), [false; 3]);
        let (gui, root, page) = (&mut p.gui, p.root, &mut p.page);
        assert_eq!(
            page.select(NICKNAME, 2),
            Some(MyInfoAction::Nickname { slot: 32 })
        );
        page::set(gui, root, NICKNAME, Prop::Selected, Value::Int(2));
        page.nickname_changed(gui, root, &persona(), Vec::new(), true, true);
        assert_eq!(p.enabled(&[FORGET, PLAY_BUDDY]), [true; 2]);
        assert!(p.flag(AUTO_LOGIN, Prop::Checked));
    }

    #[test]
    fn forget_removes_the_row_and_keeps_play_buddy() {
        let mut p = Page::open(rows(), "Other", &persona(), true);
        assert_eq!(
            p.page.activate(&mut p.gui, p.root, FORGET),
            Some(MyInfoAction::Forget {
                name: "Other".into(),
                slot: 32
            })
        );
        let names: Vec<&str> = p.page.combos[0]
            .iter()
            .map(|row| row.text.as_str())
            .collect();
        assert_eq!(names, ["Default", "tester"]);
        assert_eq!(p.page.slot(&p.gui, p.root), -1);
        assert_eq!(
            p.enabled(&[FORGET, AUTO_LOGIN, PLAY_BUDDY]),
            [false, false, true]
        );
        assert!(!p.flag(AUTO_LOGIN, Prop::Checked));
    }

    #[test]
    fn drop_lists_hold_the_native_row_counts() {
        assert_eq!(visible_rows(COUNTRY), 7);
        assert_eq!(visible_rows(COLOR), 9);
        assert_eq!(visible_rows(NICKNAME), 9);
    }
}
