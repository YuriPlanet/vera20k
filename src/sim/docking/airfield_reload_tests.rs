//! Replays `tools/spatial_oracle/airfield_reload.json`: the original
//! `BuildingClass::Mission_Repair @ 0x0044B780` on a `UnitReload=` airfield,
//! one call per row, through [`reload_visit`].

use super::*;
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/airfield_reload.json",
    ))
    .expect("airfield_reload.json");
    oracle["mission_repair"].as_array().unwrap().clone()
}

/// The oracle's contacts; a contact's id is its position plus one.
const CONTACTS: [&str; 4] = ["a", "b", "c", "d"];

fn name(contact: u64) -> &'static str {
    CONTACTS[contact as usize - 1]
}

/// Answers each call from the row and logs it as the oracle logs the native
/// call or read it stands for: `capacity` is a read of `+0xE8`, `release`
/// the three calls it makes.
struct Recorder<'a> {
    input: &'a Value,
    count: usize,
    mission_reads: [usize; 4],
    calls: Vec<String>,
}

impl<'a> Recorder<'a> {
    fn contact_input(&self, contact: u64) -> &'a Value {
        &self.input["contacts"][name(contact)]
    }
}

impl ReloadHost for Recorder<'_> {
    fn capacity(&mut self) -> usize {
        self.calls.push("count".into());
        self.count
    }

    fn contact(&mut self, slot: usize) -> Option<u64> {
        self.calls.push(format!("contact {slot}"));
        let held = self.input["slots"][slot].as_str()?;
        let index = CONTACTS.iter().position(|contact| *contact == held);
        Some(index.unwrap() as u64 + 1)
    }

    fn roger(&mut self, contact: u64, message: RadioMessage) -> bool {
        let code = format!("0x{:02X}", message.code());
        let contact_name = name(contact);
        self.calls.push(format!("transmit {code} {contact_name}"));
        self.contact_input(contact)["answers"][&code].as_i64()
            == Some(i64::from(RadioResponse::Roger.code()))
    }

    fn full_strength(&mut self, contact: u64) -> bool {
        self.calls.push(format!("full_strength {}", name(contact)));
        self.contact_input(contact)["health"].as_i64() == self.input["strength"].as_i64()
    }

    fn mission(&mut self, contact: u64) -> MissionId {
        self.calls.push(format!("mission {}", name(contact)));
        let missions = self.contact_input(contact)["missions"].as_array().unwrap();
        let read = &mut self.mission_reads[contact as usize - 1];
        let answer = missions[(*read).min(missions.len() - 1)].as_i64().unwrap();
        *read += 1;
        MissionId::from_raw(answer as i32)
    }

    fn queue_sleep(&mut self, contact: u64) {
        let sleep = MissionId::from_known(MissionType::Sleep).raw();
        self.calls
            .push(format!("queue_mission {} {sleep} 0", name(contact)));
    }

    fn release(&mut self, contact: u64) {
        let guard = MissionId::from_known(MissionType::Guard).raw();
        let contact = name(contact);
        self.calls.push(format!("enter_idle_mode {contact} 0 1"));
        self.calls.push(format!("assign_mission {contact} {guard}"));
        self.calls.push(format!("vt334 {contact}"));
        let recount = &self.input["recount"];
        if recount["contact"] == contact {
            self.count = recount["count"].as_u64().unwrap() as usize;
        }
    }

    fn queue_guard(&mut self) {
        let guard = MissionId::from_known(MissionType::Guard).raw();
        self.calls.push(format!("queue_mission airfield {guard} 0"));
    }
}

