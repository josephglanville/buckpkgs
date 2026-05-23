# Reproducibility Campaign Protocol

## Objective

Drive BuckPkgs package realization toward byte-for-byte reproducibility while
the bootstrap tree is rebuilt from the local Buck2 fork.

Implement store substitution for finalized bootstrap outputs without reconnecting
ordinary consumers to the live bootstrap turnover graph.

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
- Keep edits scoped to reproducibility and bootstrap support.
- Keep store-path identity, substitute transport identity, and realized-tree
  identity distinct.
- Fold every declared value that can change installed bytes, including
  descriptor-backed install arguments, into package store identity.
- Model package-specific shared-library linkage with declared `link_inputs`;
  recipes select the library interface while realization supplies store-backed
  link lookup, RUNPATH, and runtime closure.
- Do not add an ordinary-build fallback that silently rebuilds the bootstrap
  island when a substitute is absent.
- Treat the established `bootstrap/foreign_seed` and pinned substitute closure
  as sealed: building new higher-layer tools should use native package
  derivations, and extending either bootstrap surface requires explicit user
  approval.
- Do not publish a reduced language-runtime build tool under the canonical
  full-runtime label; reserve `python:bin` for a feature contract comparable to
  normal Nixpkgs `python3`.

## Validation Gates

- Formatter and targeted Rust tests for `pkgs-tool` changes.
- Targeted Buck2 builds for packages whose realization contract changed.
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
- For a bootstrap publication change, compare the generated closure/object
  manifests byte-for-byte with the reviewed pinned files under
  `bootstrap/substitutes/<system>/`.
- Hydrate a pinned closure into a disposable store root before relying on the
  ordinary `pkgs_hydrated_store_output(...)` projection path.

## Reviewer Policy

Use the running rebuild plus artifact inspection as the primary adversarial
check. If a suspected source of nondeterminism cannot be tied to emitted files
or a deterministic contract, keep it on the board rather than patching it.

## Incident Stops

- Stop and diagnose if a Buck2 store mismatch appears on a freshly rebuilt
  package after the current reproducibility fixes are active.
- Stop and diagnose if a package publishes host-specific absolute paths, random
  identifiers, or build-time metadata into its final store payload.
