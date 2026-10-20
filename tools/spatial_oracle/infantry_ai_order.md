# Infantry AI caller order

`infantry_ai_order.py` executes the original Logic object loop
`55B5FF..55B61B` over one supplied Infantry. Its original primary vtable
`7EB058 + 5C` resolves to `51BAB0`; that whole caller executes unchanged.
The 22 controls establish caller ordering and gates. **They do not execute a
complete shot, full Foot AI or a full Scenario.**

From the repository root, with the retail executable configured:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python tools/spatial_oracle/infantry_ai_order.py --check
```

`--check` is the default and writes nothing. `--write` explicitly regenerates
the result and metadata. Only `VERA20K_GAMEMD_EXE` (or `RA2_DIR`) is needed;
these are supplied runtime/type inputs, not physical INI/type-loading controls.
The metadata pins imported owners and saves the original caller instructions,
constructor field stores, Thief gate, readiness body and checksum reads.

## Established order and gates

| Original instruction | Effect |
|---|---|
| `51BAB8..51BAD9` | Signed Foot+684 nonnegative: Tube update `51B350`, virtual+4A0(0), return. |
| `51BADA..51BC17` | Warp predicates read Techno+270/+271; optional warp visual/link/locomotor work. A nonzero+270 clears target/destination as needed and returns before Foot AI. |
| `51BC1C`, `51BC51` | First live Ready query, optional Idle(0,1) if effective mission and queue are NONE, then Commence. |
| `51BC57..51BC9D` | Health<=0 becomes1 except Doing11..15,20,21,34..36. |
| `51BC9F -> 4DA530` | Foot AI, including its own common mission/movement work. |
| `51BCA4..51BCAC` | Object+90 false exits the remaining Infantry suffix. |
| `51BCB2..51BDCF` | Guard/AreaGuard current-building Scatter path, behind Object+81=false. |
| `51BDD3..51BDE7` | Query locomotor display layer; layer!=2 calls Mark(2). |
| `51BDE7..51BE3E` | Firing+68D and stage-repeat+10C==0: clear68D, then DoAction(28,0,0) for Doing27..30, otherwise DoAction(0,0,0). |
| `51BE3E..51BEBE` | Team+82 update when Team and Techno+3D5; teamless Guard failing568350 calls virtual+3A0 then UnInit and returns. |
| `51BEC0 -> 5202F0` | TryThiefVehicleCapture; AL!=0 exits. The actual initial type+EC5=false arm executes in ordinary rows. It is not the Engineer repair/capture handler. |
| `51BED1`, `51BF03` | Second **live** Ready query and conditional Commence; optional Idle(0,1) for NONE/NONE. Ready=false does not skip the subsequent firing tail. |
| `51BF0B -> 5200B0` | Fear handler. `51BF08` is not a CALL boundary. |
| `51BF10..51BF57` | Optional raw6DA latch clear, detailed below. |
| `51BF59 -> 5206B0` | FireAtTarget. |
| `51BF5E..51BF66` | Object+90 false skips sequencer and movement actions. |
| `51BF6A -> 520AE0` | Sequencer. |
| `51BF6F..51BF77` | Object+90 false skips movement actions. |
| `51BF7B -> 520F40` | Movement actions, then return. |

Original Ready `521B60` and Commence `5B3570` execute in the controls. A
supplied Foot callback changes Walk's retained motion state in one row:
Ready answers0 before Foot, then1 and promotes queued Attack afterward. Its
unchanged-moving control answers0 twice and retains Move/queuedAttack, while
still reaching Fear, Fire, sequencer and movement actions. The predicate also
reads the supplied native Walk head and speed fraction; a moving byte alone
does not establish IsMovingNow.

Three firing-latch controls execute the original DoAction body as well as the
caller clear. A firing latch with zero stage-repeat interval admits the second Commence after
the first Ready refused it; repeat2 retains the latch. The deployment-family
contrast requests Doing28. These controls use supplied sequence records,
not retail ART or a complete firing sequence producer.

## Raw6DA and retained timer

This packet deliberately leaves the byte unnamed. It is distinct from prone
at6DB and the Doing27..30 deploy family. Constructor stores at517A7F/517A85
anchor6C8 to the current frame and clear duration6D0;517A9E clears6DA.
The AI clear requires NavComNULL, pronefalse, nonzero6DA and timer remaining0.
It occurs after Fear and before Fire. Native controls cover before/due,
stopped nonzero/zero, NavCom/prone retention, signed wrapping deadline and
raw1/255. Both nonzero byte values clear to0 at the due boundary. The timer
itself is retained. Checksum521C90 reads the timer remainder and byte6DA.

A direct-displacement/overlapping-width scan of all executable file bytes
found only the Infantry constructor clear, AI read/clear and checksum read
for6DA; other overlapping references are in Building/Unit bodies. This scan
does **not** prove the absence of aliases, wider indirect copies or raw-load
states. No nonzero arming producer or Rust field owner was established. The
fresh ordinary constructor state bypasses the clear; retained nonzero input
lifecycle remains an explicit separate state question.

## Boundaries and production consumers

The existing `infantry_deploy_action.Fixture` owns mapping, supplied object/type
layout, original Walk construction, stack calls and native Scenario seeding.
The new script supplies one-member Logic membership. It does not construct an
Infantry or register/remove it. It observes and returns at FootAI, FireAtTarget,
sequencer and movement-action entries; selected callback controls clear alive
to test the caller's next gate. The positive Thief and Tube branches are also
declared callback boundaries. No original code or vtable is patched.

All three RNG streams are originally seeded31 and their complete states are
unchanged by these bounded visits. This is **not** a no-draw claim about the
substituted bodies. No heap/list/detach behavior of a supplied destruction
callback is established. Guard/AreaGuard building scatter, off-map cleanup,
Temporal link/warp visual, team and full Idle arms have instruction evidence
only here.

Rust integration belongs to the existing per-object turn in
`src/sim/world/object_turn.rs`, shared firing in
`src/sim/combat/world_receiver.rs`, fear owner in `src/sim/infantry.rs`, and
sequencer/movement-action owner in `src/sim/movement/infantry_action.rs`.
The latter documents that `pending_infantry_fire` approximates native68D's
lifetime; the new caller-order witness does not close that separate retained
firing-latch representation gap. Production mixed Infantry/Unit same-frame
fire tests establish the host-position fix separately from these native
callback controls.
