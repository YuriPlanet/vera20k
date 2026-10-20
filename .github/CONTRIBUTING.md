# Contributing to VERA20K

The [README's Contributing section](../README.md#contributing) is the starting
point. Code, refactors, tests, gameplay comparisons and ideas are welcome.
This guide collects optional local checks; it does not add acceptance
requirements or apply the maintainer's agent workflow to outside contributors.

## Set up a checkout

Follow [Build and run](../README.md#build-and-run) for Rust, platform build tools
and game configuration. The repository selects stable Rust in
[`rust-toolchain.toml`](../rust-toolchain.toml). Keep `Cargo.lock` when building
so the checkout uses its recorded dependency versions.

Work on a branch for one concern. If using multiple agents or checkouts, give
each change its own branch and file scope, and avoid competing builds in a
shared Cargo target directory.

`config.toml` is ignored because it contains local paths. Keep retail game
files and machine-specific configuration out of commits. The repository and
its releases contain no game files.

## Choose checks for the change

Run commands from the repository root. The manifests split the engine into
`vera20k-core` (assets, rules, maps, simulation and networking) and `vera20k`
(application, renderer, UI, audio and asset tools).

| Purpose | Command | Source |
| --- | --- | --- |
| Check Rust formatting | `cargo fmt --all -- --check` | rustfmt |
| Run a focused core test | `cargo test --locked -p vera20k-core --lib <test-filter>` | [`src/Cargo.toml`](../src/Cargo.toml) and the test name |
| Run a focused client test | `cargo test --locked -p vera20k --lib <test-filter>` | [`Cargo.toml`](../Cargo.toml) and the test name |
| Run both library suites | `cargo test --workspace --lib` | [`test.yml`](workflows/test.yml) |
| Lint libraries and binaries | `cargo clippy --workspace --lib --bins` | [`rust.yml`](workflows/rust.yml) |

Replace `<test-filter>` with the relevant module or test name and check that
the run actually selected tests. Select features explicitly when the change
concerns an optional capability, such as `--features ares`; default tests do
not establish that capability's behavior.

Formatting is usually quick. The first test or lint command compiles
dependencies and can take many minutes; the library workflow notes about
21 minutes for a cold Windows build. Warm focused tests are usually much
quicker. Let an existing build finish before starting an equivalent one.

The Windows CI jobs set
`CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=rust-lld` to use the linker shipped
with rustup. This is optional locally and still requires the Windows C++
build prerequisites from the README.

Some tests require the ignored `ini/` directory or retail assets and report
`SKIPPED` when those inputs are absent. A passing fresh-clone suite therefore
does not establish retail-data coverage. State which inputs and platform
were used, and distinguish unit tests from gameplay or native comparisons.

## Open a scoped pull request

Explain the problem, the resulting behavior and the issue it addresses.
Keep unrelated fixes separate, and include:

- The scope and any known limitations.
- Exact checks run and their results, including skipped, failed or unrun checks.
- Updated documentation when behavior or contributor commands change.
- Reproduction steps or relevant sanitized evidence for visible behavior.

The library, Clippy and Python tool workflows are started manually. Opening
a PR does not start them automatically, and their absence is not a green CI
result. Cross-platform checks are maintainer-run; see the workflow definitions
for their current coverage.

Discuss ideas or uncertain scope on [Discord](https://discord.gg/kmjRUn5m5F),
or in the relevant GitHub issue. A PR can be reviewed before every platform
or gameplay scenario has been checked; record those remaining checks honestly.
