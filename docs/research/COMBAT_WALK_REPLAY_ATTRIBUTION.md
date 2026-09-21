# Paid Walk replay attribution — 2026-09-21

The Slice6 replay hash changes because its E1 now executes the original paid
Walk direction/step instead of a normalized vector. This is a Rust regression
receipt, not a whole-scenario native golden. No hash or snapshot layout changed.

## Comparison

Baseline `ec27dc26` was built in an isolated checkout. The same temporary probe
serialized every actor and recorded all three RNG states after each of the
16 production frames in `replay_hash_stable_through_slice6`. The candidate used
the same probe. Both executions also replayed all commands through ReplayRunner
and compared each recorded frame hash.

Frames 1–11 match. On frames 12–16, both tanks and all RNG streams still match;
only E1 differs in these fields:

| Field | Baseline at frame 16 | Paid Walk at frame 16 | Cause |
| --- | --- | --- | --- |
| `position.sub_y` | 88 | 93 | Native heading/table displacement pays seven northward leptons per step, not eight. |
| `body_facing.start_frame` | 10 | 15 | Original Walk calls FacingClass snap on each paid visit. |
| `foot_speed.cached_current_speed` | 0 | 10 | Publish the actual Foot speed used by the paid step. |
| `movement_target.move_dir_x` (fixed bits) | 19136512 | 16777216 | The legacy normalized vector is no longer refreshed or read by Walk. |
| `movement_target.move_dir_y` (fixed bits) | -18874368 | -16777216 | Same retired Walk calculation; other locomotors still use these adapter fields. |
| `movement_target.move_dir_len` (fixed bits) | 26878390 | 23726522 | Same retired Walk calculation. |

Replacing only E1's final candidate state with its serialized baseline state
reproduces **both** old hashes exactly:

| Projection | Baseline | Candidate | Candidate with baseline E1 |
| --- | --- | --- | --- |
| Current | `90DA2A8E0C06D5E3` | `3D7FB762F752444A` | `90DA2A8E0C06D5E3` |
| Before schema 174 | `221E77F911A4FB24` | `8C50893ACFC759D6` | `221E77F911A4FB24` |

Final Scenario/Main/Mapgen states are respectively `9AEAD238C8AEC588`,
`4CB6FE1CCB4547FF`, `1CE8184870436163` in both builds. Local raw receipts are
`.local/walk-baseline-frames.json` and `.local/walk-replay-diffs.json`; build
logs are `%TEMP%/vera20k-walk-baseline.log` and
`%TEMP%/vera20k-walk-candidate-probe.log`. Temporary dump/normalization code
is removed from the final test.

## Native comparison and retained regression

`tools/spatial_oracle/walk_paid_step.py` now carries five native steps starting
at the production frame-12 input: XYZ `(1408,1408,0)`, accepted head
`(1728,1088,0)`, supplied integer speed 10 and retained heading 8315.
Each native output becomes the next native input. Original instructions produce
`(1415,1401,0)`, `(1422,1394,0)`, `(1429,1387,0)`, `(1436,1380,0)` and
`(1443,1373,0)`, with heading 8315 throughout. The production replay now asserts
these coordinates and headings on each corresponding frame before checking
the updated hashes. The preceding 35 native vectors are unchanged.

The original constructor, coordinate getters, FacingClass callback, angle and
trigonometric routines execute. The Infantry movement-speed receiver remains
supplied, and the corpus ends before physical placement. Admission, full Foot
getter inputs and all combat behavior are not certified by this comparison.
The separate exhaustive lookup corpus covers all 65,536 direction words;
the full-frame movement/fire test covers save/restore during the retained step.
