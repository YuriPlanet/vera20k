//! Palette bindings read out of production code paths, with citations.
//!
//! `asset_tools::palette::infer` answers "which palette is plausible" with a
//! heuristic chain, and labels its answer `heuristic` because that is all it can
//! honestly claim. But for a large slice of retail art the engine does not guess
//! at all — a named loader pairs a named palette with a named asset, in code.
//! This module is that pairing, lifted verbatim, so a browser can say "this is
//! the palette production uses" instead of "this is the palette that looked
//! right".
//!
//! Every row was read from a live call site and carries the `file.rs:line` it
//! came from. Nothing is here on the strength of a filename looking like it
//! belongs — the whole reason to prefer this table over the heuristic is that
//! its rows are checkable. The full ledger is at the bottom of this file, and
//! anything that could not be pinned to a line is listed there as omitted, not
//! quietly guessed.
//!
//! ## Dependency rules
//! - Depends on nothing. Pure data plus string matching, so the table can be
//!   read by tests, tools, and reports without an `AssetManager`.

/// One palette binding read from a production code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionBinding {
    pub palette: &'static str,
    /// "standard" or "gamemd_ui" — must match what the cited call site uses.
    pub alpha_policy: &'static str,
    /// Human-readable rule, e.g. "sidebar chrome atlas for sidec01.mix".
    pub rule: &'static str,
    /// The code path this was read from, e.g. "render/sidebar_chrome.rs:296".
    pub site: &'static str,
}

/// `Palette::from_bytes` at the cited call site.
const STANDARD: &str = "standard";
/// `Palette::from_bytes_gamemd_ui` at the cited call site.
const GAMEMD_UI: &str = "gamemd_ui";

