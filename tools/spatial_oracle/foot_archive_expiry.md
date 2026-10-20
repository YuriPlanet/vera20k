# Foot ArchiveTarget pointer expiry

The separate `--archive-expiry` mode in [foot_attack_move.py](foot_attack_move.py)
executes complete original Unit, Infantry and Aircraft expiry handlers. It uses
[the retail image and shared runner](../native_oracle.md), original
concrete vtables and no substituted callables. The default attack-move mode's
638 rows, including its bounded saved-target expiry slice, retain their prior
payload bytes and native metadata values.

```sh
python -m tools.spatial_oracle.foot_attack_move --archive-expiry --check
python -m tools.spatial_oracle.foot_attack_move --check
```

`--write` deliberately regenerates the selected corpus; `--output` selects a
separate payload/sidecar pair. The optional mode records its 22 original rows in
[foot_archive_expiry.json](foot_archive_expiry.json) and the binary, original
instruction/vtable spans and scope in
[foot_archive_expiry.meta.json](foot_archive_expiry.meta.json).

## Native identities and order

| Receiver | Original vtable | PointerExpired entry | Call to Foot |
| --- | --- | --- | --- |
| Unit | `0x007F5C70` | `0x007446E0` | `0x007446EE` |
| Infantry | `0x007EB058` | `0x0051AA10` | `0x0051AA1E` |
| Aircraft | `0x007E22A4` | `0x0041B660` | `0x0041B66E` |

Each original concrete slot `+0x28` dispatches to its complete entry. The chain
runs Foot `0x004D9960` → Techno `0x007077C0` → Radio `0x0065AAC0` → Object
`0x005F5230`, then unwinds through the remaining original bodies. `0x004D9970`
and `0x00707990` are interior instructions, not expiry function entries.

Techno's nonzero-control branch clears a matching ArchiveTarget `+0x218` directly
at `0x00707B03`. After Techno returns, Foot compares the same field with the
expired pointer at `0x004D99F1`. If it still matches, Foot calls the original
plain setter `0x0070C610` at `0x004D99FC`, writing zero at `0x0070C614`. Thus
control zero reaches the Foot setter, while control one usually arrives at
Foot's comparison with an already cleared archive. An unmatched pointer survives
both controls. This Foot behavior does not establish an unconditional archive
clear for non-Foot classes such as buildings.

TarCom `+0x2B4` is independent. On control zero, original Techno's receiver-house
sensor query (`0x00707994` → `0x004870D0`) or same-owner exemption
(`0x007079C2..CB`) can preserve a matching TarCom. Foot still clears a matching
ArchiveTarget afterward. The sensor query uses the expired object's original
physical `+0x48` coordinates and original Map `0x00565730`, without navigation
head, bridge-layer or ground-height queries. Unit/Aircraft target-clear controls
also execute actual `AssignTarget(NULL)` `0x006FCDB0`, then the native suspended
mission query `0x005B3A10`, before the archive clear.

## Corpus and explicit fixture inputs

The JSON `fixture` records shared input fields and addresses; each row's `input`
records family, control, expired flags, sensor count, pointer matches and owner
identity. All values are supplied state, not outputs inferred from Rust.

- Twelve archive-only rows cover all three families, both controls and matching
  versus unmatched archives. `expired_flags=0` probes the non-Techno/sensor guard;
  it is not a valid constructed retail Unit or evidence of a native Ghost class.
- Six live Unit-victim rows cover all families with a matching archive and
  TarCom, control zero, and either same owner with sensor count zero or a
  different owner with sensor count one.
- Four live Unit-victim rows cover Unit/Aircraft target clearing with a different
  owner, sensor count zero and both controls.

Receivers have original class tables, flags 7, alive byte 1, health 100, Rescue
mission 21, no queued or suspended mission, zero passive-scan duration and zero
unrelated links. Infantry's supplied DoType is zero. A live victim uses the
original Unit table, flags 7, alive byte 1, health 100 and physical XYZ
`[1408,1408,416]`. Owner and sensor choices are independent row inputs.

The map has a zeroed 262144-slot table with fixed stride 512. Cell `[5,5]` has
its original vtable and an explicit signed receiver-house sensor word at
`+0x7C`. Receiver-house index is zero. The shared Dummy's initial packed
coordinate `[1234,-2345]`, level 127 and slope zero are explicit; these rows
resolve the real cell. Full map construction, cell occupancy and sensor producers
are outside the fixture.

Empty original-vector table `0x007E91EC` is supplied from the original Techno
constructor load `0x006F304C` and stores `0x006F3099` (`+0x458`) and
`0x006F30C4` (`+0x470`). Original empty-vector find `0x004E0550` executes.
Construction itself is excluded. Cargo, radio slots, manager links, target lists
and navigation queues are empty fixture inputs. No hook edits native code,
vtable bytes, registers or results; hooks observe executed entries and memory
writes only. Original RET, stack cleanup, callee-saved registers, instruction
spans and all three concrete vtable byte sequences are checked.

## Output and evidence bounds

Each row exposes final `archive` and `target` pointer values, ordered `events`
with native addresses and their pre-call fields, setter arguments, and actual
`writes` with native writer address and before/after values. Native
PointerExpired has no Boolean return contract; incidental EAX/AL is not treated
as a verdict. The 22 rows reproduce the original experiment's canonical row hash
`503e0fddd2f6b994ade4e8ceb274ef4ac9e9ba94f6781131b45edc377897e420`.

This establishes the archive-clear outcome and its ordering for the declared
complete receiver paths. It excludes full-world detach traversal and object
registration, cloak/dive or UnInit triggers, manager forwarding, nonempty queue
compaction, RestoreMission, save/load, and complete Rescue/AreaGuard behavior.
Infantry target clearing's DoAction/sequence effects are excluded; its
archive-only and target-preserve rows still execute complete Infantry expiry.
Passive duration zero avoids the conditional Scenario `RandomRanged(4,8)` rearm;
its nonzero-timer RNG/timer behavior is not established by this corpus. The
flags-zero archive guard probes and common live-victim target paths are distinct
cases, even though both can clear the same saved anchor.
