//! Palette inference for browsing assets, with its reasoning exposed.
//!
//! SHP frames are palette indices; nothing in the file says which palette. The
//! chain here — override, `art.ini Palette=`, source-archive family, filename
//! heuristics, a fallback list, then any 768-byte entry in the source archive —
//! is the only one in the tree.
//!
//! Every candidate carries **why** it was proposed.
//! A wrong palette produces art that looks entirely valid, so a caller that
//! cannot see the reasoning cannot tell a good render from a plausible one.
//!
//! ## Dependency rules
//! - Depends on `assets/` and `rules/`. No render, sim, or app types.

use crate::asset_tools::names::NameDict;
use crate::asset_tools::report::{PaletteCandidate, PaletteChoice};
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;

/// A loaded palette plus the provenance a caller needs to judge it.
///
/// Every palette is decoded by `Palette::from_bytes`, the one conversion gamemd
/// applies to all of its palettes, so the choice is only which file.
pub struct PaletteLoad {
    pub palette: Palette,
    pub name: String,
    pub reason: String,
    /// `production` when an engine code path binds this palette to this asset
    /// class, `declared` when art.ini named it or the caller overrode it,
    /// `heuristic` otherwise.
    pub confidence: &'static str,
    /// For `production`, the code path the binding was read from.
    pub production_site: Option<&'static str>,
}

impl PaletteLoad {
    pub fn choice(&self) -> PaletteChoice {
        PaletteChoice {
            name: self.name.clone(),
            reason: self.reason.clone(),
            confidence: self.confidence.to_string(),
            production_site: self.production_site.map(str::to_string),
        }
    }
}

/// One proposed palette and why.
pub struct PaletteCandidateRow {
    pub name: String,
    pub reason: String,
    pub exists: bool,
}

impl PaletteCandidateRow {
    pub fn to_report(&self) -> PaletteCandidate {
        PaletteCandidate {
            name: self.name.clone(),
            reason: self.reason.clone(),
            exists: self.exists,
        }
    }
}

/// The full inference result: what won and everything that was considered.
pub struct PaletteInference {
    pub chosen: Option<PaletteLoad>,
    pub candidates: Vec<PaletteCandidateRow>,
}

/// Source archive family → most likely palette.
///
/// Assets inside a retail archive are authored for one palette family, so the
/// archive name is the strongest automatic signal available.
const ARCHIVE_PALETTE_MAP: &[(&str, &[&str])] = &[
    ("isotem", &["isotem.pal"]),
    ("isotemp", &["isotem.pal"]),
    ("isosno", &["isosno.pal"]),
    ("isosnow", &["isosno.pal"]),
    ("isourb", &["isourb.pal"]),
    ("isodes", &["isodes.pal"]),
    ("isolun", &["isolun.pal"]),
    ("isoubn", &["isoubn.pal"]),
    ("tem", &["temperat.pal", "unittem.pal"]),
    ("temperat", &["temperat.pal", "unittem.pal"]),
    ("sno", &["snow.pal", "unitsno.pal"]),
    ("snow", &["snow.pal", "unitsno.pal"]),
    ("urb", &["urban.pal", "uniturb.pal"]),
    ("urban", &["urban.pal", "uniturb.pal"]),
    ("des", &["desert.pal", "unitdes.pal"]),
    ("desert", &["desert.pal", "unitdes.pal"]),
    ("lun", &["lunar.pal", "unitlun.pal"]),
    ("lunar", &["lunar.pal", "unitlun.pal"]),
    ("sidec01", &["sidebar.pal"]),
    ("sidec02", &["sidebar.pal"]),
    ("sidec02md", &["sidebar.pal"]),
    ("cameo", &["cameo.pal"]),
    ("cameomd", &["cameomd.pal", "cameo.pal"]),
    ("conquer", &["unittem.pal"]),
    ("conqmd", &["unittem.pal"]),
    ("local", &["unittem.pal"]),
    ("localmd", &["unittem.pal"]),
    ("cache", &["unittem.pal"]),
    ("cachemd", &["unittem.pal"]),
    ("mapsmd03", &["unittem.pal"]),
    ("maps01", &["unittem.pal"]),
    ("maps02", &["unittem.pal"]),
    ("expandmd01", &["unittem.pal"]),
    ("ra2", &["unittem.pal"]),
    ("ra2md", &["unittem.pal"]),
];

