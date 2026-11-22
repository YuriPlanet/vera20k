//! Print this source's saved-game layout id in decimal, or its wire layout
//! with `--describe` to compare player saves between releases.
//!
//! The browser build compiles it in instead of tracing it at run time:
//! tools/wasm/build.py runs this and sets `VERA20K_SNAPSHOT_LAYOUT`.

fn main() -> anyhow::Result<()> {
    use vera20k_core::sim::snapshot::{GameSnapshot, traced_snapshot_layout};

    let mut args = std::env::args_os().skip(1);
    match (args.next(), args.next()) {
        (None, None) => println!("{}", traced_snapshot_layout()),
        (Some(arg), None) if arg == "--describe" => {
            let layout = vera20k_core::util::save_codec::layout::describe::<GameSnapshot>()?;
            print!("{layout}");
        }
        _ => anyhow::bail!("Usage: snapshot-layout [--describe]"),
    }
    Ok(())
}
