//! Print this source's saved-game layout id (`sim::snapshot`), in decimal.
//!
//! The browser build compiles it in instead of tracing it at run time:
//! tools/wasm/build.py runs this and sets `VERA20K_SNAPSHOT_LAYOUT`.

fn main() {
    println!("{}", vera20k_core::sim::snapshot::traced_snapshot_layout());
}
