# Native recoil state and retained rules

`recoil.py` executes original instructions from retail `gamemd.exe`.
`recoil.json` records constructor values, eight reader controls, retained-layer
controls, four firing gates, thirteen histories and four arm-helper comparisons.
The physical GTGCAN, CAEAST02, NASAM and YAGGUN rows record source hashes and the
actual original cached INI reads. The sidecar records Unicorn identity.

Supply a flat extract containing the original `RULESMD.INI`, `MPBattleMD.ini`,
and `LANGRULE.INI` if present. The default map is the repository
[Grand Cannon observation fixture](../map_observation/examples/grand_cannon_barrel.map);
`VERA20K_RECOIL_MAP` can override it when deliberately regenerating a comparison. Extract them
with the repository `asset` tool from the active retail lookup path, not RA2 base
INIs or `--all-mixes`.

```sh
VERA20K_RECOIL_ASSETS=/path/to/extract \
VERA20K_RECOIL_MAP=tools/map_observation/examples/grand_cannon_barrel.map \
python -m tools.voxel_oracle.recoil --check
```

`--check` verifies existing references without rewriting them. `--write` is an
explicit publication of newly executed original instructions; it is not a way to
accept arbitrary changed Rust outputs.

## Readers and ownership

The type constructor `0x711412..0x711454` sets `TurretRecoil=false`, and both
parts to `[travel=2, compress=1, recover=1, hold=1]`. The original preceding
instructions set `ECX=2` and `EAX=1`; zero is the established constructor register.

The retained rules reader `0x715269..0x7153DA` reads these exact-case keys from
the supplied rules INI and type section:

1. `TurretRecoil`, using original `ReadBool` and the retained flag as default.
2. `TurretTravel`, `TurretCompressFrames`, `TurretHoldFrames`,
   `TurretRecoverFrames`, using original `ReadInt` and retained turret defaults.
3. Copy the entire turret recoil configuration into the barrel configuration.
4. `BarrelTravel`, `BarrelCompressFrames`, `BarrelHoldFrames`,
   `BarrelRecoverFrames`, each using the **turret** value as its default.

The copy is part of every admitted type-section pass (`0x715320..0x71532B`).
A later section without barrel keys therefore resets the barrel's defaults to the
current turret values. An absent section does not admit this reader at all. The
corpus includes both cases. Duration setters `0x717A50`, `0x717A80`, `0x717AB0`
clamp nonpositive integers to 1; travel `0x717A30` retains its signed integer.
The eight controls include malformed native scanner inputs and wrong key case.
No Python numeric parser supplies these native fields.

Techno construction `0x6F2F61..0x6F2FA9` initializes both runtime configurations,
zero displacement and inactive state. Unused inactive velocity/countdown words
are normalized to zero by the fixture and do not assert native initialization.
`InitManagers` `0x6F4277..0x6F42E3` copies the type's four integers into each
runtime part. Original virtual GetType executes on a Building vtable.

## Trigger, update and finish

The selected successful FireAt tail `0x6FF0B7..0x6FF15B` requires virtual
`HasTurret` and type `TurretRecoil`. It arms turret then barrel independently.
A zero travel part is unchanged. Otherwise it sets compression state 1, countdown
`max(compress,1)`, and velocity `travel/countdown`. It **does not zero existing
travel**, so refiring during compression, hold or recovery preserves displacement.
The standalone body `0x70ECE0` performs the same part operation; original
Mission_Missile calls it at `0x44D4B0/0x44D4BB`. Four comparisons establish its
bounded equivalence to the active inline FireAt writes. This shared operation
has one Rust owner.

The admitted TechnoAI slice `0x6FA4D1..0x6FA4FB` checks `TurretRecoil`, then
updates turret and barrel, in that order, with `0x70ED10`. Every active update
adds velocity and decrements the integer countdown before handling its boundary:

- Compression state 1 finishes by entering hold state 2, loading `HoldFrames` and
  zeroing velocity. If the hold is 1 or less it immediately enters recovery.
- Hold state 2 finishes by entering recovery state 3, countdown `max(recover,1)`,
  velocity `-travel/countdown` based on configured travel, not accumulated travel.
- Recovery state 3 finishes by clearing state and setting travel exactly to 0.

These are visit countdowns, not Scenario clock timers. No RNG draws or detach
calls occur in the selected blocks; the fixture rejects Scenario RNG entry.
The compared boundaries supply an already successful shot and admitted AI visit.
They do not claim full FireAt admission, damage, mission scheduling, EMP cannon
mission behavior or rendering. The renderer's original recoil transforms and
cache-key decisions are compared in [building_barrel.md](building_barrel.md).

## Stock result and numeric policy

Physical retail GTGCAN and CAEAST02 both read `TurretRecoil=yes`, turret travel 0,
and barrel `[travel=8, compress=3, recover=40, hold=3]`. NASAM and YAGGUN retain
recoil disabled. The native GTGCAN/CAEAST02 histories execute sixty AI visits:
compression finishes on visit 3, hold on 6, recovery on 46, followed by an inactive
zero-displacement tail.

Native `FPCW=0x0E7F` truncates each stored float. Thus stock `8/3` yields velocity
`0x402AAAAA`; peak travel after three updates is `0x40FFFFFF`, just below8.
The corpus retains those exact bits as native evidence. Plain Rust `f32` uses
nearest rounding. The labelled `rounding_comparison` executes the same original
instructions under alternate `FPCW=0x027F`, not retail mode: over sixty stock
visits it preserves all states/countdowns and differs by at most
`0.000009775161743164062` in velocity or travel. Both finish at exact zero.
This supports a small, explicit presentation precision residual; it does not
establish exact native floating-point parity or bound arbitrary custom rules.
No x87 emulation is required for this invisible subpixel discrepancy.

Rust comparisons live in `rules::recoil::tests` and
`game_entity::voxel_recoil::tests`. They compare the original saved state and
countdowns exactly, with float tolerances of 0.00002 for the ordinary histories
and 0.0001 for the forced rapid-refire control. The production
`grand_cannon_recoil_follows_successful_launch_ai_and_snapshot` regression sends
real commands, waits for the admitted shot, advances the normal AI and resumes
the cycle through a saved game. Recoil lives in one private optional entity
component; it consumes no RNG and contributes no simulation hash fields.
