fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    // Build scripts run on the host; inspect Cargo's target for cross-builds.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("Cargo sets target OS");
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if let Some(argument) = game_stack_link_arg(&target_os, &target_env) {
        // Only the game needs the larger startup stack. Leave resource tools,
        // tests, libraries and non-Windows targets at their existing defaults.
        println!("cargo::rustc-link-arg-bin=vera20k={argument}");
    }
}

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
