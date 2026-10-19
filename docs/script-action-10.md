# Script action 10: follow friendlies

`10,0` selects a nearby friendly mobile object and coordinates the team's
movement with it. The argument is unused. This is a vanilla script action,
not an Ares extension. The existing ScriptType reader and its step limit are
unchanged.

## Selection and member orders

The action-10 branch of `Calc_Center` selects a live Unit or Infantry outside
the current team, measured from the team's list head in full XYZ. The
candidate's House must consider the team owner allied; this is directional,
not a mutual-alliance test. Active, nonzero-health candidates must be outside
limbo unless the existing object-placement scope is active. No weapon,
visibility, playfield or enemy-target requirement is added to this selection.

Units are considered before infantry. A strictly smaller whole-lepton
distance replaces the winner, using the existing Sqrt_Approx/ftol helper,
not exact squared distance. VERA uses stable ID order within each class.
The selected object becomes the centre; the closest-member field names the
team's head. Without a candidate both fields are cleared, not set to the
ordinary member mean. Other script actions retain their existing mean-centre
behavior.

A friendly with valid active tube state instead contributes its tube exit
cell. The existing tube registry and movement owner remain authoritative;
this action creates no traversal state. A stale/missing tube reference safely
falls back to the friendly object instead of indexing an invalid registry.

The handler stores the centre directly in the team's focus, preserving its
mission target, then calls the existing `Coordinate_Move`. This retains its
ordinary join-up, unload, aggressive-member and navigation policies, including
fallback to the mission target when focus is absent.

After movement coordination, if the centre is an object, live joined members
(aircraft count as joined) discard any different attack target. A stationary,
armed member whose current/effective mission is Guard queues Attack and
receives the followed object's target through the existing class-order
adapter. The handler does not call Commence. Entity and cell attack targets
are both retained. A tube exit is a cell, so the attack-target pass does not
run at all for that destination.

Arrival can finish this script step: the shared `Coordinate_Move` sets the
advance flag when its found/finished/formed conditions hold. The action has no
additional unconditional completion or forced-wait rule. In particular,
"the handler does not explicitly complete" does **not** mean "this action
can never advance."

## Source mapping and adaptations

This implementation is adapted from the author's earlier work in ra2-mono
and contributed under this repository's existing GPLv3. The C# simulation
runtime is not copied; the behavior is connected to VERA's existing owners.

| Earlier ra2-mono implementation | VERA adaptation |
| --- | --- |
| `AIScriptActionWiring.DispatchFollowFriendlies` | Action dispatch, centre/focus update, ordinary member movement and attack-order adapters |
| `TryFindClosestFollowFriendly` | Two-class candidate selection, excluding the current team, full XYZ distance and deterministic tie order |
| `ResolveFollowFriendlyDestination` | Read existing active tube state and map-owned exit cell; no duplicate tunnel implementation |
| `TryResolveFollowFriendlyAttackTarget`, `CanMirrorFollowFriendlyAttack` | Read represented TarCom and reuse locomotor, weapon and mission queries rather than C# task/component types |
| `AIDecisionTraceTests` nearest-friend, target refresh and tube-exit scenes | Asset-independent Rust regression scenes through the production VM and class setters |

The following comparisons refer to the donor snapshot first inspected for
this port. The contributor subsequently corrected the donor's permanent-wait
callback as a separate local change; that later correction is not part of
this Rust PR.

- The original C# progress callback always returned Waiting. VERA retains the shared
  native movement-completion condition instead.
- The donor compares exact squared distance. VERA reuses its native numeric
  length owner, including whole-lepton ties.
- The donor asks alliance from the team owner; the inspected native branch
  asks from the candidate's House.
- The donor's generic busy-task protection and command deduplication are not
  copied over the host's existing mission/navigation policies. Movement runs
  before the separate guarded attack-mirroring pass.

There is no new retained state, RNG stream, timer, serialization field or save
version. Selection is recomputed from existing authoritative entities and
membership; centre, focus, orders and tube state use their current save owners.
Candidate scans use the existing category-column iterator, not a new world
copy or per-team candidate cache. This is not a 20,000-unit performance claim.

The handler itself adds no RNG draw, timer write or detach call. Movement,
mission changes, destination updates and target cleanup run through their
existing owners and retain those owners' documented limitations. In particular,
the shared target setter's incomplete linked-effect, C4 and Ivan cleanup is
not resolved or newly proven by this action.

## Evidence boundary

Instruction-level references: `Calc_Center` action-10 arm
`0x006EAF13..0x006EB1B2`, action handler `0x006EDB50..0x006EDC6B`,
dispatch `0x006E9832..0x006E984A`, directional House query `0x004F9A50`,
and shared movement completion `0x006EBF25..0x006EBF45`.
The contributor inspected their existing saved disassembly from the local
Mental Omega `gamemd.exe` and the Coord magnitude instructions at `0x0041C380`.
This is static instruction-level evidence, not a newly captured retail
executable comparison. The local dump is not added as a repository artifact.

`follow_friendlies_tests.rs` exercises the Rust production paths using
synthetic rules without proprietary assets. These checks do not establish
complete retail/Ares/MO parity, native class-array slot-reuse ordering,
off-map dummy-cell identity or live gameplay acceptance. Tube references use
the host's validated `TubeId` rather than reproducing the original signed-byte
index's truncation for oversized registries. Full donor MO support is not
implied by this one action.
