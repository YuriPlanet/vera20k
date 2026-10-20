# Media argument ownership: native instruction evidence

Read-only inspection at 2026-09-28T01:06:36.176354+00:00.
Checkout HEAD: `0090b85ca1cfec6871c8075f1a3e2bda7bf10a63`.
Image base `0x00400000`; Capstone 5.0.7.

## Finding and owner boundary

The original parser at 0x0052F620 skips argv[0], uppercases each following argument, and searches for the ASCII literal -CD as a substring at 0x0052F7A0. It sets byte 0x0089E3A0 to 1. Init_Mix_Files at 0x00530460 reads this same byte for both map and movie media selection. This supports preserving case-insensitive substring matching for ASCII forms (-cd, -CDROM, prefix-CDsuffix), not narrowing to whole-token equality.

Existing Rust app/frontend/startup_options.rs already owns the classifier. AssetManager independently scans ambient process argv through MediaArchiveMode::default. Capture argv intentionally bypasses the retail classifier, yet startup manager construction still sees output/profile paths through the ambient scan. Eliminate that second decision; keep media mounting in AssetManager and retain the selected startup mode for ProcessAssets loading recovery.

Historical evidence leads, now checked against original bytes: commit 8c2e873b65c8d6327c0866bb2bd5624614060dc0 names parser 0x0052F620; commit c45af80305faa4d61078ea684e2b2d43813139df names Init_Mix_Files 0x00530460.

## Reproduce

From the repository, select the executable with VERA20K_GAMEMD_EXE (or RA2_DIR). On this host `source ~/Documents/vera20k-dev/env.sh` provided that selection and pinned Python dependencies. All commands below use the indexed `tools.native_inspect` owner; no private PE mapper or disassembler was used. Packet hashes are SHA-256 of the exact tool stdout bytes; instructions below are selected from those packets.

### Argument iteration and uppercase call

```sh
python -m tools.native_inspect disasm 0x52f620 --bytes 73
```

Tool stdout SHA-256: `fd0a91ee6fa8cd26ffab2ca4fe9f623230f3bcfba881e4043614312034be77fb`.
Coverage: 0x0052F620+73 bytes, 0 undecoded.

```text
0052F620  83ec28               sub esp, 0x28
0052F623  53                   push ebx
0052F624  55                   push ebp
0052F625  56                   push esi
0052F626  57                   push edi
0052F627  8bf9                 mov edi, ecx
0052F629  8bf2                 mov esi, edx
0052F62B  b960e9a800           mov ecx, 0xa8e960
0052F630  897c2420             mov dword ptr [esp + 0x20], edi
0052F634  e8a7941800           call 0x6b8ae0
0052F639  33db                 xor ebx, ebx
0052F63B  83ff01               cmp edi, 1
0052F63E  881d6beda800         mov byte ptr [0xa8ed6b], bl
0052F644  c744241401000000     mov dword ptr [esp + 0x14], 1
0052F64C  0f8ee5040000         jle 0x52fb37
0052F652  83c604               add esi, 4
0052F655  bd02000000           mov ebp, 2
0052F65A  89742418             mov dword ptr [esp + 0x18], esi
0052F65E  8b06                 mov eax, dword ptr [esi]
0052F660  50                   push eax
0052F661  e85ed92a00           call 0x7dcfc4
0052F666  8bf0                 mov esi, eax
0052F668  56                   push esi
```

### ASCII uppercase body

```sh
python -m tools.native_inspect disasm 0x7dcfc4 --bytes 62
```

Tool stdout SHA-256: `db0963f943d99e8c55841446c6f39845a83501e17f4498c62cf4a0a93b374019`.
Coverage: 0x007DCFC4+62 bytes, 0 undecoded.

```text
007DCFC4  55                   push ebp
007DCFC5  8bec                 mov ebp, esp
007DCFC7  51                   push ecx
007DCFC8  51                   push ecx
007DCFC9  56                   push esi
007DCFCA  33f6                 xor esi, esi
007DCFCC  3935a082b700         cmp dword ptr [0xb782a0], esi
007DCFD2  57                   push edi
007DCFD3  8975f8               mov dword ptr [ebp - 8], esi
007DCFD6  752a                 jne 0x7dd002
007DCFD8  8b4508               mov eax, dword ptr [ebp + 8]
007DCFDB  8bd0                 mov edx, eax
007DCFDD  803800               cmp byte ptr [eax], 0
007DCFE0  0f840e010000         je 0x7dd0f4
007DCFE6  8a0a                 mov cl, byte ptr [edx]
007DCFE8  80f961               cmp cl, 0x61
007DCFEB  7c0a                 jl 0x7dcff7
007DCFED  80f97a               cmp cl, 0x7a
007DCFF0  7f05                 jg 0x7dcff7
007DCFF2  80e920               sub cl, 0x20
007DCFF5  880a                 mov byte ptr [edx], cl
007DCFF7  42                   inc edx
007DCFF8  803a00               cmp byte ptr [edx], 0
007DCFFB  75e9                 jne 0x7dcfe6
007DCFFD  e9f2000000           jmp 0x7dd0f4
```

