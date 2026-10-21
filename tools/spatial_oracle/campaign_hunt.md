# Campaign Hunt without a target

Replay the native comparison with:

```sh
python -m tools.spatial_oracle.campaign_hunt --check
```

`VERA20K_GAMEMD_EXE` or `RA2_DIR` selects the local executable. `--write`
deliberately regenerates `campaign_hunt.json` and its provenance sidecar.

The fixture executes the original `FootClass::Mission_Hunt` prologue at
`0x4D5350`, then resumes at its no-target join, `0x4D54EE`. The absence of a
target is a supplied entry boundary. It does not substitute a scanner return or
claim to execute the whole scanner. No native instructions or class vtables are
changed.

The campaign computer arm calls the original human-control predicate
`0x50B730`, reads **PlayerPtr** at `0xA83D4C`, and calls
`HouseClass::GetBaseOrStartingCell` at `0x50DEF0`. The player and actor owner
are distinct supplied Houses with different base cells. The original static
initializers `0x4D30F0` and `0x4F50A0` establish the two zero-cell sentinels.
An alternate cell `(0,0)` falls back to the primary cell; `(0,12)` is a valid
alternate. A final empty cell skips both the manager reset and navigation.

For a nonempty cell, execution continues through the optional original
`SlaveManager` reset `0x6B0C80`, actual map lookup `0x5657A0`, the Unit
destination setter `0x741970` with raw flag `1`, Foot's setter `0x4D94B0`,
and Drive's `MoveTo` `0x4AFD40`. It then calls original `QueueMission`
`0x5B35E0` with `(Move,1)`. The queue invokes actual Unit readiness
`0x744270` and, when admitted, `Commence` `0x5B3570`. The cadence lookup
`0x5B3A00` therefore observes the resulting current mission. Distinct supplied
Hunt and Move rates make this distinction visible; the original x87 conversion
and Scenario `RandomRanged(0,2)` execute, with full RNG states retained.

The deploying control supplies Unit byte `+0x6E1`. The original class setter
refuses its destination, and actual readiness refuses Commence: Move remains
queued while Hunt supplies the cadence. This is a supplied deployment prestate,
not a deployment-animation or subsequent movement comparison. The optional
manager has an empty node vector, so this corpus establishes its state/frame
reset and ordering. Live-slave reset behavior remains covered by the existing
[`slave_manager.py`](slave_manager.py) owner.

Each case starts in a fresh instance of the existing
[`track_destination.py`](track_destination.py) fixture: an original constructed
Drive/Facing, original Unit/type/Cell vtables, supplied Unit and House fields,
and a flat 32 by 32 allocated map. It does not run Unit or House construction,
map loading, physical placement, pathfinding, or locomotor Process. The inherited
OS interlocked transports are the only substituted callables. Human-controlled,
campaign `PlayerControl`, and noncampaign controls stop at `0x4D5578`, immediately
before the existing idle owner; they establish only that branch and unchanged
prestate. They do not claim idle effects or its cadence.

The stock producer is recorded separately in
[`trigger_all_to_hunt.json`](trigger_all_to_hunt.json): ALL06's retained
`095EB2AC` → `05C6450C` → `05C6397C` elapsed/enable chain reaches Action6 for
YuriCountry. Its selected Wave4 Team `06C0E3DC` has the retained TaskForce of
three YTNK. Country9 has no initial Foot units, so the connected Rust test in
[`trigger_runtime_all_to_hunt_tests.rs`](../../src/sim/trigger_runtime_all_to_hunt_tests.rs)
explicitly supplies the already-arrived Wave4 team before running the production
trigger/Team/Hunt owners. The cropped no-target scene and this native join are
separate evidence boundaries; neither claims a complete native campaign replay.
