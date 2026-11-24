//! Routes `asset render` to the renderer for the asset's actual format.
//!
//! Kept separate from the format renderers so each of them stays a leaf that
//! knows only its own format, and so adding one is a single match arm here.
//!
//! Dispatch sniffs rather than trusting the filename: retail archives hold
//! entries whose extension and content disagree, and the sniffer is what every
//! other verb already reports.
//!
//! ## Dependency rules
//! - Depends on sibling `asset_tools` modules only.

use crate::asset_tools::identify;
use crate::asset_tools::locate::locate;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::report::{ErrorReport, RenderReport};
use crate::asset_tools::verb_render::RenderOptions;
use crate::asset_tools::{render_still, render_tmp, render_vxl, verb_render};
use crate::assets::asset_manager::AssetManager;
use crate::rules::art_data::ArtRegistry;

/// Formats this verb can draw. Anything else is answered with a pointer to
/// `asset info`, which reports every parsed format without rendering.
const RENDERABLE: &[&str] = &["shp", "tmp", "pcx", "pal", "vxl"];

pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    art_registry: &ArtRegistry,
    name: &str,
    opts: &RenderOptions,
) -> Result<RenderReport, ErrorReport> {
    let Some(resolved) = locate(asset_manager, name) else {
        return Err(ErrorReport {
            error: format!("asset not found: {name}"),
            hint: Some(format!(
                "run `asset find {name}` to see whether any archive holds it"
            )),
        });
    };

    let format = identify::identify(resolved.bytes).format;
    match format {
        "shp" => verb_render::run(asset_manager, dict, art_registry, name, opts),
        "tmp" => render_tmp::run(asset_manager, dict, art_registry, name, opts),
        "pcx" => render_still::run_pcx(asset_manager, name, opts),
        "pal" => render_still::run_pal(asset_manager, name, opts),
        "vxl" => render_vxl::run(asset_manager, dict, art_registry, name, opts),
        other => Err(ErrorReport {
            error: format!("`asset render` cannot draw {name}: it is {other}"),
            hint: Some(format!(
                "renderable formats are {}; use `asset info {name}` for everything else",
                RENDERABLE.join(", ")
            )),
        }),
    }
}
