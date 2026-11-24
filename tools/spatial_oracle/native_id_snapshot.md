# Scenario save/load continuation

`python -m tools.spatial_oracle.native_id_snapshot --check` executes the pinned
original `gamemd.exe`; `--write` explicitly regenerates the corpus and sidecar.
Configure `RA2_DIR` or `VERA20K_GAMEMD_EXE` through
[`native_oracle`](../native_oracle.md).

The seven historical `cases` retain their schema and observations: original
Scenario Save `0x689310`, Load `0x689470`, post-read `0x683560`, then numeric ID
increment `0x68BCB0`. `ScenarioFixture` owns that same isolated receiver and
IStream byte transport for these cases and composed consumers. It supplies a
stopped elapsed timer, empty dynamic arrays, false `+0x11F8` string flag,
diagnostic return and clock zero. It is not a complete world or native SAV load.

`scenario_bookmarks` adds three round trips of the four packed signed cell
pairs at Scenario `+0x348E/+0x3492/+0x3496/+0x349A`. Each row saves the supplied
16 bytes, overwrites the outgoing field, then observes the actual Load result.
The `input` and `loaded` arrays are directly usable by Rust tests. Negative and
extreme coordinates establish byte persistence only, not valid-map recall.

Original command bodies establish field ownership: SetView1..4 write those
slots at `0x53774C/0x53780C/0x5378CC/0x53798C`; View1 recall `0x537350` reads
the first pair before calling the existing absolute-view owner `0x6D6070`.
Command admission and recall are instruction-level evidence, not executed by
the bookmark rows.

The same fixture is composed by [`gen_rng_vectors`](../rmg_oracle/rng.md) to
observe Scenario post-read RNG initialization and the separate menu reseed
boundary. RNG fixture creation remains with that existing RNG owner; Scenario
transport has no dependency on it.
