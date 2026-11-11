use super::CarryallTote;
use crate::sim::command::Command;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::Simulation;

/// A Carryall (`[HORNET]`'s Fly locomotor with `Carryall=yes`) and two
/// tanks, with `carry` and `tank` added to their sections (an INI's later
/// copy of a section adds nothing).
fn rules(carry: &str, tank: &str) -> String {
    format!(
        "[AircraftTypes]\n0=HORNET\n1=CARRY\n\
         [CARRY]\nCarryall=yes\nLandable=yes\nSpeed=12\nSpeedType=Winged\nStrength=75\n\
         Locomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\n{carry}\
         [VehicleTypes]\n0=TANK\n1=BIGTANK\n\
         [TANK]\nStrength=300\nSpeed=4\nSpeedType=Track\nSize=2\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n{tank}\
         [BIGTANK]\nStrength=300\nSpeed=4\nSpeedType=Track\nSize=3\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n"
    )
}

struct Fixture {
    sim: Simulation,
    rules: crate::rules::ruleset::RuleSet,
    registry: crate::rules::overlay_types::OverlayTypeRegistry,
}

impl Fixture {
    fn new(carry: &str, tank: &str) -> Self {
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules(&rules(carry, tank));
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
        sim.session.set_current_house_for_test(Some(owner));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session.game_options_mut_for_test().short_game = false;
        Self {
            sim,
            rules,
            registry,
        }
    }

    fn spawn(&mut self, name: &str, rx: u16, ry: u16) -> u64 {
        self.sim
            .spawn_object_with_overlay_registry(
                name,
                "Americans",
                rx,
                ry,
                0,
                &self.rules,
                &self.registry,
            )
            .unwrap()
    }

    fn order(&mut self, command: Command) {
        assert!(
            self.sim
                .apply_command("Americans", &command, Some(&self.rules))
        );
    }

    /// Ticks until `done`, at most `frames`; whether it got there.
    fn run_until(&mut self, frames: u32, done: impl Fn(&Simulation) -> bool) -> bool {
        let grid = self.sim.path_grid.clone();
        for _ in 0..frames {
            if done(&self.sim) {
                return true;
            }
            self.sim.advance_tick(
                &[],
                Some(&self.rules),
                grid.as_deref(),
                Some(&self.registry),
                67,
            );
        }
        done(&self.sim)
    }
}

fn carried(sim: &Simulation, carryall: u64, unit: u64) -> bool {
    sim.substrate
        .entities
        .get(carryall)
        .and_then(|entity| entity.passenger_role.cargo())
        .is_some_and(|hold| hold.passengers() == [unit])
}

/// The Tote order (Move onto the Unit, `0x00417BD0`) flies the Carryall to
/// the tank, which WANT_RIDE and HOLD_STILL stop; state 3 lands on it and
/// takes it aboard (Limbo, IsOnCarryall, CargoClass). A Move order then
/// flies it away and sets the tank down under it (`0x00416AF0`).
#[test]
fn carryall_totes_an_allied_tank_and_sets_it_down() {
    let mut fx = Fixture::new("", "");
    let carryall = fx.spawn("CARRY", 15, 15);
    let tank = fx.spawn("TANK", 18, 15);
    assert_eq!(
        fx.sim.carryall_tote_action(carryall, tank, &fx.rules),
        Some(CarryallTote::Tote)
    );
    fx.order(Command::MoveToObject {
        entity_id: carryall,
        target_id: tank,
    });
    assert!(
        fx.run_until(1500, |sim| carried(sim, carryall, tank)),
        "the Carryall picks the tank up"
    );
    let cargo = fx.sim.substrate.entities.get(tank).unwrap();
    assert!(cargo.lifecycle.in_limbo());
    assert!(cargo.is_on_carryall());
    // `AircraftClass::Draw_It` draws it under the Carryall (`0x0041461D`).
    assert_eq!(fx.sim.carryall_drawn_cargo(carryall, &fx.rules), Some(tank));
    assert_eq!(fx.sim.carryall_drawn_cargo(tank, &fx.rules), None);

    fx.order(Command::Move {
        entity_id: carryall,
        target_rx: 21,
        target_ry: 15,
    });
    assert!(
        fx.run_until(1500, |sim| {
            !sim.substrate
                .entities
                .get(tank)
                .unwrap()
                .lifecycle
                .in_limbo()
        }),
        "the Carryall sets the tank down"
    );
    let cargo = fx.sim.substrate.entities.get(tank).unwrap();
    assert!(!cargo.is_on_carryall());
    assert_eq!(fx.sim.carryall_drawn_cargo(carryall, &fx.rules), None);
    assert!(
        cargo.position.rx().abs_diff(21) <= 1 && cargo.position.ry().abs_diff(15) <= 1,
        "set down at the Move's cell: {:?}",
        cargo.position
    );
    assert_eq!(
        fx.sim
            .substrate
            .entities
            .get(carryall)
            .unwrap()
            .passenger_role
            .passenger_count(),
        0
    );
}

