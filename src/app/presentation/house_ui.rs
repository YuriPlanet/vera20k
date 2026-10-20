//! A House's player-facing name and text colour, shared by the score rows
//! (`0x005C98FE`) and the Diplomacy rows (`0x00658330`): both read the same
//! House UI name `+0x1602A` and colour it through the House's colour scheme.

use crate::app::AppState;
use crate::map::houses::HouseColorMap;
use crate::rules::color_scheme::ColorSchemeEntry;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// What names and colours a House row.
pub(crate) struct HouseUiSources<'a> {
    /// The local player's handle; `None` for a launch without one.
    pub handle: Option<&'a str>,
    /// `TXT_COMPUTER`, already resolved.
    pub computer: &'a str,
    pub schemes: &'a [ColorSchemeEntry],
    pub colors: &'a HouseColorMap,
}

impl<'a> HouseUiSources<'a> {
    /// The running match's sources. `computer` is the caller's resolved
    /// `TXT_COMPUTER`.
    pub(crate) fn from_state(state: &'a AppState, computer: &'a str) -> Self {
        Self {
            handle: state.match_state.session.local_player_name(),
            computer,
            schemes: state
                .rules()
                .map_or(&[][..], |rules| rules.color_schemes.as_slice()),
            colors: &state.match_state.match_presentation.house_color_map,
        }
    }

    /// The House's UI name (`house+0x1602A`). Skirmish setup gives a human
    /// House its player's handle (`0x00688094`) and every computer House
    /// `TXT_COMPUTER` (`0x00688252..0x00688270`). A human without a recorded
    /// handle (a launch with no skirmish session) shows its country's name,
    /// else its House name.
    pub(crate) fn ui_name(
        &self,
        sim: &Simulation,
        owner: InternedId,
        country: Option<InternedId>,
        country_name: impl Fn(InternedId) -> Option<String>,
    ) -> String {
        let is_human = sim
            .houses()
            .get(&owner)
            .is_some_and(|house| house.is_human());
        if !is_human {
            return self.computer.to_string();
        }
        match self.handle {
            Some(handle) => handle.to_string(),
            None => country
                .and_then(country_name)
                .unwrap_or_else(|| sim.interner().resolve(owner).to_string()),
        }
    }

    /// The House's colour scheme (`house+0x16054`) as text RGB.
    pub(crate) fn text_rgb(&self, sim: &Simulation, owner: InternedId) -> [u8; 3] {
        let color = self
            .colors
            .get(sim.interner().resolve(owner))
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        crate::rules::house_colors::house_text_rgb(self.schemes, color)
    }

    /// A named scheme's text RGB, as `ColorScheme::FindIndex` (`0x0068CA50`)
    /// finds it; `None` when `[Colors]` lacks it.
    pub(crate) fn named_scheme_rgb(&self, name: &str) -> Option<[u8; 3]> {
        let entry = crate::rules::color_scheme::scheme_entry_by_name(self.schemes, name)?;
        Some(crate::rules::house_colors::house_text_rgb(
            self.schemes,
            crate::rules::house_colors::HouseColorIndex(u8::try_from(entry).ok()?),
        ))
    }
}

/// A country's display name: its `UIName=` through the string table, else
/// its plain `Name=`.
pub(crate) fn country_display_name(
    state: &AppState,
    sim: &Simulation,
    country: InternedId,
) -> Option<String> {
    let country = sim.interner().resolve(country);
    let (ui_key, plain) = state
        .rules()
        .map(|rules| rules.country_display_name_sources(country))
        .unwrap_or((None, None));
    let localized = ui_key
        .zip(state.process_assets.csf.as_ref())
        .map(|(key, csf)| csf.text(key).into_owned())
        .filter(|text| Some(text.as_str()) != ui_key);
    localized.or_else(|| plain.map(str::to_string))
}
