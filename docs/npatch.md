# NPatch compatibility

NPatch is a long-standing binary patch for Yuri's Revenge that predates the DLL-based Ares and
Phobos extensions. VERA20K implements the supported behavior in Rust. NPatch is in every
build and switches on from a game's data, like Ares: adding `NPatch=yes` to the Rules layers,
the selected mode or the map turns the extension on; a single feature key turns on only that
feature. The global bug-fix chains need `NPatch=yes`.

```ini
[General]
NPatch=yes
```

`[General] NPatch=` is the explicit master switch. `yes` turns every NPatch path on even
when no NPatch-only key is present; `no` keeps them off even when one is; absent leaves the
answer to the keys below. The key is a plain read, so its own presence never turns NPatch on.
The log names either this answer or the deciding NPatch-only key.

"NPatch" here names the family: CnCVK's original NPatch and its SE/AE editions, LH_Mouse's
2011 NPExt extension on top of them, and the current NPSE 2.0 continuation. NPExt is newer
than the original NPatch but older than NPSE 2.0 #359 (2026-07-07); `NPatch=yes` turns on the
implemented family, not a claim that every later NPSE build is ported.

Bug-fix chains are stricter: `RuleSet::npatch_fixes` is on only with `NPatch=yes`. An
individual NPatch key turns on its own feature paths (the canopy, AI queue or build-limit
behavior it names), not every NPExt fix. `NPatch=no` or an absent master key keeps the
fixes off while their automatic feature detection still runs.

