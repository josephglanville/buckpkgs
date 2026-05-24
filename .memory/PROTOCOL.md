# Reproducibility Campaign Protocol

## Objective

Drive BuckPkgs package realization toward byte-for-byte reproducibility while
the bootstrap tree is rebuilt from the local Buck2 fork.

Implement store substitution for finalized bootstrap outputs without reconnecting
ordinary consumers to the live bootstrap turnover graph.

Extend ordinary native package realization with named outputs and declared
`pkg-config` discovery so higher-level ports can depend on explicit,
hermetic library interfaces.

## Standing Rules

- Prefer source-backed diagnosis over speculation.
- Treat store mismatches as evidence; compare stale and fresh artifacts before
  removing any poisoned `/pkgs/store/...` path.
- Fix root causes in package realization or package definitions, not by adding
  recovery paths that accept partial or divergent outputs.
- Keep output publication atomic: artifacts are either fully present or absent.
- Seal staged package outputs at finalization. Buck2 must preserve those modes
  for store outputs rather than applying ordinary writable-output normalization,
  and must validate them while copying required metadata during atomic
  publication. Never repair existing objects on normal reuse.
- If a store invariant changes after a path was published, change semantic
  package identity and rebuild rather than mutating the existing object.
- Keep edits scoped to reproducibility, native package interfaces, and sealed
  bootstrap consumption needed by higher-layer ports.
- Keep store-path identity, substitute transport identity, and realized-tree
  identity distinct.
- Fold every declared value that can change installed bytes, including
  descriptor-backed install arguments, into package store identity.
- Model package-specific shared-library linkage with declared `link_inputs`;
  recipes select the library interface while realization supplies store-backed
  link lookup, RUNPATH, and runtime closure.
- Model package metadata discovery with declared `PkgConfigInfo` search roots;
  dependency roles choose the applicable `pkg-config` environment, and builders
  must set `PKG_CONFIG_LIBDIR` so ambient host metadata is not visible.
- Emit named package outputs from the same realization action as the primary
  output, fold split policy into identity, and repair relocated `*.pc` root
  variables before sealing. Projected outputs do not silently inherit runtime
  dependencies from the primary output; declare a `bin` to primary `lib`
  runtime edge explicitly when an executable projection loads sibling shared
  libraries.
- For newly ported packages, use `bin` for runnable programs, `lib` for
  runtime shared libraries plus indispensable loaded runtime data, `dev` for
  headers/static archives/build metadata, and `out` only for compound runtime
  payloads that cannot yet be separated without losing required runtime
  closure modeling. Defer renaming established bootstrap-facing outputs until
  that sealed interface is intentionally revised.
- Do not create `man`, `doc`, or `info` projections by default; those require
  an explicit package need.
- Prefer explicit keep-lists for primary outputs on newly ported mixed-content
  packages. Never discard all of `share` mechanically: retain runtime data
  such as terminal databases or PostgreSQL installed data explicitly.
- Do not add an ordinary-build fallback that silently rebuilds the bootstrap
  island when a substitute is absent.
- Treat the established `bootstrap/foreign_seed` and pinned substitute closure
  as sealed: building new higher-layer tools should use native package
  derivations, and extending either bootstrap surface requires explicit user
  approval. The approved GNU awk/grep/patch extension is consumed only through
  pinned imports.
- Live bootstrap producers and foreign seed wrappers must use the explicit
  `BOOTSTRAP_PRODUCER_VISIBILITY` allowlist. Ordinary package-facing labels
  must resolve through `BOOTSTRAP_SUBSTITUTE_VISIBILITY`-guarded imports, not
  live staged outputs or transport labels.
- Apply the same visibility restriction to producer-side verification targets
  and keep per-object export targets internal to the export/test surface; an
  output stamp or exporter is still a dependency bridge into its inputs.
- Declare native package build-tool inputs as execution dependencies so normal
  package actions and package-backed toolchains share published tool imports.
- Treat promoted self-hosting tools as publication generations: build a
  candidate from the pinned façade, pin its verified export, and do not expect
  the private producer target to retain that identity after the façade moves.
- Do not publish a reduced language-runtime build tool under the canonical
  full-runtime label; reserve `python:bin` for a feature contract comparable to
  normal Nixpkgs `python3`.

## Validation Gates

- Formatter and targeted Rust tests for `pkgs-tool` changes.
- Targeted Buck2 builds for packages whose realization contract changed.
- For split metadata packages, build/replay each exported output and execute a
  native `pkg-config` query that resolves headers and libraries only through
  declared output roots.
- Store-path scans for workspace paths, scratch paths, host-tool paths, and
  tool-specific metadata leaks after each confirmed fix.
- Read-only mode inspection for each newly republished package on the validated
  path; reproducibility comparisons may ignore only write-bit differences
  between a source-sealed store tree and a Buck-normalized replay artifact, and
  must retain executable-bit checks.
- Full bootstrap target rebuild for broad confirmation:
  `//bootstrap/tests:final_base_seed_free`
  `//bootstrap/tests:final_base_pkgs_interpreters`
- Targeted import/hydration tests for substitute manifest, archive, and
  imported-provider changes.
- A graph-boundary check demonstrating imported consumer targets do not depend
  on live turnover labels.
- An analysis-time visibility check for the publication graph and representative
  normal consumers after any bootstrap target visibility change.
- For a bootstrap publication change, compare the generated closure/object
  manifests byte-for-byte with the reviewed pinned files under
  `bootstrap/substitutes/<system>/`.
- Hydrate a pinned closure into a disposable store root before relying on the
  ordinary `pkgs_hydrated_store_output(...)` native import path.

## Reviewer Policy

Use the running rebuild plus artifact inspection as the primary adversarial
check. If a suspected source of nondeterminism cannot be tied to emitted files
or a deterministic contract, keep it on the board rather than patching it.

## Incident Stops

- Stop and diagnose if a Buck2 store mismatch appears on a freshly rebuilt
  package after the current reproducibility fixes are active.
- Stop and diagnose if a package publishes host-specific absolute paths, random
  identifiers, or build-time metadata into its final store payload.
