# Building mission controls

Run `python -m tools.spatial_oracle.building_guard_attack --check` (or
`--write` to regenerate). The original `guard`, `attack`, `set_target`,
`unlimbo` and `cadence` rows retain their existing inputs and scope.

`hidden_startup` adds three two-frame controls using the same Building VM.
Original `44D6A0(1,1)` under ScenarioInit establishes Idle and queues Guard.
Each frame executes UpdateAnimation `4509D0`, pre-ready `43FE27..43FE54`,
counter/mission dispatch `6FA646..6FA65A`, post-ready `43FF91..43FFB4` and
queued body `43FFB4..440042`. Type `+CA1` and prior ready `+6DD` are inputs.
No discovery, full GrandOpening or whole BuildingUpdate is claimed.

The hidden turret keeps current None/queued Guard and consumes no RNG. A
ready turret and a non-turret commence Guard; its delay uses Scenario RNG.
The non-turret remains armed to isolate the ready gate, so it is not a
retail barricade replay. The control relates to campaign startup because
BuildingUnlimbo `440D25..440D6C` gates player discovery by revealed ground
in GameMode0, and `440E5C..440E76` omits owner discovery for human houses.

Each row records initial mission/body state, per-frame native timer stores
(start/duration only), ordered observed calls, and all three complete RNG
buffers before and after the two frames. The buffers are seeded with the
input seed after VM setup. The existing fixture's presentation and weapon
answer seams remain; no target or weapon call is needed by these controls.
The generator verifies the original mapped instruction bytes are unchanged.

Rust consumers live in `src/sim/world/techno_ai/building_missions_tests.rs`;
the campaign integration regression belongs to the production map loader.
These controls do not establish whole-map or whole-campaign RNG parity.
