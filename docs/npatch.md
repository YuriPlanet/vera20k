# Optional NPatch compatibility

NPatch is a long-standing binary patch for Yuri's Revenge that predates the DLL-based Ares and
Phobos extensions. VERA20K implements its supported behavior in Rust; it does not load the
NPatch executable. Enable the capability explicitly:

```sh
cargo run --release --bin vera20k --features npatch
```

A build without the capability ignores NPatch keys and keeps stock behavior, just like a build
without `ares` or `phobos`.

The AI factory switches are `[General] DisableParallelAIQueues=` and the per-type
`DisableAIParallelProduction=`. Both default to no. The global switch comes from NPSE 2.0's
`RulesClass::ReadGeneral` hook (`0xC302E9..0xC3031A`) and its `Factory_AI` fetch hook
(`0x45033C`, cave `0xC2F07C`). The per-type behavior cites an NPSE manual
(section 1.4.11.4) that has not been independently located; its type/category scope and the
queue release semantics remain unverified. It is not a reader found in the cited executable.
The start gate uses existing live factory state rather than a second set of production counters.

## Cross-extension compatibility

The three capabilities — `ares`, `phobos`, `npatch` — are orthogonal and mixable:

```sh
cargo run --release --bin vera20k --features ares,phobos,npatch
```

Every implemented feature must keep working when the other capabilities are compiled in. A chain
may depend on another **capability's own** state (for example a Phobos key that only a Phobos type
reads), but it must not assume the other features are absent, and it must not re-own state another
capability already owns. When two extensions touch the same native site, one of them wins by native
precedence and the other keeps its own owner — the Phobos radiation activation replacing the retail
one, with the retail substitution kept for builds that lack the capability, is the pattern to
follow.

The build capability is recorded per combination: a save from one combination cannot load in
another, so a mixed build never reads a save written by a single-extension build.
