# AnimTypes without images

`AnimTypeClass::ReadINI` 0x427D00 succeeds when ART is present but the named
SHP lookups return NULL. `LoadImageAndResolveFrameBounds` 0x427B50 and
`GetImage` 0x428C30 retain that absence. End and LoopEnd default to zero;
explicit values, including lazy -1, survive. Image absence does not reject
the animation identity or its Start/Middle effects.

`imageless_anim.py` declares three bounded synthetic controls and reuses
`anim_theater_flags.run_case` for the original constructor, full ART reader,
theater postpass and GetImage. The corpus saves actual default and mixed
explicit/lazy bounds. The existing 26 theater controls are retained unchanged.

Replay: `python -m tools.rules_oracle.imageless_anim --check`.
Use `VERA20K_GAMEMD_EXE` as described in [native setup](../native_oracle.md).
Transport is limited to the existing heap/INI cache/file lookup services;
code and vtable guards remain in that shared reader. No whole-world particle,
archive-I/O, malformed-SHP or pixel parity is claimed.

The single `ArtRegistry::bind_one_anim_asset` owner records a successful
null-image binding as zero frames and zero height, while preserving explicit
bounds. An unread ART body remains distinct and retains constructor metadata.
The simulation's existing effective-bound resolution, Start/Middle, particle
producer, AI and retirement own the remaining behavior. Both strict scenario
closure and other animation consumers call this owner; no missing-image
placeholder or caller-specific bypass is added. Present malformed image
headers retain their existing error policy.

Rust regression coverage: `animation_theater_flags_match_original_fresh_read_and_postpass`
replays the 26 retained plus three new reader controls;
`imageless_retail_animation_reaches_existing_gas_particle_owner` uses the
production retail reader and actually constructs the stock particle-producing
animation against an empty image catalog, then checks particles and AI cleanup.
The latter is Rust integration, not native particle-world parity. These two
checks and `retail_meteor_profiles_keep_missing_stock_images_explicit` passed
in the integrated R25 Ares-enabled run. That batch exited 101 on seven other
fixture checks; this is named-test success, not whole-batch success. Feature-OFF
and optimized production map loading remain pending.
