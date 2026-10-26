//! Active retail D7 Network Options dialog (00560480), opened by 0055FC80.
//! Draft edits live here; the app's profile owns persisted values. Ordered
//! effects preserve the native partial commit and modal-warning boundaries.

use crate::rules::ini_parser::ini_value::{crt_atoi, scan_hex_u32, strtok};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::ComboPopup;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, ShellScale, center_offset, child_window, dlu_rect};
use crate::ui::shell::list::ListScrollInteraction;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::{COMBO_FACE_H, SavedSeedDescriptionEdit};

pub(crate) const OK: u16 = 1;
pub(crate) const CANCEL: u16 = 2;
pub(crate) const SOCKET: u16 = 0x5D1;
pub(crate) const DESTINATION: u16 = 0x5D3;
pub(crate) const CARD: u16 = 0x5D4;
pub(crate) const PORT: u16 = 0x5D7;
pub(crate) const SEND_DELAY: u16 = 0x762;
// Configured D7 ComboBox: native User32 drop rect is 72 px with 23 px
// GAME.FNT rows (observed with one and eight adapter responses). 61811E
// computes (height-1)/row_height, and 6181DA rounds the popup to whole rows.
// D7 never supplies the 4DE visible-row override used by setup combos.
const CARD_VISIBLE_ROWS: usize = 3;

pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xD7),
    title_key: "GUI:IPXOptions",
    stacked: &[MenuPageButtonSpec {
        id: OK,
        dlu_top: 122,
        csf_key: "GUI:Ok",
        tooltip_key: "STT:MultiOptButtonOk",
        result: Some(2),
    }],
    back: MenuPageButtonSpec {
        id: CANCEL,
        dlu_top: 346,
        csf_key: "GUI:Cancel",
        tooltip_key: "STT:MultiOptButtonCancel",
        result: Some(2),
    },
};

