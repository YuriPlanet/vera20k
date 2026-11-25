//! Client modules' side of the simulation (RFC 0001 section 10, phase 2):
//! messages into `on_message` through the command stream and their limits,
//! `send_notify` into the client outbox, forwarded `SimEvent`s, notification
//! IDs and the global `AutomationAllowed`. The retail tests return early
//! without `RA2_DIR`.

use super::tests::{mem_i32, mem_i64, scenario};
use super::{ModuleSource, house_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::map::entities::EntityCategory;
use crate::sim::command::{Command, CommandEnvelope, ModuleMessageRecord};
use crate::sim::intern::InternedId;

fn escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// `vera.delivery`: 240 bytes at 4096.
fn delivery() -> String {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&4096u32.to_le_bytes());
    bytes.extend_from_slice(&240u32.to_le_bytes());
    escape(&bytes)
}

/// `vera.notify` naming `names`.
fn notify(names: &[&str]) -> String {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for name in names {
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }
    escape(&bytes)
}

/// `on_message` counts messages at 8, keeps the sender at 16, the length at
/// 24 and the first byte at 28; `on_frame` copies the count to 40, so 40
/// equals 8 on the frame a message arrives only if `on_message` ran first.
fn receiver() -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
  (@custom "vera.delivery" "{delivery}")
  (memory (export "memory") 1)
  (func (export "on_message") (param $sender i64) (param $ptr i32) (param $len i32)
    (i32.store (i32.const 8) (i32.add (i32.load (i32.const 8)) (i32.const 1)))
    (i64.store (i32.const 16) (local.get $sender))
    (i32.store (i32.const 24) (local.get $len))
    (i32.store (i32.const 28) (i32.load8_u (local.get $ptr))))
  (func (export "on_frame")
    (i32.store (i32.const 40) (i32.load (i32.const 8)))))"#,
        delivery = delivery(),
    ))
    .expect("test module parses")
}

/// A house of the map and one of its units.
fn house_and_unit(scene: &HeadlessScenario) -> (InternedId, u64) {
    scene
        .sim()
        .entities()
        .iter_sorted()
        .find(|(_, e)| {
            e.category() == EntityCategory::Unit
                && e.lifecycle.object_alive()
                // Only a house the DoList serves executes its records.
                && scene
                    .sim()
                    .houses
                    .get(&e.owner())
                    .is_some_and(|house| house.event_dispatch_eligible())
        })
        .map(|(id, e)| (e.owner(), id))
        .expect("the map has a unit")
}

fn house_index(scene: &HeadlessScenario, house: InternedId) -> usize {
    scene
        .sim()
        .houses
        .index_of(house)
        .expect("registered house")
}

#[test]
fn a_message_record_carries_its_source_and_bytes() {
    let message = ModuleMessageRecord {
        house_id: 2,
        frame: 40,
        source: 1,
        data: vec![1, 2, 3],
    };
    let record = message.encode().expect("encodes");
    assert_eq!(ModuleMessageRecord::decode(&record), Some(message.clone()));
    // A message is at most 96 bytes, from source 0 or 1.
    let long = ModuleMessageRecord {
        data: vec![0; 97],
        ..message.clone()
    };
    assert!(long.encode().is_none());
    let third = ModuleMessageRecord {
        source: 2,
        ..message.clone()
    };
    assert!(third.encode().is_none());
    let full = ModuleMessageRecord {
        data: vec![9; 96],
        ..message
    };
    let record = full.encode().expect("96 bytes encode");
    assert_eq!(ModuleMessageRecord::decode(&record), Some(full));
    assert_eq!(
        crate::sim::command::event_body_len(crate::sim::command::VERA_MODULE_MESSAGE_OPCODE),
        Some(98)
    );
}

/// A message travels as a player's record, reaches the same source's
/// `on_message` with the sender's house and its bytes, before `on_frame`.
#[test]
fn a_message_reaches_on_message_before_on_frame() {
    let Some(mut scene) = scenario() else { return };
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &receiver(),
        )
        .expect("module installs");
    let (house, _) = house_and_unit(&scene);
    let sim = &mut scene.runtime.simulation;
    let record = sim
        .encode_module_message_record(house, 1, &[42, 7])
        .expect("the message encodes");
    let decoded = sim
        .decode_native_command_record(&record, sim.session.tick())
        .expect("the record decodes");
    assert_eq!(decoded.owner, house);
    assert_eq!(
        decoded.payload,
        Command::ModuleMessage {
            source: 1,
            data: vec![42, 7]
        }
    );
    let tick = sim.session.tick();
    sim.queue_command(CommandEnvelope::synchronized(record, tick));
    for _ in 0..8 {
        scene.tick();

        if mem_i32(&scene, 0, 8) == 1 {
            break;
        }
    }
    assert_eq!(mem_i32(&scene, 0, 8), 1, "on_message ran once");
    assert_eq!(mem_i32(&scene, 0, 40), 1, "on_frame ran after on_message");
    let index = house_index(&scene, house);
    assert_eq!(mem_i64(&scene, 0, 16), house_ref(index));
    assert_eq!(mem_i32(&scene, 0, 24), 2);
    assert_eq!(mem_i32(&scene, 0, 28), 42);
    // Delivered: nothing waits, and the hash forgets the rate counts once
    // their window passed.
    for _ in 0..20 {
        scene.tick();
    }
    assert!(scene.sim().ext_modules.messages.is_empty());
}

