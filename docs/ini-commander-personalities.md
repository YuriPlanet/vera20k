# Skirmish commander personalities

A *personality* is a named bundle of two things: a tilt on the skirmish AI's own
trigger draw, and a set of short feedback entries that play through the radar
movie window the way a campaign transmission does. The data model is modelled on
Red Alert 3's `AIPersonalityDefinition`
(`Schemas/xsd/SkirmishAI/AssetTypeAIPersonalityDefinition.xsd` in the RA3 MOD
SDK); the clips are RA3's talking heads re-encoded to RA2's Bink format.

This is a product extension. gamemd has neither personalities nor these videos,
so nothing here cites a native address: the only native paths it feeds are the
ones it projects onto (`draw_ai_trigger`, the team timer, `House+0x565C`).

**Assignment is opt-in.** A house with nothing requested keeps `personality =
None`, and every consumer takes its native branch: no trigger is denied or
reweighted, no team delay is scaled, no clip plays. Retail data — which has no
`[AIPersonalities]` at all — is untouched.

## Registry

```ini
[AIPersonalities]
0=Warren
1=Oleg
```

`N=Name` names the `[Name]` sections that are personalities. The index is the
identity a save file stores, so appending is safe and reordering is not. A name
without a section warns and is skipped.

## One personality

```ini
[Warren]
Country=Americans,British,French,Germans,Alliance
DisplayName=Personality:Warren
DisplayDesc=Personality:Warren:Desc

TriggerDeny=0CA2F7C0-G
TriggerWeight.0CA2F7C0-G=150
TriggerRatio=90
TeamDelayPercent=110

Entry0.Key=StartGame
Entry0.Video=AWAGR1
Entry0.To=ALLIES
Entry1.Key=FirstSeen
Entry1.Video=AWATG1
Entry1.To=ENEMIES
Entry1.Timeout=9999999
```

| Key | Value | Default | What it does |
|---|---|---|---|
| `Country=` | `[Countries]` keys, comma or space separated, any case; `*` or `All` for every country | absent = every country | Which countries may be offered this personality, at `rules.ini`'s `Owner=` granularity. Exact key match: no side expansion, no `ParentCountry=` inheritance, so a country a mod adds under its own name just works. `Side=` is not supported: it restated what a `[Countries]` key already says, and a section that still writes it is ignored with a warning rather than silently reinterpreted. |
| `DisplayName=` | CSF key or plain text (256) | none | For a selector. The simulation never reads it. |
| `DisplayDesc=` | CSF key or plain text (1024) | none | One-line description for a selector. Never read by the simulation. |
| `TriggerDeny=` | `[AITriggerTypes]` keys, comma or space separated | empty | Those triggers leave the draw for this house: never built, never counted. |
| `TriggerWeight.<id>=` | percent, `0..65535` | `100` | That trigger weighs `native * percent / 100`. |
| `TriggerRatio=` | `0..100` | `100` | The house's `+0x565C` starting value: how often a team-creation pass picks a trigger at all. Low = patient. A map's `RatioAITriggerTeam=` and trigger op 76 still overwrite it. |
| `TeamDelayPercent=` | `1..1000` | `100` | Scales `[General] TeamDelays=` for this house. Below 100 rushes, above 100 builds up. The `175 * HouseIndex` phase stagger is not scaled, so houses still do not all restart on one frame. |

Out-of-range values warn once and fall back to the default rather than being
silently clamped. `TeamDelayPercent` never produces a delay below one frame,
because a zero-frame restart would let a house create a team every pass.

### `TriggerWeight.<id>` needs the real key

`<id>` is the **section key** in `[AITriggerTypes]`, the hex identity at the
start of the line — `0CA2F7C0-G`, or a bare hex string in data that has no
suffix. It is *not* the trigger's `Name=` (`Allied Anti-Nuke 1`), and not a unit
name.

- The prefix is matched case-insensitively as a whole (`TriggerWeight.`,
  `triggerweight.`, `TRIGGERWEIGHT.` all work), but the dot must be followed
  immediately by the key: a space becomes part of the key and matches nothing.
- A key that matches no loaded trigger does nothing. Put the trigger's `Name=`
  in a comment on the same line — that is what the comment is for.
- `5000` is the selector's "clear the candidate list" sentinel. It marks
  precedence rather than magnitude, so no percentage is applied to it.
- `0` is a soft disable: the trigger stays in the table but never wins. Use
  `TriggerDeny=` to remove it outright.

### Feedback entries

`Entry<N>.*` is one clip request, grouped by `N` and read in ascending order.

