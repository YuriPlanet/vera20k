# Ares ordinary ground and powered-tower EMP

This optional chain requires the compile-time `ares` feature. Default builds do
not read the extension keys or run its Bullet/AI producers. It covers ordinary
human-controlled Drive/Walk and ordinary AI Drive victims without Team, Spawn, Slave, Capture, Temporal,
war-factory contact, harvester or falling/death-threshold dependencies. It also
admits an independent ordinary powered defensive building without linked Prism
support or factory/provider/occupant/undeploy dependencies. Other victims are not
treated as generic offline objects. This is partial EMP support.

## Independent powered building

The Building path uses the same private `EmpState`, house and animation owners.
Installed10062B40 dirties House+1FC/+5778, disables Stuff through original452480,
and preserves Focus+218 across the shared Deactivate/ResetOrdersToGuard.
`BuildingOnline`+660 remains independent and unchanged. Only the first activation
performs those House/Stuff transitions and initially saves raw LastMission;
nonzero refreshes and capped hits retain them. The existing4555D0 operational adapter now reads the shared EMP
counter, so fire admission, target admission and powered-slot consumers see the
same value. Existing Building GetFireError and delayed-fire callers remain the
fire authorities. First-hit and deferred admission both use the existing
Mission effective selector5B3040: current when present, otherwise queued.
A freshly Unlimboed building with current=-1 and queued Construction18 retains
that queue; LastMission still records raw current, not the effective mission.
Once a deferred victim's effective mission permits deactivation, AI tail
10053E1A..3E saves raw current and calls only the shared Deactivate. It does not
repeat House invalidation/DisableStuff or the first-hit Building Focus
save/restore; the shared Stun therefore clears Focus on that deferred path.

Building43FE56 calls the common Techno AI. Its category branch6FABBC..C2 skips
only generic Stage advancement (the existing UpdateAnimation4509DE owner already
stepped it), then reaches flash6FAC31 and the tail. The Structure caller now
uses the same post owner as Foots, replacing its former manual flash; it must
neither advance Stage a second time nor flash twice. The installed Ares hook
replaces original6FAF0D's counter tail at the same reached point, before the
Building-specific Gattling/repair/factory tail. The original counter body alone
does not establish an active retail positive-counter producer or a stock bug.

Expiry100627F0 samples operational state before House invalidation. Full power
enables Stuff through452410 and reactivates, preserving Focus. Low-power expiry
retires the EMP sparkle but leaves Stuff disabled and Techno deactivated.
Restored power resumes the retained powered slot via the next Building
operational edge; the actual common Techno head1004B528..575 then reactivates.
That later path does not EnableStuff or preserve Focus: the shared Guard reset
clears it. Structure lacks the Foot flag tested at100628B2, so neither AI nor
human buildings queue the ground-unit Hunt recovery mission.

The pre-effect coordinate100536E0/10053727 is local to the extension effects;
original damage keeps its own impact. Its10053660 trampoline calls the existing
planar Distance_To5F6440 owner, ignoring Z and discounting the target building's
foundation. Strict distance<64 rereads target GetCoords+48, not Building+A4 or
TargetCoordOffset. The distance API now resolves source identity from the same
Abstract ID space, including a Bullet's retained Location; all six former
Techno source callers still read their original live receiver. No second distance
kernel or coordinate cache is introduced. Eight original caller-slice controls
pin63/64, high Z, foundation-discount boundaries, nonzero target offset, absent
target and an incoming impact distinct from Bullet Location. They stop before
the effect producers and establish no whole AttachEffect/weapon equivalence.
Bare component detonations without a retained Bullet and the shared dummy-cell
route retain their explicit impact; their remaining pointer/caller behavior is
outside this ordinary live-parent chain (border/temporary-target frequency is
unmeasured, with possible local effect-center differences).

The [native corpus](../tools/rules_oracle/ares_emp.json) adds four immunity,
three foundation/collector, four connected building histories and one bounded
pause/timer control. The histories execute original Stun70F850, the selected
Building navigation/target setters, AssignMission5B2FD0 and installed activation
hooks, including nonempty powered slot3. The producer's supplied House dirty
acknowledgement and mission input changes remain explicit fixture inputs. It
bounds no RNG/Detach calls on this no-manager path, not whole weapon parity.
`ares_emp_building_native_admission_refresh_and_slot_lifecycle` consumes those
controls, plus two original effective-getter/deferred histories. Those use
actual Construction18, contrast present Move2 with a queued Construction, and
compare the acknowledged House dirty flags, raw LastMission, cleared Focus,
mission timer, retained powered slot and untouched RNG. Their supplied queue
change remains an explicit input, not a full player-placement replay. The
existing production Unlimbo -> Enter_Idle_Mode44D6A0 queue owner establishes
where current=-1/queued Construction arises. The parent-Bullet/actual-AI test
additionally connects fire refusal,
low-power expiry, next-head recovery and a restored tower shot; synthetic image
bounds are not a retail drawing comparison. Server Rust checks and a release
production capture are separate validation steps, not native-corpus claims.

