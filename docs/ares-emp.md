# Ares ordinary ground EMP

This optional chain requires the compile-time `ares` feature. Default builds do
not read the extension keys or run its Bullet/AI producers. It covers ordinary
human-controlled Drive/Walk victims without Team, Spawn, Slave, Capture, Temporal,
war-factory contact, harvester or falling/death-threshold dependencies. Other
victims are not treated as generic offline objects. This is partial EMP support.

## Input and state ownership

`RulesPassProcessor` retains each reached TechnoType/Warhead extension across
RULESMD, optional LANGRULE, selected mode and scenario reads, then projects it
into the corresponding immutable `ObjectType`/`WarheadType`. Missing sections
skip the pass. `ini_value` owns the shared Ares integer, binary64/percent,
nullable bool and int-or-bool threshold readers. Constructor defaults are
Duration=0, Cap=-1, Modifier=1, Threshold=-1, unset ImmuneToEMP and unset nullable
Type sparkle. Veteran and elite EMPIMMUNE flags retain independently on empty
reads and replace their own tier on nonempty comma-delimited reads.

`EMP.Sparkles` finds an existing AnimType without allocation. Unknown or empty
values retain the prior binding; explicit none clears it. The actual selection
is Warhead sparkle, then the nullable Type override, then retained
`[General]EMPulseSparkles`. An explicitly null Type override blocks that fallback.
The General pointer uses the existing RulesClass pointer reader/retainer.
The same retained Warhead, Type and General names enter
`effect_asset_catalog::anim_class_roots` only in Ares builds. The app's existing
tolerant SHP binder supplies their physical frame bounds before Anim construction;
its scheduler-bound set also supplies the renderer's effect-atlas roots. ART
metadata existing by name alone does not supply those bounds. This is a required
dependency of the new Ares producer, not a claim about an active retail EMP path.

`GameEntity::EmpState` privately owns Techno+504, the held Anim reference and
LastMission (constructor -1). Existing Deactivation owns the shared offline latch,
locomotor power and ResetOrdersToGuard; animation construction, ownership and
retirement remain in AnimClass. The expiry-listener index derives from the held
reference, and AnimPointerExpired clears only a matching reference. Limbo does
not clear or advance the counter. Destruction uses the existing object/attached
Anim lifecycle, without an invented EMP destructor action.

## Connected behavior

Installed Ares Bullet46920B ->100536E0/10052870 runs after radiation and before
exclusive special/ordinary damage, including Damage=0. Ground collection uses
existing CellClass occupancy and virtual GetCoords owners, the installed
10035AC0 cursor, full XYZ cutoff and signed native UniqueID set order/dedup.
It does not scan every world entity per impact.

Delivery10062320 applies source-directed alliance/nullable owner filtering,
active iron curtain, warp, type/rank immunity and the native abs(verses)<0.001
gate. Verses is an admission test, not a duration scale. Only positive durations
use the Type modifier; native signed-int conversion and cap control the retained
counter. A first active transition preserves selection, invokes the existing
Deactivate when the current mission permits it, and creates at most one attached
sparkle. Repeated capped hits do not repeat deactivation or constructor work.
The cap branch keeps signed wrapping addition inside the original min/max order:
positive overflow cannot spuriously clear an already longer effect, and a
nonpositive delta with cap0 clears it. When a refresh changes the active counter,
only an explicit Warhead sparkle can construct a replacement. Type/General
fallback belongs to first activation; an expired fallback stays absent on refresh.

The same delivery owns attached-object EMP trigger events. On first activation
it raises 63 (Under EMP By House), then 62 (Under EMP); a healing hit that clears
the counter first runs the existing disable owner, then raises 65 (Removed EMP
By House), followed by 64 (Removed EMP). Natural countdown expiry raises neither
removal event. Each raise resolves the current Tag, and the caller rereads Alive
before the generic event so a synchronous death or tag detach takes effect.
The existing Tag/Trigger owner evaluates these incidental, nonpersistent events:
the affected Techno and raised kind must match, the current counter must be
positive for 62/63 or nonpositive for 64/65, and 63/65 additionally compare the
source's raw House array index with the event parameter. They use neither the
country lookup nor Player@X conversion. The native common sparkle tail still
runs after a tag callback; it queries the live counter and held animation.
Original Ares3.0 instructions: delivery100624C5..10062563,
Tag trampoline10025FA0 -> gamemd6E53A0, predicate1004F7DC..1004F870,
flags1004F6A0 and persistence1004F610. The official release's Ares.dll.inj
installs the predicate at71E949 and delivery at46920B. These are independently
read control-flow findings, with production regressions for source filtering,
both event pairs, callback death/detachment and natural-expiry exclusion.

The actual Techno AI tail6FAF0D ->10053DF0 decrements once per visited object.
Unload, Construction and Selling defer deactivation. A later ordinary mission
can deactivate without restoring selection. Expiry invokes the existing
Reactivate/Guard behavior when powering permits; it does not resume an old human
move or attack order. It ends the held Anim's remaining loops and releases the
reference rather than hard-deleting the animation. Control, fire-error and cloak
consumers read the same counter. Runtime checksum and current-layout snapshots
include its state; rules compatibility includes the effective extension fields.
This change does not promise old binary-save layout compatibility.

The feature also extends the existing shared `techno_reactivate` owner with
Activate1004CCB0's admission. The selected ordinary live Drive/Walk path vetoes
while Techno70EFD0 reports positive EMP; PoweredUnit100454F0 additionally requires
a matching counted PowersUnit building without EMP in the existing House building
order. This gate protects power-center recovery callers as well as EMP expiry,
before orders, locomotor or sound change. The EMP caller no longer repeats a
weaker aggregate-count check. Default builds retain original70FBE0 behavior.
Unit746C90's independent DeathFrame override and custom PoweredBy/other extension
dependencies are outside this ordinary live-victim chain.

