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
