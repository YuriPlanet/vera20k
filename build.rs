#[path = "src/util/sha256.rs"]
mod sha256;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    multiplayer_build_identity();
    source_location();
    shipped_languages();
    font_coverage();

    // Build scripts run on the host; inspect Cargo's target for cross-builds.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("Cargo sets target OS");
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if let Some(argument) = game_stack_link_arg(&target_os, &target_env) {
        // Only the game needs the larger startup stack. Leave resource tools,
        // tests, libraries and non-Windows targets at their existing defaults.
        println!("cargo::rustc-link-arg-bin=vera20k={argument}");
    }
    if target_os == "windows" {
        embed_windows_resources();
    }
}

/// A package version alone cannot identify two different development builds.
/// Hash portable source inputs, never paths on the build machine, retail data,
/// credentials or timestamps. The same source and gameplay features therefore
/// advertise the same compatibility identity on macOS and Linux.
fn multiplayer_build_identity() {
    use std::path::{Path, PathBuf};
    fn sources(directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).expect("read build source directory") {
            let entry = entry.expect("read build source entry");
            let kind = entry.file_type().expect("read build source type");
            if kind.is_dir() {
                sources(&entry.path(), files);
            } else if kind.is_file() && !entry.file_name().to_string_lossy().starts_with('.') {
                // Compiled lookup tables and shader sources are inputs too.
                // Restricting this walk to Rust misses include_bytes!/include_str!.
                files.push(entry.path());
            }
        }
    }
    let root =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let mut files = Vec::new();
    for directory in ["src", "services/relay-protocol/src"] {
        println!("cargo::rerun-if-changed={directory}");
        sources(&root.join(directory), &mut files);
    }
    for file in [
        "Cargo.toml",
        "Cargo.lock",
        "src/Cargo.toml",
        "build.rs",
        "services/relay-protocol/Cargo.toml",
    ] {
        println!("cargo::rerun-if-changed={file}");
        files.push(root.join(file));
    }
    files.sort();
    let mut input = b"VERA20k multiplayer build v1\0".to_vec();
    for file in files {
        input.extend_from_slice(
            file.strip_prefix(&root)
                .expect("source below root")
                .to_string_lossy()
                .replace('\\', "/")
                .as_bytes(),
        );
        input.push(0);
        let bytes = std::fs::read(&file).expect("read multiplayer build source");
        input.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        input.extend_from_slice(&bytes);
    }
    let mut features: Vec<_> = std::env::vars()
        .filter_map(|(key, _)| key.starts_with("CARGO_FEATURE_").then_some(key))
        .collect();
    features.sort();
    for feature in features {
        println!("cargo::rerun-if-env-changed={feature}");
        input.extend_from_slice(feature.as_bytes());
        input.push(0);
    }
    println!(
        "cargo::rustc-env=VERA20K_NETWORK_BUILD={}",
        sha256::sha256_hex(&input)
    );
}

/// Where the startup splash's license notice says this build's source is: the
/// commit a release build was made from (`VERA20K_COMMIT`, which the Release
/// workflow sets), otherwise the repository.
fn source_location() {
    println!("cargo::rerun-if-env-changed=VERA20K_COMMIT");
    let source = match std::env::var("VERA20K_COMMIT") {
        Ok(commit) if !commit.is_empty() => format!("github.com/YuriPlanet/vera20k/tree/{commit}"),
        _ => "github.com/YuriPlanet/vera20k".to_owned(),
    };
    println!("cargo::rustc-env=VERA20K_SOURCE={source}");
}

