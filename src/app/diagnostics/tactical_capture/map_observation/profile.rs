//! The map observation profile: its JSON schema (audio, presentation, scheduled
//! gestures and sidebar targets), literal key parsing and profile validation.

use super::*;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MapAudioProfile {
    sound_ids: Vec<String>,
    max_events: usize,
    max_samples_per_event: usize,
    pub(super) completion_tail_ms: u64,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) coordinate_request_scope: Option<MapCoordinateRequestScope>,
}

/// The battle's Options presentation choices for one capture, as their
/// `[VERA]` values. Absent, a capture keeps zoom 100%, the Normal sidebar,
/// the native radar and the sidebar's own pixels.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct MapPresentationProfile {
    /// `Zoom=`: 0 for Auto, otherwise a percent.
    pub(super) zoom: i32,
    /// `Sidebar=`: 100, 150 or 200.
    pub(super) sidebar: i32,
    /// `HdRadar=`: optional, off when absent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) hd_radar: bool,
    /// `SmoothSidebar=`: optional, off when absent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) smooth_sidebar: bool,
}

impl MapPresentationProfile {
    pub(super) fn validate(&self) -> Result<()> {
        use crate::ui::main_menu_dialogs::options::{SidebarSize, Zoom};
        ensure!(
            Zoom::from_ini_value(self.zoom).ini_value() == self.zoom,
            "presentation.zoom must be 0 (Auto) or a 50-step percent from 100"
        );
        ensure!(
            SidebarSize::from_ini_value(self.sidebar).ini_value() == self.sidebar,
            "presentation.sidebar must be 100, 150 or 200"
        );
        Ok(())
    }

    pub(super) fn apply(&self, state: &mut AppState) {
        use crate::ui::main_menu_dialogs::options::{SidebarSize, Zoom};
        state.platform.capture_presentation = crate::app::state::CapturePresentation {
            zoom: Zoom::from_ini_value(self.zoom),
            sidebar: SidebarSize::from_ini_value(self.sidebar),
            hd_radar: self.hd_radar,
            smooth_sidebar: self.smooth_sidebar,
            map_picture: false,
        };
        crate::app::App::sync_zoom_pass(state);
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum MapCoordinateRequestScope {
    All,
    Watched,
}

impl MapAudioProfile {
    /// Filter only the passive coordinate-node projection. Playback, pool
    /// admission and the final whole-pool census remain untouched.
    pub(super) fn retain_coordinate_requests(&self, rows: &mut Vec<Value>) {
        if self.coordinate_request_scope == Some(MapCoordinateRequestScope::Watched) {
            rows.retain(|row| {
                row["sound_id"].as_str().is_some_and(|name| {
                    self.sound_ids
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(name))
                })
            });
        }
    }

    pub(super) fn config(&self) -> PcmObservationConfig {
        PcmObservationConfig {
            sound_ids: self.sound_ids.clone(),
            max_events: self.max_events,
            max_samples_per_event: self.max_samples_per_event,
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        self.config().validate().map_err(anyhow::Error::msg)?;
        ensure!(
            (1..=10_000).contains(&self.completion_tail_ms),
            "audio completion tail must be 1..10000 ms"
        );
        let mut ids = BTreeSet::new();
        ensure!(
            self.sound_ids
                .iter()
                .all(|id| ids.insert(id.to_ascii_uppercase())),
            "duplicate observed sound ID"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MapScheduledGesture {
    pub(super) issue_after_step: u32,
    pub(super) gesture: MapGesture,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum MapMouseButton {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum MapKeyEdge {
    Pressed,
    Repeat,
    Released,
}

/// A complete local input gesture between exact steps. Mouse coordinates are
/// render-target pixels; key names are literal keys, never command identities.
/// No render sees a synthetic held button or key. Omitted mouse buttons retain
/// the historical left route; right clicks use the same ordinary edge owner.
/// Click modifiers are held through the cursor move, press and release only.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum MapGesture {
    Click {
        position: [u32; 2],
        #[serde(
            default,
            deserialize_with = "deserialize_present",
            skip_serializing_if = "Option::is_none"
        )]
        button: Option<MapMouseButton>,
        #[serde(
            default,
            deserialize_with = "deserialize_present",
            skip_serializing_if = "Option::is_none"
        )]
        modifiers: Option<Vec<String>>,
    },
    Drag {
        from: [u32; 2],
        to: [u32; 2],
    },
    /// Ordinary left press/release on the live generated radar surface.
    /// Online admission and screen-to-generated-pixel remain production input.
    Minimap {
        position: [u32; 2],
    },
    /// Resolve a current retained tracker pixel, then send ordinary mouse edges.
    MinimapObject {
        entity_id: u64,
    },
    /// Resolve the named control from the current immutable sidebar view.
    /// This still sends ordinary mouse edges, never a SidebarAction.
    Sidebar {
        target: MapSidebarTarget,
        #[serde(
            default,
            deserialize_with = "deserialize_present",
            skip_serializing_if = "Option::is_none"
        )]
        button: Option<MapMouseButton>,
    },
    /// Resolve a loaded command-bar control (or its diagnostic-only Thumb
    /// target) and send ordinary mouse edges.
    CommandBar {
        command: String,
    },
    Key {
        key: String,
        /// Separate edges are accepted only for the production planning key.
        #[serde(
            default,
            deserialize_with = "deserialize_present",
            skip_serializing_if = "Option::is_none"
        )]
        edge: Option<MapKeyEdge>,
        #[serde(
            default,
            deserialize_with = "deserialize_present",
            skip_serializing_if = "Option::is_none"
        )]
        modifiers: Option<Vec<String>>,
    },
}