/// Ordered fallback list tried when nothing more specific matched.
const FALLBACK_CHAIN: &[&str] = &[
    "unittem.pal",
    "temperat.pal",
    "isotem.pal",
    "uniturb.pal",
    "unitsno.pal",
    "unitdes.pal",
    "unitlun.pal",
    "anim.pal",
    "sidebar.pal",
    "cameo.pal",
];

/// Build the art registry used for `Palette=` declarations.
///
/// Active YR loads a standalone `ARTMD.INI` rather than merging the RA2 base
/// beneath it, so `artmd.ini` is preferred outright and `art.ini` is only a
/// fallback for an RA2-only install.
pub fn load_art_registry(asset_manager: &AssetManager) -> ArtRegistry {
    for ini_name in ["artmd.ini", "art.ini"] {
        let Some(data) = asset_manager.get(ini_name) else {
            continue;
        };
        let Ok(ini) = IniFile::from_bytes(&data) else {
            continue;
        };
        return ArtRegistry::from_ini(&ini);
    }
    ArtRegistry::empty()
}

/// Infer the palette for one asset, recording every candidate considered.
pub fn infer(
    asset_manager: &AssetManager,
    dict: &NameDict,
    art_registry: &ArtRegistry,
    asset_name: Option<&str>,
    source_archive: &str,
    override_name: Option<&str>,
) -> PaletteInference {
    // (name, reason, confidence, production site)
    type Proposal = (String, String, &'static str, Option<&'static str>);
    let mut proposals: Vec<Proposal> = Vec::new();
    let mut propose =
        |name: String, reason: String, confidence: &'static str, site: Option<&'static str>| {
            if !proposals.iter().any(|(existing, ..)| *existing == name) {
                proposals.push((name, reason, confidence, site));
            }
        };

    // 1. Caller override wins outright.
    if let Some(override_name) = override_name {
        let trimmed = override_name.trim();
        if !trimmed.is_empty() {
            propose(
                trimmed.to_ascii_lowercase(),
                "override".to_string(),
                "declared",
                None,
            );
        }
    }

    // 2. A production code path that binds a palette to this asset class beats
    //    every heuristic below it, and carries the citation to prove it. When no
    //    path claims the asset this yields nothing and the chain proceeds — that
    //    is the honest outcome, not a failure.
    if let Some(binding) =
        crate::asset_tools::palette_production::binding_for(&lower_name(asset_name), source_archive)
    {
        propose(
            binding.palette.to_ascii_lowercase(),
            format!("production:{}", binding.rule),
            "production",
            Some(binding.site),
        );
    }

    let lower = asset_name.unwrap_or_default().to_ascii_lowercase();
    let source_lower = source_archive.to_ascii_lowercase();

    // 2. art.ini `Palette=` declaration for this image id.
    let base_name = lower
        .strip_suffix(".shp")
        .or_else(|| lower.strip_suffix(".vxl"))
        .or_else(|| lower.strip_suffix(".hva"))
        .unwrap_or(&lower);
    if let Some(pal_base) = art_registry.resolve_declared_palette_id(base_name, "") {
        propose(
            format!("{pal_base}.pal"),
            "art.ini Palette=".to_string(),
            "declared",
            None,
        );
    }

    // 3. Source archive family.
    let archive_base = archive_basename(&source_lower);
    for &(pattern, palettes) in ARCHIVE_PALETTE_MAP {
        if archive_base == pattern {
            for pal in palettes {
                propose(
                    (*pal).to_string(),
                    format!("archive-map:{pattern}"),
                    "heuristic",
                    None,
                );
            }
            break;
        }
    }

    // 4. Filename heuristics.
    if lower == "radary.shp" {
        propose(
            "radaryuri.pal".to_string(),
            "filename-heuristic:yuri-radar".to_string(),
            "heuristic",
            None,
        );
    }
    if lower.contains("mouse") || lower.contains("cursor") || lower.contains("pointer") {
        propose(
            "mousepal.pal".to_string(),
            "filename-heuristic:cursor".to_string(),
            "heuristic",
            None,
        );
    }
    if lower.contains("icon") || source_lower.contains("cameo") {
        propose(
            "cameomd.pal".to_string(),
            "filename-heuristic:cameo".to_string(),
            "heuristic",
            None,
        );
        propose(
            "cameo.pal".to_string(),
            "filename-heuristic:cameo".to_string(),
            "heuristic",
            None,
        );
    }
    if is_sidebar_ui(&lower, &source_lower) {
        for pal in ["sidebar.pal", "uibkgd.pal", "uibkgdy.pal"] {
            propose(
                pal.to_string(),
                "filename-heuristic:sidebar-ui".to_string(),
                "heuristic",
                None,
            );
        }
    }
    if lower.contains("anim") || source_lower.contains("anim") {
        propose(
            "anim.pal".to_string(),
            "filename-heuristic:anim".to_string(),
            "heuristic",
            None,
        );
    }
    if lower.starts_with("ls") || lower.starts_with("load") || lower.contains("loading") {
        for pal in ["ls800bkg.pal", "ls640bkg.pal", "load.pal"] {
            propose(
                pal.to_string(),
                "filename-heuristic:loading-screen".to_string(),
                "heuristic",
                None,
            );
        }
    }

    // 5. Generic fallback chain.
    for pal in FALLBACK_CHAIN {
        propose(
            (*pal).to_string(),
            "fallback-chain".to_string(),
            "heuristic",
            None,
        );
    }

    // Resolve in order; the first that both exists and parses wins.
    let mut candidates: Vec<PaletteCandidateRow> = Vec::with_capacity(proposals.len());
    let mut chosen: Option<PaletteLoad> = None;

    for (name, reason, confidence, site) in proposals {
        // Two lookup rules, in order, and both matter:
        //
        // 1. Prefer the palette that sits in the *same archive as the art*. The
        //    theme archives each carry their own `sidebar.pal`, and the
        //    production chrome builder pairs `sidec01.mix` with the `sidebar.pal`
        //    inside it. Taking the first `sidebar.pal` found anywhere pairs one
        //    theme's art with another theme's colours.
        // 2. Fall back to a global sweep that includes catalogued archives. The
        //    sidebar chrome and its palette both sit outside the name-lookup
        //    index, so production lookup alone reports `sidebar.pal` as missing
        //    and drops through to a unit palette — believable, and wrong.
        let bytes = palette_bytes_in(asset_manager, source_archive, &name).or_else(|| {
            crate::asset_tools::locate::locate(asset_manager, &name)
                .map(|resolved| resolved.bytes.to_vec())
        });
        let exists = bytes.is_some();
        if chosen.is_none()
            && let Some(bytes) = bytes
            && let Some(load) = load_palette(&name, &bytes, &reason, confidence, site)
        {
            chosen = Some(load);
        }
        candidates.push(PaletteCandidateRow {
            name,
            reason,
            exists,
        });
    }

    // Last resort: any 768-byte entry in the archive that holds the asset. This
    // is a guess of last resort and is labelled as such.
    if chosen.is_none()
        && let Some(archive) = asset_manager.archive(source_archive)
    {
        for entry in archive.entries() {
            if entry.size as usize != PAL_BYTES {
                continue;
            }
            let Some(bytes) = archive.get_by_id(entry.id) else {
                continue;
            };
            let name = dict
                .lookup(entry.id)
                .map(str::to_string)
                .unwrap_or_else(|| format!("archive-pal {:#010X}", entry.id as u32));
            let reason = "last-resort-768-byte-scan".to_string();
            if let Some(load) = load_palette(&name, bytes, &reason, "heuristic", None) {
                candidates.push(PaletteCandidateRow {
                    name: load.name.clone(),
                    reason: reason.clone(),
                    exists: true,
                });
                chosen = Some(load);
                break;
            }
        }
    }

    PaletteInference { chosen, candidates }
}

