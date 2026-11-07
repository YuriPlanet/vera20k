//! Per-country end-game presentation: the battlefield verdict picture and its
//! theme, and the score page's background and theme.
//!
//! Product extension. gamemd has one score-screen background per *side* and one
//! theme per side and outcome; it has no per-country presentation at all, so
//! nothing here may cite a native address or claim restoration.
//!
//! ```ini
//! [EndGameScreens]
//! 0=AlliedEnd
//! 1=SovietEnd
//!
//! [AlliedEnd]
//! Country=Americans,British,French,Germans,Alliance
//! ; Banner — over the battlefield, written at the card's own 354x512
//! VictoryArt=ENDAWIN.SHP
//! VictoryPalette=ENDAWIN.PAL
//! DefeatArt=ENDALOS.SHP
//! DefeatPalette=ENDALOS.PAL
//! ; Score — the score page's own 800x600 background
//! ScoreVictoryArt=SCAWIN.SHP
//! ScoreVictoryPalette=SCAWIN.PAL
//! ScoreDefeatArt=SCALOS.SHP
//! ScoreDefeatPalette=SCALOS.PAL
//!
//! [SovietEnd]
//! Country=Russians,Confederation,Africans,Arabs
//! VictoryArt=ENDSWIN.SHP
//! VictoryPalette=ENDSWIN.PAL
//! DefeatArt=ENDSLOS.SHP
//! DefeatPalette=ENDSLOS.PAL
//! ScoreVictoryArt=SCSWIN.SHP
//! ScoreVictoryPalette=SCSWIN.PAL
//! ScoreDefeatArt=SCSLOS.SHP
//! ScoreDefeatPalette=SCSLOS.PAL
//! ```
//!
//! `tools/end_game_cards.py` writes all eight SHP names above from RA3's art:
//! the four `END*` pictures from its end-game **cards**, kept whole at their
//! own 354x512, and the four `SC*` backgrounds from its artwork, cropped to
//! fill the score page's 800x600.
//!
//! ## The two keys that matter: `Victory*` is a banner, `Score*` is a page
//!
//! A finished match paints twice, and the two paints are not the same thing:
//!
//! * **Banner** — the picture over the living battlefield at the moment the
//!   result lands, where `GRFXTXT.SHP` goes. The six plain `Victory*=` /
//!   `Defeat*=` keys above are this one. They came from RA3's end-game *card*,
//!   which is a banner there too, so they belong here.
//! * **Score** — the **score page's** background and theme. That is a separate
//!   picture that happens to answer the same question, so it has its own six
//!   keys: `ScoreVictoryArt=`, `ScoreVictoryPalette=`, `ScoreVictoryMusic=`
//!   and the three `ScoreDefeat*=` beside them.
//!
//! Nothing falls from one half to the other. A rules set that only writes the
//! banner half — which is what the RA3 cards are — gets its picture on the
//! battlefield and leaves the score page exactly as the retail side art and
//! native `SCORE` theme left it. That is deliberate: borrowing the banner art
//! for the score page is how the two got confused in the first place.
//! To give the score page a picture too, write the `Score*` keys.
//!
//! The two halves stay separate keys because they are different things: the
//! `*Music=` pair names a **theme** (a track set from `theme.ini`'s
//! `[Themes]`, the thing `AUDIO` plays), the `*Art=` pair names a **file** (an
//! installed SHP and its palette). A rules set may write either pair alone.
//! Both pairs used to be spelled `VictoryTheme=`/`DefeatTheme=`, which read
//! like a picture; those spellings still answer so an INI written that way
//! keeps working, but a log line asks for the rename.
//!
//! The player switches each *place* off on its own from Options (VERA's page),
//! because the two places are not the same thing:
//!
//! * "Custom End Game Banner" (`[VERA] CustomEndGameBanner=`) is the verdict
//!   over the living battlefield — the picture drawn where `GRFXTXT.SHP` goes,
//!   started together with the theme this catalog names. Off keeps the retail
//!   banner and theme.
//! * "Custom End Game Score" (`[VERA] CustomEndGameScore=`) is the score page
//!   — its own `Score*` picture as the background and its own theme. Off keeps
//!   the retail score page, whatever else this catalog names.
//! * "Custom End Game Movie" (`[VERA] CustomEndGameMovie=`) is the third
//!   switch of the same set, and it governs the wait for a beaten commander's
//!   closing clip rather than any art here. It delays the verdict instead of
//!   painting something.
//!
//! An install without the SHP, or a screen the picture does not suit, therefore
//! turns one off and keeps the other.
//!
//! `Country=` is the owner list `rules.ini` writes for `Owner=`, and the same
//! one [`crate::rules::personality`] uses: the keys of `[Countries]`, comma or
//! space separated, any case. A section with no `Country=` (or `Country=*`)
//! fits every country, so a rules set may name one presentation for everybody
//! and override single countries above it — resolution is registration order,
//! first match wins.
//!
//! The country that decides is the *local player's*, the one the score page is
//! about. A definition is a presentation, not a rule: it never reaches the
//! simulation, and an empty catalog leaves the retail side art and the native
//! `SCORE` theme exactly as they are.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::ini_parser::{IniFile, IniSection};

