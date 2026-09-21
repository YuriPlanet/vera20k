# Rust guidelines — condensed

The rules agents get wrong, taken from Microsoft's *Pragmatic Rust Guidelines*
(<https://microsoft.github.io/rust-guidelines/agents/all.txt>, 2026-09-15, MIT) and the
[Rust API Guidelines checklist](https://rust-lang.github.io/api-guidelines/checklist.html).
Ids link to the full text. `AGENTS.md` wins on conflict: gamemd-exact semantics,
determinism and state authority outrank style.

## Porting

- **M-RUST-SHAPED** Port domain behavior, not C++ constructs. A striking technical
  resemblance to the original is an architectural smell.
- **M-SINGLE-ITEM-PATH** One public path per item; iterative refactors leave stale
  re-exports alive — redesign instead.
- **M-NO-META-DESIGN-DOCUMENTATION** Docs describe the end state — no design journeys or
  tables of guidelines followed.
- **M-TAUTOLOGICAL-TESTS** Tests assert properties, never the constant they read back.

## Correctness

- **M-PANIC-ON-BUG** Detected programming errors and contract violations panic, with
  reason and values — never an `Error` nobody can act on. Fallible operations return `Result`.
- **M-UNSAFE / M-UNSOUND** `unsafe` only for novel abstractions, benchmarked performance
  or FFI, with written safety reasoning and Miri. Unsound safe code has no exceptions.
- **M-STRONG-TYPES-GUARD** A newtype encoding an invariant enforces it: fallible
  constructor, `TryFrom`, never an infallible `From`.
- **M-AVOID-STATICS** No `static`/thread-local where a consistent view matters for
  correctness.

## Performance

- **M-HOTPATH** Benchmark hot paths, profile CPU *and* allocations, document hot spots.
  Typical wins: fewer re-allocations and `format!` strings, fewer collection clones,
  no re-hashing, fast hasher for trusted keys.
- **M-AVOID-INDIRECTION** Hot types avoid nested heap indirection; lift hot fields next
  to their reader.
- **M-MEM-REUSE** Callers reuse buffers (`get_in(id, &mut out)`, `.clear()`, arenas);
  this outranks the API Guidelines' `C-NO-OUT` in hot loops.
- **M-THROUGHPUT** Items per CPU cycle: batch, partition ahead, exploit locality; share
  state only when cheaper than recomputation.
- **M-MIMALLOC-APPS** Applications set `mimalloc` as global allocator.

## API shape

- **M-AVOID-WRAPPERS** No `Arc<Mutex<T>>`, `Rc<RefCell<T>>` or `Box<T>` in signatures;
  service types nest type parameters ≤1 level.
- **M-DI-HIERARCHY** Concrete types > generics > `dyn Trait`. Test doubles are an enum
  (`Native | Mock`), not a trait object.
- **M-APP-ERROR** One `anyhow`-style error type for the app, never mixed.
- **Forgotten API Guidelines items:** `C-CONV` (`as_/to_/into_`), `C-GETTER`,
  `C-COMMON-TRAITS`, `C-CTOR` (`new()` even with `Default`), `C-CUSTOM-TYPE`
  (no bare `bool`/`Option` arguments).

## Verification

- **M-MOCKABLE-SYSCALLS / M-TEST-UTIL** Files, clocks, entropy and seeds are mockable
  via a private `enum Core { Native, Mocked(MockCtrl) }`; test hooks behind one
  `test-util` feature.
- **M-LINT-OVERRIDE-EXPECT** `#[expect(lint, reason = "…")]`, not `#[allow]`.
- **M-STATIC-VERIFICATION** Beyond default clippy: `undocumented_unsafe_blocks`,
  `clone_on_ref_ptr`, `unused_result_ok`, `map_err_ignore`,
  `allow_attributes_without_reason`, `missing_debug_implementations`,
  `unsafe_op_in_unsafe_fn`. Adopt per module; `pedantic` crate-wide is not viable.
