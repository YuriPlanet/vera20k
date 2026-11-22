//! Discover classic RA2 resources or a renamed client resource family in a selected mod.
//!
//! This only proposes names. Startup still owns extension selection and uses
//! the ordinary asset/rules loaders. No executable or arbitrary MIX scan runs.

use std::collections::BTreeMap;
use std::path::Path;

use super::asset_manager::{AssetManager, retail_enumerated_file};
use crate::util::archive_names::ArchiveNames;
use crate::util::ini_roots::{IniProfile, IniRootConfig};

const MAX_FAMILIES: usize = 16;
const MAX_INI_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn discover(dir: &Path) -> Result<Option<(IniRootConfig, ArchiveNames)>, String> {
    let entries = crate::util::case_insensitive_path::native_listing(dir).map_err(|error| {
        format!(
            "Could not inspect mod archives in {}: {error}",
            dir.display()
        )
    })?;
    let mut families = BTreeMap::<String, BTreeMap<u8, String>>::new();
    for path in entries {
        if !retail_enumerated_file(&path) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let Some((suffix, number)) = expansion_family(&lower) else {
            continue;
        };
        // native_listing orders duplicate spellings by bytes, as the VFS's
        // loose-file catalog does. Keep its first spelling of each slot.
        families
            .entry(suffix.to_owned())
            .or_default()
            .entry(number)
            .or_insert_with(|| name.to_owned());
        if families.len() > MAX_FAMILIES {
            return Err(
                "Too many renamed archive families; declare the mod's files in mod.ini".into(),
            );
        }
    }

    let mut selected = None;
    for (suffix, files) in families {
        let Some(candidate) = inspect_family(dir, &suffix, &files)? else {
            continue;
        };
        if let Some((other, _)) = &selected {
            return Err(format!(
                "Mod has more than one complete renamed Rules/Art/AI family ({other}, {suffix}); declare the intended files in mod.ini"
            ));
        }
        selected = Some((suffix, candidate));
    }
    Ok(selected.map(|(_, resources)| resources))
}

/// An ordinary folder mod can supply the original RA2 files loose or in
/// EXPANDnn.MIX. Inspect only the mod: the installation itself contains
/// resources for both games. Mixed packages retain the existing YR choice.
pub(super) fn uses_classic_ra2(dir: &Path) -> Result<bool, String> {
    let mut assets = AssetManager::from_ini_probe_root(dir).map_err(|error| error.to_string())?;
    let has_root = |assets: &AssetManager, suffix: &str| {
        ["rules", "art", "ai", "mpmodes"]
            .iter()
            .any(|stem| assets.contains(&format!("{stem}{suffix}.ini")))
    };
    if has_root(&assets, "md")
        || (0..=99).any(|slot| assets.contains(&format!("expandmd{slot:02}.mix")))
    {
        return Ok(false);
    }
    for slot in (0..=99).rev() {
        let name = format!("expand{slot:02}.mix");
        if assets.contains(&name)
            && let Err(error) = assets.load_nested(&name)
        {
            log::warn!("Skipping unreadable discovery archive {name}: {error}");
        }
    }
    Ok(has_root(&assets, "") && !has_root(&assets, "md"))
}

fn expansion_family(name: &str) -> Option<(&str, u8)> {
    let stem = name.strip_prefix("expand")?.strip_suffix(".mix")?;
    let (suffix, number) = stem.split_at_checked(stem.len().checked_sub(2)?)?;
    if suffix.is_empty()
        || suffix == "md"
        // More than two trailing digits could instead be a numbered suffix;
        // leave that ambiguous spelling to an explicit declaration.
        || suffix.as_bytes().last().is_some_and(u8::is_ascii_digit)
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || !number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((
        suffix,
        (number.as_bytes()[0] - b'0') * 10 + number.as_bytes()[1] - b'0',
    ))
}

