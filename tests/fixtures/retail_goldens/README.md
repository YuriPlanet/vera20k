# Retail decoder regression reference

Run and export instructions: [retail corpus owner](../../../tools/retail_corpus.md).
`manifest.json` contains archive-index fingerprints and Rust decoder digests.
These are **not native oracle goldens**. Never update them from a failing test.

## October 28 sniffer change

`detect_format` no longer drops every entry whose leading zero word reads as a nested
MIX header (#1293). That newly admits 2,998 SHP and 32 PAL files (5,448 and 130 in all,
11,854 sniffed), which changes only those two counts and rollups. Every other field,
including the ten named digests, is unchanged. Across the corpus `identify` changes one
verdict: `audio.mix` inside `language.mix` was a false VPL and is now a nested MIX.

## September 27 migration

The 21 former integration tests were outside the required `cargo test --lib`
path. Moving them to `asset_tools::retail_corpus` exposed a reference last changed
at `58b5d5b3e`, before several deliberate production changes. The migration changes
no production parser or archive-loading behavior.

The recorded comparison accounts for the refreshed fields. Nothing reads it, so
it lives in history only:
`git show c273a9104:tests/fixtures/retail_goldens/migration-2026-09-27.json`.

| Reference field | Evidence and reason |
| --- | --- |
| 131 archives become 64 | All 37 unchanged names retain their entry count/index digest. Another 27 are case changes or hash-to-filename renames, matched by index fingerprint and filename hash. The remaining 67 were false nested-MIX detections: 55 SHP, 10 PCX and 2 CSF leaf files. Release `asset ls` identifies every one in its parent archive; they remain covered as leaf assets. The production traversal changed in `af75f1c1c`. |
| Format and audio counts | All 8,824 directly sniffed files, per-format counts and 3,438 bag entries are unchanged. The broader release `asset parse-check --all-mixes` identifier parses 11,855 files with zero failures; unknown formats remain explicitly uncovered. |
| CSF, PCX and SHP rollups | Replaying the historical archive order through the current decoders reproduces each old digest exactly. Only traversal order changed these aggregates. |
| PAL rollup and three named PAL digests | `58f999bfd` replaced the old full-range scaling with native component expansion. `python -m tools.sidebar_oracle.palette --check` reproduced the committed fixture against the retail executable. The corpus test now derives expected RGB from its complete native byte-domain outputs, without a second scale formula. Alpha policy is excluded from this native comparison. |
| TMP rollup | Historical-order comparison against `58b5d5b3e:src/assets/tmp_decode.rs` reproduces the old digest `11488277409661197448`. Dimensions, origins and color pixels match on every tile. Depth differs on 3,527 tiles in 1,968 files: `58f999bfd` replaced independent ExtraZ `<32`/empty-destination filtering with covered-color ExtraZ composition. Current decoders in the same historical order yield `5271243436535791568`. This is an accounted Rust baseline migration, **not newly demonstrated native TMP depth parity**. |
| Seven non-PAL named digests | Unchanged. Inactive-theater files now use the existing asset browser's explicit catalogue fallback rather than pretending they are reachable through normal runtime lookup. |

The replay used current `digest_decode` and archive bytes, with nested children
before each parent, children sorted by signed MIX hash, and the historical root
sequence stored in the comparison JSON. The TMP experiment loaded the historical
decoder source and adapted only the later-added `relative_extra_y` struct field,
which is absent from the digest. No historical decoder or traversal path remains
in production or in the maintained test suite.

To investigate another drift, export a new candidate, compare every field with
the committed reference, and inspect the relevant source history. For this
migration, `git show 58b5d5b3e:tests/fixtures/retail_goldens/manifest.json` retrieves
the original reference. The recorded archive order allows the same decoder-fold
experiment to be repeated without reconstructing any asset decoder.

The checked palette run records fixture payload SHA-256 in the
comparison JSON and the oracle's existing metadata. Original render-time TMP
depth composition remains outside this parser corpus's proof; see the explicit
limitation in the historical TMP report.
