# IFV turret switching: native component comparison

[`ifv_turret_switching.py`](ifv_turret_switching.py) executes original retail
`gamemd.exe` instructions through the shared [native oracle](../native_oracle.md).
The [payload](ifv_turret_switching.json) holds the observed outputs; its
[sidecar](ifv_turret_switching.meta.json) pins code spans, environment, input
assumptions, substitutions and payload identity. No expected scalar or timer
result is calculated by a second Python implementation.

## Reproduce

Extract physical `RULESMD.INI`, optional `LANGRULE.INI`, `MPBattleMD.ini`, the
selected map's INI text, `FV.VXL/HVA` and `FVTUR.VXL/HVA` through
`FVTUR3.VXL/HVA` with the existing `asset` owner. A `.mmx` archive itself is not
map INI text. The corpus records absent extracted files explicitly; absence in
the extract alone does not prove an asset is absent from the retail archives.

```sh
VERA20K_IFV_ASSETS=/path/to/extract \
VERA20K_IFV_MAP=/path/to/ifv-turret-switching-owned.map \
  python -m tools.spatial_oracle.ifv_turret_switching --check
```

`--write` intentionally publishes a new payload and sidecar. Review changed
observations before accepting them. Imports and `--help` do not emulate or
load retail files. Python dependencies and executable selection follow the
shared oracle guide.

The recorded production map is `ifv-turret-switching-owned.map`, SHA-256
`7116830f605a9dc9c4be383bad0152b5ac11243f1813f3b2c04f6a69c85af2cd`.
Its objects belong to `VERA-OBSERVER`; it adds no type or rules overrides.
This exact text is the final physical layer executed by the reader corpus.
The extracted `Hills.mmx` remains an encrypted MIX archive, not a decoded map
INI input: no native Hills map-layer execution is claimed. Rust's separate
Battle/Hills regression runs its own production source-selection path.

## State and native owners

| State | Owner and original writer |
| --- | --- |
| Signed current turret number, `Techno+0x124` | Constructor `0x6F2BA6` sets `-1`; `SetGunnerWeapon 0x70DC70` selects it; charge-turret AI `0x6FA4FB..0x6FA5BE` updates the same field. |
| Current weapon number, `Techno+0x138` | Constructor `0x6F2BC4` sets `0`; `SetGunnerWeapon` writes the admitted mode or `0`. |
| Turret-by-weapon table, `TechnoType+0x814` | Constructor `0x711781..0x711790` fills 18 signed entries with `-1`; `0x717890` writes one entry and `0x7178B0` reads it. |
| Charge-turret delay, `Techno+0x2F8` | Constructor `0x6F2E98` sets `0`; the existing admitted-fire paths copy their final rearm duration at `0x6FE4A4` and `0x6FF29E`. |

Original direct-call candidates for `SetGunnerWeapon` are the Unit constructor
`0x73568C`, ReceiveGunner `0x74647C/0x7464CE`, RemoveGunner `0x746596`, and
InitFromType `0x7468AF`. The constructor and InitFromType test `Gunner` before
setting mode `0`; this is why an empty stock IFV starts with turret `0` rather
than retaining the base Techno constructor's `-1`.

A literal field-write scan plus the connected function bodies identifies no
per-shot current-weapon write in `TechnoClass::Fire 0x6FDD50`. Stack accesses
and the same offsets in unrelated classes are not this state. This is a
bounded ownership audit, not a proof against arbitrary pointer aliases or
save-stream restoration.

## Rules and layer behavior

The selected original TechnoType readers execute before the complete FV-only
pair block. `TurretCount` uses ReadInt at `0x712859`, `IsChargeTurret` uses
ReadBool at `0x71288D`, and `IFVMode` uses ReadInt at `0x71478F`. Their defaults
are the retained field values; constructor defaults for these fields are
`0`, `false`, and `0`. `IFVMode` remains signed and is not clamped by its reader.

UnitType's ID comparison at `0x747BCA` is case insensitive against literal
`FV` at `0x84314C`. Other type IDs do not execute the mapping block. Each
admitted layer reads the exact-case `TurretIndex` key followed by its
`TurretWeapon` key, then immediately calls the shared table writer. The 17
prefixes, in order, are recorded in `pair_names`:

`Normal`, `Repair`, `MachineGun`, `Flak`, `Pistol`, `Sniper`, `Shock`,
`Explode`, `BrainBlast`, `RadCannon`, `Chrono`, `TerroristExplode`, `Cow`,
`Initiate`, `Virus`, `YuriPrime`, `Guardian`.

Index defaults are respectively `0, 1, 2, 3`, then `0` for every remaining
pair. Every weapon default is `-1`. These are fixed defaults **on each pass**,
not the previous mapping values. Unmentioned entries retain their previous
table values; duplicate weapon numbers take the last writer's result.