/// A .pal is exactly 256 RGB triplets.
const PAL_BYTES: usize = 768;

/// Look for a palette inside one specific archive, so themed art keeps its own
/// theme's colours instead of whichever same-named palette is found first.
fn palette_bytes_in(
    asset_manager: &AssetManager,
    source_archive: &str,
    palette_name: &str,
) -> Option<Vec<u8>> {
    let archive = asset_manager.archive(source_archive)?;
    archive
        .get_by_id(crate::assets::mix_hash::mix_hash(palette_name))
        .map(<[u8]>::to_vec)
}

fn load_palette(
    name: &str,
    bytes: &[u8],
    reason: &str,
    confidence: &'static str,
    site: Option<&'static str>,
) -> Option<PaletteLoad> {
    Some(PaletteLoad {
        palette: Palette::from_bytes(bytes).ok()?,
        name: name.to_string(),
        reason: reason.to_string(),
        confidence,
        production_site: site,
    })
}

/// Lowercased asset name, or an empty string when the caller had none.
fn lower_name(asset_name: Option<&str>) -> String {
    asset_name.unwrap_or_default().to_ascii_lowercase()
}

/// Reduce `ra2.mix -> sidec02.mix` to `sidec02`, so nested archives match the
/// family map instead of falling straight through to the fallback chain.
pub(super) fn archive_basename(source_lower: &str) -> &str {
    let last = source_lower
        .rsplit("->")
        .next()
        .unwrap_or(source_lower)
        .trim();
    last.strip_suffix(".mix").unwrap_or(last)
}

