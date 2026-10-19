//! Hash → filename dictionary for MIX archive entries.
//!
//! MIX archives store only a hash of each filename, so listing an archive means
//! guessing names and hashing them forward. Three sources are merged: the XCC
//! global mix database (~24k names) when installed, a built-in list of names and
//! generated patterns, and every `Image=`/`Cameo=` value found in the retail INIs.
//!
//! Lookups are by `HashMap`, not a linear scan — `ls` resolves one name per entry
//! and archives run to hundreds of entries.
//!
//! ## Dependency rules
//! - Depends on `assets/` only.

use std::collections::HashMap;

use crate::asset_tools::report::NameDb;
use crate::assets::asset_manager::AssetManager;
use crate::assets::mix_hash::mix_hash;
use crate::assets::xcc_database::XccDatabase;

/// Entries whose real name is unknown but whose role has been identified by hand.
const HASH_ALIASES: &[(i32, &str)] = &[(0x7AEBAE6Bu32 as i32, "unknown_top_housing.shp")];

/// A resolved hash → name table plus which sources backed it.
pub struct NameDict {
    map: HashMap<i32, String>,
    db: NameDb,
}

impl NameDict {
    /// Build the best dictionary available, including INI-derived names.
    ///
    /// This is the expensive part of any listing verb (an XCC parse plus a text
    /// scan of `rulesmd.ini`/`artmd.ini`), so verbs that take a *name* — and can
    /// therefore hash forward — must not construct one.
    pub fn build(asset_manager: &AssetManager) -> Self {
        let (mut pairs, xcc_loaded) = build_best_dictionary();
        expand_dictionary_from_ini(&mut pairs, asset_manager);

        let mut map: HashMap<i32, String> =
            HashMap::with_capacity(pairs.len() + HASH_ALIASES.len());
        for (name, hash) in pairs {
            map.entry(hash).or_insert(name);
        }
        // Hand-identified aliases win over a dictionary collision.
        for (hash, name) in HASH_ALIASES {
            map.insert(*hash, (*name).to_string());
        }

        Self {
            map,
            db: if xcc_loaded {
                NameDb::XccIni
            } else {
                NameDb::IniOnly
            },
        }
    }

    /// Build without touching the retail INIs. Used where only the built-in and
    /// XCC names are needed and startup cost matters.
    pub fn build_offline() -> Self {
        let (pairs, xcc_loaded) = build_best_dictionary();
        let mut map: HashMap<i32, String> = HashMap::with_capacity(pairs.len());
        for (name, hash) in pairs {
            map.entry(hash).or_insert(name);
        }
        for (hash, name) in HASH_ALIASES {
            map.insert(*hash, (*name).to_string());
        }
        Self {
            map,
            db: if xcc_loaded {
                NameDb::XccIni
            } else {
                NameDb::IniOnly
            },
        }
    }

    pub fn lookup(&self, hash: i32) -> Option<&str> {
        self.map.get(&hash).map(String::as_str)
    }

    /// Resolve a hash to a display name, falling back to a hex placeholder.
    /// The bool is false for the placeholder so callers can distinguish
    /// "no dictionary entry" from a real name.
    pub fn resolve(&self, hash: i32) -> (String, bool) {
        match self.map.get(&hash) {
            Some(name) => (name.clone(), true),
            None => (format!("??? ({:#010X})", hash as u32), false),
        }
    }

