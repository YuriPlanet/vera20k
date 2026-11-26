//! First port milestone: run the production core on Horizon before the desktop UI.

mod input;
#[cfg(all(any(target_os = "horizon", test), not(feature = "client")))]
mod match_runtime;

pub use vera20k_core::{assets, map, rules, sim, util};

#[cfg(all(feature = "gpu", feature = "client"))]
compile_error!("select either the GPU probe or the full client");

// The shared sidebar modules carry desktop-side `#[cfg(test)]` suites that
// depend on the desktop crate's dev-dependencies and test fixtures. The
// desktop crate owns those tests; host tests of the standalone Switch crate
// only need the gameplay/runtime modules above.
#[cfg(all(not(test), not(feature = "client")))]
#[path = "../../../src/ui/sidebar/mod.rs"]
pub mod ui_sidebar;
#[cfg(all(not(test), not(feature = "client")))]
pub mod ui {
    pub use crate::ui_sidebar as sidebar;
    pub mod power_bar_anim {
        pub use crate::ui_sidebar::power_bar_anim::PowerBarAnimState;
    }
}
#[cfg(all(not(test), not(feature = "client")))]
#[path = "../../../src/app/input/sidebar_eva.rs"]
pub mod sidebar_eva;

#[cfg(all(target_os = "horizon", feature = "client"))]
mod client_frontend;

#[cfg(all(target_os = "horizon", feature = "gpu"))]
#[path = "../../../src/render/gpu.rs"]
mod gpu;
#[cfg(all(target_os = "horizon", feature = "gpu"))]
mod gpu_frontend;
#[cfg(all(target_os = "horizon", feature = "gpu"))]
mod live_frontend;
#[cfg(all(target_os = "horizon", feature = "gpu"))]
mod render;
#[cfg(all(target_os = "horizon", feature = "gpu"))]
mod retail_objects;
#[cfg(all(target_os = "horizon", feature = "gpu"))]
mod sidebar_frontend;

use std::path::Path;
use vera20k_core::{assets::mix_archive::MixArchive, assets::mix_hash::mix_hash};

// getrandom 0.3.x custom backend for Horizon: delegates to csrngGetRandomBytes.
// Required whenever getrandom_backend="custom" is set (Switch client build).
#[cfg(all(target_os = "horizon", getrandom_backend = "custom"))]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    unsafe extern "C" {
        fn csrngGetRandomBytes(out: *mut u8, size: usize) -> u32;
    }
    let rc = unsafe { csrngGetRandomBytes(dest, len) };
    if rc == 0 {
        Ok(())
    } else {
        Err(getrandom::Error::UNEXPECTED)
    }
}

