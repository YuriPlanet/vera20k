# Initial House sight refresh

`python -m tools.spatial_oracle.house_initial_sight --check` compares ten
bounded original controls. Set `VERA20K_GAMEMD_EXE` or `RA2_DIR` to the owned
retail executable. `--write` deliberately regenerates the corpus.

Original House constructor `4F57A7` sets byte `+24B`. The local House's late
Update block `4F9338..4F935F` clears it before calling `Map4ADCD0(0,0)`.
The earlier `Fill_In_Data684F89` caller passes `(1,0)`, skipping Buildings
without consuming the House flag. Original instructions establish these
call sites; the controls execute the actual constructor store, late block,
map visitor and shared Techno sight methods.

Each control performs two late House visits. Cases cover the current-player
gate, an already clear flag, undiscovered and limbo sources, playfield and
passive-owner gates, retained camera sight, and a previously unlatched
Sight6 source. Two histories first execute the mobile-only startup visitor:
the supplied Aircraft identity is visited; the supplied Building identity is
skipped until the House update. The prior camera cache is at `(40,40)` and
the source now stands at `(41,40)`.

These are supplied object/House/type priors, not complete constructors or a
native scenario launch. Virtual RTTI/type getters return fixture identities
(Aircraft2, as returned by original41C180, and Building6). Original executable
sections and both Main/Scenario RNG buffers must remain unchanged.
Map area and border receivers record arguments without traversing native
cells. The Sight6 row supplies zero elevation/veterancy and
`LeptonsPerSightIncrease=2000`; this adds no rules-default claim. Existing
allocation/CRT and anger hooks are installed but unused by this path. No
whole-frame pixel or complete skirmish parity is claimed.

Rust comparison `initial_house_sight_matches_original_update_tail` exercises
the actual post-map dispatcher and House rung with production rules readers,
spawn, map cells and shared sight owner. It compares area/border calls,
border coordinates/radius, source cache/latch and House pending state.
`initial_house_sight_pending_survives_snapshot_and_current_house_change`
covers pending/consumed saves, hash sensitivity, locality and repeat visits.
The snapshot layout is derived automatically from the saved types.

The two callers introduce no RNG draws, timers or detach work of their own.
Their synchronous sight/gap/discovery callbacks retain their existing owners
and may execute authored effects. Existing shared-visitor residuals for
per-viewer allied admission and whole-map FogOfWar remain separate; this
caller integration does not certify those mechanisms.