/// The language files VERA ships (`src/i18n/vera20k.<tag>.ini`), which
/// src/i18n/mod.rs compiles in: shipping a language is adding its file.
/// English, which holds every label, comes first. An installer's build sets
/// `VERA20K_LANGUAGE_FOLDER` and compiles in only English: the installer puts
/// the others in a folder (.github/release/languages.sh), where they can be
/// removed after the build.
fn shipped_languages() {
    const DIRECTORY: &str = "src/i18n";
    println!("cargo::rerun-if-changed={DIRECTORY}");
    println!("cargo::rerun-if-env-changed=VERA20K_LANGUAGE_FOLDER");
    let mut tags: Vec<String> = std::fs::read_dir(DIRECTORY)
        .expect("read src/i18n")
        .filter_map(|entry| {
            let name = entry.expect("read src/i18n entry").file_name();
            let name = name.to_str()?;
            Some(
                name.strip_prefix("vera20k.")?
                    .strip_suffix(".ini")?
                    .to_owned(),
            )
        })
        .collect();
    tags.sort_by_key(|tag| (tag != "en", tag.clone()));
    assert_eq!(
        tags.first().map(String::as_str),
        Some("en"),
        "src/i18n needs vera20k.en.ini"
    );
    if std::env::var_os("VERA20K_LANGUAGE_FOLDER").is_some() {
        tags.truncate(1);
    }
    let mut code = String::from("pub(super) const FILES: &[(&str, &str)] = &[\n");
    for tag in &tags {
        code.push_str(&format!(
            "    ({tag:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{DIRECTORY}/vera20k.{tag}.ini\"))),\n"
        ));
    }
    code.push_str("];\n");
    // The optional files beside a language's text, for the languages that
    // have them (src/i18n/mod.rs).
    for (constant, suffix) in [("SUBTITLES", "subtitles.txt"), ("CAMEO_LABELS", "cameos")] {
        code.push_str(&format!(
            "pub(super) const {constant}: &[(&str, &[u8])] = &[\n"
        ));
        for tag in &tags {
            let file = format!("{DIRECTORY}/vera20k.{tag}.{suffix}");
            if std::path::Path::new(&file).exists() {
                code.push_str(&format!(
                    "    ({tag:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{file}\")) as &[u8]),\n"
                ));
            }
        }
        code.push_str("];\n");
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    std::fs::write(out.join("shipped_languages.rs"), code).expect("write shipped_languages.rs");
}

/// What each font in `src/render/fonts` draws, for the browser build, which
/// fetches the bundled fonts it leaves out when text first needs one
/// (src/render/text_engine/web_fonts.rs): every font file's name, the start
/// of its SHA-256, which versions its URL, and the code points its character
/// map gives a glyph, as sorted inclusive ranges; then the glyph tables'
/// names and versions.
fn font_coverage() {
    const DIRECTORY: &str = "src/render/fonts";
    println!("cargo::rerun-if-changed={DIRECTORY}");
    let mut files: Vec<String> = std::fs::read_dir(DIRECTORY)
        .expect("read src/render/fonts")
        .filter_map(|entry| {
            let name = entry.expect("read src/render/fonts entry").file_name();
            let name = name.into_string().ok()?;
            (name.ends_with(".ttf") || name.ends_with(".otf")).then_some(name)
        })
        .collect();
    files.sort();
    let mut code = String::from("pub(super) const FONTS: &[(&str, &str, &[(u32, u32)])] = &[\n");
    for file in &files {
        let data = std::fs::read(format!("{DIRECTORY}/{file}")).expect("read a bundled font");
        let version = &sha256::sha256_hex(&data)[..16];
        code.push_str(&format!("    ({file:?}, {version:?}, &["));
        for (start, end) in mapped_ranges(&data) {
            code.push_str(&format!("({start:#x}, {end:#x}),"));
        }
        code.push_str("]),\n");
    }
    code.push_str("];\n");
    // Glyph tables that are not fonts (`Unifont-CJK-15.bin`), with the same
    // URL versions.
    let mut tables: Vec<String> = std::fs::read_dir(DIRECTORY)
        .expect("read src/render/fonts")
        .filter_map(|entry| {
            let name = entry.expect("read src/render/fonts entry").file_name();
            let name = name.into_string().ok()?;
            name.ends_with(".bin").then_some(name)
        })
        .collect();
    tables.sort();
    code.push_str("pub(super) const TABLES: &[(&str, &str)] = &[\n");
    for file in &tables {
        let data = std::fs::read(format!("{DIRECTORY}/{file}")).expect("read a glyph table");
        let version = &sha256::sha256_hex(&data)[..16];
        code.push_str(&format!("    ({file:?}, {version:?}),\n"));
    }
    code.push_str("];\n");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    std::fs::write(out.join("font_coverage.rs"), code).expect("write font_coverage.rs");
}

/// The code points `font`'s Unicode character map gives a glyph other than
/// glyph 0, as sorted inclusive ranges: from its format-12 subtable where it
/// has one, else its format-4 one, as fonts ship them both for the same
/// characters. A font without either maps nothing.
fn mapped_ranges(font: &[u8]) -> Vec<(u32, u32)> {
    let u16_at = |at: usize| u16::from_be_bytes([font[at], font[at + 1]]);
    let u32_at =
        |at: usize| u32::from_be_bytes([font[at], font[at + 1], font[at + 2], font[at + 3]]);
    let Some(cmap) = (0..usize::from(u16_at(4)))
        .map(|table| 12 + 16 * table)
        .find(|&record| &font[record..record + 4] == b"cmap")
        .map(|record| u32_at(record + 8) as usize)
    else {
        return Vec::new();
    };
    let mut best: Option<(u16, usize)> = None;
    for subtable in 0..usize::from(u16_at(cmap + 2)) {
        let record = cmap + 4 + 8 * subtable;
        let unicode = match u16_at(record) {
            0 => true,
            3 => matches!(u16_at(record + 2), 1 | 10),
            _ => false,
        };
        let at = cmap + u32_at(record + 4) as usize;
        let format = u16_at(at);
        if unicode && matches!(format, 4 | 12) && best.is_none_or(|(known, _)| known < format) {
            best = Some((format, at));
        }
    }
    let mut codepoints = Vec::new();
    match best {
        Some((12, at)) => {
            for group in 0..u32_at(at + 12) as usize {
                let group = at + 16 + 12 * group;
                let (start, end, glyph) = (u32_at(group), u32_at(group + 4), u32_at(group + 8));
                codepoints.extend((start..=end).filter(|&cp| glyph + (cp - start) != 0));
            }
        }
        Some((_, at)) => {
            let segments = usize::from(u16_at(at + 6) / 2);
            let ends = at + 14;
            let starts = ends + 2 * segments + 2;
            let deltas = starts + 2 * segments;
            let offsets = deltas + 2 * segments;
            for segment in 0..segments {
                let (start, end) = (u16_at(starts + 2 * segment), u16_at(ends + 2 * segment));
                let delta = u16_at(deltas + 2 * segment);
                let offset = usize::from(u16_at(offsets + 2 * segment));
                for cp in start..=end {
                    let glyph = if offset == 0 {
                        cp.wrapping_add(delta)
                    } else {
                        match u16_at(offsets + 2 * segment + offset + 2 * usize::from(cp - start)) {
                            0 => 0,
                            glyph => glyph.wrapping_add(delta),
                        }
                    };
                    if glyph != 0 {
                        codepoints.push(u32::from(cp));
                    }
                }
            }
        }
        None => {}
    }
    codepoints.sort_unstable();
    codepoints.dedup();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for cp in codepoints {
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == cp => *end = cp,
            _ => ranges.push((cp, cp)),
        }
    }
    ranges
}