### CD substring branch and suffix handoff

```sh
python -m tools.native_inspect disasm 0x52f79a --bytes 38
```

Tool stdout SHA-256: `7d7bb98678ec09256c4053b0bac7b6f73796e683ef0e795336f739c214957612`.
Coverage: 0x0052F79A+38 bytes, 0 undecoded.

```text
0052F79A  68a8658200           push 0x8265a8
0052F79F  56                   push esi
0052F7A0  e80bad2900           call 0x7ca4b0
0052F7A5  83c408               add esp, 8
0052F7A8  85c0                 test eax, eax
0052F7AA  7414                 je 0x52f7c0
0052F7AC  8d4e03               lea ecx, [esi + 3]
0052F7AF  c605a0e3890001       mov byte ptr [0x89e3a0], 1
0052F7B6  e855b3f4ff           call 0x47ab10
0052F7BB  e957030000           jmp 0x52fb17
```

### Argument loop continuation

```sh
python -m tools.native_inspect disasm 0x52fb17 --bytes 32
```

Tool stdout SHA-256: `833e6d5b954eb926f54e39aa5e151efbc22c6664bd734b738b05bb6313119d3c`.
Coverage: 0x0052FB17+32 bytes, 0 undecoded.

```text
0052FB17  8b442414             mov eax, dword ptr [esp + 0x14]
0052FB1B  8b742418             mov esi, dword ptr [esp + 0x18]
0052FB1F  8b4c2420             mov ecx, dword ptr [esp + 0x20]
0052FB23  40                   inc eax
0052FB24  83c604               add esi, 4
0052FB27  3bc1                 cmp eax, ecx
0052FB29  89442414             mov dword ptr [esp + 0x14], eax
0052FB2D  89742418             mov dword ptr [esp + 0x18], esi
0052FB31  0f8c27fbffff         jl 0x52f65e
```

### Substring implementation

```sh
python -m tools.native_inspect disasm 0x7ca4b0 --bytes 124
```

Tool stdout SHA-256: `55ccd81d9621b74e99d52bdf75957db702a65c13d47c9008510f39bcbbfde995`.
Coverage: 0x007CA4B0+124 bytes, 0 undecoded.

```text
007CA4CE  8a07                 mov al, byte ptr [edi]
007CA4D0  46                   inc esi
007CA4D1  38d0                 cmp al, dl
007CA4D3  7415                 je 0x7ca4ea
007CA4D5  84c0                 test al, al
007CA4D7  740b                 je 0x7ca4e4
007CA4D9  8a06                 mov al, byte ptr [esi]
007CA4DB  46                   inc esi
007CA4DC  38d0                 cmp al, dl
007CA4DE  740a                 je 0x7ca4ea
007CA4E0  84c0                 test al, al
007CA4E2  75f5                 jne 0x7ca4d9
007CA4E4  5e                   pop esi
007CA4E5  5b                   pop ebx
007CA4E6  5f                   pop edi
007CA4E7  33c0                 xor eax, eax
007CA4E9  c3                   ret 
007CA4EA  8a06                 mov al, byte ptr [esi]
007CA4EC  46                   inc esi
007CA4ED  38f0                 cmp al, dh
007CA4EF  75eb                 jne 0x7ca4dc
007CA4F1  8d7eff               lea edi, [esi - 1]
007CA4F4  8a6102               mov ah, byte ptr [ecx + 2]
007CA4F7  84e4                 test ah, ah
007CA4F9  7428                 je 0x7ca523
007CA4FB  8a06                 mov al, byte ptr [esi]
007CA4FD  83c602               add esi, 2
007CA500  38e0                 cmp al, ah
007CA502  75c4                 jne 0x7ca4c8
007CA504  8a4103               mov al, byte ptr [ecx + 3]
007CA507  84c0                 test al, al
007CA509  7418                 je 0x7ca523
007CA50B  8a66ff               mov ah, byte ptr [esi - 1]
007CA50E  83c102               add ecx, 2
007CA511  38e0                 cmp al, ah
007CA513  74df                 je 0x7ca4f4
007CA515  ebb1                 jmp 0x7ca4c8
007CA523  8d47ff               lea eax, [edi - 1]
007CA526  5e                   pop esi
007CA527  5b                   pop ebx
007CA528  5f                   pop edi
007CA529  c3                   ret 
```

