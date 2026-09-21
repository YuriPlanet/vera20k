# Gameplay implementation guide: order, scope and goal prompts

Use this page to **choose the next work and define its goal**. It combines the
suggested implementation order with the catalogue's complete coverage and
boundaries. The [evidence companion](2026-09-11-gameplay-boundary-evidence.md)
remains supporting research to consult for the selected work.

## How to use this guide

1. Choose an entry from the [suggested order](#suggested-order), or start with a
   current match problem or your own priority. Check current code and behavior
   before treating an entry as unfinished.
2. Read its **Selection** and **Coverage and completion** together. Decide whether
   the goal is a full object/family, a named loop, or a bounded fix. Where a family
   spans entries, the links stay on this page; completing one selection does not
   close the broader family.
3. Use the entry's evidence links as starting leads. Compose the prompt with the
   [scope guidance](#start-with-the-requested-outcome) and
   [goal brief](#a-concise-goal-brief) below, stating the intended outcome and scope.
   An entry number or coverage identifier alone is not a goal prompt.

The game is already playable. This order favors everyday match interactions and
shared work; it is a recommendation, not a measured defect ranking, schedule or
completion checklist. Select the first meaningful demonstrated gap. Serious
current problems, required dependencies and user priorities can move any entry
forward. A broken Chrono return belongs with the resource goal; a LAN request
need not wait for every special weapon or campaign feature.

The 77 selections are not mandatory separate sessions or phases. Nearby entries
can be combined when their actual owners support shared work; a large authorized
goal can span several entries and PRs. The 83 coverage identifiers (R1, U4, etc.)
retain their scope, including when several selections refer to one family.
Reuse working behavior rather than refactoring merely to visit every entry.
Movement, presentation, AI and save are interleaved; any required integration
belongs to the selected goal regardless of where its other entry appears.

[AGENTS.md](../../AGENTS.md) governs evidence, architecture, scale, validation and
delivery. Rendering, audio, input, persistence, determinism and cleanup remain
obligations of every affected loop. This document grants no implementation,
publication or scheduling authority. The source observations remain dated to
the original inspection; this consolidation makes no new parity or status claims.

## Suggested order

Each link opens the selection and its scope **on this page**. Numbers preserve
the suggested order; coverage identifiers preserve the catalogue references.

| Order | Work to select | Coverage |
|---|---|---|
| 1 | [Ground orders and movement](#1-ground-orders-and-movement) | C1, U1, M1, M2 |
| 2 | [Standard resource economy](#2-standard-resource-economy) | R1 |
| 3 | [Purchases and delivery](#3-purchases-and-delivery) | B1 |
| 4 | [MCV deployment and recovery](#4-mcv-deployment-and-recovery) | B2 |
| 5 | [Power and provider-dependent behavior](#5-power-and-provider-dependent-behavior) | B5 |
| 6 | [Ordinary combat](#6-ordinary-combat) | U2 |
| 7 | [Vision and reveal publication](#7-vision-and-reveal-publication) | W4, S10 |
| 8 | [World depth and draw order](#8-world-depth-and-draw-order) | V4 |
| 9 | [World lighting and palettes](#9-world-lighting-and-palettes) | V1 |
| 10 | [Voxel appearance and shadows](#10-voxel-appearance-and-shadows) | V2, V3 |
| 11 | [GI and Guardian GI deployment](#11-gi-and-guardian-gi-deployment) | U3 |
| 12 | [AI base decisions and recovery](#12-ai-base-decisions-and-recovery) | F4 |
| 13 | [AI Team execution](#13-ai-team-execution) | F4 |
| 14 | [Save and resume](#14-save-and-resume) | F8 |
| 15 | [Building repair](#15-building-repair) | B3 |
| 16 | [Building sale](#16-building-sale) | B4 |
| 17 | [Engineer capture and building benefits](#17-engineer-capture-and-building-benefits) | B6 |
| 18 | [Unit repair/service visits](#18-unit-repairservice-visits) | B7 |
| 19 | [Bridge traversal, collapse and repair](#19-bridge-traversal-collapse-and-repair) | W1 |
| 20 | [Scenery lifecycle](#20-scenery-lifecycle) | W2 |
| 21 | [Walls and active gates](#21-walls-and-active-gates) | W3 |
| 22 | [Cargo release and retry across routes](#22-cargo-release-and-retry-across-routes) | U4, U5, S8 |
| 23 | [Transport boarding, carrying and passenger-dependent weapons](#23-transport-boarding-carrying-and-passenger-dependent-weapons) | U4 |
| 24 | [Infantry garrisons and Battle Bunkers](#24-infantry-garrisons-and-battle-bunkers) | U5 |
| 25 | [Tank Bunkers](#25-tank-bunkers) | U6 |
| 26 | [Slave Miner workforce](#26-slave-miner-workforce) | R2 |
| 27 | [Grinder admission and settlement](#27-grinder-admission-and-settlement) | B8 |
| 28 | [Hover movement](#28-hover-movement) | M3, U1 |
| 29 | [Shared air motion](#29-shared-air-motion) | M4 |
| 30 | [Airfield sorties](#30-airfield-sorties) | U7 |
| 31 | [Jumpjet unit transitions and required abilities](#31-jumpjet-unit-transitions-and-required-abilities) | U9 |
| 32 | [Spawn pools and launched rockets](#32-spawn-pools-and-launched-rockets) | U8, M6 |
| 33 | [American and Tech Airport paradrops](#33-american-and-tech-airport-paradrops) | S8 |
| 34 | [Remaining Chrono movement and recovery](#34-remaining-chrono-movement-and-recovery) | M5, X14 |
| 35 | [Concealment, detection and affected composition](#35-concealment-detection-and-affected-composition) | W5, V5 |
| 36 | [Reversible mind control](#36-reversible-mind-control) | U10 |
| 37 | [Parasite-host behavior](#37-parasite-host-behavior) | U11 |
| 38 | [Gattling progression](#38-gattling-progression) | X1 |
| 39 | [Prism support](#39-prism-support) | X2 |
| 40 | [Tesla charging and attacks](#40-tesla-charging-and-attacks) | X3 |
| 41 | [Ordinary laser attacks](#41-ordinary-laser-attacks) | X5 |
| 42 | [Sonic attacks](#42-sonic-attacks) | X4 |
| 43 | [C4 and bridge charges](#43-c4-and-bridge-charges) | X6 |
| 44 | [Ivan bombs](#44-ivan-bombs) | X7 |
| 45 | [Suicide attacks](#45-suicide-attacks) | X8 |
| 46 | [Desolator radiation](#46-desolator-radiation) | X9 |
| 47 | [Virus effects](#47-virus-effects) | X10 |
| 48 | [Chaos Drone](#48-chaos-drone) | X11 |
| 49 | [Other active fire/status effects](#49-other-active-firestatus-effects) | X12 |
| 50 | [Yuri deployment pulses](#50-yuri-deployment-pulses) | X13 |
| 51 | [Temporal erasure](#51-temporal-erasure) | X15 |
| 52 | [Magnetron lift and release](#52-magnetron-lift-and-release) | X16 |
| 53 | [Boris airstrikes](#53-boris-airstrikes) | X17 |
| 54 | [Dog leap](#54-dog-leap) | X18 |
| 55 | [Floating Disc attacks and drain](#55-floating-disc-attacks-and-drain) | X19 |
| 56 | [Spy infiltration](#56-spy-infiltration) | X20 |
| 57 | [Psychic Sensor intent warnings](#57-psychic-sensor-intent-warnings) | W6 |
| 58 | [Crates and rewards](#58-crates-and-rewards) | W7 |
| 59 | [Iron Curtain and Force Shield](#59-iron-curtain-and-force-shield) | S3, S4 |
| 60 | [Chronosphere and ChronoWarp](#60-chronosphere-and-chronowarp) | S5 |
| 61 | [Nuclear Missile](#61-nuclear-missile) | S1 |
| 62 | [Lightning Storm](#62-lightning-storm) | S2 |
| 63 | [Psychic Dominator](#63-psychic-dominator) | S6 |
| 64 | [Genetic Mutator](#64-genetic-mutator) | S7 |
| 65 | [Spy Plane](#65-spy-plane) | S9 |
| 66 | [Remaining modified-pixel composition](#66-remaining-modified-pixel-composition) | V5 |
| 67 | [Combat-light composition](#67-combat-light-composition) | V6 |
| 68 | [Searchlights](#68-searchlights) | V7 |
| 69 | [Options across launcher and in-game screens](#69-options-across-launcher-and-in-game-screens) | F3 |
| 70 | [Shell navigation and complete skirmish flow](#70-shell-navigation-and-complete-skirmish-flow) | F3, F1 |
| 71 | [Generated maps and shared launch handoff](#71-generated-maps-and-shared-launch-handoff) | F2, F1 |
| 72 | [Authored scenario execution](#72-authored-scenario-execution) | F5 |
| 73 | [Briefing and movie playback](#73-briefing-and-movie-playback) | F7 |
| 74 | [Campaign progression](#74-campaign-progression) | F6 |
| 75 | [Record and replay](#75-record-and-replay) | F9 |
| 76 | [LAN multiplayer](#76-lan-multiplayer) | F10 |
| 77 | [Chosen online service](#77-chosen-online-service) | F11 |

## Ordered goal scopes

**Selection** names the portion to consider now. **Coverage and completion**
preserves the full referenced scope; use only the portion authorized by the
chosen goal. Variants listed here are starting coverage, not an exhaustive
active-retail census. Full-family goals include reachable civilian, preplaced,
reinforcement, elite, special-acquisition and mode/map/campaign variants
where they affect the named scope. Consult the [shared-work guidance](#choose-a-porting-goal-before-selecting-coverage)
when combining entries, and the [family notes](#family-scope-notes) for
additional movement, rendering and strategic-power obligations.

### 1. Ground orders and movement

**Selection:** Selection/orders through driving, ship tracks, walking, blockage and usable arrival; group shared fixes.

**Coverage and completion:**

#### C1: Battlefield controls reach actual gameplay

Scrolling/bookmarks, single/bandbox/type/control-group selection, hotkeys, queued/planned waypoints, radar navigation and permitted pause/resume/speed changes.

View/select → cursor/order admission → visible/audible feedback → actual execution → replacement/cancellation. Reach U1/U2 or the actual selected action. A marker or directly injected simulation command cannot certify the player interaction.

#### U1: Units obey movement/replacement orders through arrival

Walking, driving, ship and hover variants.

Command → mission/destination/installed locomotor → path/turn/traffic/occupancy/crush → arrival → actual next action. Include stop, blocked recovery, replacement and death. Select the affected locomotor set explicitly; sharing this contract does not make all locomotors identical. Ability-specific movement and sorties stay integrated with their parent actions.

Related selections: [28](#28-hover-movement). These cover portions or shared consumers of the same scope.

#### M1: Drive and Ship track movement

Shared track admission/transition and lepton advancement justify joint changes across land/water variants. Cover order → route/turn → cell transition/occupancy → arrival/replacement and blocked recovery. Preserve separate path/runtime fields and ship admission rules.

#### M2: Walking and infantry spatial movement

Shared ground path/crossing machinery with Walk-specific stepping and blocked behavior. Cover real orders, subcell occupancy, bridges, recovery, arrival and next action; include stance/prone consumers where affected. This does not automatically include every infantry weapon.

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo); [E13 Movement implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e13-movement-implementation-evidence).

**Shared scope notes:** [movement lifecycle and output rules](#movement-and-locomotion-implementation-groups).

### 2. Standard resource economy

**Selection:** TIBTRE/ore/gems through War and Chrono harvesting, refinery income and the next trip; include required teleport work.

**Coverage and completion:**

#### R1: Resource supply becomes spendable income through standard miners

Map ore/gems, TIBTRE01–03, growth/spread, War/Chrono Miner, refineries, cargo/value/storage and Ore Purifier.

Supply → authoritative resource cell → visible/searchable resource → mining/cargo → return/dock → income → release → next trip. Preserve different return paths and source-placement versus existing-field-growth rules. Include relevant orders/combat interruptions and concurrent-spending checks. Existing spending systems consume the wallet; this is not ownership of all economy code.

**Starting evidence:** [E1 Resources and mining](2026-09-11-gameplay-boundary-evidence.md#e1-resources-and-mining).

### 3. Purchases and delivery

**Selection:** Shared factory lifecycle through usable units or placed buildings, including affected variants and queue continuation.

**Coverage and completion:**

#### B1: A purchase becomes a usable product and the queue continues

Infantry, vehicles, ships, aircraft, structures/walls; faction/factory variants; prerequisites, build limits, prices, active building upgrades/slots, Industrial Plant and Cloning Vat consumers.

Sidebar/AI order → eligible queue → factory-held identity → charging/hold/cancel → completion → exit or placement/buildup/activation → next product. Blocked exits, provider loss, upgrade admission/effects, cloned-product delivery and completed-object disposition belong here. Building placement and mobile delivery are terminal variants of one purchase lifecycle.

**Starting evidence:** [E3 Purchases and base lifecycle](2026-09-11-gameplay-boundary-evidence.md#e3-purchases-and-base-lifecycle).

### 4. MCV deployment and recovery

**Selection:** Stock variants through usable bases and applicable reverse conversion.

**Coverage and completion:**

#### B2: MCVs establish and relocate usable construction bases

Stock MCV/Construction Yard variants and applicable reverse conversion.

Command → stop/turn/admission → successful target creation → source removal → selection/build authority → reverse conversion and usable mobile unit. Coordinate sale-completion machinery with B4. Slave Miner stays with R2's workforce lifecycle.

**Starting evidence:** [E3 Purchases and base lifecycle](2026-09-11-gameplay-boundary-evidence.md#e3-purchases-and-base-lifecycle).

### 5. Power and provider-dependent behavior

**Selection:** Sources, occupants and capability shutdown/restoration through actual consumers.

**Coverage and completion:**

#### B5: Power/provider changes disable and restore the right capabilities

Plants, Bio Reactor occupants, radar, production, defenses, Robot Control Center/Robot Tanks and strategic availability.

Gain/change/lose provider → output/demand/provider state → actual shutdown → feedback → restoration and resumed use. Bio Reactor passenger admission/release must reach power. Establish Robot's specific provider relationship; generic positive house power is not a sufficient specification. Tesla charging and Disc drain include their B5 integration.

**Starting evidence:** [E4 Power capture and benefits](2026-09-11-gameplay-boundary-evidence.md#e4-power-capture-and-benefits).

### 6. Ordinary combat

**Selection:** Order/targeting through firing, impact, damage, death and subsequent action; select shared changed branches.

**Coverage and completion:**

#### U2: An ordinary engagement resolves from order to aftermath

Common infantry/vehicle/building/naval/air cases; direct, ballistic, homing and applicable anti-air/air-to-air/strafe/bombing attacks.

Approach/acquire → weapon/range/facing/fire gates → burst/projectile/impact → armor/damage/fear/prone/veterancy/death/attribution → next order/target. Include debris and affected terrain/resource consequences. Reuse shared combat authority; representative units are coverage cases, not duplicate implementations.

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo).

### 7. Vision and reveal publication

**Selection:** Per-viewer knowledge and complete Psychic Reveal behavior; coordinate shared publication while retaining source-specific lifetime and admission.

**Coverage and completion:**

#### W4: Exploration and shroud sources produce correct per-viewer knowledge

Scout/source activation → reconciliation → tactical/radar/selection/targeting → source loss/ownership/alliance/observer changes. Gap/SpySat are explicit source/latch cases; strategic reveals include their activation/effect lifecycle. Powered radar and revealed terrain are different facts.

#### S10: Psychic Reveal

Targeting → actual per-viewer knowledge effect → required duration/termination.

**Starting evidence:** [E7 World and information](2026-09-11-gameplay-boundary-evidence.md#e7-world-and-information); [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 8. World depth and draw order

**Selection:** Correct mixed terrain, bridge, building, sprite and voxel occlusion in motion.

**Coverage and completion:**

#### V4: Mixed terrain/SHP/VXL depth and draw order

Shared tactical draw plan, ground-parent ordering, lowering, native Z policies and final pass submission. Group terrain, bridge, building, infantry, vehicle and upper-layer consumers affected by an ordering change. Compare occlusion during motion and bridge crossings, with parts/effects attached to the correct parent. This does not require rewriting each asset decoder.

**Starting evidence:** [E14 Rendering implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).

**Shared scope notes:** [rendering output and comparison rules](#lighting-and-rendering-implementation-groups).

### 9. World lighting and palettes

**Selection:** Source/profile changes through actual lit terrain, actors and animations.

**Coverage and completion:**

#### V1: Scenario/cell lighting and palette-lit world appearance

Follow authoritative lighting sources/profile → derived cell grid → per-drawer palette/brightness choice → actual terrain, building, infantry, vehicle and animation output. Shared grid or palette-conversion changes should cover affected consumers together. Grid lifetime and palette arithmetic can be separate increments; preserve cell/ColorScheme/animation differences. Compare the same scene before/after source/profile changes and affected restore paths. Aircraft altitude brightness remains a targeted investigation.

**Starting evidence:** [E14 Rendering implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).

**Shared scope notes:** [rendering output and comparison rules](#lighting-and-rendering-implementation-groups).

### 10. Voxel appearance and shadows

**Selection:** Facing/parts/slopes through body composition; remaining shadow shape, placement and darkening across the selected full scope. Combine shared work, preserving distinct geometry producers.

**Coverage and completion:**

#### V2: Voxel bodies, parts and slope appearance

Common native preparation/raster, CPU/GPU paths, atlas keys and visible slope-transition cache. Group affected body/turret/barrel, facing and slope variants through their in-game composition. Preserve model/HVA/VPL responsibilities; previews are not production proof. A cache hit or one flat-facing image cannot establish moving/ramp appearance.

#### V3: Shadow shape, placement and destination darkening

Follow shadow producer → cached/masked geometry → projected position/depth admission → darkened scene. Common destination blending changes need affected SHP/VXL/terrain-shadow checks; their geometry producers are distinct. Ordinary Ground-band, uncloaked Drive units with flat single-section voxel shadows have a narrower established path than slopes, multiple sections or aircraft. Compare overlap, motion and applicable altitude/ramp cases; do not infer every shadow uses the same generator.

**Starting evidence:** [E14 Rendering implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).

**Shared scope notes:** [rendering output and comparison rules](#lighting-and-rendering-implementation-groups).

### 11. GI and Guardian GI deployment

**Selection:** Both sustained-stance variants through firing, interruption and resumed movement.

**Coverage and completion:**

#### U3: GI and Guardian GI fight through deployment and recovery

Both sustained deployed-infantry variants.

Deploy input → stance/animation/movement gate → actual deployed targeting/firing → undeploy/reorder/damage/death. Integrate ordinary movement/combat and applicable host interactions. Desolator radiation and Yuri pulses remain with their effects; Siege Chopper with U9.

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo).

### 12. AI base decisions and recovery

**Selection:** Production/deployment/placement decisions through a functioning base; retain the rest of full-opponent scope when requested.

This selection focuses on base decisions. A complete F4 opponent also requires [Team execution](#13-ai-team-execution).

**Coverage and completion:**

#### F4: An AI opponent builds, fights, defends and recovers

House decisions → ordinary production/deploy/place/attack consumers → resulting state → future decisions. Connected internal loops: base economy/placement/rebuilding and team selection/recruitment/script execution/replenishment. Faction/difficulty are variants. A loaded registry or advancing cursor cannot establish an actual attack/defense. Base decisions and Team execution are distinct implementation focuses within this full outcome.

Related selections: [13](#13-ai-team-execution). These cover portions or shared consumers of the same scope.

**Starting evidence:** [E9 AI scenarios and session flows](2026-09-11-gameplay-boundary-evidence.md#e9-ai-scenarios-and-session-flows); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 13. AI Team execution

**Selection:** Recruitment, scripts and real combat effects through replenishment; integrate with existing base decisions.

Select recruitment, script effects and replenishment here; integrate with base decisions when the goal is a complete opponent.

**Coverage and completion:**

- [F4: An AI opponent builds, fights, defends and recovers.](#f4-an-ai-opponent-builds-fights-defends-and-recovers) — full scope in [entry 12](#12-ai-base-decisions-and-recovery).

**Starting evidence:** [E9 AI scenarios and session flows](2026-09-11-gameplay-boundary-evidence.md#e9-ai-scenarios-and-session-flows); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 14. Save and resume

**Selection:** Group quickload/panel restoration; include cold-start content preparation for the complete supported loading flow.

**Coverage and completion:**

#### F8: Save a match and resume it through supported entry points

UI → content/version validation → prepared world/fixups → commit → rebuild presentation/reset pacing → continued play. Include shell startup/in-game paths, failure/cancel and active feature state. Group quickload/panel restoration work; cold-start content preparation is an additional focus. Restoring within an existing content context cannot by itself establish cold-start loading.

**Starting evidence:** [E10 Persistence networking and presentation](2026-09-11-gameplay-boundary-evidence.md#e10-persistence-networking-and-presentation); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 15. Building repair

**Selection:** Spending/healing through interruption and continued building operation.

**Coverage and completion:**

#### B3: Buildings repair correctly while the match continues

Repair toggle, funding, HP/damage states and power effects.

Order → scheduled debit/healing → presentation/power update → completion/cancel/destruction. Include competing production/depot spending where ordering matters. The building remains the same live object; this differs from sale and unit service.

**Starting evidence:** [E3 Purchases and base lifecycle](2026-09-11-gameplay-boundary-evidence.md#e3-purchases-and-base-lifecycle).

### 16. Building sale

**Selection:** Dependents, refund and capability changes through usable released units and ground.

**Coverage and completion:**

#### B4: Selling releases a building, its dependents and its value correctly

Ordinary, occupied, docked, upgraded and capability-providing variants where active.

Sell → animation/state → crew/passenger/refinery/bunker release → removal → refund and house/production/power changes → usable ground and released units. Check the resulting world, not only a refund formula. B2 owns reverse-MCV acceptance when this machinery is involved.

**Starting evidence:** [E3 Purchases and base lifecycle](2026-09-11-gameplay-boundary-evidence.md#e3-purchases-and-base-lifecycle).

### 17. Engineer capture and building benefits

**Selection:** Capture through actual benefit and later revocation, preserving distinct benefit owners.

**Coverage and completion:**

#### B6: Engineers capture buildings, use their benefits and lose them correctly

Base/factory structures, Oil Derrick, Secret Lab, Tech Hospital/Machine Shop and other active tech variants.

Approach/admission → old-owner cleanup → transfer/engineer disposition → actual benefit → later loss/revocation. Each benefit retains its mechanism owner: production, periodic income, unlocks or passive healing. Hospitals/Machine Shops are not repair-depot visits. Engineer bridge-hut behavior integrates W1.

**Starting evidence:** [E4 Power capture and benefits](2026-09-11-gameplay-boundary-evidence.md#e4-power-capture-and-benefits).

### 18. Unit repair/service visits

**Selection:** Admission, contention and spending through release and the next order or mining trip.

**Coverage and completion:**

#### B7: A unit visits a service facility and returns to use

Active repair-depot/service variants, including affected miners.

Enter/radio/reservation → service/debit/healing → release → next order or resumed mining. Include contention, interruption and provider loss. Reuse docking primitives while retaining refinery unload and airfield sorties as their distinct complete loops.

**Starting evidence:** [E4 Power capture and benefits](2026-09-11-gameplay-boundary-evidence.md#e4-power-capture-and-benefits).

### 19. Bridge traversal, collapse and repair

**Selection:** Connected topology, occupancy, engineer and visible-world lifecycle.

**Coverage and completion:**

#### W1: Bridges remain coherent through traversal, damage, collapse and repair

High/low/orientation variants share the world lifecycle. Include occupants/on-under passage, combat, engineer/hut admission, topology/zone/overlay/radar refresh and traversal after repair. Repair animation is insufficient without restored passage.

**Starting evidence:** [E7 World and information](2026-09-11-gameplay-boundary-evidence.md#e7-world-and-information).

### 20. Scenery lifecycle

**Selection:** Loading/occupation through active damage/removal and spatial/visual cleanup.

**Coverage and completion:**

#### W2: Scenery affects the world throughout its lifetime

Trees/rocks share terrain lifecycle with type/theater/immune/damage variants: load → occupation/appearance → applicable interaction → removal/spatial cleanup. TIBTRE integrates this terrain owner while spawning is R1. Do not assume every tree burns or can be crushed.

**Starting evidence:** [E7 World and information](2026-09-11-gameplay-boundary-evidence.md#e7-world-and-information).

### 21. Walls and active gates

**Selection:** Placement and connected obstacle behavior through passage changes and removal.

**Coverage and completion:**

#### W3: Walls and active gates alter passage correctly

Connections, placement/load, opening/obstruction, damage/removal and navigation/appearance stay together. B1 owns purchased wall delivery; this loop owns the obstacle. Prove active YR gate variants before importing legacy fence behavior.

**Starting evidence:** [E7 World and information](2026-09-11-gameplay-boundary-evidence.md#e7-world-and-information).

### 22. Cargo release and retry across routes

**Selection:** Shared departure work across vehicles, landed aircraft, garrisons and paradrops; route-specific rules stay explicit.

The shared change is cargo-head removal, recorded size and failed-release restoration across vehicle, landed-aircraft, garrison and paradrop callers. Cover successful release and retry. Preserve route geometry, cadence, parachute attachment and weapon-reset differences. Full transport combat, garrison combat and strategic-power lifecycles are additional scopes; NATBNK uses a separate reciprocal link.

**Coverage and completion:**

- [U4: Mobile transports and passengers work through carrying, fighting and release. Land/sea/air transports, IFV and Battle Fortress.](#u4-mobile-transports-and-passengers-work-through-carrying-fighting-and-release) — full scope in [entry 23](#23-transport-boarding-carrying-and-passenger-dependent-weapons).

- [U5: Infantry occupy, fight from and leave buildings. Civilian garrisons and Soviet Battle Bunker `NABNKR`.](#u5-infantry-occupy-fight-from-and-leave-buildings) — full scope in [entry 24](#24-infantry-garrisons-and-battle-bunkers).

- [S8: American and Tech Airport paradrops](#s8-american-and-tech-airport-paradrops) — full scope in [entry 33](#33-american-and-tech-airport-paradrops).

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo); [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 23. Transport boarding, carrying and passenger-dependent weapons

**Selection:** Complete mobile cargo behavior; distinguish IFV host weapons from Battle Fortress passenger firing.

**Coverage and completion:**

#### U4: Mobile transports and passengers work through carrying, fighting and release

Land/sea/air transports, IFV and Battle Fortress.

Admission → cargo membership/concealment → movement and applicable firing → unload placement/retry or host destruction → usable passengers. IFV selects the host's weapon; Battle Fortress passenger firing retains its own responsibility. Air-transport landing/exit is a required variant. Use the cargo-departure group across U5/S8 when changing that shared operation; keep host-weapon and passenger-firing work explicit.

Related selections: [22](#22-cargo-release-and-retry-across-routes). These cover portions or shared consumers of the same scope.

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 24. Infantry garrisons and Battle Bunkers

**Selection:** Occupation through firing, ownership changes and usable evacuation.

**Coverage and completion:**

#### U5: Infantry occupy, fight from and leave buildings

Civilian garrisons and Soviet Battle Bunker `NABNKR`.

Admission → occupant/ownership state → occupant firing/credit and occupied art → voluntary/forced evacuation, sale or destruction → released actors/building state. Keep presentation and firing with occupancy.

Related selections: [22](#22-cargo-release-and-retry-across-routes). These cover portions or shared consumers of the same scope.

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo).

### 25. Tank Bunkers

**Selection:** The separate reciprocal vehicle link through installation, fighting and release.

**Coverage and completion:**

#### U6: Yuri Tank Bunker `NATBNK` installs, supports and releases a vehicle

Approach/radio → reciprocal single-vehicle link/install → actual combat → release/sale/destruction → link cleared and vehicle usable. This differs from U5's infantry occupation model.

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo).

### 26. Slave Miner workforce

**Selection:** Shared resource consumers plus the distinct worker, deployment, relocation and liberation lifecycle.

**Coverage and completion:**

#### R2: Slave Miner and its workforce sustain harvesting through relocation and loss

Mobile/deployed master, slaves, assignment, harvesting, replacement/liberation and ownership.

Share R1's resource/payout contracts; own master/worker state, return/deposit, deployment and cleanup here. Verify resources produced by R1 remain usable by slaves. Worker deposit does not use the standard refinery queue.

**Starting evidence:** [E2 Slave economy](2026-09-11-gameplay-boundary-evidence.md#e2-slave-economy).

### 27. Grinder admission and settlement

**Selection:** Consumption and value through interruption/cleanup and continued facility use.

**Coverage and completion:**

#### B8: A Grinder admits and consumes a unit with correct settlement

Eligible units, ownership and interrupted entry.

Approach/admission → consumption → value/house effects → cleanup and continued facility use. Shared credits or Enter machinery do not turn this into building repair or sale. Remaining exact admission/settlement rules require a targeted native trace.

**Starting evidence:** [E4 Power capture and benefits](2026-09-11-gameplay-boundary-evidence.md#e4-power-capture-and-benefits).

### 28. Hover movement

**Selection:** Steering, height and permitted terrain transitions through usable arrival.

Select Hover-specific work from U1 here; reuse the ground-order contracts established in entry 1.

**Coverage and completion:**

#### M3: Hover motion

Steering, throttle and vertical motion form a distinct focus using ground movement consumers. Cover permitted land/water transitions, acceleration/turning/height, stop/replacement and usable arrival. Check other locomotors when changing their shared ground infrastructure.

- [U1: Units obey movement/replacement orders through arrival. Walking, driving, ship and hover variants.](#u1-units-obey-movementreplacement-orders-through-arrival) — full scope in [entry 1](#1-ground-orders-and-movement).

**Starting evidence:** [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo); [E13 Movement implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e13-movement-implementation-evidence).

**Shared scope notes:** [movement lifecycle and output rules](#movement-and-locomotion-implementation-groups).

### 29. Shared air motion

**Selection:** Changed Fly/Jumpjet motion through real cell-list and altitude transitions; carry required consumer work with it.

**Coverage and completion:**

#### M4: Shared Fly/Jumpjet air motion and branch-specific transitions

Joint common air-motion/cell-list changes must exercise both; Fly and Jumpjet retain different speed/altitude/landing decisions. Airfield return/rearm (U7), Jumpjet deployment (U9) and cargo (U4) are explicit additional focuses required for their complete parent outcomes. Common flight work alone does not certify complete aircraft behavior.

**Starting evidence:** [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups); [E13 Movement implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e13-movement-implementation-evidence).

**Shared scope notes:** [movement lifecycle and output rules](#movement-and-locomotion-implementation-groups).

### 30. Airfield sorties

**Selection:** Harrier/Black Eagle through attack, return, rearm and repeated sorties.

**Coverage and completion:**

#### U7: Harrier and Black Eagle complete repeatable airfield sorties

Both aircraft and provider/pad variants.

Production/idle → takeoff/attack → return/reservation/landing → rearm → next sortie. Include provider loss/capture, contention, replacement orders and aircraft loss. Flight alone cannot complete this loop.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects).

### 31. Jumpjet unit transitions and required abilities

**Selection:** Select remaining landing/deployment/cargo/attack outcomes by actual shared owner; whole-unit scope includes its abilities.

**Coverage and completion:**

#### U9: Hovering airborne units move and fight through required transitions

Rocketeer, Kirov, Nighthawk, Disc, Siege Chopper and active scenario variants.

Jumpjet/altitude behavior → actual attack/carry order → stop/landing where permitted → next action. Chopper landing/deployment/weapon change/resumed flight is a complete named outcome. Disc drain and Nighthawk passengers integrate their effect/cargo owners; locomotion alone cannot certify them. Split shared flight work from those additional effect/cargo/deployment focuses when selecting a bounded goal.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 32. Spawn pools and launched rockets

**Selection:** Shared launcher pools and relevant child flight through real impact/return, regeneration and the next attack.

**Coverage and completion:**

#### U8: Launchers manage spawned aircraft or rockets through repeated attacks

Carrier/Destroyer and V3/Dreadnought/Boomer.

Parent target → fixed spawn pool → child launch/attack → return/reload or missile regeneration → next attack. Include parent/child/target loss and owner changes. Returning aircraft and expendable missiles are variants of the same pool owner. Boris has a different designation/airstrike lifecycle.

#### M6: Launcher-rocket flight and impact

Rocket phases/payload are separate from ordinary air ticking. Follow parent launch → movement phases → impact/damage → child cleanup → parent regeneration/next attack (U8). A completed trajectory flag is not a completed attack.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects); [E13 Movement implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e13-movement-implementation-evidence).

**Shared scope notes:** [movement lifecycle and output rules](#movement-and-locomotion-implementation-groups).

### 33. American and Tech Airport paradrops

**Selection:** Shared handler variants through actual landed passengers and aircraft cleanup.

**Coverage and completion:**

#### S8: American and Tech Airport paradrops

Provider/payload variants → aircraft/drop → actual landed passengers and aircraft cleanup. Check both variants.

Related selections: [22](#22-cargo-release-and-retry-across-routes). These cover portions or shared consumers of the same scope.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 34. Remaining Chrono movement and recovery

**Selection:** Complete the selected infantry/unit variants and locomotor restoration; reuse miner work already done.

**Coverage and completion:**

#### M5: Teleport movement and locomotor recovery

Follow destination admission → relocation/occupancy/visual state → recovery/restored locomotor → next action. Include Chrono Miner return and ordinary player-movement consumers where changed. Active Teleport and temporary locomotor overrides remain distinct branches; Chronosphere's full two-click effect is not automatically completed by this movement work.

#### X14: Chrono movement

Teleport admission → movement/occupancy transition → recovery → usable next action, including affected infantry/miner variants. R1 includes the miner's complete return outcome.

**Starting evidence:** [E1 Resources and mining](2026-09-11-gameplay-boundary-evidence.md#e1-resources-and-mining); [E5 Movement stance and cargo](2026-09-11-gameplay-boundary-evidence.md#e5-movement-stance-and-cargo); [E13 Movement implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e13-movement-implementation-evidence).

General teleport/infantry recovery needs a targeted native-owner lookup;
the miner and installed-movement references are not its full contract.

**Shared scope notes:** [movement lifecycle and output rules](#movement-and-locomotion-implementation-groups).

### 35. Concealment, detection and affected composition

**Selection:** Simulation admission/viewer knowledge through real targeting and visible cloak behavior; retain other modified-pixel cases.

Select cloak/detection admission and affected visible composition here. Other V5 pixel effects remain selectable in [entry 66](#66-remaining-modified-pixel-composition).

**Coverage and completion:**

#### W5: Concealment and detection govern observer knowledge and attacks

Cloak/disguise exposure or sensor/provider movement → counted detection/resident reevaluation → targeting/picking/rendering → owner/limbo/expiry cleanup. Include Spy/Mirage/naval variants; share W4 without collapsing distinct concealment rules.

#### V5: Translucency, cloak and other modified-pixel composition

Select the actual native blitter/effect family and affected production consumers; follow admission/strength → source/destination pixel operation → depth/output → recovery. Current opaque palette conversion deliberately routes alpha/FX pixels elsewhere, so ordinary opaque palette parity cannot close these effects. Establish which effects share implementation before expanding to all cloaking, translucency or distortion.

Related selections: [66](#66-remaining-modified-pixel-composition). These cover portions or shared consumers of the same scope.

**Starting evidence:** [E7 World and information](2026-09-11-gameplay-boundary-evidence.md#e7-world-and-information); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups); [E14 Rendering implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).

**Shared scope notes:** [rendering output and comparison rules](#lighting-and-rendering-implementation-groups).

### 36. Reversible mind control

**Selection:** Controller variants through acquisition, real ownership consumers, capacity/overload and release.

**Coverage and completion:**

#### U10: Reversible mind control maintains and releases victims

Yuri Clone/Prime, Psychic Tower and Master Mind.

Acquisition → controller membership/owner transfer → actual order/house/production/visual consumers → capacity/overload and release/controller/victim loss → required restored state. Group controller variants; permanent Dominator effects and deployment pulses differ. Inspect acquisition and release callers, not only manager fields.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects).

### 37. Parasite-host behavior

**Selection:** Drone/Squid relationship variants through continuing effects, interruption and cleanup.

**Coverage and completion:**

#### U11: Parasites maintain and release their hosts

Terror Drone infestation and Giant Squid grapple.

Admission → parasite-host relationship → continuing effects → service/escape/detach/death → cleanup. Group the relationship with explicit variant rules; do not merge Temporal or Magnetron merely because they share the special-weapon dispatcher.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects).

### 38. Gattling progression

**Selection:** Tank/Cannon stage changes through actual firing and cooldown.

**Coverage and completion:**

#### X1: Gattling Tank/Cannon

Fire → stage progression and actual weapon/damage/feedback changes → interruption/retarget/cooldown → subsequent firing. Check unit/building consumers together.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 39. Prism support

**Selection:** The support network through supported shots and provider/target loss.

**Coverage and completion:**

#### X2: Prism support

Support membership/availability → supported shot/damage/beam → target/provider interruption and cleanup. Prism Tank is an attack consumer, not automatically part of the tower network.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 40. Tesla charging and attacks

**Selection:** Trooper/coil variants through power exceptions, actual attack and recovery.

**Coverage and completion:**

#### X3: Tesla charging/attacks

Trooper/coil relationship → charging/overpower and power exceptions → actual shot/bolt → loss/release/recovery. Ordinary Tesla weapons reuse confirmed effect machinery.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 41. Ordinary laser attacks

**Selection:** Shared applicable beam behavior through hit effects and cleanup.

**Coverage and completion:**

#### X5: Ordinary laser attacks

Fire → beam and actual hit behavior → effect/source/target expiry. Disc-specific attack/drain is X19.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 42. Sonic attacks

**Selection:** Wave creation through actual hits and source/target expiry.

**Coverage and completion:**

#### X4: Sonic attacks

Fire → wave and actual hit behavior → effect/source/target expiry. Shared drawing code does not merge this with lasers.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 43. C4 and bridge charges

**Selection:** Legal placement/action through explosion, world consequences and cleanup.

**Coverage and completion:**

#### X6: C4 and bridge charges

Legal approach/action → explosion and actor/bridge consequences → cleanup. Include world and release consumers.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 44. Ivan bombs

**Selection:** Attachment, fuse/defusing and detonation through cleanup.

**Coverage and completion:**

#### X7: Ivan bombs

Attachment/ownership → fuse/defusing → detonation or removal → cleanup/feedback. This is distinct from an immediate explosive attack.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 45. Suicide attacks

**Selection:** Firers and collateral damage through attribution and removal.

**Coverage and completion:**

#### X8: Suicide attacks

Firing/detonation and firer removal → collateral damage/credit/cleanup. Verify active unit variants.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 46. Desolator radiation

**Selection:** Deployment/firing through radiation effects, expiry and continuing world state.

**Coverage and completion:**

#### X9: Desolator radiation

Deployment/firing → radiation-site/target damage → source changes/expiry → continuing world state. Deployment belongs with this effect.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 47. Virus effects

**Selection:** Persistent attack consequences through affected actors and expiry.

**Coverage and completion:**

#### X10: Virus effects

Source attack → persistent damage/effect lifecycle → affected actors/ownership → expiry/cleanup. Include actual damage and feedback.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 48. Chaos Drone

**Selection:** Behavior changes through eligibility, interruption and recovery.

**Coverage and completion:**

#### X11: Chaos Drone

Effect admission → changed behavior → ownership/liveness interactions → expiration and resumed behavior.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 49. Other active fire/status effects

**Selection:** Select the named reachable effect and its actual consumers; no inferred all-effects mega-task.

**Coverage and completion:**

#### X12: Other active fire/status effects

Select the named active effect and its consumers; follow creation → continuing damage/behavior → expiration. This is a coverage family, not a task to implement every particle system.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 50. Yuri deployment pulses

**Selection:** Shared pulse variants through actual area damage and stance recovery.

**Coverage and completion:**

#### X13: Yuri/Prime deployment pulses

Deploy → area-damage weapon/target rules → stance recovery → next orders. Group pulse variants separately from reversible capture.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 51. Temporal erasure

**Selection:** Links and progress through competing attackers, cancellation or erasure.

**Coverage and completion:**

#### X15: Temporal erasure

Attacker/target link → suspension/progress/competing attackers → interruption or erasure → cleanup.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects).

### 52. Magnetron lift and release

**Selection:** Locomotor handoff through lift/carry/drop, restoration and damage.

**Coverage and completion:**

#### X16: Magnetron

Source/victim locomotor handoff → lift/carry → drop/restoration and terrain/damage effects → cleanup. Jumpjet is a dependency, not proof this is an ordinary aircraft.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects).

### 53. Boris airstrikes

**Selection:** Designation through actual aircraft attack/departure and repeated use.

**Coverage and completion:**

#### X17: Boris airstrike

Designation → called aircraft → actual attack/departure → source/target/cancellation cleanup and subsequent use.

**Starting evidence:** [E6 Sorties pools and attached effects](2026-09-11-gameplay-boundary-evidence.md#e6-sorties-pools-and-attached-effects).

### 54. Dog leap

**Selection:** Legal attack through impact, interruption and recovery.

**Coverage and completion:**

#### X18: Dog leap

Legal attack → leap/impact → target loss or interruption → recovery/next order, with applicable disguise/detection interactions.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 55. Floating Disc attacks and drain

**Selection:** Required flight/target admission through money/power/defense effects and release.

**Coverage and completion:**

#### X19: Floating Disc

Flight/attack/drain admission → target-class money/power/defense effects → release/source/target loss → dependent capabilities recover.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 56. Spy infiltration

**Selection:** Disguise/admission through target-specific result and actual consumer.

**Coverage and completion:**

#### X20: Spy infiltration

Disguise/approach/detection/admission → target-specific result → spy disposition → actual power/economy/information/production/research consumer. Shared Enter/ownership services do not make this engineer capture.

**Starting research:** use the [effect-owner lookup](#effect-owner-lookup) for this named ability.

### 57. Psychic Sensor intent warnings

**Selection:** Actual enemy-order eligibility through warning update and removal.

**Coverage and completion:**

#### W6: Psychic Sensors show the correct enemy-intent warnings

Provider/enemy-order eligibility → warning lines → order/source changes and cleanup. Establish its own radius/rules; this is not merely radar dots or cloak detection.

**Starting evidence:** [E7 World and information](2026-09-11-gameplay-boundary-evidence.md#e7-world-and-information).

### 58. Crates and rewards

**Selection:** Pickup through concrete rewards, removal and subsequent effects.

**Coverage and completion:**

#### W7: Crates yield complete rewards

Spawn/regeneration → ordinary movement pickup → effect/feedback/removal → later unit/economy consequences. Active reward classes are variants; include ordinary stock skirmish.

**Starting evidence:** [E7 World and information](2026-09-11-gameplay-boundary-evidence.md#e7-world-and-information).

### 59. Iron Curtain and Force Shield

**Selection:** Joint protection work with distinct target selection and blackout/recovery rules.

**Coverage and completion:**

#### S3: Iron Curtain

Eligible target application → protection/damage interactions → expiry and subsequent use.

#### S4: Force Shield

Application → protection and applicable house/power consequences → recovery.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 60. Chronosphere and ChronoWarp

**Selection:** Both linked clicks through transfer and aftermath; reuse proven movement work.

**Coverage and completion:**

#### S5: Chronosphere plus ChronoWarp

Both linked clicks → destination admission → affected-object transfer and aftermath. The first click cannot close the action.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 61. Nuclear Missile

**Selection:** Launch/descent through impact, damage and world cleanup.

**Coverage and completion:**

#### S1: Nuclear Missile (`MultiMissile`)

Launch/descent/impact → damage and world effects → cleanup.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 62. Lightning Storm

**Selection:** Scheduling through real strikes, termination and continued play.

**Coverage and completion:**

#### S2: Lightning Storm

Activation → storm scheduling and actual strikes → termination and continued play.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 63. Psychic Dominator

**Selection:** Effect sequence through permanent ownership/damage and continuing victim behavior.

**Coverage and completion:**

#### S6: Psychic Dominator

Effect sequence → actual damage/ownership result → continuing victim/world behavior.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 64. Genetic Mutator

**Selection:** Eligibility through transformation, usable replacements and source cleanup.

**Coverage and completion:**

#### S7: Genetic Mutator

Eligibility → transformation → usable resulting actors and source cleanup.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 65. Spy Plane

**Selection:** Targeting through flight, information effect and departure.

**Coverage and completion:**

#### S9: Spy Plane

Targeting → approach/overflight and actual information effect → departure/cleanup.

**Starting evidence:** [E8 Strategic powers](2026-09-11-gameplay-boundary-evidence.md#e8-strategic-powers).

**Shared scope notes:** [power lifecycle and variant rules](#strategic-powers).

### 66. Remaining modified-pixel composition

**Selection:** Translucency/distortion and other named native blitter cases not completed with cloak work.

Select the V5 translucency, distortion or other native blitter cases still required after the cloak work; reuse its supported composition changes.

**Coverage and completion:**

- [V5: Translucency, cloak and other modified-pixel composition](#v5-translucency-cloak-and-other-modified-pixel-composition) — full scope in [entry 35](#35-concealment-detection-and-affected-composition).

**Starting evidence:** [E14 Rendering implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).

**Shared scope notes:** [rendering output and comparison rules](#lighting-and-rendering-implementation-groups).

### 67. Combat-light composition

**Selection:** Live sources through overlapping scene edits and expiry.

**Coverage and completion:**

#### V6: Combat-light screen composition

Shared combat-light preparation, scene snapshot and RGB565 mask editing through real effect sources, movement and expiry. Validate overlapping lights and the resulting scene, not only a generated mask. This screen-composition path is distinct from V1's cell-light grid. Source lifecycle changes remain integrated with their weapon/particle consumers.

**Starting evidence:** [E14 Rendering implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).

**Shared scope notes:** [rendering output and comparison rules](#lighting-and-rendering-implementation-groups).

### 68. Searchlights

**Selection:** Active source/registration through submitted beam output and cleanup.

**Coverage and completion:**

#### V7: Searchlight/spotlight projection and visible beam

Follow the active source/child-light lifecycle into beam or mask generation and submitted output. Existing renderer support alone does not prove live delivery: the inspected instance builder supplies an empty spotlight vector. Trace registration/admission before implementation scope is fixed. Do not merge this with combat flashes solely because both are called lights.

**Starting evidence:** [E14 Rendering implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).

**Shared scope notes:** [rendering output and comparison rules](#lighting-and-rendering-implementation-groups).

### 69. Options across launcher and in-game screens

**Selection:** Shared profile/persistence and actual consumers, with each apply/cancel policy.

Select shared options profile, persistence and actual consumers across launcher and in-game screens, preserving their apply/cancel/preview policies. F3 also includes [shell navigation](#70-shell-navigation-and-complete-skirmish-flow).

**Coverage and completion:**

#### F3: Navigate the shell and retain settings

Menu/dialog → correct child route/settings operation → apply/cancel/back/focus → handoff or exit/persistence. Shell navigation is bounded; campaign/network mechanics are destination goals. “All destinations work” requires combined integration. Include controls, display/audio/gameplay options and restart.

Related selections: [70](#70-shell-navigation-and-complete-skirmish-flow). These cover portions or shared consumers of the same scope.

**Starting evidence:** [E9 AI scenarios and session flows](2026-09-11-gameplay-boundary-evidence.md#e9-ai-scenarios-and-session-flows); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 70. Shell navigation and complete skirmish flow

**Selection:** Menus/setup/loading through playable match, results and clean restart/exit.

Select shell routing and the complete fixed-map skirmish flow here. Reuse options work from entry 69; destination backends retain their own acceptance.

**Coverage and completion:**

- [F3: Navigate the shell and retain settings.](#f3-navigate-the-shell-and-retain-settings) — full scope in [entry 69](#69-options-across-launcher-and-in-game-screens).

#### F1: Configure, launch, play and finish a skirmish

Shell choices → actual map/rules/mode/assets/starts → loading → playable match → outcome/surrender/scores/statistics/results/restart/exit → clean next session. Include launch failure/cancel paths. Faction/map/difficulty are variants. Exercise content precedence, theaters, ramps/cliffs/shores/bounds and mode/map overrides through actual world construction and play.

Related selections: [71](#71-generated-maps-and-shared-launch-handoff). These cover portions or shared consumers of the same scope.

**Starting evidence:** [E9 AI scenarios and session flows](2026-09-11-gameplay-boundary-evidence.md#e9-ai-scenarios-and-session-flows); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 71. Generated maps and shared launch handoff

**Selection:** Accepted generation/seed identity through the actual playable map; check fixed maps when changing shared launch.

Select generation/seed semantics and accepted-map handoff here. Check the fixed-map consumer when changing shared launch; a preview alone does not establish the playable map.

**Coverage and completion:**

#### F2: Generate a map and play that same map

Options/seed → generator/preview → matching launch → movement/construction and cleanup. Share F1's launch contract; generation has its own RNG/content identity.

- [F1: Configure, launch, play and finish a skirmish.](#f1-configure-launch-play-and-finish-a-skirmish) — full scope in [entry 70](#70-shell-navigation-and-complete-skirmish-flow).

**Starting evidence:** [E9 AI scenarios and session flows](2026-09-11-gameplay-boundary-evidence.md#e9-ai-scenarios-and-session-flows); [E12 Further implementation groups](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups).

### 72. Authored scenario execution

**Selection:** Events and Team consumers through real consequences, objectives and saved continuation.

**Coverage and completion:**

#### F5: Authored scenario events produce their full consequences

Trigger/Tag/variables/latches → ordered action → real units/teams/reinforcements/camera/messages/objectives → next event/outcome and saved continuation. Share Team/command machinery with F4; scenario triggers have different conditions and persistent state. Include active convoy/special-mission cases when established.

**Starting evidence:** [E9 AI scenarios and session flows](2026-09-11-gameplay-boundary-evidence.md#e9-ai-scenarios-and-session-flows).

### 73. Briefing and movie playback

**Selection:** Media/speech/subtitles through skip/finish and correct destination.

**Coverage and completion:**

#### F7: Movies and briefing media play and return correctly

Start → synchronized video/speech/subtitles → skip/finish → correct screen/mission. Formats and destinations are variants; reusable media machinery stays shared.

**Starting evidence:** [E10 Persistence networking and presentation](2026-09-11-gameplay-boundary-evidence.md#e10-persistence-networking-and-presentation).

### 74. Campaign progression

**Selection:** Launch, mission outcomes, carryover and resumed progress with required scenario/media work.

**Coverage and completion:**

#### F6: Campaigns launch, advance and resume progress

Campaign/mission/difficulty/briefing → actual scenario → outcome → next mission/carryover → persisted progression. Integrate F5 scripts and F7 media. Campaign progression is not a skirmish setting variant.

**Starting evidence:** [E9 AI scenarios and session flows](2026-09-11-gameplay-boundary-evidence.md#e9-ai-scenarios-and-session-flows).

### 75. Record and replay

**Selection:** Actual initialized match and timed commands through completion or actionable divergence.

**Coverage and completion:**

#### F9: Record and replay a match

Identity/scenario/seed → scenario initialization → timed commands → completion/stop or actionable divergence. Shared deterministic commands do not make replay a save restore or LAN session.

**Starting evidence:** [E10 Persistence networking and presentation](2026-09-11-gameplay-boundary-evidence.md#e10-persistence-networking-and-presentation).

### 76. LAN multiplayer

**Selection:** Host/join through synchronized play, observation, outcome and disconnect handling.

**Coverage and completion:**

#### F10: Host, join, play, observe and leave LAN matches

Discovery/lobby/content/seed/options/transfer → launch → synchronized real peers → communications/alliances/observers → result/disconnect/recovery-or-abort. Select complete children such as host-to-finished-match or transferred-map-to-play. Staging/network internals alone do not establish playable LAN.

**Starting evidence:** [E10 Persistence networking and presentation](2026-09-11-gameplay-boundary-evidence.md#e10-persistence-networking-and-presentation).

### 77. Chosen online service

**Selection:** After an explicit service/support decision, actual multiplayer handoff and return.

**Coverage and completion:**

#### F11: Use a chosen online service to enter and leave actual matches

Explicit service/support decision → session/account/chat/discovery → actual multiplayer handoff → return/disconnect. Reuse F10 match authority; settle service policy before a retail-equivalent online claim.

**Starting evidence:** [E10 Persistence networking and presentation](2026-09-11-gameplay-boundary-evidence.md#e10-persistence-networking-and-presentation).

## Choose a porting goal before selecting coverage

The purpose is to port active retail behavior into VERA20k through research,
implementation, integration and demonstrated comparison. Distinguish three levels:

- **Porting goal:** the user-visible capability or family whose retail behavior
  the owner must complete. It can span multiple PRs and resumptions.
- **Coverage item:** a behavior, variant or interaction that the goal must account
  for. The coverage identifiers name these items, not separate sessions.
- **Implementation increment:** a coherent, reviewable change within the goal.
  Finishing it does not close the remaining goal.

Group work by implementation leverage: shared behavior research, state authority,
production callers and validation fixtures that a single change can improve together.
The player-visible result remains the acceptance bar, but a common gameplay theme
alone does not establish an efficient implementation group. Neither does sharing
one dispatcher, wallet or generic helper.

### Implementation groups supported by the inspected source

These are starting proposals for selecting goals, not a task queue. They describe
shared work at the inspected baseline, not how much remains or measured effort
savings. The [implementation evidence](2026-09-11-gameplay-boundary-evidence.md#e11-implementation-grouping)
records the source paths and limits. Preserve existing working behavior.

| Work to consider together | Why it can save repeated work; where to stop |
|---|---|
| War and Chrono Miner harvesting, return and unloading (R1) | Both enter the same Harvest/state-machine and refinery sequence. Research/fix their common decisions once, cover drive/teleport differences and the next trip. Include resource supply changes needed for the selected result. |
| TIBTRE placement, ore/gem cell mutation and growth (within R1) | Shared live resource authority joins producer and consumer. Combine when the change touches that authority; exercise real harvesting. A TIBTRE-only admission fix need not reopen every correct refinery branch. |
| GI and Guardian GI deployed combat (U3) | Same sustained-stance predicate and mission/weapon consumers. Cover both variants through actual fire and recovery. Other deploy effects need affected-branch checks, not automatic full radiation/pulse implementation. |
| Factory queue lifecycle across product categories (B1) | Enqueue/cancel/held identity/completion share an owner; mobile delivery and building/wall placement are terminal consumers. Include affected delivery variants. MCV conversion and sale do not become queue work merely because they involve buildings. |
| Spawn-pool launchers (U8) | Carrier/Destroyer and V3/Dreadnought/Boomer use the same manager and child-slot lifecycle. Cover returning aircraft and expendable missile branches through repeated attacks. Child flight changes may require a narrower additional implementation focus; Boris uses another owner. |
| American and Tech Airport paradrops (S8) | Both dispatch to one launch handler with payload/provider branches and the same carrier construction. Complete both variants through usable landed passengers. Spy Plane is not proven to be another variant of this handler. |
| Iron Curtain and Force Shield protection (S3–S4) | Shared invulnerability state/application and damage consumers make joint protection work a strong candidate. Keep their different recipient selection, infantry handling and Force Shield blackout/recovery explicit. Shared protection does not imply identical launch semantics. |
| Stock MCV conversion variants (B2) | Shared replacement/transfer/removal flow supports working across stock variants. Include sale machinery where reverse conversion actually uses it; it does not imply a full purchase/repair/sale goal. |

### Further cross-row groups and broad rows to subdivide

The [second-pass evidence](2026-09-11-gameplay-boundary-evidence.md#e12-further-implementation-groups)
adds the following implementation boundaries. “Together” means the named shared
behavior through its real callers; it does not absorb every behavior of each row.

| Implementation group | Coverage to combine and work to keep distinct |
|---|---|
| Cargo departure and failed-release recovery | U4/U5/S8 cross at cargo-head removal, recorded size and retry restoration. Handle affected vehicle, landed-aircraft, garrison and paradrop routes together, through successful release and retry. Keep route geometry, cadence, parachute attachment and weapon-reset differences explicit. This does not require all garrison combat or the entire strategic-power lifecycle. |
| Passenger-dependent weapon behavior | Within U4, boarding/release feeds IFV host-weapon selection and OpenTopped registration. Shared membership changes require both consumers. Full IFV gunner behavior and passenger-owned firing are distinct implementation focuses; a cargo list alone proves neither. U5 occupied-building weapon substitution also needs checks when shared selection changes. |
| Jumpjet movement across active users | Within U9, shared altitude/acceleration/landing decisions justify variant work together. Nighthawk cargo, Disc drain and Siege Chopper deployed combat add their own state and acceptance; treat them as explicit additional focuses instead of silently including them all in a locomotion fix. Whole-unit goals still include each required ability. |
| Per-viewer reveal publication | W4/S10 meet at real Psychic Reveal → vision publication. Changes to viewer propagation or knowledge state should include this strategic source alongside affected ordinary sources. Temporary reveal, whole-map exploration, Gap and cloak detection retain different producers and lifetime rules; do not equate them. |
| Options profile and its consumers across screens | Within F3, launcher and in-game options share the retail options profile and persistence. Group profile/consumer changes across both screens, with each apply/cancel/preview policy. Menu navigation/routing is a separate focus from settings behavior. |
| Existing-context save restoration | Within F8, quickload and the in-game load panel converge on preparation and commit. Group failure isolation, world replacement and output rebuilding across those entries. Cold-start content preparation/loading is a distinct additional focus required for full F8. |
| Fixed/generated-map launch handoff | F1/F2 share the accepted-map loading handoff. Group changes to that handoff with both map sources; generator algorithms/seed semantics and match outcome handling are separate work. A generated preview is not the gameplay map acceptance test. |
| AI base decisions and Team execution | F4 contains separate production/deployment/placement decisions and Team script/effect execution. Select the changed owner and its real consumers; integrate both when completing a whole AI opponent. Do not assume either implementation will cheaply finish the other. |

For example, “port passenger release and recovery across the live cargo routes”
is a coherent cross-row goal; “port all transports, garrisons and paradrops” is a
larger explicit scope. Similarly, a whole AI or save-system request can retain one
goal while using the distinct implementation focuses above internally. These
subdivisions are not a return to disconnected infrastructure phases: each selected
focus must deliver its named behavior through the production path.

### Conditional combinations and useful separations

- **Standard and Slave Miner:** combine resource extraction or payout changes
  where both use the changed contract; check both consumers. Slave assignment,
  relocation, replacement and liberation have a separate worker lifecycle (R2).
  Porting that entire lifecycle is not a cheap extra inferred from shared ore.
- **Strategic powers:** work on common availability/charge/launch support once
  when needed and validate affected real powers. S3–S4 and S8 have stronger effect
  implementation overlap. Storm scheduling, mutation and reveal use distinct
  owners; S1/S5/S6/S9 need further effect-path investigation before claiming an
  efficient grouping. “All strategic powers” remains a valid explicitly requested
  goal, but is not the default inferred from shared sidebar machinery.
- **Tank Bunker and special abilities:** the cargo-departure group above does
  not include NATBNK's reciprocal vehicle link. Inspect changed admission,
  targeting, damage or release owners before combining additional full effects;
  sharing Enter or an effect dispatcher is insufficient.
- **Repair, sale, service and capture:** generic money/ownership services support
  different transactions. Combine a demonstrated shared defect and its consumer
  checks, not every complete transaction merely because it changes house state.
- **Scenarios, campaigns, media, replay and multiplayer:** no new full-scope
  efficiency grouping is established here. The more specific shell, save and AI
  findings above do not imply these other backends are cheap additions.

Before composing a goal, identify what shared research/change will be done once,
which additional variants need mostly branch-specific work, and which would add
an independent lifecycle or substantial investigation. No numeric effort estimate
is required; uncertainty stays explicit. Include an additional variant when the
shared work provides a concrete reason, not just because its file is nearby.

Do not stop at a shared helper: the selected goal must reach real retail behavior
through its consumers and continuation/cleanup. Separate implementation increments
are fine within that goal. A regression check of an adjacent consumer does not
claim its full parity, and discovered required work cannot be relabeled adjacent.
An explicit full-family request retains its entire completion obligation even when
the implementation is best divided internally.

## Start with the requested outcome

**A row identifier alone is not a goal prompt.** R1, B1, B5, U1/U2, U9 and
F1/F4/F10 in particular describe broad families. State the actual intended scope:

| User's intended result | Preserve this completion bar |
|---|---|
| Complete an object or family like retail | Enumerate active behavior and variants, including required interactions found during the work. Sample scenarios start the investigation; they cannot close the full scope. |
| Complete a named gameplay loop like retail | Cover that loop and its named variants through continuation/cleanup. Include broken required handoffs; do not absorb unrelated behavior merely because a shared helper reaches it. |
| Fix a demonstrated regression or refactor existing code | State the trigger and intended behavior-preservation/fix bar. Do not silently turn this into exhaustive family parity or treat existing Rust output as retail evidence. |

Preserve what the user requested. Do not downgrade a full-family goal to one
convenient scenario, or expand a bounded fix into a full-family project. Resolve
discoverable details from current source/evidence; ask only when a consequential
scope or support decision cannot be inferred. A wide scope can take multiple
PRs and resumptions without becoming multiple disconnected goals.

### A concise goal brief

Use the [goal-prompt skill](../../.agents/skills/goal-prompt/SKILL.md) when composing
the final text. State the outcome/reason, scope/variants, starting evidence,
comparison and production bar, dependency boundary and completion condition.
For a proposed grouping, name the shared research or implementation that makes
the included variants worth doing together.
Use current branch/HEAD and reproduction when available; an unknown discrepancy
can be investigated inside the goal without inventing a finding. This brief is
the prompt itself, not an additional mandatory planning artifact.

> Follow AGENTS.md. Deliver **[outcome]** because **[problem or capability]**.
> The scope is **[full object/family, named loop, or bounded fix]**, including
> **[variants and necessary interactions]**. Start from **[relevant evidence and
> known current context]**, verifying assumptions against current source.
> Compare **[the same native operation/inputs, or the behavior-preservation bar]**
> and validate **[real entry → result → continuation/release]**, including
> **[important interruption/ownership cases and observable outputs]**.
> Reuse working behavior, include coherent prerequisites and check affected
> consumers. After each coherent increment, obtain fresh independent criticism
> of requirements, original evidence, the complete diff and validation; correct
> confirmed findings and repeat review until passed. Maintain affected evidence
> and documentation after independent confirmation, respecting annotation authority.
> Complete the named scope with a final audit for omissions and cross-mechanism
> gaps; a helper, passing sample or merged PR
> alone does not close it. Preserve any remaining in-scope work for continuation.

Carry the current goal-prompt skill's standing PR-creation preference into final
implementation prompts unless narrowed by the user; merge needs its own authority.
Preserve established publication authority, budgets and workflow settings. Leave
design/decomposition/tools open except for real constraints. The scope
examples in this guide also require that skill's independent correction loop and final
whole-scope acceptance when expanded into implementation prompts.
If implementation is authorized, preparation alone does not complete the task.

### Load the relevant evidence, not the whole guide

Each numbered entry links directly to the relevant companion section. These are
dated starting leads, not a fixed reading list or permission to skip newly found
consumers. Follow current source and native references as the action requires.
The phase inventory and migration history are optional coverage aids.

## How relationships determine scope

| Relationship found | Consequence for a goal |
|---|---|
| Same state machine or transaction, with different data/branches | Usually include as coverage of one goal and investigate together, with explicit variant checks. War/Chrono Miner, GI/Guardian GI deployment and different factory products are examples. |
| Direct producer and consumer | Include the integration needed for the named result. TIBTRE creates resource cells that growth and miners consume; a spawning-only test does not complete that resource loop. |
| A relationship that persists during an action | Own establishment, operation and release together: passenger/transport, controller/victims, launcher/spawn pool, aircraft/airfield. |
| Shared service, but different state and termination | Keep each complete action, and check affected consumers when changing the service. Sharing credits does not merge repair and sale; sharing ownership transfer does not merge engineers and mind control. |
| Same button, class name, visual effect or theme | Insufficient reason to group. MCV, GI, Desolator and Slave Miner all deploy, but their resulting lifecycles differ. |

**A goal can cross catalogue rows.** These entries identify behavior coverage
and important relationships; they are not walls around source directories.
A family can be requested whole or explicitly narrowed, and a large goal may take several
PRs. Neither fact permits a required consumer to be deferred while claiming the
named goal complete.

Do not merge every transitive dependency into one giant task. Stop expanding when
the selected action reaches its required continuing state through an established
consumer contract. For example, a purchase must yield a usable tank that can
receive its first order; it need not reimplement every tank weapon. If that first
order exposes a broken required handoff, include the repair in the purchase goal.

## Family scope notes

These notes apply to the linked coverage wherever it appears in the order.

### Resources and the base

Coverage: [R1](#r1-resource-supply-becomes-spendable-income-through-standard-miners), [R2](#r2-slave-miner-and-its-workforce-sustain-harvesting-through-relocation-and-loss), [B1](#b1-a-purchase-becomes-a-usable-product-and-the-queue-continues), [B2](#b2-mcvs-establish-and-relocate-usable-construction-bases), [B3](#b3-buildings-repair-correctly-while-the-match-continues), [B4](#b4-selling-releases-a-building-its-dependents-and-its-value-correctly), [B5](#b5-powerprovider-changes-disable-and-restore-the-right-capabilities), [B6](#b6-engineers-capture-buildings-use-their-benefits-and-lose-them-correctly), [B7](#b7-a-unit-visits-a-service-facility-and-returns-to-use), [B8](#b8-a-grinder-admits-and-consumes-a-unit-with-correct-settlement).

Coverage references are identifiers, separate from the numbered selection order. Each coverage entry includes the normal
entry, actual gameplay result, feedback and continuation or cleanup. Variants
named here are starting coverage; exhaustive goals require a full active census.
Include reachable civilian, preplaced, reinforcement, elite, special-acquisition
and mode/map/campaign variants where they affect the named scope.

#### Example: resources and miners

A broad goal can be:

> Make the standard resource economy work like retail from map/TIBTRE supply
> through ore/gem growth, War and Chrono Miner harvesting, refinery unloading and
> income, including interruptions and the next trip. Reuse existing behavior and
> establish native coverage for both miner variants.

A narrower complete goal can be:

> Make TIBTRE01–03 work from normal map loading and animation through resource
> creation, subsequent resource behavior and real miner harvesting. Include
> the shared resource changes required for that result and verify affected consumers.

The narrower goal does not become “parse TIBTRE fields,” nor does it require
reproving every unrelated gem/cost-modifier branch. If full R1 parity was
requested, all its included variants remain required. Selecting a smaller goal
must be explicit; it cannot silently shrink an existing full goal.

### Units, passengers and continuing combat relationships

Coverage: [C1](#c1-battlefield-controls-reach-actual-gameplay), [U1](#u1-units-obey-movementreplacement-orders-through-arrival), [U2](#u2-an-ordinary-engagement-resolves-from-order-to-aftermath), [U3](#u3-gi-and-guardian-gi-fight-through-deployment-and-recovery), [U4](#u4-mobile-transports-and-passengers-work-through-carrying-fighting-and-release), [U5](#u5-infantry-occupy-fight-from-and-leave-buildings), [U6](#u6-yuri-tank-bunker-natbnk-installs-supports-and-releases-a-vehicle), [U7](#u7-harrier-and-black-eagle-complete-repeatable-airfield-sorties), [U8](#u8-launchers-manage-spawned-aircraft-or-rockets-through-repeated-attacks), [U9](#u9-hovering-airborne-units-move-and-fight-through-required-transitions), [U10](#u10-reversible-mind-control-maintains-and-releases-victims), [U11](#u11-parasites-maintain-and-release-their-hosts).

#### Distinct abilities that retain their own effect loops

Coverage: [X1](#x1-gattling-tankcannon), [X2](#x2-prism-support), [X3](#x3-tesla-chargingattacks), [X4](#x4-sonic-attacks), [X5](#x5-ordinary-laser-attacks), [X6](#x6-c4-and-bridge-charges), [X7](#x7-ivan-bombs), [X8](#x8-suicide-attacks), [X9](#x9-desolator-radiation), [X10](#x10-virus-effects), [X11](#x11-chaos-drone), [X12](#x12-other-active-firestatus-effects), [X13](#x13-yuriprime-deployment-pulses), [X14](#x14-chrono-movement), [X15](#x15-temporal-erasure), [X16](#x16-magnetron), [X17](#x17-boris-airstrike), [X18](#x18-dog-leap), [X19](#x19-floating-disc), [X20](#x20-spy-infiltration).

These are effect coverage items. Include the relevant ones when porting a unit or
family; a specifically requested ability can also be a bounded goal. Distinct
effects do not mandate separate sessions. Each includes ordinary movement/combat
and presentation consumers required to exercise it. Exact branch coverage remains
work for the selected goal.

##### Effect-owner lookup

For X abilities without a specific companion section,
start with [weapon selection](../../src/sim/combat/combat_weapon.rs),
[effect dispatch](../../src/sim/combat/world_receiver.rs) and their real callers,
then trace the active native owner. The companion does not establish a full
contract for each effect. Use E4/E5/E7 for affected power, input and world
consumers, and preserve their actual admission and lifetime rules.


### World interaction and information

Coverage: [W1](#w1-bridges-remain-coherent-through-traversal-damage-collapse-and-repair), [W2](#w2-scenery-affects-the-world-throughout-its-lifetime), [W3](#w3-walls-and-active-gates-alter-passage-correctly), [W4](#w4-exploration-and-shroud-sources-produce-correct-per-viewer-knowledge), [W5](#w5-concealment-and-detection-govern-observer-knowledge-and-attacks), [W6](#w6-psychic-sensors-show-the-correct-enemy-intent-warnings), [W7](#w7-crates-yield-complete-rewards).



### Strategic powers

Coverage: [S1](#s1-nuclear-missile-multimissile), [S2](#s2-lightning-storm), [S3](#s3-iron-curtain), [S4](#s4-force-shield), [S5](#s5-chronosphere-plus-chronowarp), [S6](#s6-psychic-dominator), [S7](#s7-genetic-mutator), [S8](#s8-american-and-tech-airport-paradrops), [S9](#s9-spy-plane), [S10](#s10-psychic-reveal).

S1–S10 describe effect coverage, not a predetermined session grouping. Prefer
shared implementation work where established above: notably S3–S4 protection and
both S8 variants. Include granting/revocation, charge, targeting and recovery
needed by the selected powers. If the user explicitly requests all strategic
powers, keep all ten in scope across the necessary implementation increments.

Weather, protection, transformation and permanent ownership changes have different
state and cleanup. Keep their evidence and implementation responsibilities explicit
inside the selected goal. S5's stages and S8's variants remain coupled
coverage; neither effect differences nor row identifiers prescribe session boundaries.

Provider loss/capture, relevant power changes, targeting cancellation, repeated
use and save/restore belong to each selected power. Shared-framework changes
require affected-consumer checks. An unregistered INI section or an EMPulse-named
key used by the live nuke is not evidence of another active strategic power.

### Opponents and complete application flows

Coverage: [F1](#f1-configure-launch-play-and-finish-a-skirmish), [F2](#f2-generate-a-map-and-play-that-same-map), [F3](#f3-navigate-the-shell-and-retain-settings), [F4](#f4-an-ai-opponent-builds-fights-defends-and-recovers), [F5](#f5-authored-scenario-events-produce-their-full-consequences), [F6](#f6-campaigns-launch-advance-and-resume-progress), [F7](#f7-movies-and-briefing-media-play-and-return-correctly), [F8](#f8-save-a-match-and-resume-it-through-supported-entry-points), [F9](#f9-record-and-replay-a-match), [F10](#f10-host-join-play-observe-and-leave-lan-matches), [F11](#f11-use-a-chosen-online-service-to-enter-and-leave-actual-matches).



### Movement and locomotion implementation groups

Coverage: [M1](#m1-drive-and-ship-track-movement), [M2](#m2-walking-and-infantry-spatial-movement), [M3](#m3-hover-motion), [M4](#m4-shared-flyjumpjet-air-motion-and-branch-specific-transitions), [M5](#m5-teleport-movement-and-locomotor-recovery), [M6](#m6-launcher-rocket-flight-and-impact).

These references refine U1/U7/U8/U9/X14 and relevant cargo/combat consumers; they
are not additional copies of those systems. Start with
[E13 Movement evidence](2026-09-11-gameplay-boundary-evidence.md#e13-movement-implementation-evidence).
Common order admission, destination replacement, locomotor install/restore and
per-object scheduling remain integrated with whichever groups a change affects.
Do not create a disconnected “movement infrastructure first” completion gate.

Installed-locomotor lifetime and order teardown can justify a cross-group goal
when the same change affects several classes. Name the affected branches and
validate their real transitions; do not infer every locomotor shares the same end
gate. Active-YR reachability is required for fallback/dormant classes. Movement
state also feeds visible facing, height, slope and shadows: use V2/V3/V4 for those
handoffs when affected, without duplicating simulation authority in rendering.

### Lighting and rendering implementation groups

Coverage: [V1](#v1-scenariocell-lighting-and-palette-lit-world-appearance), [V2](#v2-voxel-bodies-parts-and-slope-appearance), [V3](#v3-shadow-shape-placement-and-destination-darkening), [V4](#v4-mixed-terrainshpvxl-depth-and-draw-order), [V5](#v5-translucency-cloak-and-other-modified-pixel-composition), [V6](#v6-combat-light-screen-composition), [V7](#v7-searchlightspotlight-projection-and-visible-beam).

These groups make visual work selectable in its own right, while remaining part
of any gameplay goal that changes the same output. Start with
[E14 Rendering evidence](2026-09-11-gameplay-boundary-evidence.md#e14-rendering-implementation-evidence).
They are shared-work proposals, not a claim that all remaining parity gaps are known.

Changes crossing these groups stay coherent: for example V2's body mask can affect
V3, and V4's depth ordering can affect V3/V5. Include that handoff and affected
output without silently absorbing every independent producer. Palette changes must
respect indexed/remap data and destination color arithmetic; body geometry, cell
illumination and shadow darkening are different responsibilities.

Use saved retail comparisons and relevant native fixtures with their stated
coverage. Validate the production GPU output through captures/readbacks where
needed; CPU math and diagnostic asset renders alone do not establish the scene.
For scale-sensitive changes, check the affected workload under ENGINE's scale
contract. Extra zoom/filtering behavior needs an explicit VERA target and must not
silently change the retail-size comparison bar. This catalogue pass ran no game,
GPU experiment or performance benchmark.

## Shared work stays integrated

Rendering, audio, input, persistence, determinism, identity and reference expiry
are obligations of affected loops, not later finishing phases. A bridge repair
includes visible/audible results; a deposit includes cargo/pips, refinery feedback
and displayed credits; save replacement clears old effects and rebuilds output.

Shared defects can justify a focused cross-consumer goal: mixed TMP/SHP/voxel
depth/palette/light/shadow composition, selection/camera projection, concurrent
unit-voice/EVA arbitration, positional sound or music transitions. Scope that
through real consumers/output, rather than “finish the rendering module.” Extra
VERA zoom behavior needs an explicit target instead of implied retail parity.

Command queues, pause/game-speed policy, clocks/timers/RNG, snapshot/hash/replay,
lifecycle cleanup and scale remain standing ENGINE obligations. Check them where
the selected action depends on them. Preserve one owner per mechanism;
overlapping acceptance does not authorize competing implementations.

## Keep the goal intact while working

When investigation crosses a row or module, distinguish these cases:

| Finding | Action within the authorized goal |
|---|---|
| Required state, producer, handoff or cleanup is broken | Include the coherent prerequisite and its validation, even across rows. A TIBTRE result cannot close while its created ore is invisible or unharvestable. |
| A changed shared owner has other consumers | Check the affected behavior in those consumers and fix regressions caused by the change. This does not automatically require full retail parity for each consumer. Preserve one owner and coordinate with any task already changing it. |
| Existing adjacent defect does not prevent the requested result and is not a regression from this change | Record its concrete trigger/effect and continue the selected goal. If the user's full-family scope includes it, it is required work rather than an adjacent deferral. |

Use current implementations and reusable native comparisons where their identity,
inputs and coverage still apply. Focus new acceptance on the selected operation
and the handoffs it can break: repeated trips/attacks, replacement orders,
contention, provider/target loss or relevant saved-state continuation. These are
case-selection aids, not a fixed test quota or a substitute for exhaustive scope.
Keep the native comparison bar separate from Rust regressions and rendered/audio
output checks, as ENGINE requires.

One owner carries the result across PRs and resumptions. Follow the existing
[checkpoint convention](../../.agents/skills/_shared/handoff.md) for sustained
work; do not create another catalogue status tracker. On resume, recover the
governing prompt/amendments, actual Git state, supported work, remaining scope,
validation/review state and next safe action. A completed prerequisite, successful
test or merged PR does not end a larger authorized goal. Missing required proof
keeps the relevant scope open; it does not authorize weakening completion or
ignoring an explicit stop/budget limit.

### Filled example: one loop, two unit variants

> Follow AGENTS.md. Complete GI and Guardian GI sustained deployed combat like
> retail (U3), preserving existing playable behavior. Scope includes both units'
> normal deploy input, stance/animation and movement gates, actual deployed
> targeting/firing, undeploy/replacement orders and relevant damage/death cleanup.
> Start with E5 and verify its current command, stance, mission and weapon
> consumers. Establish active native branches and compare the same states/inputs;
> exercise the real control-to-combat path, visible transitions and resumed
> movement/firing for both variants. Cover interruption and repeated use, with
> saved-state checks where affected. Include broken required handoffs and check
> other consumers of changed shared state. Complete this loop and retain its
> evidence. After each coherent increment, obtain fresh independent review of
> original evidence, the complete diff and validation; correct findings and repeat
> review until passed. Update affected evidence after confirmation and finish with
> a whole-scope omission/regression audit. This does not certify the entire infantry
> roster or either unit's unrelated abilities. Preserve outstanding in-scope behavior
> for continuation. Apply the goal-prompt skill's standing PR-creation preference,
> preserving any narrower instructions and separate merge authority.

For “make the GI work exactly like retail,” that narrower example is insufficient:
whole-object scope also covers ordinary movement/combat/damage, applicable host
and special-effect interactions, and lifecycle. Use entries to find those
relationships rather than substituting U3 for the user's larger objective.

## Coverage, priority and limits

The [old phase inventory](2026-07-30-clean-slate-system-implementation-order.md)
remains a source of scope candidates, including its added stock mechanisms.
This catalogue retains the original gameplay breadth while integrating assets,
presentation and lifecycle with their consumers. The
[evidence document](2026-09-11-gameplay-boundary-evidence.md) includes a migration
crosswalk so consolidation does not silently discard earlier scope.

Retain active-YR proof gates for TS fog, veins/veinhole, legacy fences,
inactive locomotors/missions/transports and strategic variants. Preserve required
data/enum round trips; section names alone are not liveness proof. Editor-only
and cheat behavior is not silently part of these goals.

Select work from demonstrated current-match problems and the benefit of finishing
a connected action. Reuse working behavior; merged fixes are not automatically
open again. Reassess after a completed goal and actual match feedback. The
[suggested order](#suggested-order) is an adaptable
selection guide, not a “finish all resources, then all units” gate.

**Evidence limit:** this examination used main
`ed8f4837910be9329505c3dfc2fc074d9c1f3106`. No game run or fresh native execution
was performed for this revision. Current paths were read directly; existing
native reports were used with their limits and stale current-status claims
excluded. The companion evidence distinguishes supported relationships from
remaining uncertainty. This is a better-grounded planning catalogue, not an
exhaustive retail-parity certification.
