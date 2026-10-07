# Cloak material caller and producer evidence

This packet preserves static original-byte inspection supporting the canonical
[`visual_character`](../../../../src/sim/cloak_disguise.rs) query and its screen
consumers. It supplements the [executed pixel/producer corpus](../../translucent_blitter_a.md);
it does not certify a whole original scene or cloak lifecycle.

The selected Steam `gamemd.exe` SHA256 is
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.
[manifest.json](manifest.json) records every preserved file hash and the exact
`tools.native_inspect` command for each static JSON. Those JSONs independently
record the binary identity, requested VA/range, Capstone version and decoding
coverage. Source packets are retained byte-for-byte, including their UTF-8 BOM.
No executable copy is included.

## Native behavior established by bytes and connected callers

- [Building inspection](building/README.md): primary vtable `7E3EBC+68`
  dispatches to `4544A0`; actual shared SHP `705E24` invokes this slot with
  `(0,NULL)`. Building `+6ED == 0` delegates to `703860`. Nonzero signed-byte
  stages use the Building override and its own visibility gates. Building RTTI
  `459EC0` returns6. Screen sensors use `70D420` and a virtual center producer;
  they are not proven equivalent to querying the stored anchor cell.
- [Locomotor inspection](locomotor/README.md): actual registered factory,
  CreateInstance, constructor and instance-vtable stores connect eight exact
  Drive/Hover/Walk/Fly/Teleport/Ship/Jumpjet/Rocket tables to virtual `+34`,
  whose original leaf `55ABC0` is `xor eax,eax; ret 8`. Foot `4DA4E0` delegates
  to canonical `703860` when this leaf returns zero. This constant leaf needs
  no second state or decision owner. Tunnel is a preserved counterexample
  outside the inspected retail-installed roster.
- `703860` reads signed progress `+224` and signed Rules stage count `+628`;
  its nonzero-progress arithmetic is `FILD`, `FIDIV`, multiply by the exact
  original double256, then original `ftol7C5F00`. The [native query packet](building/techno-703860.json)
  preserves its thresholds and observer branches. The executed corpus uses
  real Unit/Drive tables, the original Drive constructor and this Foot wrapper.

These queries and constant leaves perform no RNG draws, timer writes or detach
calls in the inspected/executed paths. This does not remove such effects from
upstream cloak transitions or prove their lifecycle. The existing simulation
cloak owner retains those responsibilities.

## Signed arithmetic proof and its execution boundary

Let `p` be the signed progress reaching the arithmetic (`1 <= p <= INT_MAX`)
and `s` a nonzero signed i32 stage count. The rational scaled value is
`r = 256*p/s`; its numerator fits signed64 and has magnitude below `2^39`.
The established native control word is `0E7F`: 53-bit significand precision
and rounding toward zero. `FILD p` is exact, and multiplication by256 changes
only the exponent and is exact here. A 53-bit division has absolute error after
scaling strictly below `2^(-13)/abs(s)`. For a noninteger rational `r`, its
separation from any integer is at least `1/abs(s)`, so this error cannot cross
an integer boundary. If `r` is an integer, the intermediate `p/s = r/256` is
exactly representable with fewer than53 significant bits, so no rounding occurs.
Thus signed integer `256*p/s` truncated toward zero has the same integer result
as this finite native path, before the native low32 narrowing. The magnitude
also fits native signed64 `FISTP`; narrowing to i32 wraps rather than saturates.
This is an arithmetic argument under the established precision/control policy,
not permission to substitute arbitrary host floating-point math.

`s == 0` is handled separately: the masked native invalid conversion stores
signed64 indefinite, whose consumed low32 bits are zero. The original executed
zero-stage controls record that behavior; clamping the divisor to1 changes it.
Negative progress returns before this arithmetic. Negative divisors retain their
sign. The 256 original transition controls exercise stage0/1/9/12/13/-1/-9,
signed progress boundaries, force/current-house variants and state0/2 controls;
they are bounded execution checks, not an exhaustive test over all i32 inputs.

## Reproduce and compare

