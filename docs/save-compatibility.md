# Player save compatibility

The stock desktop game supports player saves from v0.3.8 onward. A matching
save goes through the ordinary loader. For a supported older layout, select
the file in either load menu and choose Convert. The save dialog opens with
the empty slot selected and the original description ready to edit. Use it
to create a new slot, or select a compatible save and confirm the overwrite.
Older saves stay visible for reference. Conversion backs up the original
and selects the result in the load list. Choose Load to resume the game.
After confirming an overwrite during conversion, the previous contents of
the destination slot can be recovered from `saves/backups/replaced/`.

Loading requires the resources used by the save. Modified v0.3.8 ART with
aircraft frame offsets beyond `u16`, or animation particle counts above
256, can fail the rules check because later engines interpret those values
differently. The stock retail data falls within these limits.

## Changing the save format

Compare the source and target with `snapshot-layout --describe` to see the
saved fields, types and enum variant order. Review their meaning too,
since adjacent releases can share both the layout ID and description.
The load path also compares a digest of the processed rules and bound
assets. When a change affects that digest, check an older save that uses
the affected assets and verify that different rules are still rejected.

Keep the published writers' inputs in the
[release fixtures](../tools/save_compatibility/fixtures/README.md). Add a
frozen file from the new writer whenever the envelope or layout changes.
Record the release or pre-release revision, build configuration and
reproduction command. Register that file and its wire identity in
[the matrix](../src/app/persistence/release_save_tests.rs). Keep the older rows
and verify each against the new build. Give each release its own row even
when its writer shares a layout with a previous release.

For a change to a field's meaning, add a save containing that state and check
what it does after load. For a change inside an optional or repeated value,
keep both an empty case and a populated case from the historical writer.
Examples in the matrix are a queued speed change
that takes effect on the next frame and a unit that must leave a map tunnel
with its cell membership restored. The v0.4.0 glow case keeps one live and
one expired AlphaShape. Conversion sets their animation-listener flags to
false, preserves their positions and ownership, and allows the next tick to
remove only the expired glow. The ordinary v0.4.0 save covers the empty
collection. The pre-release writer at `eb05f451e` uses layout
`0xFD54FFFF`. Its saved object images predate the alternate-theater ART
flag and convert with that flag off. The `41f140f93` writer's layout
`0xCB545137` predates the fixed high-bridge deck level and Fly's EMP spin;
its bridge cells convert with no fixed deck. The `6d6caa108` writer covers
direct loading of layout `0xCADF6067`.

Run the menu and compatibility tests locally with the pull request workflow's command.

```sh
python -m tools.cargo_run -- test --locked -p vera20k --no-default-features --lib app::persistence::resource_load::release_save_tests:: -- app::persistence::save_write_tests:: ui::shell::saved_file_input:: i18n::tests::shipped_languages_are_complete
```

The tests use redistributable authored data. They exercise the desktop
repository and backup, the normal GPU-independent preparation of resources
and the saved world, the running simulation, the current writer and a
second cold restore. Presentation upload and rendering can be exercised
separately in the game.

CI additionally builds the stock `snapshot-layout` binary without test
features and compares its output with the newest frozen save. This checks
the shipping layout as well as the one compiled into the test executable.
