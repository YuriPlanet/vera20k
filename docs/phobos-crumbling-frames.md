# Terrain crumbling frames

Build with `--features phobos` to let a terrain object play a crumbling stage
after a lethal hit instead of vanishing in the same call. Stock gamemd removes
a destroyed tree immediately; with `HasCrumblingFrames=yes` the object stays
live at zero health, draws a crumbling animation, and is removed only when the
stage reaches half the SHP's frames.

```ini
[SOMETREE]                          ; TerrainType, rulesmd.ini
HasCrumblingFrames=no               ; boolean, default no
CrumblingSound=                     ; sound name, played when crumbling starts
DestroyAnim=                        ; anim list, one random pick at removal
DestroySound=                       ; sound name, played at removal
HasDamagedFrames=no                 ; boolean, the SHP holds a damaged half
AnimationLength=                    ; integer, unset = Frames / (2*(HasDamagedFrames+1))
```

`HasCrumblingFrames`, `CrumblingSound`, `DestroySound`, `HasDamagedFrames` and
`AnimationLength` were already read; this page covers the `DestroyAnim` list
(`ValueableVector`, one registered entry picked per removal) and the consumer
lifecycle.

## Behavior

A lethal hit on a `HasCrumblingFrames` type (`TerrainClass_TakeDamage_NowDead_Add`,
`0x71BB2C`, `Ext/TerrainType/Hooks.cpp:286-307`) skips `PlayDestroyEffects` and
the same-call UnInit: it joins the object to Logic, plays `CrumblingSound` and
restarts its Stage at rate 2 / value 0 (`Start_To_Crumble @ 0x71C8E0`). The
object keeps health 0 and stays drawn.

Two non-lethal triggers start the same animation without the lethal arm:

- `TerrainClass_FireOut_Crumbling` (`0x71C6EE`, `:239-257`) is the zero-Health
  tail of `TerrainClass::Finish_Fire_Death` (`0x0071C6B0`, the tree's
  vt+0x17C): a burning tree that reaches zero Health and whose last fire anim
  leaves starts `Start_To_Crumble` instead of vanishing. A type without the
  key takes the stock vanish.
- `TerrainClass_TakeDamage_SetContext` (`0x71B965`) stores the pre-hit
  `GetHealthPercentage()`, and `TerrainClass_TakeDamage_RefreshDamageFrame`
  (`0x71B98B`, `:265-284`) latches `IsCrumbling` on a **non-lethal** hit when
  `!IsAnimated && HasDamagedFrames` and the ratio crosses
  `ConditionYellow.Terrain` downward (`prior > yellow && now <= yellow`), joining
  Logic so the next draw takes the crumbling arm. Native's own comment calls
  the flag a dirty hack; this arm restarts no Stage and plays no
  `CrumblingSound`. The next `TerrainClass_AI_Crumbling` visit clears it while
  Health is above zero, so it lasts one frame.

`ConditionYellow.Terrain` is `[AudioVisual] ConditionYellow.Terrain`
(`Ext/Rules/Body.cpp` `ConditionYellow_Terrain`), defaulting to the base
`ConditionYellow`. It is parsed and has one `RuleSet` owner that both the
damage latch and `TerrainClass_Draw_PickFrame` read.

Each Logic visit while crumbling (`TerrainClass_AI_Crumbling`, `0x71C812`,
`Hooks.cpp:65-95`) steps the Stage and folds
`currentStage = Animation.Value + (IsAnimated ? AnimationLength*(HasDamagedFrames+1) : HasDamagedFrames)`.
When `currentStage + 1 == GetImage()->Frames / 2` it plays
`PlayDestroyEffects` (`DestroySound` at the coords plus one random registered
`DestroyAnim`, `Ext/TerrainType/Body.cpp:15-21`) and removes the object. The
`HasDamagedFrames && Health > 0` arm instead clears `IsCrumbling` (and leaves
Logic when the type is neither animated nor flammable), resuming the ordinary
draw.

The draw (`TerrainClass_Draw_PickFrame`, `0x71C1FE`, `Hooks.cpp:96-126`) selects
`1 + Animation.Value` for a non-animated crumbling type and
`AnimationLength*(HasDamagedFrames+1) + 1 + Animation.Value` for an animated one;
the stock stage / `health < 2` arms are unchanged when the flag is off.

Retail leaves `HasCrumblingFrames` unset, so stock removal is bit-for-bit
unchanged.

## Where VERA20K owns it

- `rules/ruleset.rs` - `terrain_condition_yellow`, the one owner of
  `[AudioVisual] ConditionYellow.Terrain` over `ConditionYellow`: the damage
  latch and the draw both ask it.
- `rules/terrain_object_type.rs` - the `DestroyAnim` list
  (`ValueableVector<AnimTypeClass*>`, one registered entry picked per removal,
  `Ext/Anim/Body.cpp:407-412`) and the per-pass `TerrainTypeExt` read state;
  the effect asset catalog binds every listed anim.
- `sim/terrain_object.rs` - `TerrainObjectState::is_crumbling` and the store's
  `set_crumbling`; `TerrainObjectStore::start_crumble_animation` seeds or
  rewinds the Stage at rate 2.
