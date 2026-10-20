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
    /// The string table a map `UIName=` key resolves through.
    pub csf: Option<&'a crate::assets::csf_file::CsfFile>,
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
            csf: state.process_assets.csf.as_ref(),
            computer,
            schemes: state
                .rules()
                .map_or(&[][..], |rules| rules.color_schemes.as_slice()),
            colors: &state.match_state.match_presentation.house_color_map,
        }
    }

    /// The House's UI name (`house+0x1602A`), as the House holds it. The
    /// type's UIName shows its country's display name, else the House name.
    pub(crate) fn ui_name(
        &self,
        sim: &Simulation,
        owner: InternedId,
        country: Option<InternedId>,
        country_name: impl Fn(InternedId) -> Option<String>,
    ) -> String {
        use crate::sim::house_state::HouseUiName;
        let type_name = || {
            country
                .and_then(&country_name)
                .unwrap_or_else(|| sim.interner().resolve(owner).to_string())
        };
        match sim.houses().get(&owner).map(|house| house.ui_name()) {
            Some(HouseUiName::Handle(handle)) => handle.clone(),
            Some(HouseUiName::Computer) => self.computer.to_string(),
            // Read_Scenario_INI 500CD7..500CEC: LoadString, then 20 chars.
            Some(HouseUiName::Csf(key)) => HouseUiName::bounded(
                &self
                    .csf
                    .map_or_else(|| key.clone(), |csf| csf.text(key).into_owned()),
            ),
            Some(HouseUiName::TypeName) | None => type_name(),
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

/// The running match's UI name for `owner` (House+0x1602A), as every
/// House-named message reads it.
pub(crate) fn house_ui_name(state: &AppState, owner: InternedId) -> Option<String> {
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    let country = sim.houses().get(&owner)?.country();
    let computer = crate::app::frontend::shell_pass::resolve_csf(state, "TXT_COMPUTER");
    Some(
        HouseUiSources::from_state(state, &computer).ui_name(sim, owner, country, |country| {
            country_display_name(state, sim, country)
        }),
    )
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
