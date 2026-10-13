# Infantry deployment handler and shared Stop receiver

The optional mode in the existing `infantry_deploy_action.py` owner executes 52
whole original `InfantryClass::Mission_Unload` bodies (`0x0051F6E0`). It uses the
same supplied Infantry/type/42 signed sequence records/House/Walk fixture as the
355 historical deployment rows. Neither those rows nor their metadata change.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.infantry_deploy_action --mission-unload --check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.infantry_deploy_action --check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.infantry_action_callback --check
```

The companion JSON and metadata pin active-retail `gamemd.exe` SHA256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`, the executed
original code slices, the actual Infantry and constructor-installed Walk vtables,
and the generator hash. Only OS interlocked imports are supplied. No gameplay
body or vtable entry is replaced. Scenario/Main/MapGen are separately seeded31 by
original `0x0065C6D0`; all three complete RNG objects are recorded before and
after each row and remain unchanged in this handler corpus.

The original call order is forced DoAction27/31, the live SprayAttack slot query
`0x0070E120 -> 0x0070DD70 -> 0x0070E140`, optional AreaFire handling, direct Guard
assignment `0x005B2FD0`, and Infantry NULL destination `0x0051AA40`. The slot is
`SprayAttack ? 0 : 1`; it is not GetCurrentWeapon `0x0070E1A0`. `DESO` is the
literal at `0x0082557C`: its AreaFire delay is the signed Deploy count plus one,
then an authored nonnegative UndeployDelay takes precedence. Other AreaFire
weapons target the original Map-resolved current navigation Cell. A refused
DoAction still reaches the assignment and destination suffix. The fallback
`0x004DA2B0 -> 0x005B2EF0` returns450 without a cadence draw.

The rows cover Doing0 and27..31, human class destination refusal, automatic delay,
missing Deploy/Undeploy records, both SprayAttack slots, absent weapon,
DESO/ordinary AreaFire, existing Cell target/firing, and pending deployment with
an absent or retained paid Walk head. Ordered field writes and calls are retained.
Walk Stop `0x0075ADA0` clears destination first; only with no head does it clear
both moving bytes and invoke Infantry callback `0x00521B40`. That callback clears
private6E4 before its unforced Deploy27 request, preserving recursive Stop order.

Rust comparisons in `src/sim/deploy_tests.rs` feed these declared supplied inputs
through the production RuleSet/ART readers and controlled instance owners. They
also compare196 historical Stop/callback rows, all64 Deploy/Undeploy sequencer
rows (including the below-count controls), and14 passive targeting timer controls.
The14 controls execute `0x0070F770`, which shortens+180/+188 with an original
Scenario(4,8) draw only above10 remaining frames and outside globalA8E7AC's bracket.
It does not modify weapon rearm. On completed Deploy27, forced Deployed28 is
followed by independent crush2A4/timer effects even when Deployed28 refuses;
Undeploy31 requests Ready0 then changes2A4 without this timer draw.

The new52 rows supply Type fields and sequence counts explicitly; they do not
execute the Type/INI constructors or establish retail defaults. Rules+1768 is
supplied0 and bound in Rust through the existing `[AI]BlockagePathDelay` reader.
Cell10,10 is a supplied native map-table receiver with no tube. Comparisons bound
the retained fields in the JSON; they do not claim full loaded-world, full timer
auxiliary storage, waypoint, projectile, audio playback or scheduler parity.
The104 timer auxiliary dword is recorded native stack residue with no represented
decision. The command publication regression is backed by original caller
reading; its Rust validation results are recorded by the mechanism owner. These52
rows begin at the handler, not the EventClass prefix.

Still required outside this supplied handler boundary: `CanDeploy700D50`'s
`Cell484AE0` tube-neighborhood UI admission; a nonempty Foot+5AC object-vector and
its producers/cleanup (distinct from NavQueue+588/+598); complete retail deployment
reader closure; ordinary AI ordering and discharge/damage/retirement. The existing
44 `infantry_action_callback` rows retain whole zero-health action/Stop recursion
with a separately supplied current-cell CanEnter answer; this mode supplies only
positive health and does not claim those collision premises.