Airburst child fans and AttachEffect remain separate whole-weapon residuals.
Parent `Airburst=yes` still enters the existing detonation receiver once, and
EMP runs before ordinary damage and the later native Airburst tail; absence of
a child-fan consumer is not a reason to suppress the parent's EMP. No complete
Raccoon, factory, Radar/SpySat, Gap, superweapon, PowersUnit, LaserFence,
Tesla charging, occupied building, undeploy or linked Prism behavior is claimed.
Positive-output buildings (including explicit ImmuneToEMP=no providers and
upgrade-added output) are not covered by the negative-power tower comparisons.
Their EMP/output accounting and downstream House effects remain unproved;
frequency is unknown and explicit use may expose incorrect power availability.
No native immunity/output behavior is inferred from that coverage gap.

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

The existing global `AresGlobalControls` owner retains nullable
`[CombatDamage]EMPAIRecoverMission`. Constructor1002A360 initializes its value0
and present=false. The unconditional before-Type-data reader679A15 ->1002B356
->1002AB60/1002AECF uses ReadString2048 and the existing named Mission lookup
5B3910. Missing, empty and invalid values retain the previous selection through
RULESMD, LANGRULE, mode and scenario passes; named values set it. Numeric strings
and `none` are not mission aliases. This read is distinct from the earlier
Process-entry phase-gated GlobalControls read. New Rules construction/reset clears
the nullable setting; the same owner supplies the rules compatibility hash.

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
Effective Unload, Construction and Selling defer deactivation. A later ordinary
mission can deactivate without restoring selection or repeating first-hit
Building side effects. Expiry invokes the existing
Reactivate/Guard behavior when powering permits; it does not resume an old human
move or attack order. It ends the held Anim's remaining loops and releases the
reference rather than hard-deleting the animation. Control, fire-error and cloak
consumers read the same counter. Runtime checksum and current-layout snapshots
include its state; rules compatibility includes the effective extension fields.
This change does not promise old binary-save layout compatibility.

After that expiry prefix, installed disable100627F0's1006293A block uses the
existing ControlledByHuman50B730 decision. Ordinary AI Drive victims queue the
configured recovery mission, or Hunt15 when unset, through the existing
QueueMission5B35E0 owner with force=false. It does not Commence, restore the saved
old order, reset a timer or draw RNG. A same-current Guard queue keeps the
original queue/bypass guards. Campaign PlayerControl counts as human; in
non-campaign modes only the IsHuman flag does. Human victims skip this block.

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

The additional `ai_mission_readers` histories and `ai_expiry` controls execute the
installed nullable reader and the original QueueMission body. The
`ares_emp_ai_recovery_queue_matches_native_post_reactivate_controls` regression
compares only the explicit post-Reactivate service boundary, preserving its
timer sentinels and all three RNG streams. It does not certify Reactivate
internals from a substituted service. A separate
`ares_emp_ai_drive_hit_and_actual_visits_queue_recovery_after_sparkle` regression
connects delivery, two real object AI visits, shared Reactivate, sparkle retirement
and queued-only Area Guard recovery. These Rust checks require execution before
claiming a tested implementation; rendered gameplay comparison remains separate.
The AI MCV regression reuses the same shared MCV scene, exercises autonomous
Hunt visits during the positive counter, then expiry, queued Hunt and conversion.
No MCV category exclusion or second deployment guard is used. Its mission-handler
visits retain the existing Hunt cadence RNG draws; the new admission comparison
itself draws nothing and performs no timer or detach writes.

The same-named global-mission/hash regression lives inside the RuleSet owner.
Different physical layer histories legitimately have different `source_ini_hash`
values, even with the same final text. The test first checks that distinction,
then normalizes only that private test input before checking retained Ares state
changes the effective hash ON and leaves it equal OFF. Production hashing and
visibility remain unchanged.

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

Ledger: the EMP delivery/expiry blocks draw no RNG and call no DetachAll; the
reused Hunt handler retains its existing cadence draw and the Anim constructor
can draw for authored RandomRate/bounce settings. Counter writes
are delivery/cap and one decrement per actual AI visit; Deactivate/Reactivate keep
their existing mission/destination timer writes. Anim expiry writes loop_remaining=0.

Still open: AI Walk and Team/task resumption, other building mechanisms/aircraft and airborne half-distance
collection, positive threshold death, managers/temporal/capture/harvester interactions.
Their trigger is an EMP hit on those victims; the current producer omits the EMP
effect there, which can change combat and scripted behavior whenever such victims
are used. Native TypeImmune scanner indices18/19 have synthetic getter evidence,
but their production configuration meaning is not established; the shared 18-slot
weapon owner is not expanded. The native lazy default-immunity cache is represented
by an equivalent immutable Type/category decision on this chain, not persisted as
a second mutable rules authority. Original active-retail TS EMP producer
reachability is unproven and is not claimed fixed by this Ares extension.
