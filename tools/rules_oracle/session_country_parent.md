# Retained ParentCountry and AI ownership masks

Run the bounded original-instruction corpus with an original `gamemd.exe`:

```sh
python -m tools.rules_oracle.session_country --parent --check
```

Use the executable selection documented in [native_oracle.md](../native_oracle.md).
This mode extends the existing physical Rules/Session Country oracle; it does
not read retail INIs, extract game assets or change the default Session corpus.

The native Country constructor `0x5113F0` clears only the first byte of retained
`ParentCountry` storage at `+0x98` (`0x511404`). A successful body read
`0x511850` reads exact-case `ParentCountry` from the current stored-ID section,
with the current ID as default and capacity 25 (`0x5118CF..0x511928`). It keeps
at most 24 bytes. Missing or wrong-case sections return before that read and
retain old text. Missing keys in a surviving section reset it to the current ID.

The physical INI loader drops empty or whitespace-only values. If no other key
survives, the section also disappears and old parent text remains. If `Name=`
keeps the section present, that same authored empty parent defaults to self.
These controls use original physical loading and the complete Country reader;
they do not inject the distinct cached-empty-entry edge case.

`0x5117D0` resolves the first Country with matching stored `Name` or ID, ignoring
ASCII case. It returns `+0xB8`, -1 for unknown/empty/`<none>`, and -2 for the
special `<random>` token. It does not recursively follow parent text. Native
32-bit shifts therefore use bits 31 and 30 for these negative results.

`HouseClass::FirstBuildableFromArray` (`0x5051E0`) deliberately uses two identities:

| Gate | Native source |
| --- | --- |
| `Owner=` | Parent text `+0x98` → `0x5117D0`, `0x5051E9..0x505224` |
| `RequiredHouses=` | Actual Country index `+0xB8`, `0x50523B..0x50524B` |
| `ForbiddenHouses=` | Actual Country index `+0xB8`, `0x50525E..0x50526E` |

Ten whole-function controls distinguish these gates with one supplied
candidate and a House pointing to the original constructed Country. They also
execute unknown and `<random>` parent shifts. Three cases use an authored
Owner value read through the original physical INI loader and whole
`ReadOwners` (`0x4750D0`), then feed that result directly into FirstBuildable.
Candidate side is unrestricted
and super weapons are enabled. This does not establish whole-AI scheduling,
factory production, initial-force generation or player sidebar parity.

Eight additional whole `ReadOwners` controls cover ordinary names, aliases,
unknown values, duplicates and special tokens. Its token helper `0x48DEB0`
rejects only lookup result -1, so `<random>` (-2) contributes bit 30.
An unregistered `<none>` contributes zero; the TriggerType-specific rule
mapping it to country zero does not apply. Existing tokenization and capacity
128 remain with the production INI list reader. Physical layer history of
Owner/RequiredHouses/ForbiddenHouses beyond these declared reads is not covered.

The corpus retains three complete seeded RNG states and asserts no change for
each selected call. It records one 12-byte action-timer sentinel and its writes;
it does not cover other timers or whole-world detachment. Original mapped code
bytes are checked unchanged. Windows/file/heap/CRT/TLS transport and synthetic
input/state boundaries remain owned by `RetailRulesHistory`.
