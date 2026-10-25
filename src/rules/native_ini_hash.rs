//! The Rules, AI and Art INI hashes a network game compares
//! (`[0xB77E00]`/`[0xB77E08]`/`[0xB77E04]` for WOL, written by the WOL
//! reset `0x00780110` at `0x0078068E..0x007806B2`; `[0xAC026C..0xAC0274]`
//! for LAN, written at `LANLobby__Run` `0x005DC35A..0x005DC37D`).
//!
//! Every input is a process-lifetime INI object: the startup-selected rules
//! root `[0x887048]`, AIMD `0x887128` (loaded once, digest-checked), the
//! mode INIs `0x005D67B0` caches, CoopCampMD (latched once in
//! `[0xB053B4]`/`[0xB053B8]`) and ARTMD, which `0x00679EE0` re-reads fresh
//! before its hash. The hashes are therefore the same at every computation
//! in one process.

use crate::rules::ini_parser::IniFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeIniHashes {
    /// `0x00679D90`.
    pub rules: u32,
    /// `0x00679ED0`.
    pub ai: u32,
    /// `0x00679EC0`.
    pub art: u32,
}

impl NativeIniHashes {
    /// `rules_root` and `art` are the fresh loads; `ai` is AIMD as loaded,
    /// its `[Digest]` still present. `mode_inis` holds the mode INIs of the
    /// ids `1..=count` in id order (`0x005D5ED0`, `0x005D5F30`), an empty
    /// INI for a mode whose file is missing. `cooperative` is CoopCampMD,
    /// `None` when it did not load (the term is then 0).
    pub fn compute<'a>(
        rules_root: &IniFile,
        mode_inis: impl IntoIterator<Item = &'a IniFile>,
        cooperative: Option<&IniFile>,
        ai: &IniFile,
        art: &IniFile,
    ) -> Self {
        // 0x00679D90: the three terms are 32-bit wrapping adds.
        let rules = mode_inis
            .into_iter()
            .fold(rules_root.native_crc(), |sum, mode| {
                sum.wrapping_add(mode.native_crc())
            })
            .wrapping_add(cooperative.map_or(0, IniFile::native_crc));
        Self {
            rules,
            ai: ai.digest_checked_crc(),
            art: art.native_crc(),
        }
    }

    /// `[0xB77E04] ^ [0xB77E08] ^ [0xB77E00]`, the hash exInfo publishes
    /// (`0x0077E7A0`) and admission compares (`0x0079BB16..0x0079BBAE`).
    pub fn combined(&self) -> i32 {
        (self.art ^ self.ai ^ self.rules) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::NativeIniHashes;
    use crate::rules::ini_parser::IniFile;

    fn corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/ini_crc.json",
        ))
        .expect("native INI CRC corpus")
    }

    /// The production reader over a retail file as native loads it.
    fn retail(name: &str) -> Option<IniFile> {
        let bytes = crate::rules::retail_ini_fixture::retail_ini_bytes(&name.to_ascii_lowercase())
            .or_else(|| crate::rules::retail_ini_fixture::retail_ini_bytes(name))?;
        Some(IniFile::from_bytes(&bytes).expect("retail INI"))
    }

    #[test]
    fn synthetic_controls_hash_as_native_get_crc() {
        for case in corpus()["synthetic"]
            .as_array()
            .expect("synthetic controls")
        {
            let ini = if case["absent"].as_bool() == Some(true) {
                IniFile::empty()
            } else {
                let input = case["input"].as_str().expect("input text");
                IniFile::from_bytes(&crate::util::native_string::narrow_widened(input)).unwrap()
            };
            assert_eq!(
                ini.native_crc(),
                case["crc_unsigned"].as_u64().unwrap() as u32,
                "{}",
                case["name"]
            );
        }
    }

    /// `0x00476D80` executed on each retail file with the game's
    /// `ReadCCFile` arguments, against the production reader.
    #[test]
    fn retail_files_hash_as_native_get_crc() {
        for case in corpus()["retail"].as_array().expect("retail controls") {
            let name = case["file"].as_str().unwrap();
            let Some(ini) = retail(name) else {
                return;
            };
            let crc = if case["digest_argument"].as_i64() == Some(1) {
                ini.digest_checked_crc()
            } else {
                ini.native_crc()
            };
            assert_eq!(crc, case["crc_unsigned"].as_u64().unwrap() as u32, "{name}");
        }
    }

    /// `0x00679D90`, `0x00679ED0` and `0x00679EC0` executed natively over
    /// the retail files.
    #[test]
    fn retail_composite_matches_the_native_run() {
        let corpus = corpus();
        let Some(rules) = retail("RULESMD.INI") else {
            return;
        };
        let modes = corpus["modes"].as_object().expect("mode files");
        let mode_inis: Vec<IniFile> = (1..=modes.len())
            .map(|id| retail(modes[&id.to_string()]["file"].as_str().unwrap()).unwrap())
            .collect();
        let hashes = NativeIniHashes::compute(
            &rules,
            &mode_inis,
            retail("COOPCAMPMD.INI").as_ref(),
            &retail("AIMD.INI").unwrap(),
            &retail("ARTMD.INI").unwrap(),
        );
        let composite = &corpus["composite"];
        let value = |key: &str| composite[key]["unsigned"].as_u64().unwrap() as u32;
        assert_eq!(
            (hashes.rules, hashes.ai, hashes.art),
            (value("rules"), value("ai"), value("art"))
        );
        assert_eq!(
            hashes.combined(),
            composite["rules_xor_ai_xor_art"]["signed"]
                .as_i64()
                .unwrap() as i32
        );
    }
}
