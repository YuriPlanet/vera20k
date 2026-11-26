//! In-battle Diplomacy dialog, RT_DIALOG 0x73 (in-scenario state 8):
//! `Show_Diplomacy_Menu` 0x006586D0, proc 0x00657DA0.
//!
//! Geometry: `ResizeShellChildControl` 0x0060C0C0 sends title 0x694 to the
//! right-panel static (0x00608D4B), Return 0x423 to the lower button
//! (0x006097AB -> 0x00609A81) and status line 0x695 to 0x0060B550; 0x73 is in
//! the 0x0060C3E1 list that sends every other child to 0x0060B7A0. The
//! template, those placements and the 0x006040B0 help keys are pinned by
//! `tools/storage_oracle/diplomacy_shell_layout.py`.
//!
//! The row contents ([`DiplomacyView`]) are resolved by the app from the
//! simulation; this module owns only control identity, geometry and the
//! pointer's help target.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use super::descriptor::DialogResourceId;
use super::geom::RectPx;
use super::in_game_shell::{self, InGameShellLayout};
use crate::ui::shell::checkbox::CheckboxFace;

pub const DIALOG: DialogResourceId = DialogResourceId(0x73);

/// Physical row slots. VERA pages the same slots when more Houses are shown.
pub const DIPLOMACY_ROWS: usize = 8;

/// Each row's control ids (0x8391FC, 0x83921C, 0x83923C, 0x83925C id
/// tables; insignia 0x004E3320 -> 0x6DA+row). The rects are
/// `shell_layout.ini`'s.
const NAME_IDS: [u16; DIPLOMACY_ROWS] = [0x411, 0x412, 0x413, 0x414, 0x415, 0x416, 0x417, 0x418];
const KILLS_IDS: [u16; DIPLOMACY_ROWS] = [0x419, 0x41A, 0x41B, 0x41C, 0x41D, 0x41E, 0x41F, 0x420];
const ALLIES_IDS: [u16; DIPLOMACY_ROWS - 1] = [0x6CF, 0x6D0, 0x6D1, 0x6D2, 0x6D3, 0x6D4, 0x6D5];
const CHAT_IDS: [u16; DIPLOMACY_ROWS - 1] = [0x6D6, 0x6D7, 0x6D8, 0x6D9, 0x6E2, 0x6E3, 0x6E4];

/// Column headers 0x421/0x597/0x425/0x422.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiplomacyHeader {
    Player,
    Allies,
    Chat,
    Kills,
}

impl DiplomacyHeader {
    pub const ALL: [Self; 4] = [Self::Player, Self::Allies, Self::Chat, Self::Kills];

    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Player => ("GUI:Player", "Player"),
            Self::Allies => ("GUI:Allies", "Allies"),
            Self::Chat => ("GUI:Chat", "Chat"),
            Self::Kills => ("GUI:Kills", "Kills"),
        }
    }
}

pub const TITLE_LABEL: (&str, &str) = ("GUI:DiplomacyMenu", "Diplomacy");
pub const RETURN_LABEL: (&str, &str) = ("GUI:Return", "Return");
/// 0x730's caption, drawn by the checkbox proc beside its box.
pub const TAUNTS_LABEL: (&str, &str) = ("GUI:TauntsOn", "Taunts On");

/// A child the pointer can rest on, for the status-line help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiplomacyControl {
    Return,
    PreviousPage,
    NextPage,
    Header(DiplomacyHeader),
    Insignia(usize),
    Name(usize),
    Kills(usize),
    /// Rows 1..=7.
    Allies(usize),
    /// Rows 1..=7.
    Chat(usize),
    Taunts,
    ModeName,
    MapName,
}

