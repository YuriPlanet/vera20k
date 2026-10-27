# Optional Phobos InfantryAutoDeploy

Build with `--features phobos` to let computer-controlled GI-like infantry
deploy when they engage, as they did in Red Alert 2.

```ini
[General]
InfantryAutoDeploy=false  ; boolean, default no

[SOMEINFANTRY]            ; InfantryType
InfantryAutoDeploy=       ; boolean, defaults to [General]
```

`InfantryClass::Approach_Target @ 0x00522340` splits on the current Doing:
27..30 take the deployed arm at `0x00522378`, every other value runs the Foot
body at `0x0052236E` and returns at `0x00522373`. That return is where RA2's
deploy check used to be, which is why Yuri's Revenge AI infantry stopped
deploying. The hook `InfantryClass_ApproachTarget_InfantryAutoDeploy` replaces
it with a jump back into `0x00522378`, so the undeployed man runs the arm at
`0x00522434` once the Foot body has finished.

That arm keeps native's gates unchanged: a computer house
(`0x00522378..0x00522386`), `Deployer=`/`DeployFire=`, a non-`AreaFire=` weapon
in the `DeployFireWeapon=` slot, an in-reach CanFireAt with it, and the caller's
zero no-move argument. A moving locomotor is stopped (`ILocomotion+0x48`, whose
own `+0x54C` callback runs first), Infantry `+0x6E4` pending-deploy is armed and
one Scenario `RandomRanged(0, 2)` is drawn and discarded
(`0x0052249C..0x005224C1`); a stopped one requests unforced `Do_Action(27)`.
CanFireAt answers true for a null Target (`0x006F77BB`), so a man whose Target
the Foot body has just cleared passes the reach gate too.

Source: `src/Ext/Infantry/Hooks.cpp`, `src/Ext/InfantryType/Body.cpp` and
`src/Ext/Rules/Body.cpp` (by TaranDahl).

## Where VERA20K owns it

- `rules/ruleset.rs` — `[General] InfantryAutoDeploy` and
  `RuleSet::infantry_auto_deploy_for`.
- `rules/object_type.rs` — the per-type key, read for `InfantryTypes` only.
- `sim/movement/foot_approach.rs` — `approach_target` enters
  `approach_infantry_deploy_arm`, the one port of `0x00522378..0x005224C6`
  that the retail Doing 27..30 entry also uses, after the Foot body when the
  key resolves true.

## Residuals

- The deploy request itself belongs to the existing `Do_Action(27)` owner; the
  arm only decides when to ask.
- A retail-rules test drives a still computer GI's deployment; the moving
  arm (Stop, pending deploy, RandomRanged) follows the cited instructions
  without a test.
- `Get_MissionControl` at `0x005224A8` only supplies a pointer the arm
  discards, so it has no port.
