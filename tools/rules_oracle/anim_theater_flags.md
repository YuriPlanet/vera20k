# Fresh animation image flags

`anim_theater_flags.py` executes the original AnimType factory `428B80`,
constructor `427530`, full ART reader `427D00`, theater postpass `427A80`, and
`GetImage` `428C30`. Its small corpus compares fresh `Theater` and `NewTheater`
resolution, including the stock `CAMOV02_AD` alias of `CAMOV02_A`.

The shared ObjectType reader `5F92D0` reads the flags from the section named by
the resolved, one-hop `Image` string. AnimType then reads its own section with
those current values as defaults (`427DAF` and `427DCC`). This second pass requires
a nonempty Image and a null image pointer. Fresh AnimTypes satisfy the null-pointer
condition even if a literal file exists: their original RTTI method `428E50`
returns 5, which skips the shared ObjectType image load. The first Anim-specific
load follows the own-section reads.

Consequently a recognized own value overrides the image-section value, while a
missing or invalid own value retains it. A missing image section starts from the
constructor's false defaults. The target section's further `Image` key is not
followed. Empty Image and conflicting own/target values are separate controls.

The corpus supplies cached INI strings and named synthetic SHP headers containing
eight frames. The three stock-ART rows retain the selected authored key values;
the headers are declared test resources, not copied retail images. Original
filename selection and frame-bound calculation execute, while the existing
`LoadFileFromMIX` transport supplies the named header or absence. The original
per-type theater postpass is admitted directly with Urban or Snow and untouched
startup defaults; the full Scenario startup loop is not executed.

Rows record flags both before and after the postpass, the original Boolean reads
with their default and return values, resource requests, and selected frame
counts. Paired controls keep ART identical while making the literal SHP present
or absent. These establish the fresh-reader boundary rather than inferring it
from file availability.

`src/rules/art_data_tests.rs::animation_theater_flags_match_original_fresh_read_and_postpass`
compares the production fresh-reader flags and the supported binding projection.
The separate retail Engineer test composes image binding with ordinary combat,
Building slot replacement and repair; those gameplay steps are not executed by
this native corpus.

Resident-image INI rereads, demand-load histories, the complete Theater filename
policy, Anim instance lifetime, Building slot replacement, damage/repair and
rendered pixels are outside this comparison. It does not establish a missing-image
instance-lifetime defect.

From the repository root, with `VERA20K_GAMEMD_EXE` selecting the original game:

```text
python -m tools.rules_oracle.anim_theater_flags --write
python -m tools.rules_oracle.anim_theater_flags --check
```

The standard lazy `finish_vectors` publisher keeps import/help independent of
native execution. The sidecar describes fixture boundaries and checked entry
points; it contains no executable identity or repository source hashes.

For a manual gameplay check, start an Allied skirmish on retail Drive-In
(`XInvasion.map`) and locate the Neutral `CAMOV02` building at map cell `(35,61)`.
Use a tank to force-attack it until its health falls below half, then stop firing
and allow launched shots to finish. Its damaged animation should remain visible.
Send an Engineer to repair the empty building; full health and the normal
animation should return. This is a reproducible check to perform, not a claim
that an interactive original-game comparison was run.
