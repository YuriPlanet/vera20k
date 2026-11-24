# Ground transport D / Event9 controls

The independent `--deploy-event` mode of `transport_unload.py` reuses
`refinery_dock.make_dock_fixture`: original Unit/UnitType vtables, constructed
Drive locomotor, map queries, Scenario RNG, class setters, Queue and Commence.
It adds no VM, INI reader or Ares dependency. Default state-1 and `--retail`
generators and corpora remain separate.

```sh
VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
python -m tools.spatial_oracle.transport_unload --deploy-event --check
```

`RA2_DIR` also selects the executable through the shared native loader. Python,
Unicorn and Capstone requirements are in [native setup](../native_oracle.md).
Use `--write` only to deliberately regenerate this corpus. Its sidecar records
the command, entry points and supplied boundaries. No mod files are required.

## Producer and receiver

`deploy_key` contains two whole selected-unit `IssueDeployCommand730AF0`
prefixes, stopping at event producer `6FFE00`. Both empty and count-1 inputs
pass actual `CanDeploySlashUnload700D50` and emit DEPLOY event opcode9.
This is not Mission9. The singleton selected Unit, local House and transport
type are supplied; event encoding, delivery and the UI success tail are excluded.

`receiver` contains Guard5/count0, Move2/count0, already-Unload16/count0 and
Move2/count1 controls. The resolved actor is supplied in ESI at `4C76D7`, after
Event9's token-resolution/planning prefix. Original alive/limbo/tether/EMP,
ground-projection, current-mission, class and WeaponsFactory tests execute
through `4C7818`. There is no cargo-count test in this receiver. It calls:

1. OVER_OUT3 to contact0 (`4C77EA`, original `65ACB0`).
2. Unit destination setter with NULL/flag1 (`4C77F8`, original `741970`).
3. Techno target setter with NULL (`4C7804`, original `6FCDB0`).
4. Queue Unload16/non-immediate0 (`4C7812`, original `5B35E0`).

The no-contact fixture excludes live radio/detach effects. In ordinary
Guard/Move, the pending mission becomes16 while current mission and status
remain unchanged; the movement-bypass byte clears. Current target/NavCom clear,
but ArchiveTarget, suspended target/NavCom and suspended mission remain.
In Move inputs, original Drive IsMoving changes true to false and the native
destination clears. An empty transport's D therefore has observable state effects.

## Promotion, empty first visit and sound

Each receiver row next executes whole original `Commence5B3570` as an explicit
promotion component. It promotes pending16, resets status and task timing, and
clears pending. Native Ready/Logic scheduling is not executed. The repeated
already-Unload/queue=-1 control retains status7, bypass1 and its old timer:
same-mission Queue does not restart it, and Commence returns false. Status7 is a
preservation sentinel; this row is not dispatched as an Unload handler visit.

For the two empty Guard/Move rows, one complete `Mission_Unload73D630` call runs
after promotion, on ordinary land with a stopped Drive. It queues Guard5 at
`73D887`. Current16 remains until later promotion. The native rate epilogue
`73E289..73E2BF` makes one Scenario `RandomRanged(0,2)` call; supplied seed1
returns1 and supplied Unload Rate0.016 produces returned delay15. Raw before/after
Scenario states and the ordered draw are retained. The caller's later timer
scheduling, Guard promotion and further Unload visits are excluded.

The count-1 comparison uses a nonnull head sentinel that is never dereferenced;
it covers admission only. No loaded passenger handler, ejection or placement
claim follows from that row. No full pathfinding or death mechanism is executed.

`default_sound` executes constructor OR/store components `66585F` and `66606A`
to produce Rules `+72C`, then original `750920` with D's volume/flags and audio
enabled. Default sound ID is -1; the call returns0 without reaching the observed
sample allocation/play or QueueVoice entries. Full Rules construction, sound
catalog loading and OS playback are excluded. The D producer `6FFE00` has no
VoiceDeploy call; the UI success tail `730D31..730D5D` uses this separate sound.

The named-call ledger also observes MainRandom, action-timer start and detach
entries. No such calls occur in the supplied no-contact receiver, promotion or
empty first visit; that first visit's Scenario draw is recorded separately.
Direct native timer field writes are represented by the snapshots.

## Rust consumer boundary

`app/input/dispatch.rs::deploy_key_command` should preserve the Event9 identity
for ordinary ground transports, including empty ones. The existing receiver
owner is `sim/deploy.rs::issue_order`; the current `Command::DeployMcv` payload
routes there. Sending D through the count-gated self-click unload resolver loses
the empty order and the event's distinct admission/state effects.

Keep the empty self-click's NoDeploy30 behavior. Do not globally loosen
`transport_orders::unload_command`, or duplicate Event9 in the MEGAMISSION
`UnloadPassengers` path. `sim/transport_unload.rs::unit_mission_unload` owns the
empty status0 Guard fallback and cadence. These native components provide
goldens for those consumers; no Rust regression or full-chain parity result is
claimed by the harness itself.