impl MapGesture {
    pub(super) fn is_minimap(&self) -> bool {
        matches!(self, Self::Minimap { .. } | Self::MinimapObject { .. })
    }

    pub(super) fn is_right_button(&self) -> bool {
        matches!(
            self,
            Self::Click {
                button: Some(MapMouseButton::Right),
                ..
            } | Self::Sidebar {
                button: Some(MapMouseButton::Right),
                ..
            }
        )
    }

    pub(super) fn modifiers(&self) -> &[String] {
        match self {
            Self::Click { modifiers, .. } | Self::Key { modifiers, .. } => {
                modifiers.as_deref().unwrap_or_default()
            }
            _ => &[],
        }
    }

    pub(super) fn is_quickload(&self) -> bool {
        matches!(self, Self::Key { key, modifiers: Some(modifiers), .. }
            if key.eq_ignore_ascii_case("N") && modifiers.len() == 2
                && modifiers.iter().any(|m| m == "Ctrl") && modifiers.iter().any(|m| m == "Shift"))
    }
    pub(super) fn points(&self) -> Option<([u32; 2], [u32; 2])> {
        match self {
            Self::Click { position, .. } | Self::Minimap { position } => {
                Some((*position, *position))
            }
            Self::Drag { from, to } => Some((*from, *to)),
            Self::Sidebar { .. }
            | Self::CommandBar { .. }
            | Self::Key { .. }
            | Self::MinimapObject { .. } => None,
        }
    }
}

/// Press or release literal modifiers through the shared modifier-key owner,
/// releasing in reverse order.
pub(super) fn literal_modifier_edges(
    state: &mut AppState,
    codes: &[winit::keyboard::KeyCode],
    pressed: bool,
) {
    let mut edge = |code| {
        crate::app::input::hotkeys::record_modifier_key(
            &mut state.platform.held_modifier_keys,
            &mut state.platform.live_modifiers,
            &mut state.match_state.input.hotkey_modifiers,
            code,
            pressed,
            false,
        );
    };
    if pressed {
        codes.iter().copied().for_each(&mut edge);
    } else {
        codes.iter().rev().copied().for_each(&mut edge);
    }
}

pub(super) fn literal_modifiers(names: &[String]) -> Result<Vec<winit::keyboard::KeyCode>> {
    use winit::keyboard::KeyCode;
    ensure!(names.len() <= 4, "too many literal modifiers");
    let mut seen = BTreeSet::new();
    names
        .iter()
        .map(|name| {
            ensure!(seen.insert(name), "duplicate literal modifier");
            Ok(match name.as_str() {
                "Ctrl" => KeyCode::ControlLeft,
                "Shift" => KeyCode::ShiftLeft,
                "Alt" => KeyCode::AltLeft,
                "Super" => KeyCode::SuperLeft,
                _ => bail!("unknown literal modifier"),
            })
        })
        .collect()
}