/// Which half of the score page a presentation is for. The local player's own
/// `HouseClass+0x1F5` defeated flag picks it, the same bit the native score
/// theme selection reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndGameKind {
    Victory,
    Defeat,
}

/// One `[<Name>]` section `[EndGameScreens]` names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndGameDef {
    /// The registry ID that named this section.
    pub name: String,
    /// Uppercase `[Countries]` keys this presentation applies to. `None`
    /// (`Country=` absent, `*` or `All`) applies to every country.
    pub countries: Option<BTreeSet<String>>,
    /// `VictoryMusic=` — the **banner's** theme: what starts over the living
    /// battlefield when the local player won, together with `VictoryArt=`. A
    /// theme name from the loaded theme table, not a file name. The score page
    /// has its own theme below and does not read this one.
    pub victory_music: Option<String>,
    /// `DefeatMusic=` — the banner's theme for a lost match.
    pub defeat_music: Option<String>,
    /// `VictoryArt=` and its `VictoryPalette=`: the banner drawn over the
    /// tactical viewport where `GRFXTXT.SHP` goes, on a win.
    pub victory_art: Option<String>,
    pub victory_palette: Option<String>,
    /// `DefeatArt=` / `DefeatPalette=`, the banner for a lost match.
    pub defeat_art: Option<String>,
    pub defeat_palette: Option<String>,
    /// `ScoreVictoryMusic=` — the **score page's** theme for a won match. The
    /// score page is its own screen with its own art; a rules set that only
    /// brought the banner half leaves these unset and keeps the retail page.
    pub score_victory_music: Option<String>,
    /// `ScoreDefeatMusic=` — the score page's theme for a lost match.
    pub score_defeat_music: Option<String>,
    /// `ScoreVictoryArt=` / `ScoreVictoryPalette=` — the score page's own
    /// background on a win. Nothing here falls back to the banner art: seeing
    /// the battlefield card again behind the score columns is the mix-up this
    /// split exists to prevent.
    pub score_victory_art: Option<String>,
    pub score_victory_palette: Option<String>,
    /// `ScoreDefeatArt=` / `ScoreDefeatPalette=` for a lost match.
    pub score_defeat_art: Option<String>,
    pub score_defeat_palette: Option<String>,
}

/// The background a score page draws instead of the side's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndGameArt {
    /// The SHP whose first frame is the page background.
    pub shape: String,
    /// The palette that frame is drawn with. `None` keeps the side's own
    /// palette, which is what an art override that only swaps the shape needs.
    pub palette: Option<String>,
}

impl EndGameDef {
    /// Whether this presentation covers a house of `country` (a `[Countries]`
    /// key). An absent `Country=` covers every country.
    pub fn admits(&self, country: &str) -> bool {
        self.countries
            .as_ref()
            .is_none_or(|countries| countries.contains(&country.to_ascii_uppercase()))
    }

    /// The **banner's** theme for `kind`, played over the battlefield. `None`
    /// leaves whatever the verdict already plays.
    pub fn banner_music(&self, kind: EndGameKind) -> Option<&str> {
        match kind {
            EndGameKind::Victory => self.victory_music.as_deref(),
            EndGameKind::Defeat => self.defeat_music.as_deref(),
        }
    }