The native writer has no bounds check. In particular, weapon `-1` writes a
full dword at `Type+0x810`, whose low byte is `IsChargeTurret`. The corpus pins
this alias: index `256` clears the flag, `257` sets it. A reached FV section
with all pairs missing ends with Guardian's default `0` there; a missing
section skips the entire pass and retains it. Weapon `-2` and `18` controls
demonstrate writes outside the mapping array. Rust's safe policy for other
invalid custom indices must be stated separately from native fidelity.

Original readers applied to the physical retail values produce:

| Weapon modes | Turret model |
| --- | --- |
| `0` | `FVTUR` |
| `1` (engineer) | `FVTUR2` |
| `2..5` (including GI and Conscript) | `FVTUR1` |
| `6..16` (including Tesla Trooper and Guardian GI) | `FVTUR3` |
| `17` | `-1`, the unassigned constructor entry |

The physical-file receipts include hashes and lexical strings. Original
archive loading and global layer scheduling are not emulated; Rust's
production retail reader must independently demonstrate those connected paths.

## Passenger cycle and shared charge consumer

`SetGunnerWeapon` immediately returns when `IsChargeTurret` is set. Otherwise
signed modes `0..17` select that weapon and its mapped turret. All other modes
select weapon `0` and `table[0]`. It has no Gattling or TurretCount gate.

Whole ordinary ReceiveGunner and RemoveGunner bodies execute in the corpus:
enter selects the passenger's type `IFVMode`; removal selects `0`, including a
null passenger. A receive/remove/receive history pins re-adoption after a
failed vehicle unload. Six additional controls execute original
FootClass::RemoveFirstPassenger and CargoClass head removal: a `Gunner` hold
calls RemoveGunner when its resulting count is zero, including an already
empty hold. Exit placement and actual failed Unlimbo are
existing production owners outside this fixture.
Ordinary rows execute without replaced calls, RNG draws, timer writes or
detach calls.

Temporal contrasts preserve the separate dependency: ReceiveGunner transfers
the TemporalClass, selects mode `7`, optionally gets ROF for slot `7` and
restarts the rearm timer, then selects the passenger's final mode. Removal
returns the TemporalClass and optionally gets ROF for slot `0` before the final
mode-`0` reset. The corpus supplies `GetROF=19` and a TemporalClass with no
target. It does **not** certify that boundary's real RNG, LetGo, temporal
target lifecycle or whole rearm mechanism.

The indexed Prism Tank uses the same turret field. Its native charge block
admits `IsChargeTurret && TurretCount>0 && !IsGattling`. A nonpositive saved
charge delay writes turret `0`. Otherwise it multiplies the remaining rearm
frames by TurretCount using wrapping 32-bit integer multiplication, divides by
the positive saved charge delay with signed truncation, and clamps to
`0..TurretCount-1`. The corpus executes startup, frame boundaries, stopped
timers, negative durations, clock wrap, multiplication overflow and gate
controls. This block changes no weapon, timer or RNG state.

The charge block runs after target validity and recoil updates, before
TransitionTimer, mission/passive dispatch, managers and StageClass advancement
inside TechnoAI. Unit firing occurs later, so a newly started shot's charge
frame updates on the next visit. The fire-writer rows execute the original
post-GetROF stores, including ordinary Berserk halving toward zero and the
DiskLaser path's unhalved copy. ROF computation and weapon admission remain
with their existing owners; these rows do not establish Prism beam parity.

## Loader and drawing limits

The indexed turret loader admits UnitType, positive TurretCount and
`!IsGattling`, independently of `Turret`. Its loop visits `0..TurretCount`
exclusively and stops on the first failed turret load. The separate barrel
loop has an additional `Turret=yes` gate. Filename rows execute the original
formatters: index zero uses `FVTUR`/`FVBARL`; subsequent indices use suffixes
`1`, `2`, `3`. The loop rows supply load success/failure explicitly; they do
not execute archive IO or voxel parsing.

Static connected loader bodies establish that absent BARL VXL is optional,
while absent or malformed required TUR/HVA can fail the whole model. The
failure tail `0x5F8A6A` clears body VXL/HVA and calls indexed cleanup
`0x5F8080`. A Rust safe missing/custom-asset policy is not native parity for
that malformed-content path.

Draw admission is `Turret || (TurretCount>0 && selected!=-1)`. The indexed
consumer requires positive TurretCount and `!IsGattling`; otherwise it uses
the plain TUR/BARL pair. Its cache key includes `selected << 24`, and the
turret and barrel pairs are `Type+0xC8+8*selected` and
`Type+0x158+8*selected`. Native drawing does not bounds-check that number.
For example, selected `-1` with `Turret=yes` can alias the plain barrel pair;
it is not a universal native "omit all parts" command. Invalid-mod omission
in Rust must be identified as a safe bounded policy.

The stock Unit shadow tail draws the body pair at `Type+0xB0` with ShadowIndex
and a body/locomotor-derived key; it does not incorporate current turret
selection. This is instruction-level evidence. The corpus does not execute
GPU/raster output, shadow composition, palette handling or crash rendering.
Production visual captures and asset-backed load checks are separate evidence.