/// Which assets a rule claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetMatch {
    /// Exact lowercase filename, e.g. `"radary.shp"`.
    Name(&'static str),
    /// Lowercase extension without the dot, e.g. `"tem"`.
    Ext(&'static str),
    /// Any asset — only meaningful paired with a specific archive.
    Any,
}

/// Which source archive a rule claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveMatch {
    /// Leaf archive basename with no `.mix`, e.g. `"sidec02md"`.
    Leaf(&'static str),
    /// Any archive — the binding holds wherever the asset resolved from.
    Any,
}

impl AssetMatch {
    /// Specificity of a match, or `None` when this rule does not claim the asset.
    fn score(self, name: &str, ext: &str) -> Option<u8> {
        match self {
            Self::Name(want) => (want == name).then_some(2),
            Self::Ext(want) => (!ext.is_empty() && want == ext).then_some(1),
            Self::Any => Some(0),
        }
    }
}

impl ArchiveMatch {
    fn score(self, leaf: &str) -> Option<u8> {
        match self {
            Self::Leaf(want) => (want == leaf).then_some(1),
            Self::Any => Some(0),
        }
    }
}

/// A binding plus the keys that select it.
struct Rule {
    asset: AssetMatch,
    archive: ArchiveMatch,
    binding: ProductionBinding,
}

/// Emit the matcher table and the public documentation table from one source
/// list.
///
/// [`BINDINGS`] is public API and [`RULES`] is what the matcher walks; they must
/// never disagree about what was found in the engine. Keeping two hand-written
/// tables in sync is exactly the kind of bookkeeping that silently rots, so the
/// rows are written once and both tables are generated from them.
macro_rules! production_bindings {
    ($(
        $asset:expr, $archive:expr => $palette:expr, $alpha:expr, $rule:expr, $site:expr;
    )*) => {
        const RULES: &[Rule] = &[$(
            Rule {
                asset: $asset,
                archive: $archive,
                binding: ProductionBinding {
                    palette: $palette,
                    alpha_policy: $alpha,
                    rule: $rule,
                    site: $site,
                },
            },
        )*];

        /// Every verified binding, for documentation and tests.
        pub const BINDINGS: &[ProductionBinding] = &[$(
            ProductionBinding {
                palette: $palette,
                alpha_policy: $alpha,
                rule: $rule,
                site: $site,
            },
        )*];
    };
}

use ArchiveMatch::{Any as AnyArchive, Leaf};
use AssetMatch::{Any as AnyAsset, Ext, Name};

production_bindings! {
    // ---------------------------------------------------------------- sidebar
    // Theme atlases. `build_sidebar_chrome_set` names the archive and the
    // palette in the same call, and `resolve_theme_palette_with_source` prefers
    // the copy inside that archive — so the pairing is per-archive, not global.
    Name("radar.shp"), Leaf("sidec01")
        => "sidebar.pal", STANDARD,
           "Allied sidebar radar art from sidec01.mix",
           "render/sidebar_chrome.rs:296";
    Name("radar.shp"), Leaf("sidec02")
        => "sidebar.pal", STANDARD,
           "Soviet sidebar radar art from sidec02.mix",
           "render/sidebar_chrome.rs:306";
    Name("radary.shp"), Leaf("sidec02md")
        => "radaryuri.pal", STANDARD,
           "Yuri sidebar radar art from sidec02md.mix",
           "render/sidebar_chrome.rs:316";

    // Sidebar backgrounds are drawn with the theme palette (`tabs_palette` is a
    // clone of it), so the plain names take sidebar.pal and the `*y` names take
    // the Yuri palette.
    Name("bkgdlg.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Allied/Soviet large sidebar background",
           "render/sidebar_chrome.rs:602";
    Name("bkgdmd.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Allied/Soviet medium sidebar background",
           "render/sidebar_chrome.rs:603";
    Name("bkgdsm.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Allied/Soviet small sidebar background",
           "render/sidebar_chrome.rs:604";
    Name("bkgdlgy.shp"), AnyArchive
        => "radaryuri.pal", STANDARD,
           "Yuri large sidebar background",
           "render/sidebar_chrome.rs:318";
    Name("bkgdmdy.shp"), AnyArchive
        => "radaryuri.pal", STANDARD,
           "Yuri medium sidebar background",
           "render/sidebar_chrome.rs:318";
    Name("bkgdsmy.shp"), AnyArchive
        => "radaryuri.pal", STANDARD,
           "Yuri small sidebar background",
           "render/sidebar_chrome.rs:318";

    // tabs.shp and power.shp also take the theme palette, and unlike the
    // backgrounds they keep one name across all three themes — so the archive
    // is what decides.
    Name("tabs.shp"), Leaf("sidec01")
        => "sidebar.pal", STANDARD,
           "Allied sidebar tab strip, theme palette",
           "render/sidebar_chrome.rs:526";
    Name("tabs.shp"), Leaf("sidec02")
        => "sidebar.pal", STANDARD,
           "Soviet sidebar tab strip, theme palette",
           "render/sidebar_chrome.rs:526";
    Name("tabs.shp"), Leaf("sidec02md")
        => "radaryuri.pal", STANDARD,
           "Yuri sidebar tab strip, theme palette",
           "render/sidebar_chrome.rs:526";
    Name("power.shp"), Leaf("sidec01")
        => "sidebar.pal", STANDARD,
           "Allied power bar frame, theme palette",
           "render/sidebar_chrome.rs:587";
    Name("power.shp"), Leaf("sidec02")
        => "sidebar.pal", STANDARD,
           "Soviet power bar frame, theme palette",
           "render/sidebar_chrome.rs:587";
    Name("power.shp"), Leaf("sidec02md")
        => "radaryuri.pal", STANDARD,
           "Yuri power bar frame, theme palette",
           "render/sidebar_chrome.rs:587";

    // Generic sidebar gadgets. These are resolved through the side route and
    // decoded with SIDEBAR.PAL for every theme, including Yuri — so they must
    // outrank the archive-wide Yuri rule below.
    Name("side1.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar body panel, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:521";
    Name("side2.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar body panel, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:522";
    Name("side3.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar body panel, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:523";
    Name("tab00.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar tab button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:536";
    Name("tab01.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar tab button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:536";
    Name("tab02.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar tab button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:536";
    Name("tab03.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar tab button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:536";
    Name("repair.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar repair button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:552";
    Name("sell.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Sidebar sell button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:561";
    Name("r-up.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Build strip scroll-up button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:577";
    Name("r-dn.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Build strip scroll-down button, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:579";
    Name("powerp.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Power bar meter strip, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:592";
    Name("gclock2.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "Production progress clock, generic SIDEBAR.PAL",
           "render/sidebar_chrome.rs:641";

    // Archive-wide fallbacks. Every remaining piece in a theme archive is
    // decoded with that theme's palette by `collect_extra_entries`, so an
    // unrecognised sidebar sprite still has a production answer.
    AnyAsset, Leaf("sidec01")
        => "sidebar.pal", STANDARD,
           "Any remaining Allied sidebar piece, theme palette",
           "render/sidebar_chrome.rs:631";
    AnyAsset, Leaf("sidec02")
        => "sidebar.pal", STANDARD,
           "Any remaining Soviet sidebar piece, theme palette",
           "render/sidebar_chrome.rs:631";
    AnyAsset, Leaf("sidec02md")
        => "radaryuri.pal", STANDARD,
           "Any remaining Yuri sidebar piece, theme palette",
           "render/sidebar_chrome.rs:631";

    // ----------------------------------------------------------------- cameos
    // The cameo archives are mounted immediately before the cameo atlas is
    // built, and the atlas decodes every cameo SHP with one palette chosen from
    // an ordered list whose first entry is cameo.pal.
    AnyAsset, Leaf("cameo")
        => "cameo.pal", STANDARD,
           "Sidebar cameo art from cameo.mix",
           "app_init.rs:1259";
    AnyAsset, Leaf("cameomd")
        => "cameo.pal", STANDARD,
           "Sidebar cameo art from cameomd.mix",
           "app_init.rs:1259";

    // ---------------------------------------------------------------- cursors
    Name("mouse.sha"), AnyArchive
        => "mousepal.pal", STANDARD,
           "Software cursor sprite sheet",
           "render/cursor_atlas.rs:350";
    Name("mouse.shp"), AnyArchive
        => "mousepal.pal", STANDARD,
           "Software cursor sprite sheet, .shp spelling",
           "render/cursor_atlas.rs:354";

    // ------------------------------------------------------------ world anims
    Name("oregath.shp"), AnyArchive
        => "anim.pal", STANDARD,
           "Harvest overlay animation, effect palette",
           "render/sprite_atlas.rs:1343";

    // ------------------------------------------------------- selection pips
    // Named "the general game palette, NOT unittem.pal" at the call site; the
    // unittem.pal fallback only fires when palette.pal is missing.
    Name("pips.shp"), AnyArchive
        => "palette.pal", STANDARD,
           "Health and occupant pips",
           "render/selection_overlay.rs:701";
    Name("pips2.shp"), AnyArchive
        => "palette.pal", STANDARD,
           "Tiberium cargo pips",
           "render/selection_overlay.rs:999";
    Name("pipbrd.shp"), AnyArchive
        => "palette.pal", STANDARD,
           "Health bar backing plate",
           "render/selection_overlay.rs:1095";

    // ------------------------------------------------------- startup splash
    Name("glssmd.shp"), AnyArchive
        => "glsmd.pal", GAMEMD_UI,
           "Startup splash, 640-wide client",
           "app/frontend/startup_splash.rs:219";
    Name("glslmd.shp"), AnyArchive
        => "glsmd.pal", GAMEMD_UI,
           "Startup splash, wider than 640",
           "app/frontend/startup_splash.rs:219";

    // ---------------------------------------------------------- shell chrome
    // SHELL.PAL / SHELL2.PAL / the dedicated dialog palettes. The main-menu and
    // Skirmish builders agree on every asset listed here; the two that disagree
    // are omitted and recorded in the ledger.
    Name("sdtp.shp"), AnyArchive
        => "shell.pal", STANDARD,
           "Shell right-panel top cap",
           "render/main_menu_shell_chrome.rs:95";
    Name("sdbtm.shp"), AnyArchive
        => "shell.pal", STANDARD,
           "Shell right-panel bottom cap",
           "render/main_menu_shell_chrome.rs:96";
    Name("lwscrns.shp"), AnyArchive
        => "shell.pal", STANDARD,
           "Shell lower-side chrome, 640-wide",
           "render/main_menu_shell_chrome.rs:97";
    Name("lwscrnl.shp"), AnyArchive
        => "shell.pal", STANDARD,
           "Shell lower-side chrome, wide",
           "render/main_menu_shell_chrome.rs:98";
    Name("mnscrnl.shp"), AnyArchive
        => "shell.pal", STANDARD,
           "Shell parent background, wide",
           "render/main_menu_shell_chrome.rs:102";
    Name("sdmpbtn.shp"), AnyArchive
        => "shell.pal", STANDARD,
           "Skirmish shell multiplayer button",
           "render/skirmish_shell_chrome.rs:327";
    Name("startbut.shp"), AnyArchive
        => "shell.pal", STANDARD,
           "Skirmish start marker",
           "render/skirmish_shell_chrome.rs:390";
    Name("sdbtnbkgd.shp"), AnyArchive
        => "shell2.pal", STANDARD,
           "Shell right-panel tile",
           "render/main_menu_shell_chrome.rs:107";
    Name("sdbtnanm.shp"), AnyArchive
        => "sdbtnanm.pal", STANDARD,
           "Shell owner-draw button animation",
           "render/main_menu_shell_chrome.rs:76";
    Name("sidebttn.shp"), AnyArchive
        => "sidebar.pal", STANDARD,
           "In-game Options dialog buttons",
           "render/skirmish_shell_chrome.rs:273";
    Name("mnbttn.shp"), AnyArchive
        => "mainbttn.pal", STANDARD,
           "Validation modal button",
           "render/skirmish_shell_chrome.rs:293";
    Name("pudlgbgn.shp"), AnyArchive
        => "dialogn.pal", STANDARD,
           "Validation modal background",
           "render/skirmish_shell_chrome.rs:314";
    Name("mnscrnlcustomizebattle.shp"), AnyArchive
        => "mnscrnlcustomizebattle.pal", STANDARD,
           "Choose Map modal background",
           "render/skirmish_shell_chrome.rs:375";

    // -------------------------------------------------------- loading screen
    // One palette per country, from the loading-art manifest, decoded with the
    // non-alpha-baking conversion.
    Name("progbarm.shp"), AnyArchive
        => "mpls.pal", GAMEMD_UI,
           "Loading screen progress bar",
           "render/loading_screen_chrome.rs:296";
    Name("ls640yuri.shp"), AnyArchive
        => "mpyls.pal", GAMEMD_UI,
           "Loading background, Yuri, 640-wide",
           "render/loading_screen_chrome.rs:156";
    Name("ls800yuri.shp"), AnyArchive
        => "mpyls.pal", GAMEMD_UI,
           "Loading background, Yuri, 800-wide",
           "render/loading_screen_chrome.rs:156";
    Name("ls640obs.shp"), AnyArchive
        => "mplsobs.pal", GAMEMD_UI,
           "Loading background, Observer, 640-wide",
           "render/loading_screen_chrome.rs:157";
    Name("ls800obs.shp"), AnyArchive
        => "mplsobs.pal", GAMEMD_UI,
           "Loading background, Observer, 800-wide",
           "render/loading_screen_chrome.rs:157";
    Name("ls640ustates.shp"), AnyArchive
        => "mplsu.pal", GAMEMD_UI,
           "Loading background, Americans, 640-wide",
           "render/loading_screen_chrome.rs:158";
    Name("ls800ustates.shp"), AnyArchive
        => "mplsu.pal", GAMEMD_UI,
           "Loading background, Americans, 800-wide",
           "render/loading_screen_chrome.rs:158";
    Name("ls640russia.shp"), AnyArchive
        => "mplsr.pal", GAMEMD_UI,
           "Loading background, Russians, 640-wide",
           "render/loading_screen_chrome.rs:159";
    Name("ls800russia.shp"), AnyArchive
        => "mplsr.pal", GAMEMD_UI,
           "Loading background, Russians, 800-wide",
           "render/loading_screen_chrome.rs:159";
    Name("ls640libya.shp"), AnyArchive
        => "mplsl.pal", GAMEMD_UI,
           "Loading background, Africans, 640-wide",
           "render/loading_screen_chrome.rs:160";
    Name("ls800libya.shp"), AnyArchive
        => "mplsl.pal", GAMEMD_UI,
           "Loading background, Africans, 800-wide",
           "render/loading_screen_chrome.rs:160";
    Name("ls640korea.shp"), AnyArchive
        => "mplsk.pal", GAMEMD_UI,
           "Loading background, Alliance, 640-wide",
           "render/loading_screen_chrome.rs:161";
    Name("ls800korea.shp"), AnyArchive
        => "mplsk.pal", GAMEMD_UI,
           "Loading background, Alliance, 800-wide",
           "render/loading_screen_chrome.rs:161";
    Name("ls640iraq.shp"), AnyArchive
        => "mplsi.pal", GAMEMD_UI,
           "Loading background, Arabs, 640-wide",
           "render/loading_screen_chrome.rs:162";
    Name("ls800iraq.shp"), AnyArchive
        => "mplsi.pal", GAMEMD_UI,
           "Loading background, Arabs, 800-wide",
           "render/loading_screen_chrome.rs:162";
    Name("ls640germany.shp"), AnyArchive
        => "mplsg.pal", GAMEMD_UI,
           "Loading background, Germans, 640-wide",
           "render/loading_screen_chrome.rs:163";
    Name("ls800germany.shp"), AnyArchive
        => "mplsg.pal", GAMEMD_UI,
           "Loading background, Germans, 800-wide",
           "render/loading_screen_chrome.rs:163";
    Name("ls640france.shp"), AnyArchive
        => "mplsf.pal", GAMEMD_UI,
           "Loading background, French, 640-wide",
           "render/loading_screen_chrome.rs:164";
    Name("ls800france.shp"), AnyArchive
        => "mplsf.pal", GAMEMD_UI,
           "Loading background, French, 800-wide",
           "render/loading_screen_chrome.rs:164";
    Name("ls640cuba.shp"), AnyArchive
        => "mplsc.pal", GAMEMD_UI,
           "Loading background, Confederation, 640-wide",
           "render/loading_screen_chrome.rs:165";
    Name("ls800cuba.shp"), AnyArchive
        => "mplsc.pal", GAMEMD_UI,
           "Loading background, Confederation, 800-wide",
           "render/loading_screen_chrome.rs:165";
    Name("ls640ukingdom.shp"), AnyArchive
        => "mplsuk.pal", GAMEMD_UI,
           "Loading background, British, 640-wide",
           "render/loading_screen_chrome.rs:166";
    Name("ls800ukingdom.shp"), AnyArchive
        => "mplsuk.pal", GAMEMD_UI,
           "Loading background, British, 800-wide",
           "render/loading_screen_chrome.rs:166";

    // -------------------------------------------------------- theater tiles
    // A TMP's extension *is* the theater selector: theater load pairs each
    // theater with one exact iso palette, and the tile atlas decodes that
    // theater's tiles with it. Loaded with the non-alpha-baking conversion.
    Ext("tem"), AnyArchive
        => "isotem.pal", GAMEMD_UI,
           "Temperate terrain tiles",
           "map/theater.rs:78";
    Ext("sno"), AnyArchive
        => "isosno.pal", GAMEMD_UI,
           "Snow terrain tiles",
           "map/theater.rs:89";
    Ext("urb"), AnyArchive
        => "isourb.pal", GAMEMD_UI,
           "Urban terrain tiles",
           "map/theater.rs:106";
    Ext("lun"), AnyArchive
        => "isolun.pal", GAMEMD_UI,
           "Lunar terrain tiles",
           "map/theater.rs:117";
    Ext("des"), AnyArchive
        => "isodes.pal", GAMEMD_UI,
           "Desert terrain tiles",
           "map/theater.rs:128";
    Ext("ubn"), AnyArchive
        => "isoubn.pal", GAMEMD_UI,
           "New Urban terrain tiles",
           "map/theater.rs:139";
}

/// The binding production would use for this asset, if any path claims it.
///
/// `source_archive` is the archive chain the asset resolved from (e.g.
/// "ra2.mix -> sidec01.mix"); `asset_name` is the requested filename.
/// Returns None when no production path binds a palette to this asset — which
/// is a real answer, not a failure, and must not be papered over.
///
/// ## Precedence
///
/// Rules are scored `asset * 2 + archive`, where an exact filename scores 2, an
/// extension scores 1, and "any" scores 0 on the asset side; an exact leaf
/// archive scores 1 and "any" scores 0 on the archive side. Highest score wins,
/// and among equal scores the earlier row wins. In descending order:
///
/// 1. exact filename in an exact archive (`tabs.shp` in `sidec02md`)
/// 2. exact filename in any archive (`gclock2.shp`)
/// 3. extension in an exact archive
/// 4. extension in any archive (`*.tem`)
/// 5. any asset in an exact archive (an unrecognised piece in `sidec01`)
///
/// A filename claim always outranks an archive claim, because the archive rules
/// are catch-alls for the pieces the named rules do not cover.
pub fn binding_for(asset_name: &str, source_archive: &str) -> Option<&'static ProductionBinding> {
    let name = asset_name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }
    let ext = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    let archive_lower = source_archive.to_ascii_lowercase();
    let leaf = super::palette::archive_basename(&archive_lower);

    let mut best: Option<(u8, &'static Rule)> = None;
    for rule in RULES {
        let Some(asset_score) = rule.asset.score(&name, ext) else {
            continue;
        };
        let Some(archive_score) = rule.archive.score(leaf) else {
            continue;
        };
        let score = asset_score * 2 + archive_score;
        if best.is_none_or(|(best_score, _)| score > best_score) {
            best = Some((score, rule));
        }
    }
    best.map(|(_, rule)| &rule.binding)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette_for(asset: &str, archive: &str) -> Option<&'static str> {
        binding_for(asset, archive).map(|binding| binding.palette)
    }

    #[test]
    fn leaf_reduction_is_applied_before_matching() {
        // The same asset, reached through a nested chain and directly.
        assert_eq!(
            palette_for("radary.shp", "ra2md.mix -> sidec02md.mix"),
            Some("radaryuri.pal")
        );
        assert_eq!(
            palette_for("radary.shp", "SIDEC02MD.MIX"),
            Some("radaryuri.pal")
        );
    }

    #[test]
    fn exact_name_and_archive_beats_archive_wide_rule() {
        // tabs.shp is claimed by name in sidec02md AND by the archive-wide Yuri
        // rule; the named rule has to win or the tab strip gets the wrong theme.
        assert_eq!(
            palette_for("tabs.shp", "sidec02md.mix"),
            Some("radaryuri.pal")
        );
        assert_eq!(palette_for("tabs.shp", "sidec01.mix"), Some("sidebar.pal"));
    }

    #[test]
    fn name_in_any_archive_beats_archive_wide_rule() {
        // The generic gadgets keep SIDEBAR.PAL even inside the Yuri archive.
        assert_eq!(
            palette_for("gclock2.shp", "sidec02md.mix"),
            Some("sidebar.pal")
        );
        assert_eq!(
            palette_for("side1.shp", "sidec02md.mix"),
            Some("sidebar.pal")
        );
    }

    #[test]
    fn archive_wide_rule_catches_unnamed_pieces() {
        assert_eq!(
            palette_for("someunknownpiece.shp", "sidec02md.mix"),
            Some("radaryuri.pal")
        );
        assert_eq!(
            palette_for("someunknownpiece.shp", "sidec01.mix"),
            Some("sidebar.pal")
        );
    }

    #[test]
    fn extension_rule_claims_theater_tiles() {
        assert_eq!(
            palette_for("clat01.tem", "isotemmd.mix"),
            Some("isotem.pal")
        );
        assert_eq!(
            palette_for("clat01.sno", "isosnomd.mix"),
            Some("isosno.pal")
        );
        assert_eq!(
            palette_for("clat01.ubn", "isoubnmd.mix"),
            Some("isoubn.pal")
        );
    }

    #[test]
    fn unclaimed_assets_return_none() {
        // Unit art is theater-selected, so no name-plus-archive rule can answer
        // it; None is the correct result, not a fallback.
        assert_eq!(palette_for("harv.shp", "conquer.mix"), None);
        assert_eq!(palette_for("gi.shp", "ra2md.mix -> conqmd.mix"), None);
        // Conflicting call sites are deliberately absent from the table.
        assert_eq!(palette_for("mmpb.shp", "ra2md.mix"), None);
        assert_eq!(palette_for("mnscrns.shp", "ra2md.mix"), None);
        // Empty input is not a match.
        assert_eq!(palette_for("", "sidec01.mix"), None);
        assert_eq!(palette_for("   ", "sidec01.mix"), None);
    }

    #[test]
    fn every_binding_cites_a_code_line() {
        for binding in BINDINGS {
            let (path, line) = binding
                .site
                .rsplit_once(':')
                .unwrap_or_else(|| panic!("site {:?} has no ':line' suffix", binding.site));
            assert!(
                path.ends_with(".rs"),
                "site {:?} does not name a Rust file",
                binding.site
            );
            assert!(
                !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()),
                "site {:?} does not end in a line number",
                binding.site
            );
            assert!(
                !binding.rule.is_empty(),
                "binding for {} has no rule text",
                binding.palette
            );
        }
    }

    #[test]
    fn alpha_policy_is_only_ever_one_of_the_two_legal_strings() {
        for binding in BINDINGS {
            assert!(
                binding.alpha_policy == "standard" || binding.alpha_policy == "gamemd_ui",
                "binding {} has illegal alpha policy {:?}",
                binding.site,
                binding.alpha_policy
            );
        }
    }

    #[test]
    fn palette_names_are_lowercase_pal_files() {
        for binding in BINDINGS {
            assert!(
                binding.palette.ends_with(".pal"),
                "binding {} names {:?}, which is not a .pal",
                binding.site,
                binding.palette
            );
            assert_eq!(
                binding.palette,
                binding.palette.to_ascii_lowercase(),
                "palette names are stored lowercase for lookup"
            );
        }
    }

    #[test]
    fn public_table_and_matcher_table_agree() {
        assert_eq!(BINDINGS.len(), RULES.len());
        for (binding, rule) in BINDINGS.iter().zip(RULES) {
            assert_eq!(*binding, rule.binding);
        }
    }

    #[test]
    fn theater_tiles_use_the_non_alpha_baking_conversion() {
        // The theater loader takes the gamemd UI conversion for iso palettes;
        // getting this backwards renders terrain as an opaque black square.
        let tile = binding_for("clat01.tem", "isotemmd.mix").expect("temperate tile binding");
        assert_eq!(tile.alpha_policy, "gamemd_ui");
        let sidebar = binding_for("side1.shp", "sidec01.mix").expect("sidebar binding");
        assert_eq!(sidebar.alpha_policy, "standard");
    }
}

// ---------------------------------------------------------------------------
// CITATION LEDGER
//
// Every row above, with the call site it was read from. Line numbers are as of
// the read; if one drifts, re-read the function rather than trusting the row.
//
// SIDEBAR CHROME — src/render/sidebar_chrome.rs
//   :296  build_sidebar_chrome_set pairs sidec01.mix with "sidebar.pal"
//   :306  ...pairs sidec02.mix with "sidebar.pal"
//   :316  ...pairs sidec02md.mix with "radaryuri.pal"
//   :297/:307/:317  radar art name per theme: radar.shp, radar.shp, radary.shp
//   :298/:308/:318  background names per theme: bkgd{lg,md,sm}[y].shp
//   :452/:458  resolve_theme_palette_with_source uses Palette::from_bytes
//              (standard), preferring the copy inside the theme archive
//   :496/:498  generic_palette_name = "SIDEBAR.PAL", Palette::from_bytes
//   :506  tabs_palette = theme_palette.clone()
//   :521-523  side1/2/3.shp decoded with the generic SIDEBAR.PAL
//   :526  tabs.shp decoded with tabs_palette (= theme palette)
//   :536  tab0N.shp decoded with the generic SIDEBAR.PAL
//   :552/:561  repair.shp, sell.shp with the generic SIDEBAR.PAL
//   :577/:579  r-up.shp, r-dn.shp with the generic SIDEBAR.PAL
//   :587  power.shp with theme_palette
//   :592  powerp.shp with the generic SIDEBAR.PAL
//   :602-604  backgrounds with tabs_palette (= theme palette)
//   :631  collect_extra_entries decodes every remaining archive piece with the
//         theme/tabs palette — the basis for the archive-wide rows
//   :641  GCLOCK2.SHP with the generic SIDEBAR.PAL
//
// CAMEOS
//   src/app_init.rs:1259 (now app/loading/init.rs)  cameomd.mix / cameo.mix mounted "for sidebar cameo icons"
//   src/app_init.rs:1266 (now app/loading/init.rs)  build_sidebar_cameo_atlas called immediately after
//   src/app_init_helpers.rs:80 (now app/loading/init_helpers.rs)  palette order cameo.pal, cameomd.pal, mousepal.pal,
//         anim.pal, unittem.pal, unit.pal, temperat.pal — first present wins
//   src/app_init_helpers.rs:91 (same file)  Palette::from_bytes (standard)
//   Note: src/render/sidebar_cameo_atlas.rs:140 is export_debug_palette_sheet,
//         a RA2_DEBUG_CAMEO_PALETTES-gated comparison sheet, not a production
//         binding; it is not in the table.
//
// CURSORS — src/render/cursor_atlas.rs
//   :350  build_software_cursor loads "mousepal.pal" with Palette::from_bytes
//   :353-354/:359-360  sprite sheet is mouse.sha, falling back to mouse.shp
//
// WORLD ANIMATION — src/render/sprite_atlas.rs
//   :1343/:1350  oregath.shp harvest overlay decoded with anim.pal, from_bytes
//   :941  world effect SHPs also take anim.pal, but the asset set comes from
//         rules effect_type_ids, so it is not name-derivable — omitted
//
// SELECTION PIPS — src/render/selection_overlay.rs
//   :701/:703  pips.shp building pips: palette.pal ("the general game palette,
//              NOT unittem.pal"), unittem.pal only as a missing-file fallback
//   :810, :905  the unit and occupant pip atlases repeat the same pairing
//   :999/:1001  pips2.shp tiberium cargo pips
//   :1095/:1097  pipbrd.shp health bar backing
//
// STARTUP SPLASH — src/app/frontend/startup_splash.rs
//   :32-34  SMALL_SPLASH_SHP = GLSS.SHP, LARGE_SPLASH_SHP = GLSL.SHP,
//           SPLASH_PALETTE = GLS.PAL — the Red Alert 2 art, chosen deliberately
//           over the Yuri's Revenge `*MD` variants
//   :219  Palette::from_bytes_gamemd_ui
//
// SHELL CHROME
//   src/render/main_menu_shell_chrome.rs:76   SDBTNANM.SHP → SDBTNANM.PAL
//   src/render/main_menu_shell_chrome.rs:95   SDTP.SHP → SHELL.PAL
//   src/render/main_menu_shell_chrome.rs:96   SDBTM.SHP → SHELL.PAL
//   src/render/main_menu_shell_chrome.rs:97   LWSCRNS.SHP → SHELL.PAL
//   src/render/main_menu_shell_chrome.rs:98   LWSCRNL.SHP → SHELL.PAL
//   src/render/main_menu_shell_chrome.rs:102  MNSCRNL.SHP → SHELL.PAL
//   src/render/main_menu_shell_chrome.rs:107  SDBTNBKGD.SHP → SHELL2.PAL
//   src/render/main_menu_shell_chrome.rs:144  load_named_palette uses from_bytes
//   src/render/skirmish_shell_chrome.rs:216/237/327/352/390  SDTP, SDBTM,
//           SDMPBTN, MNSCRNL, STARTBUT → SHELL.PAL (agrees with the main menu)
//   src/render/skirmish_shell_chrome.rs:230  SDBTNBKGD.SHP → SHELL2.PAL
//   src/render/skirmish_shell_chrome.rs:273  SIDEBTTN.SHP → SIDEBAR.PAL
//   src/render/skirmish_shell_chrome.rs:293  MNBTTN.SHP → MAINBTTN.PAL
//   src/render/skirmish_shell_chrome.rs:314  PUDLGBGN.SHP → DIALOGN.PAL
//   src/render/skirmish_shell_chrome.rs:375  MnScrnLCustomizeBattle.shp →
//           MnScrnLCustomizeBattle.PAL (palette loaded at :544)
//   src/render/skirmish_shell_chrome.rs:567  load_named_palette uses from_bytes
//
// LOADING SCREEN — src/render/loading_screen_chrome.rs
//   :156-166  LoadingArtVariant::manifest maps each country token to its exact
//             palette: yuri→MPYLS, obs→MPLSOBS, ustates→MPLSU, russia→MPLSR,
//             libya→MPLSL, korea→MPLSK, iraq→MPLSI, germany→MPLSG,
//             france→MPLSF, cuba→MPLSC, ukingdom→MPLSUK
//   :186  background_asset = format!("ls{width_prefix}{country_token}.shp"),
//         prefix 640 or 800 (:93-96)
//   :283  background decoded with the manifest palette
//   :296  PROGBARM.SHP decoded with a ramp-remapped MPLS.PAL (:284)
//   :477  load_named_ui_palette uses Palette::from_bytes_gamemd_ui
//
// THEATER TILES — src/map/theater.rs
//   :77-80    TEMPERATE: extension "tem", iso isotem.pal, unit unittem.pal,
//             theater temperat.pal
//   :88-91    SNOW: "sno", isosno.pal, unitsno.pal, snow.pal
//   :105-108  URBAN: "urb", isourb.pal, uniturb.pal, urban.pal
//   :116-119  LUNAR: "lun", isolun.pal, unitlun.pal, lunar.pal
//   :127-130  DESERT: "des", isodes.pal, unitdes.pal, desert.pal
//   :138-141  NEWURBAN: "ubn", isoubn.pal, unitubn.pal, urbann.pal
//   :818-827  iso / unit / theater palettes loaded by exact name
//   :1107     load_exact_palette uses Palette::from_bytes_gamemd_ui
//   src/app_init.rs:709 (now app/loading/init.rs)  the tile atlas is built with td.iso_palette and
//             td.extension, so a `.{ext}` tile is an iso-palette asset
//
// ---------------------------------------------------------------------------
// OMITTED — real production pairings that this table deliberately does not make
//
// mmpb.shp — two production paths, two palettes: the loading screen decodes it
//   with MPLS.PAL under from_bytes_gamemd_ui
//   (src/render/loading_screen_chrome.rs:284, :382, :302) while the Skirmish
//   shell decodes it with SHELL.PAL under from_bytes
//   (src/render/skirmish_shell_chrome.rs:387-392). Name and archive cannot tell
//   the two apart, so the matcher returns None rather than pick one.
//
// MNSCRNS.SHP — same problem: SHELL.PAL at
//   src/render/main_menu_shell_chrome.rs:101 and
//   src/render/skirmish_shell_chrome.rs:395, but
//   MnScrnLCustomizeBattle.PAL in the 640-wide Choose Map dialog.
//   MNSCRNL.SHP has no such split and is in the table.
//
// Unit, infantry, building and overlay art — the palette is theater-selected
//   (src/map/theater.rs:818-827: unit{tem,sno,urb,lun,des,ubn}.pal and the
//   theater ore palette), and the theater is not derivable from a filename or
//   an archive name. conquer.mix art therefore returns None. The heuristic
//   chain is the right tool there, and it says so.
//
// Theater-archive SHPs (trees and terrain objects in tem.mix / sno.mix / ...)
//   — the theater archives hold both TMP tiles and SHPs, and no call site was
//   found that states which palette the SHPs take. Only the `.{ext}` TMP rows
//   are claimed.
//
// World effect SHPs — anim.pal at src/render/sprite_atlas.rs:941, but the asset
//   set is rules-driven (effect_type_ids), not name-derivable. Only the one
//   hardcoded name, oregath.shp, is in the table.
//
// ---------------------------------------------------------------------------
// DRIFT NOTED AGAINST THE HEURISTIC — for whoever reconciles the two
//
// asset_tools::palette::ARCHIVE_PALETTE_MAP proposes "isonurb.pal" for a
//   NEWURBAN archive. src/map/theater.rs:139 names isoubn.pal. The heuristic
//   name does not appear anywhere in the engine.
//
// asset_tools::palette::GAMEMD_UI_PALETTES omits unit*.pal, but
//   src/map/theater.rs:820 loads the unit palette through
//   load_exact_palette, which is from_bytes_gamemd_ui (:1107). The fallback
//   paths that run only when no theater loaded (src/app_init_helpers.rs:632, now app/loading/init_helpers.rs,
//   src/app_sim_tick.rs:1707, src/app_skirmish.rs:2897 - now under app/) use from_bytes. Since a
//   skirmish always loads a theater, the production policy for unit*.pal is
//   gamemd_ui.