- `sim/world/lifecycle.rs` - `terrain_start_crumble` (IsCrumbling, Stage
  restart, Logic join and `CrumblingSound`), `terrain_crumble_finish`, and
  `terrain_latch_damaged_frame` (the `0x71B98B` latch: IsCrumbling and the
  Logic join only).
- `sim/terrain_fire.rs` - `terrain_finish_fire_death` clears IsOnFire and runs
  the `0x71C6EE` tail: a zero-Health `HasCrumblingFrames` tree not already
  crumbling calls `terrain_start_crumble`.
- `sim/combat/world_receiver.rs` - `commit_terrain` stores the pre-receive
  Health (`0x71B965`) and runs the `0x71B98B` latch on a non-lethal `Damaged`
  write; the lethal transaction reads `HasCrumblingFrames=` after DetachAll and
  the nested C4, and a crumble skips `PlayDestroyEffects` and the same-call
  UnInit.
- `sim/terrain_spawn.rs` - `tick_terrain_crumble` (the `Frames/2` fold, the
  damaged-redraw clear, destroy effects and removal) and
  `terrain_play_destroy_effects` (`DestroySound` plus one random `DestroyAnim`).
- `app/presentation/instances/overlays.rs` - `terrain_draw_frame` selects
  `1 + Animation.Value` for a static crumbling type and
  `AnimationLength*(HasDamagedFrames+1) + 1 + Animation.Value` for an animated
  one; the simulation owns the Stage, not the renderer.

## Tests

`sim::terrain_spawn::tests::phobos_crumbling_tree_keeps_alive_then_destroys_and_removes`
drives the AI: survival with Logic and `CrumblingSound`, then removal on the
visit whose Stage step reaches `Frames/2 - 1` with `DestroySound` and one
random `DestroyAnim`.
`a_lethal_area_hit_routes_crumbling_and_plain_trees_through_the_receiver`
drives the production area receiver: a crumbling tree stays live and leaves
the transaction's finalizing set, a plain tree finalizes in the call, and
(under Ares) an on-fire tree skips Start_To_Crumble.
`app::presentation::instances::terrain_render_tests::phobos_terrain_draw_frame_follows_type_keys_and_crumbling`
pins both draw folds and the unchanged stock selector, and that the draw
threshold is the same `RuleSet` owner the simulation reads.
`sim::terrain_fire::tests::a_burning_tree_with_crumbling_frames_crumbles_instead_of_vanishing`
drives the `0x71C6EE` tail: a burning tree at zero Health whose last fire anim
leaves starts the crumble (Logic, `CrumblingSound`, Stage value 0, rate 2 by the
one-frame-then-step AI visit) and stays represented, while a sibling without the
key never starts one.
`sim::terrain_spawn::tests::a_nonlethal_hit_across_condition_yellow_latches_crumbling_for_one_frame`
drives `0x71B965`/`0x71B98B` through the production area receiver: the downward
crossing latches IsCrumbling and Logic, the next AI visit clears both, and an
upward crossing, an already-damaged ratio, an animated type and a type without
damaged frames refuse.
`sim::terrain_spawn::tests::condition_yellow_terrain_overrides_the_base_threshold_in_sim_and_draw`
crosses `ConditionYellow.Terrain=0.7` at 0.65, which the base 0.5 would not.

The receiver-path test, the on-fire skip and the step-before-fold order were
each mutated red and restored; the new latch, fire-out and override rows are
written to fail without their arms.

## Residuals

- The stock vanish of `Finish_Fire_Death`'s zero-Health tail (`0x71C72B`) is
  not ported: it needs the Uninit context that callback does not own, and
  VERA's `0x71BADC` on-fire lethal arm already retires a burning tree before its
  fire anims end, so the remaining path is the fire-out crumble above.
- `0x71B98B` also runs inside the lethal receive, before `0x71BADC`/`0x71BB2C`,
  so native keeps a `HasDamagedFrames` + `HasCrumblingFrames` tree that is
  killed while on fire (the latch sets IsCrumbling, `0x71BB2C` then preserves
  it). VERA runs the latch only on the non-lethal `Damaged` write and keeps its
  lethal gate (`HasCrumblingFrames` && not on fire), so that tree is destroyed
  instead. Trigger: only that key combination on a burning lethal hit.
- `Mark(Change)` / `Disappear(true)` and the dirty-rect registration in
  `TerrainTypeExt::Remove` are presentation bookkeeping; VERA redraws every
  frame and retires through the common non-entity owner, so no separate dirty
  area is registered.
- The anim asset catalog binds every authored `DestroyAnim` token; a token
  that resolves to no AnimType is dropped at the random pick (the native
  `ValueableVector` parser drops it earlier), so the draw range and RNG
  stream match but an unresolved name can produce a load warning.
- An animated crumbling type with no `AnimationLength` derives
  `Frames/(2*(HasDamagedFrames+1))`, which makes `currentStage + 1 == Frames/2`
  unreachable at value 0; native never removes it either. Author
  `AnimationLength` to terminate an animated crumble.
