# VERA's own computer player

A VERA feature, off by default. With it off, every computer house plays the
way `gamemd.exe` plays. With it on, the computer houses play by the numbers
a `[VeraAI]` rules section holds.

```ini
[VeraAI]
Enabled=yes           ; default no
MaxHouses=0           ; default 0: every computer house
DecisionPeriod=30     ; default 30 frames between decisions
AttackThreshold=6     ; default 6 combat objects before the army leaves
HomeGuard=2           ; default 2 combat objects stay at the base
AttackWaves=4         ; default 4 squads may be on the attack at once
InfantryWeight=1.0    ; default 1.0
VehicleWeight=1.0     ; default 1.0
AircraftWeight=0.25   ; default 0.25
InfantryPeriod=90     ; default 90 frames between production requests
VehiclePeriod=150     ; default 150
AircraftPeriod=300    ; default 300
TargetProduction=yes  ; default yes
OpeningDelay=300      ; default 300 frames before the first attack
```

A mod's `rulesmd.ini`, a map, or a scenario may carry the section. In an
online game every player needs the same rules, which the rules compatibility
check compares.

## What it changes

The stock computer player is the rule of the game: it is a faithful port of
gamemd's player (`sim::house_strategy`, `sim::ai_base_building`) and it
builds its base, picks a unit and keeps a crude queue. VERA's own player
sits above that queue and decides what the army does. It never changes a
rule of the game: it issues the same orders a player issues, through the
same owners (`Simulation::apply_megamission_command`), and it reads the same
rules the stock player reads.

Every computer house the section takes:

- **Picks its enemy.** It targets the nearest living, non-passive opponent
  with a base center and skips allied houses.
- **Splits its army into eight squads.** A combat object's squad is its
  stable id modulo eight, so a squad keeps its members for as long as they
  live without the player remembering who joined it. The first
  `AttackWaves` squads go out to attack; the rest hold the base.
- **Waits for the army.** No squad attacks until the house holds
  `AttackThreshold` combat objects. The weakest `HomeGuard` objects never
  leave; they guard the base.
- **Strikes what is worth most.** A base unit comes first — a house that
  holds no base unit and no unit is defeated — then a production building,
  then a defense.
- **Supplies production choices.** When the stock chooser leaves a category
  empty, VERA cycles through buildable armed units on that category's cadence.
  The computer factory builds and deploys them. `TargetProduction=no` leaves
  all production choices to the stock player.

The policy commands living, deployed, armed units that are available for
retasking. Harvesters, units in teams and units in radio contact keep their
current duties.

## The keys

| Key | Default | What it does |
| --- | --- | --- |
| `Enabled` | no | Whether VERA's own player plays the computer houses. |
| `MaxHouses` | 0 | How many computer houses it takes, in house order. Zero takes every one; one takes the first, which is what a match between a VERA house and a stock house wants. |
| `DecisionPeriod` | 30 | Frames between decisions. |
| `AttackThreshold` | 6 | Combat objects the house must hold before its army leaves. |
| `HomeGuard` | 2 | Combat objects that hold the base. |
| `AttackWaves` | 4 | Squads that may be on the attack at once, at most eight. |
| `InfantryWeight` | 1.0 | A positive value enables infantry choices from this policy. |
| `VehicleWeight` | 1.0 | A positive value enables land vehicle choices from this policy. |
| `AircraftWeight` | 0.25 | A positive value enables aircraft choices from this policy. |
| `InfantryPeriod` | 90 | Frames between infantry production requests. |
| `VehiclePeriod` | 150 | Frames between vehicle production requests. |
| `AircraftPeriod` | 300 | Frames between aircraft production requests. |
| `TargetProduction` | yes | Whether the player queues production over the stock player's queue. |
| `OpeningDelay` | 300 | Frames before the first attack, so the base is standing before the army leaves. |

A number outside its range is clamped, never refused, so a bad value cannot
stop the game. The values come from the reader's own defaults; no retail INI
holds a `[VeraAI]` section.

## Training it

The player is a small set of numbers, so a trainer may move them and measure
the result. Two programs do that, both in the core crate:

```text
ai-train --maps NewHghts.mmx,Round.mmx --seeds 2 --generations 20 --out best.json
ai-eval  --map NewHghts.mmx --seeds 4 --policy best.json
```

`ai-train` plays the same matches for every candidate — the maps and seeds
on its command line — through `headless_scenario::ai_match`, the same core
the game plays, and keeps the candidates that score better. The score of a
match is `+1` for a win, `-1` for a loss, and, for an undecided match,
half the difference in remaining house value divided by the combined value. `ai-eval`
measures one policy the same way, so a number the trainer found means the
same thing when it is read back.

The search is a (μ/μ, λ)-evolution strategy with rank weights: every
generation samples a population around the current mean, keeps the better
half and moves the mean onto it. Each knob is scaled into `[0, 1]` before
the search, so one step size fits them all, and the step size grows when a
generation brings no improvement and shrinks when it does.

`ai-train --baseline` also measures the stock computer player on the same
matches so you can compare it with a trained policy.