impl DiplomacyControl {
    /// The status-line help key: the 0x006040B0 table for dialog 0x73
    /// (`diplomacy_shell_layout.json`). The 0x73 proc answers no 0x4E9.
    pub fn help_key(self) -> Option<&'static str> {
        match self {
            Self::Return => Some("STT:DiplomacyButtonReturn"),
            Self::Name(_) => Some("STT:DiplomacyLabelPlayer"),
            Self::Kills(_) => Some("STT:DiplomacyCBoxKills"),
            Self::Allies(_) => Some("STT:DiplomacyCBoxAllies"),
            Self::Chat(_) => Some("STT:DiplomacyCBoxListeners"),
            Self::Taunts => Some("STT:TauntsOn"),
            Self::Header(_)
            | Self::Insignia(_)
            | Self::ModeName
            | Self::MapName
            | Self::PreviousPage
            | Self::NextPage => None,
        }
    }

    /// Every child in template (Z) order, as the common handler's
    /// `ChildWindowFromPointEx(.., CWP_SKIPINVISIBLE)` (0x00622D24) finds
    /// them; VERA's paging buttons follow Return.
    fn all() -> impl Iterator<Item = Self> {
        [Self::Return, Self::PreviousPage, Self::NextPage]
            .into_iter()
            .chain((0..DIPLOMACY_ROWS).flat_map(|row| {
                [
                    Self::Insignia(row),
                    Self::Name(row),
                    Self::Kills(row),
                    Self::Allies(row),
                    Self::Chat(row),
                ]
            }))
            .chain(DiplomacyHeader::ALL.map(Self::Header))
            .chain([Self::ModeName, Self::MapName, Self::Taunts])
    }

    pub fn from_id(id: u16) -> Option<Self> {
        Self::all().find(|control| control.id() == id)
    }

    /// Native control id.
    pub fn id(self) -> u16 {
        match self {
            Self::Return => 0x423,
            Self::PreviousPage => super::paging::PREVIOUS,
            Self::NextPage => super::paging::NEXT,
            Self::Header(DiplomacyHeader::Player) => 0x421,
            Self::Header(DiplomacyHeader::Allies) => 0x597,
            Self::Header(DiplomacyHeader::Chat) => 0x425,
            Self::Header(DiplomacyHeader::Kills) => 0x422,
            Self::Insignia(row) => 0x6DA + row as u16,
            Self::Name(row) => NAME_IDS[row],
            Self::Kills(row) => KILLS_IDS[row],
            Self::Allies(0) => 0xF203,
            Self::Chat(0) => 0xF204,
            Self::Allies(row) => ALLIES_IDS[row - 1],
            Self::Chat(row) => CHAT_IDS[row - 1],
            Self::Taunts => 0x730,
            Self::ModeName => 0x426,
            Self::MapName => 0x71B,
        }
    }
}

/// Physical rectangles of every control in the active-game shell's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiplomacyLayout {
    pub title: RectPx,
    pub return_button: RectPx,
    pub paging: [RectPx; 2],
    pub status_line: RectPx,
    pub headers: [RectPx; 4],
    pub insignia: [RectPx; DIPLOMACY_ROWS],
    pub names: [RectPx; DIPLOMACY_ROWS],
    pub kills: [RectPx; DIPLOMACY_ROWS],
    /// Slot 0 has no box on the first page; later pages show a peer there.
    pub allies: [RectPx; DIPLOMACY_ROWS],
    pub chat: [RectPx; DIPLOMACY_ROWS],
    pub taunts: RectPx,
    pub mode_name: RectPx,
    pub map_name: RectPx,
}

pub fn diplomacy_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
) -> DiplomacyLayout {
    let rects = super::layout_data::in_game("0x73", &shell, width, height, button_size);
    let rect = |control: DiplomacyControl| rects.get(control.id()).unwrap_or_default();
    let rows =
        |control: fn(usize) -> DiplomacyControl| std::array::from_fn(|row| rect(control(row)));
    let return_button = rect(DiplomacyControl::Return);
    DiplomacyLayout {
        title: in_game_shell::right_panel_title_rect(width),
        return_button,
        paging: super::paging::button_rects(return_button),
        status_line: in_game_shell::status_line_rect(height),
        headers: DiplomacyHeader::ALL.map(|header| rect(DiplomacyControl::Header(header))),
        insignia: rows(DiplomacyControl::Insignia),
        names: rows(DiplomacyControl::Name),
        kills: rows(DiplomacyControl::Kills),
        allies: rows(DiplomacyControl::Allies),
        chat: rows(DiplomacyControl::Chat),
        taunts: rect(DiplomacyControl::Taunts),
        mode_name: rect(DiplomacyControl::ModeName),
        map_name: rect(DiplomacyControl::MapName),
    }
}

