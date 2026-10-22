# Ares builtin warhead AttachEffect

The `ares` feature enables the first neutral-stat, ordinary ground-victim
AttachEffect chain. Without the feature there is no attachment vector, update,
damage pre-effect or animation root in the game. Ares snapshots include the new
state in the automatic snapshot layout digest; old layouts are rejected.

## Owners and order

The retained rules registry owns `AresWarheadRules`: EMP and AttachEffect share
the existing House admission function, rather than maintaining separate copies.
WarheadExt constructor `10051890` supplies the defaults. Reader `10058A50` reads
the 15 AttachEffect keys in native order through the existing Ares scalar readers
and registered AnimType lookup. Missing and invalid keys retain the live value.
`RuleSet` projects that configuration; its animation enters the existing asset
closure and physical frame-bound binding before simulation construction.

`GameEntity::attach_effects` owns the ordered instances and hidden-animation
flag corresponding to TechnoExt+20. Each instance retains its warhead identity,
animation, remaining duration and first source House. It is saved and hashed.
There is no additional timer, stat aggregate or pointer registry elsewhere.

Installed Bullet hook `46920B -> 100536E0` captures the live firer's House before
EMP callbacks, then calls AttachEffect `10052700` after EMP and before ordinary
damage, including zero-damage hits. Both effects call the one ground collector
`10076660` port. House, health, live/limbo, absolute verse and the native Iron
Curtain timer decide admission; EMP immunity does not reject AttachEffect.

Attach `10058520` resets noncumulative durations without replacing their first
source House. Common head `1004B434 -> 10058E00` handles animation visibility
before positive-duration decrement, ahead of delayed EMP recovery `1004B528`.
Limbo, transporter and the Chronosphere's +27C latch skip the entire visit.
Negative durations do not decrement. Zero expires the instance.

Animation creation `100586E0` uses the existing Anim constructor, flags `600`,
SetOwnerObject, loop byte255 and the retained source House. The Anim pointer
expiry receiver `10046F20 -> 10058770` clears only the held reference. A missing
pointer does not independently respawn; a hidden-to-visible transition does.
When replacing an animation, Create publishes the new reference before old
UnInit, then sets owner/iterations/House. Its early visibility gates retain the
old animation. Reapply does not clear it before calling Create.
The existing expiry index derives its references from the attachment owner.

Remove `6F6AC9 -> 1004B140 -> 100588E0` destroys held animations and sets the
hidden flag only for nonempty vectors. Default effects remain paused; those with
DiscardOnEntry are removed. Techno destructor `6F4500 -> 10046FA0 -> 10042C30`
destroys the remaining animations before the stock voice tail. Both reuse the
existing Anim UnInit/expiry/sound/deferred-deletion owner.

## Evidence and limits

[Native controls](../tools/rules_oracle/ares_attach_effect.json) and their
[reproducible harness](../tools/rules_oracle/ares_attach_effect.py) execute six
histories: repeated hits with another source, external animation pointer expiry,
Remove/unlimbo/destructor, DiscardOnEntry, a new hit before unhide, and animation
reset across a cloak transition. The last two were executed as one incremental
batch without replaying the original four. The
[metadata](../tools/rules_oracle/ares_attach_effect.meta.json) pins Ares identity,
executed entry points and explicit world-service boundaries. Animation internal
RNG, rendering, constructor and Detach cascades are not native-emulated here;
Rust integration exercises the production owners separately. No whole-mod or
pixel-parity claim follows from these controls.

The current chain admits only identity firepower/armor/ROF/speed multipliers,
with Cloakable, ForceDecloak and TemporalHidesAnim disabled. Nonneutral aggregate
consumers, self-effects/Delay/InitialDelay, Temporal hide behavior and airborne
half-distance collection remain later mechanisms, not silently visual-only
effects. Their active callers keep the wider compatibility goal open. The
shared collector retains its documented fractional-distance rounding residual.
No explicit RNG draw or timer is added beyond actual positive-duration visits;
the existing Anim constructor owns its own RNG, and synchronous UnInit owns
detach/expiry work. Manual game acceptance remains pending.
