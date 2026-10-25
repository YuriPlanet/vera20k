//! Landscape touch controls and editable, persisted shortcut order.
//!
//! This is application UI state only. The existing input command owners execute
//! the semantic shortcuts; neither this model nor its JSON owns game state.

use super::ios_battle_touch::{ControlRegion, JoystickRegion, TouchLayout, TouchRect};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const MAX_SHORTCUTS: usize = 16;
const CURRENT_PAGE_SIZE: usize = 6;
const CATALOG_PAGE_SIZE: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Shortcut {
    Home,
    Type,
    Combat,
    Event,
    Scatter,
    Deploy,
    Next,
    Stop,
    AttackMove,
    ForceAttack,
    Cancel,
    View,
    Guard,
    Team1,
    Team2,
    Team3,
    Planning,
    Repair,
    Sell,
    Pause,
    Edit,
    Save,
    Discard,
    Reset,
    Remove,
    MoveLeft,
    MoveRight,
    PagePrevious,
    PageNext,
    SlotPagePrevious,
    SlotPageNext,
    Slot(usize),
    Catalog(usize),
}

pub(super) const CATALOG: [Shortcut; 20] = [
    Shortcut::Home,
    Shortcut::Type,
    Shortcut::Combat,
    Shortcut::Event,
    Shortcut::Scatter,
    Shortcut::Deploy,
    Shortcut::Next,
    Shortcut::Stop,
    Shortcut::AttackMove,
    Shortcut::ForceAttack,
    Shortcut::Cancel,
    Shortcut::View,
    Shortcut::Guard,
    Shortcut::Team1,
    Shortcut::Team2,
    Shortcut::Team3,
    Shortcut::Planning,
    Shortcut::Repair,
    Shortcut::Sell,
    Shortcut::Pause,
];

impl Shortcut {
    /// Atlas slots 0/1 are joystick base/knob. Slots >=16 are vector glyphs
    /// produced by the renderer, not UV coordinates into the generated atlas.
    pub(super) fn icon_index(self) -> usize {
        match self {
            Self::Home => 2,
            Self::Type => 3,
            Self::Combat => 4,
            Self::Event => 5,
            Self::Scatter => 6,
            Self::Deploy => 7,
            Self::Next => 8,
            Self::Stop => 9,
            Self::AttackMove => 10,
            Self::ForceAttack => 11,
            Self::Cancel => 12,
            Self::View => 13,
            Self::Guard => 14,
            Self::Team1 | Self::Team2 | Self::Team3 => 15,
            Self::Planning => 16,
            Self::Repair => 17,
            Self::Sell => 18,
            Self::Pause => 19,
            Self::Edit => 20,
            Self::Save => 21,
            Self::Discard => 22,
            Self::Reset => 23,
            Self::Remove => 24,
            Self::MoveLeft => 25,
            Self::MoveRight => 26,
            Self::PagePrevious | Self::SlotPagePrevious => 27,
            Self::PageNext | Self::SlotPageNext => 28,
            Self::Slot(_) | Self::Catalog(_) => 20,
        }
    }

    pub(super) fn id(self) -> Option<&'static str> {
        Some(match self {
            Self::Home => "home",
            Self::Type => "type",
            Self::Combat => "combat",
            Self::Event => "event",
            Self::Scatter => "scatter",
            Self::Deploy => "deploy",
            Self::Next => "next",
            Self::Stop => "stop",
            Self::AttackMove => "attack_move",
            Self::ForceAttack => "force_attack",
            Self::Cancel => "cancel",
            Self::View => "view",
            Self::Guard => "guard",
            Self::Team1 => "team1",
            Self::Team2 => "team2",
            Self::Team3 => "team3",
            Self::Planning => "planning",
            Self::Repair => "repair",
            Self::Sell => "sell",
            Self::Pause => "pause",
            _ => return None,
        })
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        CATALOG
            .into_iter()
            .find(|shortcut| shortcut.id() == Some(id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Preferences {
    pub(super) shortcuts: Vec<Shortcut>,
    /// UI-only paging state, deliberately omitted from the saved document.
    pub(super) page: usize,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            shortcuts: vec![
                Shortcut::Team1,
                Shortcut::Team2,
                Shortcut::Type,
                Shortcut::Deploy,
                Shortcut::Guard,
                Shortcut::Stop,
                Shortcut::Planning,
            ],
            page: 0,
        }
    }
}

impl Preferences {
    pub(super) fn normalized(&self) -> Self {
        let mut seen = HashSet::new();
        Self {
            shortcuts: self
                .shortcuts
                .iter()
                .copied()
                .filter(|value| value.id().is_some() && seen.insert(*value))
                .take(MAX_SHORTCUTS)
                .collect(),
            page: self.page,
        }
    }