/// Two messages per player per source per frame, sixteen in any fifteen
/// frames; a message for a source without `on_message`, a third source or
/// more than 96 bytes is refused.
#[test]
fn messages_past_the_limits_are_dropped() {
    let Some(mut scene) = scenario() else { return };
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &receiver(),
        )
        .expect("module installs");
    let (house, _) = house_and_unit(&scene);
    let send = |scene: &mut HeadlessScenario, source: u8, data: &[u8]| {
        scene
            .runtime
            .simulation
            .receive_module_message(house, source, data)
            .is_ok()
    };
    assert!(!send(&mut scene, 0, b"x"), "no mod module");
    assert!(!send(&mut scene, 2, b"x"), "no third source");
    assert!(!send(&mut scene, 1, &[0; 97]), "too long");
    assert!(send(&mut scene, 1, &[0; 96]));
    let mut accepted = 1;
    // Two a frame, then the window of fifteen frames holds sixteen.
    let mut refused_in_window = false;
    for frame in 0..15 {
        let first = frame > 0 && send(&mut scene, 1, b"a");
        let second = send(&mut scene, 1, b"b");
        let third = send(&mut scene, 1, b"c");
        assert!(!third, "a third message in one frame");
        accepted += usize::from(first) + usize::from(second);
        refused_in_window |= frame > 0 && !first;
        scene.tick();
    }
    assert_eq!(accepted, 16, "sixteen in fifteen frames");
    assert!(refused_in_window);
    // Once the window moved on, messages pass again.
    for _ in 0..15 {
        scene.tick();
    }
    assert!(send(&mut scene, 1, b"d"));
}

/// A module sends a declared notification to a house: the client outbox
/// holds it, at most 256 a frame, and the state hash does not see it.
#[test]
fn notifications_go_to_the_client_outbox() {
    let Some(mut scene) = scenario() else { return };
    let (house, _) = house_and_unit(&scene);
    let to = house_ref(house_index(&scene, house));
    let id = vera20k_wasm_ext::notify_id("Bought");
    // `on_frame` sends 300 notifications, counting the `Ok`s at 8.
    let sender = |send: bool| {
        wat::parse_str(format!(
            r#"(module
  (import "vera:sim@1.0" "send_notify" (func $send (param i64 i32 i32 i32) (result i32)))
  (@custom "vera.notify" "{notify}")
  (memory (export "memory") 1)
  (data (i32.const 1100) "abc")
  (func (export "on_frame") (local $i i32)
    (if (i32.eqz (i32.const {send})) (then (return)))
    (block $done (loop $next
      (br_if $done (i32.ge_u (local.get $i) (i32.const 300)))
      (if (i32.eqz (call $send (i64.const {to}) (i32.const {id}) (i32.const 1100) (i32.const 3)))
        (then (i32.store (i32.const 8) (i32.add (i32.load (i32.const 8)) (i32.const 1)))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $next)))))"#,
            notify = notify(&["Bought"]),
            send = i32::from(send),
        ))
        .expect("test module parses")
    };
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &sender(true),
        )
        .expect("module installs");
    assert_eq!(super::notify_id(scene.sim(), "BOUGHT"), Some(id));
    assert_eq!(super::notify_id(scene.sim(), "Sold"), None);
    scene.runtime.simulation.take_client_outbox();
    scene.tick();
    assert_eq!(mem_i32(&scene, 0, 8), 256);
    let outbox = scene.runtime.simulation.take_client_outbox();
    assert_eq!(outbox.notifies.len(), 256);
    let first = &outbox.notifies[0];
    assert_eq!(
        (first.to, first.source, first.id, first.data.as_slice()),
        (to, vera20k_wasm_ext::client::Source::Map, id, &b"abc"[..])
    );
    assert!(scene.runtime.simulation.take_client_outbox().is_empty());
}