/// Diagnostic spelling to winit identity only. The production keyboard owner
/// still resolves the loaded binding and admits both press and release edges.
pub(super) fn literal_key(key: &str) -> Result<(winit::keyboard::KeyCode, winit::keyboard::Key)> {
    use winit::keyboard::{Key, KeyCode, NamedKey};
    if key == "Escape" {
        return Ok((KeyCode::Escape, Key::Named(NamedKey::Escape)));
    }
    let [byte] = key.as_bytes() else {
        bail!("key must be one ASCII letter/digit or Escape")
    };
    let code = match byte.to_ascii_uppercase() {
        b'A'..=b'Z' => [
            KeyCode::KeyA,
            KeyCode::KeyB,
            KeyCode::KeyC,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyF,
            KeyCode::KeyG,
            KeyCode::KeyH,
            KeyCode::KeyI,
            KeyCode::KeyJ,
            KeyCode::KeyK,
            KeyCode::KeyL,
            KeyCode::KeyM,
            KeyCode::KeyN,
            KeyCode::KeyO,
            KeyCode::KeyP,
            KeyCode::KeyQ,
            KeyCode::KeyR,
            KeyCode::KeyS,
            KeyCode::KeyT,
            KeyCode::KeyU,
            KeyCode::KeyV,
            KeyCode::KeyW,
            KeyCode::KeyX,
            KeyCode::KeyY,
            KeyCode::KeyZ,
        ][usize::from(byte.to_ascii_uppercase() - b'A')],
        b'0'..=b'9' => [
            KeyCode::Digit0,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ][usize::from(*byte - b'0')],
        _ => bail!("key must be one ASCII letter/digit or Escape"),
    };
    Ok((code, Key::Character(key.into())))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum MapSidebarTarget {
    Tab {
        tab: String,
    },
    /// The INI type name, or the SuperWeapon section rather than its image.
    Cameo {
        type_id: String,
    },
    ScrollUp {},
    ScrollDown {},
    Repair {},
    Sell {},
}

pub(super) fn sidebar_tab_name(tab: crate::ui::sidebar::SidebarTab) -> &'static str {
    use crate::ui::sidebar::SidebarTab;
    match tab {
        SidebarTab::Building => "building",
        SidebarTab::Defense => "defense",
        SidebarTab::Infantry => "infantry",
        SidebarTab::Vehicle => "vehicle",
    }
}