    /// The banner over the tactical viewport for `kind`.
    pub fn banner_art(&self, kind: EndGameKind) -> Option<EndGameArt> {
        let (shape, palette) = match kind {
            EndGameKind::Victory => (&self.victory_art, &self.victory_palette),
            EndGameKind::Defeat => (&self.defeat_art, &self.defeat_palette),
        };
        Some(EndGameArt {
            shape: shape.clone()?,
            palette: palette.clone(),
        })
    }

    /// The **score page's** theme for `kind`, or `None` to keep the side's own.
    pub fn score_music(&self, kind: EndGameKind) -> Option<&str> {
        match kind {
            EndGameKind::Victory => self.score_victory_music.as_deref(),
            EndGameKind::Defeat => self.score_defeat_music.as_deref(),
        }
    }

    /// The score page's own background for `kind`, or `None` to keep the side's
    /// art. Independent of [`Self::banner_art`] on purpose: no picture here
    /// means the retail page, not the battlefield's card.
    pub fn score_art(&self, kind: EndGameKind) -> Option<EndGameArt> {
        let (shape, palette) = match kind {
            EndGameKind::Victory => (&self.score_victory_art, &self.score_victory_palette),
            EndGameKind::Defeat => (&self.score_defeat_art, &self.score_defeat_palette),
        };
        Some(EndGameArt {
            shape: shape.clone()?,
            palette: palette.clone(),
        })
    }

    /// Whether this section says anything at all about `kind`, in either half.
    pub fn covers(&self, kind: EndGameKind) -> bool {
        self.banner_music(kind).is_some()
            || self.banner_art(kind).is_some()
            || self.score_music(kind).is_some()
            || self.score_art(kind).is_some()
    }
}

/// Every `[<Name>]` section `[EndGameScreens]` names, in registration order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndGameCatalog {
    definitions: Vec<EndGameDef>,
}

impl EndGameCatalog {
    /// No `[EndGameScreens]` in any rules layer: the retail score page is
    /// untouched.
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    /// The first definition whose `Country=` admits `country`, whatever it
    /// carries. This is the country question on its own.
    pub fn for_country(&self, country: &str) -> Option<&EndGameDef> {
        self.definitions
            .iter()
            .find(|definition| definition.admits(country))
    }

    /// The presentation a finished match of `country` gets, along with its
    /// music theme and background override. `None` when nothing covers that
    /// country.
    ///
    /// Registration order is the only precedence: the first definition that
    /// admits `country` *and* says something about `kind` wins whole, so a
    /// rules set writes the narrow entries above a catch-all one. An entry that
    /// only configures the win does not silence the catch-all's defeat: the
    /// search skips it for that outcome instead of returning an empty answer.
    pub fn presentation(
        &self,
        country: &str,
        kind: EndGameKind,
    ) -> Option<EndGamePresentation<'_>> {
        let definition = self
            .definitions
            .iter()
            .find(|definition| definition.admits(country) && definition.covers(kind))?;
        Some(EndGamePresentation {
            def: definition,
            kind,
        })
    }

    fn push(&mut self, definition: EndGameDef) {
        self.definitions.push(definition);
    }
}

/// One resolved presentation: the definition plus the half of it that applies.
#[derive(Debug, Clone, Copy)]
pub struct EndGamePresentation<'a> {
    def: &'a EndGameDef,
    kind: EndGameKind,
}

impl EndGamePresentation<'_> {
    /// The banner's theme, started with the card over the battlefield.
    pub fn banner_music(&self) -> Option<&str> {
        self.def.banner_music(self.kind)
    }

    /// The banner picture drawn over the tactical viewport.
    pub fn banner_art(&self) -> Option<EndGameArt> {
        self.def.banner_art(self.kind)
    }

    /// The score page's own theme. Independent of [`Self::banner_music`]: an
    /// unset score theme keeps the retail one even when the banner has its own.
    pub fn score_music(&self) -> Option<&str> {
        self.def.score_music(self.kind)
    }

    /// The score page's own background. Never the banner picture — see
    /// [`EndGameDef::score_art`].
    pub fn score_art(&self) -> Option<EndGameArt> {
        self.def.score_art(self.kind)
    }
}

