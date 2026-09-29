# Test-suite audit (2026-09, static read at e83df3d)

Read-only audit of ~9,850 `#[test]` in `src/` plus 51 in `tests/`, split into 11 partitions.
Each file lists candidates as `path::test | CATEGORY | evidence | confidence | action`.
**Unverified:** the adversarial verification pass was stopped before completion; re-check each
claim (callers, `#[cfg(test)]` gating, named covering test runs by default) before deleting.

Categories: TESTONLY (tests `#[cfg(test)]`/uncalled code), OBSOLETE, MIGRATION, NOTATEST, WRONG,
MIRROR (restates implementation), DUP, E2E (covered by a named non-ignored test), CONSOLIDATE.

| Partition | Tests | High | High+Medium (incl. merges) |
|---|---|---|---|
| sim/world | 1,138 | ~62 | ~166 (15%) |
| sim/*.rs A–I | 689 | ~100 | ~162 (24%) |
| sim/*.rs J–Z | 696 | ~31 | ~113 (16%) |
| sim movement+pathfinding | 1,093 | ~144 | ~208 (19%) |
| sim combat/production/miner/particles | 1,028 | ~96 | ~146 (14%) |
| other sim, crate root, net, util | 1,086 | ~115 | ~277 (26%) |
| rules/audio/sidebar | 873 | ~54 | ~195 (22%) |
| map + rmg | 909 | ~60 | ~148 (16%) |
| render/assets/asset_tools | 1,045 | ~115 | ~278 (27%) |
| app | 965 | ~75 | ~151 (16%) |
| ui | 460 | ~38 | ~98 (21%) |
| **Total** | **~9,900** | **~890 (9%)** | **~1,940 (20%)** |

Cross-cutting:
- `tests/` (51): never run (`--lib` only); 47 `#[ignore]`, 13 files assertion-free,
  `extract_ini_files.rs` duplicates `src/bin/extract-ini.rs`.
- `src/sim/world/hash_schema.rs` test-only historical hash projections (~830 LOC incl. gates).
- ~1,255 `#[cfg(test)]` items in production files; ~6,000 LOC of test-only/unwired code found.
- 7 `#[ignore]` `panic!` stub tests; 19 tests silently `return` without a retail install.
- Retail e2e tests are `#[ignore]`; the global parity harness covers one movement/combat/harvester
  scenario. E2E coverage justified almost no deletions.
