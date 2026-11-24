# Explicit media archive policy

`app::frontend::startup_options` is the only owner that classifies retail `-CD`
arguments. It stores `MediaArchiveMode` directly in the startup options. The app
passes this value to `AssetManager::new` and retains it in `ProcessAssets`, even
when startup has no usable archives. Loading recovery uses that retained value.
The asset layer only executes the policy; it never reads process arguments.

Capture constructors, headless loading, tools and fixtures select
`MediaArchiveMode::STOCK_DIGITAL` explicitly (media index 2, the `03` family).
Capture profile/output paths and tool paths containing `-Cd` cannot change it.
There is no ambient `Default` or compatibility constructor that reparses argv.
The interactive parser preserves ASCII case-insensitive substring matching,
including `-cd`, `-CDROM` and `prefix-CDsuffix`.

[Original instruction evidence](media_policy.native.md) records the pinned
executable, exact inspection commands, callers, flag write and consumers. This
is control-flow evidence. No arithmetic, RNG, timer, detach or archive-enumeration
algorithm changed. Native suffix handling after argument-start + 3 remains a
separate recorded residual; this change does not claim the whole native `-CD`
mechanism. Existing OS wildcard enumeration behavior remains unchanged.

Reproduce focused checks with the indexed build owner:

```sh
python -m tools.cargo_run -- test -p vera20k --lib app::frontend::startup_options::
python -m tools.cargo_run -- test -p vera20k --lib app::frontend::launch::
python -m tools.cargo_run -- test -p vera20k-core --lib assets::asset_manager::
python -m tools.cargo_run -- test -p vera20k --lib app::loading::pump::tests::absent_and_lost_loading_managers_preserve_media_policy
```

The reconstruction test creates distinct numbered-only/wildcard-only lookup
coverage and drives the production loading lease recovery in both policies.
Capture validation uses [map observation](map_observation.md), comparing the
same profile with ordinary and mixed-case `-Cd` output paths. Headless and asset
CLI runs use the preserved release binaries, including `-Cd` in path arguments.

Recorded validation (`tools/media_policy.validation.json`, in git at 1b1a8b43e) bounds the preserved builds,
source inventory, Rust checks and production comparisons.
