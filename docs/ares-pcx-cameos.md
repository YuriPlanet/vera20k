# Optional Ares PCX sidebar icons

Build with `--features ares` to read ART `CameoPCX` and `AltCameoPCX`, and
the SuperWeaponType rules key `SidebarPCX`. Default builds ignore these keys.
Filenames include their extension. Normal and promoted keys are read only from
the type's rules `Image` ART section; that section's own `Image` redirect does
not redirect these reads. A present name is lowercased and limited to 31 bytes.
`none` and `<none>` are literal attempted filenames, not clear tokens.

The existing asset manager and PCX decoder supply indexed pixels and their
embedded palette. A promoted loaded PCX takes precedence over the normal PCX;
the normal PCX takes precedence over a promoted SHP. A failed PCX load falls
back to the existing SHP path. Successfully loaded transparent PCXs stay
selected. RGB565-packed magenta is transparent; black is opaque. The destination
canvas is 60x48, while the source pixel extent is retained without stretching.

The atlas is derived presentation state. Object, promoted and superweapon
entries have separate namespaces keyed by type identity. Two superweapons
sharing a missing PCX can therefore retain different SHP fallbacks, and a
superweapon cannot inherit an object's promoted icon on an ID collision.
No new simulation or snapshot state is introduced by this feature.

The [23-control original-instruction corpus](../tools/sidebar_oracle/pcx_cameos.md)
establishes bounded filename reading, selection, palette packing and word
transparency. Renderer regressions consume these goldens and pass synthetic
PCX files through the production decoder and collector. The controls supply
some runtime services; they do not execute the complete native file decoder,
GPU presentation or DLL startup. 24-bit PCXs and whole-mod visual parity are
not established. Existing guessed SHP fallback filenames remain a separate
pre-existing limitation. No mod assets or resource adapters are included.
