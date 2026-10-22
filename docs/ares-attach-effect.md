# Ares builtin warhead AttachEffect

The `ares` feature enables ordinary ground-victim AttachEffect lifetime and
its ordered Foot speed multiplier. Without the feature there is no attachment vector, update,
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
There is no additional timer, effect list or pointer registry elsewhere.

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

## Attached speed

`GameEntity::recompute_ares_attached_speed` owns the speed arm of native
`100460A0`. It reads that same ordered instance vector. `FootSpeedState` keeps
the permanent crate baseline (Ares TechnoExt+90) separate from the derived
Foot+580 output; only its methods write those private fields. The derived
cache is saved and hashed, and rules-bound snapshot restoration rejects a
cache inconsistent with its baseline and retained effects. The automatic
layout digest changes for Ares saves; the feature-off layout stays unchanged.

New attachments aggregate after animation construction (`1005869F`). Common
update compacts all expired instances before one recomputation (`10058F79`),
and Remove aggregates only if DiscardOnEntry removed instances (`1004B197`).
The speed-crate recipient (`1005FA00`) admits against the permanent baseline,
writes it, then recomputes. Refresh, pointer expiry, visibility changes and
destruction do not invent aggregate writes.

The feature also preserves installed Ares's speed-crate class gates:
Speed1 goes directly to481D52 after testing the permanent baseline, bypassing
stock Aircraft exclusion. Speed2 rejects WhatAmI3, which the original vtables
identify as AircraftType, not Aircraft2. Supported ground-display Foot actors
(Unit1, Aircraft2 and Infantry15) therefore share the Ares baseline admission.
The default build retains both original Aircraft exclusions. A small production
class/baseline table checks both profiles; no aircraft flight-speed or natural
crate-reachability equivalence follows from this admission check.

The existing `current_type_speed`/Foot getter consumes the effective multiplier
in its original House, truncation, FASTER and speed-fraction order. Drive/Ship,
Walk, Hover, movement readiness and target leading already share that getter;
none multiplies AttachEffect again. The fold retains native ordered x87 state
until the binary64 store, through the existing deterministic arithmetic owner.
Current Rust Fly, Jumpjet and Rocket movement does not read this Foot getter.
Their attached-speed consumer behavior remains unproven by this Drive chain;
that source observation is not evidence that native needs no such support.

[Four native speed histories](../tools/rules_oracle/ares_attach_speed.json)
cover refresh/expiry, a prior speed crate, crate pickup while slowed followed
by duplicate rejection and DiscardOnEntry, and coexistence with a neutral
attachment. Their [producer](../tools/rules_oracle/ares_attach_speed.py) reuses
the existing native probe and executes the real reader, aggregate, crate
hooks and common Foot getter. These histories were executed separately from
the six retained neutral histories; the asserted-output projection does not
rerun or derive numeric goldens from Rust. [Metadata](../tools/rules_oracle/ares_attach_speed.meta.json)
records the native identity and supplied world boundaries.

In those bounded controls, text `0.2` stores `3FC9999999999999`; raw speed10
becomes1, not2. A prior1.2 crate gives slowed speed2 and recovers11 on expiry.
Drive Process and the parent Bullet are Rust production integration checks,
not native movement-distance goldens. The parent Airburst detonation reaches
the shared pre-effect before its later child fan. The unimplemented Airburst
fan remains a separate projectile residual; this chain does not fabricate it.

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

The current chain admits only identity firepower/armor/ROF multipliers,
with Cloakable, ForceDecloak and TemporalHidesAnim disabled. Other nonneutral aggregate
consumers, self-effects/Delay/InitialDelay, Temporal hide behavior and airborne
half-distance collection remain later mechanisms, not silently visual-only
effects. Their active callers keep the wider compatibility goal open. The
shared collector retains its documented fractional-distance rounding residual.
No explicit RNG draw or timer is added beyond actual positive-duration visits;
the existing Anim constructor owns its own RNG, and synchronous UnInit owns
detach/expiry work. Manual game acceptance remains pending.
