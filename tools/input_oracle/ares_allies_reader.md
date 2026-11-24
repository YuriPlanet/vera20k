# Ares Allies reader buffer controls

Reproduce with enrolled local binaries, without any mod map or artwork:

```sh
VERA20K_GAMEMD_EXE='<local-retail-gamemd.exe>' \
VERA20K_ARES_DLL='<local-Ares.dll>' \
python -m tools.input_oracle.campaign_start --ares-allies --check
```

The [corpus](ares_allies_reader.json) extends the existing campaign producer;
its `allies_reader_controls` has the same registry/rows/delimiter schema as
`campaign_start_houses.json`. The feature-disabled control remains the original
27-case native corpus. This producer executes those27 cases again and rejects
any mismatch before generating the feature-enabled27 counterparts. Use
`--write` only when deliberately accepting newly executed references.

## Executed chain and transport boundary

The installed enrolled Ares DLL has SHA-256
`93ce890f387a6c1b3240a2dd7b674f25fec5a0e96f0b0d0306796b2edf7b5b83`.
Its hook records47527C/475297 dispatch handlers1005BED0/1005BEF0. The first
handler writes the pending ReadString output pointer and capacity2048; the
second writes saved ECX for the first `strtok`. Both return0 for ordinary
continuation. See pinned public
[Bugfixes.Buffer.cpp](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Misc/Bugfixes.Buffer.cpp#L327).

The existing Ares `Probe` executes both actual handler bodies against declared
register-frame/stack storage. Its observed output words, not hand constants,
are transported at original47527C/475297 in the existing `CampaignReader` VM.
The two VMs deliberately remain separate: CampaignReader's existing stock
stack occupies the Ares DLL's address range. The declared runtime buffer is in
an unused portion of that original fixture stack. No DLL code is mapped over
its stack, and no original executable instruction is patched.

This **does not execute the Syringe loader, patch trampoline, full DLL startup
or complete scenario admission**. Only the stated hook-frame ABI outputs are
transported. Original475260, ReadString528A10, comma-only CRT tokenization,
House::FromName50C170 and32bit mask instructions execute. No parser, lookup,
mask or gameplay return is supplied. Existing INI cache/allocation/CRT transport
and synthetic House array are inherited fixture inputs. All original code and
mapped Ares executable bytes are checked unchanged; RNG state is unchanged and
observed RNG/detach calls are rejected.

## Observed results

| Control | Original128-byte reader | Ares2048-byte reader |
| --- | --- | --- |
| Existing126`X` bytes followed by `,B` | `0x80000000` | `0x80000002` |
| Nineteen synthetic15-byte names,303-byte list | `0x000000ff`,127 copied | `0x0007ffff`,303 copied |
|2045`X` bytes followed by `,B` (2047 total) | `0x80000000` | `0x80000002`,2047 copied |
|2046`X` bytes followed by `,B` (2048 total) | `0x80000000` | `0x80000000`,2047 copied |

These are native execution outputs, recorded with original copied bytes and
lookup tokens in the extended controls. The old boundary's bit31 comes from
native unknown-name lookup and x86's masked shift, not an added invalid-name
policy. No32bit alliance-width extension, symmetrization or Country/Side-based
friendship is implied. Long text can lose alliance targets even with fewer
than32 Houses. Existing full House/ScenarioInit and one-way-alliance controls
remain in the original campaign corpus; this isolated reader corpus does not
claim to execute them or establish whole-game diplomacy parity.
