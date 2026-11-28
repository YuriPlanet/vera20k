//! Regression tests for the JSON Lines control module.
//!
//! The session under test runs on an empty simulation: the protocol and the
//! order gate are what these cover, and neither needs retail files.

use std::io::Cursor;

use crate::sim::runtime::SimRuntime;
use crate::sim::world::Simulation;

use super::command::{Order, Request};
use super::observe::Event;
use super::protocol::{self, Outcome};
use super::session::Session;

/// A session over an empty match with one local house.
fn session() -> Session {
    let mut sim = Simulation::new();
    let house = sim.interner_mut_for_test().intern("Player");
    Session::from_runtime(SimRuntime::from_simulation(sim), house, None)
}

/// The events the protocol writes for `input`, read back as JSON objects.
fn events(input: &str) -> (Vec<serde_json::Value>, Outcome) {
    let mut session = session();
    let mut output: Vec<u8> = Vec::new();
    let outcome = protocol::run(&mut session, Cursor::new(input.as_bytes()), &mut output)
        .expect("the protocol loop runs");
    let text = String::from_utf8(output).expect("the protocol writes UTF-8");
    let events = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("every line is a JSON object"))
        .collect();
    (events, outcome)
}

#[test]
fn every_request_line_answers_one_event() {
    let (events, outcome) = events("{\"cmd\":\"state\"}\n{\"cmd\":\"quit\"}\n");
    assert_eq!(events.len(), 2, "one event per request line");
    assert_eq!(events[0]["event"], "state");
    assert_eq!(events[1]["event"], "finished");
    assert_eq!(events[1]["reason"], "quit");
    assert_eq!(outcome, Outcome::Finished);
}

#[test]
fn request_limit_counts_the_newline() {
    let quit = r#"{"cmd":"quit"}"#;
    let padding = " ".repeat(protocol::MAX_LINE_BYTES - quit.len() - 1);
    let (accepted, outcome) = events(&format!("{quit}{padding}\n"));
    assert_eq!(outcome, Outcome::Finished);
    assert_eq!(accepted[0]["reason"], "quit");

    let (refused, outcome) = events(&format!("{quit}{padding} \n{quit}\n"));
    assert!(matches!(outcome, Outcome::Failed(_)));
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0]["fatal"], true);
}

#[test]
fn a_blank_line_is_not_a_request() {
    let (events, _) = events("\n   \n{\"cmd\":\"quit\"}\n");
    assert_eq!(events.len(), 1, "blank lines carry no request");
    assert_eq!(events[0]["event"], "finished");
}

#[test]
fn a_malformed_line_answers_an_error_and_the_match_goes_on() {
    let (events, outcome) = events("{\"cmd\":\"nope\"\n{\"cmd\":\"quit\"}\n");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], "error");
    assert_eq!(events[0]["fatal"], false, "a bad request is not fatal");
    assert_eq!(events[1]["event"], "finished");
    assert_eq!(outcome, Outcome::Finished);
}

#[test]
fn the_end_of_the_input_finishes_the_match() {
    let (events, outcome) = events("");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], "finished");
    assert_eq!(events[0]["reason"], "eof");
    assert_eq!(outcome, Outcome::Finished);
}

#[test]
fn a_quit_ends_the_loop_before_the_next_request() {
    let (events, _) = events("{\"cmd\":\"quit\"}\n{\"cmd\":\"state\"}\n");
    assert_eq!(events.len(), 1, "the loop stops at the finished match");
    assert_eq!(events[0]["event"], "finished");
}

#[test]
fn an_order_for_an_unknown_object_is_refused() {
    let mut session = session();
    let event = session.handle(Request::Order {
        order: Order::Move {
            unit: 4242,
            x: 0,
            y: 0,
        },
    });
    match event {
        Event::Error { message, fatal } => {
            assert!(!fatal);
            assert!(
                message.contains("4242"),
                "the refusal names the object: {message}"
            );
        }
        other => panic!("a move for an absent unit must not be queued: {other:?}"),
    }
}

#[test]
fn a_rally_order_needs_a_producer() {
    let mut session = session();
    let event = session.handle(Request::Order {
        order: Order::Rally {
            x: 0,
            y: 0,
            producers: Vec::new(),
        },
    });
    match event {
        Event::Error { message, .. } => {
            assert!(
                message.contains("producer"),
                "the refusal explains why: {message}"
            );
        }
        other => panic!("a rally order without a producer must not be queued: {other:?}"),
    }
}

#[test]
fn a_step_beyond_the_limit_is_refused() {
    let mut session = session();
    let event = session.handle(Request::Tick { ticks: 0 });
    match event {
        Event::Error { message, .. } => {
            assert!(
                message.contains("ticks"),
                "the refusal explains why: {message}"
            );
        }
        other => panic!("a step of no ticks must not be taken: {other:?}"),
    }
}
