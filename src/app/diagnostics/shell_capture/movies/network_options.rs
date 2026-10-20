//! D5 -> D7 diagnostics inside the existing production shell capture driver.
//! The native corpus supplies prior values and user inputs. SaveReturn WRITES
//! settings through D7 and the final D5 exit: run it with an isolated RA2_DIR.
//! Other checkpoints stop before the final warning or cancel without D7 saves.
//! D5 applies its draft before entry but saves only on its final exit. Keep
//! controlled profile runs isolated from ordinary user settings.

use super::*;
use crate::app::input::network_options as input;
use crate::ui::main_menu_dialogs::options::{LauncherButton, LauncherParentResult, OptionsPage};
use crate::ui::network_options::{CANCEL, NetworkOptionsMessage, OK, PORT, SOCKET};
use crate::ui::shell::geom::RectPx;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkOptionsTarget {
    Steady,
    Cards,
    InvalidPort,
    Warning1,
    Warning2,
    Back,
    /// Accept both warnings, save D7, then leave D5 to persist LAN fields.
    /// This checkpoint writes RA2MD.INI; its game directory must be isolated.
    SaveReturn,
}

impl NetworkOptionsTarget {
    pub(super) fn is_message(self) -> bool {
        matches!(self, Self::InvalidPort | Self::Warning1 | Self::Warning2)
    }

    fn native_case(self) -> &'static str {
        match self {
            Self::InvalidPort => "port_invalid_after_lan_commit",
            Self::Warning1 | Self::Warning2 | Self::SaveReturn => "warnings_in_order",
            _ => "blank_defaults",
        }
    }

    fn message_key(self) -> Option<&'static str> {
        match self {
            Self::InvalidPort => Some("TXT_PORT_NUMBER_ERROR"),
            Self::Warning1 => Some("TXT_FIREWALL_SETTINGS_WARNING1"),
            Self::Warning2 => Some("TXT_FIREWALL_SETTINGS_WARNING2"),
            _ => None,
        }
    }
}

fn native_case(target: NetworkOptionsTarget) -> Result<Value> {
    let corpus: Value = super::native_corpus("tools/storage_oracle/network_options.json")?;
    corpus["cases"]
        .as_array()
        .context("native D7 cases")?
        .iter()
        .find(|case| case["name"] == target.native_case())
        .cloned()
        .context("native D7 input case missing")
}

fn profile_state(state: &AppState) -> Value {
    let profile = &state.persistence.options_profile;
    json!({"socket": profile.network_socket, "card": profile.network_card,
        "destination": profile.network_destination,
        "send_delay": profile.network_send_delay, "port": profile.network_port})
}

fn persisted_state(state: &AppState) -> Result<Value> {
    let path = state
        .platform
        .game_config
        .as_ref()
        .context("D7 capture game config")?
        .paths
        .settings_dir()
        .join("RA2MD.INI");
    let bytes =
        std::fs::read(&path).with_context(|| format!("read saved settings {}", path.display()))?;
    let ini = crate::rules::ini_parser::IniFile::from_bytes(&bytes)
        .context("parse physically saved RA2MD.INI")?;
    let network = ini.section("Network").context("saved Network section")?;
    let multiplayer = ini
        .section("MultiPlayer")
        .context("saved MultiPlayer section")?;
    for key in ["Socket", "NetCard"] {
        ensure!(network.is_present(key), "saved Network.{key} is missing");
    }
    for key in ["SendDelay", "PortNumberOverride"] {
        ensure!(
            multiplayer.is_present(key),
            "saved MultiPlayer.{key} is missing"
        );
    }
    // Native INI parsing omits empty values. DestNet="" therefore reads as
    // the ordinary empty default, whether the writer kept an empty key or not.
    Ok(json!({"path": path, "state": {
        "socket": network.read_int("Socket", -1) as u16,
        "card": network.read_int("NetCard", -1),
        "destination": network.read_string("DestNet", "", 0x40),
        "send_delay": multiplayer.read_bool("SendDelay", false),
        "port": multiplayer.read_int("PortNumberOverride", 0),
    }}))
}

fn verify_saved_settings(state: &AppState) -> Result<Value> {
    let case = native_case(NetworkOptionsTarget::SaveReturn)?;
    ensure!(
        profile_state(state) == case["state"],
        "accepted D7 profile differs from native final state"
    );
    let persisted = persisted_state(state)?;
    ensure!(
        persisted["state"] == case["state"],
        "physical RA2MD.INI differs from native final state"
    );
    Ok(persisted)
}