impl DiplomacyLayout {
    pub fn control_rect(&self, control: DiplomacyControl) -> RectPx {
        match control {
            DiplomacyControl::Return => self.return_button,
            DiplomacyControl::PreviousPage => self.paging[0],
            DiplomacyControl::NextPage => self.paging[1],
            DiplomacyControl::Header(header) => self.headers[header as usize],
            DiplomacyControl::Insignia(row) => self.insignia[row],
            DiplomacyControl::Name(row) => self.names[row],
            DiplomacyControl::Kills(row) => self.kills[row],
            DiplomacyControl::Allies(row) => self.allies[row],
            DiplomacyControl::Chat(row) => self.chat[row],
            DiplomacyControl::Taunts => self.taunts,
            DiplomacyControl::ModeName => self.mode_name,
            DiplomacyControl::MapName => self.map_name,
        }
    }
}

/// One Allies box: the dual-state checkbox 0x006163A0 after 0x4E5(1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiplomacyAlliesBox {
    /// BM_SETCHECK: the local player allies this house.
    pub checked: bool,
    /// 0x4E6: this house allies the local player.
    pub partner: bool,
    /// Enabled only for another human while the local player is active.
    pub enabled: bool,
}

impl DiplomacyAlliesBox {
    pub fn face(self) -> CheckboxFace {
        CheckboxFace::dual_state(self.checked, self.partner)
    }
}

/// One shown row (init 0x497 arm, refresh 0x00658330).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiplomacyRow {
    /// The 0x004E3560 insignia PCX, when the house type has one.
    pub insignia: Option<String>,
    /// House UI name (House+0x1602A).
    pub name: String,
    /// Name and kills text colour (0x498).
    pub rgb: [u8; 3],
    /// Kills, formatted `%d`; `None` leaves the static blank.
    pub kills: Option<i32>,
    /// `None` when hidden; row 0 never has one.
    pub allies: Option<DiplomacyAlliesBox>,
    /// `None` when hidden. Shown only for human rows.
    pub chat: Option<DiplomacyChatBox>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiplomacyChatBox {
    pub checked: bool,
    pub enabled: bool,
}

/// The dialog's resolved contents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiplomacyView {
    pub rows: [Option<DiplomacyRow>; DIPLOMACY_ROWS],
    /// VERA extension: the physical rows window the full House-order projection.
    pub first_row: usize,
    pub page_index: usize,
    pub page_count: usize,
    /// 0x597, hidden when no row showed an Allies box at init.
    pub allies_header: bool,
    /// 0x425, hidden when no row is human-controlled.
    pub chat_header: bool,
    /// 0x426 MPGameMode title, set only in LAN/WOL; otherwise `GUI:Blank`.
    pub mode_name: Option<String>,
    /// 0x730 TauntsOn: shown, never disabled, with the mode's check in
    /// LAN/WOL; `None` (hidden) otherwise.
    pub taunts: Option<bool>,
    /// 0x71B: the selected scenario's description (Session+0xEA).
    pub map_name: String,
}

/// Every child: Return and the paging buttons, the boxes (taking their
/// press, `0x00616700`; the row boxes anywhere in their window), the
/// insignia pictures and the left-aligned statics, each with its status
/// help, then VERA's page counter, the title and the status line. Disabled
/// children still hover.
pub fn spec() -> NodeSpec {
    let left = Value::Int(i32::from(crate::ui::shell::geom::ShellAlign::NONE.0));
    let controls = DiplomacyControl::all().map(|control| {
        let kind = match control {
            DiplomacyControl::Return
            | DiplomacyControl::PreviousPage
            | DiplomacyControl::NextPage => NodeKind::Button,
            DiplomacyControl::Allies(_) | DiplomacyControl::Chat(_) | DiplomacyControl::Taunts => {
                NodeKind::Checkbox
            }
            DiplomacyControl::Insignia(_) => NodeKind::Image,
            _ => NodeKind::Text,
        };
        let node = NodeSpec::new(kind)
            .id(control.id().into())
            .tooltip(control.help_key().unwrap_or_default());
        match control {
            DiplomacyControl::PreviousPage => node.label(super::paging::PREVIOUS_LABEL),
            DiplomacyControl::NextPage => node.label(super::paging::NEXT_LABEL),
            _ if kind == NodeKind::Text => node.with(Prop::Align, left.clone()),
            _ => node,
        }
    });
    NodeSpec::new(NodeKind::Dialog)
        .children(controls)
        .child(super::paging::counter_node())
        .children(in_game_shell::panel_statics())
}