    pub fn db(&self) -> NameDb {
        self.db
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Build the best available hash dictionary for reverse-looking up MIX entries.
///
/// Tries the XCC global mix database (~24,000 filenames) first and merges the
/// built-in list on top, because the built-in list includes generated patterns
/// (`tab00..tab19`, `side1..side9`) that XCC does not carry.
/// Returns `(dictionary, xcc_loaded)`.
pub fn build_best_dictionary() -> (Vec<(String, i32)>, bool) {
    match XccDatabase::load_from_disk() {
        Ok(xcc) => {
            log::info!("Using XCC global mix database ({} entries)", xcc.len());
            let mut dict = xcc.build_hash_dictionary();
            dict.extend(build_hash_dictionary());
            dict.sort_by_key(|(_, hash)| *hash);
            dict.dedup_by_key(|(_, hash)| *hash);
            log::info!("Combined dictionary: {} unique hashes", dict.len());
            (dict, true)
        }
        Err(err) => {
            log::info!(
                "XCC database not available ({}), using built-in dictionary",
                err
            );
            (build_hash_dictionary(), false)
        }
    }
}

/// Build the built-in dictionary of plausible RA2/YR filenames.
///
/// This is the fallback when the XCC database is not installed; it is always
/// merged in regardless, for the generated patterns XCC lacks.
/// Returns (filename, hash) pairs sorted by hash.
pub fn build_hash_dictionary() -> Vec<(String, i32)> {
    let mut names: Vec<String> = Vec::new();

    for n in [
        "radar.shp",
        "radary.shp",
        "side1.shp",
        "side2.shp",
        "side3.shp",
        "side2a.shp",
        "side2b.shp",
        "side3a.shp",
        "side3b.shp",
        "tabs.shp",
        "repair.shp",
        "sell.shp",
        "power.shp",
        "credits.shp",
        "clock.shp",
        "pipbrd.shp",
        "pips.shp",
        "pips2.shp",
        "place.shp",
        "sidebar.pal",
        "uibkgd.pal",
        "uibkgdy.pal",
        "radaryuri.pal",
        "cameo.pal",
        "cameomd.pal",
        "mousepal.pal",
        "anim.pal",
    ] {
        names.push(n.to_string());
    }

    for i in 0..20 {
        names.push(format!("tab{i:02}.shp"));
    }
    for i in 1..=9 {
        names.push(format!("side{i}.shp"));
        names.push(format!("side{i}a.shp"));
        names.push(format!("side{i}b.shp"));
    }

    for name in [
        "sidebar",
        "sidebarp",
        "sidebarmd",
        "chrome",
        "cameo",
        "cameomd",
        "unit",
        "unittem",
        "unitsno",
        "uniturb",
        "unitdes",
        "unitlun",
        "temperat",
        "snow",
        "urban",
        "desert",
        "lunar",
        "newurban",
        "isotem",
        "isosno",
        "isourb",
        "isodes",
        "isolun",
        "isonurb",
        "grftxt",
        "mousepal",
        "anim",
        "lib",
        "theater",
    ] {
        names.push(format!("{name}.pal"));
    }

    let sidebar_names = [
        "sidebar.shp",
        "sidebarbg.shp",
        "sidec.shp",
        "chromeframe.shp",
        "chrome.shp",
        "bttn.shp",
        "button.shp",
        "btn.shp",
        "repair2.shp",
        "sell2.shp",
        "power2.shp",
        "repairon.shp",
        "sellon.shp",
        "poweron.shp",
        "repairoff.shp",
        "selloff.shp",
        "poweroff.shp",
        "pgup.shp",
        "pgdn.shp",
        "up.shp",
        "down.shp",
        "hscroll.shp",
        "vscroll.shp",
        "scroll.shp",
        "scrollup.shp",
        "scrolldn.shp",
        "radarbg.shp",
        "radarfr.shp",
        "radarui.shp",
        "radarframe.shp",
        "radarlogo.shp",
        "strip.shp",
        "cameo.shp",
        "queue.shp",
        "ready.shp",
        "hold.shp",
        "onhold.shp",
        "paused.shp",
        "upgrade.shp",
        "upgrdarw.shp",
        "options.shp",
        "diplomcy.shp",
        "battle.shp",
        "mslogo.shp",
        "dialog.shp",
        "dialogs.shp",
        "menu.shp",
        "menubar.shp",
        "mfill.shp",
        "mbar.shp",
        "mbtn.shp",
        "grdylw.shp",
        "grred.shp",
        "grgrn.shp",
        "grwht.shp",
        "gryel.shp",
        "pbar.shp",
        "pbargrn.shp",
        "pbarred.shp",
        "hbar.shp",
        "hpbar.shp",
        "hpips.shp",
        "pwrbar.shp",
        "pwrbaron.shp",
        "pwrbaroff.shp",
        "tooltip.shp",
        "txtbg.shp",
        "text.shp",
        "version.shp",
        "logo.shp",
        "westwood.shp",
        "title.shp",
        "titlebar.shp",
        "mouse.shp",
        "cursor.shp",
        "pointer.shp",
        "preview.shp",
        "pview.shp",
        "eva.shp",
        "evabg.shp",
        "evabar.shp",
        "select.shp",
        "health.shp",
        "rank.shp",
        "vet.shp",
        "elite.shp",
        "spyplane.shp",
        "paradrop.shp",
        "nuke.shp",
        "lightning.shp",
        "chrono.shp",
        "iron.shp",
        "ironcurt.shp",
        "waypoint.shp",
        "beacon.shp",
        "guard.shp",
        "stop.shp",
        "deploy.shp",
    ];
    for n in sidebar_names {
        names.push(n.to_string());
    }

    let prefixes = [
        "side", "tab", "btn", "bttn", "ctrl", "knob", "bar", "strip", "slot", "cell", "cam",
        "icon", "pip", "tic", "clock", "radar", "pbar", "hbar", "grn", "red", "yel", "wht", "gry",
        "grdylw", "grred", "grgrn", "grwht",
    ];
    for prefix in prefixes {
        for i in 0..30 {
            names.push(format!("{prefix}{i:02}.shp"));
            names.push(format!("{prefix}{i}.shp"));
        }
    }

    for base in [
        "sidebar", "sidebarp", "radar", "side1", "side2", "side3", "tabs", "tab00", "tab01",
        "tab02", "tab03", "repair", "sell", "power", "credits", "strip", "cameo", "chrome",
    ] {
        names.push(format!("{base}md.shp"));
        names.push(format!("{base}.shp"));
        names.push(format!("{base}md.pal"));
        names.push(format!("{base}.pal"));
    }

    let misc = [
        "gafscrn",
        "gafscrnmd",
        "nafscrnmd",
        "nafscreen",
        "yafscrn",
        "yafscrnmd",
        "gascren",
        "nascren",
        "yascren",
        "gaside1",
        "gaside2",
        "gaside3",
        "naside1",
        "naside2",
        "naside3",
        "yaside1",
        "yaside2",
        "yaside3",
        "gatabs",
        "natabs",
        "yatabs",
        "garadar",
        "naradar",
        "yaradar",
        "sldbkgd",
        "sldbar",
        "sldbkg",
        "bkgnd",
        "bkgd",
        "background",
        "pwrup",
        "pwrdn",
        "pwrbar",
        "credbar",
        "credbg",
        "crednum",
        "mnubtns",
        "mnubtn",
        "menubtn",
        "optbtns",
        "optbtn",
        "frame",
        "framebg",
        "framefg",
    ];
    for n in misc {
        names.push(format!("{n}.shp"));
        names.push(format!("{n}.pal"));
    }

    let common_assets = [
        "gapowr", "gacnst", "garefn", "gawall", "gagate", "gapile", "gaweap", "gaairc", "gadept",
        "gatech", "garobo", "gaspysat", "gaorep", "napowr", "nacnst", "narefn", "nawall", "nagate",
        "nahand", "naweap", "nayard", "natech", "naflak", "nalasr", "naradr", "yapowr", "yacnst",
        "yarefn", "yawall", "yagate", "yabrck", "yaweap", "yayard", "yatech", "yagrnd", "amcv",
        "smcv", "pcv", "harv", "sref", "htnk", "mtnk", "ltnk", "fv", "ifv", "bfrt", "apoc", "deso",
        "dred", "aegis", "dest", "howi", "v3", "flak", "rhino", "grizzly", "mirage", "prism", "e1",
        "e2", "e3", "e4", "dog", "snipe", "spy", "engi", "chrono", "seal", "boris", "tanya",
        "ivan", "yuri", "init",
    ];
    for n in common_assets {
        names.push(format!("{n}.shp"));
        names.push(format!("{n}.vxl"));
        names.push(format!("{n}icon.shp"));
        names.push(format!("{n}uiicon.shp"));
    }

    let mut dict: Vec<(String, i32)> = names
        .into_iter()
        .map(|n| {
            let hash = mix_hash(&n);
            (n, hash)
        })
        .collect();
    dict.sort_by_key(|(_, hash)| *hash);
    dict.dedup_by_key(|(_, hash)| *hash);
    dict
}

/// Expand the hash dictionary with filenames extracted from the retail INIs.
///
/// Scans every `Image=`, `Cameo=` and `AltCameo=` value and adds the `.shp`,
/// `.vxl`, `.hva` and `icon.shp` variants. This identifies hundreds of otherwise
/// unknown archive entries.
pub fn expand_dictionary_from_ini(dict: &mut Vec<(String, i32)>, asset_manager: &AssetManager) {
    let mut extra_names: Vec<String> = Vec::new();

    for ini_name in ["rules.ini", "rulesmd.ini", "art.ini", "artmd.ini"] {
        let Some(data) = asset_manager.get(ini_name) else {
            continue;
        };
        let text = String::from_utf8_lossy(&data);
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(';') || trimmed.starts_with('[') || !trimmed.contains('=') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            let key_upper = key.trim().to_ascii_uppercase();
            let val = value.trim();
            if val.is_empty() {
                continue;
            }

            match key_upper.as_str() {
                "IMAGE" | "CAMEO" | "ALTCAMEO" => {
                    let lower = val.to_ascii_lowercase();
                    extra_names.push(format!("{lower}.shp"));
                    extra_names.push(format!("{lower}.vxl"));
                    extra_names.push(format!("{lower}icon.shp"));
                    extra_names.push(format!("{lower}.hva"));
                }
                _ => {}
            }
        }
    }

    if extra_names.is_empty() {
        return;
    }

    let new_entries: Vec<(String, i32)> = extra_names
        .into_iter()
        .map(|n| {
            let hash = mix_hash(&n);
            (n, hash)
        })
        .collect();

    dict.extend(new_entries);
    dict.sort_by_key(|(_, hash)| *hash);
    dict.dedup_by_key(|(_, hash)| *hash);
    log::info!(
        "Hash dictionary expanded to {} entries from INI",
        dict.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_dictionary_is_deduped_and_sorted_by_hash() {
        let dict = build_hash_dictionary();
        assert!(!dict.is_empty());
        for pair in dict.windows(2) {
            assert!(pair[0].1 < pair[1].1, "dictionary must be sorted, no dupes");
        }
    }

    #[test]
    fn built_in_dictionary_round_trips_a_known_name() {
        let dict = build_hash_dictionary();
        let target = mix_hash("sidebar.pal");
        assert!(
            dict.iter()
                .any(|(name, hash)| *hash == target && name == "sidebar.pal")
        );
    }

    #[test]
    fn unknown_hash_resolves_to_a_flagged_placeholder() {
        let dict = NameDict {
            map: HashMap::new(),
            db: NameDb::IniOnly,
        };
        let (name, identified) = dict.resolve(0x1234_5678);
        assert!(!identified);
        assert!(name.contains("0x12345678"), "got {name}");
    }

    #[test]
    fn hand_identified_alias_wins() {
        let dict = NameDict::build_offline();
        let (alias_hash, alias_name) = HASH_ALIASES[0];
        assert_eq!(dict.lookup(alias_hash), Some(alias_name));
    }
}