pub(super) fn deserialize_present<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapCaptureProfile {
    pub(crate) schema_version: String,
    pub(crate) launch: SkirmishLaunchSession,
    pub(crate) seed: u32,
    pub(crate) input_delay_ticks: u32,
    pub(crate) ticks: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) timeout_seconds: u32,
    // Option retains field presence in the sealed profile receipt: old v1
    // profiles serialize byte-equivalent JSON values without new defaults.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) commands: Option<Vec<MapScheduledCommand>>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) gestures: Option<Vec<MapScheduledGesture>>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_owners: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_types: Option<Vec<String>>,
    // Immutable global projectile/animation owner snapshots. Their absence
    // preserves historical profile and receipt bytes.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_projectiles: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_waves: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_anim_types: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_audio: Option<MapAudioProfile>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) allow_load_segments: Option<bool>,
    // Opt-in immutable inputs for native Attack-coordinate comparisons.
    // Historical profiles and their exact observation rows stay unchanged.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_action_line_inputs: Option<bool>,
    /// Passive post-render tooltip owner state on the presentation clock.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_tooltips: Option<bool>,
    /// Read retained disguise state and the actual body-builder decisions.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_disguise_inputs: Option<bool>,
    /// Read live lasers, process detail state and each building's Prism latch.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_lasers: Option<bool>,
    /// Read copied bolts, light/particle owners and RNG at Logic and draw boundaries.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_electric_bolts: Option<bool>,
    /// Copy the live radiation registry and actual last-composite line work.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_radiation_beams: Option<bool>,
    /// Optional committed-step whitelist for the same effect snapshots and
    /// composite observations. Absence retains every historical boundary.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_electric_bolt_steps: Option<Vec<u32>>,
    /// Read the existing bomb owner and observed actors' retained FireAt state.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_bombs: Option<bool>,
    /// Read the existing Temporal and passenger owners with retained FireAt state.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_temporal: Option<bool>,
    /// Add immutable cloak timers and playfield membership to the existing projection.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_cloak: Option<bool>,
    /// Read the existing spawn manager's ordered pool and count queries.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_spawns: Option<bool>,
    /// Read occupied slots from the existing MapClass crate authority.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_crates: Option<bool>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) camera_cell: Option<[u16; 2]>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) cursor_position: Option<[u32; 2]>,
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) terrain_cells: Option<Vec<[u16; 2]>>,
    // Opt-in Super rows on each observed House: the interned type id a
    // `LaunchSuperWeapon` command names, grant, readiness, hold and the
    // charge timer. Historical profiles and their rows stay unchanged.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_super_weapons: Option<bool>,
    // Opt-in diplomacy on each observed House: its allies and its enemy.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_diplomacy: Option<bool>,
    /// Exact committed steps whose retained projection is observed. A draw
    /// marks its matching row rendered only after comparing the actual output.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) observe_sidebar_steps: Option<Vec<u32>>,
    /// Zoom and Sidebar size for the battle frame. The frame
    /// readback stays the window size; incompatible with gestures and the
    /// cursor, whose coordinates are frame pixels at zoom 100%.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) presentation: Option<MapPresentationProfile>,
}

