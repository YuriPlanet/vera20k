//! Wasm extension modules embedded in a map (RFC 0001, appendix "INI keys").
//!
//! `[VeraWasm]` declares the API (`Api=1.0`) and each module's size
//! (`RulesSize=`, `UISize=`). `[VeraWasmRulesPack]` and `[VeraWasmUIPack]`
//! hold the module as standard base64 split over numbered lines, decoding to
//! the chunked LZO format of `IsoMapPack5`.

use crate::rules::ini_parser::IniFile;
use crate::util::{base64, lzo};

pub const SECTION: &str = "VeraWasm";
pub const RULES_PACK: &str = "VeraWasmRulesPack";
pub const UI_PACK: &str = "VeraWasmUIPack";
pub const RULES_SIZE: &str = "RulesSize";
pub const UI_SIZE: &str = "UISize";

/// The module API this game implements (`vera:sim@1.0`).
pub const API: (u32, u32) = (1, 0);
/// The largest module a map may embed (RFC open question 7).
pub const MAX_MODULE_BYTES: usize = 8 << 20;
/// Characters per pack line, as `IsoMapPack5` writes them; the INI reader
/// keeps 127 bytes of a value.
const LINE_CHARS: usize = 70;
/// Bytes per LZO chunk; a chunk's lengths are 16-bit.
const CHUNK_BYTES: usize = 8192;

/// The map's `rules.wasm`, or `None` when it embeds none. An error names
/// what is wrong; a match with such a map must not start.
pub fn rules_module(map: &IniFile) -> Result<Option<Vec<u8>>, String> {
    read(map, RULES_PACK, RULES_SIZE)
}

/// The map's `ui.wasm`, as [`rules_module`].
pub fn ui_module(map: &IniFile) -> Result<Option<Vec<u8>>, String> {
    read(map, UI_PACK, UI_SIZE)
}

/// Checks a declared `Api=` (`major.minor`) against [`API`].
pub fn check_api(declared: &str) -> Result<(), String> {
    let parsed = declared
        .trim()
        .split_once('.')
        .and_then(|(major, minor)| Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?)));
    match parsed {
        Some((major, minor)) if major == API.0 && minor <= API.1 => Ok(()),
        Some(_) => Err(format!(
            "needs module API {}; this game supports {}.{}",
            declared.trim(),
            API.0,
            API.1
        )),
        None => Err(format!("[{SECTION}] Api={declared} is not major.minor")),
    }
}

fn read(map: &IniFile, pack: &str, size_key: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(packed) = map.section(pack) else {
        return Ok(None);
    };
    let meta = map
        .section(SECTION)
        .ok_or_else(|| format!("the map has [{pack}] but no [{SECTION}]"))?;
    check_api(&meta.read_string("Api", "", 32))?;
    let size = meta.read_int(size_key, -1);
    let size = usize::try_from(size)
        .ok()
        .filter(|size| (1..=MAX_MODULE_BYTES).contains(size))
        .ok_or_else(|| format!("[{SECTION}] {size_key}={size} is not 1..={MAX_MODULE_BYTES}"))?;
    let compressed = base64::base64_decode(&packed.read_packed_text())
        .map_err(|e| format!("[{pack}] is not base64: {e}"))?;
    let module = lzo::decompress_chunks(&compressed, size)
        .map_err(|e| format!("[{pack}] does not decompress: {e:?}"))?;
    if module.len() != size {
        return Err(format!(
            "[{pack}] holds {} bytes, [{SECTION}] {size_key}={size}",
            module.len()
        ));
    }
    Ok(Some(module))
}

/// The `[VeraWasm]` size key and pack lines for embedding `module`: the
/// numbered keys `1`, `2`, ... and their values.
pub fn pack_lines(module: &[u8]) -> Vec<(String, String)> {
    let text = base64::base64_encode(&lzo::compress_chunks(module, CHUNK_BYTES));
    text.as_bytes()
        .chunks(LINE_CHARS)
        .enumerate()
        .map(|(index, line)| {
            (
                (index + 1).to_string(),
                String::from_utf8_lossy(line).into_owned(),
            )
        })
        .collect()
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;
    use crate::util::ini_writer::{replace_ini_section, set_ini_values};

    fn map_with(module: &[u8], api: &str, size: usize) -> IniFile {
        let lines = pack_lines(module);
        let pairs: Vec<(&str, &str)> = lines
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let text = replace_ini_section(b"[Basic]\nName=t\n", RULES_PACK, &pairs);
        let size = size.to_string();
        let text = set_ini_values(&text, SECTION, &[("Api", api), (RULES_SIZE, &size)]);
        IniFile::from_bytes(&text).expect("test INI parses")
    }

    #[test]
    fn an_embedded_module_comes_back_byte_for_byte() {
        // Past one LZO chunk, with runs and noise.
        let module: Vec<u8> = (0..20_000u32)
            .map(|i| if i % 7 == 0 { (i * 31) as u8 } else { 0 })
            .collect();
        let map = map_with(&module, "1.0", module.len());
        assert_eq!(rules_module(&map), Ok(Some(module)));
        assert_eq!(
            rules_module(&IniFile::from_bytes(b"[Basic]\n").expect("test INI parses")),
            Ok(None)
        );
    }

    #[test]
    fn a_wrong_size_or_newer_api_refuses_the_map() {
        let module = vec![0, 97, 115, 109, 1, 0, 0, 0];
        assert!(
            rules_module(&map_with(&module, "1.0", 9))
                .unwrap_err()
                .contains("holds 8 bytes")
        );
        assert!(
            rules_module(&map_with(&module, "1.1", 8))
                .unwrap_err()
                .contains("API 1.1")
        );
        assert!(rules_module(&map_with(&module, "2.0", 8)).is_err());
    }
}
