# Initial Building records and Action130

`house_initial_technos.py` composes the existing Engineer input reader and
`building_sale_triggers.Fixture`. Its two native controls retain a single
retail GAPOWR at cell10,10 through House50D610, execute a real live Action14
ownership transfer during the supplied sale-completion boundary, and directly
dispatch reader-created Action130 through a real live Trigger. One control
leaves the foreign Building resident; the other executes its original virtual
F8 UnInit before dispatch. The deferred deletion queue is not drained.

The native House records, constructor, ownership change, cell-resident query,
ForceCreate458A80, Unlimbo and UnInit run without gameplay-result overrides.
The fixture constructs two cold Base members with original42E6F0 before the
first Building, then executes original455F10 for the supplied placement prior.
It never copies or resets an active Base. Original510690 and the HouseCtor
4F562B..4F5637 tail initialize the empty initial-record members; only50D610
produces their records.

The JSON preserves normalized ownership/lifecycle/location, initial records,
pending objects, action and placement returns, final native resident and all
three RNG boundaries. Raw allocation pointers and copied stack residue are not
semantic goldens. In particular Techno+104 is auxiliary stack data in the
existing Stage clock, not a reference that restoration must recreate.

The Rust consumer is
`sim::trigger_runtime::building_sale_native_tests::restore130_after_owner_transfer_matches_native_blocked_and_cleared_cells`.
It shares the existing sale comparison's native-derived GAPOWR/Country/ART
inputs. It dispatches the actual Action14 directly, then uses the production
record, UnInit, live Action130, constructor and placement owners. The separate
sale comparison owns refund/RecordKill/callback-history coverage. The RNG prior
is imported from native execution, so setup's Rust draws cannot define the
expected restoration cursor.

This is a prepared32x32 component comparison in measured runtime mode1, not a
complete House constructor or native map load. The synthetic event13=999 is
not polled. Natural HighExpR startup/owner timing, later Building AI/body
completion, other types and the whole458A80 input domain remain outside scope.
Inherited arena allocation/free, TLS/interlocked, OS/audio and presentation
boundaries are described in the producer metadata and the shared fixture.

The reference was executed from the original retail `gamemd.exe`.
Configure `VERA20K_GAMEMD_EXE` using the
[shared native runner](../native_oracle.md) and set
`VERA20K_ENGINEER_REPAIR_ASSETS` to the physical input directory documented by
the [shared Engineer input owner](engineer_repair_joined.md), then replay:

```sh
python -m tools.spatial_oracle.house_initial_technos --check
```

Deliberate reference regeneration uses the same command with `--write`.
The `.meta.json` sidecar records runtime versions, entry points, assumptions,
substitutions and replay command. It deliberately contains no repository
source hashes.
