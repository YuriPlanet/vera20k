# V55 — Tactical Efficiency (skirmish test candidate)

This is the cumulative V53 regression recovery and V54 cohesion candidate,
with a bounded final tactical integration pass. No campaign harness work.
No V53/V54/V55 gameplay result is available.

## V55 changes relative to V54
- Micro considers an existing attack worth preserving only when the target
  is visible, damageable, not invulnerable, and in actual native firing
  range. A nearby ready economic-threat/controller contact may interrupt
  a building attack. Native ready shots outrank merely nominal pursuit
  candidates within the same self-defence priority. Direct current
  self-defence still comes first; neither ammo nor rearm timers are changed.
- Friendly concentration is inferred from available units with a real
  in-range target, not just units holding an Attack target. Existing
  special target preferences and native weapon admission remain.
- The V54 supported-battery shot criterion is reused by micro and reload
  kiting; ordinary kiting no longer interrupts such a supported shot.
  Parasite/blast/controller emergencies and final recovery/safety are
  separate owners. This is actual in-range capability, not proof allies
  have fired, nor a new pursuit exemption.
- A new successful Attack or firing-post Move proposal from the battery
  commander is treated as work in the current frame. A Stop is not.
- Target selection uses a real actor near the median main-ground-fist
  position when the previous first actor is more than 12 cells from it.
  Keeps the first actor for small or already coherent groups.
- Advance and assault preparation use the current director's observed
  threat classification instead of an additional "any enemy near base"
  gate. Actual home/miner raids, loss recovery and force comparison remain.
- Restrict blackout micro exemption to actual available Attack-squad
  members near the operation instead of all nearby fighting units.
- Counter procurement ranks only legal, affordable, currently idle-line
  candidates, excluding products already in production. Preserve stock
  caps; sufficient standoff stock suppresses further discretionary
  counters, and sufficient air counters suppress fallback heavy purchases.
  Recheck all costs/permissions through the existing shared budget.
- An unavailable land-vehicle production line no longer justifies the
  ordinary infantry-share saving gate. Essential recovery, actual budget,
  opening mobilization and specialist admission still apply.

## Validation
The final source snapshot passes the repository's Python group_tests
check and 35 Python regression tests. The fixed manifest, runner and
formal acceptance tool are unchanged. All patch application checks use
exact affected-file predecessor snapshots. Rust compiler and live game
assets are not available here; Rust tests and actual matches remain
unexecuted. The new risk calibrations, contact rules, and median-anchor
threshold are proposed changes, not proven improvements.

See the packaged Chinese handoff for the fixed test protocol, historical
regression evidence, and requirements for next-round diagnosis.
