# Ordinary mobile first-reveal admission

`shroud.py --entity-reveal` executes the original Unit, Infantry and Aircraft
DrawIfVisible entries (vtables `7F5C70`/`7EB058`/`7E22A4`, `+104` entries
`73B0B0`/`5F4B10`) up to their DrawIt bodies `73CEC0`/`518F90`/`4144B0`.

```sh
python -B -m tools.procedural_drawing_oracle.shroud --entity-reveal --check
```

All nine class and anchor contrasts (`Cell+12C` = 0/08/18) reach DrawIt without
reading the anchor cell. Redraw-not-ready, limbo and off-camera controls refuse.
Dead and unmarked retained members still enter, because the wrapper does not own
Display cleanup. `shroud_pop_in_mobile_admission_matches_original_class_entries`
in `src/app/presentation/instances/helpers.rs` reads this corpus. Raster output
and special tint are not covered; Infantry calls `487950` at `5190D1` and clears
its special tint at `5190DA`.
