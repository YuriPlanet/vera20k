//! Command-bar assets share the ordinary side archive and87F6CC converter.

use super::*;

pub struct CommandBarArt<T> {
    pub spacer: Option<T>,
    pub left_cap: [Option<T>; 3],
    pub right_cap: Option<T>,
    pub background: Option<T>,
    pub buttons: [[Option<T>; 2]; 11],
    pub slots: [Vec<Option<usize>>; 2],
}

impl<T> CommandBarArt<T> {
    pub fn entries(&self) -> impl Iterator<Item = &T> {
        self.spacer
            .iter()
            .chain(self.left_cap.iter().flatten())
            .chain(self.right_cap.iter())
            .chain(self.background.iter())
            .chain(self.buttons.iter().flatten().flatten())
    }
    pub fn map<U>(&self, mut f: impl FnMut(&T) -> U) -> CommandBarArt<U> {
        CommandBarArt {
            spacer: self.spacer.as_ref().map(&mut f),
            left_cap: self
                .left_cap
                .each_ref()
                .map(|entry| entry.as_ref().map(&mut f)),
            right_cap: self.right_cap.as_ref().map(&mut f),
            background: self.background.as_ref().map(&mut f),
            buttons: self
                .buttons
                .each_ref()
                .map(|frames| frames.each_ref().map(|entry| entry.as_ref().map(&mut f))),
            slots: self.slots.clone(),
        }
    }
}

pub(super) fn load(
    route: SidebarSideRoute<'_>,
    palette: &Palette,
    theme: SidebarTheme,
) -> CommandBarArt<RenderedChromeEntry> {
    let render = |name: &str, frame| route.render(name, palette, frame);
    // The bar's pictures, as the built-in `veraui_default.ini` names them
    // (`[Bar] Rail=`, `LeftCap=`, `RightCap=`, `Slot=`).
    let piece = |key, frame| render(built_in_name("Bar", key, theme)?, frame);
    // InitSideMixFiles534FA0 loads UIMD.INI before674650 installs ButtonList.
    let ini = route
        .asset_manager
        .get_ref("UIMD.INI")
        .and_then(|bytes| crate::rules::ini_parser::IniFile::from_bytes(bytes).ok());
    let slots = ["AdvancedCommandBar", "MultiplayerAdvancedCommandBar"].map(|section| {
        ini.as_ref()
            .and_then(|ini| ini.section(section))
            .map(crate::ui::sidebar::command_bar::read_button_list)
            .unwrap_or_default()
    });
    CommandBarArt {
        spacer: piece("Rail", 0),
        left_cap: std::array::from_fn(|i| piece("LeftCap", i)),
        right_cap: piece("RightCap", 0),
        background: piece("Slot", 0),
        // TabClass::LoadSHPs @ 0x006D02B0 loads Button00..Button24.SHP
        // (FreeSHPs @ 0x006D0270 frees them), indexed by command; only the
        // 11 command names (6CFCC0) can take a slot, so 11..24 never draw.
        buttons: std::array::from_fn(|id| {
            std::array::from_fn(|frame| render(&format!("Button{id:02}.SHP"), frame))
        }),
        slots,
    }
}
