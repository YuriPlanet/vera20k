use std::path::Path;
use vera20k::assets::asset_manager::{AssetManager, MediaArchiveMode};
use vera20k::map::map_file;
use vera20k::rules::ini_parser::IniFile;
fn main() {
    let retail_root = std::env::var("RA2_DIR").expect("set RA2_DIR to the retail asset directory");
    let retail = Path::new(&retail_root);
    let assets = AssetManager::new(retail, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let mut rows = Vec::new();
    for name in [
        "RULESMD.INI",
        "ARTMD.INI",
        "LANGRULE.INI",
        "MPBattleMD.ini",
        "Hills.mmx",
    ] {
        let ini = if name == "Hills.mmx" {
            // Retail MMX wraps the map INI in a MIX; use the application's loader.
            Some(map_file::load_from_path(&retail.join(name)).unwrap().ini)
        } else {
            assets
                .get(name)
                .map(|bytes| IniFile::from_bytes(&bytes).unwrap())
        };
        if let Some(ini) = ini {
            for section in [
                "TREE01",
                "TIBTRE01",
                "DBRIS1LG",
                "DBRIS1SM",
                "DBRIS4SM",
                "D",
                "CombatDamage",
                "General",
                "AP",
                "HE",
                "Super",
            ] {
                for key in [
                    "Damage",
                    "DamageRadius",
                    "Warhead",
                    "Wood",
                    "Verses",
                    "CellSpread",
                    "PercentAtMax",
                    "Strength",
                    "Armor",
                    "Immune",
                    "SpawnsTiberium",
                    "C4Warhead",
                    "C4",
                    "TreeStrength",
                    "Bouncer",
                    "Elasticity",
                    "TemperateOccupationBits",
                    "SnowOccupationBits",
                    "Wall",
                ] {
                    if let Some(value) = ini.section(section).and_then(|s| s.get(key)) {
                        rows.push(format!("{{\"file\":{name:?},\"section\":{section:?},\"key\":{key:?},\"raw_value\":{value:?}}}"));
                    }
                }
            }
        } else {
            rows.push(format!("{{\"file\":{name:?},\"absent\":true}}"));
        }
    }
    println!("[{}]", rows.join(",\n"));
}
