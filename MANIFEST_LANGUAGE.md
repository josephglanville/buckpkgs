# Manifest Language

For the first prototype, BuckPkgs will use **restricted Starlark** package
definitions that evaluate to a fixed Rust schema.

Strict TOML remains a useful comparison point, but it is not the v0 prototype
format.

Output roles, dependency roles, and payload selection for definitions written
in this language are specified in [PACKAGING.md](./PACKAGING.md).

The target is not "most expressive." The target is the smallest authoring model
that represents real packages cleanly without creating a second general-purpose
package language.

## Non-Negotiable Constraints

Whichever syntax wins:

- the evaluated result must be strict Rust-validated data
- package graph construction may not perform network access or ambient
  filesystem reads
- recipe identity must be canonicalizable for hashing, while built outputs stay
  identified by CAS digests
- code embedded in TOML is not allowed
- unrestricted Starlark is not allowed
- no Nix evaluator is part of the BuckPkgs build path

## Representative Package Corpus

Use packages that match the first product goals and exercise different kinds of
complexity. The bootstrap-to-bash ladder in
[`BOOTSTRAP_TO_BASH_SPIKE.md`](./BOOTSTRAP_TO_BASH_SPIKE.md) is the first
implementation slice; the broader corpus below decides whether the syntax still
holds up once we move beyond the base tools:

| Package | Why it is in the spike |
| --- | --- |
| `gnused` | small tool, mostly declarative |
| `gnugrep` | conditional patch/test logic and runtime shell dependency |
| `gawk` | real variant (`interactive`) plus conditional deps/outputs |
| `bash` | bootstrap-sensitive tool with a custom build flow |
| `sqlite` | native library with multiple outputs and multiple source archives |
| `postgresql` | feature matrix, version family, service-sized package |

This set is intentionally better than toy examples. If a syntax only looks good
for `hello`, it has not proven anything useful.

## Why Starlark For v0

Restricted Starlark is the shortest path to a useful prototype because:

- package definitions are directly visible to ordinary Buck2 analysis
- package graphs can be constructed without an out-of-band generator
- the bootstrap prototype can stay inside normal Buck2 Starlark plus Rust helper
  binaries
- the first real corpus already wants finite conditional composition and shared
  helpers

The Starlark surface is still intentionally narrow:

- package constructors
- typed helper constructors
- finite selectors
- no arbitrary Buck2 rule authoring
- no ambient filesystem or network access during graph construction

## Evaluation Criteria

Score each package and each syntax on:

1. **Directness**: does the package read like its actual build?
2. **Schema pressure**: how many one-off builder knobs or fields were added?
3. **Escape-hatch pressure**: how often did the package need hook files or
   custom scripts?
4. **Conditionals**: are platform/variant conditions still obvious?
5. **Reuse**: can common package families be expressed without copy-paste?
6. **Portability**: can a human translate the matching nixpkgs recipe quickly
   without inventing package-specific schema?
7. **Canonicalization**: can the evaluated form be hashed without ambiguity?
8. **Evaluation cost**: is package-set loading still predictably cheap?
9. **Reviewability**: can a reviewer see what changed without mentally running
   a program?

## Failure Modes To Watch For

### Restricted Starlark Fails If

- ordinary packages require users to understand control flow
- package-set evaluation becomes meaningfully expensive
- helper libraries recreate a fixed-point package language
- arbitrary abstraction makes review harder than the package deserves

## Revisit Criteria

Reconsider the decision only if the actual package corpus shows that:

- ordinary recipes are no longer obviously data-shaped
- review requires mentally executing too much code
- helper layers start recreating Nix-like fixed-point package programming
- package-set evaluation cost becomes a real iteration problem

The thing to avoid is not Starlark itself. The thing to avoid is a general
package language smuggled in under the name of convenience.