/// Parse `[EndGameScreens]` and each section it names.
///
/// `ids` is the registry walk (see `parse_registry` in `ruleset`); the caller
/// owns it so the stored-ID projection stays in one place. Nothing here needs
/// the finished `RuleSet`: eligibility is a string comparison against the
/// house's own `[Countries]` key.
pub(crate) fn parse_end_game_screens(ini: &IniFile, ids: &[String]) -> EndGameCatalog {
    let mut catalog = EndGameCatalog::default();
    for id in ids {
        let Some(section) = ini.section(id) else {
            log::warn!("[EndGameScreens] names {id}, but that section is missing");
            continue;
        };
        catalog.push(parse_end_game_def(id, section));
    }
    if !catalog.is_empty() {
        // A country covered by two entries is not an error — the first wins —
        // but it is almost always a copy/paste mistake, so name it once.
        let mut seen: BTreeMap<String, &str> = BTreeMap::new();
        for definition in &catalog.definitions {
            for country in definition.countries.iter().flatten() {
                if let Some(first) = seen.insert(country.clone(), definition.name.as_str()) {
                    log::warn!(
                        "[EndGameScreens] {country} is listed by both {first} and {}; \
                         {first} wins",
                        definition.name
                    );
                }
            }
        }
        let bound = catalog
            .definitions
            .iter()
            .filter(|definition| definition.countries.is_some())
            .count();
        log::info!(
            "EndGameScreens: {} loaded ({bound} with Country=, {} any country)",
            catalog.len(),
            catalog.len().saturating_sub(bound)
        );
    }
    catalog
}

fn parse_end_game_def(id: &str, section: &IniSection) -> EndGameDef {
    let countries = section.read_name("Country", 1024).and_then(|list| {
        let mut keys = BTreeSet::new();
        for token in list.split([',', ' ']).filter(|token| !token.is_empty()) {
            if token == "*" || token.eq_ignore_ascii_case("all") {
                return None;
            }
            keys.insert(token.to_ascii_uppercase());
        }
        (!keys.is_empty()).then_some(keys)
    });
    let victory_art = section.read_name("VictoryArt", 260).map(str::to_owned);
    let defeat_art = section.read_name("DefeatArt", 260).map(str::to_owned);
    let score_victory_art = section.read_name("ScoreVictoryArt", 260).map(str::to_owned);
    let score_defeat_art = section.read_name("ScoreDefeatArt", 260).map(str::to_owned);
    // A palette on its own has nothing to draw; the side's palette would then
    // silently pair with a background the author did not ask for, so say so.
    for (name, art, palette) in [
        ("Victory", victory_art.as_deref(), "VictoryPalette"),
        ("Defeat", defeat_art.as_deref(), "DefeatPalette"),
        (
            "ScoreVictory",
            score_victory_art.as_deref(),
            "ScoreVictoryPalette",
        ),
        (
            "ScoreDefeat",
            score_defeat_art.as_deref(),
            "ScoreDefeatPalette",
        ),
    ] {
        if art.is_none() && section.read_name(palette, 260).is_some() {
            log::warn!("[{id}] {palette}= is ignored without {name}Art=");
        }
    }
    EndGameDef {
        name: id.to_owned(),
        countries,
        victory_music: read_music(section, id, "VictoryMusic", "VictoryTheme"),
        defeat_music: read_music(section, id, "DefeatMusic", "DefeatTheme"),
        victory_palette: section.read_name("VictoryPalette", 260).map(str::to_owned),
        defeat_palette: section.read_name("DefeatPalette", 260).map(str::to_owned),
        victory_art,
        defeat_art,
        score_victory_music: read_music(section, id, "ScoreVictoryMusic", "ScoreVictoryTheme"),
        score_defeat_music: read_music(section, id, "ScoreDefeatMusic", "ScoreDefeatTheme"),
        score_victory_palette: section
            .read_name("ScoreVictoryPalette", 260)
            .map(str::to_owned),
        score_defeat_palette: section
            .read_name("ScoreDefeatPalette", 260)
            .map(str::to_owned),
        score_victory_art,
        score_defeat_art,
    }
}

