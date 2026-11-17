# A human 1v29 win, read back as a reference

Source: `autoplay/reference/replay_tick214095.json` — `rmap38631.yrm`, seed
367877234, 1 human against 29 computer houses on one team. Read with
`autoplay-replay` (see `autoplay/reference/README.md`):

- 214,096 ticks at 45 Hz = **4757.7 s** (79 min) of game time;
- **714 recorded state hashes, all matched**, final `0x6c04a6c7afb21e35` — the
  numbers below are the match that was played, not a reconstruction;
- the player won: all 29 computer houses were destroyed, the last at tick
  214,006.

Everything here is *one game on one map*. It is a reference to compare against,
not a rule to copy: the value is that the numbers are in the same units the
policy is judged in.

## The numbers

| | Player (human) | The 29 computers |
|---|---|---|
| Credits | 100,000 → 349,603 | 2,611 … 160,489 each at the end |
| Income (earned) | **942,440** | 69,243 … 234,445 spent each |
| Spent | 592,837 | ~3.2M combined |
| Peak army value | **309,667** (tick 199,126) | ~217,140 combined (tick 750) |
| Final army value | 292,917 | 1,430 (one house) |
| Objects lost | **322** | **~13,435** |
| Score | 3,165,821 | — |

Exchange: about **41 enemy objects for each one lost**. The v4 autoplay baseline
on the same map and options (`831fd2604`: seed 24301, 36,000 ticks) reads 55
deaths, 4 enemy units killed, exchange **0.07**, and ends with
`building=2 yard=0 fighters=11`.

## What the win was made of

### 1. The economy came first, and the army followed

- First attack-move order: tick **78,075** (28:55). Before that the human only
  defended — the first contact was an IFV killing a rifleman at (30,201) at
  tick 20,100.
- The army value crosses the whole field's at about tick **92,000** (34 min) and
  never looks back: 179,400 vs 190,095 at 89,775, 207,200 vs 156,560 by
  94,275, 267,152 vs 116,995 at 3600 s, 309,667 vs 58,655 at 4425 s.
- Income arrives in lumps (captured refineries, crates, sold yards): visible
  jumps at 23.6 s (99k → 163k), 445.2 s (53k → 132k), 733.3 s (132k → 275k),
  1372.5 s (275k → 391k), 2257.6 s (176k → 353k), 3345.6 s (117k → 376k).

### 2. A narrow, tall army

Damage credited by attacker type, of 3,141,956 credits in total (26 types):

| attacker | damage value | share |
|---|---|---|
| SREF (Prism Tank) | 1,139,968 | 36.3% |
| MGTK (Mirage Tank) | 723,624 | 23.0% |
| JUMPJET (Rocketeer) | 389,561 | 12.4% |
| NABNKR | 287,058 | 9.1% |
| FV, DISK, APOC, ATESLA | 368,778 | 11.7% |
| everything else | 232,967 | 7.4% |

Three types carry **71.7%** of the damage; four carry 80.8%. The rest of the
roster exists to screen, capture and repair, not to deal damage.

### 3. Captured tech, not just built tech

The roster contains types the player could not have built from their own yard:
APOC (Apocalypse), YTNK (Yuri Tank), TELE (Tesla Tank), MIND, DISK, HTNK, plus
civilian structures (CAEUR04, CATEXS05, CATEXS06) and three different
construction yards (NACNST, GACNST, YACNST) that were later sold. Capturing an
enemy base and using it is a force multiplier the build-only policy does not
have.

### 4. Cheap units died; the core did not

322 losses, and the pairs say which: 20 SLAV (no attacker), 17 JUMPJET ← HTK,
14 ENGINEER ← SNIPE, 12 SPY ← ROBO, 11 FV ← DRON, 11 FV ← ROBO, 10 GGI ← E1,
9 SLAV ← SMIN. Of the 322, **283 were worn down and only 39 died at full
strength** — the line absorbed damage instead of being deleted by one-shot
attacks. The expensive core (SREF/MGTK) barely appears in the loss table.

### 5. Selling what cannot be held

Four sales, all of captured or forward yards: NACNST at 60,703, NARADR at
72,430, GACNST at 110,353, YACNST at 135,861. Converting a building that
cannot be defended into cash is a decision the current policy never makes.

## What this says about the current policy

