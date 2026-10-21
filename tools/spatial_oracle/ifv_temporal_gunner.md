# Temporal gunner timer handover

The separate `--temporal-gunner` mode of
[`ifv_turret_switching.py`](ifv_turret_switching.py) composes the existing
BulletReader VM, physical lexical INI cache and native oracle runner. The
historical default mode and its GetROF=19 contrasts remain separate. New
expected values come only from original execution, never a Python ROF formula
or Rust output. Generation and replay must pass before accepting a new corpus.

## Reproduce

Select a supported original `gamemd.exe` as described in
[`native_oracle.md`](../native_oracle.md). Provide a physical extract containing
`RULESMD.INI`, `ARTMD.INI`, `MPBattleMD.ini` and optional `LANGRULE.INI`; provide
the selected ordinary production map as INI text, not a MIX archive.

```sh
VERA20K_SHRAPNEL_INPUTS=/path/to/physical-inputs \
VERA20K_IFV_MAP=/path/to/ordinary-temporal-gunner.map \
  python -m tools.spatial_oracle.ifv_turret_switching --temporal-gunner --check
```

`--write` explicitly generates the small consumed `ifv_temporal_gunner.json`
and `ifv_temporal_gunner.meta.json`. The payload retains physical bytes/hashes,
selected lexical strings, original read defaults and retained results. The
sidecar records the entry points, assumptions, transport and replay command;
it contains no executable or repository source hashes. The runner guards
loaded Python sources during generation. Imports and `--help` do not execute
the native image. The normal IFV mode's independent `--check` protects its
retained output.

## Native chain and physical readers

Whole Unit ReceiveGunner `746420..7464DC` and RemoveGunner
`7464E0..7465A4` execute using original concrete vtables. Their actual `+318`
target is GetROF `6FCFA0`; its actual GetWeapon `70E140`, normal/elite type slot
getters `7177C0/7177E0`, rank predicates, x87 arithmetic and Scenario RNG run.
No gameplay return is substituted.

Receive samples the departing passenger's rearm before transferring Temporal,
sets temporary mode7, increments the receiving IFV's burst and queries slot7
when the sampled timer remains nonzero. It restarts the receiver at the current
frame, then selects the passenger type's final mode. Stock CLEG final mode10
does not change the earlier query: slot7 is FV Weapon8 `CRNuke`, while final
mode10 is Weapon11 `CRNeutronRifle`. Remove samples the departing IFV timer
before transferring the manager. Its actual LetGo `71ABC0` precedes the
receiving CLEG's burst increment, GetROF(0) and timer restart; it then resets
IFV mode0. Rank, House and burst belong to the receiver at that query point.
The callbacks perform INC without FireAt's signed remainder and do not write
charge delay `+2F8`.

The mode executes original FV `7470D0/747620` and CLEG `5236A0/5240A0`
constructors/readers, fixed ClegSequence `523D00`, full selected Weapon
constructors/readers `772FA0/771C70/772080` and ChronoBeam `75D3A0` across
RULESMD, optional LANGRULE, Battle and the exact supplied map. The four weapon
types retained are `CRNuke`, `CRNeutronRifle`, `NeutronRifle` and
`NeutronRifleE`. Other physical type references can be constructed by those
readers but their unrelated gameplay is outside this component.
The original Warhead registry at `8874C0` has capacity before those readers
run. All three ChronoBeam references must resolve to one actual pointer before
its whole reader executes; incomplete registry setup fails this input gate.

Rules `665650`, selected physical General keys through whole `66D530`, whole
ReadDifficulties `674500 -> 66D270`, and selected CombatDamage keys through
whole `66BBB0` establish consumed defaults and multipliers. Physical Countries
and Sides declarations initialize the original type registries. Original
Country `5113F0/511850` and House SetDifficulty `4F6EC0` produce the consumed
HouseROF for declared mode5, country and difficulty. The contrast uses
Americans/difficulty0 for the IFV and Russians/difficulty2 for the passenger;
this supplied cross-House context proves query ownership, not ordinary
boarding admission or full House construction.

Original InitManagers `6F4154..6F41C0` executes real slot0/warhead admission,
allocation, Temporal constructor `71A4E0`, owner binding and weapon Damage
copy before case snapshots. Runtime actor construction, cargo/command
admission, parent binding and world placement remain explicit boundaries.

## Controls, outputs and limits

Twelve small cases cover active rookie receive/remove/retry/removal, exact
deadline and stopped timers, absent manager, NULL passenger removal, receiving
veteran/elite contexts, receiver House context, burst values above weapon Burst,
signed INC overflow at `2147483647`, and a native FireAt rearm producer fragment.
The elite-passenger contrast also uses a distinct Scenario seed.

Each row records its literal input, actual House results, initial actor/link
state and full Scenario/Main/MapGen states. Each operation records before/after
actor and link fields, every full RNG state after it, observed original calls
and actual returns, ordered raw draw/rejection values/counts and object writes.
No RNG reset occurs between operations. Main and MapGen are initialized and
compared in full. Counts come from original raw-consumer sites, not an assumed
entry counter. `*_bits` use canonical hexadecimal integer bit patterns;
`difficulty_rows` and full RNG states use little-endian memory byte hex.

The supplied already-warping single-head target is a live full-health Unit,
with retained remaining700 and no other managers. Actual LetGo clears its head
and warped flag and calls original FootMark(2) `4D3780`; that immediate true arm
requires no map reconstruction. No building/House warning, target warp AI,
erase, multi-head ordering or rendered effect is claimed. NULL-passenger
removal intentionally retains the existing manager/target; it is a guard
control, not a cleanup success. Remove/retry is the callback sequence after a
failed exit; failed Unlimbo itself is a production test boundary.

The `actual_fire_rearm_tail` case additionally executes the existing native
burst owner's `6FF274..6FF2D1` with real GetROF, original timer/charge stores
and signed remainder. This starts **after shot effects**; it does not establish
full FireAt, bullet or Temporal-shot admission. A Rust consumer may import the
measured producer boundary and compare the subsequent callbacks without
claiming a Rust FireAt-tail comparison.

Native timer `+2F0` copies an uninitialized local stack word. The harness clears
caller scratch storage for reproducibility and retains that raw write, but the
logical timer comparison uses frame/duration only. FPCW `0E3F` and the ftol
cache are supplied as in the existing Mission owner. Setup uses the existing
allocator/delete/TLS, physical file-byte and GUID OS transports; visual asset
loads and CRT atexit are bounded setup transports. Original code is unchanged.
No RGB, audio, ordinary production admission or whole Temporal/Unit parity
follows from this component corpus.