/// The game's icon and version details, which Explorer, the Start menu and
/// Task Manager show for vera20k.exe. `winresource` compiles them with the
/// Windows SDK's resource compiler, which only a Windows host has; a build on
/// another host leaves them out.
#[cfg(windows)]
fn embed_windows_resources() {
    // Red Alert 2's own icon: the 32-bit images of retail game.exe's icon group.
    const ICON: &str = r".github\release\windows\vera20k.ico";
    println!("cargo::rerun-if-changed={ICON}");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(ICON)
        .set("ProductName", "VERA20k")
        .set("FileDescription", "VERA20k")
        .set("OriginalFilename", "vera20k.exe")
        .set(
            "LegalCopyright",
            "Copyright © 2026 Halvor Nikolai Rusten and VERA20k contributors. \
             Free software under the GNU GPL version 3. \
             Not affiliated with or endorsed by Electronic Arts.",
        );
    // Only the game, like the stack size. It runs without them, so a missing
    // resource compiler costs a warning rather than the build; the Release
    // workflow checks that the shipped program has them.
    if let Err(error) = resource.compile_for("vera20k") {
        println!("cargo::warning=vera20k.exe has no icon or version details: {error}");
    }
}

#[cfg(not(windows))]
fn embed_windows_resources() {}

fn game_stack_link_arg(target_os: &str, target_env: &str) -> Option<String> {
    if target_os != "windows" {
        return None;
    }

    // Startup retains large App/AppState values across the event-loop and
    // rules-loading calls. Reserve 8 MiB without raising initial commitment.
    const RESERVE_BYTES: usize = 8 * 1024 * 1024;
    match target_env {
        "msvc" => Some(format!("/STACK:{RESERVE_BYTES}")),
        "gnu" => Some(format!("-Wl,--stack,{RESERVE_BYTES}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::game_stack_link_arg;

    #[test]
    fn msvc_reserves_eight_mib_without_overriding_commit() {
        assert_eq!(
            game_stack_link_arg("windows", "msvc").as_deref(),
            Some("/STACK:8388608")
        );
    }

    #[test]
    fn gnu_windows_uses_the_pe_linker_option() {
        assert_eq!(
            game_stack_link_arg("windows", "gnu").as_deref(),
            Some("-Wl,--stack,8388608")
        );
    }

    #[test]
    fn other_targets_do_not_receive_windows_linker_options() {
        for (os, environment) in [("linux", "gnu"), ("macos", ""), ("windows", "")] {
            assert_eq!(game_stack_link_arg(os, environment), None);
        }
    }
}