| | v4 baseline | reference |
|---|---|---|
| Deaths | 55 | 322 |
| Enemy objects destroyed | 4 | ~13,435 |
| Exchange | 0.07 | ~41 |
| End state | 2 buildings, 0 yards, 11 fighters | 65 structures, 185 units, 292,917 army value |
| Result | the base is rolled at ~36k ticks | victory at 214k |

The gap is **not** target choice or micro. The baseline loses because it is
broke and small: 36k ticks in, it has no yard and 11 fighters. The reference
spends 593k and ends with 65 structures and a 300k army.

## What to build next, in order

1. **Measure the same units on the headless run.** `autoplay-match` already
   prints credits, `spent`, `earned`, army counts and the death ledger. The
   first question is not "why did it lose the fight" but "why was it at 2
   buildings and 11 fighters at 36k ticks" — the economy curve is the
   independent variable.
2. **A bank target, not a push target.** The reference's army crosses the
   field at ~34 min, after the income lumps at 23.6 s and 445.2 s. A policy
   that attacks when it is *outnumbered* attacks too early; a policy that
   attacks when its *income* is comfortably above its spend rate has the better
   trigger.
3. **Narrow the composition.** The reference's 72% comes from three types.
   The role weights currently spread damage across everything the house can
   build; the reference suggests a small set of high-damage types with cheap
   screens.
4. **Engineers and captures.** The reference's roster is full of captured
tech; the policy has an engineer path but no capture *intent*.
5. **Selling.** A captured or forward yard that cannot be held is cash.

## The second reference: a superweapon win, read from the log

`autoplay/reference/replay_tick151530.json` has **no startup**, so it cannot be
replayed and its numbers cannot be recovered. Its log still carries the one
thing that matters: every superweapon either side launched.

| | Player | The 29 computers |
|---|---|---|
| Nukes (`NukeSpecial`) | **102** | — |
| Lightning Storms (`LightningStormSpecial`) | **72** | — |
| Psychic Dominators | 8 | — |
| Psychic Reveals | 2 | — |
| Iron Curtains | — | 58 |
| Force Shields | — | 35 |

Match length: 19:42:17Z → 20:43:08Z (56 min); the player's first superweapon
is a Psychic Reveal at 19:46:45 (4.5 min in) and the last is a Psychic
Dominator at 20:43:06, two seconds before the verdict.

Two things follow:

1. **The computers used superweapons defensively** — Iron Curtain and Force
   Shield only, 93 of them, none offensive. The player used 184, all but two
   offensive, and rebuilt them continuously under a limit of 20.
2. **Three factions' superweapons at once** (Allied Storm, Soviet Nuke, Yuri
   Dominator) means captured construction yards, not a single national tree —
   the same captured-tech pattern as the 1v29 win.

That is the second half of the reference: on the largest map, against 29 brutal
free-for-all houses, the win came from *spending* the enemy's own tech back at
them, not from out-building them.

## The build order, exactly

862 production clicks and 68 placements, decoded from the native records with
the rules' own type arrays. The opening:

| tick | time | what |
|---|---|---|
| 374 | 0:08 | power plant (`GAPOWR`) |
| 980 | 0:22 | placed at (22,198) |
| 1178 | 0:26 | barracks (`GAPILE`) |
| 1585 | 0:35 | placed at (25,202) |
| 1795 | 0:40 | refinery (`GAREFN`) |
| 2090 | 0:46 | 5 attack dogs |
| 2216 | 0:49 | 10 GI |
| 2321 | 0:52 | 10 Guardian GI |
| 3246 | 1:12 | second refinery placed |
| 3614 | 1:20 | second power plant |
| 4139 | 1:32 | 10 more GI |
| 4657 | 1:44 | airfield (`AMRADR`) |
| 5414 | 2:00 | placed at (20,208) |
| 5538 | 2:03 | war factory (`GAWEAP`) |
| 7063 | 2:37 | service depot (`GADEPT`) |
| 7759 | 2:52 | battle lab (`GATECH`) |
| 8070 | 2:59 | first Rocketeer (`JUMPJET`) |
| 8261 | 3:04 | first IFV (`FV`) |
| 9296 | 3:27 | spy satellite (`GASPYSAT`) |
| 10911 | 4:02 | second refinery complex (`GAOREP`) |
| 12340 | 4:34 | first chrono miner (`CMIN`) |
| 13125 | 4:52 | first Tesla tank (`ATESLA`) — captured tech |
| 17736 | 6:34 | first Mirage tank (`MGTK`) — the damage core |
| 18186 | 6:44 | first Spy (`SPY`) |
| 29978 | 11:06 | robot control center (`GAROBO`) |
| 37405 | 13:52 | first Engineer (`ENGINEER`) |