fn inspect_family(
    dir: &Path,
    suffix: &str,
    files: &BTreeMap<u8, String>,
) -> Result<Option<(IniRootConfig, ArchiveNames)>, String> {
    let mut assets = AssetManager::from_ini_probe_root(dir).map_err(|error| error.to_string())?;
    // Init_Base_Mix_Files (0x005301A0) visits 99 down to 00. The existing
    // resolver keeps loose files first and the first registered MIX winner;
    // an empty higher-priority root must not expose a lower one.
    for name in files.values().rev() {
        if let Err(error) = assets.load_nested(name) {
            log::warn!("Skipping unreadable discovery archive {name}: {error}");
        }
    }
    let present = |name: &str| -> Result<bool, String> {
        assets
            .resolve_ref_bounded(name, MAX_INI_BYTES)
            .map(|source| source.is_some_and(|source| !source.bytes.is_empty()))
            .map_err(|error| format!("Could not inspect mod file {name}: {error}"))
    };
    let roots = [
        format!("rules{suffix}.ini"),
        format!("art{suffix}.ini"),
        format!("ai{suffix}.ini"),
    ];
    for name in &roots {
        if !present(name)? {
            return Ok(None);
        }
    }
    let [rules, art, ai] = roots;
    let mut ini = IniRootConfig {
        profile: IniProfile::Custom,
        rules: Some(rules),
        art: Some(art),
        ai: Some(ai),
        ..Default::default()
    };
    for (stem, slot) in [
        ("sound", &mut ini.sound),
        ("eva", &mut ini.eva),
        ("theme", &mut ini.theme),
        ("battle", &mut ini.battle),
        ("mission", &mut ini.mission),
        ("mapsel", &mut ini.mapsel),
    ] {
        let name = format!("{stem}{suffix}.ini");
        if present(&name)? {
            *slot = Some(name);
        }
    }
    // Only the supported native theater control names imply this suffix;
    // UIMD, MPModesMD and other fixed names do not change with the family.
    for stem in ["temperat", "snow", "urban", "urbann", "desert", "lunar"] {
        if present(&format!("{stem}{suffix}.ini"))? {
            ini.theater_suffix = Some(suffix.to_owned());
            break;
        }
    }
    let mut archives = ArchiveNames {
        expansion_prefix: format!("expand{suffix}"),
        ..ArchiveNames::default()
    };
    for (stem, slot) in [
        ("maps", &mut archives.map_prefix),
        ("mov", &mut archives.movie_prefix),
    ] {
        let prefix = format!("{stem}{suffix}");
        if (0..=99).any(|index| assets.contains(&format!("{prefix}{index:02}.mix"))) {
            *slot = prefix;
        }
    }
    for (stem, slot) in [
        ("multi", &mut archives.multiplayer),
        ("theme", &mut archives.theme),
    ] {
        let name = format!("{stem}{suffix}.mix");
        if assets.contains(&name) {
            *slot = name;
        }
    }
    ini.validate()?;
    archives.validate()?;
    Ok(Some((ini, archives)))
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;
    use crate::assets::asset_manager::test_support::TestDirectory;
    use crate::map::source::test_support::make_new_format_mix_bytes;

    #[test]
    fn discovery_uses_real_loose_and_numbered_archive_winners() {
        let dir = TestDirectory::new("numbered-discovery-winners");
        let write = |name: &str, entries: &[(&str, &[u8])]| {
            std::fs::write(dir.path().join(name), make_new_format_mix_bytes(entries)).unwrap();
        };
        write(
            "expandalt01.mix",
            &[
                ("rulesalt.ini", b"[General]\n"),
                ("artalt.ini", b"[Unit]\n"),
                ("aialt.ini", b"[AI]\n"),
                ("soundalt.ini", b"[SoundList]\n"),
                ("snowalt.ini", b"[General]\n"),
            ],
        );
        write("EXPANDALT99.MIX", &[("artalt.ini", b"")]);
        assert!(discover(dir.path()).unwrap().is_none());

        write("EXPANDALT99.MIX", &[("artalt.ini", b"[Unit]\nVoxel=yes\n")]);
        write("mapsalt03.mix", &[("delta.map", b"[Map]\n")]);
        let (ini, archives) = discover(dir.path()).unwrap().unwrap();
        assert_eq!(ini.rules.as_deref(), Some("rulesalt.ini"));
        assert_eq!(ini.art.as_deref(), Some("artalt.ini"));
        assert_eq!(ini.ai.as_deref(), Some("aialt.ini"));
        assert_eq!(ini.sound.as_deref(), Some("soundalt.ini"));
        assert_eq!(ini.theater_suffix.as_deref(), Some("alt"));
        assert!(ini.modes.is_none() && ini.map_catalog.is_none());
        assert_eq!(archives.expansion_prefix, "expandalt");
        assert_eq!(archives.map_prefix, "mapsalt");
        assert_eq!(archives.theme, ArchiveNames::default().theme);
        assert_eq!(archives.multiplayer, ArchiveNames::default().multiplayer);
        assert_eq!(archives.movie_prefix, ArchiveNames::default().movie_prefix);

        std::fs::write(dir.path().join("ARTALT.INI"), b"").unwrap();
        assert!(discover(dir.path()).unwrap().is_none());
    }

    #[test]
    fn discovery_preserves_stock_and_refuses_ambiguous_complete_families() {
        let dir = TestDirectory::new("numbered-discovery-ambiguity");
        dir.write_mix("expandmd99.mix", "rulesmd.ini", b"[General]\n");
        dir.write_mix("expandpartial99.mix", "rulespartial.ini", b"[General]\n");
        std::fs::write(
            dir.path().join("expandghost001.mix"),
            make_new_format_mix_bytes(&[
                ("rulesghost0.ini", b"[General]\n"),
                ("artghost0.ini", b"[Unit]\n"),
                ("aighost0.ini", b"[AI]\n"),
            ]),
        )
        .unwrap();
        assert!(discover(dir.path()).unwrap().is_none());
        for suffix in ["first", "second"] {
            std::fs::write(
                dir.path().join(format!("expand{suffix}01.mix")),
                make_new_format_mix_bytes(&[
                    (&format!("rules{suffix}.ini"), b"[General]\n"),
                    (&format!("art{suffix}.ini"), b"[Unit]\n"),
                    (&format!("ai{suffix}.ini"), b"[AI]\n"),
                ]),
            )
            .unwrap();
        }
        let error = discover(dir.path()).unwrap_err();
        assert!(error.contains("first") && error.contains("second"));
    }
}
