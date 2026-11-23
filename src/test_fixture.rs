//! Fixture files for library tests, read from the checkout at run time, and
//! fixture values built once per test process.
//!
//! Tests name native goldens and other fixtures by their path from the
//! checkout root (`tools/...`, `tests/fixtures/...`) instead of embedding them with
//! `include_str!`/`include_bytes!`. Embedded fixtures made up most of the test
//! executable, and every fixture edit recompiled the crate. Each file is read
//! and checked once per test process, by the first test that asks for it, and
//! kept until the process exits.
//!
//! A test executable reads the fixtures in its own checkout as they are when it
//! runs, so a saved test binary reproduces its results only while that checkout
//! holds the same files. `architecture_guards` keeps fixtures out of
//! `include_*!` and checks that every literal fixture path exists.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};

/// The checkout root that fixture paths start from. This crate's manifest
/// sits in `src/`, and Cargo runs its tests from there, so tests name checkout
/// files from this root, never from the working directory.
pub fn checkout_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the core manifest sits in the checkout's src/")
}

/// `GameConfig::load` as a process started in the checkout root sees it: the
/// checkout's `config.toml`, else the retail directory beside the executable.
pub fn game_config() -> anyhow::Result<crate::util::config::GameConfig> {
    let executable = std::env::current_exe()?;
    crate::util::config::GameConfig::load_from(checkout_root(), &executable)
}