### Map media flag consumer

```sh
python -m tools.native_inspect disasm 0x530792 --bytes 62
```

Tool stdout SHA-256: `b5f5784e600502bbc0ef9f01b38417709764800cbd3c82712463f322819bc871`.
Coverage: 0x00530792+62 bytes, 0 undecoded.

```text
00530792  b93c000000           mov ecx, 0x3c
00530797  e80489f4ff           call 0x4790a0
0053079C  8bf0                 mov esi, eax
0053079E  a0a0e38900           mov al, byte ptr [0x89e3a0]
005307A3  46                   inc esi
005307A4  3c01                 cmp al, 1
005307A6  8974247c             mov dword ptr [esp + 0x7c], esi
005307AA  0f857e020000         jne 0x530a2e
005307B0  bf9c678200           mov edi, 0x82679c
005307B5  83c9ff               or ecx, 0xffffffff
005307B8  33c0                 xor eax, eax
005307BA  8d9424a4000000       lea edx, [esp + 0xa4]
005307C1  f2ae                 repne scasb al, byte ptr es:[edi]
005307C3  f7d1                 not ecx
005307C5  2bf9                 sub edi, ecx
005307C7  8bc1                 mov eax, ecx
005307C9  8bf7                 mov esi, edi
005307CB  8bfa                 mov edi, edx
005307CD  c1e902               shr ecx, 2
```

### Numbered map format call

```sh
python -m tools.native_inspect disasm 0x530a2e --bytes 22
```

Tool stdout SHA-256: `60f9ee28cb454249d2ea593c4a81e2aad32376ecc9353d029442dec679acb7a7`.
Coverage: 0x00530A2E+22 bytes, 0 undecoded.

```text
00530A2E  56                   push esi
00530A2F  8d9424a8000000       lea edx, [esp + 0xa8]
00530A36  68ecc28100           push 0x81c2ec
00530A3B  52                   push edx
00530A3C  e8b3842900           call 0x7c8ef4
00530A41  83c40c               add esp, 0xc
```

### Movie media flag consumer

```sh
python -m tools.native_inspect disasm 0x530d05 --bytes 18
```

Tool stdout SHA-256: `505c6081fa24e2a523eeed84479b2b722248dd0104f67d69a4dbc3752665659c`.
Coverage: 0x00530D05+18 bytes, 0 undecoded.

```text
00530D05  803da0e3890001       cmp byte ptr [0x89e3a0], 1
00530D0C  0f8562040000         jne 0x531174
00530D12  bf48678200           mov edi, 0x826748
```

### Parser caller

```sh
python -m tools.native_inspect disasm 0x6bc082 --bytes 23
```

Tool stdout SHA-256: `9a576753dacfe8aef05bb7996efd1a67efe54f2445db1a23aae077a6cd83cd97`.
Coverage: 0x006BC082+23 bytes, 0 undecoded.

```text
006BC082  8bf3                 mov esi, ebx
006BC084  8d958cfeffff         lea edx, [ebp - 0x174]
006BC08A  8bcf                 mov ecx, edi
006BC08C  e88f35e7ff           call 0x52f620
006BC091  84c0                 test al, al
006BC093  0f8452200000         je 0x6be0eb
```

### Archive initialization caller

```sh
python -m tools.native_inspect disasm 0x52c588 --bytes 29
```

Tool stdout SHA-256: `543db9d92fd20d5724a191fa9d734ec27c55c49c16c256734f7c9be730904896`.
Coverage: 0x0052C588+29 bytes, 0 undecoded.

```text
0052C588  8bcd                 mov ecx, ebp
0052C58A  e821cbf4ff           call 0x4790b0
0052C58F  68805f8200           push 0x825f80
0052C594  e847a3edff           call 0x4068e0
0052C599  83c404               add esp, 4
0052C59C  e8bf3e0000           call 0x530460
0052C5A1  84c0                 test al, al
0052C5A3  752e                 jne 0x52c5d3
```

### CD literal

```sh
python -m tools.native_inspect read 0x8265a8 --bytes 4
```

