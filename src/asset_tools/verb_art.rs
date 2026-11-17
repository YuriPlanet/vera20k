//! `asset art-for <TYPE>` — from a rules type id to the files that actually back it.
//!
//! "Which SHP does this unit use, and does it exist" is a question that costs a
//! throwaway test every time it is asked, because the answer is three chained
//! conventions: rules `Image=` → art.ini `Image=`/`Cameo=` → a per-theater
//! filename rule that rewrites the second character of the name. Getting one
//! link wrong produces a filename that looks entirely plausible and resolves to
//! nothing.
//!
//! So this verb does not stop at the convention. Every candidate it proposes is
//! looked up, and the report says which archive held it and what the bytes
//! actually are. **The candidate list alone is a guess; the resolution is the
//! answer** — a caller reading only `shp_candidates` has learned nothing it
//! could not have spelled itself.
//!
//! Cameo candidates mirror the production sidebar resolver in
//! `render::sidebar_cameo_atlas` rather than inventing a second convention. Its
//! `.PCX` spelling is deliberately *not* proposed: nothing in this tree resolves
//! a cameo from a PCX, and an unconfirmed convention in the candidate list reads
//! as a documented one.
//!
//! ## Dependency rules
//! - Depends on `assets/` (archive resolution), `rules/` (the art registry and
//!   its filename-convention helpers), `map::theater` (the production theater
//!   table), and the sibling `identify` / `locate` / `report` modules.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`. The cameo
//!   ordering is *mirrored* from the render-side resolver, not imported.

use crate::asset_tools::identify;
use crate::asset_tools::report::{AnimSlot, ArtCandidate, ArtForReport, ErrorReport};
use crate::assets::asset_manager::AssetManager;
use crate::map::theater::theater_names_and_extensions;
use crate::rules::art_data::{
    ArtRegistry, effect_shp_candidates, object_shp_candidates, voxel_asset_names,
};

/// Theater assumed when the caller does not pass `--theater`. Temperate is the
/// theater every stock skirmish map uses unless it says otherwise.
const DEFAULT_THEATER: &str = "tem";

/// The stock placeholder a type without loadable cameo art shows
/// (`TechnoTypeClass::ReadINI`, `0x00715A4F`), and the name its savegame
/// reload reads when no `Cameo=` is declared (`0x00716C86`).
const PLACEHOLDER_CAMEO: &str = "XXICON";

/// Emitted when the registry carries no art.ini at all. Without this the report
/// looks like a successful lookup that happened to find nothing declared.
const EMPTY_REGISTRY_WARNING: &str = "art registry is empty — no art.ini was loaded, so Image=, Cameo= and Palette= were never \
     read. Every id below is the type id itself and every candidate is a bare filename \
     convention, not a lookup. Run with a valid retail install so artmd.ini resolves.";

/// Emitted when the caller could not supply the rules `Image=` value. A type
/// whose rules section overrides `Image=` resolves to different art than this.
const NO_RULES_IMAGE_WARNING: &str = "no rules Image= was supplied, so the effective image id derives from the type id alone. Pass \
     `--image <VALUE>` when rules declares one — types such as BFRT point their art at another \
     type's section.";

/// Options for one `asset art-for` invocation.
#[derive(Debug, Clone)]
pub struct ArtForOptions {
    /// Theater extension, e.g. `tem`, or its long name (`temperate`), in any
    /// case; the production theater table in `map::theater` is the accepted set.
    pub theater: String,
}

impl Default for ArtForOptions {
    fn default() -> Self {
        Self {
            theater: DEFAULT_THEATER.to_string(),
        }
    }
}