Every type queued, with its first tick:

| type | queued | first | | type | queued | first |
|---|---|---|---|---|---|---|
| JUMPJET | 102 | 2:59 | | MGTK | 50 | 6:34 |
| SREF | 95 | 16:30 | | SPY | 70 | 6:44 |
| FV | 85 | 3:04 | | GAROBO | 1 | 11:06 |
| APOC | 80 | 28:00 | | ENGINEER | 35 | 13:52 |
| DISK | 60 | 57:45 | | YTNK | 25 | 66:15 |
| E1 | 55 | 0:49 | | MTNK | 25 | 18:30 |
| GGI | 10 | 0:52 | | TELE | 20 | 72:30 |

The shape: **mass infantry first** (dogs, GI, Guardian GI), an **airfield and
war factory by 2:03**, the **battle lab by 2:52**, and the damage core
(Rocketeers, Mirage, Prism) online inside 17 minutes. Everything after that is
production, not decisions: the queue never stops.

## The third reference: 1v95, same playbook

`autoplay/reference/replay_tick211801.json` — `match96.yrm` (96 start points),
seed 431,813,468, 1 human against 95 computers, 211,801 ticks = **78.4 min** of
game time, and a win (log: `Match end ready for local player 'HaoJun0823':
你胜利了！` at 13:48:10Z). Options: 100,000 credits, game speed 1, **no
superweapons**, tech 10.

It cannot be replayed here: the v4 loader caps a launch at 30 slots
(`skirmish_launch::NETWORK_PLAYER_SLOT_COUNT`), so `SkirmishLaunchSession::parse`
refuses 95 opponents before any simulation. The command stream is still a
complete record, and every local command is a native 111-byte record, so it was
decoded directly:

```
python autoplay/reference/decode_records.py replay_tick211801.json type_table.json
```

with `type_table.json` dumped from the retail rules by
`autoplay-replay --dump-type-table`.

**1,149 production clicks, 88 buildings placed.** The opening is the 1v29
opening again:

| tick | time | what |
|---|---|---|
| 88 | 0:02 | power plant, placed (44,396) |
| 622 | 0:14 | barracks, placed (44,400) |
| 965 | 0:21 | 6 engineers |
| 1045 | 0:23 | refinery, placed (44,404) |
| 2474 | 0:55 | war factory, placed (35,395) |
| 3793 | 1:24 | 5 IFVs |

Then the same shape at 95-player scale:

| type | queued | first | | type | queued | first |
|---|---|---|---|---|---|---|
| APOC | **341** | 12:34 | | MGTK | 20 | 5:52 |
| SREF | 205 | 9:26 | | E2 | 90 | 2:13 |
| SPY | 185 | 4:47 | | FV | 30 | 1:24 |
| JUMPJET | 95 | 4:29 | | E1 | 20 | 1:41 |

And the same captured tech, more of it: 8 Soviet war factories, 4 Yuri war
factories, 4 nuclear reactors, 3 Soviet barracks, 27 Soviet battle bunkers, 13
French Grand Cannons.

Two lessons this adds:

1. **The playbook does not depend on superweapons.** The 1v29 needed them; the
   1v95 won with `super_weapons: false` and pure armour mass (341 Apocalypses).
2. **Capture scales with the map.** On a 96-start map the captured buildings
   are not a bonus, they are the production base: two war factories beyond the
   player's own, plus a third faction's barracks.

## What this reference cannot tell us

- One game, one map, one country, one seed. It says what *a* win looks like,
  not what always wins.
- The player's build order is recovered from sampled rosters (every 15 s) plus
  the command stream; the exact production ticks are in the JSON report, but
  the engine keeps production and placement as raw native records, so the
  report's `queue`/`place` rows are decoded from those records and the roster
  table is the cross-check.
- The second reference (`replay_tick151530`, a superweapon win on the largest
  map against 29 brutal free-for-all houses) has **no startup** and cannot be
  replayed; it stands only as proof that the same playbook won a second,
very different match.