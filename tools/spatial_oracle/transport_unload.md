# Transport Unload controls

`transport_unload.py` reuses the dock VM for four state-1 controls and
`anytown_damage.foot_missions.FootMissions` for four bounded retail histories.
It does not replace a native gameplay function. `UnitClass::AI 7360C0`,
`Facing_Update 736990` and `Mission_Unload 73D630` execute original instructions.

The gate corpus varies hull and turret rotation independently. The actual
Facing_Update produces `Unit+6AF` before the original state-1 handler reads it.
The Rust regression consumes the resulting state, delay and full Scenario RNG
state. A clear latch advances to state 3 with delay 1 and no RNG draw; a set
latch takes the mission-rate epilogue.

The retail histories use BFRT with a required hull turn, already-aligned BFRT,
and already-aligned FV/HTK with an admitted turret arc. The shared fixture owns
physical layered type/ART readers, native constructors, locomotor COM factories,
map cells and initial placement. This composition additionally reads each
physical `[Unload]` section through Rules' original MissionControl loop
`679C92..679CAF` / `5B3760`. It supplies one already-boarded E1, frame 100 and
initial facings, then visits the carrier's complete original Unit AI each frame.
Records retain handler calls, body/turret timers, latch, delay, ordered RNG
draws, actual ejection heading, passenger placement/queue/link cleanup and the
carrier's return to Guard. The inherited cropped-map, supplied House and
OS/presentation boundaries remain; passenger movement and other global phases
are not visited.

Reproduce the gate corpus with a supported retail executable available to the
native loader:

```sh
RA2_DIR=/path/to/retail python -m tools.spatial_oracle.transport_unload --check
```

For the histories, prepare the same physical input directories as
[FootMissions](anytown_damage/foot_missions.md), with physical `XMP03T4.MAP`,
its cropped TEMPERATE tiles, and BFRT/FV/HTK/E1 rules and ART available. Use the
existing `asset extract` command for missing files; retain exact filename case.

```sh
RA2_DIR=/path/to/retail \
VERA20K_SHRAPNEL_INPUTS=/path/to/extracted/inputs \
VERA20K_ANYTOWN_INPUTS=/path/to/extracted/inputs \
python -m tools.spatial_oracle.transport_unload --retail --check
```

Use `--write` only to deliberately regenerate a corpus. Each sidecar records
the replay command and fixture boundaries. The retail
Rust command tests use physical stock rules/ART on an explicit flat arena;
the native crop histories establish the corresponding handler and facing
controls, rather than a complete match comparison.

`unit_unload_shore.py` isolates the water-to-shore failure handoff. It supplies
FNPC's packed empty result `(0,0)` and executes the original Unload handler,
Unit/Foot destination setters, Cell getter and Hover MoveTo. The four controls
vary real versus missing `(0,0)`, a missing cell west of the carrier, and
height 120 versus 0. No search, whole map, passenger-placement or full-match
comparison is claimed by these controls.

The native caller still passes the nonnull Cell returned by Map lookup after
failure. The Unit setter's Unload arm looks up the cell west of the carrier
before testing `Harvester`. If that lookup misses too, the retained Dummy's
coordinate changes before Foot reads its virtual `+4C`; it keeps Dummy's own
level and slope even when its live coordinate names a real cell. A subsequent
lookup changes that retained target again, while Hover's copied destination
stays fixed. Fresh map queries and retained pointer getters have distinct
effects and must keep their original order.

The controls enter Hover's synchronous Path_And_Arrival/ProcessMovement pair.
For this initial stopped, null-head, cleared-path state, Foot has already
started a positive movement-timer remainder, so these calls leave the head and
path unchanged without FindPath or RNG. This evidence does not close the
separate existing-head/direct-MoveTo timing residual. Timer comparisons use
native start/duration; the auxiliary middle word is recorded separately
because it inherits destination Y from compiler stack reuse.

```sh
RA2_DIR=/path/to/retail python -m tools.spatial_oracle.unit_unload_shore --check
```

The corresponding Rust regression runs the actual nearby-cell search over an
all-water fixture before comparing the retained handoff with this corpus.
A second regression uses physical stock LCRF/SAPC/YHVR rules, the player Unload
command and ordinary object turns. Foot4D3810 rejects destination cell `(0,0)`
for a valid MovementZone; Hover5166BD..5166E4 clears the destination in the
same frame. The regression verifies that completed-frame cleanup, then supplies
an explicit handler dispatch to save the retained assignment before Hover
Process. Independent restored/control Dummy cells continue through the next
ordinary frames with equal hashes and retained cargo. The shared Cell
getter regression also stamps Dummy onto a real cell with a different height
to distinguish retained allocation identity from a coordinate re-lookup.