    pub(super) fn add(&mut self, shortcut: Shortcut) -> bool {
        if shortcut.id().is_none()
            || self.shortcuts.len() >= MAX_SHORTCUTS
            || self.shortcuts.contains(&shortcut)
        {
            return false;
        }
        self.shortcuts.push(shortcut);
        true
    }

    pub(super) fn remove(&mut self, index: usize) -> bool {
        if index >= self.shortcuts.len() {
            return false;
        }
        self.shortcuts.remove(index);
        true
    }

    pub(super) fn move_shortcut(&mut self, from: usize, to: usize) -> bool {
        if from >= self.shortcuts.len() || to >= self.shortcuts.len() || from == to {
            return false;
        }
        let shortcut = self.shortcuts.remove(from);
        self.shortcuts.insert(to, shortcut);
        true
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn load(path: &Path) -> io::Result<Self> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let value: serde_json::Value = serde_json::from_slice(&data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if value.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported shortcut version",
            ));
        }
        let values = value
            .get("shortcuts")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "shortcuts must be an array")
            })?;
        let preferences = Self {
            shortcuts: values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter_map(Shortcut::from_id)
                .collect(),
            page: 0,
        };
        Ok(preferences.normalized())
    }

    pub(super) fn save(&self, path: &Path) -> io::Result<()> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let preferences = self.normalized();
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "shortcuts": preferences.shortcuts.iter().filter_map(|value| value.id()).collect::<Vec<_>>(),
        })).map_err(io::Error::other)?;
        let parent = path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "shortcut path needs a file name",
            )
        })?;
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = parent.join(temporary_name);
        let mut temporary_owned = false;
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            temporary_owned = true;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)
        })();
        if result.is_err() && temporary_owned {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

#[derive(Clone, Debug)]
pub(super) struct ShortcutEditor {
    pub(super) draft: Preferences,
    pub(super) selected_slot: Option<usize>,
    pub(super) current_page: usize,
    pub(super) catalog_page: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EditorOutcome {
    None,
    Changed,
    Save(Preferences),
    Discard,
}

impl ShortcutEditor {
    pub(super) fn new(preferences: &Preferences) -> Self {
        let draft = preferences.normalized();
        let selected_slot = (!draft.shortcuts.is_empty()).then_some(0);
        Self {
            draft,
            selected_slot,
            current_page: 0,
            catalog_page: 0,
        }
    }

    pub(super) fn handle(&mut self, command: Shortcut) -> EditorOutcome {
        use Shortcut::*;
        match command {
            Save => return EditorOutcome::Save(self.draft.normalized()),
            Discard | Edit => return EditorOutcome::Discard,
            Reset => {
                self.draft.reset();
                self.selected_slot = Some(0);
                self.current_page = 0;
            }
            Slot(index) if index < self.draft.shortcuts.len() => self.selected_slot = Some(index),
            Catalog(index) if index < CATALOG.len() => {
                if !self.draft.add(CATALOG[index]) {
                    return EditorOutcome::None;
                }
                self.selected_slot = Some(self.draft.shortcuts.len() - 1);
                self.current_page = self.selected_slot.unwrap() / CURRENT_PAGE_SIZE;
            }
            Remove => {
                let Some(index) = self.selected_slot else {
                    return EditorOutcome::None;
                };
                if !self.draft.remove(index) {
                    return EditorOutcome::None;
                }
                self.selected_slot = (!self.draft.shortcuts.is_empty())
                    .then(|| index.min(self.draft.shortcuts.len() - 1));
                self.current_page = self.selected_slot.unwrap_or(0) / CURRENT_PAGE_SIZE;
            }
            MoveLeft | MoveRight => {
                let Some(from) = self.selected_slot else {
                    return EditorOutcome::None;
                };
                let to = if command == MoveLeft {
                    from.saturating_sub(1)
                } else {
                    from + 1
                };
                if !self.draft.move_shortcut(from, to) {
                    return EditorOutcome::None;
                }
                self.selected_slot = Some(to);
                self.current_page = to / CURRENT_PAGE_SIZE;
            }
            PagePrevious | PageNext => {
                let count = CATALOG.len().div_ceil(CATALOG_PAGE_SIZE);
                self.catalog_page = next_page(self.catalog_page, count, command == PageNext);
            }
            SlotPagePrevious | SlotPageNext => {
                let count = self
                    .draft
                    .shortcuts
                    .len()
                    .div_ceil(CURRENT_PAGE_SIZE)
                    .max(1);
                self.current_page = next_page(self.current_page, count, command == SlotPageNext);
            }
            _ => return EditorOutcome::None,
        }
        EditorOutcome::Changed
    }
}

fn next_page(page: usize, count: usize, forward: bool) -> usize {
    let count = count.max(1);
    let page = page % count;
    if forward {
        (page + 1) % count
    } else {
        (page + count - 1) % count
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ShortcutButton {
    pub(super) rect: TouchRect,
    pub(super) command: Shortcut,
    pub(super) icon: Shortcut,
    pub(super) selected: bool,
    pub(super) enabled: bool,
}

pub(super) struct LandscapeLayout {
    pub(super) touch: TouchLayout,
    pub(super) buttons: Vec<ShortcutButton>,
    pub(super) bar: TouchRect,
    /// The painted strip extends from just above the reserved bar anchor to
    /// the window bottom. Keep it separate so centering buttons cannot move
    /// the world/joystick/editor boundary or move their background with them.
    bar_panel: TouchRect,
    pub(super) editor_panel: Option<TouchRect>,
    pub(super) page_count: usize,
}

impl LandscapeLayout {
    pub(super) fn bar_background(&self) -> TouchRect {
        self.bar_panel
    }

    /// All ordinary shortcuts, page arrows and the fixed editor button share
    /// the center of the full painted strip, including its safe-area padding.
    pub(super) fn bar_button_y(&self) -> f32 {
        self.bar_panel.y + (self.bar_panel.height - self.bar.height) * 0.5
    }

    /// All arguments and output rectangles use physical window pixels. `scale`
    /// is the physical-pixels-per-point value, not the tactical zoom.
    pub(super) fn new(
        width: f32,
        height: f32,
        scale: f32,
        sidebar_width: f32,
        preferences: &Preferences,
        editor: Option<&ShortcutEditor>,
    ) -> Self {
        let width = finite_positive(width, 1.0);
        let height = finite_positive(height, 1.0);
        let scale = finite_positive(scale, 1.0);
        let left = (52.0 * scale).min(width * 0.16);
        let right = (width - sidebar_width.max(52.0 * scale))
            .max(left + 1.0)
            .min(width);
        let bottom = (24.0 * scale).min(height * 0.12);
        let available = (right - left).max(1.0);
        // At iPhone landscape sizes these are 44pt targets. The fit cap keeps
        // synthetic/very small resized windows bounded rather than overlapping.
        let size = (44.0 * scale).min(available / 4.6).min(height * 0.15);
        let gap = (8.0 * scale).min(size * 0.2);
        let bar = rect(left, (height - bottom - size).max(0.0), available, size);
        let panel_top = (bar.y - 4.0 * scale).max(0.0);
        let bar_panel = rect(0.0, panel_top, right, height - panel_top);
        let capacity_without_pages =
            ((available - size - gap) / (size + gap)).floor().max(1.0) as usize;
        let normalized = preferences.normalized();
        let needs_pages = normalized.shortcuts.len() > capacity_without_pages;
        let capacity = if needs_pages {
            capacity_without_pages.saturating_sub(2).max(1)
        } else {
            capacity_without_pages
        };
        let page_count = normalized.shortcuts.len().div_ceil(capacity).max(1);
        let page = preferences.page % page_count;
        let mut result = Self {
            touch: TouchLayout {
                scale_factor: scale,
                world: rect(0.0, 0.0, (width - sidebar_width).max(0.0), bar.y),
                joystick: None,
                controls: Vec::new(),
                excluded: vec![rect(
                    0.0,
                    bar.y,
                    (width - sidebar_width).max(0.0),
                    height - bar.y,
                )],
            },
            buttons: Vec::new(),
            bar,
            bar_panel,
            editor_panel: None,
            page_count,
        };
        let button_y = result.bar_button_y();
        let settings = rect(right - size, button_y, size, size);
        result.button(
            settings,
            Shortcut::Edit,
            Shortcut::Edit,
            editor.is_some(),
            true,
        );
        if let Some(editor) = editor {
            result.editor(width, height, scale, left, right, size, gap, editor);
            return result;
        }
        for (slot, shortcut) in normalized
            .shortcuts
            .iter()
            .enumerate()
            .skip(page * capacity)
            .take(capacity)
        {
            let x = left + (slot % capacity) as f32 * (size + gap);
            result.button(
                rect(x, button_y, size, size),
                *shortcut,
                *shortcut,
                false,
                true,
            );
        }
        if needs_pages {
            result.button(
                rect(settings.x - 2.0 * (size + gap), button_y, size, size),
                Shortcut::PagePrevious,
                Shortcut::PagePrevious,
                false,
                true,
            );
            result.button(
                rect(settings.x - size - gap, button_y, size, size),
                Shortcut::PageNext,
                Shortcut::PageNext,
                false,
                true,
            );
        }
        let radius = (48.0 * scale).min(available * 0.18).min(height * 0.18);
        let ring_size = size;
        let orbit = radius + ring_size * 0.65;
        let center = [
            left + radius,
            (bar.y - gap - radius - ring_size * 0.6).max(radius),
        ];
        result.touch.joystick = Some(JoystickRegion {
            center,
            radius,
            travel: 32.0 * scale,
        });
        for (command, degrees) in [
            (Shortcut::AttackMove, -90.0_f32),
            (Shortcut::ForceAttack, -30.0),
            (Shortcut::Cancel, 25.0),
        ] {
            let angle = degrees.to_radians();
            let x = center[0] + orbit * angle.cos() - ring_size * 0.5;
            let y = center[1] + orbit * angle.sin() - ring_size * 0.5;
            result.button(
                rect(x, y, ring_size, ring_size),
                command,
                command,
                false,
                true,
            );
        }
        result
    }

    fn button(
        &mut self,
        rect: TouchRect,
        command: Shortcut,
        icon: Shortcut,
        selected: bool,
        enabled: bool,
    ) {
        self.buttons.push(ShortcutButton {
            rect,
            command,
            icon,
            selected,
            enabled,
        });
        if enabled {
            self.touch.controls.push(ControlRegion { rect, command });
        }
        // Disabled controls also reserve their area: a full toolbar item must
        // never pass a world-order tap through its greyed icon.
        self.touch.excluded.push(rect);
    }

    fn editor(
        &mut self,
        width: f32,
        height: f32,
        scale: f32,
        left: f32,
        right: f32,
        size: f32,
        gap: f32,
        editor: &ShortcutEditor,
    ) {
        let inner_width = (right - left).max(1.0);
        // Keep eight cells across: six editable slots plus two page arrows,
        // and six editing actions plus the catalogue arrows. A narrow phone
        // fits by reducing this editor's cells; the ordinary battle targets
        // keep their 44pt size and page instead.
        let size = size
            .min(40.0 * scale)
            .min(inner_width / 8.9)
            .min(((self.bar.y - gap * 2.0) / 4.6).max(1.0));
        let gap = gap.min(6.0 * scale).min(size * 0.12);
        // Six current slots and twelve catalogue entries per page; the page
        // sizes are shared with ShortcutEditor::handle so resize never changes
        // the meaning of an already-pressed page control.
        let columns = ((inner_width + gap) / (size + gap))
            .floor()
            .max(1.0)
            .min(8.0) as usize;
        let current_rows = (CURRENT_PAGE_SIZE + 2).div_ceil(columns);
        let catalog_rows = CATALOG_PAGE_SIZE.div_ceil(columns);
        let action_rows = 8_usize.div_ceil(columns);
        let total_rows = current_rows + catalog_rows + action_rows;
        let row_step = size + gap;
        let panel_width = columns as f32 * row_step - gap;
        let left = left + (inner_width - panel_width) * 0.5;
        let panel_height = total_rows as f32 * row_step + gap;
        let panel = rect(
            left,
            (self.bar.y - gap - panel_height).max(0.0),
            panel_width,
            panel_height.min(self.bar.y),
        );
        self.editor_panel = Some(panel);
        self.touch.world = rect(0.0, 0.0, 0.0, 0.0);
        self.touch.excluded.push(rect(0.0, 0.0, width, height));
        let current_page_count = editor
            .draft
            .shortcuts
            .len()
            .div_ceil(CURRENT_PAGE_SIZE)
            .max(1);
        let current_page = editor.current_page % current_page_count;
        let mut y = panel.y + gap;
        for (offset, index) in (current_page * CURRENT_PAGE_SIZE..editor.draft.shortcuts.len())
            .take(CURRENT_PAGE_SIZE)
            .enumerate()
        {
            self.button(
                rect(
                    left + (offset % columns) as f32 * row_step,
                    y + (offset / columns) as f32 * row_step,
                    size,
                    size,
                ),
                Shortcut::Slot(index),
                editor.draft.shortcuts[index],
                editor.selected_slot == Some(index),
                true,
            );
        }
        for (offset, command) in [Shortcut::SlotPagePrevious, Shortcut::SlotPageNext]
            .into_iter()
            .enumerate()
        {
            let position = CURRENT_PAGE_SIZE + offset;
            self.button(
                rect(
                    left + (position % columns) as f32 * row_step,
                    y + (position / columns) as f32 * row_step,
                    size,
                    size,
                ),
                command,
                command,
                false,
                current_page_count > 1,
            );
        }
        y += current_rows as f32 * row_step;
        for (offset, command) in [
            Shortcut::MoveLeft,
            Shortcut::Remove,
            Shortcut::MoveRight,
            Shortcut::Reset,
            Shortcut::Save,
            Shortcut::Discard,
        ]
        .into_iter()
        .enumerate()
        {
            let selected = editor.selected_slot;
            let enabled = match command {
                Shortcut::MoveLeft => selected.is_some_and(|slot| slot > 0),
                Shortcut::MoveRight => {
                    selected.is_some_and(|slot| slot + 1 < editor.draft.shortcuts.len())
                }
                Shortcut::Remove => selected.is_some(),
                _ => true,
            };
            self.button(
                rect(
                    left + (offset % columns) as f32 * row_step,
                    y + (offset / columns) as f32 * row_step,
                    size,
                    size,
                ),
                command,
                command,
                false,
                enabled,
            );
        }
        for (offset, command) in [Shortcut::PagePrevious, Shortcut::PageNext]
            .into_iter()
            .enumerate()
        {
            let position = 6 + offset;
            self.button(
                rect(
                    left + (position % columns) as f32 * row_step,
                    y + (position / columns) as f32 * row_step,
                    size,
                    size,
                ),
                command,
                command,
                false,
                true,
            );
        }
        y += action_rows as f32 * row_step;
        let catalog_page = editor.catalog_page % CATALOG.len().div_ceil(CATALOG_PAGE_SIZE);
        for (offset, index) in (catalog_page * CATALOG_PAGE_SIZE..CATALOG.len())
            .take(CATALOG_PAGE_SIZE)
            .enumerate()
        {
            let shortcut = CATALOG[index];
            let present = editor.draft.shortcuts.contains(&shortcut);
            self.button(
                rect(
                    left + (offset % columns) as f32 * row_step,
                    y + (offset / columns) as f32 * row_step,
                    size,
                    size,
                ),
                Shortcut::Catalog(index),
                shortcut,
                present,
                !present && editor.draft.shortcuts.len() < MAX_SHORTCUTS,
            );
        }
    }
}

fn finite_positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> TouchRect {
    TouchRect {
        x,
        y,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_are_isolated_until_save_and_cancel_preserves_original() {
        let preferences = Preferences::default();
        let mut editor = ShortcutEditor::new(&preferences);
        editor.handle(Shortcut::Remove);
        editor.handle(Shortcut::Catalog(0));
        editor.handle(Shortcut::MoveLeft);
        assert_eq!(preferences, Preferences::default());
        assert_eq!(editor.handle(Shortcut::Discard), EditorOutcome::Discard);
        let EditorOutcome::Save(saved) = editor.handle(Shortcut::Save) else {
            panic!("save");
        };
        assert_ne!(saved, preferences);
        editor.handle(Shortcut::Reset);
        assert_eq!(editor.draft, preferences);
    }

    #[test]
    fn validation_only_keeps_known_unique_shortcuts_up_to_limit() {
        let mut preferences = Preferences {
            shortcuts: vec![Shortcut::Edit, Shortcut::Home, Shortcut::Home],
            page: 0,
        };
        preferences.shortcuts.extend(CATALOG);
        let validated = preferences.normalized();
        assert_eq!(validated.shortcuts.len(), MAX_SHORTCUTS);
        assert_eq!(validated.shortcuts[0], Shortcut::Home);
        assert!(!validated.shortcuts.contains(&Shortcut::Edit));
        assert!(!preferences.add(Shortcut::Save));
    }

    #[test]
    fn atomic_preferences_roundtrip_and_unknown_values_are_sanitized() {
        let directory = std::env::temp_dir().join(format!(
            "vera20k-ios-shortcuts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("shortcuts.json");
        assert_eq!(Preferences::load(&path).unwrap(), Preferences::default());
        let mut preferences = Preferences::default();
        preferences.move_shortcut(0, 3);
        preferences.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).unwrap(), preferences);
        fs::write(
            &path,
            br#"{"version":1,"shortcuts":["home","unknown","home","stop",7]}"#,
        )
        .unwrap();
        assert_eq!(
            Preferences::load(&path).unwrap().shortcuts,
            vec![Shortcut::Home, Shortcut::Stop]
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn landscape_has_three_ring_commands_fixed_settings_and_paged_full_bar() {
        let preferences = Preferences {
            shortcuts: CATALOG[..16].to_vec(),
            page: 0,
        };
        let layout = LandscapeLayout::new(1280.0, 720.0, 2.0, 280.0, &preferences, None);
        assert!(layout.page_count > 1);
        assert!(layout.touch.joystick.is_some());
        for command in [
            Shortcut::AttackMove,
            Shortcut::ForceAttack,
            Shortcut::Cancel,
            Shortcut::Edit,
        ] {
            assert!(
                layout
                    .buttons
                    .iter()
                    .any(|button| button.command == command)
            );
        }
        assert!(layout.buttons.iter().all(|button| button.rect.x >= 0.0
            && button.rect.y >= 0.0
            && button.rect.x + button.rect.width <= 1280.0
            && button.rect.y + button.rect.height <= 720.0));
        let editor = ShortcutEditor::new(&preferences);
        let editing = LandscapeLayout::new(1280.0, 720.0, 2.0, 280.0, &preferences, Some(&editor));
        assert!(editing.touch.joystick.is_none());
        assert_eq!(editing.touch.world.width, 0.0);
        assert!(
            editing
                .buttons
                .iter()
                .any(|button| button.command == Shortcut::Save)
        );
        assert!(
            editing
                .buttons
                .iter()
                .any(|button| button.command == Shortcut::Discard)
        );
    }

    #[test]
    fn bottom_buttons_and_hit_targets_share_the_full_strip_center_at_phone_sizes() {
        let preferences = Preferences {
            shortcuts: CATALOG[..16].to_vec(),
            page: 0,
        };
        let editor = ShortcutEditor::new(&preferences);
        for (width, height, scale, sidebar, original_bar_y) in [
            (568.0, 320.0, 2.0, 173.0, 252.0),
            (844.0, 390.0, 3.0, 180.0, 322.0),
            (926.0, 428.0, 3.0, 180.0, 360.0),
        ] {
            let layout = LandscapeLayout::new(
                width * scale,
                height * scale,
                scale,
                sidebar * scale,
                &preferences,
                None,
            );
            let panel = layout.bar_background();
            // The world and editor anchor stays in its original place; only
            // the row's buttons move inside the existing full-height strip.
            assert_eq!(layout.bar.y, original_bar_y * scale);
            assert_eq!(layout.touch.world.height, layout.bar.y);
            assert_eq!(layout.touch.excluded[0].y, layout.bar.y);
            assert_eq!(panel.y, layout.bar.y - 4.0 * scale);
            assert_eq!(panel.y + panel.height, height * scale);
            assert_eq!(panel.x, 0.0);
            assert_eq!(panel.width, layout.bar.x + layout.bar.width);
            assert!(layout.page_count > 1);
            let row: Vec<_> = layout
                .buttons
                .iter()
                .filter(|button| button.rect.y >= layout.bar.y)
                .collect();
            assert!(
                row.iter()
                    .any(|button| button.command == Shortcut::PagePrevious)
            );
            assert!(
                row.iter()
                    .any(|button| button.command == Shortcut::PageNext)
            );
            assert!(row.iter().any(|button| button.command == Shortcut::Edit));
            for button in row {
                assert_eq!(button.rect.height, 44.0 * scale);
                assert!((button.rect.center()[1] - panel.center()[1]).abs() < 0.001);
                assert!((button.rect.y - layout.bar_button_y()).abs() < 0.001);
                assert!(
                    height * scale - button.rect.y - button.rect.height >= 14.0 * scale - 0.001
                );
                assert!(layout.touch.controls.iter().any(|control| {
                    control.command == button.command && control.rect == button.rect
                }));
            }
            let editing = LandscapeLayout::new(
                width * scale,
                height * scale,
                scale,
                sidebar * scale,
                &preferences,
                Some(&editor),
            );
            let gear = editing
                .buttons
                .iter()
                .find(|button| button.command == Shortcut::Edit)
                .unwrap();
            assert!((gear.rect.center()[1] - editing.bar_background().center()[1]).abs() < 0.001);
            assert_eq!(editing.bar.y, layout.bar.y);
            assert!(
                editing.editor_panel.unwrap().y + editing.editor_panel.unwrap().height
                    <= editing.bar.y
            );
            let joystick = layout.touch.joystick.unwrap();
            assert!((joystick.center[1] / scale - (original_bar_y - 82.4)).abs() < 0.001);
        }
    }

    #[test]
    fn editor_pages_wrap_and_catalog_can_add_last_available_item() {
        let mut editor = ShortcutEditor::new(&Preferences::default());
        editor.handle(Shortcut::PageNext);
        assert_eq!(editor.catalog_page, 1);
        editor.handle(Shortcut::PageNext);
        assert_eq!(editor.catalog_page, 0);
        editor.handle(Shortcut::Catalog(19));
        assert!(editor.draft.shortcuts.contains(&Shortcut::Pause));
        editor.handle(Shortcut::SlotPagePrevious);
        assert!(editor.current_page < editor.draft.shortcuts.len().div_ceil(CURRENT_PAGE_SIZE));
    }

    #[test]
    fn narrow_landscape_editor_and_bottom_safe_area_do_not_leak_input() {
        let preferences = Preferences::default();
        let editor = ShortcutEditor::new(&preferences);
        let layout = LandscapeLayout::new(1136.0, 640.0, 2.0, 320.0, &preferences, Some(&editor));
        let panel = layout.editor_panel.unwrap();
        for button in &layout.buttons {
            assert!(button.rect.x >= 0.0 && button.rect.x + button.rect.width <= 1136.0);
            assert!(button.rect.y >= 0.0 && button.rect.y + button.rect.height <= 640.0);
            if button.command != Shortcut::Edit {
                assert!(button.rect.y >= panel.y);
                assert!(button.rect.y + button.rect.height <= layout.bar.y);
            }
        }
        let battle = LandscapeLayout::new(1136.0, 640.0, 2.0, 320.0, &preferences, None);
        assert!(
            battle
                .touch
                .excluded
                .iter()
                .any(|rect| rect.contains([1.0, 639.0]))
        );
        // The native right sidebar, including its bottom scrolling gadgets,
        // remains a live UI-pointer region rather than part of the new bar.
        assert!(
            !battle
                .touch
                .excluded
                .iter()
                .any(|rect| rect.contains([1135.0, 639.0]))
        );
    }

    #[test]
    fn widened_sidebar_keeps_short_landscape_hud_and_world_gestures_disjoint() {
        use super::super::ios_battle_touch::{BattleTouch, TouchAction};
        use std::time::Instant;
        use winit::dpi::PhysicalPosition;
        use winit::event::{DeviceId, Touch, TouchPhase};
        for (width, height, density) in [(568, 320, 2), (844, 390, 3)] {
            let frame = crate::ui::sidebar::SidebarFrame::for_ios(
                width * density,
                height * density,
                density as f32,
            );
            let layout = LandscapeLayout::new(
                (width * density) as f32,
                (height * density) as f32,
                density as f32,
                frame.width_in_frame() as f32,
                &Preferences::default(),
                None,
            );
            let sidebar_left = frame.frame_left();
            assert!(layout.bar.x + layout.bar.width <= sidebar_left);
            assert!(layout.buttons.iter().all(|button| {
                button.rect.x >= 0.0
                    && button.rect.x + button.rect.width <= sidebar_left
                    && button.rect.y >= 0.0
                    && button.rect.y + button.rect.height <= (height * density) as f32
            }));
            let joystick = layout.touch.joystick.unwrap();
            assert!(joystick.center[0] + joystick.radius <= sidebar_left);
            assert!(joystick.center[1] + joystick.radius < layout.bar.y);
            for button in layout.buttons.iter().filter(|button| {
                matches!(
                    button.command,
                    Shortcut::AttackMove | Shortcut::ForceAttack | Shortcut::Cancel,
                )
            }) {
                assert!(button.rect.y + button.rect.height < layout.bar.y);
            }
            let now = Instant::now();
            let event = |id, phase, [x, y]: [f32; 2]| Touch {
                device_id: DeviceId::dummy(),
                id,
                phase,
                location: PhysicalPosition::new(x as f64, y as f64),
                force: None,
            };
            let mut touch = BattleTouch::default();
            let ui = [
                sidebar_left + frame.width_in_frame() as f32 / 2.0,
                100.0 * density as f32,
            ];
            assert_eq!(
                touch.handle(event(1, TouchPhase::Started, ui), now, &layout.touch),
                vec![TouchAction::BeginDrag { position: ui }]
            );
            assert_eq!(
                touch.handle(event(1, TouchPhase::Ended, ui), now, &layout.touch),
                vec![TouchAction::EndDrag { position: ui }]
            );
            let point = [
                layout.touch.world.width * 0.65,
                layout.touch.world.height * 0.25,
            ];
            assert!(
                touch
                    .handle(event(2, TouchPhase::Started, point), now, &layout.touch)
                    .is_empty()
            );
            assert_eq!(
                touch.handle(event(2, TouchPhase::Ended, point), now, &layout.touch),
                vec![TouchAction::Click {
                    position: point,
                    force: false,
                    attack_move: false
                }]
            );
            let second = [point[0] + 40.0 * density as f32, point[1]];
            touch.handle(event(3, TouchPhase::Started, point), now, &layout.touch);
            touch.handle(event(4, TouchPhase::Started, second), now, &layout.touch);
            let delta = 30.0 * density as f32;
            assert!(
                touch
                    .handle(
                        event(3, TouchPhase::Moved, [point[0] + delta, point[1]]),
                        now,
                        &layout.touch
                    )
                    .is_empty()
            );
            let pan = touch.handle(
                event(4, TouchPhase::Moved, [second[0] + delta, second[1]]),
                now,
                &layout.touch,
            );
            assert!(matches!(pan.as_slice(), [TouchAction::CameraPan { .. }]));
            assert!(
                touch
                    .handle(
                        event(3, TouchPhase::Ended, [point[0] + delta, point[1]]),
                        now,
                        &layout.touch
                    )
                    .is_empty()
            );
            assert!(
                touch
                    .handle(
                        event(4, TouchPhase::Ended, [second[0] + delta, second[1]]),
                        now,
                        &layout.touch
                    )
                    .is_empty()
            );
        }
    }
}
