# Autoplay

The auto-play stack: an agent that drives the local player's house.

This folder is the policy crate's documentation. `../src/` is the code;
`../../docs/autoplay.md` is the actuation-layer specification and
`../../docs/ai-layers.md` is the layer table.

## Read in this order

| Document | What it answers |
|---|---|
| [status.md](status.md) | Where the policy stands today: what runs, what is measured, what is broken. |
| [handover.md](handover.md) | How to build, test and run it on this machine, and which landmines are already known. |
| [plan.md](plan.md) | The boundary this line holds to, and what is next in what order. |
| [spec.md](spec.md) | The acceptance criteria: every requirement and its observable test. |
| [acceptance.md](acceptance.md) | How a match is scored, and what the reference replays are for. |
| [reference-1v29-win.md](reference-1v29-win.md) | A human 1v29 win read back as a reference. |
| [matrix.md](matrix.md) | Running a whole matrix: factions x opponents x maps, the CSV, and the pivot tables. |
| [benchmark-v42.1.md](benchmark-v42.1.md) | Fixed V41 seeds, durable staged regression runs, same-engine A/B and recovery diagnostics. |
| [split-orders.md](split-orders.md) | The plan for splitting `assist::orders`: scope, the function-by-function target tree, and why the eight-way split was rejected. Not applied. |

## history/

Everything in [`history/`](history/) is **frozen**. It is the record of how
the current state was reached: fourteen numbered rounds of measurement, ten
investigations that each settled a question, and the superseded v2-era plans.
It is kept because a measurement that is not written down gets repeated — but
nothing there describes the code as it is now, and none of it is an
acceptance source. Where `history/` and `status.md` disagree, `status.md`
wins.

## Naming

The engine's decision layer is `sim::strategy::assist`, and its brain is
`AssistBrain`. It was called `retail` until 2026-11-20; the rename is
recorded at the top of `src/sim/strategy/assist/mod.rs`. A "retail" in this
repository otherwise always means the original game — retail INIs, the retail
install, retail oracles — and never this module.
