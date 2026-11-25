//! Dump every unit profile the rules describe, as JSON.
//!
//! This is the review artifact: "does the policy understand this unit" is a
//! question about this file, not about the code. Run it with RA2_DIR set:
//!
//! cargo run -p vera20k-autoplay --features retail-fixture --bin autoplay-profiles

fn main() {
    let Ok(rules) = std::env::var("RA2_DIR") else {
        eprintln!("set RA2_DIR to the retail install");
        std::process::exit(2);
    };
    let _ = rules;
    let Some(retail) = vera20k_core::rules::retail_ini_fixture::retail_battle_rules() else {
        eprintln!("the retail rules did not load");
        std::process::exit(2);
    };
    let profiles = autoplay::profile::Profiles::build(&retail.rules);
    println!("{}", profiles.to_json());
    eprintln!("{} types", profiles.len());
}