/// With a module installed, a player's order raises `Commanded` for the
/// client outbox though no trigger listens; without modules nothing is
/// built.
#[test]
fn forwarded_events_reach_the_outbox_only_with_modules() {
    let Some(mut plain) = scenario() else { return };
    let Some(mut scene) = scenario() else { return };
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &receiver(),
        )
        .expect("module installs");
    for run in [&mut plain, &mut scene] {
        let (house, unit) = house_and_unit(run);
        let sim = &mut run.runtime.simulation;
        let record = sim.encode_stop_record(house, unit).expect("Stop encodes");
        let tick = sim.session.tick();
        sim.queue_command(CommandEnvelope::synchronized(record, tick));
        for _ in 0..4 {
            run.tick();
        }
    }
    assert!(plain.runtime.simulation.take_client_outbox().is_empty());
    let (house, unit) = house_and_unit(&scene);
    let outbox = scene.runtime.simulation.take_client_outbox();
    let commanded = outbox
        .sim_events
        .iter()
        .find(|event| {
            i32::from_le_bytes(event.record[4..8].try_into().unwrap())
                == vera20k_wasm_ext::triggers::EVENT_COMMANDED
        })
        .expect("Commanded was forwarded");
    let index = house_index(&scene, house);
    assert_eq!(commanded.house, house_ref(index));
    assert_eq!(
        i64::from_le_bytes(commanded.record[24..32].try_into().unwrap()),
        unit as i64
    );
    // Forwarding built nothing the simulation keeps: both runs agree on
    // everything but the module's own state.
    assert_eq!(
        plain.sim().trigger_runtime.module_triggers().kinds(),
        scene.sim().trigger_runtime.module_triggers().kinds()
    );
}

/// The outbox keeps the newest entries when nobody drains it.
#[test]
fn an_undrained_outbox_keeps_the_newest() {
    use super::client_outbox::{ClientNotify, ClientOutbox, MAX_KEPT};
    let mut outbox = ClientOutbox::default();
    for id in 0..(MAX_KEPT as i32 * 2) {
        outbox.push_notify(ClientNotify {
            to: 0,
            source: vera20k_wasm_ext::client::Source::Mod,
            id,
            data: Vec::new(),
        });
    }
    assert!(outbox.notifies.len() <= MAX_KEPT + MAX_KEPT / 4);
    assert!(outbox.notifies.len() >= MAX_KEPT);
    assert_eq!(
        outbox.notifies.last().map(|n| n.id),
        Some(MAX_KEPT as i32 * 2 - 1)
    );
}

/// The rate counts: two per frame, sixteen in fifteen frames, per house
/// and source; they are forgotten after the window.
#[test]
fn the_message_rate_counts_per_house_and_source() {
    let mut messages = super::messages::ClientMessages::default();
    assert!(messages.admit(0, ModuleSource::Map, 10));
    assert!(messages.admit(0, ModuleSource::Map, 10));
    assert!(!messages.admit(0, ModuleSource::Map, 10));
    // Another house and another source count apart.
    assert!(messages.admit(1, ModuleSource::Map, 10));
    assert!(messages.admit(0, ModuleSource::Mod, 10));
    let mut accepted = 2;
    for frame in 11..25 {
        accepted += usize::from(messages.admit(0, ModuleSource::Map, frame));
        accepted += usize::from(messages.admit(0, ModuleSource::Map, frame));
    }
    assert_eq!(accepted, 16);
    // Frame 25: frame 10's two left the window.
    assert!(messages.admit(0, ModuleSource::Map, 25));
    messages.prune(100);
    assert!(messages.is_empty());
    let round: super::messages::ClientMessages =
        serde_json::from_str(&serde_json::to_string(&messages).unwrap()).unwrap();
    assert_eq!(round, messages);
}

/// `AutomationAllowed` reads the room switch: 0 by default.
#[test]
fn automation_allowed_reads_the_room_switch() {
    use crate::skirmish_launch::HostRules;
    let mut own = crate::rules::personal_rules::PersonalRules::default();
    assert!(own.compatibility_crc().is_none());
    own.set_automation_allowed(true);
    assert!(own.compatibility_crc().is_some());
    let room = HostRules::from_own(&own);
    assert!(!room.is_empty());
    assert!(room.personal_rules().unwrap().automation_allowed());
    assert!(
        !HostRules::default()
            .personal_rules()
            .unwrap()
            .automation_allowed()
    );

    let Some(mut scene) = scenario() else { return };
    // `get(global, AutomationAllowed)` into 0 each frame.
    let prop = vera20k_wasm_ext::property_id("AutomationAllowed").expect("property");
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_frame")
    (drop (call $get (i64.const {global}) (i32.const {prop}) (i32.const 0)))))"#,
        global = 4i64 << 48,
    ))
    .expect("test module parses");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &module)
        .expect("module installs");
    scene.tick();
    assert_eq!(mem_i64(&scene, 0, 0), 0);
    scene.runtime.simulation.ext_modules.automation_allowed = true;
    scene.tick();
    assert_eq!(mem_i64(&scene, 0, 0), 1);
    assert!(scene.sim().ext_modules().automation_allowed());
}

