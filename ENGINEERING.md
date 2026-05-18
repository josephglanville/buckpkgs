# Engineering

These are project ground rules for implementing BuckPkgs.

## Language And Dependencies

- All non-Starlark code should be written in Rust unless there is a strong,
  concrete reason to use something else.
- Avoid C and C++ library dependencies inside BuckPkgs whenever practical. If the
  needed surface is small and stable, prefer porting the required functionality
  to Rust over pulling native libraries into the implementation.
- Use `clap` for command-line interfaces.
- Use `thiserror` for typed errors throughout the Rust codebase.

## Code Style

For performance-sensitive code and overall architecture, take cues from Andrew
Gallant's work on `regex`, `ripgrep`, and `bstr`:

- explicit data flow
- careful allocation behavior
- measured performance work
- small, composable internals
- correctness-first low-level code
- pragmatic APIs around well-factored cores

For public ergonomics, take cues from David Tolnay's libraries such as `serde`
and `thiserror`:

- good defaults
- clear type-driven APIs
- concise but expressive user-facing surfaces
- errors that explain the problem without forcing callers to decode internals

## Testing

- Prefer fewer high-quality integration tests over many small unit tests when the
  behavior is best understood end-to-end.
- Add focused unit tests when a component or function has enough local edge cases
  that integration tests would be a poor fit.
- Use property-based testing when it naturally matches the problem, especially
  for parsers, normalizers, canonicalization, dependency-role transforms, and
  store-key logic. Do not use it by default when ordinary examples are clearer.
- Verify claims about built binaries against the binaries themselves. If we say
  an output has a particular interpreter, linkage, RPATH/RUNPATH, needed library
  set, or no foreign references, use `readelf` or another format-aware inspector
  plus reference scanning to prove it instead of inferring it from the recipe.

## Performance Posture

- Build the first implementation the fast and simple way when that is enough to
  answer the design question.
- Watch iteration speed closely.
- Once the simple implementation becomes a bottleneck, pivot quickly to a
  well-engineered fast implementation rather than accumulating workarounds around
  slow code.
- Prefer measured rewrites over speculative complexity, but do not stay attached
  to a prototype after it has become the limiting factor.

## Practical Consequences

These rules imply:

- Rust helper binaries are the default escape hatch around Buck2/Starlark.
- Keep helper binaries split by action surface. A change to one builder should
  not perturb the executable identity, action key, or rebuild set for unrelated
  package actions.
- The current `DESTDIR`-then-copy package install flow is a prototype hack to
  preserve final `/pkgs/store/...` prefixes while Buck2 still only sees ordinary
  artifact trees. When BuckPkgs moves into a Buck2 fork, replace that with native
  Buck2 support for the package store rather than normalizing this workaround.
- Normal package builds must keep disposable clean-room workdirs. For local
  package debugging, use Buck2's recorded repro commands from
  `buck2 log what-failed` or `buck2 log what-ran --failed` together with explicit
  debug workdirs rather than making production actions incremental. Current OSS
  Buck2 does not provide a Bazel-style preserved local sandbox mode; local-only
  actions are not sandboxed, while RE exposes failed-input materialization
  instead.
- Native-library bindings in the BuckPkgs implementation itself should be rare and
  justified.
- Early code may be intentionally straightforward, but public formats,
  canonicalization, and store identity logic should be designed carefully from
  the start because they are difficult to change later.