Select the supported original executable as described in [native setup](../../../../tools/native_oracle.md).
From the checkout root, the following reuses the existing inspector and compares
all static packets semantically (BOM/newline formatting is not evidence):

```python
import json, subprocess
from pathlib import Path
root = Path("tools/procedural_drawing_oracle/validation/native-controlflow")
manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
for row in manifest["packets"]:
    if "command" not in row:
        continue
    saved = json.loads((root / row["path"]).read_text(encoding="utf-8-sig"))
    current = json.loads(subprocess.check_output(row["command"], text=True, encoding="utf-8"))
    assert current == saved, row["path"]
```

```sh
python -m tools.procedural_drawing_oracle.blitter_a --translucent --check
```

The [static replay receipt](static-check.receipt.json) records40 semantically
matching original inspector packets. This is static-byte validation, not execution.
The [historical pre-replay-extension native replay receipt](translucent-check.receipt.json)
records the prior corpus command,
exit status, binary identity and [output](translucent-check.log). The current
payload contains20 selector rows,6136 single-call pixel cases,54 sequential
parent replay controls,256 visual-character controls and14 offset controls.
The [new original-generation receipt](translucent-generation.receipt.json) and
[output](translucent-generation.log) record the successful extension generation
and unchanged canonical SHA of all6136 existing cases. Reproduce the current
corpus with the same `--translucent --check` command; the historical check receipt
does not claim that the new54 replay controls were checked by that earlier run. Its raw file SHA is
`ad9da73bb4a6ea1f6fa472bcafc2ea9e132f487b944b762578677fc717a9b445`;
the metadata SHA `a1ea5ca49f238eb894f4f7e276dd1bfc31122226f856c1135ab2c0333da4c5e6`
is the distinct sorted compact canonical-JSON payload hash, not a conflicting
file identity.

Rust regression links are `game_entity::tests::visual_character_matches_original_executed_transition_controls`
and `game_entity::tests::native_cloak_offset_matches_original_identity_float_and_wrap_controls`
in [game_entity.rs](../../../../src/sim/game_entity.rs). GPU pixel comparisons
are in [packed_material_gpu_tests.rs](../../../../src/app/presentation/render/packed_material_gpu_tests.rs).
Their actual Rust/GPU runs need their own receipts; this native packet does not
assert those tests passed.

## Ghidra leads and limits

[The preserved Ghidra identity](ghidra/get_metadata.txt) and
[program inventory](ghidra/list_open_programs.txt) identify the Steam source path,
x86 little-endian32 image and base00400000 in the shared analyzed database.
That metadata does not itself attest the loaded program's SHA. The inspector and
oracle independently check the selected file hash. The two preserved decompilations
([703860](ghidra/0x00703860-decompile.c), [70BE50](ghidra/0x0070BE50-decompile.c))
are leads only: the offset decompile omits the actual floating conversion inputs.
No shared database edits were made. Re-read them with explicit program selection
and `tools.ghidra_compare.client.Client.get('/decompile_function', address=...)`
under the [Ghidra workflow](../../../../docs/research/ghidra-workflow.md); the
local endpoint `http://127.0.0.1:8089` is a session configuration, not a portable
service guarantee.

Remaining boundaries: no nonzero Building+6ED production/lifecycle proof; no
selected scenario retail-reader execution in these static packets; no exhaustive
alliance/sensor lifecycle, Mech/DropPod/external-vtable analysis, full Unit source
cache/3D part-depth composition, complete warp route, ordinary native framebuffer
pitch or full rendered-frame execution. Supplied pixel-leaf offsets and destination
fixtures are not producer-lifecycle evidence. These gaps keep corresponding broad
mechanism claims open.

The [ordinary provenance refresh receipt](ordinary-write.receipt.json) records
that regeneration after this shared oracle-source edit retained the original
ordinary payload byte-for-byte; only its source provenance changed.

Current extended-corpus independent replay: [receipt](translucent-replay-check.receipt.json) and [log](translucent-replay-check.log), exit0; includes54 triple-parent controls in addition to the unchanged6136 single-call cases. The prior translucent-check receipt is retained as historical pre-extension evidence.