/// One `*Music=` value, or the `*Theme=` spelling this key had before the
/// rename. The old spelling sat next to `*Art=` and read like a picture, so it
/// still answers — a rules set written that way keeps working — but the log
/// names the new key once per section.
fn read_music(section: &IniSection, id: &str, key: &str, legacy: &str) -> Option<String> {
    if let Some(value) = section.read_name(key, 128) {
        return Some(value.to_owned());
    }
    let value = section.read_name(legacy, 128)?;
    log::warn!("[{id}] {legacy}= names music, not a picture; the key is now {key}=");
    Some(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "\
[EndGameScreens]
0=AmericansEnd
1=AlliesEnd
2=EveryEnd

[AmericansEnd]
Country=Americans
VictoryMusic=US_WIN
VictoryArt=USWIN.SHP
VictoryPalette=USWIN.PAL

[AlliesEnd]
Country=British,French,Germans,Alliance
VictoryMusic=ALLY_WIN
DefeatMusic=ALLY_LOSE
DefeatArt=ALLYLOSE.SHP

[EveryEnd]
VictoryMusic=EVERY_WIN
";

    fn rules_catalog(text: &str) -> EndGameCatalog {
        let ini = IniFile::from_str(text);
        let ids = ini
            .section("EndGameScreens")
            .map(|section| {
                section
                    .registry_ids()
                    .into_iter()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        parse_end_game_screens(&ini, &ids)
    }

    /// The narrow entry beats the catch-all, and the catch-all still covers a
    /// country nothing else names: registration order is the whole precedence.
    #[test]
    fn the_first_matching_country_wins() {
        let catalog = rules_catalog(RULES);
        let victory = |country: &str| {
            catalog
                .presentation(country, EndGameKind::Victory)
                .and_then(|presentation| presentation.banner_music().map(str::to_owned))
        };
        assert_eq!(victory("Americans").as_deref(), Some("US_WIN"));
        assert_eq!(victory("British").as_deref(), Some("ALLY_WIN"));
        assert_eq!(victory("Russians").as_deref(), Some("EVERY_WIN"));
    }

    /// `Country=` is the `[Countries]` key in any case, exactly like `Owner=`.
    #[test]
    fn the_country_key_is_case_insensitive() {
        let catalog = rules_catalog(RULES);
        assert_eq!(
            catalog
                .presentation("AMERICANS", EndGameKind::Victory)
                .and_then(|presentation| presentation.banner_music().map(str::to_owned))
                .as_deref(),
            Some("US_WIN")
        );
    }

    /// The two outcomes resolve separately: an entry that only configures the
    /// win does not answer for a loss, so the catch-all below it still does.
    #[test]
    fn an_outcome_a_narrow_entry_does_not_configure_falls_through() {
        let with_loss = RULES.replace(
            "VictoryMusic=EVERY_WIN",
            "VictoryMusic=EVERY_WIN\nDefeatMusic=EVERY_LOSE",
        );
        let catalog = rules_catalog(&with_loss);
        // `AmericansEnd` says nothing about losing, so the catch-all answers.
        let defeat = catalog
            .presentation("Americans", EndGameKind::Defeat)
            .expect("the catch-all covers a loss");
        assert_eq!(defeat.banner_music(), Some("EVERY_LOSE"));
        assert!(defeat.banner_art().is_none());
        // `British` has its own loss entry, which wins whole.
        let allied = catalog
            .presentation("British", EndGameKind::Defeat)
            .expect("British has its own entry");
        assert_eq!(allied.banner_music(), Some("ALLY_LOSE"));
        assert_eq!(
            allied.banner_art(),
            Some(EndGameArt {
                shape: "ALLYLOSE.SHP".into(),
                palette: None,
            }),
            "a palette is optional: the side's own stays"
        );
        // With no defeat key anywhere, the retail page keeps its own theme.
        assert!(
            rules_catalog(RULES)
                .presentation("Americans", EndGameKind::Defeat)
                .is_none()
        );
    }

    /// A section that names no `Country=` covers everybody, and an empty
    /// registry leaves the whole feature inert.
    #[test]
    fn an_absent_country_list_covers_every_country() {
        let catalog = rules_catalog(
            "\
[EndGameScreens]
0=All
[All]
VictoryMusic=WIN
",
        );
        assert_eq!(catalog.len(), 1);
        assert!(
            catalog
                .presentation("Arabians", EndGameKind::Victory)
                .is_some()
        );
        assert!(rules_catalog("").is_empty());
        assert!(
            rules_catalog("")
                .presentation("Americans", EndGameKind::Victory)
                .is_none(),
            "no catalog leaves the retail page alone"
        );
    }

    /// `*` is the wildcard the personality sections also accept.
    #[test]
    fn a_star_country_list_is_no_restriction() {
        let catalog = rules_catalog(
            "\
[EndGameScreens]
0=All
[All]
Country=*
VictoryMusic=WIN
",
        );
        assert!(catalog.for_country("anything").is_some());
    }

    /// The pre-rename `*Theme=` spelling still names music, so an INI written
    /// against the first cut of this section keeps its themes; the new key wins
    /// when a section has both.
    #[test]
    fn the_old_theme_spelling_still_names_music() {
        let catalog = rules_catalog(
            "\
[EndGameScreens]
0=All
[All]
VictoryTheme=OLD_WIN
DefeatMusic=NEW_LOSE
DefeatTheme=OLD_LOSE
",
        );
        let victory = catalog
            .presentation("Americans", EndGameKind::Victory)
            .expect("the old key answers");
        assert_eq!(victory.banner_music(), Some("OLD_WIN"));
        let defeat = catalog
            .presentation("Americans", EndGameKind::Defeat)
            .expect("the new key answers");
        assert_eq!(
            defeat.banner_music(),
            Some("NEW_LOSE"),
            "DefeatMusic= outranks the old DefeatTheme= in the same section"
        );
    }

    /// The banner half and the score half never borrow from each other. A rules
    /// set that brought only a battlefield picture — which is what the RA3
    /// end-game cards are — must leave the score page on the retail side art
    /// and the native `SCORE` theme instead of showing that picture twice.
    #[test]
    fn the_score_page_does_not_borrow_the_banner_picture() {
        let catalog = rules_catalog(
            "\
[EndGameScreens]
0=All
[All]
VictoryMusic=BANNER_WIN
VictoryArt=ENDAWIN.SHP
VictoryPalette=ENDAWIN.PAL
",
        );
        let victory = catalog
            .presentation("Americans", EndGameKind::Victory)
            .expect("the banner half covers this country");
        assert_eq!(victory.banner_music(), Some("BANNER_WIN"));
        assert_eq!(
            victory.banner_art(),
            Some(EndGameArt {
                shape: "ENDAWIN.SHP".into(),
                palette: Some("ENDAWIN.PAL".into()),
            })
        );
        assert!(
            victory.score_music().is_none() && victory.score_art().is_none(),
            "an entry with only the banner half keeps the retail score page"
        );
    }

    /// An entry carrying both halves keeps them apart, so one picture can be a
    /// battlefield card and another the page behind the score columns.
    #[test]
    fn a_section_may_name_a_banner_and_a_score_page_at_once() {
        let catalog = rules_catalog(
            "\
[EndGameScreens]
0=All
[All]
VictoryArt=ENDAWIN.SHP
VictoryMusic=BANNER_WIN
ScoreVictoryArt=SCOREWIN.SHP
ScoreVictoryPalette=SCOREWIN.PAL
ScoreVictoryMusic=SCORE_WIN
",
        );
        let victory = catalog
            .presentation("Americans", EndGameKind::Victory)
            .expect("both halves cover this country");
        assert_eq!(
            victory.banner_art().map(|art| art.shape),
            Some("ENDAWIN.SHP".into())
        );
        assert_eq!(
            victory.score_art(),
            Some(EndGameArt {
                shape: "SCOREWIN.SHP".into(),
                palette: Some("SCOREWIN.PAL".into()),
            })
        );
        assert_eq!(victory.banner_music(), Some("BANNER_WIN"));
        assert_eq!(victory.score_music(), Some("SCORE_WIN"));
    }

    /// The `Score*Music=` keys take the same pre-rename `*Theme=` compassion
    /// the banner half has, so a page written against the old spelling answers.
    #[test]
    fn the_score_theme_reads_its_own_keys_only() {
        let catalog = rules_catalog(
            "\
[EndGameScreens]
0=All
[All]
ScoreVictoryTheme=OLD_SCORE_WIN
",
        );
        let victory = catalog
            .presentation("Americans", EndGameKind::Victory)
            .expect("the old score spelling answers");
        assert_eq!(victory.score_music(), Some("OLD_SCORE_WIN"));
        assert!(
            victory.banner_music().is_none(),
            "the score theme is not the banner theme"
        );
    }
}