/// Resolve a rules type id to the art files that back it, and check each one.
///
/// `rules_image` is the `Image=` value from rules, or an empty string when the
/// caller does not know it. Errors are values, not panics: an unknown theater or
/// an empty type id yields an [`ErrorReport`] whose hint names the flag that
/// fixes it.
pub fn run(
    asset_manager: &AssetManager,
    art_registry: &ArtRegistry,
    rules_image: &str,
    type_id: &str,
    opts: &ArtForOptions,
) -> Result<ArtForReport, ErrorReport> {
    let type_id = validate_type_id(type_id)?;
    let (theater_ext, theater_name) =
        resolve_theater(&opts.theater).ok_or_else(|| unknown_theater_error(&opts.theater))?;

    // The resolver is injected so the assembly below is exercisable without a
    // mounted retail install — see the tests at the foot of this file.
    let mut resolve = |name: &str| resolve_candidate(asset_manager, name);

    Ok(assemble(
        art_registry,
        type_id,
        rules_image.trim(),
        theater_ext,
        theater_name,
        asset_manager.ships_ares(),
        &mut resolve,
    ))
}

/// Reject a blank type id before it turns into candidates like `.SHP`.
fn validate_type_id(type_id: &str) -> Result<&str, ErrorReport> {
    let trimmed = type_id.trim();
    if trimmed.is_empty() {
        return Err(ErrorReport {
            error: "art-for needs a rules type id".to_string(),
            hint: Some(
                "pass the section name from rules, e.g. `asset art-for GAPOWR` or \
                 `asset art-for HTNK`"
                    .to_string(),
            ),
        });
    }
    Ok(trimmed)
}

/// Map a `--theater` value to `(theater_ext, theater_name)` through the
/// production theater table.
///
/// Both strings are what the `rules::art_data` candidate builders expect, and
/// they are not interchangeable: `theater_ext` becomes the file extension
/// (`GAPOWR.TEM`), while `theater_name` selects the `NewTheater` substitution
/// letter and only matches on the long uppercase spelling. Passing `"tem"` where
/// `"TEMPERATE"` belongs silently disables the substitution and yields candidates
/// that never resolve.
///
/// Matches the extension or the long name, case-insensitively, so a caller that
/// types `TEMPERATE` is not punished for it.
fn resolve_theater(requested: &str) -> Option<(&'static str, &'static str)> {
    let wanted = requested.trim();
    theater_names_and_extensions()
        .find(|(name, ext)| wanted.eq_ignore_ascii_case(ext) || wanted.eq_ignore_ascii_case(name))
        .map(|(name, ext)| (ext, name))
}

/// The rejection path for an unknown theater, listing every valid short name.
fn unknown_theater_error(requested: &str) -> ErrorReport {
    let valid: Vec<&str> = theater_names_and_extensions().map(|(_, ext)| ext).collect();
    ErrorReport {
        error: format!("unknown theater \"{}\"", requested.trim()),
        hint: Some(format!(
            "--theater must be one of: {}. Theater art only exists in the theater it was \
             authored for, so the theater changes which files resolve.",
            valid.join(", ")
        )),
    }
}

/// Candidate filenames for one type, before any archive resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateNames {
    shp: Vec<String>,
    cameo: Vec<String>,
    voxel: Vec<String>,
}