/// UTF-8 text of the fixture at `path`, relative to the checkout root.
pub fn text(path: &'static str) -> &'static str {
    fixture(path).text.get_or_init(|| {
        std::str::from_utf8(bytes(path))
            .unwrap_or_else(|error| panic!("test fixture {path} is not UTF-8: {error}"))
    })
}

/// The fixture at `path`, relative to the checkout root, parsed as JSON. Tests
/// that read one corpus share its parse instead of repeating it.
///
/// A corpus that stores its RNG states once, in a top-level `shared_rng_states` table
/// (`tools/native_oracle.py`, `shared_rng_states`), reads back with each
/// `{"rng_state": n}` replaced by entry `n` and without the table.
pub fn json(path: &'static str) -> &'static serde_json::Value {
    fixture(path).json.get_or_init(|| {
        let mut value = serde_json::from_str(text(path))
            .unwrap_or_else(|error| panic!("test fixture {path} is not JSON: {error}"));
        expand_shared_rng_states(path, &mut value);
        value
    })
}

/// Asserts that `value`, something this game sends, equals the JSON fixture at
/// `path`: a copy another program's tests check, such as the lobby's
/// (`tests/fixtures/lobby/`). `VERA20K_UPDATE_FIXTURES=1` writes `value` there
/// instead; the diff is then a change that program has to accept.
pub fn assert_sent_json(path: &'static str, value: &serde_json::Value) {
    if std::env::var_os("VERA20K_UPDATE_FIXTURES").is_some_and(|flag| flag == "1") {
        let full = checkout_root().join(path);
        let text = serde_json::to_string_pretty(value).expect("a JSON value prints") + "\n";
        std::fs::write(&full, text)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", full.display()));
        return;
    }
    assert!(
        json(path) == value,
        "{path} differs from what the game sends; rerun with VERA20K_UPDATE_FIXTURES=1 \
         and change the programs that read it to accept the diff"
    );
}

fn expand_shared_rng_states(path: &str, document: &mut serde_json::Value) {
    fn expand(value: &mut serde_json::Value, table: &[serde_json::Value], path: &str) {
        let reference = match &*value {
            serde_json::Value::Object(map) if map.len() == 1 => {
                map.get("rng_state").and_then(serde_json::Value::as_u64)
            }
            _ => None,
        };
        if let Some(number) = reference {
            *value = usize::try_from(number)
                .ok()
                .and_then(|number| table.get(number))
                .unwrap_or_else(|| panic!("test fixture {path} has no RNG state {number}"))
                .clone();
            return;
        }
        match value {
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| expand(v, table, path)),
            serde_json::Value::Array(items) => {
                items.iter_mut().for_each(|v| expand(v, table, path))
            }
            _ => {}
        }
    }
    let Some(table) = document
        .as_object_mut()
        .and_then(|map| map.remove("shared_rng_states"))
    else {
        return;
    };
    let table = table
        .as_array()
        .unwrap_or_else(|| panic!("test fixture {path} shared_rng_states is not a list"));
    expand(document, table, path);
}

/// Serialize declared section/key string inputs into the production INI parser.
/// This does not tokenize values or resolve any referenced rule/catalog names.
pub fn ini_sections(sections: &serde_json::Value) -> crate::rules::ini_parser::IniFile {
    let mut text = String::new();
    for (name, entries) in sections
        .as_object()
        .expect("fixture sections are an object")
    {
        text.push_str(&format!("[{name}]\n"));
        for (key, value) in entries.as_object().expect("fixture section is an object") {
            text.push_str(&format!(
                "{key}={}\n",
                value.as_str().expect("fixture INI values are strings")
            ));
        }
    }
    crate::rules::ini_parser::IniFile::from_str(&text)
}

/// The bytes a fixture's hex string spells, two digits each.
pub fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "odd-length hex fixture");
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// Contents of the fixture at `path`, relative to the checkout root.
pub fn bytes(path: &'static str) -> &'static [u8] {
    fixture(path).bytes.get_or_init(|| {
        let full = checkout_root().join(path);
        std::fs::read(&full)
            .unwrap_or_else(|error| panic!("cannot read test fixture {}: {error}", full.display()))
            .leak()
    })
}

/// One fixture file; tests that ask for it while it is being read wait for
/// that read instead of starting their own.
#[derive(Default)]
struct Fixture {
    bytes: OnceLock<&'static [u8]>,
    text: OnceLock<&'static str>,
    json: OnceLock<serde_json::Value>,
}

fn fixture(path: &'static str) -> &'static Fixture {
    static FIXTURES: Mutex<BTreeMap<&str, &Fixture>> = Mutex::new(BTreeMap::new());
    let mut fixtures = FIXTURES.lock().unwrap_or_else(PoisonError::into_inner);
    fixtures
        .entry(path)
        .or_insert_with(|| Box::leak(Box::<Fixture>::default()))
}

/// A copy of the value `load` returns for `key`, built once per test process.
///
/// For fixture values that depend only on `key` and on inputs that do not
/// change while tests run (the checkout's files, `ini/`, `RA2_DIR`), so every
/// later build would make the same value. Concurrent first callers wait for
/// one build; a build that panics is retried. A value built while the thread
/// reads Rules the Ares way (`rules::ares_switch::ares_way`) is kept apart
/// from the stock-way one.
pub fn once_per_process<K, T>(key: K, load: impl FnOnce() -> Option<T>) -> Option<T>
where
    K: Ord + Send + 'static,
    T: Clone + Send + Sync + 'static,
{
    use std::any::{Any, TypeId};
    use std::sync::Arc;

    type Slots = BTreeMap<TypeId, Box<dyn Any + Send>>;
    static SLOTS: Mutex<Slots> = Mutex::new(BTreeMap::new());
    let slot = {
        let mut slots = SLOTS.lock().unwrap_or_else(PoisonError::into_inner);
        let by_key = slots
            .entry(TypeId::of::<(K, T)>())
            .or_insert_with(|| Box::new(BTreeMap::<(K, bool), Arc<OnceLock<Option<T>>>>::new()))
            .downcast_mut::<BTreeMap<(K, bool), Arc<OnceLock<Option<T>>>>>()
            .expect("slot map type follows its TypeId");
        let ares_way = crate::rules::ares_switch::ares_way();
        Arc::clone(by_key.entry((key, ares_way)).or_default())
    };
    slot.get_or_init(load).clone()
}

/// The stack of the threads the game loads and plays on: its main thread has
/// 8 MiB on every platform (`build.rs` links `/STACK` on Windows), as the
/// simulation thread has (`SimWorker::spawn`).
pub const GAME_STACK_BYTES: usize = 8 << 20;

/// Run a test's body on a thread with the game's stack instead of libtest's
/// 2 MiB one, under the test's name, and pass its panic on. Unoptimized, the
/// frames of a map or save load hold whole copies of the rules, the snapshot
/// and the simulation, more than 2 MiB together; the game runs them on
/// [`GAME_STACK_BYTES`].
pub fn on_game_stack(body: impl FnOnce() + Send + 'static) {
    let name = std::thread::current().name().unwrap_or("test").to_owned();
    let thread = std::thread::Builder::new()
        .name(name)
        .stack_size(GAME_STACK_BYTES)
        .spawn(body)
        .expect("start the test's thread");
    if let Err(panic) = thread.join() {
        std::panic::resume_unwind(panic);
    }
}

#[cfg(all(test, tests_scenario))]
mod tests {
    #[test]
    fn shared_rng_states_read_back_in_place_without_the_table() {
        let state =
            serde_json::json!({"disabled": false, "index_a": 0, "index_b": 31, "state": [7]});
        let mut document = serde_json::json!({
            "rows": [{"rng_before": {"rng_state": 0}, "rng_after": {"rng_state": 0}}],
            "shared_rng_states": [state.clone()],
        });
        super::expand_shared_rng_states("inline", &mut document);
        assert_eq!(
            document,
            serde_json::json!({"rows": [{"rng_before": state.clone(), "rng_after": state}]})
        );
    }

    #[test]
    fn a_corpus_with_its_own_rng_states_table_reads_back_unchanged() {
        // pending_production.json keeps its own `rng_states`, keyed by checksum.
        let document = serde_json::json!({
            "rng_states": {"1234": {"disabled": false, "index_a": 0, "index_b": 31}},
            "rows": [{"rng_checksum": 1234}],
        });
        let mut expanded = document.clone();
        super::expand_shared_rng_states("inline", &mut expanded);
        assert_eq!(expanded, document);
    }
}