impl DiplomacyView {
    fn shows(&self, control: DiplomacyControl) -> bool {
        let row = |row: usize| self.rows[row].as_ref();
        match control {
            DiplomacyControl::Return | DiplomacyControl::ModeName | DiplomacyControl::MapName => {
                true
            }
            DiplomacyControl::PreviousPage | DiplomacyControl::NextPage => self.page_count > 1,
            DiplomacyControl::Insignia(r)
            | DiplomacyControl::Name(r)
            | DiplomacyControl::Kills(r) => row(r).is_some(),
            DiplomacyControl::Allies(r) => row(r).is_some_and(|row| row.allies.is_some()),
            DiplomacyControl::Chat(r) => row(r).is_some_and(|row| row.chat.is_some()),
            DiplomacyControl::Header(DiplomacyHeader::Allies) => self.allies_header,
            DiplomacyControl::Header(DiplomacyHeader::Chat) => self.chat_header,
            DiplomacyControl::Header(_) => true,
            DiplomacyControl::Taunts => self.taunts.is_some(),
        }
    }

    /// Control `id`'s rect; `None` while it is hidden.
    pub fn rect(&self, layout: &DiplomacyLayout, id: u16) -> Option<RectPx> {
        match id {
            super::paging::COUNTER => {
                return (self.page_count > 1).then(|| super::paging::counter_rect(layout.paging));
            }
            in_game_shell::TITLE => return Some(layout.title),
            in_game_shell::FOOTER => return Some(layout.status_line),
            _ => {}
        }
        let control = DiplomacyControl::from_id(id)?;
        self.shows(control).then(|| layout.control_rect(control))
    }