Without the global key, `rules::npatch_switch` decides per game from any key only an NPatch
reader found (`VehicleParachute`, `DisableParallelAIQueues` or its ext173 name
`DisableAIParallelProduction`, `EnableAIBuildLimitation`, a TechnoType's `AIBuildLimit=`,
a BuildingType's `AIPlanningCount=`). Retail data holds none of them, so a retail game plays
as gamemd. There is no build feature: a plain `cargo run` carries the code, and a mod that
ships `NPatch=yes` or NPatch keys plays NPatch.

The older automatic example remains valid:

```ini
[General]
VehicleParachute=VEHCHUTE
DisableParallelAIQueues=no
```

The keys the processing passes read travel with the processed rules
(`ProcessedRulesLayers::npatch_inputs`), so a save, a replay and every peer decide the same
answer from the same files. A save written before the switch existed converts with an empty
read list and keeps NPatch off, as those matches played ([save compatibility](save-compatibility.md)
lists each layout).

`[General] VehicleParachute=` chooses the canopy for airdropped non-infantry, falling back to
`Parachute=` when unset. Infantry keeps `Parachute=`. The name uses the retained animation
reference and asset-loading owners, including empty-layer retention and `none` clearing.
This follows NPSE 2.0 #359's ReadGeneral (`0xC31005..0xC31017`) and ObjectClass::Paradrop
(`0x5F5ADD`, `0xC2B0E4`, `0xC2FA75`, `0xC2F939`) from the
[author-linked release drive](https://zhytat.ysepan.com/). Parachuted bombs and their `IsBomb=`
branch remain outside the current descent implementation.

Per-building `AIPlanningCount=` repeats eligible base-plan candidates before seed selection
(`0x5056C1 → 0xC2C331`). The reader stores one byte; the native signed loop comparison repeats
counts 1–127, while zero and high-bit counts retain one entry. The existing planner and RNG
owners handle the resulting candidate vector.

Per-type `AIBuildLimit=` limits AI production using the existing tracked and produced-count
owners (`Factory_AI`, `0xC335A0..0xC33726`). Positive limits include limbo factory objects;
non-positive limits compare the magnitude against both counters, and zero blocks the type.
Reaching a cap clears the standing choice, without advancing the base plan. A capped building
can therefore stall construction.

NPSE's naval vehicle choice uses the existing mobile-choice owner. House Update asks the
land and naval channels in order (`0xC33AB0`), sharing the harvester decision and team
chooser. Suggest_New_Object reads the producer's channel (`0xC2F130`); exit, abandonment
and build-limit rejection clear the corresponding choice (`0xC33940`, `0xC33B50`,
`0xC33570`). These paths run when the game's data turns NPatch on (`rules::npatch_switch`).

The legacy `[General] EnableAIBuildLimitation=yes` switch applies the same comparison to
`BuildLimit=` (NPExt ext173 reader `0xC2D132`, gate `0xC2CF67`). It defaults to no. This is
compatibility with the [older NPExt release](https://ppmforums.com/topic-29537/npatch-extended-finally-released/);
NPSE #359 replaced the global switch with `AIBuildLimit=`. If a VERA ruleset explicitly sets
both, both constraints apply before production starts. Clearing a choice does not skip an
unsatisfied building-plan node or guarantee the mobile chooser selects an uncapped type.

`[General] DisableParallelAIQueues=` defaults to no; its key turns NPatch on.
NPatch Extended (ext173) spells the same global switch
`DisableAIParallelProduction=` ("whether we should prevent AI factories from
'cloning' units"). ext173's `ReadGeneral` hook reaches the reader at
`0xC2BDDA..0xC2BDE7`, which writes `Rules+0x640`, immediately before
`EnableAIBuildLimitation` at `Rules+0x641`; `0xC2D482` gates the same five
counters. VERA reads the NPSE name first and falls through to the ext173 name
when it is absent or unparsable. The 2022-era revision that
[PR #2877](https://github.com/YuriPlanet/vera20k/pull/2877) discusses sets the
name per type; only `[General]` is consulted here, so a type-section key stays
unread instead of driving this global switch. The NPSE native reader preserves
the current byte (`0xC30303..0xC3031A`); the Rules constructor zeroes it
(`0x66569B`). With yes, Factory_AI reserves one of five House
production counters before compatibility, allocation and construction (`0xC2F07C`):
aircraft, land vehicles, naval vehicles, infantry and buildings. Only vehicles split
by `Naval=`. The existing House AI production owner saves and hashes those counters.

Mobile exits release at their native class admission prefix, before later placement
can fail; buildings release after successful Unlimbo (`0xC33940`, `0xC2F184`,
`0xC2F1A3`). Abandonment releases the held object's House before refund (`0xC2EFB4`).
Failed starts and PointerExpired retain their reservation. Releases run independently
of the switch and clamp signed negative counts to zero, matching the original hooks.

## Bug-fix chains (NPExt changelog)

NPatch also carries the NPExt (NPatch Extended) bug-fix corrections this
implementation adopted: each chain fixes a defect the original game carries,
so with NPatch data on they are deliberate, documented deviations from native
behavior, not parity fixes. The inventory and its policy live in
[#3024](https://github.com/YuriPlanet/vera20k/issues/3024). Retail data turns
none of them on. Two unrelated corrections run in every build: a war factory
captured while letting a unit out still lets it out
([#3593](https://github.com/YuriPlanet/vera20k/issues/3593)), and a
`Crashable=` transport such as the Night Hawk destroyed on the ground lets
its passengers out instead of killing them
([#2958](https://github.com/YuriPlanet/vera20k/issues/2958)).
Each implemented fix records its issue, the NPExt changelog item it adopts,
and the native site the defect lives at, next to the Rust that changes it.
The sections follow the behavior chains of #3024.

### Rules and repeated INI reads

- #2928 (NPExt changelog item 5): a `[General]` pass that omits
  `PrismSupportModifier=` keeps the retained percentage exactly instead of
  multiplying it by another 100 (`ReadGeneral67114F`, scale at
  `0x0067116E`). An Ares build keeps Ares's own rescale
  (`RulesClass_Addition_General_PrismSupportModifier`, `0x1005D040`), which
  chops most values by one per keyless pass.

### Superweapon and AI strategy

- #2933 (NPExt changelog item 86): the secret-lab startup assignment draws
  uniformly over the choices that remain, removes the granted one, and
  refills the whole list for further labs. Native
  `AssignSecretLabProduction68C050` (`68C112..68C1B5`) uses the drawn
  position against the original arrays — shrinking range, duplicate grants —
  and returns early when labs outnumber types, leaving every lab inert.
  NPExt's exact rewrite was not disassembled; this implements the corrected
  contract the report states. The draw ranges match native while labs do not
  outnumber types; each refilled lab adds one draw.
- #2935 (NPExt changelog item 9): the AI super-weapon rally pick
  (`AI_FindBestRallyTarget50CBF0`, choice `0x0050CF38..0x0050D16B`) skips a
  fully cloaked entry instead of letting its `RandomRanged(0, best + 10)`
  draw beat every real candidate.
- #2936 (NPExt changelog item 11): a pre-selected ChronoWarp click is
  refused, with no write, while its paired Sphere is held (IsSuspended, which
  `Update_Owned_Supers @ 0x0050AF10` sets on the IsPowered Sphere for low
  power or no online provider). Native `Fire_SW @ 0x004FAE50` launched the
  warp through ClickFire's PostClick exemption (`0x006CB933..0x006CB95F`),
  then cleared the Sphere's readiness and restarted its countdown while held
  (`0x004FAEA6..0x004FAEC1`).
- #2938 (NPExt changelog item 80): the Psychic Dominator strike's area
  damage books its kills to the Dominator's owner. Native passes the
  lightning-storm house slot (`0x00A9FACC`) — empty without an active
  storm — so building kills reached no score and no destroyed-buildings
  count.

### Combat target, order, fire and damage eligibility

- #2937 (NPExt changelog item 41): the passive acquisition scan
  (`TechnoClass::Greatest_Threat @ 0x006F8DF0`'s candidate gates) refuses an
  invulnerable candidate. Native gates G1..G13 have no invulnerability arm —
  the Iron Curtain is enforced receiver-side only — so auto-acquisition
  dumped fire into curtained units. The capture and occupy scans (masks
  `0x200`, `0x8000`, `0x10000`) keep native.
- #2942 (NPExt changelog item 7): a temporal erase pays one alliance-gated
  veterancy award. Native pays the Update's `VeterancyStruct::Add`
  (`0x0071A917..0x0071A978`, no alliance test) and Record_The_Kill's award
  (`0x00702D40`, zeroed for allies) together — friendly victims paid and
  enemy victims paid twice.
- #2945 (NPExt changelog item 54): a NULL `Assign_Target` (`0x006FCF5B`)
  mid-burst keeps the burst slot instead of resetting it, so the
  interrupted burst resumes and the full ROF still separates bursts — the
  cancellation no longer skips the reload.
- #2955 (NPExt changelog item 10): the guard hotkey's miner fallback
  (`Guard730D60`'s Unit slave-manager/Harvester arm) demands the issuing
  house's ownership. Native pairs that ownership-free arm with the MegaMission
  executor's owner-free retask (Queue `0x004C73B9`), so a foreign harvester
  caught in the selection could be stalled on area guard.
- #2961 (NPExt changelog item 72): the fire-error cell arm
  (`0x006FC7EB..0x006FC868`) reads only LandTargeting, so a `NavalTargeting=6`
  firer (dogs, Terror and Chaos Drones) could force-attack a water or beach
  cell. Under NPatch that firer type (`+0x600`) refuses the cell; every other
  firer is unchanged.
- #2968 (NPExt changelog item 65): HP exactly on `strength >> 1` counts as
  the yellow side, so the hit landing on it is the `DamageState::Yellow`
  crossing (yellow Tags 39/42, the damaged-building cue) and the next hit
  from it is no second crossing. Native `Object5F5390` answers Yellow only
  for `half <= previous && post < half`, so the exact boundary stayed
  Damaged and the `BuildingClass::ReceiveDamage @ 0x00442230` result table
  (`0x00442476`) never reached the global `BuildingDamageSound` request
  (`0x00442706`).
- #2981 (NPExt changelog item 55): a kill's veterancy award divides by the
  recipient's OWN house cost factors. Native `Record_The_Kill`
  (`0x00702ED1..0x00702FD4`) prices every recipient arm through the victim's
  house, so industrial plants or country discounts on the victim's house
  skewed the killer's promotion speed.

### Weapon, wave and projectile source, path and range

- #2946 (NPExt changelog item 59): the GetROF early return for sonic
  weapons and live spark/fire/railgun systems (`0x006FCFE0..0x006FD036`)
  also takes the ROF-ability reduction (`0x006FD0E2..0x006FD14C`), so
  veteran and elite dolphins, squids and effect weapons keep their reload
  bonus.
- #2948 (NPExt changelog item 84): the InRange arming ceiling over
  structural bridges (`0x006F74D7..0x006F7504`) consults the projectile's
  `SubjectToCliffs=` — a projectile that ignores cliffs may shell a deck
  three or more levels up; cliff-respecting arcing shots stay refused.
- #2951 (NPExt changelog item 99): a missile whose aircraft target dies, or
  expires while it has a home pad and is not in the air, loses the target
  instead of taking the ground cell. Native's PointerExpired repair
  (`LoseTarget468430` / `PointerExpired468517` cell substitution) treats the
  low aircraft as ordinary, so anti-air fire that downs a landing aircraft
  flew into the airbase. The pad (Aircraft+0x6CC) stays set for the
  aircraft's life, so parked and departing aircraft take the arm too.
- #2986 (NPExt changelog item 85): the magnetic wave decays once its held
  target leaves its owner's weapon 0 `Range=`. Native's tracking decay
  (`WaveClass::Update_Wave @ 0x00762AF0`) skips type 3, so a magnetron beam
  followed its target at any distance. The other types' 2172-lepton bound
  would end beams inside the retail MagneticBeam's 12-cell range.

### Building placement, construction, economy and sensors

- #2930 (NPExt changelog item 53): the selected-building ring radius
  (`GetSensorRange @ 0x004566B0`) no longer consults `SensorArray=` — a
  `SensorArray=yes` building falls through to its normal weapon-range radius
  instead of drawing `CloakRadiusInCells=`. The cloak-generator arm keeps
  native behavior.
- #2953 (NPExt changelog item 17): a naval factory's rally accepts only a
  resolved water cell. Native `SetRallyPoint` (`0x00443860`) resolves the
  click in the Amphibious zone, so land tiles in that zone were accepted and
  beached the ships. A refused click still plays the rally announcement, and
  an opening shipyard whose offset cell is land gets no rally.
- #2965 (NPExt changelog item 6): a refused `UndeploysInto=` conversion
  (`BuildingClass::Mission_Selling @ 0x00449C30`, the Unlimbo refund tail
  `0x0044A16B..0x0044A176`) deletes the never-placed limbo Unit with the
  building. Native retains it, so its `Add_Tracking @ 0x004FF700` count kept
  feeding ShortGame's BaseUnit sum and the defeat gate
  (`HouseClass::Update` `0x004F8E92..0x004F8F77`) never declared the emptied
  house defeated. This is part of #2965: undeploying a yard already being
  sold is a separate VERA-versus-native gap.
- #2966 (NPExt changelog item 34): a `CanBeOccupied=` building refuses every
  entrant at or below red health, owner included
  (`BuildingClass::CanBeOccupiedBy @ 0x00457CE0`, `IsRedHP` at `0x00457D8F`), so
  infantry released from a battle bunker their own house let drop to red could
  never re-garrison it. Under NPatch the building's own house is exempt;
  other houses keep the refusal.
- #2971 (NPExt changelog item 95): a tech building (`NeedsEngineer=`) owned
  by a MultiplayPassive house skips Grand_Opening's Idle18 + Active3..6 fill,
  and the capture's `Grand_Opening` re-run (`BuildingClass::ChangeOwner`,
  `0x00448CEF`) fills those empty slots once a non-passive house owns it.
  Native fills them at first opening (`0x0044632E..0x00446374`) with no
  ownership arm, so an un-owned oil derrick pumped from scenario placement.
  Placement for a real house keeps the native fill; a building handed back
  to a passive house keeps the slots it has.
- #2978 (NPExt changelog item 18): the prism supporter walk
  (`0x0044B357..0x0044B4BD`) refuses a tower in body state 0
  (Building+534), so unfinished towers no longer contribute beams during
  base build-up. A tower being sold is in the same state and is refused too.

### Movement, crash, ejection, garrison and lifecycle

- #2934 (NPExt changelog item 4): the `MakeInfantry=` anim end
  (`AnimClass::AI @ 0x00423AC0`, refused-Unlimbo branch `0x00424B23`) frees
  the failed creation instead of leaving it in limbo. Natively a Genetic
  Mutator over an occupied cell allocated a fresh brute on every completed
  frame and never destroyed the refused one — unbounded object growth while
  the cell stayed blocked. The anim still steps its stage back and retries.
- #2956 (NPExt changelog item 19, parts 1–3): a unit the Magnetron lifts
  is not self-propelled — held locomotion plays no move sounds, flies on
  its ordinary `Speed=` instead of the type's higher `JumpjetSpeed=`, and a
  drop plays neither the crash voice nor the crashing sound. Native ran the
  victim's own foot move-sound tail over the held jumpjet route
  (`FootClass::AI @ 0x004DA530`), whose skip also drops that tail's Main RNG
  draw (`0x004DAACB`), copied the Jumpjet block at `0x54AD5C`, and raised
  the drop edge's crash latch (`0x004DEBD3..0x004DEC5F`). The changelog's
  held-aircraft death claim is wrong: native takes the Aircraft death
  explosion (`0x0041661F`), then `Crash` (`0x00416694`) and the Jumpjet
  drop (`0x0054AF33..0x0054AF60`), and VERA already falls that way.
- #2967 (NPExt changelog item 52): a manual garrison release with no ring
  exit is a no-op under NPatch instead of `KillOccupants(NULL)`
  (`SellBuilding @ 0x004580A9`, no-exit arm `0x00458180..0x00458190`), so a
  walled-in garrison no longer vanishes without a kill record.
- #2983 (NPExt changelog item 69): the water `DoAction` remap
  (`InfantryClass::Do_Action @ 0x0051D6F0`) maps Down to Tread like
  Walk/Stop, so Tanya or a SEAL ordered to lie down on water uses the
  tread pose instead of the native swim/standing inconsistency.

### Capture, mind control, lift and retaliation

- #2939 (NPExt changelog item 81): a C4 demolition, one fatal forced packet
  at the building's current health (`BuildingClass::Update`), still raises
  the owner's NotifyUnderAttack alarm before the death postlude. Native's
  non-fatal hit-response block never ran for that packet.
- #2977 (NPExt changelog item 42): an AI victim that already holds an attack
  target no longer overrides its mission for every accepted hit
  (`ShouldRetaliate`'s existing-target arm, `0x0070884A`). Native applied
  that arm only to a human house, so focus fire re-aimed any AI defender at
  the newest attacker. The battle-fortress/prism-tower charge loop the report
  describes is that same retarget; it has no separate runtime repro.
- #2943 (NPExt changelog item 13): a `BalloonHover=no` helicopter's crash
  takes the DeathWeapon arm, not only the `BalloonHover=yes` branch
  (`FootClass` crash end); a crashing helicopter now fires its death weapon
  whether it balloons or falls.
- #2959 (NPExt changelog item 58): when a squid's owner is defeated and the
  eater dies, the host's `ParalysisTimer` is cleared with the dropped
  `Parasite.Victim` link. Native only cleared the link
  (`0x004D998C..0x004D99A4`), leaving the ship paralyzed forever.
- #2962 (NPExt changelog item 74, parts 1 and 2): Stop on a lifting Magnetron
  releases the lift (`0x004C75F8` tail), and a wave that decays — on a target
  switch or at the end of its tracking — releases the victim it still holds.
  Native's IDLE event cleared only radio, destination and target, so the beam
  ended while the victim kept flying.
- #2973 (NPExt changelog item 30): capturing a building clears every other
  entity's attack target that still names it (`ChangeOwner` mission tail,
  `0x00448CEF`), and a C4 plant is refused when the target is already
  allied to the planter, so a Tanya or Boris cannot demolish the capture.
- #2974 (NPExt changelog item 62): a garrisoned psychic's control link
  follows the outermost transport hull instead of its stale stored
  position; the origin uses the `TechnoClass+0x11C` chain.
- #2975 (NPExt changelog item 63): selecting a battle fortress or transport
  exposes the control links of the psychics riding inside, through
  `CaptureManagerClass::DrawLinks`.
- #2976 (NPExt changelog item 64): a control ring whose owner is cloaked
  is not drawn until the owner decloaks; the anim and its state stay
  untouched.

### UI, presentation, audio and scenario lifetime

- #2931 (NPExt changelog item 79): scenario transitions clear the screen
  shake. Native's offset is the global Map's `+4`/`+8` (`0x0087F7E8`), which
  only decays while battle input runs (`0x004F4BB0`), so quitting mid-shake
  started the next game with leftover camera movement.
- #2932 (NPExt changelog items 82-83): a country's initial-veteran lists
  apply to every trainable category in the side bar, and a naval type never
  takes the spy-granted war-factory veteran flag (only its country's initial
  veterans).
- #2940 (NPExt changelog item 89): a bare Super weapon selection is
  cancelled by a control-group key or by the repair/sell toggles, mirroring
  the modes those Display setters already clear (`0x004AC660`, `0x004AC8C0`,
  `0x00731205`).
- #2941 (NPExt changelog item 95): a key release clears the held-key state
  even while the pause gate refuses the edge, so a held scroll key cannot
  keep the camera moving after a pause or focus loss.
- #2964 (NPExt changelog item 15): a `Spawned=yes` aircraft such as a
  carrier's Hornet is not selectable under NPatch. Native `Object5F6C30`
  (+138) read only `Selectable=`; the type's `Spawned=` flag (Techno+0x6B8)
  kept no selection arm, so players could box-select and order carrier-born
  jets.
- #2970 (NPExt changelog item 73): a building entering the Selling mission
  releases its gattling `Report=` loop immediately, instead of waiting for
  stage 2's limbo release (`0x006F6C6B`), so a sold Gatling cannon is silent.
- #2979 (NPExt changelog item 28): the action determination's warp rung also
  catches a still-warping-in actor over an entity target. Native limits the
  warp-in arm to Cell targets, so a pre-emergence chrono legionnaire fell
  through to the attack rung's pure weapon-range alternative and showed a
  force-attack cursor over friendlies. The rung answers every object target
  while warping in, enemies included.
- #2980 (NPExt changelog item 45): the T type-select seed set no longer grows
  the `DeploysInto`/`UndeploysInto` counterpart type
  (`TypeSelect__BuildSeedTypeIDs @ 0x00732D10`, Type+0x404/+0x408), so T with
  an MCV selected gathers MCVs only instead of also taking the Construction
  Yard, and vice versa. An Ares build follows `TypeSelectUseDeploy=`.
- #2984 (NPExt changelog item 70): the mind-control removal sound plays
  only for the two involved parties — the (former) controller and the
  owner the unit returns to. Native emitted a plain positional VocAt with
  no house filter, so every player heard it.

## Original project and authors

VERA20K implements the documented behavior in Rust. It neither loads nor patches
`gamemd.exe`, and it does not redistribute NPatch binaries.

NPatch (NewPatch) is a RockPatch-based YR platform created by **CnCVK (VK)**,
a former RockPatch developer. The project has several editions:

- **NPatch 1.00** - the original release, published by CnCVK.
- **NPatch Extended (NPExt)** - released by **LH_Mouse** in January 2011 as an extension
  of NPatch SE/AE; its manual is NPExt #169 (2011), and it thanks pd and CnCVK. It is newer
  than the original NPatch but older than NPSE 2.0. VERA's legacy
  `EnableAIBuildLimitation` path cites the NPExt `ext173` reader
  (`0xC2D132`, gate `0xC2CF67`).
- **NPatch Special Edition (NPSE)** - installer #0020 on CnCVK's NPatch
  download page. The **NPSE 2.0 #359** manual and changelog are authored by
  **lydjjlyd**; its latest entry is dated 2026-07-07.
- **NPatch Autumn Edition (NPAE)** - installer #0011 on the same page.

The NPSE 2.0 manual credits its testers **我爱二次元**, **蓝天白云fd**,
**好人先昇**, **行少** and **凌敏SHOS**. That manual and its release are the
source for the `0xC3xxxx` addresses cited above.

简体中文：[扩展支持与原作者](zh-CN/credits.md)。

## Cross-extension compatibility

Ares, Phobos and NPatch are orthogonal and mixable. Ares and NPatch are in every build and a
game's data switches them on ([Ares](ares.md), `rules::npatch_switch`); Phobos is still a build
feature:

```sh
cargo run --release --bin vera20k --features phobos
```

Every implemented feature must keep working when Ares plays or the other capabilities are
compiled in. A chain
may depend on another **capability's own** state (for example a Phobos key that only a Phobos type
reads), but it must not assume the other features are absent, and it must not re-own state another
capability already owns. When two extensions touch the same native site, one of them wins by native
precedence and the other keeps its own owner — the Phobos radiation activation replacing the retail
one, with the retail substitution kept for builds that lack the capability, is the pattern to
follow.

The build capability is recorded per combination: a save from one combination cannot load in
another, so a mixed build never reads a save written by a single-extension build. NPatch left
that matrix when it became a per-game switch: every build carries it, and a game with NPatch
keys differs from a stock game through its rules digest, not through the build stamp. A save
written by a build that still had the `npatch` feature names that capability and is refused by
the current build, which names the mismatch.