| Key | Value | Default |
|---|---|---|
| `EntryN.Key=` | one of the six events below | required — an entry without it cannot fire |
| `EntryN.Video=` | a `[Videos]` key, e.g. `AWAGR1` | none (the entry cannot play anything) |
| `EntryN.To=` | `ALLIES`, `ENEMIES`, `NEUTRAL`, combined with commas or spaces | `ALL` |
| `EntryN.Timeout=` | seconds before the same entry may fire again; a negative value or absent means no limit | no limit |

`EntryN.Text=` is not read yet: the clip's line comes from the movie subtitle
table (`subtitlemd.txt`, `BeginMovie <clip>` … `EndMovie`), exactly like a
campaign transmission.

## The six moments

| Event | Fires when | Spoken by |
|---|---|---|
| `StartGame` | the match begins | a friend |
| `InTrouble` | the house's building count falls below its own high-water mark by `TroubleBuildingsLost=` | a friend |
| `FirstSeen` | a unit, structure or aircraft of the house first stands in a cell the local player can see | an opponent |
| `DefeatedLocalPlayer` | this house defeated the local player | an opponent |
| `DefeatedByLocalPlayer` | the local player destroyed this house | whoever lost |
| `Lost` | this house was eliminated by anyone else | whoever lost |

Two of the six are a friend's — the opening greeting and the call for help —
and two are an opponent's. The two defeat lines belong to whoever lost, so both
sides may say them.

That split is a gate, not a preference: a moment whose relation does not match
is not played at all, whatever the data says. `EntryN.To=` is a second, per-line
filter on top of it, so a mis-filed entry can only ever cost a line. **Nothing is
borrowed across the two sides** — an opponent with no `StartGame` line for its
relation stays silent, because an enemy greeting you in a friend's voice, or
begging you for help, reads as a bug.

## Switches

```ini
[CommanderFeedback]
Enabled=yes
Personalities=yes
TroubleBuildingsLost=1
TroubleQuiet=180
Assign.Computer1=Warren
```

| Key | Default | Effect |
|---|---|---|
| `Enabled=` | `yes` | `no` plays no clips. Every computer house keeps its personality and therefore its `TriggerDeny=` / `TriggerWeight.` / pacing tilt. |
| `Personalities=` | `yes` | `no` gives no house a personality at all: retail's AI, weights untouched, no clips. This is what a map with preset AI (tower defence, a scripted mission) wants. |
| `TroubleBuildingsLost=` | `1` | Buildings a house must lose from its own high-water mark before `InTrouble` fires. Minimum 1. |
| `TroubleQuiet=` | `180` | Seconds of silence after an `InTrouble` clip whose entry names no `Timeout=`. |
| `Assign.<house>=` | none | Names a personality for one launch slot — `Computer1`…`ComputerN`, the same names the startup log prints. The test seam until a selector exists; a slot's own `commander` field outranks it. |

The player also has `[VERA] CommanderFeedback=` in their profile (default on),
which switches the clips off without touching the data. It is presentation only:
the personality and its strategy tilt stay.

Ways to say "no commander" for one house: `Assign.Computer2=none` (also `off` or
`silent`), or the reserved personality `Standard` — no clips, no tilt, retail's
AI. `Standard` is never handed out by a draw; it exists to be named.

## Verifying

Startup, one line per rules load:

```
AIPersonalities: 6 loaded (6 with Country=, 0 any country), 0 with strategy entries
```

`0 with strategy entries` means no personality writes `TriggerDeny=` /
`TriggerWeight.` / a non-default `TriggerRatio=` or `TeamDelayPercent=`: the
strategy half is present but neutral, and the AI draw is native.

Per match, once the scenario is built:

```
Commander feedback active: 6 personalities, 3 computer houses with a commander; local=Americans; speakers: Computer1 (Warren, ally), Computer2 (Oleg, enemy) …
```

`0 computer houses with a commander` with `Personalities=yes` means nothing
requested one — which is the default state, not an error. Add an `Assign.` line.

Each clip that plays logs its own line:

```
Commander feedback: AWAGR1.BIK for Computer1 (Warren, ally, StartGame)
```

## What this does not do

- No selector UI yet. `NormalizedSkirmishSlot::commander` and
  `PersonalityCatalog::candidates_for` are the seams: the first takes a name, the
  second lists what a given country may be offered.
- `Country=` is a filter on what a draw may offer, not a restriction on explicit
  choices: a name requested by a map, a test or a future selector is assigned
  even when the country is not listed, with a warning.
- Screens and save files carry the personality as its registry index. Inserting a
  personality in the middle of `[AIPersonalities]` renumbers the rest.
