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
pub fn json(path: &'static str) -> &'static serde_json::Value {
    fixture(path).json.get_or_init(|| {
        serde_json::from_str(text(path))
            .unwrap_or_else(|error| panic!("test fixture {path} is not JSON: {error}"))
    })
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
/// one build; a build that panics is retried.
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
            .or_insert_with(|| Box::new(BTreeMap::<K, Arc<OnceLock<Option<T>>>>::new()))
            .downcast_mut::<BTreeMap<K, Arc<OnceLock<Option<T>>>>>()
            .expect("slot map type follows its TypeId");
        Arc::clone(by_key.entry(key).or_default())
    };
    slot.get_or_init(load).clone()
}