    /// The rows on their nodes under `root`: the boxes' states (an Allies
    /// box's partner flag in `Selected`, its dual-state face), the insignia
    /// PCX in `Art`, names and kills in the row's colour, the mode and map.
    /// Texts allocate only when they change.
    pub fn sync(&self, layout: &DiplomacyLayout, gui: &mut Gui, root: NodeId) {
        use super::page::{set, set_text};
        let boxed = |gui: &mut Gui, control: DiplomacyControl, checked: bool, enabled: bool| {
            let id = control.id();
            set(gui, root, id, Prop::Checked, Value::Bool(checked));
            set(gui, root, id, Prop::Enabled, Value::Bool(enabled));
            let rect = layout.control_rect(control);
            if control != DiplomacyControl::Taunts {
                set(gui, root, id, Prop::HitRect, Value::Rect(rect));
            }
        };
        for (index, row) in self.rows.iter().enumerate() {
            let Some(row) = row else { continue };
            let insignia = row.insignia.as_deref().unwrap_or_default();
            set_text(gui, root, DiplomacyControl::Insignia(index).id(), Prop::Art, insignia);
            let [r, g, b] = row.rgb;
            let rgb = Value::Rgba(vera20k_gui::Rgba(r, g, b, 255));
            let name = DiplomacyControl::Name(index).id();
            set_text(gui, root, name, Prop::Text, &row.name);
            set(gui, root, name, Prop::Color, rgb.clone());
            let kills = DiplomacyControl::Kills(index).id();
            match row.kills {
                Some(count) => set_text(gui, root, kills, Prop::Text, &count.to_string()),
                None => set_text(gui, root, kills, Prop::Text, ""),
            }
            set(gui, root, kills, Prop::Color, rgb);
            if let Some(allies) = row.allies {
                let control = DiplomacyControl::Allies(index);
                boxed(gui, control, allies.checked, allies.enabled);
                let partner = Value::Int(i32::from(allies.partner));
                set(gui, root, control.id(), Prop::Selected, partner);
            }
            if let Some(chat) = row.chat {
                boxed(gui, DiplomacyControl::Chat(index), chat.checked, chat.enabled);
            }
        }
        if let Some(on) = self.taunts {
            boxed(gui, DiplomacyControl::Taunts, on, true);
        }
        let mode = self.mode_name.as_deref().unwrap_or_default();
        set_text(gui, root, DiplomacyControl::ModeName.id(), Prop::Text, mode);
        set_text(gui, root, DiplomacyControl::MapName.id(), Prop::Text, &self.map_name);
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::in_game_shell::allied_fixture_layout as shell;

    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/diplomacy_shell_layout.json",
        ))
        .unwrap()
    }

    fn all_controls() -> Vec<DiplomacyControl> {
        let mut out = vec![DiplomacyControl::Return];
        out.extend(DiplomacyHeader::ALL.map(DiplomacyControl::Header));
        for row in 0..DIPLOMACY_ROWS {
            out.extend([
                DiplomacyControl::Insignia(row),
                DiplomacyControl::Name(row),
                DiplomacyControl::Kills(row),
            ]);
            if row > 0 {
                out.extend([DiplomacyControl::Allies(row), DiplomacyControl::Chat(row)]);
            }
        }
        out.extend([
            DiplomacyControl::Taunts,
            DiplomacyControl::ModeName,
            DiplomacyControl::MapName,
        ]);
        out
    }

    /// The Allied shell at a stock resolution, from the executed geometry
    /// fixture's loaded canvas sizes.

    /// Every ordinary child lands where original 0x0060B7A0 put it at 640x480,
    /// 800x600 and 1024x768, from the pinned template's own rects.
    #[test]
    fn ordinary_children_match_the_executed_native_placement() {
        let corpus = corpus();
        let template: std::collections::HashMap<u64, Vec<i64>> = corpus["controls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let rect = c["dlu_rect"].as_array().unwrap();
                (
                    c["control_id"].as_u64().unwrap(),
                    rect.iter().map(|v| v.as_i64().unwrap()).collect(),
                )
            })
            .collect();
        let cases = corpus["cases"].as_array().unwrap();
        let controls = all_controls();
        let mut checked = 0;
        for control in &controls {
            if *control == DiplomacyControl::Return {
                continue;
            }
            let id = u64::from(control.id());
            for case in cases.iter().filter(|case| case["control_id"] == id) {
                let (width, height) = (
                    case["width"].as_i64().unwrap() as i32,
                    case["height"].as_i64().unwrap() as i32,
                );
                let layout = diplomacy_layout(width, height, shell(width, height), [125, 25]);
                let rect = layout.control_rect(*control);
                assert_eq!(
                    serde_json::json!([rect.x, rect.y, rect.w, rect.h]),
                    case["rect"],
                    "control {id:#x} at {width}x{height} (template {:?})",
                    template[&id]
                );
                checked += 1;
            }
        }
        // 4 headers, 8x3 row statics, 7x2 boxes, taunts, mode and map names.
        assert_eq!(checked, (4 + 24 + 14 + 3) * 3);
    }

    /// Title 0x694, Return 0x423 and status line 0x695 take B5's title,
    /// Resume and footer places (0x00608D4B, 0x006097AB, 0x00601360).
    #[test]
    fn title_return_and_status_line_share_the_pause_menu_placements() {
        use crate::ui::shell::pause_menu::{PauseMenuButton, PauseMenuTemplate, pause_menu_layout};
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let shell = shell(width, height);
            let layout = diplomacy_layout(width, height, shell, [125, 25]);
            let pause =
                pause_menu_layout(width, height, shell, [125, 25], PauseMenuTemplate::Offline);
            assert_eq!(layout.title, pause.title);
            assert_eq!(
                layout.return_button,
                pause.button(PauseMenuButton::Resume).unwrap()
            );
            assert_eq!(layout.status_line, pause.footer);
        }
    }

    /// The help keys are the executed 0x006040B0 answers for dialog 0x73.
    #[test]
    fn help_keys_match_the_native_help_table() {
        let corpus = corpus();
        for control in all_controls() {
            let native = corpus["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["control_id"] == u64::from(control.id()))
                .unwrap_or_else(|| panic!("{control:?} is in the template"));
            assert_eq!(
                control.help_key().map(str::to_owned),
                native["help"].as_str().map(str::to_owned),
                "{control:?}"
            );
        }
    }

    /// The painter's props: the insignia picture, the row texts in the row
    /// colour, the Allies box's partner flag, the mode and map texts.
    #[test]
    fn sync_puts_the_rows_on_their_nodes() {
        use crate::ui::shell::page::get;
        let layout = diplomacy_layout(800, 600, shell(800, 600), [125, 25]);
        let mut view = DiplomacyView {
            map_name: "Dustbowl".into(),
            ..DiplomacyView::default()
        };
        view.rows[1] = Some(DiplomacyRow {
            insignia: Some("usai.pcx".into()),
            name: "Peer".into(),
            rgb: [10, 20, 30],
            kills: Some(7),
            allies: Some(DiplomacyAlliesBox {
                checked: true,
                partner: true,
                enabled: false,
            }),
            chat: None,
        });
        let mut gui = Gui::new(256);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        crate::ui::shell::page::ensure_built(&mut gui, root, spec);
        view.sync(&layout, &mut gui, root);
        let text = |gui: &Gui, control: DiplomacyControl| {
            match get(gui, root, control.id(), Prop::Text) {
                Some(Value::Text(text)) => text.clone(),
                _ => String::from("<none>"),
            }
        };
        let insignia = gui.find_id(root, DiplomacyControl::Insignia(1).id().into()).unwrap();
        assert_eq!(gui.tree().kind(insignia).unwrap(), NodeKind::Image);
        assert_eq!(
            get(&gui, root, DiplomacyControl::Insignia(1).id(), Prop::Art),
            Some(&Value::Text("usai.pcx".into()))
        );
        assert_eq!(text(&gui, DiplomacyControl::Name(1)), "Peer");
        assert_eq!(text(&gui, DiplomacyControl::Kills(1)), "7");
        assert_eq!(
            get(&gui, root, DiplomacyControl::Kills(1).id(), Prop::Color),
            Some(&Value::Rgba(vera20k_gui::Rgba(10, 20, 30, 255)))
        );
        let allies = DiplomacyControl::Allies(1).id();
        assert_eq!(get(&gui, root, allies, Prop::Selected), Some(&Value::Int(1)));
        assert_eq!(get(&gui, root, allies, Prop::Enabled), Some(&Value::Bool(false)));
        assert_eq!(text(&gui, DiplomacyControl::ModeName), "");
        assert_eq!(text(&gui, DiplomacyControl::MapName), "Dustbowl");
    }

    #[test]
    fn hidden_children_take_no_pointer_and_disabled_ones_do() {
        let layout = diplomacy_layout(800, 600, shell(800, 600), [125, 25]);
        let row = |allies| DiplomacyRow {
            insignia: None,
            name: String::new(),
            rgb: [0; 3],
            kills: None,
            allies,
            chat: None,
        };
        let disabled = DiplomacyAlliesBox {
            checked: false,
            partner: false,
            enabled: false,
        };
        let mut view = DiplomacyView {
            rows: [
                Some(row(None)),
                Some(row(Some(disabled))),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            ..DiplomacyView::default()
        };
        let control_at = |view: &DiplomacyView, x, y| {
            let mut gui = Gui::new(256);
            let root = gui.create(NodeKind::Dialog, None).unwrap();
            crate::ui::shell::page::ensure_built(&mut gui, root, spec);
            crate::ui::shell::page::sync_rects(&mut gui, root, |id| view.rect(&layout, id));
            view.sync(&layout, &mut gui, root);
            let node = gui.hit_at(root, x, y, false)?;
            DiplomacyControl::from_id(gui.id_of(node)? as u16)
        };
        let allies = layout.allies[1];
        assert_eq!(
            control_at(&view, allies.x, allies.y),
            Some(DiplomacyControl::Allies(1))
        );
        let chat = layout.chat[1];
        assert_eq!(control_at(&view, chat.x, chat.y), None);
        let name2 = layout.names[2];
        assert_eq!(control_at(&view, name2.x, name2.y), None);
        view.rows[1] = Some(row(None));
        assert_eq!(control_at(&view, allies.x, allies.y), None);
        let taunts = layout.taunts;
        assert_eq!(control_at(&view, taunts.x, taunts.y), None);
        view.taunts = Some(false);
        assert_eq!(
            control_at(&view, taunts.x, taunts.y),
            Some(DiplomacyControl::Taunts)
        );
    }
}
