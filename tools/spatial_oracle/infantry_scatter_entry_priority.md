# Infantry priority admission controls

This additive corpus executes the complete original Infantry CanEnter body
`0x0051BF90` against declared state. It contains 192 Infantry/Unit cases in
`cases` and 30 Building/Aircraft/closed Gate cases in `structure_cases`.
Every row records input, native return, observed original PCs, both globals,
all three full RNG buffers before/after, and original executable-section hashes.
It establishes those admission boundaries, not whole Infantry or building death
parity.

```sh
python -m tools.spatial_oracle.infantry_scatter_entry --priority --check
```

Set `VERA20K_GAMEMD_EXE` to the active retail executable.
Unicorn is required. `--output` selects another file; `--write` deliberately
records new native outputs and sidecar provenance.

The existing `infantry_scatter_entry.py` owns both CLI modes and reuses
`unit_source_scatter.make_source_fixture`, `unit_scatter_state`, `map_queries`,
and `native_oracle`. It creates no alternate native engine or gameplay helper.
Without `--priority`, the existing 14 source Scatter rows still execute and use
their original JSON payload.

## Supplied state and observations

The fixture supplies a flat physical 32x32 map with original Cell vtables, a
usable or rim rectangle, original Infantry/Type vtables, Guard/Doing -1, mover
`+3D5=1`, and native owner indexes 0/1 with self-alliance masks. Counter 0/1/2 is
written to the actual DWORD `A8E7AC`; mode 0/1 is separately written to actual
DWORD `A8B238`. This distinguishes the counter from a historical `unit_entry.py`
input named `game_mode_nonzero`, which actually writes `A8E7AC`.

An armed actor has a supplied ordinary first weapon with Damage 1, AmbientDamage
0 and second slot NULL. GetWeapon and original damage-value query `6F3970` run;
their answers are not replaced. Infantry and Unit blockers use original
Walk/Drive construction and stated stationary/moving inputs. Building/Aircraft
use original class/type tables; a closed Gate supplies `Type+16B7=true`.
Complete constructors, full retail readers and producers of all those retained
states are outside this primitive comparison.

List-only cases intentionally use raw owner -1 and no raw bits. Separate
coherent enemy raw-owner cases use owner 1 and bits 4 or 0x20. This separation
tests the local occupant classifier and the independently retained raw-owner
check. All queries preserve all three RNG states byte-for-byte; any measured RNG
or allocator call fails. Original executable sections and Infantry `vt+1AC`
must remain unchanged. The reused OS Interlocked hook supplies no gameplay
answer and is not reached by the measured CanEnter body.

## Original boundaries

At `51C144`, nonzero `A8E7AC` bypasses the usable-area refusal for the explicit
mover-in-playfield prior. Whole `51BF90` contains no `A8B238` read. Earlier
building, target, transporter and Gate checks remain active. The closed Gate
checks `4525F0`, reverse House alliance `4F9A50` at `51C511`, and IsArmed before
the later counter read.

At `51C580`, actual House alliance `4F9A90` runs. Only its false result reaches
`51C58D`; nonzero counter then jumps to the native allied occupant classifier
`51C650`. This does not alter House alliance generally. Wall ownership and later
raw-owner `4F9A10` at `51C80A` continue to use actual relationships.

With no raw owner, native armed stationary enemy Unit returns 5 at depth 0 and
6 at depths 1/2; moving Unit returns 5 then 2. Unarmed returns 7 then 6/2. A
regular armed enemy Building/Aircraft returns 5 then 7 because the original
allied switch refuses those classes. The closed Gate remains armed 5/unarmed 7
at every depth and never reaches the later override. The native rows record
all results; depth 1 and 2 are equivalent for these readers.

## Connection to escaped crew

Original Building Phase B increments at `443141` before the initial ordinary
Place and keeps the counter through complete Unlimbo, health, NULL Scatter,
synchronous Walk and mission/target writes; it decrements at `443288`.
Initial Place `443182` receives explicit priority false even while the counter
is raised. Infantry Unlimbo `51E027` then reads the global to select its own
floor-placement priority. Object Reveal `5F4F1B` skips its CanEnter recheck only
for nonzero counter. Successful NULL Scatter reaches immediate Walk through
`51D478`; those synchronous entry calls must observe the same scoped counter.

The joined building-death oracle owns the actual retail producer/lifecycle
comparison. This primitive matrix does not execute that full chain, the
counter writers, damage-health draw, all Scatter fallbacks, tagged receivers or
Selling/capture/garrison routes.

Cell subcell offsets used by placement require the original initializer
`48E480..48E4F3`, whose pointer at `812B28` lies in the active PE CRT initializer
range `812000..815DA4`. These entry-only controls never call Place. The joined
producer executes that original initializer; cold zero offsets cannot serve as
placement evidence. Retail E1's retained Nominal default is false; broader
Nominal/Technician routes are not certified by this corpus.
