//! The observer strip's pictures. `SidebarClass::LoadSHPs @ 0x006A5840`
//! loads a background for each side and a flag for each country index
//! (`0x006A5AB7..0x006A5BDB`) through the game's file lookup (`0x004A38D0`),
//! not a side's archives: retail keeps them once, in LOCALMD.MIX. The strip
//! draws them through a converter of OBSERVER.PAL (`[0x0087F6D0]`, made at
//! `0x006A5A48..0x006A5AA5`), the Yuri flag through YRII.PAL's
//! (`[0x00B0FC08]`, `0x006AA1E8`); `0x0072FC34..0x0072FC59` loads both.

use super::*;

/// Flag files by country index + 3, as the jump table at `0x006AA5C4`
/// picks them: the observer, Random, no flag, then the ten retail countries.
const FLAG_NAMES: [Option<&str>; 13] = [
    Some("OBSI.SHP"),
    Some("RANI.SHP"),
    None,
    Some("USAI.SHP"),
    Some("JAPI.SHP"),
    Some("FRAI.SHP"),
    Some("GERI.SHP"),
    Some("GBRI.SHP"),
    Some("DJBI.SHP"),
    Some("ARBI.SHP"),
    Some("LATI.SHP"),
    Some("RUSI.SHP"),
    Some("YRII.SHP"),
];

/// The country index whose flag draws through YRII.PAL (`0x006AA1E3`).
const YURI_FLAG: i32 = 9;

/// The observer strip's backgrounds and flags.
pub struct ObserverArt<T> {
    /// OBSALLI, OBSSOVI and OBSYURI (`[0xB0B490]`, `[0xB0B494]`,
    /// `[0xB0B498]`), by the country's side.
    pub backgrounds: [Option<T>; 3],
    /// The flags by country index + 3 ([`FLAG_NAMES`]).
    pub flags: [Option<T>; 13],
}

impl<T> ObserverArt<T> {
    pub fn entries(&self) -> impl Iterator<Item = &T> {
        self.backgrounds.iter().chain(&self.flags).flatten()
    }

    pub fn map<U>(&self, mut f: impl FnMut(&T) -> U) -> ObserverArt<U> {
        ObserverArt {
            backgrounds: self.backgrounds.each_ref().map(|e| e.as_ref().map(&mut f)),
            flags: self.flags.each_ref().map(|e| e.as_ref().map(&mut f)),
        }
    }

    /// The background of a country on `side` (`HouseType+0xBC`); none for
    /// any side but the three (`0x006AA0CA..0x006AA0EF`).
    pub fn background(&self, side: i32) -> Option<&T> {
        self.backgrounds.get(usize::try_from(side).ok()?)?.as_ref()
    }

    /// The flag of country index `country` (`HouseType+0xB8`); none outside
    /// -3..=9 (`0x006AA16D`).
    pub fn flag(&self, country: i32) -> Option<&T> {
        self.flags
            .get(usize::try_from(country.checked_add(3)?).ok()?)?
            .as_ref()
    }
}

pub(super) fn load(
    assets: &AssetManager,
    mix: &MixArchive,
    mix_name: &str,
    ares: bool,
) -> ObserverArt<RenderedChromeEntry> {
    let palette =
        |name| resolve_theme_palette_with_source(assets, mix, mix_name, name, ares).map(|(p, _)| p);
    let (observer, yuri) = (palette("OBSERVER.PAL"), palette("YRII.PAL"));
    let picture = |name: &str, palette: Option<&Palette>| {
        let shp = ShpFile::from_bytes(assets.get_ref(name)?).ok()?;
        render_shp(&shp, palette?, 0)
    };
    ObserverArt {
        backgrounds: ["OBSALLI.SHP", "OBSSOVI.SHP", "OBSYURI.SHP"]
            .map(|name| picture(name, observer.as_ref())),
        flags: std::array::from_fn(|slot| {
            let palette = if slot as i32 - 3 == YURI_FLAG {
                yuri.as_ref()
            } else {
                observer.as_ref()
            };
            FLAG_NAMES[slot].and_then(|name| picture(name, palette))
        }),
    }
}
