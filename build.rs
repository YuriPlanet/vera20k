#[path = "src/util/sha256.rs"]
mod sha256;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    multiplayer_build_identity();
    source_location();

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