impl MapCaptureProfile {
    pub(crate) fn load(path: &Path) -> Result<SealedJsonFile<Self>> {
        let (bytes, digest) = read_stable_regular_bytes(path, "map observation profile")?;
        let value: Self = parse_strict_json(&bytes, "map observation profile")?;
        value.validate()?;
        Ok(SealedJsonFile {
            path: path.to_path_buf(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
            bytes,
            value,
        })
    }

    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            matches!(
                self.schema_version.as_str(),
                PROFILE_V1 | PROFILE_V2 | PROFILE_V3
            ),
            "unsupported map observation schema"
        );
        if self.schema_version == PROFILE_V1 {
            ensure!(
                self.commands.is_none()
                    && self.gestures.is_none()
                    && self.observe_owners.is_none()
                    && self.observe_types.is_none()
                    && self.observe_projectiles.is_none()
                    && self.observe_waves.is_none()
                    && self.observe_anim_types.is_none()
                    && self.observe_audio.is_none()
                    && self.allow_load_segments.is_none()
                    && self.observe_action_line_inputs.is_none()
                    && self.observe_tooltips.is_none()
                    && self.observe_disguise_inputs.is_none()
                    && self.observe_lasers.is_none()
                    && self.observe_electric_bolts.is_none()
                    && self.observe_radiation_beams.is_none()
                    && self.observe_electric_bolt_steps.is_none()
                    && self.observe_bombs.is_none()
                    && self.observe_temporal.is_none()
                    && self.observe_cloak.is_none()
                    && self.observe_spawns.is_none()
                    && self.observe_crates.is_none()
                    && self.camera_cell.is_none()
                    && self.cursor_position.is_none()
                    && self.terrain_cells.is_none()
                    && self.observe_super_weapons.is_none()
                    && self.observe_diplomacy.is_none()
                    && self.observe_sidebar_steps.is_none()
                    && self.presentation.is_none(),
                "map observation profile v1 cannot declare v2 extension fields"
            );
        }
        if let Some(presentation) = &self.presentation {
            presentation.validate()?;
            ensure!(
                self.gestures.is_none() && self.cursor_position.is_none(),
                "presentation cannot combine with gestures or cursor_position"
            );
        }
        ensure!(
            (640..=4096).contains(&self.width) && (480..=4096).contains(&self.height),
            "capture extent must be 640..4096 by 480..4096"
        );
        if let Some([x, y]) = self.cursor_position {
            ensure!(
                x > 0 && x < self.width - 1 && y > 0 && y < self.height - 1,
                "cursor_position must be inside the outermost capture pixels"
            );
        }
        ensure!(self.ticks <= 100_000, "capture tick budget exceeds 100000");
        if let Some(audio) = &self.observe_audio {
            audio.validate()?;
        }
        ensure!(
            (1..=900).contains(&self.timeout_seconds),
            "timeout must be 1..900 seconds"
        );
        ensure!(
            self.commands().len() <= MAX_COMMANDS,
            "too many scheduled commands"
        );
        let mut previous = 0;
        let mut symbolic_count = 0;
        for command in self.commands() {
            let count = command.symbolic_count();
            ensure!(
                count == 0 || self.schema_version == PROFILE_V3,
                "symbolic commands require map observation profile v3"
            );
            symbolic_count += count;
            ensure!(
                symbolic_count <= MAX_COMMANDS,
                "profile exceeds 1024 symbolic command bindings"
            );
            ensure!(
                command.issue_after_step >= previous && command.issue_after_step < self.ticks,
                "commands must be ordered by issue_after_step before the final step"
            );
            ensure!(!command.owner.is_empty(), "command owner is empty");
            ensure!(
                matches!(
                    command.payload,
                    Command::Select { .. }
                        | Command::Move { .. }
                        | Command::Stop { .. }
                        | Command::Attack { .. }
                        | Command::Guard { .. }
                        | Command::DeployMcv { .. }
                        | Command::SetRally { .. }
                        | Command::ForceAttackCell { .. }
                        | Command::QueueProduction { .. }
                        | Command::PlaceReadyBuilding { .. }
                        | Command::CaptureBuilding { .. }
                        | Command::PlantC4 { .. }
                        | Command::ToggleRepair { .. }
                        | Command::MegaMission {
                            mission: crate::sim::mission::MissionType::Enter,
                            ..
                        }
                        | Command::UnloadPassengers { .. }
                        | Command::SellBuilding { .. }
                        | Command::LaunchSuperWeapon { .. }
                        | Command::TogglePrimaryFactory { .. }
                ),
                "command is outside the map observation's ordinary order coverage"
            );
            previous = command.issue_after_step;
        }
        ensure!(
            self.gestures().len() <= MAX_GESTURES,
            "too many scheduled gestures"
        );
        if self.gestures.is_some() {
            ensure!(
                self.cursor_position.is_some(),
                "gestures require a sealed cursor_position"
            );
        }
        let (tactical_width, tactical_height) =
            crate::ui::sidebar::SidebarFrame::native(self.width, self.height).tactical_size();
        let mut previous = 0;
        for gesture in self.gestures() {
            ensure!(
                gesture.issue_after_step >= previous && gesture.issue_after_step < self.ticks,
                "gestures must be ordered by issue_after_step before the final step"
            );
            if let Some((from, to)) = gesture.gesture.points() {
                // A drag may end over the sidebar to observe the held cursor.
                let drag = matches!(gesture.gesture, MapGesture::Drag { .. });
                for (index, [x, y]) in [from, to].into_iter().enumerate() {
                    if gesture.gesture.is_minimap() || (drag && index == 1) {
                        ensure!(
                            x > 0 && x < self.width - 1 && y > 0 && y < self.height - 1,
                            "minimap point or drag end must be inside the render target's outermost pixels"
                        );
                    } else {
                        ensure!(
                            x > 0 && x < tactical_width - 1 && y > 0 && y < tactical_height - 1,
                            "gesture points must be inside the tactical viewport's outermost pixels"
                        );
                    }
                }
                ensure!(
                    !matches!(gesture.gesture, MapGesture::Drag { .. }) || from != to,
                    "drag endpoints must differ"
                );
            }
            if let MapGesture::MinimapObject { entity_id } = gesture.gesture {
                ensure!(
                    entity_id != 0,
                    "minimap object requires a live nonzero identity"
                );
            }
            if let MapGesture::Sidebar { target, .. } = &gesture.gesture {
                ensure!(
                    !gesture.gesture.is_right_button()
                        || matches!(target, MapSidebarTarget::Cameo { .. }),
                    "right sidebar gestures require a cameo target"
                );
                match target {
                    MapSidebarTarget::Tab { tab } => ensure!(
                        crate::ui::sidebar::SidebarTab::all()
                            .into_iter()
                            .any(|candidate| sidebar_tab_name(candidate) == tab.as_str()),
                        "unknown sidebar tab"
                    ),
                    MapSidebarTarget::Cameo { type_id } => {
                        ensure!(!type_id.is_empty(), "empty sidebar cameo identity")
                    }
                    MapSidebarTarget::ScrollUp {}
                    | MapSidebarTarget::ScrollDown {}
                    | MapSidebarTarget::Repair {}
                    | MapSidebarTarget::Sell {} => {}
                }
            }
            if let MapGesture::Key { key, .. } = &gesture.gesture {
                literal_key(key)?;
            }
            literal_modifiers(gesture.gesture.modifiers())?;
            if let MapGesture::CommandBar { command } = &gesture.gesture {
                command_bar_index(command)?;
            }
            ensure!(
                !gesture.gesture.is_quickload() || self.allow_load_segments == Some(true),
                "literal quickload requires allow_load_segments"
            );
            previous = gesture.issue_after_step;
        }
        if let Some(steps) = &self.observe_sidebar_steps {
            ensure!(steps.len() <= MAX_GESTURES, "too many sidebar observations");
            ensure!(
                steps.iter().all(|step| *step <= self.ticks)
                    && steps.windows(2).all(|pair| pair[0] < pair[1]),
                "sidebar observation steps must be unique, increasing and within the capture"
            );
        }
        if let Some(steps) = &self.observe_electric_bolt_steps {
            ensure!(
                self.observe_electric_bolts == Some(true),
                "electric observation steps require observe_electric_bolts"
            );
            ensure!(
                !steps.is_empty() && steps.len() <= MAX_OBSERVATION_SAMPLES,
                "electric observation steps must contain 1..100000 entries"
            );
            ensure!(
                steps.iter().all(|step| *step <= self.ticks)
                    && steps.windows(2).all(|pair| pair[0] < pair[1]),
                "electric observation steps must be unique, increasing and within the capture"
            );
        }
        let owners = self.observe_owners();
        ensure!(
            owners.len() <= MAX_OBSERVED_OWNERS,
            "too many observed owners"
        );
        let mut unique_owners = BTreeSet::new();
        for owner in owners {
            ensure!(
                !owner.is_empty() && unique_owners.insert(owner),
                "empty or duplicate observed owner"
            );
        }
        if let Some(types) = &self.observe_types {
            ensure!(
                !types.is_empty() && types.len() <= MAX_OBSERVED_TYPES,
                "observed types must contain 1..256 names"
            );
            let mut unique_types = BTreeSet::new();
            for name in types {
                ensure!(
                    !name.is_empty() && unique_types.insert(name),
                    "empty or duplicate observed type"
                );
            }
        }
        if let Some(types) = &self.observe_anim_types {
            ensure!(
                !types.is_empty() && types.len() <= MAX_OBSERVED_TYPES,
                "observed animation types must contain 1..256 names"
            );
            let mut unique = BTreeSet::new();
            for name in types {
                ensure!(
                    !name.is_empty() && unique.insert(name.to_ascii_uppercase()),
                    "empty or duplicate observed animation type"
                );
            }
        }
        let cells = self.terrain_cells();
        ensure!(
            cells.len() <= MAX_TERRAIN_CELLS,
            "too many observed terrain cells"
        );
        ensure!(
            cells.iter().collect::<BTreeSet<_>>().len() == cells.len(),
            "duplicate observed terrain cell"
        );
        ensure!(
            matches!(
                crate::app::match_bootstrap::classify_startup_session(&self.launch),
                StartupSessionClassification::AcceptedExplicitFixedBattle(_)
            ),
            "map observation requires an explicit fixed Battle launch"
        );
        ensure!(
            self.launch.humans.len() == 1
                && self.launch.local_player_index == 0
                && self.launch.humans[0].raw_slot_index == Some(0)
                && !self.launch.humans[0].is_observer()
                && self.launch.ai_player_count == self.launch.opponents.len() as i32
                && self
                    .launch
                    .opponents
                    .iter()
                    .enumerate()
                    .all(|(index, opponent)| usize::from(opponent.raw_slot_index) == index + 1)
                && self.launch.house_roster_is_consistent(),
            "map observation requires the ordinary one-human compact skirmish roster"
        );
        let mut starts = std::collections::BTreeSet::new();
        let mut colors = std::collections::BTreeSet::new();
        for (start, color) in std::iter::once((
            self.launch.local_slot().start_position,
            self.launch.local_slot().color_index,
        ))
        .chain(
            self.launch
                .opponents
                .iter()
                .map(|slot| (slot.start_position, slot.color_index)),
        ) {
            let LaunchStartPosition::Position(start) = start else {
                bail!("automatic start is unsupported")
            };
            ensure!(
                start < 8 && starts.insert(start),
                "invalid or duplicate explicit start"
            );
            ensure!(
                color < 8 && colors.insert(color),
                "invalid or duplicate house color"
            );
        }
        Ok(())
    }

    pub(super) fn commands(&self) -> &[MapScheduledCommand] {
        self.commands.as_deref().unwrap_or_default()
    }

    pub(super) fn observes_electric_step(&self, step: u64) -> bool {
        self.observe_electric_bolts == Some(true)
            && self
                .observe_electric_bolt_steps
                .as_ref()
                .is_none_or(|steps| {
                    u32::try_from(step).is_ok_and(|step| steps.binary_search(&step).is_ok())
                })
    }

    pub(super) fn gestures(&self) -> &[MapScheduledGesture] {
        self.gestures.as_deref().unwrap_or_default()
    }

    pub(super) fn gesture_policy(&self) -> &'static str {
        if self
            .gestures()
            .iter()
            .any(|entry| entry.gesture.is_right_button())
        {
            RIGHT_BUTTON_GESTURE_POLICY
        } else if self
            .gestures()
            .iter()
            .any(|entry| matches!(entry.gesture, MapGesture::CommandBar { .. }))
        {
            COMMAND_BAR_GESTURE_POLICY
        } else if self.observes_local_input() {
            KEYBOARD_GESTURE_POLICY
        } else if self
            .gestures()
            .iter()
            .any(|entry| matches!(entry.gesture, MapGesture::Sidebar { .. }))
        {
            SIDEBAR_GESTURE_POLICY
        } else {
            GESTURE_POLICY
        }
    }

    pub(super) fn observes_effects(&self) -> bool {
        self.observe_projectiles == Some(true)
            || self.observe_waves == Some(true)
            || self.observe_anim_types.is_some()
    }

    pub(super) fn observes_local_input(&self) -> bool {
        self.gestures().iter().any(|entry| {
            matches!(entry.gesture, MapGesture::Key { .. })
                || entry.gesture.is_minimap()
                || entry.gesture.is_right_button()
                || !entry.gesture.modifiers().is_empty()
        })
    }

    pub(super) fn observes_sidebar_step(&self, frame: &MapFrameObservation) -> bool {
        // Requested rows follow exact-step receipts even when a load rewinds
        // the simulation clock retained by the same frame observation.
        let step = frame.completed_steps;
        self.observe_sidebar_steps.as_ref().is_some_and(|steps| {
            u32::try_from(step).is_ok_and(|step| steps.binary_search(&step).is_ok())
        })
    }

    pub(super) fn observe_owners(&self) -> &[String] {
        self.observe_owners.as_deref().unwrap_or_default()
    }

    pub(super) fn terrain_cells(&self) -> &[[u16; 2]] {
        self.terrain_cells.as_deref().unwrap_or_default()
    }

    pub(super) fn validate_observed_rule_types(&self, rule_types: &[Value]) -> Result<()> {
        if let Some(types) = &self.observe_types {
            for name in types {
                ensure!(
                    rule_types
                        .iter()
                        .any(|row| row["type_id"].as_str() == Some(name.as_str())),
                    "observed type {name:?} is absent from the loaded rule types"
                );
            }
        }
        Ok(())
    }
}
