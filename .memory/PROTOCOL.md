# Reproducibility Campaign Protocol

## Objective

Drive BuckPkgs package realization toward byte-for-byte reproducibility while
the bootstrap tree is rebuilt from the local Buck2 fork.

## Standing Rules

- Prefer source-backed diagnosis over speculation.
- Treat store mismatches as evidence; compare stale and fresh artifacts before
  removing any poisoned `/pkgs/store/...` path.
- Fix root causes in package realization or package definitions, not by adding
  recovery paths that accept partial or divergent outputs.
- Keep output publication atomic: artifacts are either fully present or absent.
- Keep edits scoped to reproducibility and bootstrap support.

## Validation Gates

- Formatter and targeted Rust tests for `pkgs-tool` changes.
- Targeted Buck2 builds for packages whose realization contract changed.
- Store-path scans for workspace paths, scratch paths, host-tool paths, and
  tool-specific metadata leaks after each confirmed fix.
- Full bootstrap target rebuild for broad confirmation:
  `//bootstrap/tests:final_base_seed_free`
  `//bootstrap/tests:final_base_pkgs_interpreters`

## Reviewer Policy

Use the running rebuild plus artifact inspection as the primary adversarial
check. If a suspected source of nondeterminism cannot be tied to emitted files
or a deterministic contract, keep it on the board rather than patching it.

## Incident Stops

- Stop and diagnose if a Buck2 store mismatch appears on a freshly rebuilt
  package after the current reproducibility fixes are active.
- Stop and diagnose if a package publishes host-specific absolute paths, random
  identifiers, or build-time metadata into its final store payload.