/// Loaded, the Carryall's Unload (the self-click, ClickedAction case 4)
/// waits only for its locomotor to stop (`0x0041549D`), though it rests at
/// its loaded landing base, then sets the tank down (`0x004154E7`).
#[test]
fn a_loaded_carryall_sets_its_cargo_down_on_unload() {
    let mut fx = Fixture::new("", "");
    let carryall = fx.spawn("CARRY", 15, 15);
    let tank = fx.spawn("TANK", 18, 15);
    fx.order(Command::MoveToObject {
        entity_id: carryall,
        target_id: tank,
    });
    assert!(fx.run_until(1500, |sim| carried(sim, carryall, tank)));
    fx.order(Command::UnloadPassengers {
        transport_id: carryall,
    });
    assert!(
        fx.run_until(600, |sim| {
            !sim.substrate
                .entities
                .get(tank)
                .unwrap()
                .lifecycle
                .in_limbo()
        }),
        "the Carryall sets the tank down"
    );
    let cargo = fx.sim.substrate.entities.get(tank).unwrap();
    assert!(!cargo.is_on_carryall());
    assert!(
        cargo.position.rx().abs_diff(18) <= 1 && cargo.position.ry().abs_diff(15) <= 1,
        "set down where it was lifted: {:?}",
        cargo.position
    );
}

/// WANT_RIDE, Unit `0x0073745C`: ROGER from a free Unit, NEGATORY once it
/// is tethered (`+0x418`).
#[test]
fn a_tethered_unit_refuses_want_ride() {
    let mut fx = Fixture::new("", "");
    let carryall = fx.spawn("CARRY", 15, 15);
    let tank = fx.spawn("TANK", 18, 15);
    let want_ride = |fx: &mut Fixture| {
        crate::sim::radio::transmit(
            &mut fx.sim,
            carryall,
            tank,
            RadioMessage::WantRide,
            RadioPayload::default(),
            &fx.rules,
            Default::default(),
        )
    };
    assert_eq!(want_ride(&mut fx), RadioResponse::Roger);
    fx.sim
        .substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .tether
        .set_for_test(Some(carryall));
    assert_eq!(want_ride(&mut fx), RadioResponse::Negatory);
}

/// Ares 3.0 CarryallCanLift (1003D9E0): `Carryall.SizeLimit=2` lifts the
/// `Size=2` tank but not the `Size=3` one, and `Carryall.Allowed=no` on a
/// type refuses it; a refusal keeps the base action (hook 0x417D75).
#[cfg(feature = "ares")]
#[test]
fn ares_carryall_keys_gate_the_tote() {
    let mut fx = Fixture::new("Carryall.SizeLimit=2\n", "Carryall.Allowed=yes\n");
    let carryall = fx.spawn("CARRY", 15, 15);
    let tank = fx.spawn("TANK", 18, 15);
    let big = fx.spawn("BIGTANK", 18, 17);
    assert_eq!(
        fx.sim.carryall_tote_action(carryall, tank, &fx.rules),
        Some(CarryallTote::Tote)
    );
    assert_eq!(
        fx.sim.carryall_tote_action(carryall, big, &fx.rules),
        Some(CarryallTote::Kept)
    );
    let mut fx = Fixture::new("", "Carryall.Allowed=no\n");
    let carryall = fx.spawn("CARRY", 15, 15);
    let tank = fx.spawn("TANK", 18, 15);
    assert_eq!(
        fx.sim.carryall_tote_action(carryall, tank, &fx.rules),
        Some(CarryallTote::Kept)
    );
}