/// Build the report from resolved ids and a candidate resolver.
///
/// The resolver returns the candidate row plus an optional warning, so a hit in
/// a catalogued-but-unreachable archive is reported rather than passed off as a
/// normal lookup. `ares` is whether the game is an Ares game, whose sidebar
/// draws a declared PCX cameo first.
fn assemble(
    art_registry: &ArtRegistry,
    type_id: &str,
    rules_image: &str,
    theater_ext: &str,
    theater_name: &str,
    ares: bool,
    resolve: &mut dyn FnMut(&str) -> (ArtCandidate, Option<String>),
) -> ArtForReport {
    // Retail ids are uppercase everywhere in art.ini; normalise once so the
    // report and its warnings never disagree about how the type is spelled.
    let type_upper = type_id.to_ascii_uppercase();
    let effective_image_id = art_registry.resolve_effective_image_id(type_id, rules_image);
    let cameo_id = art_registry
        .resolve_declared_cameo_id(type_id, rules_image)
        .unwrap_or_else(|| PLACEHOLDER_CAMEO.to_owned());
    let declared_palette = art_registry.resolve_declared_palette_id(type_id, rules_image);

    let mut names = candidate_names(
        art_registry,
        &effective_image_id,
        &cameo_id,
        theater_ext,
        theater_name,
    );
    if ares
        && let [Some(filename), _] = art_registry.resolve_declared_pcx_cameos(type_id, rules_image)
    {
        names.cameo.insert(0, filename.to_owned());
    }

    let mut warnings: Vec<String> = Vec::new();
    // Registry state first: it changes how every id below must be read.
    if art_registry.is_empty() {
        warnings.push(EMPTY_REGISTRY_WARNING.to_string());
    }
    if rules_image.is_empty() {
        warnings.push(NO_RULES_IMAGE_WARNING.to_string());
    }

    let shp_candidates = resolve_all(&names.shp, resolve, &mut warnings);
    let cameo_candidates = resolve_all(&names.cameo, resolve, &mut warnings);
    let voxel_candidates = resolve_all(&names.voxel, resolve, &mut warnings);

    let any_hit = [&shp_candidates, &cameo_candidates, &voxel_candidates]
        .iter()
        .any(|rows| rows.iter().any(|row| row.exists));
    if !any_hit {
        warnings.push(format!(
            "no candidate resolved for {type_upper} in theater {theater_name} — every name below \
             is a filename convention, not a lookup. Try `asset find <NAME>` on one of them, or \
             another `--theater`."
        ));
    }

    // Building art: footprint, ground pad, and the overlay anim slots. These
    // answer "what should be playing while it builds / runs / burns", which the
    // object SHP alone cannot.
    let entry = art_registry.get(&effective_image_id.to_ascii_uppercase());
    let foundation = entry.and_then(|e| e.foundation).map(|id| {
        crate::rules::foundation::FOUNDATION_TABLE[usize::from(id)]
            .name
            .to_owned()
    });
    let bib_shape = entry.and_then(|e| e.bib_shape.clone()).map(|bib| {
        let (row, note) = resolve(&format!("{}.SHP", bib.to_ascii_uppercase()));
        if let Some(note) = note {
            warnings.push(note);
        }
        row
    });

    let building_anims = entry
        .map(|e| {
            e.building_anims
                .iter()
                .map(|anim| {
                    // Each slot is its own art section with its own image id and
                    // theater convention, so it resolves independently.
                    let slot_upper = anim.anim_type.to_ascii_uppercase();
                    let slot_image = art_registry.resolve_effective_image_id(&slot_upper, "");
                    let names = effect_shp_candidates(
                        crate::rules::art_data::EffectImageKind::Animation,
                        Some(art_registry),
                        &slot_upper,
                        &slot_image,
                        theater_ext,
                        theater_name,
                    );
                    AnimSlot {
                        slot: slot_upper,
                        kind: format!("{:?}", anim.kind),
                        is_primary: anim.is_primary,
                        offset: [anim.x, anim.y],
                        rate: anim.rate,
                        damaged_variant: anim.damaged_variant.as_ref().map(|v| v.anim_type.clone()),
                        garrisoned_variant: anim
                            .garrisoned_variant
                            .as_ref()
                            .map(|v| v.anim_type.clone()),
                        candidates: resolve_all(&names, resolve, &mut warnings),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // An anim slot naming art that does not resolve is a real missing-overlay
    // bug, and it is invisible unless called out separately from the object SHP.
    let dead_slots: Vec<&str> = building_anims
        .iter()
        .filter(|slot| !slot.candidates.iter().any(|row| row.exists))
        .map(|slot| slot.slot.as_str())
        .collect();
    if !dead_slots.is_empty() {
        warnings.push(format!(
            "{} building anim slot(s) resolve to no art in theater {theater_name}: {}",
            dead_slots.len(),
            dead_slots.join(", ")
        ));
    }

    ArtForReport {
        type_id: type_upper,
        theater: theater_name.to_string(),
        declared_image: rules_image.to_string(),
        effective_image_id,
        declared_palette,
        cameo_id,
        foundation,
        bib_shape,
        shp_candidates,
        cameo_candidates,
        voxel_candidates,
        building_anims,
        warnings,
    }
}

/// Assemble every candidate filename this type could be backed by.
fn candidate_names(
    art_registry: &ArtRegistry,
    effective_image_id: &str,
    cameo_id: &str,
    theater_ext: &str,
    theater_name: &str,
) -> CandidateNames {
    let (vxl, hva) = voxel_asset_names(effective_image_id);
    CandidateNames {
        shp: object_shp_candidates(
            Some(art_registry),
            effective_image_id,
            theater_ext,
            theater_name,
        ),
        cameo: cameo_candidate_names(cameo_id),
        voxel: vec![vxl, hva],
    }
}

/// Cameo filenames in the order the production sidebar resolver tries them:
/// the declared cameo, then the stock placeholder, so the first row with
/// `exists: true` is the file the sidebar would show.
fn cameo_candidate_names(cameo_id: &str) -> Vec<String> {
    let cameo = cameo_id.to_ascii_uppercase();
    let mut names = vec![format!("{cameo}.SHP")];
    if cameo != PLACEHOLDER_CAMEO {
        names.push(format!("{PLACEHOLDER_CAMEO}.SHP"));
    }
    names
}

/// Resolve a batch of candidate names, folding their warnings in without
/// repeating the same archive caveat once per candidate.
fn resolve_all(
    names: &[String],
    resolve: &mut dyn FnMut(&str) -> (ArtCandidate, Option<String>),
    warnings: &mut Vec<String>,
) -> Vec<ArtCandidate> {
    names
        .iter()
        .map(|name| {
            let (candidate, warning) = resolve(name.as_str());
            if let Some(warning) = warning
                && !warnings.contains(&warning)
            {
                warnings.push(warning);
            }
            candidate
        })
        .collect()
}

/// Look one candidate up and describe what was found.
///
/// Resolution goes through `locate`, not `AssetManager::resolve_ref`: a large
/// share of the mounted archives are catalogued but absent from the name-lookup
/// index, and a browser that reported those files as missing would be wrong
/// about exactly the assets most often under investigation.
fn resolve_candidate(asset_manager: &AssetManager, name: &str) -> (ArtCandidate, Option<String>) {
    let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, name) else {
        return (
            ArtCandidate {
                name: name.to_string(),
                exists: false,
                source_archive: None,
                detail: None,
            },
            None,
        );
    };

    let identified = identify::identify(resolved.bytes);
    let warning = resolved.catalog_warning();
    (
        ArtCandidate {
            name: name.to_string(),
            exists: true,
            source_archive: Some(resolved.source_archive),
            detail: Some(identified.detail),
        },
        warning,
    )
}

#[cfg(all(test, tests_asset_tools))]
mod tests {
    use super::*;
    use crate::map::theater::theater_extension;

    /// A resolver that reports every name as missing.
    fn miss() -> impl FnMut(&str) -> (ArtCandidate, Option<String>) {
        |name: &str| {
            (
                ArtCandidate {
                    name: name.to_string(),
                    exists: false,
                    source_archive: None,
                    detail: None,
                },
                None,
            )
        }
    }

    /// A resolver that finds exactly `hit`, in a catalogued archive.
    fn hit_only(hit: &'static str) -> impl FnMut(&str) -> (ArtCandidate, Option<String>) {
        move |name: &str| {
            if name == hit {
                (
                    ArtCandidate {
                        name: name.to_string(),
                        exists: true,
                        source_archive: Some("ra2.mix -> sidec02.mix".to_string()),
                        detail: Some("SHP(TS) 60x48, 1 frames".to_string()),
                    },
                    Some("catalogued archive".to_string()),
                )
            } else {
                (
                    ArtCandidate {
                        name: name.to_string(),
                        exists: false,
                        source_archive: None,
                        detail: None,
                    },
                    None,
                )
            }
        }
    }

    #[test]
    fn theater_lookup_accepts_the_long_spelling_and_ignores_case() {
        for (ext, name) in [
            ("tem", "TEMPERATE"),
            ("sno", "SNOW"),
            ("urb", "URBAN"),
            ("lun", "LUNAR"),
            ("des", "DESERT"),
            ("ubn", "NEWURBAN"),
        ] {
            assert_eq!(resolve_theater(ext), Some((ext, name)));
            assert_eq!(resolve_theater(name), Some((ext, name)));
            assert_eq!(theater_extension(name), Some(ext));
        }
        assert_eq!(resolve_theater("newurban"), Some(("ubn", "NEWURBAN")));
        assert_eq!(resolve_theater("  SNO  "), Some(("sno", "SNOW")));
    }

    #[test]
    fn unknown_theater_is_rejected_and_names_every_valid_short_name() {
        assert_eq!(resolve_theater("mars"), None);
        // The empty string is not a silent default — the caller passed something.
        assert_eq!(resolve_theater(""), None);

        let err = unknown_theater_error("mars");
        assert!(err.error.contains("mars"), "{}", err.error);
        let hint = err.hint.expect("the rejection must name the valid set");
        for short in ["tem", "sno", "urb", "lun", "des", "ubn"] {
            assert!(hint.contains(short), "hint omits {short}: {hint}");
        }
    }

    #[test]
    fn a_blank_type_id_is_rejected_with_an_example() {
        assert_eq!(validate_type_id("  GAPOWR ").unwrap(), "GAPOWR");
        let err = validate_type_id("   ").expect_err("blank must not become \".SHP\"");
        assert!(err.hint.expect("a hint").contains("art-for GAPOWR"));
    }

    #[test]
    fn shp_candidates_follow_the_new_theater_convention_of_the_chosen_theater() {
        let registry = ArtRegistry::empty();
        let names = candidate_names(&registry, "GAPOWR", "GAPOWR", "tem", "TEMPERATE");
        assert_eq!(
            names.shp,
            vec![
                "GTPOWR.SHP",
                "GTPOWR.TEM",
                "GGPOWR.SHP",
                "GGPOWR.TEM",
                "GAPOWR.SHP",
                "GAPOWR.TEM",
            ]
        );

        // Snow substitutes to the same letter the name already has, so the
        // theater-specific pair collapses into the plain one.
        let snow = candidate_names(&registry, "GAPOWR", "GAPOWR", "sno", "SNOW");
        assert_eq!(
            snow.shp,
            vec!["GAPOWR.SHP", "GAPOWR.SNO", "GGPOWR.SHP", "GGPOWR.SNO"]
        );
    }

    #[test]
    fn a_type_without_the_new_theater_prefix_gets_the_plain_pair_only() {
        let registry = ArtRegistry::empty();
        let names = candidate_names(&registry, "E1", "E1", "tem", "TEMPERATE");
        assert_eq!(names.shp, vec!["E1.SHP", "E1.TEM"]);
    }

    #[test]
    fn report_puts_the_authored_pcx_ahead_of_the_placeholder() {
        let registry = ArtRegistry::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[IMAGE]\nImage=BODY\nCameoPCX=Normal.PCX\n[BODY]\nCameoPCX=wrong.pcx\n",
        ));
        let report = assemble(
            &registry,
            "TYPE",
            "IMAGE",
            "tem",
            "TEMPERATE",
            true,
            &mut miss(),
        );
        assert_eq!(report.cameo_candidates[0].name, "normal.pcx");
        assert_eq!(report.cameo_candidates[1].name, "XXICON.SHP");
        let stock = assemble(
            &registry,
            "TYPE",
            "IMAGE",
            "tem",
            "TEMPERATE",
            false,
            &mut miss(),
        );
        assert_eq!(stock.cameo_candidates[0].name, "XXICON.SHP");
    }

    #[test]
    fn cameo_candidates_mirror_the_production_sidebar_resolver() {
        // The declared cameo, then the stock placeholder; no image-id guesses.
        assert_eq!(
            cameo_candidate_names("CIVICON"),
            vec!["CIVICON.SHP", "XXICON.SHP"]
        );
        assert_eq!(cameo_candidate_names("XXICON"), vec!["XXICON.SHP"]);
    }

    #[test]
    fn report_carries_the_resolved_ids_and_every_candidate_list() {
        let registry = ArtRegistry::empty();
        let report = assemble(
            &registry,
            "gapowr",
            "GAPOWR",
            "tem",
            "TEMPERATE",
            false,
            &mut miss(),
        );

        assert_eq!(report.type_id, "GAPOWR");
        assert_eq!(report.theater, "TEMPERATE");
        assert_eq!(report.declared_image, "GAPOWR");
        assert_eq!(report.effective_image_id, "GAPOWR");
        assert_eq!(report.cameo_id, "XXICON");
        assert_eq!(report.declared_palette, None);
        assert_eq!(report.shp_candidates.len(), 6);
        assert_eq!(report.voxel_candidates.len(), 2);
        assert!(!report.cameo_candidates.is_empty());
        assert!(report.shp_candidates.iter().all(|row| !row.exists));
    }

    #[test]
    fn a_missing_rules_image_is_warned_about() {
        let registry = ArtRegistry::empty();
        let report = assemble(
            &registry,
            "GAPOWR",
            "",
            "tem",
            "TEMPERATE",
            false,
            &mut miss(),
        );
        assert!(
            report.warnings.iter().any(|w| w == NO_RULES_IMAGE_WARNING),
            "{:?}",
            report.warnings
        );
        // The type id still drives resolution, so the answer is usable.
        assert_eq!(report.effective_image_id, "GAPOWR");
        assert_eq!(report.declared_image, "");
    }

    #[test]
    fn nothing_resolving_is_warned_about_with_the_verbs_that_help() {
        let registry = ArtRegistry::empty();
        let report = assemble(
            &registry,
            "GAPOWR",
            "GAPOWR",
            "lun",
            "LUNAR",
            false,
            &mut miss(),
        );
        let warning = report
            .warnings
            .last()
            .expect("a report where nothing resolved must say so");
        assert!(warning.contains("asset find"), "{warning}");
        assert!(warning.contains("--theater"), "{warning}");
        assert!(warning.contains("LUNAR"), "{warning}");
    }

    #[test]
    fn one_hit_suppresses_the_nothing_resolved_warning_and_dedupes_the_archive_caveat() {
        let registry = ArtRegistry::empty();
        let report = assemble(
            &registry,
            "GAPOWR",
            "GAPOWR",
            "tem",
            "TEMPERATE",
            false,
            &mut hit_only("GAPOWR.SHP"),
        );

        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("no candidate resolved")),
            "{:?}",
            report.warnings
        );
        // "GAPOWR.SHP" is proposed by both the SHP and the cameo list; the
        // caveat it carries must appear once, not twice.
        let caveats = report
            .warnings
            .iter()
            .filter(|w| w.as_str() == "catalogued archive")
            .count();
        assert_eq!(caveats, 1, "{:?}", report.warnings);

        let hit = report
            .shp_candidates
            .iter()
            .find(|row| row.name == "GAPOWR.SHP")
            .expect("the hit is in the SHP list");
        assert!(hit.exists);
        assert_eq!(
            hit.source_archive.as_deref(),
            Some("ra2.mix -> sidec02.mix")
        );
        assert!(hit.detail.as_deref().unwrap_or_default().contains("SHP"));
    }
}
