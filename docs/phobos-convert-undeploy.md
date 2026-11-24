# Convert.Deploy / Convert.Undeploy

An infantryman that finishes its Deploy or Undeploy sequence can convert to
another type once per sequence.

```ini
[A]                                ; InfantryType
Convert.Deploy=                    ; Valueable<TechnoTypeClass*>, absent/none = off
Convert.Undeploy=                  ; Valueable<TechnoTypeClass*>, absent/none = off
```

Readers `Ext/TechnoType/Body.cpp:1230` (`Convert.Deploy`) and `:1014`
(`Convert.Undeploy`), declarations `Ext/TechnoType/Body.h:249-250`,
constructors null at `:681-682`. The Unit simple-deployer arm reads the same
`Convert.Deploy` key through its Ares owner.

## Behavior

`InfantryClass_DoingAI_DeployConvert_Deploy @ 0x520B3E` and
`InfantryClass_DoingAI_DeployConvert_Undeploy @ 0x520B99`
(`Ext/Infantry/Hooks.cpp:18-50`), in the DoingAI completion case: when the
type sets the matching key and its latch (`HasDeployConverted` /
`HasUndeployConverted`) is false, set that latch, clear the other, and call
`TechnoExt::ConvertToType`. Both latches are reset by
`InfantryClass_DoingAI_DeployConvert_ResetFlags @ 0x520E75`
(`:52-66`) whenever the current `SequenceAnim` is neither Deploy nor
Undeploy, so the next matching sequence converts again.

## Residuals

The deploy and undeploy latches are separate persisted fields: each arm sets its own and clears the
other, and the `0x520E75` reset clears both. Nothing else in the conversion shares them.

## Where VERA20K owns it

`rules::phobos_convert` reads the key. `Simulation::infantry_deploy_completion`
(`sim/deploy.rs`), the production owner the infantry sequencer calls with
`completed_doing == 31` for the Undeploy completion, runs the conversion
through the single `convert_techno_type` owner before the completion's
Ready `Do_Action`, as the native hook does. The `HasUndeployConverted`
latch is a new phobos-gated `GameEntity` field (serde default, hashed only
when set). `infantry_sequencer` (`sim/movement/infantry_action.rs`) runs the
`0x520E75` reset after its action dispatch whenever the current sequence is
not Deploy (27) or Undeploy (31).

The two latches are separate persisted phobos fields; each arm sets its own
and clears the other, and the `0x520E75` reset clears both.
