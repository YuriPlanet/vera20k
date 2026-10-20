# Live cell levels and bridge flags in tactical picking

`python -m tools.bridge_click_state_oracle --check` executes 104 calls to the
original retail Tactical inverse `0x006D6590` using the same fixture and binary
verification as `tools/bridge_click_oracle.py`. `vectors.meta.json` records the
binary hash and assumptions. `--write` explicitly regenerates the references.

The original caller resolves each scan candidate through `Get_CellClass`
`0x006D674C → 0x005657A0`. It reads the returned cell's live flag word at
`0x006D6760`, tests structural `0x100` at `0x006D6771`, then resolves cardinal
neighbors through `0x00481810`. Candidate orientation is read again at
`0x006D6793`, `0x006D67C1`, and `0x006D67E9`; the `0x800` bit chooses the edge
branch. Neighbor structural tests include `0x006D67A7`, `0x006D67D5`,
`0x006D6800`, and `0x006D681A`. The later 60-pixel adjustment reads the current
candidate flag again at `0x006D6948` or `0x006D69A3`. These are current CellClass
reads, with no initial-map bridge projection between them.

This comparison supplies flat level-2 terrain and intact, collapsed, then
repaired flag values on the same four CellClass addresses for each orientation.
The original inverse, lookup, neighbor, projection, and numeric conversion bodies
execute unchanged. At world pixel `(0,420)`, the native result changes from
`(16,16)` to `(14,14)` when structural flags clear, then returns after repair.
These are executed outputs, not geometric expectations calculated in Rust.

Flag publication, the damage driver, and engineer repair do not execute in this
fixture. The separate `tools.spatial_oracle.bridge_body_publication` comparison
covers the original setter; this fixture only establishes the downstream reader.
The existing larger bridge-click comparison covers 1,782 candidate cases and
572 full inverse calls. All lookups here must resolve mapped real cells. Dummy,
negative/off-map fallback, window events, rendering, and movement remain outside
this comparison.

`src/app/input/bridge_live_click_tests.rs` consumes these outputs through the app's
shared click owner after the existing runtime flag setter and after a real
`PreparedLoad` transaction against an intact map template. This is a bounded
state-authority regression; it does not certify the whole bridge mechanism.

The additional 60 queries retain the same native cell identities and overwrite
candidate, east-neighbor and south-neighbor `CellClass+0x11B` levels between 2
and 6, then restore them. Original `0x006D674C` resolves the cell, and
`0x006D6753` sign-extends its current level byte. Both bridge orientations are
covered. A candidate-level change alone changes the `(0,480)` result from
`(17,16)` to `(16,16)` in direction 0. The supplied cell bytes are unchanged
by these queries. Publication, gameplay RNG/timers/detach effects, and stock
occurrence are outside these reader controls.

The app regression also saves changed levels and flags through `PreparedLoad`
and resolves against the restored live world while its load template retains
the initial state. Candidate and neighbor levels and flags now share the
borrowed `TacticalCellLookup`; no app height/deck map survives load or restore.

Four additional upstream controls supply coherent EW bridge cells and deck-height
centre/infantry right-subcell anchors for the existing click/shroud suffix fixtures. Four more
queries consume the native full-body publication snapshots retained in
`tools/spatial_oracle/bridge_middle_tiles.json`, on its C3 map inputs. The NS
pixel `(1260,2085)` changes from `(92,50)` to the ordinary flat fallback `(90,48)`
after both neighboring spans collapse and the native level4-to0 publication.
These reader controls do not execute damage admission,
publication, the native loader or a whole pointer-message chain.