/// Native OwnerDraw__GetTooltipStringLabel 006040B0, D7 branch.
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    Some(match control {
        CARD => "STT:MultiOptComboCard",
        SOCKET => "STT:MultiOptEditSocketNum",
        DESTINATION => "STT:MultiOptEditDestAddr",
        SEND_DELAY => "STT:MultiOptSendDelay",
        PORT => "STT:MultiOptPortNumber",
        OK => "STT:MultiOptButtonOk",
        CANCEL => "STT:MultiOptButtonCancel",
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkOptionsMessage {
    InvalidSocket,
    InvalidDestination,
    InvalidPort,
    SendDelayWarning,
    PortWarning,
}

impl NetworkOptionsMessage {
    pub(crate) fn csf_key(self) -> &'static str {
        match self {
            Self::InvalidSocket => "TXT_INVALID_SOCKET",
            Self::InvalidDestination => "TXT_DESTNET_ADDR_ERROR",
            Self::InvalidPort => "TXT_PORT_NUMBER_ERROR",
            Self::SendDelayWarning => "TXT_FIREWALL_SETTINGS_WARNING1",
            Self::PortWarning => "TXT_FIREWALL_SETTINGS_WARNING2",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkOptionsEffect {
    Lan {
        socket: u16,
        destination: String,
        card: i32,
    },
    Message(NetworkOptionsMessage),
    SendDelay(bool),
    /// Store port, reset firewall detection, write the MultiPlayer settings,
    /// then close with native result 2 (00560B60..00560BD4).
    PortAndClose(i32),
    /// Cancel also returns result 2, but writes no new values.
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Editing,
    Error(NetworkOptionsMessage),
    SendDelayWarning(i32),
    PortWarning(i32),
    Closed,
}

#[derive(Debug, Clone)]
pub(crate) struct NetworkOptionsState {
    pub(crate) socket: SavedSeedDescriptionEdit,
    pub(crate) destination: SavedSeedDescriptionEdit,
    pub(crate) port: SavedSeedDescriptionEdit,
    /// Successful card address strings in the service's enumeration order.
    /// The empty list is displayed as TXT_NO_BOUND_NET_CARD, at index zero.
    pub(crate) cards: Vec<String>,
    pub(crate) selected_card: usize,
    pub(crate) send_delay: bool,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) focused_control: u16,
    pub(crate) combo_open: bool,
    pub(crate) card_top_index: usize,
    pub(crate) card_hot_row: Option<usize>,
    pub(crate) card_scroll: ListScrollInteraction,
    focus_started_at: crate::util::time::Instant,
    original_send_delay: bool,
    original_port: i32,
    phase: Phase,
}

impl NetworkOptionsState {
    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
        self.card_scroll.cancel();
    }

    pub(crate) fn new(
        socket: u16,
        destination: &str,
        card: i32,
        send_delay: bool,
        port: i32,
        cards: &[String],
    ) -> Self {
        let mut socket_edit = SavedSeedDescriptionEdit::default();
        if socket != u16::MAX {
            socket_edit.set_units_limited(socket.to_string().encode_utf16(), 5);
        }
        let mut destination_edit = SavedSeedDescriptionEdit::default();
        destination_edit.set_units_limited(destination.encode_utf16(), 11);
        let mut port_edit = SavedSeedDescriptionEdit::default();
        if port != 0 {
            port_edit.set_units_limited(port.to_string().encode_utf16(), 0);
        }
        // NewEdit replacement creation leaves the last edit focused. Native
        // D7 GUI thread readback after entry: GetGUIThreadInfo -> control 5D7;
        // an untouched Enter is consequently consumed by NewEdit, not IDOK.
        port_edit.focused = true;
        // 00560573 enumerates the first twenty card slots and does not add
        // duplicate address strings to its combo.
        let mut choices: Vec<String> = Vec::new();
        for address in cards.iter().take(20) {
            if !choices
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(address))
            {
                choices.push(address.clone());
            }
        }
        let selected_card = usize::try_from(card)
            .ok()
            .filter(|index| *index < choices.len())
            .unwrap_or(0);
        Self {
            socket: socket_edit,
            destination: destination_edit,
            port: port_edit,
            cards: choices,
            selected_card,
            send_delay,
            buttons: ShellButtonInteraction::default(),
            focused_control: PORT,
            combo_open: false,
            card_top_index: 0,
            card_hot_row: None,
            card_scroll: ListScrollInteraction::default(),
            focus_started_at: crate::util::time::Instant::now(),
            original_send_delay: send_delay,
            original_port: port,
            phase: Phase::Editing,
        }
    }

    pub(crate) fn focus(&mut self, control: u16) {
        // Common owner610CA0 (6118BF..6118DE) restores the previous HWND
        // when an owner-draw button receives focus. D7's tabstops are edits.
        if !matches!(control, SOCKET | DESTINATION | PORT) {
            return;
        }
        if self.focused_control != control {
            self.focus_started_at = crate::util::time::Instant::now();
        }
        self.focused_control = control;
        self.socket.focused = control == SOCKET;
        self.destination.focused = control == DESTINATION;
        self.port.focused = control == PORT;
    }

    /// NewEdit's focus handler clears its phase, then toggles every 1000 ms.
    pub(crate) fn caret_visible(&self, now: crate::util::time::Instant) -> bool {
        now.saturating_duration_since(self.focus_started_at)
            .as_secs()
            % 2
            == 0
    }

    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        // D7's WS_TABSTOP controls in original resource order; 00600C70
        // delegates Tab/Shift-Tab to GetNextDlgTabItem.
        const ORDER: [u16; 3] = [SOCKET, DESTINATION, PORT];
        let next = match ORDER.iter().position(|id| *id == self.focused_control) {
            Some(index) if reverse => (index + ORDER.len() - 1) % ORDER.len(),
            Some(index) => (index + 1) % ORDER.len(),
            None if reverse => ORDER.len() - 1,
            None => 0,
        };
        // 600C70 changes only edit focus; no native popup close is sent.
        self.focus(ORDER[next]);
    }

    pub(crate) fn focused_edit_mut(&mut self) -> Option<&mut SavedSeedDescriptionEdit> {
        match self.focused_control {
            SOCKET => Some(&mut self.socket),
            DESTINATION => Some(&mut self.destination),
            PORT => Some(&mut self.port),
            _ => None,
        }
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let limit = match self.focused_control {
            SOCKET => 5,
            DESTINATION => 11,
            _ => 0,
        };
        if self.phase == Phase::Editing {
            if let Some(edit) = self.focused_edit_mut() {
                edit.insert_text_limited(text, limit);
            }
        }
    }

    pub(crate) fn select_card(&mut self, index: usize) {
        if index < self.cards.len().max(1) {
            self.selected_card = index;
        }
        self.combo_open = false;
        self.card_scroll.cancel();
    }

    pub(crate) fn card_popup(&self, layout: &NetworkOptionsLayout) -> Option<ComboPopup> {
        if !self.combo_open {
            return None;
        }
        Some(ComboPopup::new(
            layout.card,
            layout.page.scale,
            self.cards.len().max(1),
            CARD_VISIBLE_ROWS,
            self.card_top_index,
        ))
    }

    fn error(&mut self, message: NetworkOptionsMessage) -> NetworkOptionsEffect {
        self.phase = Phase::Error(message);
        NetworkOptionsEffect::Message(message)
    }

    pub(crate) fn pending_message(&self) -> Option<NetworkOptionsMessage> {
        match self.phase {
            Phase::Error(message) => Some(message),
            Phase::SendDelayWarning(_) => Some(NetworkOptionsMessage::SendDelayWarning),
            Phase::PortWarning(_) => Some(NetworkOptionsMessage::PortWarning),
            _ => None,
        }
    }

    pub(crate) fn accept(&mut self) -> Vec<NetworkOptionsEffect> {
        if self.phase != Phase::Editing {
            return Vec::new();
        }
        self.combo_open = false;
        let socket_text = self.socket.narrow_text(16);
        let socket = if socket_text.is_empty() {
            u16::MAX
        } else {
            crt_atoi(&socket_text) as u16
        };
        // Native compares DX, after narrowing atoi; 65536 is accepted as 0.
        if !socket_text.is_empty() && (socket_text.chars().count() > 5 || socket > 0x3FFF) {
            return vec![self.error(NetworkOptionsMessage::InvalidSocket)];
        }
        let destination = self.destination.narrow_text(32);
        if !destination.is_empty() {
            let mut parts = strtok(&destination, &['.']);
            let valid = (0..4).all(|_| {
                parts
                    .next()
                    .and_then(|part| scan_hex_u32(part.as_bytes()))
                    .is_some_and(|value| value <= 255)
            });
            // Native's unbounded fifth token overwrites its result pointer.
            // Keep malformed addresses safe rather than reproducing corruption.
            if !valid || parts.next().is_some() {
                return vec![self.error(NetworkOptionsMessage::InvalidDestination)];
            }
        }
        // These globals are committed before validating the firewall port.
        let mut effects = vec![NetworkOptionsEffect::Lan {
            socket,
            destination,
            card: self.selected_card as i32,
        }];
        let port = crt_atoi(&self.port.narrow_text(8));
        // 00560A43 is signed JLE: negative overrides are accepted.
        if port > 0xFFFF {
            effects.push(self.error(NetworkOptionsMessage::InvalidPort));
            return effects;
        }
        if self.send_delay && !self.original_send_delay {
            self.phase = Phase::SendDelayWarning(port);
            effects.push(NetworkOptionsEffect::Message(
                NetworkOptionsMessage::SendDelayWarning,
            ));
        } else {
            effects.extend(self.commit_send_delay(port));
        }
        effects
    }

    fn commit_send_delay(&mut self, port: i32) -> Vec<NetworkOptionsEffect> {
        let mut effects = vec![NetworkOptionsEffect::SendDelay(self.send_delay)];
        if port > 0 && self.original_port == 0 {
            self.phase = Phase::PortWarning(port);
            effects.push(NetworkOptionsEffect::Message(
                NetworkOptionsMessage::PortWarning,
            ));
        } else {
            self.phase = Phase::Closed;
            effects.push(NetworkOptionsEffect::PortAndClose(port));
        }
        effects
    }

    pub(crate) fn acknowledge(&mut self) -> Vec<NetworkOptionsEffect> {
        match self.phase {
            Phase::Error(_) => {
                self.phase = Phase::Editing;
                Vec::new()
            }
            Phase::SendDelayWarning(port) => self.commit_send_delay(port),
            Phase::PortWarning(port) => {
                self.phase = Phase::Closed;
                vec![NetworkOptionsEffect::PortAndClose(port)]
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn cancel(&mut self) -> Vec<NetworkOptionsEffect> {
        if self.phase != Phase::Editing {
            return Vec::new();
        }
        self.phase = Phase::Closed;
        vec![NetworkOptionsEffect::Close]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> NetworkOptionsState {
        NetworkOptionsState::new(u16::MAX, "", 0, false, 0, &[])
    }

    fn set(edit: &mut SavedSeedDescriptionEdit, text: &str) {
        edit.set_units_limited(text.encode_utf16(), 0);
    }

    #[test]
    fn ordered_profile_effects_match_executed_native_d7() {
        use serde_json::json;
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/network_options.json",
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let initial = &case["initial"];
            let input = &case["inputs"];
            let mut state = NetworkOptionsState::new(
                initial["socket"].as_u64().unwrap() as u16,
                initial["destination"].as_str().unwrap(),
                initial["card"].as_i64().unwrap() as i32,
                initial["send_delay"].as_bool().unwrap(),
                initial["port"].as_i64().unwrap() as i32,
                &["card0".into(), "card1".into(), "card2".into()],
            );
            // The corpus injects control getter results, including over-limit
            // text, so bypass typing limits just as the native harness does.
            set(&mut state.socket, input["socket"].as_str().unwrap());
            set(
                &mut state.destination,
                input["destination"].as_str().unwrap(),
            );
            set(&mut state.port, input["port"].as_str().unwrap());
            state.send_delay = input["send_delay"].as_bool().unwrap();
            state.select_card(input["card"].as_u64().unwrap() as usize);
            let mut profile = initial.clone();
            let mut events = Vec::new();
            let mut result = 0;
            let mut effects = state.accept();
            while !effects.is_empty() {
                for effect in effects {
                    match effect {
                        NetworkOptionsEffect::Lan {
                            socket,
                            destination,
                            card,
                        } => {
                            profile["socket"] = json!(socket);
                            profile["destination"] = json!(destination);
                            profile["card"] = json!(card);
                        }
                        NetworkOptionsEffect::Message(message) => {
                            events.push(
                                json!({"kind":"message", "key":message.csf_key(), "state":profile}),
                            );
                        }
                        NetworkOptionsEffect::SendDelay(value) => {
                            profile["send_delay"] = json!(value)
                        }
                        NetworkOptionsEffect::PortAndClose(port) => {
                            profile["port"] = json!(port);
                            events.extend([
                                json!({"kind":"reset_firewall"}),
                                json!({"kind":"write_ini","section":"MultiPlayer","key":"SendDelay","value":profile["send_delay"]}),
                                json!({"kind":"write_ini","section":"MultiPlayer","key":"PortNumberOverride","value":port}),
                                json!({"kind":"save_file"}),
                            ]);
                            result = 2;
                        }
                        NetworkOptionsEffect::Close => result = 2,
                    }
                }
                effects = state.acknowledge();
            }
            assert_eq!(profile, case["state"], "state: {}", case["name"]);
            assert_eq!(json!(events), case["events"], "events: {}", case["name"]);
            assert_eq!(
                json!(result),
                case["dialog_result"],
                "result: {}",
                case["name"]
            );
        }
    }

    #[test]
    fn adapter_arrow_has_clickable_area_without_opening_from_the_text_face() {
        for layout in [
            NetworkOptionsLayout::new(800, 600),
            NetworkOptionsLayout::responsive(1600, 1200),
        ] {
            let arrow =
                crate::ui::skirmish_shell::combo_arrow_hit_rect(layout.card, layout.page.scale);
            let (x, y) = (arrow.x + arrow.w / 2, arrow.y + arrow.h / 2);
            assert!(arrow.contains(x, y));
            assert_eq!(layout.control_at(x, y), Some(CARD));
            assert!(!arrow.contains(layout.card.x + 5, y));
        }
    }

    #[test]
    fn tab_cycles_edits_without_dismissing_the_adapter_popup() {
        let mut state = fresh();
        state.combo_open = true;
        state.cycle_focus(false);
        assert_eq!(state.focused_control, SOCKET);
        assert!(state.combo_open);
        state.cycle_focus(true);
        assert_eq!(state.focused_control, PORT);
        assert!(state.combo_open);
    }

    #[test]
    fn ownerdraw_buttons_preserve_the_edit_focus_for_error_recovery() {
        let mut state = fresh();
        state.focus(PORT);
        state.port.set_units_limited("65536".encode_utf16(), 0);
        state.focus(OK);
        assert_eq!(state.focused_control, PORT);
        assert_eq!(
            state.accept(),
            vec![
                NetworkOptionsEffect::Lan {
                    socket: u16::MAX,
                    destination: String::new(),
                    card: 0,
                },
                NetworkOptionsEffect::Message(NetworkOptionsMessage::InvalidPort)
            ]
        );
        state.acknowledge();
        state.port.home();
        for _ in 0..5 {
            state.port.delete();
        }
        state.insert_text("1234");
        assert_eq!(state.port.display_text(), "1234");
        state.focus(CANCEL);
        assert_eq!(state.focused_control, PORT);
    }

    #[test]
    fn initialization_and_input_use_newedit_units_and_preserve_caret_on_focus() {
        let mut state = NetworkOptionsState::new(123, "AA.BB.CC.DD.extra", 9, true, -123, &[]);
        assert_eq!(state.socket.display_text(), "123");
        assert_eq!(state.destination.display_text(), "AA.BB.CC.DD");
        assert_eq!(state.port.display_text(), "-123");
        assert_eq!(state.selected_card, 0);
        assert_eq!(state.focused_control, PORT);
        state.focus(SOCKET);
        state.socket.home();
        state.insert_text("67");
        assert_eq!(state.socket.display_text(), "67123");
        state.focus(PORT);
        state.focus(SOCKET);
        assert_eq!(state.socket.caret, 2);
        state.insert_text("8");
        assert_eq!(state.socket.display_text(), "67123");
    }

    #[test]
    fn firewall_warnings_suspend_each_native_write_in_order() {
        let mut state = fresh();
        state.send_delay = true;
        set(&mut state.port, "1234");
        assert_eq!(
            state.accept(),
            vec![
                NetworkOptionsEffect::Lan {
                    socket: u16::MAX,
                    destination: String::new(),
                    card: 0
                },
                NetworkOptionsEffect::Message(NetworkOptionsMessage::SendDelayWarning),
            ]
        );
        assert!(state.accept().is_empty());
        assert!(state.cancel().is_empty());
        assert_eq!(
            state.acknowledge(),
            vec![
                NetworkOptionsEffect::SendDelay(true),
                NetworkOptionsEffect::Message(NetworkOptionsMessage::PortWarning),
            ]
        );
        assert_eq!(
            state.acknowledge(),
            vec![NetworkOptionsEffect::PortAndClose(1234)]
        );
        assert!(state.acknowledge().is_empty());
    }

    #[test]
    fn newedit_narrow_getter_uses_low_bytes_and_stops_at_nul() {
        let mut edit = SavedSeedDescriptionEdit::default();
        edit.set_units_limited([0x0131, 0x0132, 0x0100, 0x0033], 0);
        assert_eq!(edit.narrow_text(8), "12");
        assert_eq!(edit.narrow_text(2), "1");
        assert_eq!(edit.narrow_text(0), "");
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NetworkOptionsLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) socket: RectPx,
    pub(crate) destination: RectPx,
    pub(crate) port: RectPx,
    pub(crate) card: RectPx,
    pub(crate) send_delay: RectPx,
    pub(crate) labels: [(&'static str, RectPx); 4],
    pub(crate) groups: [(&'static str, RectPx); 2],
}

impl NetworkOptionsLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let offset = if scale.is_retail() {
            (center_offset(width, 800), center_offset(height, 600))
        } else {
            (0, 0)
        };
        let px = |rect: RectPx| scale.scale_rect(rect.translate(offset.0, offset.1));
        let dlu = |x, y, w, h| px(dlu_rect(x, y, w, h));
        // RT_DIALOG D7 original DLU rectangles. NewEdit's init (614C7A)
        // insets the grown child window by one pixel on every side.
        let edit = |x, y, w, h| {
            let rect = child_window(x, y, w, h);
            px(RectPx::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2))
        };
        let page = if scale.is_retail() {
            menu_page::compute_layout(&PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_responsive_layout(&PAGE, width as u32, height as u32)
        };
        Self {
            page,
            socket: edit(132, 100, 165, 12),
            destination: edit(132, 140, 165, 12),
            port: edit(132, 205, 165, 12),
            card: {
                let rect = child_window(132, 60, 165, 12);
                px(RectPx::new(rect.x, rect.y, rect.w, COMBO_FACE_H))
            },
            send_delay: dlu(132, 229, 211, 10),
            labels: [
                ("GUI:NetworkCard", dlu(132, 44, 165, 10)),
                ("GUI:SocketNumber", dlu(132, 84, 165, 10)),
                ("GUI:DestinationNetwork", dlu(132, 124, 165, 10)),
                ("GUI:PortNumber", dlu(132, 189, 165, 10)),
            ],
            groups: [
                ("GUI:LANSettings", dlu(63, 33, 300, 132)),
                ("GUI:FirewallSettings", dlu(63, 178, 300, 108)),
            ],
        }
    }

    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        for button in &self.page.buttons {
            if button.rect.contains(x, y) {
                return Some(button.id);
            }
        }
        [
            (SOCKET, self.socket),
            (DESTINATION, self.destination),
            (PORT, self.port),
            (CARD, self.card),
            (SEND_DELAY, self.send_delay),
        ]
        .into_iter()
        .find_map(|(id, rect)| rect.contains(x, y).then_some(id))
    }
}
