# Auto play: scripted command injection

Roadmap for driving the local player's house from an agent. The agent takes the place of
the player's hands, not of the game: it observes what the player would see and issues the
same commands the player's clicks produce, through the command path the game already uses.

Status: the hands are implemented, and the assist policy's base-building chain runs in a
local match: turning auto play on makes the driven house deploy its MCV and build the
base its loaded rules ask for. See [Current state](#current-state).

This document specifies the **actuation layer** ("the hands") only: observation, the agent
interface, the host, and the admission gate. It deliberately does **not** specify the
**policy layer** ("the brain") — what to build, when to attack, which enemy to engage.
Hands first. Every decision about policy is listed under
[Deferred: the brain](#deferred-the-brain) and is open.

## Goal and non-goals

Goals:

- An agent drives the local player's house by issuing `Command`s: build, place, move,
  attack, sell, deploy, fire a superweapon.
- Those commands travel the existing deterministic path — `CommandEnvelope` →
  `Simulation::queue_command` → the frame-tail dispatch — so replay, save/load and future
  lockstep are unaffected by the agent's existence.
- The agent is just another command producer. Nothing downstream changes.

Non-goals:

- **No policy.** This document does not choose what the agent decides, and does not port,
  wrap or reuse `sim::ai_*`. That is the brain, deferred.
- **No new gameplay AI.** The existing computer-house AI is untouched by this layer.
- **No direct state writes.** The agent may not touch `Simulation` mutably. Anything that
  writes gameplay state outside `queue_command` is a takeover, not auto play, and is out
  of scope. `sim::cheats::TrainerAction` and `apply_debug_tools` are explicitly excluded:
  they run before every frame phase, are refused in network mode, and close the current
  replay segment (`src/app/match_diagnostics.rs:47-53`).
- **No native parity requirement.** This is a new capability, not a port of gamemd
  behavior. No `gamemd`-derived evidence is needed for the hands.
- **No strict equivalence with human clicks.** The agent addresses entities by id; it does
  not reproduce `What_Action`, screen coordinates, or the sidebar's cameo decision tree.
- **No network transport for the agent.** An out-of-process agent is a later, optional
  stage; the in-process interface comes first so it can be tested headlessly.

## Why hands before brain

1. Every policy candidate needs somewhere to put its hands. Building the policy first
   means building it against an imagined interface and rewriting it later.
2. The hands are cheap and risk-free: new files only, no production behavior changed, no
   owner moved. The brain is not: reusing `sim::ai_*` means splitting decisions from
   their house-state writes, which touches a live owner and every one of its callers.
3. The hands are verifiable on their own — a scripted agent can drive a full opening
   build with no policy at all, and that alone exercises the whole deterministic path.

## What already exists

The three parts that are usually hard are done.

| Piece | Where | State |
|---|---|---|
| Action space | `Command`, `src/sim/command.rs:721` | Complete: build, place, move, attack, attack-move, guard, stop, deploy, sell, rally, superweapon, and more. Semantic, not pixel coordinates. |
| Authority | `Simulation::queue_command(s)`, `src/sim/world/command_schedule.rs:30` | Append-only ingress. Preserves owner, tick and insertion order; does not sort, retime or deduplicate. |
| Replay | `ReplayLog`, `src/sim/replay.rs:725` | Records the command stream plus a state hash every 15 frames (`PLAY_HASH_INTERVAL`, `replay.rs:695`). An agent needs no new recording: its commands are already commands. |
| Stable references | `GameEntity::stable_id()`, `src/sim/game_entity.rs:1270` | Entities keep their `u64` across frames, so an agent can hold a unit across frames without its own mapping. |

Building blocks the observation layer will read, all already ported:

| Query | Where |
|---|---|
| What a house may build | `can_build`, `src/sim/production/can_build.rs:80` (native `HouseClass::CanBuild @ 0x004F7870`) |
| Where to put a building | `find_base_building_site`, `src/sim/ai_base_site.rs:146` (native `FindBaseBuildingSite @ 0x005060B0`) — read-only `&Simulation`, usable as-is |
| Whether a cell is visible | `Fog::is_cell_visible`, `src/sim/vision/mod.rs:1571` |
| Which house the player is | `ScenarioSession::current_house`, `src/sim/scenario_session.rs:839` |

Timing the agent must be written against:

- `execute_tick` is the current tick when a command is issued
  (`src/app/input/commands.rs:1036`), and a command is due when
  `execute_tick <= tick + 1` (`take_due_commands`, `command_schedule.rs:154-166`).
- Ordinary commands execute in the frame tail, inside `run_late_region`
  (`apply_due_commands`, `command_schedule.rs:676`).
- So: **a command issued before frame N's drain is drained by frame N and takes
  effect in frame N's tail** — the host runs immediately before the drain, so
  the agent's answer reaches the frame it was given. An agent must still keep
  its own waiting state: nothing in the simulation remembers what it asked for.

## Current state

| Area | State |
|---|---|
| `src/sim/control/observation.rs` | Implemented: `Observation`, the L2 read-only projection — credits, power output/drain/balance, own entities, `max_health`, ready buildings, ground walkability and the `pub` queries the assist brain needs. Each field reads its authoritative owner. |
| `src/sim/control/brain.rs` | Implemented: `Brain` and `Outbox`, the decision port. Declared on the L2 side so L2 never names a brain. See [the AI layers](ai-layers.md). |
| `src/sim/control/board.rs` | Implemented: `ControlBoard::step` — state requests → decisions → managed stepping → gate → `queue_commands`, with the two domain switches and the per-frame budget. The gate reuses `Simulation::entity_owned_by_id` and `Simulation::order_actor_admits` rather than restating them, and refuses any command whose actor is one of the player's control groups. Its four counts are logged every forty seconds (600 frames at 15 a second), because "the brain does nothing" and "the brain is refused" look the same from the screen. |
| `src/sim/behavior/` | Implemented: the L1 execution layer — the `Goal` vocabulary, the single `Funnel` with its readiness gate, `ObjectView`/`ThreatView`, and the managed routine contract. |
| `src/sim/strategy/assist/` | Implemented: `AssistBrain`, the assist decision layer. It plans over the ledger it rebuilds every frame — a staged build list (`plan::STEPS`) whose gates read the wallet and the income the house measures — adopts a base that already stands, and answers in orders and intents: deploy the Construction Yard maker, queue the first open node's type, splice a power plant ahead of a node whose drain would black the house out, place the finished product, count the node satisfied, keep the harvesters `AI_Choose_Unit`'s branch asks for, ask its factories for combat units, send the army at the nearest enemy object. It raises no `TeamClass` of its own: the native team block is gated on the house not being human, so there is no team for this brain to raise (measured, see [the native team pipeline, measured](#the-native-team-pipeline-measured)). It also carries a way back from a lost yard: buy the maker the factories can still make, drive it to a cell that admits the yard before pressing D, and pay for the maker's own `Prerequisite=` while the wallet is rich. See [the assist policy chain](#the-assist-policy-chain). |
| `src/rules/unit_role.rs` | Implemented: what each type is *for*, derived from the rules rather than from a table of names — the type's warheads' `Verses=` read into seven job weights (anti-infantry, anti-vehicle, anti-structure, anti-air, scout, artillery, support), with armour classes bracketed by what retail's warheads actually do. A type carries several weights at once, and `answer(need)` ranks the roster against one composition. A mod that retunes a warhead moves the weights; nothing here lists mod content. |
| `src/sim/autoplay/scripted_policy.rs` | Implemented: `ScriptedPolicy`, a fixed `(frame, Command)` schedule — a test brain, not what the hotkey installs. |
| `src/sim/autoplay/host.rs` | Implemented: `AutoPlayHost`, the facade the client holds. It forwards to `ControlBoard` and exposes the domain switch, the brain name, the rejection counts and the managed table. |
| `src/sim/autoplay/autoplay_tests.rs` | Implemented: 36 tests over the envelope, the gate's three rejections, the per-frame cap, input lock, the release schedule, the observation, the assist chain (MCV → yard → the plan's first buildings → army), the wallet's price bands, the yard recovery chain, the player's own units being left alone, the order funnel, managed mode and the external schema. Several run on synthetic rules so they hold without `RA2_DIR`; the rest need it to spawn real object types. |
| `docs/ai-layers.md` | The three-layer contract: boundaries, module inventory, data ownership, the direct/managed split and the reserved external seam. |
| `vera20k` hotkey category | Groundwork, delivered separately: `catalog::VERA_CATEGORY_KEY` is declared in the input layer and listed after every category the retail commands carry. |
| AutoPlay hotkey | Implemented, four keys. `HotkeyCommand::ToggleAutoPlay` is all-or-nothing: any switch off turns the whole thing on, everything on takes the host down. `HotkeyCommand::AutoPlayBuild`, `HotkeyCommand::ToggleAutoPlayForces` and `HotkeyCommand::AutoPlayAbilities` switch the base, the army and the abilities (superweapon launches: a missile, a paradrop, a spy plane) on their own. All four sit in the `vera20k` category with **no default key** — `KEYBOARDMD.INI` has no entry for them, so the player assigns them. Auto play starts **off in every match**: the host is the app's, not the world's, so a new scenario clears it. |
| Frame-loop wiring | Implemented: `advance_one_simulation_frame` steps the host immediately before `take_due_commands`, when one is installed. A match with no host takes the unchanged path. |
| Anything in `sim::ai_*` | Read by the policy through the same pure generators the computer AI uses; no AI code was changed. |

### The assist policy chain

`AssistBrain` reproduces the native order of work, one plan node at a time, and
each step is a command rather than a house-state write:

| Step | Native authority | Command |
|---|---|---|
| Generate the plan | `AI_RecalcBuildOptions 0x005054B0`, run when the yard appears | — |
| Anchor node zero, adopt the standing base | `0x00739926`; a takeover pairs each node with the building already there | — |
| Deploy the yard maker | `UnitClass::Deploy 0x007393C0` | `DeployMcv` |
| Pick the open node | `AI_Choose_Building 0x004FE3E0`, `Find_Node 0x0042EB50` | — |
| Ask whether it may be built | `HouseClass::CanBuild 0x004F7870` | — |
| Splice a plant ahead of a node its drain would black out | `AI_Choose_Building 0x004FE6F6..0x004FE943` | `QueueProduction` |
| Start it | yard `Suggest_New_Object 0x004FBD80` | `QueueProduction` |
| Place it | `Place_Production 0x004FB0E0`, `CanPlaceAt` | `PlaceReadyBuilding` |
| Count the node | `0x0042E780` / `0x0042F260` | — |
| Keep the harvesters the house needs | `AI_Choose_Unit 0x004F4D40`'s harvester branch | `QueueProduction` |
| Make another yard maker after losing the yard | `UnitClass::Deploy 0x007393C0`'s lifted `CanPlaceAt` for the site; retail `[AMCV] Prerequisite=GAWEAP,GADEPT` | `QueueProduction`, `Move`, `DeployMcv` |
| Make what the house's own teams need | `HouseClass::Suggest_New_Object 0x004FBD80`, whose slot `HouseClass::Update` fills from `TeamClass::Get_Needed_Types` over `AIMD.INI`'s TaskForces — an engineer, a spy or a transport is only ever asked for here. For a human house `Factory_AI` never reads the slot, so the policy reads it | `QueueProduction` |
| Buy what the army is short of, when no team asked | no native body: the enemy's composition against the army's job weights (`rules::unit_role` + `build::shortfall`) | `QueueProduction` |
| Raise the teams its triggers pick | not built, and not reachable as a command: `HouseClass::Update`'s team block (`0x004F8A00`) is gated on the house not being human, so it never runs here. `sim::ai_team_creation` is called once per house per frame from `HouseClass::Update` itself and has no intent or command behind it. See [the native team pipeline, measured](#the-native-team-pipeline-measured) | — (unreachable) |
| Ask for a combat unit | `AI_Choose_Unit 0x004F4D40`'s Unit, Infantry and Aircraft branches over the house's forming teams (`TeamClass::Get_Needed_Types`) | `QueueProduction` |
| Send the army at the nearest enemy object | what an attack click on that object means; native's teams and their scripts are absent. Targets are houses this one is at war with — the ally graph's complement — never a `MultiplayPassive` house, which owns the map's tech buildings, garrisonable houses and parked cars | `Attack` |
| Send an engineer, or a spy at what is worth the walk, into a building it may take | what a Capture click on that building means; `InfantryClass::PerCellProcess` decides between capturing and infiltrating on arrival. The two clicks differ in what they accept — `Capturable=` for an engineer, `Spyable=` for a spy (`InfantryClass::What_Action_OnObject 0x0051EE3B..0x0051EEEF`) — and the spy's order of targets follows `AIMD.INI`'s own spy teams (`General Spy vs Soviet Power` before `vs Allied War`, `vs Sov Tech`) | `CaptureBuilding` |
| Walk an engineer into a `BridgeRepairHut=` whose span is down | the engineer's PerCell entry into the hut, which is the only thing that rebuilds a bridge: no timer, no `Capturable=`, no other command reaches it | `CaptureBuilding` |
| Garrison a building near home | `BuildingClass::CanBeOccupiedBy 0x00457CE0` decides whether this `Occupier=` infantryman may walk in; the entry itself is the same Enter click a player makes | `MegaMission { Enter }` |

### The native team pipeline, measured

`autoplay-match --dump-teams <frames>` prints, every 600 frames and at the end,
each side's activation latches, chosen enemy, live `TeamClass` count, the units
they hold, each team's TeamType/script/cursor/current action, and how many
orderable non-building units stand outside every team. It is read-only: it
walks `TeamScriptVm::teams_in_order`, `team_for_member` and the entity store,
and never steps the simulation. The same run answers "who is fighting".

On the pinned match (`Death.mmx`, seed `0x5eed`, normal difficulty):

| Frame | Player (this brain) | Computer house |
|---|---|---|
| 0 | 0 teams, 13 loose | 0 teams, 17 loose |
| 600 | 0 teams, 12 loose, `ai_triggers_active=false` | 0 teams, 16 loose, `ai_triggers_active=true`, enemy = Player |
| 3000 | 0 teams, 13 loose | 1 team, 5 members, action `-1` (forming) |
| 3600 | 0 teams, 13 loose | 1 team, 8 members, action 54 |
| 4800 | 0 teams, 14 loose | 1 team, 8 members, action 5 |
| 6000 | 0 teams, 15 loose | 2 teams, 10 members, 12 loose |

So: this house never has a native team at all — `HouseClass::Update`'s team
block refuses a human house (`ai_team_creation::update_team_creation`'s
`is_controlled_by_human`), which is native behaviour and stays. The computer
house does get teams and its retail scripts do advance (54 → 58 → 5), but two
teams at a time and about ten of its units inside them. None of its 165
registered `AITriggerTypes` had produced a completed team by frame 6000, which
is the trigger table's own accounting (`RegisterSuccess`/`RegisterFailure` run
when a team ends), not proof that no trigger ever fired.

Consequences for the brain: it is the whole army on this side, no unit of ours
is ever inside a team, and there is nothing for the native script and this
module to fight over. The script surface is not the gap either: of the 20
action ids the retail team scripts referenced by `AIMD.INI`'s `[TeamTypes]`
actually use (`0,5,6,8,9,11,14,21,43,46,47,49,53,54,55,57,58,61,62,63`), all 20
are implemented in `team_script_vm/actions.rs`; the 25 implemented ids retail's
team scripts never reference are transport, entry and camera steps.

### Two switches

`AutoPlayDomains { production, forces }` is the host's filter, so both official
policies obey it:

| Switch | On | Off |
|---|---|---|
| `production` | queue, place, sell, repair, rally, deploy the MCV | stop issuing that kind; what is already queued keeps building |
| `forces` | unit orders and superweapon use | stop issuing new orders; orders already given keep running |

The master key installs with both; the other two keys switch one half each, so a
player can keep the base growing while commanding the troops, or hand the troops
over and take building back. A switch that would leave neither half running
takes the host down instead of leaving one driving nothing. Neither switch
undoes what was already issued — cancelling a queued building or recalling an
order would be a destructive decision the policy should not make for the player.

A third, per-object switch is the player's own control groups. The frame flattens
the ten slots into the board before each step, and the policy never reaches for a
unit in one: `Observation::owned_by_hand` is the one answer every place that
picks a unit asks — a control group member, or a unit a `TeamClass` script has
recruited — and the board's gate refuses one anyway if a policy that does not
ask ever emits it. A control group is a promise that the unit stays where the
player put it, so taking a unit is enough to take it back.

The gate alone was not enough: a refused command still spends the frame's
command budget and still counts the unit as sent for `ORDER_REFRESH_FRAMES`, so
a grouped unit quietly ate the army's orders and held its own place in the push
while doing neither. The exclusions are the board's, handed to the projection it
builds for the brain (`Observation::with_player_groups`), so the decision layer
sees them through its own read-only surface and no new query crosses a layer.

The gate filters what a policy emits. `AssistBrain` fills both halves:
production is the base plan, the spliced plants, the harvesters, the teams its
own selector raises and the units those teams are short of; forces is the
`Attack` each combat unit gets on the nearest enemy object, plus the specialists
a unit's own flags name — an engineer into a bridge hut or a building it may
capture, a spy into the building worth most, an `Occupier=` rifleman into a
garrison near home — each refreshed when a nearer target appears or after about
fifteen seconds. A unit takes one order a frame: the steps run in that priority
order and the first answer keeps the unit. Switching `forces` off leaves the army
where it stands and stops new orders of that kind; switching `production` off
stops the base, the factories and the team raises while the army keeps its
orders. Superweapons fire: `orders` launches a charged one at an enemy cell through `Intent::LaunchSuperWeapon`, because `HouseClass::AI_TryFireSW` is itself gated on the house not being human. The defensive half — a charged Iron Curtain, Force Shield or Chrono pair spent on the house's own force — is not ported.

There is no boarding step, and that is a measurement rather than an omission: an
idle rifleman climbing into his own empty carrier is a correct Enter click, but
wiring it cost the reference match nine thousand ticks (`won at 25636` to
`won at 34897`) and, with the army's strength count corrected to skip units the
gate would refuse, the match stopped winning inside its 36000-tick cap. What
goes wrong — the carrier's value once loaded, or where the boarded unit stands
relative to the push — is not established; the step stays out and recorded.

Residuals, all recorded in the module docs: a garrison is only taken within
twelve cells of one of the house's own buildings, and the bridge hut has to be
that near too — both are the policy's own radius, not a rule the game states, and
neither carries a notion of which garrison or bridge is worth holding. Units
already inside a carrier or a building are left alone: there is no unload, no
re-board and no garrison withdrawal, because when to leave is a different
decision from where to go. What the fleet is short of is read
from the enemy's *whole* composition, fog aside, on the same footing as the
target picture below; `unit_role` ranks a type by the mean of its best weapon's
`Verses=` over an armour bracket, so it does not yet weigh the mix of armour it
will actually meet, its range against the enemy's, or how many weapons a turret
can bring to bear, and the price bands behind it are the policy's own cuts rather
than a rule the game states; control nodes (`-1` walls, base
defenses, `-2`, `-3` perimeter) are stepped over rather than driven;
`AI_Manage_Build_Queue 0x004FDD10` — the recovery that sells, splices and
reorders around a node that cannot be built — is absent, so a node that makes no
progress for 900 frames is given up and the rest of the plan runs (a plan stuck
forever leaves a play test with a dead base, which is worse than a slightly
out-of-order one); the placement cell is the policy's own choice, because a
human house has no native plan centre (`+0x5750` is AI-only) and therefore no
`FindBaseBuildingSite` answer, so the policy sweeps outwards from its own
buildings for the first cell the placement gate admits, moving on when the
receiver refuses one, and abandons a product that finds no cell at all; and the
unit chooser's two AI-only gates (`IsHumanPlayer`, `House+0x24C CurrentIQ`) are
read as an AI house's would be, because a policy-driven house is played in the
player's place and a human house's IQ stays 0, which would leave the house with
no income at all.

The army's unit choices are native: a factory's own suggestion
(`HouseClass::Suggest_New_Object 0x004FBD80`) is asked first, and the enemy's
composition against the army's own job weights answers when it has none. This
house raises no team, so no `TeamClass::Get_Needed_Types` ever fills that slot:
`HouseClass::Update`'s team block is gated on the house not being human
(measured, see [the native team pipeline, measured](#the-native-team-pipeline-measured)).
The order step still stands down for a unit a team has recruited — nothing
issues twice — but on this side no unit ever is, so every unit is the policy's
to order: each combat unit is sent at the nearest enemy object and each engineer
at the nearest capturable building — what a player's click on it means — with
the whole map in view the way the computer AI has it. `AI_Manage_Build_Queue`
is the next chain.

### What citing the retail AI buys, and what it does not

A play test of this stage reported the shape of the problem plainly: units that
attack everything, engineers that are told to shoot, a base that builds whatever
the plan asks for whether or not it can pay, and an army that is "working but
not clever". None of that contradicts the retail AI; it is what you get when its
*decisions* are cited without the *state and target selection* they read.

| Part of the native chain | Where it stands here |
|---|---|
| `AI_RecalcBuildOptions` → BasePlan nodes | ported (`sim::base_plan_generation`), driven by this policy |
| `Find_Node`, `BaseClass` node satisfaction | ported (`sim::ai_base_building`) |
| `AI_Choose_Building`'s power splice | ported, now driven by this policy |
| `CanBuild`, `Place_Production`'s gates | ported, called as-is |
| `AI_Choose_Unit`'s harvester branch | ported, driven by this policy |
| `AI_Choose_Unit`'s unit/infantry/aircraft branches | reachable now that the house owns teams: their input is what the forming teams still need (`TeamClass::Get_Needed_Types`). Beside them the policy asks its own question of a free factory — the enemy's composition against the army's job weights — and that question has no native body |
| `AITrigger` evaluation, `CreateTeam`, `AIMD.INI` team types | ported, and now driven: the policy asks the house's own selector when its team timer expires, and the host applies the plan through `sim::ai_team_creation` |
| `AI_Choose_Unit`'s unit/infantry/aircraft branches | ported and reachable now that the house owns teams; the policy drives them with its own draws and leaves the house's choice slot alone |
| Team script missions (`Attack Enemy`, …) | ported and running: the team AI pass has no house gate, so a policy-driven house's teams recruit and drive their members by script. The policy's nearest-hostile rule covers only the units no team has taken |
| `AI_Manage_Build_Queue` (sell, splice, reorder, credit-cutoff production modes) | not ported; a node that cannot be built is given up after 900 frames |
| Base defense and wall nodes (`-1`, `-2`, `-3`) | stepped over |

So the rule this stage earns is: **citing a native decision is only as reliable
as the inputs you give it.** A ported generator fed by invented inputs — the
nearest hostile object, a cursor over the rules' type lists — produces a policy
that runs, which is what this stage proves, and that plays badly, which is what
it does not hide. Where a stand-in replaces a native body, it is named as one
here and in the module docs, and it is replaced by the native mechanism rather
than tuned: teams before unit choice, team scripts before target selection,
`AI_Manage_Build_Queue` before any hand-written ordering rule.

That last point is why income came before the rest of the unit chain: a base
that only builds buildings spends its starting wallet and stops part-way through
its plan, at whichever node the credits run out — a play test reported exactly
that stall at the war factory.

`ScriptedPolicy` takes a typed `Vec<(u64, Command)>`, not JSON. `Command` is
serde-derived, but its `InternedId` fields are numeric, so a JSON script could
not be written by hand without a second, competing command spelling. A
name-resolving script format is therefore a brain-stage question, not a hand
one, and H0's "Done when" below is adjusted accordingly.

## Target ownership

| Responsibility | Owner |
|---|---|
| Read-only projection of what the house can see | `sim::control::observation` |
| Decision port — the swappable brain | `sim::control::brain` (`Brain`, `Outbox`) |
| Admission gate, per-frame budget, envelope construction, managed stepping — the stable hand | `sim::control::board` |
| Object orders, readiness, the per-object view, micro routines | `sim::behavior` |
| What to build, raise, send and take | `sim::strategy` |
| Calling the host once per frame | the frame loop: `app/match_runtime/sim_tick` |
| Everything else | unchanged. No existing owner moves. |

**Placement: `src/sim/{behavior,control,strategy}/`, inside the core crate.**
Three reasons:

- The projection must call `pub(crate)` queries (`can_build`,
  `find_base_building_site`); from `app/` it cannot reach them.
- The core crate can then drive an agent with no window, which is where the tests live.
- It depends on `sim` only, so the `sim/` → `render|ui|audio|net` rule holds.

`src/sim/autoplay/` is the facade over those three: `AutoPlayHost` (installed by the
hotkey, stepped by the frame loop), `AutoPlayDomains` (the three switches: base, army,
abilities), `ScriptedPolicy` (the test brain) and the tests.

## The three layers

**[`docs/ai-layers.md`](ai-layers.md) is the contract**: boundaries, the module
inventory, data ownership, the direct/managed split, the reserved external seam and
the recorded gaps. This page keeps the player-facing half — the hotkeys, the switches
and the actuation rules below.

In one paragraph: **L1 executes** one object's order (the `Goal` vocabulary, the single
`Funnel` that gates on `Simulation::mission_ready_to_commence` and answers
`Queued`/`NotReady`/`Rejected`, `ObjectView`/`ThreatView`, and the managed micro
routines); **L2 stores and delivers** (the board's frame order, the domain switches,
the per-frame budget, the managed phase table, the projection); **L3 decides** (the
`Brain` port, with `AssistBrain` as its one implementation today). Dependencies run one
way, L3 → L2 → L1, and
`architecture_guards.rs::sim_ai_layers_depend_one_way` fails the tests if that stops
being true.

The gate is deliberately minimal. Strict human equivalence is a non-goal, so the gate
only prevents self-harm, it does not re-derive `What_Action`:

- the acting entity belongs to `owner` — needed, because MegaMission dispatch checks the
  actor's lifetime but **not** its owner (`src/sim/world/world_commands.rs:2465-2469`
  leaves that to the input layer, and the input layer is not involved here);
- the actor is alive and not in limbo;
- at most `max_per_frame` commands;
- `session.input_locked()` is respected.

## Determinism rules for policies

A policy is part of the deterministic input stream, so:

- no wall clock, no file or network reads during a decision;
- no iteration over unordered containers;
- no `f32` in a decision;
- any randomness comes from a seeded generator the policy owns, never from a simulation
  stream.

Replay stays correct regardless of what a policy does, because replay re-runs the command
stream, not the policy.

Residual: policy state is not serialized, so a save/load mid-match loses the policy's
memory of what it was waiting for. Accepted for now; it belongs with whichever policy
first needs to survive a load.

## Implementation chains

One PR each. A stage is done when its "Done when" holds.

### Stage H0: scripted actuation (skeleton delivered)

Scope: `observation`, the `policy` seam with a `ScriptedPolicy` playing a fixed list of
`(frame, Command)` pairs, `host`, and tests.

Delivered: the three modules and their tests, the frame-loop wiring, and the AutoPlay
hotkey that installs the host.

Still to do for this stage, before it counts as done:

- ~~Where the script comes from.~~ Answered by the assist policy: the hotkey installs
  `AssistBrain`, and `ScriptedPolicy` stays a test fixture rather than the shipping
  brain.
- An end-to-end scenario in `src/headless_scenario.rs`: deploy the MCV, queue a power
  plant, place it, queue a unit — reproducible through `ReplayRunner` with identical state
  hashes. The chain is covered in `autoplay_tests.rs` today, which asserts the buildings
  stand but does not replay it.

Done when: the replay scenario holds.

### Stage H1: full observation surface

Scope: fill in `Observation` — credits, power, own entities, visible enemies, buildable
list, production queue, pending placement, placement site, passability. Each field with a
test asserting it reads the authoritative owner and not a derived cache.

Done when: every field has a test that fails if the source is swapped for a stale or
derived one, and the headless script from H0 still replays identically.

After H1 the hands are complete and the brain can be designed against a real, tested
surface.

## Decided: where the brain's decisions come from

**Decision: the retail AI becomes one policy implementation on the `policy` seam — it is
not bridged into the host.** This is the difference between a Modula-2-style plugin host
and a direct coupling: the host stays stable and knows nothing about which policy drives
it, and the retail AI is a peer of a script and of a later controller rather than a
special case wired into the hand.

Two consequences worth stating, because they shape the work:

- **Nothing here is a port of gamemd's AI.** The retail AI's own decision code is a
  policy's business; the host and the observation layer are VERA's, and they must not
  grow parts that only make sense for one policy.
- **AI function is split across two INI layers, and the policy reads both**: the loaded
  rules decide the basics (what may be built, which buildings are economic and which are
  defensive), and `AIMD.INI` decides strategy, tactics and team instruction groups. A mod
  that extends either file changes the policy's decisions without touching VERA.

The policy does not write house state, `is_human` stays true, and the player keeps
selection, the sidebar and the right to order anything at any time. AI takeover is
operation, not control.

One premise behind that decision needs recording precisely, because it is easy to
overstate:

> There is no single native instruction that hands a player's house to the AI.

What exists instead is four trigger actions — **3, 13, 30, and 74/75** — each writing one
independent AI activation byte on a `HouseClass`:

| Action | Writes | Enables |
|---|---|---|
| 3 | `+0x1EE` Production | AI factory production (`production/factory_ai.rs`) |
| 13 | `+0x1EF` AutocreateAllowed | AI autocreate teams |
| 30 | `+0x1F3` AutoBaseBuilding | AI base building / BasePlan (`mcv_deploy.rs`) |
| 74 / 75 | `+0x1F2` AITriggersActive | AITrigger evaluation and team creation |

They resolve their target house through the shared action selector and have **no human
gate**, so a map can target the player's own house. But nothing in the trigger runtime
ever writes `is_human` or `player_control`: the player's selection, sidebar and ordering
rights survive untouched. That is the property to preserve, and it is also why these four
bytes are the natural granularity for "AI does the operating, the player keeps the
choice".

Note the consequence for a human house: `ai_base_building` and the other AI entries gate
on `is_controlled_by_human`, so setting these latches on the player's house does **not**
start the AI today. Auto play is what gives them meaning for a human house.

## Deferred: the brain

The seam is in place, and the base-building half of the assist policy now occupies it:
`AssistBrain` generates the plan and builds it, without touching the house's AI fields
(they stay the computer AI's). Still open:

1. **The rest of the assist policy.** Base building is the first chain. Units and
   infantry choices (`AI_Choose_Unit` and friends), `AI_Manage_Build_Queue`, base defense
   placement, walls, team creation and `AIMD.INI` strategy remain: each is a new chain on
   the same policy, and each needs its own evidence.
2. **Fog honesty.** The computer AI sees the whole map, so a policy reusing its decisions
   makes the driven house omniscient. Fine for unattended play, wrong for anything
   competitive. `Observation` offers the queries a fog-filtered view would need, and
   whether to use them is the brain's call.
3. **Per-house AI state ownership.** `ai_production`, `ai_unit_choices` and the team timers
   are the computer AI's memory, and one owner writes them: the existing AI for a computer
   house, or a policy for the house it drives — never both at once.
4. **RNG.** Some AI decision paths draw from a simulation stream. A policy must draw from
   its own, so its decisions cannot disturb the simulation's sequence; a path that shares
   the stream would make such a match unusable for native parity.
5. **Team scripts.** `team_script_vm` assigns missions directly. Some actions have no
   player equivalent, so translation is lossy and needs an action-by-action ruling.
6. **Out-of-process policy.** Whether a policy can live outside the process (BWAPI-Client /
   PySC2 style), and if so over what transport. The seam is in-process today.
7. **Driving a computer house.** `AutoPlayHost` already takes an owner rather than
   assuming the local player, so this is reachable; what it means for the existing
   computer AI, which writes house state directly, is undecided.

## The strategy layer: books, not passes

The unit decisions used to be fifteen passes over the army, each deciding for
itself and each carrying its own list of orders to defer to. That is why fixing
one unit's behaviour kept reopening another's: the rule had fifteen copies.
Two ranks now decide instead — one per frame, one across frames — and the
remaining question is what the layer above them *is*.

An RTS AI of the kind worth copying keeps four books and a map, and hands the
map's decisions down to units through an executor:

| Book | What it holds | Cadence |
|---|---|---|
| Economy | income, harvester round trip, line occupancy | 1–2 s |
| Build/tech | the stage, the next structure, prerequisites | event |
| Army | group, task, destination, strength, reinforcement | 0.5–2 s |
| Map | threat coverage, seen versus remembered, ore | 1 s or event |

The decision is about groups and books; a unit is an executor. Retail's own
shape is the content: power, then barracks, refinery, war factory, radar, lab;
two miners per bay; `Verses` deciding what counters what; kiting, crushing,
garrisoning, transports, repairs and super weapons as tactics.

The simulation already implements far more than the brain uses — gunner
(IFV turret), disguise, mind control, cloak, detector and spawns are all
represented and the policy mentions none of them. The work is the layer, not
the mechanisms.

Three invariants, and the migration that establishes them one slice at a time:

1. One object, one order per frame, chosen by the arbiter. No pass issues a
   command on its own.
2. Decisions are about groups. A group has a task, a destination and a
   strength; units execute it.
3. Every refusal comes back — unreachable, not ready, out of budget, taken by a
   stronger order — and the layer that asked answers it.

| Slice | Content | Acceptance |
|---|---|---|
| S0 | One threat type and index (`assist::threats`); the shell in `dodge` named as one | tests green, pinned match unchanged |
| S1 | `UnitRuntime`: directive, committed order, review frame | jitter count zero |
| S2 | Refusal loop (unreachable is in; not-ready, budget and preemption follow) | units idle with no intent |
| S3 | Honest perception, shadow mode first | difference list |
| S4 | Army groups replace the per-unit loops in `advance` and `forces` | units under one task stop acting alone |
| S5 | Capability tags and the pilot table: Crush, Parasite, Subdue, Gunner, Capture, Infiltrate, Transport (delivery and boarding) | 1–3 unit-level tests per capability |
| S6 | `--trace <id>`, behaviour fingerprints, golden replays | every change shows its diff |

Status (2026-11-01): S0, S1, S2, S3, S4 (advance + forces), S5 (tables) and S6 (trace) are in the
tree; S3 is on by default — the brain acts on sightings and remembered
sightings (with a term and no invented positions), and each unit sees by its
own Sight=. AUTOPLAY_HONEST=0 restores the omniscient list for a comparison:
the pinned map wins at 27792 honest against 23086 omniscient. The census line prints seen=<visible>/<all>;
on the pinned map the omniscient run reads 0/19 to 2/32 through the opening,
and the honest run reads 9/9, which is the gap a policy has to close. S4's
forces pass is deliberately on the wider census set: wiring it to the army took
the same match from a win at 23086 to a timeout, so the group's definition has
one consumer at a time until the guards are part of it.


A rule lives in one of four places — the rank table, the pilot table, the
refusal loop, or the army book — and nowhere else.



## Tracing one unit

AUTOPLAY_TRACE=<entity id> makes the brain print that unit's orders and
refusals at info level, from the two places every order passes through
(orders::issue and Brain::orders_refused):

    trace 4211: Attack { target: 91 } issued (rank 3, taken true)
    trace 4211: Move { cell: (120, 44) } refused by the world

It is a start-up switch, read once when the brain is built, and never an input
to a decision.

## Validation per stage

- Headless scenario runs (`src/headless_scenario.rs`), never retail assets in the lib
  suite.
- Replay reproduction and state-hash equality for every scripted scenario.
- The existing lib suite for the touched modules.
- No native comparison: this is not a port. If a later stage ports policy from gamemd,
  that stage states its own evidence level per the project contract.

## The push, measured

`advance` sends the whole army at one structure: the enemy economy structure
nearest the first fighter in entity-storage order. Three attempts to make that
rule smarter were run end to end on the pinned match and all three lost the
attack the current rule wins at frame 25636, so all three were reverted:

| Variant | Result |
|---|---|
| Current rule | won at 25636 (46 own entities, 2 enemy) |
| Each unit aimed at the structure whose defenders its own `unit_role` weights answer best | timeout at 36000, 4 enemy entities left |
| The fist aimed from the army's centre instead of its first fighter | timeout at 36000, 4 enemy entities left |
| One target held until destroyed instead of re-reading the nearest each frame | timeout at 36000, 11 enemy entities left |

Each order carries the target's cell, so a target that moves re-tasks units
that are already on the road; that is the mechanism the second and third
variants were meant to exploit and the first was meant to avoid, and the
measurements say the current sequence is not something a plausible local
change improves. Replacing it needs a model of why it wins — which target is
chosen in which frames — rather than another ranking of the same candidates.

The reference match on the other difficulty tier loses: `--difficulty hard` on
the same map and seed ends `lost at 32019`. Its shape, from the same log line:
the house holds about 30 objects while the computer house grows past 80, power
falls from 387 to a brownout and the wallet starts piling up, and by 30600
every command the brain emits is refused (781 of them) — the base is gone and
no factory is left to build from. So the push is not what fails there; what
fails is the base surviving long enough to feed one. Narrowing
`base_under_threat` to a house with nothing standing does not move it, because
on this map the house still has buildings standing when it matters.

## Open questions

- Does the host belong in the app frame loop only, or also inside the headless runner as
  a first-class mode? (H0 answers this in practice.)
- Should `max_per_frame` be native-derived or a plain configurable cap? Unknown native
  equivalent; propose a plain cap and record the residual.
- Does an agent-driven house need `is_human` left untouched? Assumed yes — the computer
  AI gates on `HouseState::is_controlled_by_human` (`src/sim/house_state.rs`), and with
  the measured result above the whole native team block stays off for a player house. If
  that ever changed, two owners would drive one house.