fn is_sidebar_ui(lower: &str, source_lower: &str) -> bool {
    const UI_SUBSTRINGS: &[&str] = &[
        "radar", "power", "repair", "sell", "clock", "credits", "dialog", "title", "button", "btn",
        "scroll", "tooltip", "menu", "sidebar",
    ];
    lower.starts_with("side")
        || lower.starts_with("tab")
        || UI_SUBSTRINGS.iter().any(|s| lower.contains(s))
        || source_lower.contains("sidec")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_archive_chain_reduces_to_its_leaf() {
        assert_eq!(archive_basename("ra2.mix -> sidec02.mix"), "sidec02");
        assert_eq!(archive_basename("cameomd.mix"), "cameomd");
        assert_eq!(archive_basename("loose:c:/x/foo.pal"), "loose:c:/x/foo.pal");
    }

    #[test]
    fn the_newurban_iso_palette_is_spelled_as_the_engine_spells_it() {
        // theater.rs:139 names it isoubn.pal. `isonurb.pal` was a fabricated
        // name carried in from the GUI browser's map and exists nowhere.
        assert!(
            ARCHIVE_PALETTE_MAP
                .iter()
                .all(|(_, palettes)| !palettes.contains(&"isonurb.pal"))
        );
    }

    #[test]
    fn sidebar_detection_covers_name_and_archive() {
        assert!(is_sidebar_ui("tab00.shp", "cameo.mix"));
        assert!(is_sidebar_ui("powerp.shp", "ra2.mix"));
        assert!(is_sidebar_ui("anything.shp", "ra2.mix -> sidec02.mix"));
        assert!(!is_sidebar_ui("harv.shp", "conquer.mix"));
    }
}
