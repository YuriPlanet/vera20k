//! `asset palette-for <NAME>` — which palette an asset should be drawn with, and why.
//!
//! This verb is what makes `render` trustworthy. Palette inference is a
//! heuristic chain, and a wrong palette produces art that looks entirely valid,
//! so the chain reports its reasoning rather than just a name.
//!
//! ## Dependency rules
//! - Depends on `assets/`, `rules/`, and sibling `asset_tools` modules only.

use crate::asset_tools::names::NameDict;
use crate::asset_tools::palette;
use crate::asset_tools::report::{ErrorReport, PaletteReport};
use crate::assets::asset_manager::AssetManager;
use crate::rules::art_data::ArtRegistry;

/// Stated on every report. A palette that merely renders is not evidence that
/// it is the palette the game uses.
const CAVEAT: &str = "Palette choice is inferred, not read from the asset. Only `art.ini Palette=` \
                      and an explicit override are declared; everything else is a heuristic. A \
                      plausible-looking image is not evidence the palette is correct.";

#[derive(Default)]
pub struct PaletteForOptions {
    pub palette_override: Option<String>,
}

pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    art_registry: &ArtRegistry,
    name: &str,
    opts: &PaletteForOptions,
) -> Result<PaletteReport, ErrorReport> {
    let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, name) else {
        return Err(ErrorReport {
            error: format!("asset not found: {name}"),
            hint: Some(format!(
                "run `asset find {name}` — it also reports catalogued archives that name lookup \
                 cannot reach"
            )),
        });
    };
    let source_archive = resolved.source_archive.clone();

    let inference = palette::infer(
        asset_manager,
        dict,
        art_registry,
        Some(name),
        &source_archive,
        opts.palette_override.as_deref(),
    );

    Ok(PaletteReport {
        asset: name.to_string(),
        source_archive,
        chosen: inference.chosen.as_ref().map(|load| load.choice()),
        candidates: inference
            .candidates
            .iter()
            .map(|row| row.to_report())
            .collect(),
        caveat: CAVEAT.to_string(),
    })
}