Tool stdout SHA-256: `e1243bd9e491b19faf9c1c3fd87ad0bc19370c8074101e8c931addc4213af591`.

Raw bytes: `2d434400`; ASCII `-CD`.

### Wildcard map literal

```sh
python -m tools.native_inspect read 0x82679c --bytes 12
```

Tool stdout SHA-256: `7695be5d2e884b6fdda5f0f391a528d725fc8dcb021c8d21ba0523f8dbaf17a3`.

Raw bytes: `4d4150534d442a2e4d495800`; ASCII `MAPSMD*.MIX`.

### Numbered map literal

```sh
python -m tools.native_inspect read 0x81c2ec --bytes 15
```

Tool stdout SHA-256: `880eb8c0e09763c671344382b9a6b911a9291736faccb040af8b0d1cce381c0d`.

Raw bytes: `4d4150534d44253032642e4d495800`; ASCII `MAPSMD%02d.MIX`.

### Wildcard movie literal

```sh
python -m tools.native_inspect read 0x826748 --bytes 11
```

Tool stdout SHA-256: `5e8bfa3e3b91a74d91367225fa6918af28f231b05a06dfda489b83b2986fa4be`.

Raw bytes: `4d4f564d442a2e4d495800`; ASCII `MOVMD*.MIX`.

### Parser direct caller scan

```sh
python -m tools.native_inspect calls 0x52f620
```

Tool stdout SHA-256: `1899940a446a93e094178adc46ef04cae06b4adac676608b9f663ff56c7c5551`.
Coverage: 0x00401000+4063232 bytes, 66 undecoded.

```text
006BC08C  e88f35e7ff           call 0x52f620
```

### Mount direct caller scan

```sh
python -m tools.native_inspect calls 0x530460
```

Tool stdout SHA-256: `a7b4904b2497961f03069d3e3ea5ffa93364fcefd02d9e7c5111570f43f1cb53`.
Coverage: 0x00401000+4063232 bytes, 66 undecoded.

```text
0052C59C  e8bf3e0000           call 0x530460
```

## Coverage and residual limits

- This is static instruction/body/caller/data evidence, not native execution, Windows startup capture, a golden, or complete argument-parser parity. Direct caller scans establish decoded call sites, not exhaustive alias/indirect-call absence or runtime reachability. The parser caller 0x006BC08C is in the known WinMain startup chain; the mount caller is 0x0052C59C. Their surrounding control flow and flags must remain part of any wider startup claim.
- Only the ASCII uppercase fast path was inspected here. Locale-dependent branches and non-ASCII argv conversion are outside this packet. Earlier switch/CRC branches precede the CD test; this packet does not certify every parser edge case.
- The stock digital media index 2 remains an existing Rust policy; this packet proves the native index-plus-one use, not the runtime provenance of the value 2. Do not present it as newly executed evidence.
- A matched CD argument also passes argument start + 3 (not substring match + 3) to 0x0047AB10 at 0x0052F7B6. Prior bounded inspection observed append/tokenization with semicolon literal 0x0081C8FC and global 0x0089E41C. Current Rust ignores that suffix side effect. Complete filesystem effects and native execution remain unestablished; moving the existing media decision does not close that residual.
- Flag 0x0089E3A0 is not file-backed; native_inspect correctly rejected a static read there. No runtime initializer/value was inferred from a nonexistent file span.
- Keep existing numbered/wildcard mount ordering and errors unchanged. Capture paths are a VERA-specific sealed interface; selecting numbered media independently of their spelling is intentional, not a claim about native argument-value parsing.

## Current constructor inventory and other-owner overlaps

Current source search finds 97 one-argument AssetManager::new spellings across 66 Rust files. These include dormant integration-test sources and are not a claim that all callers currently execute.

Overlap checked with read-only `git worktree list --porcelain` and `git -C <worktree> status --porcelain=v1 -z`. Only dirty paths intersecting anticipated constructor/process-mode changes are listed below. No other checkout or process was changed.

- `~/Documents/vera20k-worktrees/vera20k-deadcode`: ` M src/map/terrain.rs`
- `~/Documents/vera20k-worktrees/vera20k-shell-score`: ` M src/app/frontend/skirmish_shell_render.rs`
- `~/Documents/vera20k-worktrees/vera20k-shell-score`: ` M src/render/skirmish_shell_chrome.rs`

These are status snapshots, not permission to modify those checkouts. Recheck immediately before implementation/rebase, keep edits in the owned worktree, and resolve any resulting overlapping integration changes without overwriting the other owners.