fn click(state: &mut AppState, rect: RectPx) {
    state.match_state.input.cursor_x = (rect.x + rect.w / 2) as f32;
    state.match_state.input.cursor_y = (rect.y + rect.h / 2) as f32;
    input::cursor_moved(state);
    input::mouse(state, MouseButton::Left, true);
    input::mouse(state, MouseButton::Left, false);
}

fn type_field(state: &mut AppState, rect: RectPx, value: &str) {
    click(state, rect);
    replace_focused_edit(state, value);
}

fn replace_focused_edit(state: &mut AppState, value: &str) {
    input::key_press(state, Some(KeyCode::Home), None);
    // More than any field in this bounded input corpus, through ordinary edit
    // commands rather than replacing the draft owned by the dialog.
    for _ in 0..32 {
        input::key_press(state, Some(KeyCode::Delete), None);
    }
    input::key_press(state, None, Some(value));
}

fn button_rect(state: &AppState, id: u16) -> Result<RectPx> {
    input::layout(state)
        .page
        .buttons
        .into_iter()
        .find(|button| button.id == id)
        .map(|button| button.rect)
        .context("D7 action missing from production layout")
}

impl MoviesCapture {
    pub(super) fn network_options_after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<bool> {
        let MoviesTarget::NetworkOptions(target) = self.target else {
            return Ok(false);
        };
        match self.phase {
            Phase::Options => {
                if rendered == PresentedShell::Options
                    && Self::slide_settled(state, ShellSlideKind::Options)
                {
                    let layout = crate::app::frontend::shell_layout::launcher_options_layout(
                        state,
                        OptionsPage::Main,
                    );
                    let network = LauncherButton::Exit(LauncherParentResult::Network);
                    let rect = layout
                        .buttons
                        .iter()
                        .find(|(action, _)| *action == network)
                        .map(|(_, rect)| *rect)
                        .context("D5 Network button")?;
                    state.match_state.input.cursor_x = (rect.x + rect.w / 2) as f32;
                    state.match_state.input.cursor_y = (rect.y + rect.h / 2) as f32;
                    App::handle_launcher_options_mouse(state, None);
                    App::handle_launcher_options_mouse(state, Some(true));
                    App::handle_launcher_options_mouse(state, Some(false));
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "D5 Network must run its teardown slide"
                    );
                    self.route.push(json!({"dialog": 0xd5, "frame": frame,
                        "action": "Network through ordinary pointer handler"}));
                    self.phase = Phase::NetworkOptions;
                }
            }
            Phase::NetworkOptions => {
                if rendered == PresentedShell::NetworkOptions
                    && Self::slide_settled(state, ShellSlideKind::NetworkOptions)
                {
                    let case = native_case(target)?;
                    let initial = &case["initial"];
                    let profile = &mut state.persistence.options_profile;
                    profile.network_socket = initial["socket"].as_u64().context("socket")? as u16;
                    profile.network_card = initial["card"].as_i64().context("card")? as i32;
                    profile.network_destination = initial["destination"]
                        .as_str()
                        .context("destination")?
                        .to_owned();
                    profile.network_send_delay =
                        initial["send_delay"].as_bool().context("send_delay")?;
                    profile.network_port = initial["port"].as_i64().context("port")? as i32;
                    let cards = if target == NetworkOptionsTarget::Steady
                        || target == NetworkOptionsTarget::Back
                    {
                        Vec::new()
                    } else {
                        (0..8)
                            .map(|index| format!("00.00.00.{index:02X}:00.00.00.00.00.01"))
                            .collect()
                    };
                    // The ordinary route has already created and presented D7.
                    // Supply service responses and known prior settings through
                    // the same opener; native IPX discovery is outside capture.
                    input::open(state, &cards);
                    Self::restore_neutral_pointer(state);
                    input::cursor_moved(state);
                    self.route.push(json!({"dialog": 0xd7, "frame": frame,
                        "action": "controlled service and prior-settings inputs",
                        "native_case": target.native_case(), "initial": initial,
                        "cards": cards, "profile_directory": state.platform.game_config.as_ref().map(|config| config.paths.settings_dir())}));
                    self.phase = Phase::NetworkOptionsInput;
                }
            }
            Phase::NetworkOptionsInput => {
                if rendered != PresentedShell::NetworkOptions
                    || !Self::slide_settled(state, ShellSlideKind::NetworkOptions)
                    || !state.frontend.shell_page_title.is_terminal()
                {
                    return Ok(true);
                }
                if target == NetworkOptionsTarget::Back {
                    let cancel = button_rect(state, CANCEL)?;
                    click(state, cancel);
                    self.route
                        .push(json!({"dialog": 0xd7, "frame": frame, "action": "Cancel"}));
                    self.phase = Phase::NetworkOptionsReturning;
                } else if target == NetworkOptionsTarget::Steady {
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                } else {
                    let layout = input::layout(state);
                    let arrow = crate::ui::skirmish_shell::combo_arrow_hit_rect(
                        layout.card,
                        layout.page.scale,
                    );
                    click(state, arrow);
                    if target == NetworkOptionsTarget::Cards {
                        let popup = state
                            .frontend
                            .network_options
                            .as_ref()
                            .and_then(|dialog| dialog.card_popup(&layout))
                            .context("adapter popup")?;
                        let hot = popup.row_rect(2).context("third adapter row")?;
                        state.match_state.input.cursor_x = (hot.x + hot.w / 2) as f32;
                        state.match_state.input.cursor_y = (hot.y + hot.h / 2) as f32;
                        input::cursor_moved(state);
                        let bar = popup.scrollbar().context("adapter scrollbar")?;
                        state.match_state.input.cursor_x = (bar.x + bar.w / 2) as f32;
                        state.match_state.input.cursor_y = (bar.y + bar.h / 2) as f32;
                        input::cursor_moved(state);
                        input::key_press(state, Some(KeyCode::Tab), None);
                        let dialog = state.frontend.network_options.as_ref().context("D7")?;
                        ensure!(
                            dialog.combo_open && dialog.focused_control == SOCKET,
                            "Tab must advance edit focus while keeping the adapter popup"
                        );
                        ensure!(
                            dialog.card_hot_row == Some(2),
                            "scrollbar hover lost adapter hot row"
                        );
                        Self::restore_neutral_pointer(state);
                        input::cursor_moved(state);
                        ensure!(
                            state
                                .frontend
                                .network_options
                                .as_ref()
                                .unwrap()
                                .card_hot_row
                                == Some(2),
                            "outside hover lost adapter hot row"
                        );
                        self.route.push(json!({"dialog": 0xd7, "frame": frame,
                            "action": "open popup, hover third adapter, leave via scrollbar, Tab and neutral",
                            "hot_row": 2, "focused_control": SOCKET}));
                        self.phase = Phase::Settling(SETTLE_FRAMES);
                    } else {
                        let case = native_case(target)?;
                        let inputs = &case["inputs"];
                        let row = inputs["card"].as_u64().context("card input")? as usize;
                        let rect = state
                            .frontend
                            .network_options
                            .as_ref()
                            .and_then(|dialog| dialog.card_popup(&input::layout(state)))
                            .and_then(|popup| popup.row_rect(row))
                            .context("fixture card row")?;
                        click(state, rect);
                        let layout = input::layout(state);
                        type_field(
                            state,
                            layout.socket,
                            inputs["socket"].as_str().context("socket input")?,
                        );
                        type_field(
                            state,
                            layout.destination,
                            inputs["destination"]
                                .as_str()
                                .context("destination input")?,
                        );
                        type_field(
                            state,
                            layout.port,
                            inputs["port"].as_str().context("port input")?,
                        );
                        if state
                            .frontend
                            .network_options
                            .as_ref()
                            .context("D7")?
                            .send_delay
                            != inputs["send_delay"].as_bool().context("delay input")?
                        {
                            let before =
                                state.frontend.network_options.as_ref().unwrap().send_delay;
                            // A label click is inert; only the native icon toggles.
                            click(state, layout.send_delay);
                            ensure!(
                                state.frontend.network_options.as_ref().unwrap().send_delay
                                    == before,
                                "Send Delay label changed the checkbox"
                            );
                            let icon = crate::ui::skirmish_shell::checkbox_icon_rect(
                                layout.send_delay,
                                layout.page.scale,
                            );
                            state.match_state.input.cursor_x = (icon.x + icon.w / 2) as f32;
                            state.match_state.input.cursor_y = (icon.y + icon.h / 2) as f32;
                            input::cursor_moved(state);
                            input::mouse(state, MouseButton::Left, true);
                            ensure!(
                                state.frontend.network_options.as_ref().unwrap().send_delay
                                    != before,
                                "Send Delay must toggle on button-down"
                            );
                            input::mouse(state, MouseButton::Left, false);
                        }
                        let ok = button_rect(state, OK)?;
                        if target == NetworkOptionsTarget::SaveReturn {
                            // Exercise error recovery without clicking the edit again:
                            // native owner-draw OK must preserve PORT focus.
                            type_field(state, layout.port, "65536");
                            click(state, ok);
                            ensure!(
                                state
                                    .frontend
                                    .network_options
                                    .as_ref()
                                    .unwrap()
                                    .pending_message()
                                    == Some(NetworkOptionsMessage::InvalidPort),
                                "invalid port recovery did not open CE"
                            );
                            input::key_press(state, Some(KeyCode::Enter), None);
                            ensure!(
                                state
                                    .frontend
                                    .network_options
                                    .as_ref()
                                    .unwrap()
                                    .focused_control
                                    == PORT,
                                "D7 OK stole port edit focus after validation"
                            );
                            replace_focused_edit(
                                state,
                                inputs["port"].as_str().context("port input")?,
                            );
                            input::key_press(state, Some(KeyCode::Enter), None);
                            let dialog = state.frontend.network_options.as_ref().unwrap();
                            ensure!(
                                dialog.pending_message().is_none()
                                    && dialog.port.display_text()
                                        == inputs["port"].as_str().unwrap(),
                                "post-error typing or inert NewEdit Enter differs from native"
                            );
                            self.route.push(json!({"dialog": 0xd7, "frame": frame,
                                "action": "dismiss invalid port; correct focused edit without pointer; Enter remains inert"}));
                        }
                        click(state, ok);
                        Self::restore_neutral_pointer(state);
                        input::cursor_moved(state);
                        self.route.push(json!({"dialog": 0xd7, "frame": frame,
                            "action": "type native-case draft and click OK", "inputs": inputs}));
                        self.phase = Phase::NetworkOptionsMessage;
                    }
                }
            }
            Phase::NetworkOptionsMessage | Phase::NetworkOptionsPortWarning => {
                if rendered != PresentedShell::NetworkOptions {
                    return Ok(true);
                }
                let accepts_first = matches!(
                    target,
                    NetworkOptionsTarget::Warning2 | NetworkOptionsTarget::SaveReturn
                );
                let expected = if accepts_first && self.phase == Phase::NetworkOptionsMessage {
                    "TXT_FIREWALL_SETTINGS_WARNING1"
                } else if target == NetworkOptionsTarget::SaveReturn {
                    "TXT_FIREWALL_SETTINGS_WARNING2"
                } else {
                    target.message_key().context("D7 message target")?
                };
                let actual = state
                    .frontend
                    .network_options
                    .as_ref()
                    .and_then(|dialog| dialog.pending_message())
                    .map(|message| message.csf_key());
                ensure!(
                    actual == Some(expected),
                    "D7 message mismatch: {actual:?}, expected {expected}"
                );
                let case = native_case(target)?;
                let native_event = case["events"]
                    .as_array()
                    .context("native D7 events")?
                    .iter()
                    .find(|event| event["key"] == expected)
                    .context("native D7 message event")?;
                ensure!(
                    profile_state(state) == native_event["state"],
                    "D7 profile at {expected} differs from native message state"
                );
                self.route.push(json!({"dialog": 0xd7, "frame": frame,
                    "action": "observe modal", "key": actual, "profile": profile_state(state)}));
                if accepts_first && self.phase == Phase::NetworkOptionsMessage {
                    input::key_press(state, Some(KeyCode::Enter), None);
                    self.phase = Phase::NetworkOptionsPortWarning;
                } else if target == NetworkOptionsTarget::SaveReturn {
                    input::key_press(state, Some(KeyCode::Enter), None);
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "D7 acceptance must run its teardown slide"
                    );
                    self.route.push(json!({"dialog": 0xd7, "frame": frame,
                        "action": "acknowledge final warning and save MultiPlayer settings"}));
                    self.phase = Phase::NetworkOptionsReturning;
                } else {
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            Phase::NetworkOptionsMainMenu => {
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))?
                {
                    Self::restore_neutral_pointer(state);
                    let persisted = verify_saved_settings(state)?;
                    self.route.push(json!({"dialog": 0xe2, "frame": frame,
                        "action": "accepted settings saved; Main Menu settled", "persisted": persisted}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            Phase::NetworkOptionsReturning => {
                if rendered == PresentedShell::Options
                    && state.frontend.network_options.is_none()
                    && Self::slide_settled(state, ShellSlideKind::Options)
                {
                    Self::restore_neutral_pointer(state);
                    App::handle_launcher_options_mouse(state, None);
                    self.route.push(
                        json!({"dialog": 0xd5, "frame": frame, "action": "returned to Options"}),
                    );
                    if target == NetworkOptionsTarget::SaveReturn {
                        let case = native_case(target)?;
                        ensure!(
                            profile_state(state) == case["state"],
                            "D7 return lost accepted profile values"
                        );
                        let layout = crate::app::frontend::shell_layout::launcher_options_layout(
                            state,
                            OptionsPage::Main,
                        );
                        let main_menu = LauncherButton::Exit(LauncherParentResult::Back);
                        let rect = layout
                            .buttons
                            .iter()
                            .find(|(action, _)| *action == main_menu)
                            .map(|(_, rect)| *rect)
                            .context("D5 Main Menu button")?;
                        state.match_state.input.cursor_x = (rect.x + rect.w / 2) as f32;
                        state.match_state.input.cursor_y = (rect.y + rect.h / 2) as f32;
                        App::handle_launcher_options_mouse(state, None);
                        App::handle_launcher_options_mouse(state, Some(true));
                        App::handle_launcher_options_mouse(state, Some(false));
                        // E2 readiness checks its sealed neutral pointer before
                        // accepting the first returned frame.
                        Self::restore_neutral_pointer(state);
                        ensure!(
                            state.frontend.shell_exit.is_some(),
                            "D5 Main Menu must run its teardown slide"
                        );
                        self.route.push(json!({"dialog": 0xd5, "frame": frame,
                            "action": "Main Menu through ordinary pointer handler; persist LAN fields"}));
                        self.phase = Phase::NetworkOptionsMainMenu;
                    } else {
                        self.phase = Phase::Settling(SETTLE_FRAMES);
                    }
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub(super) fn network_options_ready(
        &self,
        state: &AppState,
        target: NetworkOptionsTarget,
    ) -> Result<bool> {
        if target == NetworkOptionsTarget::SaveReturn {
            ensure!(
                state.frontend.network_options.is_none() && state.frontend.options_dialog.is_none(),
                "saved settings capture retained a settings dialog"
            );
            verify_saved_settings(state)?;
            return steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state));
        }
        if target == NetworkOptionsTarget::Back {
            return Ok(state.frontend.network_options.is_none()
                && state.frontend.options_dialog.is_some()
                && Self::slide_settled(state, ShellSlideKind::Options));
        }
        let dialog = state
            .frontend
            .network_options
            .as_ref()
            .context("D7 disappeared before capture")?;
        ensure!(
            dialog.pending_message().map(|message| message.csf_key()) == target.message_key(),
            "D7 modal changed before capture"
        );
        if target == NetworkOptionsTarget::Cards {
            ensure!(
                dialog.cards.len() == 8 && dialog.combo_open,
                "eight-card popup closed before capture"
            );
        }
        if target.is_message() {
            let case = native_case(target)?;
            let expected = case["events"]
                .as_array()
                .context("native events")?
                .iter()
                .find(|event| event["key"] == target.message_key().unwrap())
                .context("native message event")?;
            ensure!(
                profile_state(state) == expected["state"],
                "D7 production profile at modal differs from executed native case"
            );
        }
        Ok(Self::slide_settled(state, ShellSlideKind::NetworkOptions))
    }

    pub(super) fn network_options_receipt(
        &self,
        state: &AppState,
        target: NetworkOptionsTarget,
    ) -> Result<Value> {
        let case = native_case(target)?;
        let mut receipt = json!({"native_case": case["name"], "supplied_initial": case["initial"],
            "profile": profile_state(state),
            "message": state.frontend.network_options.as_ref().and_then(|dialog| dialog.pending_message()).map(|message| message.csf_key()),
            "cards": state.frontend.network_options.as_ref().map(|dialog| &dialog.cards),
            "popup_open": state.frontend.network_options.as_ref().map(|dialog| dialog.combo_open),
            "hot_row": state.frontend.network_options.as_ref().and_then(|dialog| dialog.card_hot_row),
            "returned_to_options": matches!(target, NetworkOptionsTarget::Back | NetworkOptionsTarget::SaveReturn),
            "writes_settings": target == NetworkOptionsTarget::SaveReturn,
            "scope": "production pointer/key handlers and final renderer; explicit prior settings and card responses"});
        if target == NetworkOptionsTarget::SaveReturn {
            receipt["persisted"] = verify_saved_settings(state)?;
            receipt["returned_to_main_menu"] = json!(true);
        }
        Ok(receipt)
    }
}