/// RFC 0001 section 10's round trip with the SDK example
/// (`sdk/examples/ui-reinforcements`), when `VERA20K_TEST_UI_REINFORCEMENTS`
/// names its `rules.wasm|ui.wasm`: the client module loads; the button's
/// message travels the command stream to the map's `on_message`, which
/// creates one Rhino for the sender at waypoint 3 and notifies its player;
/// a second purchase is refused.
#[test]
fn ui_reinforcements_round_trip() {
    let Some(paths) = std::env::var("VERA20K_TEST_UI_REINFORCEMENTS").ok() else {
        return;
    };
    let (rules_path, ui_path) = paths
        .split_once('|')
        .expect("VERA20K_TEST_UI_REINFORCEMENTS is rules.wasm|ui.wasm");
    let ui = std::fs::read(ui_path.trim()).expect("ui.wasm reads");
    let client = vera20k_wasm_ext::ClientModule::load(&ui, vera20k_wasm_ext::Limits::MAP)
        .unwrap_or_else(|e| panic!("ui.wasm loads: {e}"));
    for export in [
        "client_on_start",
        "client_on_render",
        "client_on_events",
    ] {
        assert!(client.has_export(export), "ui.wasm exports {export}");
    }
    let Some(mut scene) = scenario() else { return };
    // Waypoint 3 is a start of this map, its MCV on the cell, where the
    // module rightly refuses (no tank, no charge). Move the waypoint over
    // free cells beside it until the land admits a Rhino.
    let (x, y) = scene
        .sim()
        .session
        .waypoint_cell(3)
        .expect("the map sets waypoint 3");
    let free: Vec<(u16, u16)> = {
        let sim = scene.sim();
        let (x, y) = (x as u16, y as u16);
        (x.saturating_sub(4)..=x + 4)
            .flat_map(|cx| (y.saturating_sub(4)..=y + 4).map(move |cy| (cx, cy)))
            .filter(|&cell| {
                !sim.terrain_objects.cell_index().contains_key(&cell)
                    && sim
                        .entities()
                        .iter_sorted()
                        .all(|(_, e)| (e.position.rx(), e.position.ry()) != cell)
            })
            .collect()
    };
    let bytes = std::fs::read(rules_path.trim()).expect("rules.wasm reads");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, &bytes)
        .expect("rules.wasm installs");
    let (house, _) = house_and_unit(&scene);
    let rhinos = |scene: &HeadlessScenario| {
        let sim = scene.sim();
        sim.entities()
            .iter_sorted()
            .filter(|(_, e)| {
                e.owner() == house
                    && e.lifecycle.object_alive()
                    && sim.interner.resolve(e.type_ref()) == "HTNK"
            })
            .count()
    };
    let notify = vera20k_wasm_ext::notify_id("Reinforced");
    let to = house_ref(house_index(&scene, house));
    let buy = |scene: &mut HeadlessScenario| {
        let sim = &mut scene.runtime.simulation;
        let record = sim
            .encode_module_message_record(house, 1, &[1])
            .expect("the message encodes");
        let tick = sim.session.tick();
        sim.queue_command(CommandEnvelope::synchronized(record, tick));
        for _ in 0..8 {
            scene.tick();
        }
        let outbox = scene.runtime.simulation.take_client_outbox();
        outbox
            .notifies
            .iter()
            .filter(|n| n.to == to && n.id == notify)
            .count()
    };
    let before = rhinos(&scene);
    let mut notified = 0;
    for &(rx, ry) in &free {
        let drop = crate::map::waypoints::Waypoint { index: 3, rx, ry };
        scene
            .runtime
            .simulation
            .session
            .install_waypoints(&std::collections::HashMap::from([(3, drop)]));
        notified += buy(&mut scene);
        if rhinos(&scene) > before {
            break;
        }
    }
    assert_eq!(rhinos(&scene), before + 1, "one Rhino for one purchase");
    assert_eq!(notified, 1, "the buyer's player is told once");
    // Bought once: a second message changes nothing.
    notified += buy(&mut scene);
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    eprintln!("ui-reinforcements log: {log:?}");
    assert_eq!(scene.sim().ext_modules().status()[0].1, None, "rules.wasm runs");
    assert_eq!(rhinos(&scene), before + 1, "a second purchase is refused");
    assert_eq!(notified, 1);
}
