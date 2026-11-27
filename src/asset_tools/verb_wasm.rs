//! `asset wasm-check|wasm-embed|wasm-extract`: Wasm extension modules for
//! maps (RFC 0001 section 13). They need no retail install.
//!
//! - `asset wasm-check <module.wasm | map>` runs the load checks.
//! - `asset wasm-embed <map> <rules.wasm> <out-map>` writes `[VeraWasm]` and
//!   `[VeraWasmRulesPack]`.
//! - `asset wasm-extract <map> <out.wasm>` writes the map's `rules.wasm`.
//!
//! Each prints one JSON object and exits nonzero on failure.

use std::path::Path;

use crate::map::ext_module_pack::{self as pack, API, RULES_PACK, RULES_SIZE, SECTION};
use crate::rules::ini_parser::IniFile;
use crate::util::ini_writer::{replace_ini_section, set_ini_values};

/// Runs a `wasm-*` verb; `None` when `verb` is not one.
pub fn run(verb: &str, args: &[String]) -> Option<Result<serde_json::Value, String>> {
    Some(match (verb, args) {
        ("wasm-check", [input]) => check(Path::new(input)),
        ("wasm-embed", [map, module, out]) => {
            embed(Path::new(map), Path::new(module), Path::new(out))
        }
        ("wasm-extract", [map, out]) => extract(Path::new(map), Path::new(out)),
        ("wasm-check" | "wasm-embed" | "wasm-extract", _) => {
            Err("wrong arguments; run `asset --help`".into())
        }
        _ => return None,
    })
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn map_ini(bytes: &[u8]) -> Result<IniFile, String> {
    IniFile::from_bytes(bytes).map_err(|e| format!("map INI: {e}"))
}

/// The load checks the game runs before a match.
fn check_module(module: &[u8]) -> Result<(), String> {
    vera20k_wasm_ext::SimModule::load(module, vera20k_wasm_ext::Limits::MAP)
        .map(drop)
        .map_err(|e| e.to_string())
}

fn check(input: &Path) -> Result<serde_json::Value, String> {
    let bytes = read(input)?;
    let module = if bytes.starts_with(b"\0asm") {
        bytes
    } else {
        pack::rules_module(&map_ini(&bytes)?)?.ok_or("the map embeds no rules module")?
    };
    check_module(&module)?;
    Ok(serde_json::json!({ "ok": true, "bytes": module.len() }))
}

fn embed(map: &Path, module: &Path, out: &Path) -> Result<serde_json::Value, String> {
    let module = read(module)?;
    check_module(&module)?;
    let lines = pack::pack_lines(&module);
    let pairs: Vec<(&str, &str)> = lines
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let text = replace_ini_section(&read(map)?, RULES_PACK, &pairs);
    let size = module.len().to_string();
    let api = format!("{}.{}", API.0, API.1);
    let text = set_ini_values(&text, SECTION, &[("Api", &api), (RULES_SIZE, &size)]);
    // Read back what was written, as the game will.
    let embedded = pack::rules_module(&map_ini(&text)?)?;
    if embedded.as_deref() != Some(module.as_slice()) {
        return Err("the embedded module does not read back".into());
    }
    write(out, &text)?;
    Ok(serde_json::json!({ "ok": true, "bytes": module.len(), "lines": lines.len() }))
}

fn extract(map: &Path, out: &Path) -> Result<serde_json::Value, String> {
    let module =
        pack::rules_module(&map_ini(&read(map)?)?)?.ok_or("the map embeds no rules module")?;
    write(out, &module)?;
    Ok(serde_json::json!({ "ok": true, "bytes": module.len() }))
}
