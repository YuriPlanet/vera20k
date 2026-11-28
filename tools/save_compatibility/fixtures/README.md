# Historical player-save fixtures

These authored worlds contain VERA state and a small hand-written ruleset. They
can be redistributed and loaded without the original player's files. Each
fixture comes from the `GameSnapshot::save_validated_with_presentation` writer
at the revision listed below. The input includes a saved rules record, terrain with live
cell land, a local view and a registered player House. The historical writer
loads its own file through `load_with_resources` and `restore_player_world`
and executes the saved command before it accepts the fixture.

| File | Writer commit | Generator mode |
| --- | --- | --- |
| `v0.3.8.sav` | `9edbfe483` | `skirmish` |
| `v0.3.9.sav` | `e553406e4` | `campaign` |
| `v0.3.9-tunnel.sav` | `e553406e4` | `tunnel` |
| `v0.4.0.sav` | `fee2773ca` | `skirmish` |
| `v0.4.0-alpha.sav` | `fee2773ca` | `alpha` |
| `v0.4.0-sequence.sav` | `fee2773ca` | `sequence` |
| `main-eb05f451e.sav` | `eb05f451e` (pre-release) | `skirmish` |
| `main-eb05f451e-images.sav` | `eb05f451e` (pre-release) | `image-states` |
| `main-41f140f93.sav` | `41f140f93` (pre-release) | `skirmish` |
| `main-6d6caa108.sav` | `6d6caa108` (pre-release) | `skirmish` |
| `main-ffe8b25af.sav` | `ffe8b25af` (pre-release) | `skirmish` |
| `main-93ab2ce86.sav` | `93ab2ce86` (pre-release) | `skirmish` |
| `main-06789b62b.sav` | `06789b62b` (pre-release) | `skirmish` |
| `main-8d55b4a73.sav` | `8d55b4a73` (pre-release) | `skirmish` |
| `main-8852f7a76.sav` | `8852f7a76` (pre-release) | `skirmish` |
| `main-bc55c35e7.sav` | `bc55c35e7` (pre-release) | `skirmish` |
| `main-30e9a0f38.sav` | `30e9a0f38` (pre-release) | `skirmish` |
| `main-a33fcac81.sav` | `a33fcac81` (Wasm extension modules) | `skirmish` |
| `main-d17e45e42.sav` | `d17e45e42` (Wasm module control points) | `skirmish` |
| `main-2e39b88dd.sav` | `2e39b88dd` (Wasm modules and ArtImageSwap) | `skirmish` |
| `main-2e67ebaa8.sav` | `2e67ebaa8` (pre-release merge) | `skirmish` |
| `main-fb29ff473.sav` | `fb29ff473` (Wasm modules, ArtImageSwap and NPatch) | `skirmish` |
| `main-b268ba798.sav` | `b268ba798` (Wasm module control points, ArtImageSwap and NPatch) | `skirmish` |
| `main-0b918d053.sav` | `0b918d053` (Wasm client modules on the control points) | `skirmish` |
| `main-c3609b58a.sav` | `c3609b58a` (per-house driver policy, PR #4576) | `skirmish` |
| `main-a17a144ca.sav` | `a17a144ca` (shared Chronosphere and Temporal warp-out flag) | `temporal` |

The Kratos writer revision is in PR #5080. In a fresh clone, fetch it with
`git fetch origin pull/5080/head` before checking out `bc55c35e7`. The Wasm
modules writers are in PR #5183: fetch `pull/5183/head` before checking out
`a33fcac81` or `2e39b88dd`. The NPatch writers are in PR #5172: fetch
`pull/5172/head` before checking out `2e67ebaa8` or `fb29ff473`. The control points writers are in PR #5245: fetch
`pull/5245/head` before checking out `d17e45e42` or `b268ba798`. The client
modules writer is in PR #5251: fetch `pull/5251/head` before checking out
`0b918d053`. Fetch `pull/4576/head` for the driver-policy writer `c3609b58a`. The
`2e67ebaa8` row is the writer after NPatch left the build flags: its layout
`0xE5279FB2` stores each processed rules layer's NPatch key reads beside
Tiberian Sun's web fields, which `30e9a0f38` predated.

The ordinary worlds use `compat.map`, a flat 3×3 terrain and an authored
APC type. At tick 91 the Americans player has 1,000 credits. The pending
game-speed command changes the speed to 4 on the next simulation tick. The
tunnel world starts with an APC in a map tunnel. It uses the release's
movement fixture terrain and production entry and movement functions.
Source bytes stay fixed when the current writer
changes. The alpha scenario gives the two APCs the authored one-pixel
`COMPATGLOW.SHP` in a 3×3 canvas. The v0.4.0 Unlimbo path creates both glows,
and pointer expiry disables one before the writer runs. The historical reader
restores both and removes the expired glow on the next tick.

The sequence scenario registers one building, `POST`. Its idle animation adds
a real entry to the rules animation catalog. v0.4.0 hashed that catalog before
the SHP firing layout became part of each entry. The desktop test converts
it, executes the queued command, saves with the current digest and reloads
the result.

The image-states scenario registers the authored `COMPATGLOW` animation and
a `POST` overlay using the same picture. The writer at `eb05f451e` included
both types' image state in the rules digest before the alternate-theater ART
flag existed. Its test checks both registries after conversion.

The `a17a144ca` fixture represents a unit after Chronosphere arrival with a live
erase head and a clear warp-out flag. The shooter still holds its Attack order.
Loading and applying the queued tick-92 speed change leaves the unit able to act
and advances the shooter's erase progress from 1000 to 992. Both sides of the
link survive the next ordinary save and cold reload.

To reproduce a fixture, create a detached worktree at its writer commit. Run
the following from that worktree with your current checkout available at
`/path/to/current`. The helper stages an authored scenario and the revision's
desktop resource binding functions in this disposable checkout. It also
enables the tagged tube setup for the v0.3.9 transit case. It leaves the
revision's serializer, deserializer and restoration behavior intact.

```sh
python -m tools.prepare_checkout
python /path/to/current/tools/save_compatibility/writer/prepare.py
python -m tools.cargo_run -- build --locked -p vera20k-core --features test-support --bin snapshot-layout
python -m tools.cargo_run --resolve snapshot-layout --profile debug
```

Invoke the resolved executable with a new output path and the mode from the
table, for example `<executable> v0.4.0-alpha.sav alpha`. Each run checks the
restored world and the next command. It also writes the source layout trace to
`target/save-fixture-writer/layout.txt`. In the v0.3.8 writer the processed rules
serialize their type families from a HashMap. Two independent writes put
Animation first in one file and Vehicle first in the other. Their headers,
rules hashes and restored results matched. `SHA256SUMS` identifies the
specific bytes frozen for the tests.
