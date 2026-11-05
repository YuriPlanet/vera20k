//! Host-level tests: loading, dispatch, error isolation and lifecycle.
//!
//! These run the real `mlua` VM against throwaway mod trees on disk; nothing
//! here needs a simulation or a window.

use std::fs;
use std::path::PathBuf;

use super::{LuaHost, LuaPaths};

/// A temporary game root carrying `mods/` and `scripts/active_mods.txt`.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("vera20k-lua-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create the temporary game root");
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("file has a parent")).expect("create mod dirs");
        fs::write(path, contents).expect("write the mod file");
    }

    fn host(&self) -> LuaHost {
        LuaHost::new(LuaPaths {
            game_root: Some(self.root.clone()),
            settings_root: None,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn active_mod_receives_committed_frames() {
    let fixture = Fixture::new("frames");
    fixture.write("scripts/active_mods.txt", "example\n");
    fixture.write(
        "mods/example/main.lua",
        r#"
        local Mod = {}
        function Mod.Update(frame)
            Engine.PrintMessage("frame " .. frame)
        end
        return Mod
        "#,
    );

    let mut host = fixture.host();
    assert!(host.take_messages().is_empty(), "loading queues no line");

    host.on_frame(3);
    assert_eq!(host.take_messages(), vec!["frame 3".to_string()]);

    host.on_frame(4);
    assert_eq!(host.take_messages(), vec!["frame 4".to_string()]);
    assert_eq!(
        host.mods().collect::<Vec<_>>(),
        vec![("example", false)],
        "one active mod, still enabled"
    );
}

#[test]
fn scenario_start_is_a_global_hook() {
    let fixture = Fixture::new("scenario");
    fixture.write("scripts/active_mods.txt", "example\n");
    fixture.write(
        "mods/example/main.lua",
        r#"
        local Mod = {}
        function OnScenarioStart()
            Engine.PrintMessage("start")
        end
        function Mod.Update() end
        return Mod
        "#,
    );

    let mut host = fixture.host();
    host.on_scenario_start();
    assert_eq!(host.take_messages(), vec!["start".to_string()]);
    // A loaded save re-fires the same hook on the same VM.
    host.on_scenario_start();
    assert_eq!(host.take_messages(), vec!["start".to_string()]);
}

#[test]
fn broken_mod_does_not_stop_the_next_one() {
    let fixture = Fixture::new("isolation");
    fixture.write("scripts/active_mods.txt", "aaa_broken\nbbb_healthy\n");
    fixture.write(
        "mods/aaa_broken/main.lua",
        r#"
        local Mod = {}
        function Mod.Update()
            error("deliberate failure")
        end
        return Mod
        "#,
    );
    fixture.write(
        "mods/bbb_healthy/main.lua",
        r#"
        local Mod = {}
        function Mod.Update()
            Engine.PrintMessage("ok")
        end
        return Mod
        "#,
    );

    let mut host = fixture.host();
    for frame in 0..12 {
        host.on_frame(frame);
    }

    // The healthy mod kept running every frame...
    let messages = host.take_messages();
    assert_eq!(messages.len(), 12);
    assert!(messages.iter().all(|line| line == "ok"));
    // ...and the broken one was switched off instead of failing forever.
    let mods: Vec<_> = host.mods().collect();
    assert_eq!(mods, vec![("aaa_broken", true), ("bbb_healthy", false)]);
}

#[test]
fn missing_mod_is_skipped_without_stopping_the_host() {
    let fixture = Fixture::new("missing");
    fixture.write("scripts/active_mods.txt", "absent\n");
    fixture.write(
        "mods/present/main.lua",
        r#"
        local Mod = {}
        function Mod.Update()
            Engine.PrintMessage("present")
        end
        return Mod
        "#,
    );

    let mut host = fixture.host();
    host.on_frame(0);
    assert!(host.take_messages().is_empty());

    // The host is still usable for the next scenario and the next mod list.
    host.on_scenario_start();
    assert!(host.mods().next().is_none());
}

#[test]
fn no_active_mods_file_means_no_scripts() {
    let fixture = Fixture::new("empty");
    let mut host = fixture.host();
    host.on_frame(0);
    host.on_scenario_start();
    assert!(host.take_messages().is_empty());
    assert!(host.mods().next().is_none());
}

/// Step-1 probe: the repo-shipped `mods/lua_probe` loads through the real
/// loader and its global `OnScenarioStart` reaches the HUD queue. Runs against
/// the crate root, so it also proves the retail `game_root/mods/active_mods.txt`
/// is discovered first (see loader.rs candidate order).
#[test]
fn repo_probe_mod_prints_on_scenario_start() {
    let mut host = LuaHost::new(LuaPaths {
        game_root: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        settings_root: None,
    });
    host.on_scenario_start();
    let messages = host.take_messages();
    assert!(
        messages.iter().any(|line| line == "probe online"),
        "lua_probe OnScenarioStart did not reach the HUD queue: {messages:?}"
    );
}
