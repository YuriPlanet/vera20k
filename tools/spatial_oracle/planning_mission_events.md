# Planning Mode special mission records

Reproduce the retained controls from the repository root, with the supported
retail executable selected as described in [native setup](../native_oracle.md):

```sh
python -m tools.spatial_oracle.planning_mission_events --check
```

`planning_mission_events.py` reuses `KeyboardFixture` for the VM and allocator.
It executes Event construction at `0x4C6860` and plan publication at `0x637DD0`,
stopping at entry to enqueue `0x6521C0`. Twelve controls cover existing
Attack/Move/AreaGuard/AttackMove and Enter7, Capture8, Eaten9, Harvest10,
Unload16 (NULL or building-self Target), Sabotage17 and Patrol25.
The publication wrapper changes only the planning byte at event offset `0x1D`.
The adjacent metadata records the supplied tokens. This is record
construction/copy evidence, not complete gameplay or input admission parity.
`sim::command::tests::destination_missions_preserve_native_constructor_and_planning_fields`
consumes these native outputs. It excludes the numeric value of a NULL auxiliary
token; the existing writer retains its source-template UID there.

Instruction evidence from the same supported executable establishes the active
click route:

| Input | Original producer | Mission and tokens |
| --- | --- | --- |
| C4 on a building | Foot `4D7788..4D77A8` | Sabotage17, NULL Target, building Destination |
| Capture/infiltrate/garrison | Foot `4D7728..4D773A` | Capture8, NULL Target, object Destination |
| Enter transport/bunker/depot/refinery | Foot `4D76F6..4D7701` | Enter7, NULL Target, object Destination |
| Grinder action | Foot `4D7761..4D7773` | Eaten9, NULL Target, object Destination |
| Harvest ground | Foot `4D7E8E..4D7EA4` | Harvest10, NULL Target, Cell Destination |
| Loaded building self-click | Building `443534..443562` | Unload16, self Target, NULL Destination |
| Mobile self-click | Foot `4D75E1` | Unload16, all tokens NULL |

`Player_Send_Command6FFBE0` checks `637AA0` at `6FFC23`; its planning branch
retains the mission/tokens, constructs the event and publishes through `637DD0`.
Committed records use the existing Event receiver `4C71F6..4C74BA`; no separate
C4, capture, cargo or docking state belongs to the planner. Ordinary and
planned Enter share the class setters' Enter arms, including tank-bunker
destination and radio contact handling. The joined original-code controls in
[bunker_entry.md](bunker_entry.md) cover that route separately from the planner.

Special-event distinctions:

- The D key sends opcode9 through virtual `+374` (`730B5C`, `730D0F`), whose
  Building/Techno slot is `6FFE00`. Its planning branch calls `639FD0`; the
  opcode9 arm `639FD2..639FFB` selects `MSG:PlanningModeNoDeploy` at `836C34`.
  It refuses deployment, including passenger unloading through the D key.
- Three NULL mission tokens resolve to `(-1,-1,-1)` at `633C3A..633C55`;
  node admission `638158..63816B` rejects that sentinel. Mobile self-clicks
  therefore do not gain an invented actor-position node or immediate fallback.
- Building primary selection, rally/archive and cell-driven sell/repack use
  direct events (`443410`, `443637..443683`, `4436F0`), rather than the mission
  publication route. Existing ordinary event scheduling is retained.

The audit covers the already-resolved context commands entering
`app::input::planning_mode::queue_orders_in_sim`, shared record decoding,
committed dispatch, save/restore, shared-node cleanup and native refusal routes.
Move, object/forced Attack (including weapon-based special actions), attack-move
and AreaGuard continue through their existing codecs. It does not establish
full-window rendering parity, all map/mode combinations or the general Planning
Mode visual/cluster work in issue #1273. The pre-existing hospital/grinder input
producer ([#2022](https://github.com/YuriPlanet/vera20k/issues/2022)) and tank-bunker
self-eject input/native mapping need separate work;
they are not silently executed as immediate orders while planning.

The earlier no-refinery probe in [#2024](https://github.com/YuriPlanet/vera20k/issues/2024)
was not a harvesting defect: original `73E67F..73E6C2` queues Guard when the
house owns no instance of any `Dock=` type. The retail frame regression now
covers actual ore extraction with an owned refinery, plus no-refinery and
other-house refinery controls, for ordinary and planned Harvest orders.
