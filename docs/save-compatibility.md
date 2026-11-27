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

Loading requires the resources used by the save. A converted save keeps
its saved processed rules but not the old writer's rules digest: it loads
with the rules and assets this game has, and its next ordinary save records
the current digest. A save from a modded game should be converted with the
same mod installed.

## Changing the save format

Compare the source and target with `snapshot-layout --describe` to see the
saved fields, types and enum variant order. Review their meaning too,
since adjacent releases can share both the layout ID and description.
A change to the rules digest needs no conversion step: converted saves
carry `CONVERTED_RULES_HASH` and skip the comparison.

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
its bridge cells convert with no fixed deck. The `6d6caa108` writer's layout
`0xCADF6067` predates a map House's Ares `Degrades=`; its Houses convert with
none and keep their country's answer. The `ffe8b25af` writer's layout
`0xD6C75D57` predates the Firestorm ice chain; its worlds convert with no ice.
The `93ab2ce86` writer's layout `0xAFC7F643` predates the pending terrain
dent; it converts with none pending. The `06789b62b` writer's layout
`0x82C32EA4` predates the Tiberian Sun limpets; its objects convert with none.
The `8d55b4a73` writer's layout `0xF6127DC1` predates Tiberian Sun's
BounceSound and ExpireSound; its animation types convert with neither sound.
The `8852f7a76` writer's layout `0x9441E8D8` predates the Kratos infantry firing
offsets. Its saves convert with Kratos off, as those matches played. The
`bc55c35e7` writer's layout `0x9F1BD701` predates Tiberian Sun webs and
the processed rules' NPatch key reads; its saves convert with no web and the
switch off, as those matches played. The `30e9a0f38` writer's layout
`0xF215BF3A` predates the Wasm extension modules, Phobos ArtImageSwap and the
NPatch reads; its saves convert with no module, no swapped image and the
switch off. Later writers each stored part of the current layout: the
`a33fcac81` writer's layout `0xE18EA373` holds the modules, main's writer from
#5205, layout `0x8C73B1B3`, the swap, the `2e39b88dd` writer's layout
`0xB65C0664` the modules and the swap, the `2e67ebaa8` writer's layout
`0xE5279FB2` only the NPatch reads, the `fb29ff473` writer's layout
`0xBA51A42C` all three without the modules' control points, and the
`d17e45e42` writer's layout `0x919AB3CD` the modules with their control points.
Each converts with its missing parts empty: no projectile path, push or
immunity modifier, no swapped image, and without the reads the switch stays
off, as those matches played. No frozen save holds `0x8C73B1B3`. The
`b268ba798` writer's layout `0xC66CE753` holds everything but the client
modules' state of layout `0x9909B4E7` (RFC 0001 section 10: the
messages waiting for `on_message` and the room's `AutomationAllowed`); its
saves convert with no message waiting and automation not allowed. The
`0b918d053` writer predates the module visuals. Later writers retained module
animations (`0xED16B100`), doors (`0x9593BB1E`) and image overrides
(`0xAA201352`). Conversion keeps each saved collection and starts the
missing ones empty. The `c3609b58a` writer covers layout `0xD5A96CCA`,
which holds the former external AI controller flag and economy settings.
On load, those computer houses resume ordinary AI with their saved money,
units and production queues. Any saved Wasm control claims still apply.
The next save clears the retired settings.

Run the menu and compatibility tests with this command.

```sh
python -m tools.cargo_run -- test --locked -p vera20k --no-default-features --lib app::persistence::resource_load::release_save_tests:: -- app::persistence::save_write_tests:: ui::shell::saved_file_input:: i18n::tests::shipped_languages_are_complete
```

The tests use redistributable authored data. They exercise the desktop
repository and backup, the normal GPU-independent preparation of resources
and the saved world, the running simulation, the current writer and a
second cold restore. Presentation upload and rendering can be exercised
separately in the game.

To check the shipping layout as well as the one compiled into the test
executable, build the stock `snapshot-layout` binary without test features
and set `VERA20K_STOCK_WRITER_LAYOUT` to its output; the test then compares
it with the newest frozen save.
