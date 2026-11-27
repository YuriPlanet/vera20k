pub use vera20k_core::{assets, map, rules, sim, util};

mod bindings {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../target/save-fixture-writer/bindings.rs"
    ));
}
mod writer {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../target/save-fixture-writer/scenario.rs"
    ));
}

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../target/save-fixture-writer/revision_cases.rs"
));

fn main() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(write_fixture)
        .unwrap()
        .join()
        .unwrap();
}

fn write_fixture() {
    let description = util::save_codec::layout::describe::<sim::snapshot::GameSnapshot>().unwrap();
    std::fs::write("target/save-fixture-writer/layout.txt", description).unwrap();
    let mut args = std::env::args_os().skip(1);
    let path = args.next().expect("supply an output path");
    let path = std::path::Path::new(&path);
    assert!(!path.exists(), "choose a new output file");
    let mode = args
        .next()
        .expect("supply skirmish, campaign, tunnel, alpha, sequence, image-states, or temporal");
    match mode.to_str() {
        Some("campaign") => writer::write(path, true, writer::STANDARD),
        Some("skirmish") => writer::write(path, false, writer::STANDARD),
        Some("sequence") => writer::write(path, false, writer::SEQUENCES),
        Some("image-states") => writer::write(path, false, writer::IMAGE_STATES),
        Some("tunnel") => write_tunnel(path),
        Some("alpha") => write_alpha(path),
        Some("temporal") => write_temporal(path),
        _ => panic!("supply skirmish, campaign, tunnel, alpha, sequence, image-states, or temporal"),
    }
    println!("layout {:#010X}", sim::snapshot::traced_snapshot_layout());
}