// The HTTPS provider uses getrandom 0.2; it needs the same platform entropy.
#[cfg(target_os = "horizon")]
fn platform_getrandom_02(dest: &mut [u8]) -> Result<(), getrandom_02::Error> {
    unsafe extern "C" {
        fn csrngGetRandomBytes(out: *mut u8, size: usize) -> u32;
    }
    let rc = unsafe { csrngGetRandomBytes(dest.as_mut_ptr(), dest.len()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(getrandom_02::Error::UNSUPPORTED)
    }
}

#[cfg(target_os = "horizon")]
getrandom_02::register_custom_getrandom!(platform_getrandom_02);

#[cfg(any(target_os = "horizon", test))]
fn retail_probe(directory: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use vera20k_core::assets::asset_manager::{
        AssetManager, MediaArchiveMode, holds_master_archives,
    };

    if !holds_master_archives(directory) {
        return Ok(vec![
            format!("retail_root={}", directory.display()),
            "retail_required=ra2md.mix,ra2.mix".into(),
            "retail_assets_present=false".into(),
        ]);
    }
    let assets = AssetManager::new(directory, MediaArchiveMode::STOCK_DIGITAL)?;
    let ini = assets
        .get_ref("rulesmd.ini")
        .ok_or("rulesmd.ini missing from retail archive stack")?;
    if ini.is_empty() {
        return Err("rulesmd.ini is empty".into());
    }
    Ok(vec![
        format!("retail_root={}", directory.display()),
        "retail_assets_present=true".into(),
        format!("retail_rules_bytes={}", ini.len()),
        "retail_rules_lookup=true".into(),
    ])
}

fn core_probe(directory: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rayon::prelude::*;
    let mut results = Vec::new();
    std::fs::create_dir_all(directory)?;
    let payload = b"[SwitchProbe]\nRuntime=Horizon\n";
    let mut mix = Vec::new();
    mix.extend_from_slice(&1_u16.to_le_bytes());
    mix.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    mix.extend_from_slice(&mix_hash("probe.ini").to_le_bytes());
    mix.extend_from_slice(&0_u32.to_le_bytes());
    mix.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    mix.extend_from_slice(payload);
    let path = directory.join("probe.mix");
    std::fs::write(&path, mix)?;
    let archive = MixArchive::load(&path)?;
    let extracted = archive
        .get_by_name("probe.ini")
        .ok_or("MIX lookup failed")?;
    if extracted != payload {
        return Err("MIX payload mismatch".into());
    }
    results.push("mix_file_load=true".into());
    let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(extracted)?;
    if !ini.section_or_empty("SwitchProbe").is_present("Runtime") {
        return Err("INI section/key missing".into());
    }
    results.push(format!("ini_parse=true crc={:08x}", ini.native_crc()));
    let file_time = (11_644_473_600 + 1_785_419_112) * 10_000_000_u64;
    let (date, time) = vera20k_core::util::native_file_time::format_file_time_parts(file_time)
        .ok_or("timestamp formatting failed")?;
    results.push(format!("timestamp_format=true date={date} time={time}"));

    let pool = rayon::ThreadPoolBuilder::new().num_threads(3).build()?;
    let sum = pool.install(|| (0..4096_u64).into_par_iter().sum::<u64>());
    if sum != 8_386_560 {
        return Err("Rayon reduction failed".into());
    }
    results.push(format!(
        "rayon=true workers={} checksum={sum}",
        pool.current_num_threads()
    ));

    let simulation = vera20k_core::sim::world::Simulation::with_seed(12345);
    let hash = simulation.state_hash();
    results.push(format!("simulation_construct=true state_hash={hash:016x}"));
    let mut runtime = vera20k_core::sim::runtime::SimRuntime {
        simulation,
        resources: vera20k_core::sim::runtime::SimResources::empty().into(),
    };
    for _ in 0..16 {
        runtime
            .advance_frame_for_tooling(&[], vera20k_core::headless_scenario::SIM_TICK_MS)
            .map_err(|error| format!("simulation frame failed: {error:?}"))?;
    }
    results.push(format!(
        "simulation_frames=16 state_hash={:016x}",
        runtime.simulation.state_hash()
    ));
    Ok(results)
}

#[cfg(all(target_os = "horizon", not(any(feature = "gpu", feature = "client"))))]
mod horizon {
    use std::{io::Write, time::Instant};

    unsafe extern "C" {
        fn platform_console_init();
        fn platform_running() -> bool;
        fn platform_buttons() -> u64;
        fn platform_present();
        fn platform_console_exit();
        fn platform_is_nro() -> bool;
        fn platform_graphics_init();
        fn platform_graphics_ready() -> bool;
        fn platform_graphics_present();
        fn platform_graphics_exit();
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn rust_main() -> i32 {
        unsafe { platform_console_init() };
        let binary_format = if unsafe { platform_is_nro() } {
            "nro"
        } else {
            "nsp"
        };
        println!("VERA20k Switch — core port probe\n");
        let start = Instant::now();
        let result = std::thread::Builder::new()
            .name("vera20k-core".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let root = std::path::Path::new("sdmc:/game");
                let mut lines = super::retail_probe(root).map_err(|error| error.to_string())?;
                lines.extend(
                    super::core_probe(std::path::Path::new("sdmc:/switch/vera20k"))
                        .map_err(|error| error.to_string())?,
                );
                Ok::<_, String>(lines)
            });
        let (passed, mut report) = match result {
            Ok(worker) => match worker.join() {
                Ok(Ok(lines)) => (true, lines.join("\n")),
                Ok(Err(error)) => (false, format!("core_error={error}")),
                Err(_) => (false, "core_worker_panicked=true".into()),
            },
            Err(error) => (false, format!("std_thread_error={error}")),
        };
        report.push_str(&format!("\nbinary_format={binary_format}\n"));
        report.push_str(&format!(
            "\nelapsed_ms={}\ncore_probe_passed={passed}\n",
            start.elapsed().as_millis()
        ));
        let graphics_ready = if passed {
            unsafe { platform_graphics_init() };
            unsafe { platform_graphics_ready() }
        } else {
            false
        };
        report.push_str(&format!("graphics_context={graphics_ready}\n"));
        if graphics_ready {
            unsafe { platform_graphics_present() };
        }
        println!(
            "{report}\n{}\n\nA: input test   +: exit",
            if passed {
                "CORE PROBE PASSED"
            } else {
                "CORE PROBE FAILED"
            }
        );
        let mut presses = 0;
        while unsafe { platform_running() } {
            let buttons = unsafe { platform_buttons() };
            let edges = super::input::PadEdges::from_mask(buttons);
            if edges.plus {
                break;
            }
            if edges.a {
                presses += 1;
                println!("A presses: {presses}");
            }
            if graphics_ready {
                unsafe { platform_graphics_present() };
            } else {
                unsafe { platform_present() };
            }
        }
        report.push_str(&format!("a_button_presses={presses}\nnormal_exit=true\n"));
        let saved = std::fs::write(
            format!("sdmc:/switch/vera20k/core-probe-{binary_format}.txt"),
            report,
        );
        let _ = std::io::stdout().flush();
        if graphics_ready {
            unsafe { platform_graphics_exit() };
        } else {
            unsafe { platform_console_exit() };
        }
        if passed && saved.is_ok() { 0 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn production_core_probe_reads_an_archive_and_constructs_a_simulation() {
        let directory =
            std::env::temp_dir().join(format!("vera20k-core-probe-{}", std::process::id()));
        let result = super::core_probe(&directory).expect("production core probe");
        for line in &result {
            println!("{line}");
        }
        assert!(
            result
                .iter()
                .any(|line| line.starts_with("simulation_construct=true"))
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
