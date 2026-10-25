# Synthetic installer fixture

These tiny archives contain authored test data, not retail game files. They
exercise the shipped file checker in the Windows installer smoke test and the
app's report tests. They are not a playable game and are never packaged.

The MIX bytes were authored with `assets::asset_manager::test_support` and
`map::source::test_support::make_new_format_mix_bytes`; the single-label CSF
uses `assets::csf_file::build_test_csf`'s encoder. `ra2md.mix` holds the CSF and
the Rules, Art, AI and music INIs, so these tests require archive resolution.
The music INI names `test-song.wav`, which is intentionally absent: setup
must warn and allow installation. The other archives hold only `unrelated.bin`.
The loose palette and five-frame 1x1 SHP use the authored fixture from
`render::main_menu_shell_chrome::tests::mandatory_button_loader_preserves_failure_details_and_palette_fallback`.

The neighboring `test-language-german.csf` contains the same authored CSF with
language ID 2. Tests use it as a loose override to verify that the picker reports
the game file's language instead of the operating system's language.

`picker-tests.iss` exercises the production picker functions and native controls
in both installer languages. The release workflow assembles it with the actual
installer code; the test setup exits before installing files.
