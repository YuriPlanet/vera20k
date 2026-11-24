# Rust bots

*(VERA)* A bot is a computer player written in Rust (`src/sim/bot/`). It plays
a House through the same commands a player sends, so it needs no network
traffic and stays in lockstep: every peer runs it once per frame, right after
the Houses' own AI.

- **Fair.** The House stays human for every rule: no computer starting money,
  build speed, armor or rate-of-fire bonus. The bot reads only `BotView`: its
  own objects, the rules, and other Houses' objects where its House has
  explored (fog of war, cloak and disguise apply). Anything more is a declared
  `Cheat`, named in the match's chat. Vanguard declares none.
- **Deterministic.** Decisions use no clocks, no `HashMap` iteration and no
  floating point; random choices come from the bot's own seeded generator,
  saved with the match.
- **Off by default.** Stock AI rows are unchanged. The Skirmish page's AI type
  list has a **Vanguard** row; that slot starts as a Normal computer House and
  the bot takes it before the computer money is granted.

Adding a bot: implement `Bot` for a `Serialize + Deserialize + Clone` type, add
a `BotBrain` variant and a `BOTS` row. Chores every bot shares (repairs, idle
miners, crates, tech buildings, garrisons, keeping out of enemy reach) live in
`chores.rs`.

Not yet: bot rows in LAN/online rooms (the setup wire carries only the stock
difficulties), and control changes during a match.

## Headless matches

`sim-bench --scenario skirmish` plays a bot against the stock AI without a
window:

    sim-bench --ra2-dir <game> --map Tower.mmx --scenario skirmish --computers 2 \
      --bot vanguard --ai hard --countries 8,2 --client-timing --ticks 162000 --seed 1

`--countries` takes indices into America..Yuri (0..9) for the computer rows in
order; the first row is the bot. `--crates` turns the Crates option on. The run
prints each defeat, stops when one House is left, and prints every House's
worth at the end.
