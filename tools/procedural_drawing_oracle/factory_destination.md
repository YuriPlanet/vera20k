# Factory destination and Stop prerequisite

Native evidence uses active retail `gamemd.exe`.
Regenerate with `python -m tools.procedural_drawing_oracle.factory_destination --write`;
`--check` compares without replacing the evidence. The shared `native_oracle`
loader checks the immutable executable before mapping it. No instructions or
virtual results are substituted by this fixture.

The 110 executed cases establish:

- 65 complete Building destination calls (`455D50`, actual Building vtable
  `7E3EBC+480`): current mission Selling (`19`) returns untouched; otherwise
  HasRallyPoint (`455DA0`) or ConstructionYard (`type+16B9`) sends the requested
  pointer to the sole ArchiveTarget store (`70C610`, `actor+218`). The base
  Techno destination tail (`709A30`) is `RET 8`, with no state effect. Null,
  unchanged and new pointers, mission Guard/Construction/Selling, the factory,
  barracks, repair and cloning branches, a Construction Yard, a plain building
  and an aircraft factory are covered. Separate controls show unused mode 0
  and a positive EMP counter do not change the setter's result.
- 24 original Event6 actor slices (`4C74E8` through `4C75F3` or the refusal
  epilogue `4C8109`): alive/limbo/tether and Construction/Selling admission,
  ground-cell query, empty-contact radio BREAK and virtual null destination
  execute. Only admitted eligible buildings clear ArchiveTarget. The supplied
  actor boundary excludes event token and owner resolution; the end boundary
  excludes target clearing and subsequent manager effects.
- 21 complete Stop selection-loop prefixes (`730EA0` to enqueue `6FFE00`,
  speech `730F00`, or return `730F1C`): a selected live fixture with a rally
  point and nonpositive EMP reaches event 6; positive EMP refuses it.
  Non-rally unarmed controls reach the separate `7010D0` armed admission and
  are refused. This corpus does not claim armed-building, undeployable
  Construction Yard, multiplayer House or whole command-queue coverage.

The real BuildingType vtable is established by constructor `45E2CD` storing
`7E4570`. Fixture type fields are supplied boundaries, not native type-reader
outputs; the sibling rally reader corpus establishes their retail inputs.
The base fixture supplies a map cell. The setter never dereferences its
destination pointer, so its Rust owner also does not require a path coordinate
or a currently resolved object for the archive store.

Rust ownership is `Simulation::set_building_destination` in
`src/sim/mission/concrete_effects.rs`. Both shared null dispatch and nonnull
concrete-effect dispatch use it. Stop, Stun, owner change, capture reset and
other existing null consumers consequently share this body. Buildings are
excluded from Foot mission Override/Restore destination calls by their existing
wrapper owners. Remaining direct represented destination writes were audited:
production mobile exits, Fly orders and Foot-only restore are not Building
setters and retain their existing owners.

`building_destination_tests.rs` compares the 64 native mode-one calls through
`assign_destination_represented` for cell, entity, object and building pointer
shapes, and the 24 Event6 destination results through production `Command::Stop`.
It also checks unchanged Foot navigation, mission/timer state and scenario RNG.
These tests do not certify all Stop effects or the input queue.

The Building setter has no RNG draws, timer writes or detach calls. The Event6
prefix calls its existing radio owner before destination assignment; this fixture
uses empty contact slots. Nonempty contacts and later target/spawn/slave cleanup
remain their existing mechanisms. The input prefix has no timer or RNG effects.

Residual: Rust has no EMP `Techno+504` state producer or lifetime owner, and
active-retail EMP creation/load reachability remains unestablished. Input currently
queues every selected owned actor, so it cannot reproduce EMP-positive refusal.
This is a separate EMP prerequisite, not a destination-setter condition; adding
one here would incorrectly suppress native calls already dispatched. Trigger and
frequency in active retail are not yet established. If reachable, Stop on an
EMP-disabled factory could incorrectly clear its rally and alter later production
orders. The broader procedural drawing goal remains open.
