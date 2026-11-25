//! Raw identities shared by the installed terrain reader and multiplayer build check.
use std::path::Path;

pub fn identities(terrain: &Path) -> [(&'static str, usize, [u8; 32]); 2] {
    ["vera_ice.mix", "vera_ice_tiles.ini"].map(|name| {
        let path = terrain.join(name);
        println!("cargo::rerun-if-changed={}", path.display());
        let bytes = std::fs::read(path).expect("read release terrain file");
        (name, bytes.len(), crate::sha256::sha256_bytes(&bytes))
    })
}