/// Every row: the call log and the returned frames.
#[test]
fn reload_visit_matches_the_original() {
    let rows = rows();
    assert_eq!(rows.len(), 307);
    let mut disagreements = Vec::new();
    for row in &rows {
        let input = &row["input"];
        let bits = input["reload_rate"].as_str().unwrap();
        let reload_rate = f64::from_bits(u64::from_str_radix(&bits[2..], 16).unwrap());
        let mut host = Recorder {
            input,
            count: input["count"].as_u64().unwrap() as usize,
            mission_reads: [0; 4],
            calls: Vec::new(),
        };
        let frames = reload_visit(reload_rate, &mut host);
        let rust = json!({ "ret": frames, "calls": host.calls });
        let native = json!({ "ret": row["ret"], "calls": row["calls"] });
        if rust != native {
            disagreements.push(format!(
                "{}:\n  native {native}\n  rust   {rust}",
                input["name"]
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} of {} rows disagree:\n{}",
        disagreements.len(),
        rows.len(),
        disagreements.join("\n")
    );
}

/// Retail `[General] ReloadRate=.3`, read through the production reader,
/// is 0.3 scanned as a float and widened (ReadDouble `0x005283D0`), not
/// 0.3's own double: a visit that hands a sleeping contact a round returns
/// 270 frames, as the original's `retail` row does (0.3's double gives 269).
#[test]
fn retail_reload_rate_services_every_270_frames() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(
        rules.general.reload_rate.to_bits(),
        f64::from(0.3_f32).to_bits()
    );

    /// One sleeping contact short of a round.
    struct Reloading;
    impl ReloadHost for Reloading {
        fn capacity(&mut self) -> usize {
            1
        }
        fn contact(&mut self, _slot: usize) -> Option<u64> {
            Some(1)
        }
        fn roger(&mut self, _contact: u64, message: RadioMessage) -> bool {
            matches!(message, RadioMessage::NeedToMove | RadioMessage::Reload)
        }
        fn full_strength(&mut self, _contact: u64) -> bool {
            true
        }
        fn mission(&mut self, _contact: u64) -> MissionId {
            MissionId::from_known(MissionType::Sleep)
        }
        fn queue_sleep(&mut self, _contact: u64) {}
        fn release(&mut self, _contact: u64) {
            panic!("a contact taking a round stays");
        }
        fn queue_guard(&mut self) {
            panic!("a servicing visit queues no Guard");
        }
    }
    assert_eq!(reload_visit(rules.general.reload_rate, &mut Reloading), 270);
}

/// A scripted host for Ares' per-docker visit: each contact keeps its own
/// mission and answers, and the visit's calls are logged.
#[cfg(feature = "ares")]
struct AresDock {
    frame: i32,
    contacts: Vec<Option<u64>>,
    missions: Vec<MissionType>,
    reloads: Vec<bool>,
    timers: DockReloadTimers,
    calls: Vec<String>,
}

#[cfg(feature = "ares")]
impl ReloadHost for AresDock {
    fn capacity(&mut self) -> usize {
        self.contacts.len()
    }

    fn contact(&mut self, slot: usize) -> Option<u64> {
        self.contacts[slot]
    }

    fn roger(&mut self, contact: u64, message: RadioMessage) -> bool {
        self.calls.push(format!("{message:?} {contact}"));
        match message {
            RadioMessage::NeedToMove => true,
            RadioMessage::Reload => self.reloads[contact as usize],
            _ => false,
        }
    }

    fn full_strength(&mut self, _: u64) -> bool {
        true
    }

    fn mission(&mut self, contact: u64) -> MissionId {
        MissionId::from_known(self.missions[contact as usize])
    }

    fn queue_sleep(&mut self, contact: u64) {
        self.calls.push(format!("sleep {contact}"));
        self.missions[contact as usize] = MissionType::Sleep;
    }

    fn release(&mut self, contact: u64) {
        self.calls.push(format!("release {contact}"));
    }

    fn queue_guard(&mut self) {
        self.calls.push("guard".into());
    }
}

#[cfg(feature = "ares")]
impl AresReloadHost for AresDock {
    fn frame(&mut self) -> i32 {
        self.frame
    }

    fn reload_rate(&mut self, contact: u64) -> f64 {
        // 0.1 minutes for contact 0, 0.2 for contact 1: 90 and 180 frames.
        0.1 * (contact + 1) as f64
    }

    fn timers(&mut self) -> &mut DockReloadTimers {
        &mut self.timers
    }
}

/// Ares 10014190 (hook 0x44C844): each docker sleeps on arrival, then
/// reloads on its own type's timer, and the airfield revisits every frame.
#[cfg(feature = "ares")]
#[test]
fn ares_dockers_reload_on_their_own_timers() {
    let mut dock = AresDock {
        frame: 100,
        contacts: vec![Some(0), Some(1)],
        missions: vec![MissionType::Guard, MissionType::Guard],
        reloads: vec![true, true],
        timers: DockReloadTimers::default(),
        calls: Vec::new(),
    };
    assert_eq!(ares_reload_visit(&mut dock), 1);
    assert_eq!(
        dock.calls,
        [
            "QueryReloaded 0",
            "NeedToMove 0",
            "sleep 0",
            "QueryReloaded 1",
            "NeedToMove 1",
            "sleep 1",
        ]
    );
    // Asleep with a cleared timer, which is not this frame: each timer moves
    // on by its own rate and nothing reloads yet.
    dock.frame = 101;
    dock.calls.clear();
    assert_eq!(ares_reload_visit(&mut dock), 1);
    assert_eq!(dock.timers.get(0), 101 + 90);
    assert_eq!(dock.timers.get(1), 101 + 180);
    // At frame 191 only the first docker is due; a refused RELOAD and
    // REPAIR release it.
    dock.frame = 191;
    dock.reloads[0] = false;
    dock.calls.clear();
    assert_eq!(ares_reload_visit(&mut dock), 1);
    assert_eq!(
        dock.calls,
        [
            "QueryReloaded 0",
            "NeedToMove 0",
            "Reload 0",
            "RepairTick 0",
            "release 0",
            "QueryReloaded 1",
            "NeedToMove 1",
        ]
    );
    assert_eq!(dock.timers.get(0), -1);
}