The existing Unit `can_deploy_slash_unload` owner applies the positive-EMP
virtual veto700E47..700E53 (live Unit746C90 ->70EFD0). This protects an already
commenced human Unload and autonomous AI deployment through the same Deploy7393CC
caller, before MCV conversion can discard the EMP victim. Default builds retain
their existing constructor-zero path; active-retail EMP producer reachability
is not inferred from this shared native consumer. The separate DeathFrame virtual
override remains outside the ordinary live-ground chain.

## Reproducible evidence and bounds

The installed DLL SHA256 is pinned by the producer and corpus. The exact
entries, untouched-code guards, transport substitutions and replay command are in
[`ares_emp.meta.json`](../tools/rules_oracle/ares_emp.meta.json). Input executable
identities and the replay receipt are retained privately; executable bytes are not
distributed. The existing
[`ares_emp.py`](../tools/rules_oracle/ares_emp.py) producer records native reader,
cap, delivery, collector, AI, sparkle and pointer-expiry executions in
[`ares_emp.json`](../tools/rules_oracle/ares_emp.json). Original Deactivate,
Reactivate and Anim constructor internals are declared downstream service
boundaries in that harness, not certified by its connected sample. The Rust
chain calls their existing owners instead of duplicating those implementations.
The `activation` controls additionally execute the installed Activate export and
the actual Unit/Building EMP virtual leaves, plus its PoweredUnit building scan.
They use explicit ordinary DeathFrame=-1 and declared downstream interfaces;
Syringe patch installation, custom PoweredBy and full activation internals are
not certified. `sparkle_refresh` executes pointer expiry before unchanged/changed
hits and distinguishes explicit, Type and General sparkle provenance. The Rust
power-center interleaving regression goes through `powered_center_edge` and the
shared Reactivate owner, rather than an EMP-only admission helper.
The existing [`mcv_deploy_oracle`](../tools/mcv_deploy_oracle.py) additionally
executes whole700D50 with the actual Unit EMP virtual and the7393C0 caller
admission boundary. Its original mission/turn/site controls stay unchanged.
`ares_emp_mcv_shared_admission_and_pending_unload_respect_emp` consumes the live
admission outputs and drives a commenced human Unload through real object AI
after an EMP hit. The native caller's accepted path stops before movement and
placement, so the corpus does not certify full conversion or its timing.
The generic `ares_emp_sparkle_roots_bind_physical_bounds_before_anim_construction`
regression uses production retained Rules/ART reads, the existing loose-asset
binder and real Anim constructor. It covers all three name sources and the
default-off absence without test-only frame-count injection; synthetic SHP headers
establish metadata binding, not decoded pixels or rendered parity. Its initial
reader checks consume `registered_animation_readers`: actual constructor427530
registers generic types before the Ares reader/Find resolves them without further
allocation. Mod-specific registered-input replay and runtime receipts remain
private; the public corpus contains only generic names. The runtime observation that led to this
dependency showed a successful EMP state cycle with no Anim instance, not a
renderer visibility defect.

Rust regressions named `ares_emp_` consume these native outputs, exercise the
production layered reader and occupancy/coordinate owners, and cover hit,
current-layout save/load, expiry and pointer clearing. The asset frame count in
the synthetic animation test is an explicit fixture boundary. An actual rendered
single-player hit/expiry comparison is separate validation.

The shared INI owner keeps stock atof's existing nearest projection and an
explicit Ares `%lf` chop mode. WinMain's pre-argument PC53/chop setup is pinned by
`tools/rules_oracle/read_double_percent.py`; the installed Ares scanner preserves
that mode. For finite decimal input the Ares mode compares the nearest binary64
magnitude to its exact decimal rational using the existing big-integer dependency,
adjusting one ULP toward zero only when needed. Overflow remains infinity and
signed zero retains its sign. This policy matches the native reader controls;
arbitrary long-decimal/legacy CRT intermediate double rounding remains unproven.
It does not implement a second lexical parser or change default-build readers.

Duration math uses the existing deterministic PC53 arithmetic/conversion owner.
Ares CRT100772B0 converts out-of-i32 values to INT_MIN, unlike the stock qword/low32
conversion. The native corpus includes software dispatch and normal SSE3 controls;
Unicorn's incorrect SSE3 overflow result is excluded and documented. Range compares
squared lengths to avoid platform sqrt differences; exact zero/half-cell and XYZ
boundary controls are pinned, while arbitrary FSQRT rounding boundaries remain a
small unproven precision residual.

Ledger: selected delivery/AI paths draw no RNG and call no DetachAll; the reused
Anim constructor can draw for authored RandomRate/bounce settings. Counter writes
are delivery/cap and one decrement per actual AI visit; Deactivate/Reactivate keep
their existing mission/destination timer writes. Anim expiry writes loop_remaining=0.

Still open: AI mission/task resumption, buildings/aircraft and airborne half-distance
collection, positive threshold death, managers/temporal/capture/harvester interactions.
Their trigger is an EMP hit on those victims; the current producer omits the EMP
effect there, which can change combat and scripted behavior whenever such victims
are used. Native TypeImmune scanner indices18/19 have synthetic getter evidence,
but their production configuration meaning is not established; the shared 18-slot
weapon owner is not expanded. The native lazy default-immunity cache is represented
by an equivalent immutable Type/category decision on this chain, not persisted as
a second mutable rules authority. Original active-retail TS EMP producer
reachability is unproven and is not claimed fixed by this Ares extension.
