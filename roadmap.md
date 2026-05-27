# Roadmap: repair pass for language-neutral local access

This is a **corrective** roadmap, not a feature roadmap. Each step removes a
Rust-shaped shortcut and replaces it with descriptor-driven machinery, then
proves the result with paired Rust + Swift fixtures.

Work top-down. Do not skip ahead — later steps assume the earlier ones
landed. Commit at every checkpoint. If a step produces a negative result,
commit it and report; do not revert and do not delete tests.

## Ground rules

- No `git checkout`/`git reset`/file deletion to "make it clean". Commit
  and discuss.
- No `--no-verify`, no warning suppression, no silently-disabled tests.
- Use `tracing` for diagnostics, never `println!`/`eprintln!`.
- Never re-emit JSON by hand; use facet-json if anything needs serializing.
- Use `cargo info <crate>` to inspect dependencies; don't guess versions.
- Every step ends with a green `cargo nextest run` for the relevant crate,
  and the audit script described in step 0 passing.

## Step 0 — Lock in the audit (do this first, do not skip)

Goal: make drift mechanically detectable so it cannot creep back.

1. Add a small audit test (or a `cargo xtask` / shell script) under
   `rust/binette/tests/` that fails if any of the following appears
   **outside the allowlisted producer modules**:
   - identifier `NicheString`
   - identifier `RustOptionStringBytes`
   - pattern `LocalBackend::RustFacet` used in a `match` arm in
     `src/stencil/**` or in interpreter execution code
   - the literal string `"Option<String>"` in `src/stencil/**`
2. Allowlist: `src/local_access.rs` producer helpers (`rust_facet`
   constructors), the Rust facet descriptor builder, the Swift probe
   importer, and tests/fixtures.
3. Land this audit **before** removing anything. It must be red against
   today's tree; that is the proof it works. Then each later step turns
   one site green.

Exit criteria: the audit runs in CI / nextest and currently reports the
expected set of violations.

## Step 1 — Generalize option representation

The architectural error is that `LocalOptionRepresentation::NicheString`
and `EncodeOptionLayout::NicheString` exist at all. The niche belongs to
the option, not to the payload.

1. In `rust/binette/src/local_access.rs`, replace
   `LocalOptionRepresentation::NicheString { string, none_tag, none_value }`
   with the generic `Niche { tag, tag_width, none_value, some }` plus, on
   the `some` child descriptor, whatever direct-storage facts the existing
   `LocalSequenceStorage` already encodes for a string body. The child
   descriptor for `T` carries its own storage facts; the option only
   carries niche facts.
2. If the current `Niche` variant cannot express a niche whose tag lives
   inside the payload pointer (because tag access and payload access
   overlap), extend `LocalAccess` / `Niche` with the **generic** fields
   needed (e.g. tag access that may alias the some-payload region), not
   with a string-shaped variant. Document the new field in a one-line
   comment on the variant.
3. Update the Rust facet descriptor producer (`RustFacetDescriptorBuilder`
   in `local_access.rs` around L601+) to emit `Niche { … }` for
   `Option<String>` with the same runtime facts it currently encodes into
   `NicheString`.
4. Update the Swift niche option probe path (the work in commits
   `9090c2a` / `c6db98d` / `2ae7ab1`) to emit the same generic `Niche`
   shape. Confirm via the existing Swift niche option fixture.

Exit criteria: `NicheString` is gone from `local_access.rs`. Step 0's audit
shows one violation removed. All decode/encode tests still pass.

## Step 2 — Remove the stencil-level NicheString variants

1. Delete `EncodeOptionLayout::NicheString` from
   `rust/binette/src/stencil/types.rs`.
2. In `src/stencil/compile.rs` (~L1536, L1557, L1592, L2707) replace the
   `NicheString`-shaped emission with the generic `Niche` path that uses
   tag-offset / tag-width / none-value / some-offset from descriptor
   facts. The child stencil for the some payload is whatever the string
   descriptor already produces (direct bytes via `DirectSequenceBytes`).
3. In `src/stencil/aarch64.rs` (~L1168, L1181) delete the `NicheString`
   match arm. The `DirectTag` arm should be the only option-layout arm.
4. Delete `StencilHelper::RustOptionStringBytes` from
   `src/stencil/types.rs` (~L127) and its runtime case in
   `src/stencil/runtime.rs` (~L225, L356). The decode path becomes:
   read tag → branch → if some, run the child string decoder against
   the some-offset, writing `(ptr, len, cap)` via the generic
   `DirectSequenceBytes` op the descriptor already implies.
5. Confirm nothing in `src/stencil/**` writes `Option<String>` through a
   `ptr::write::<Option<String>>(…)`. The construction must be three
   generic field writes (ptr, len, cap) plus the niche-tag write the
   compiler already plans.

Exit criteria: `RustOptionStringBytes` and `EncodeOptionLayout::NicheString`
are gone. The audit script's count drops accordingly. The decode tests
that exercise `Option<String>` still pass, and the Swift niche option
fixture exercises the same compiled path.

## Step 3 — Sweep `LocalBackend::RustFacet` out of execution code

Currently there are 22 references to `LocalBackend::RustFacet`. Many are
legitimate (producer constructors, thunk-binding identifiers, tests).
Some are not.

1. Categorize each of the 22 sites in `rust/binette/src/` into:
   - **Producer**: inside the Rust descriptor builder or its tests. Keep.
   - **Thunk binding**: a `LocalThunk::new(LocalBackend::RustFacet, "…")`
     call paired with a binding registration. Keep.
   - **Execution check**: any `match descriptor.backend { … }` or
     `if backend == RustFacet { … }` in `src/stencil/**`, decode/encode
     dispatch, or interpreter code. Remove.
2. For each execution check, replace it with a query against descriptor
   facts. If the fact does not exist on the descriptor, that is the bug:
   add the fact to the descriptor and have **both** the Rust producer
   and the Swift producer fill it in.
3. Forbid new execution-side `LocalBackend` matches via the step-0 audit.

Exit criteria: `grep -n 'LocalBackend::RustFacet' src/stencil src/decode.rs
src/encode.rs` returns only thunk-name constructions, not control-flow.

## Step 4 — Promote helpers to ordinary thunks

The user's invariant: a helper is a descriptor-provided function. Today
some "helpers" are compiler/runtime special cases. Make them ordinary.

1. Inventory `StencilHelper` and `StencilEncodeHelper` variants. Any that
   exist solely to call back into Rust-specific code for a shape the
   compiler should understand (string bytes, sequence bytes, sequence
   fixed elements when the descriptor already says "direct contiguous")
   must be reduced to a single generic `Thunk { backend, name, … }`
   variant invoked at unsupported subtree boundaries.
2. Strict mode: if a subtree would require a thunk, the strict compiler
   refuses. This is already partly the case; tighten it so strict mode
   does **not** lower any `StencilHelper` variant at all.
3. Hybrid mode: at the unsupported boundary, emit the thunk call. The
   thunk is bound by `(backend, name)` from `LocalThunkBindings`. Neither
   the compiler nor the runtime should know whether the backend is Rust
   or Swift.
4. Move the existing Rust thunk implementations (`Facet.List.len`,
   `Facet.List.element`, `Facet.Option.some`, …) into a Rust-only
   binding module that is *registered* with the runtime, not *named* by
   the runtime.

Exit criteria: strict-mode compilation never selects a `StencilHelper`
variant. Hybrid-mode helper dispatch goes through a single generic thunk
op. The same hybrid path works when bindings are provided by the Swift
side.

## Step 5 — Paired Rust + Swift fixtures per supported shape

For every shape the codebase currently claims to support, add a paired
fixture. The bar for "supported" is **both** sides drive the same
compiled path.

Shapes to cover, in order:

1. Primitives
2. Stored-field structs
3. Nested structs
4. Tuples
5. Fixed arrays
6. Lists / `Vec<T>` (direct contiguous and thunk-backed)
7. Strings (direct bytes and thunk-backed)
8. Options (direct tag, niche, thunk-backed)
9. Enums (unit, fixed payload, sequence payload)

For each shape:

- A Rust descriptor fixture (existing `rust_facet(...)` builders).
- A Swift descriptor fixture (existing probe importer; extend if the
  shape isn't already probed).
- A single test that runs both fixtures through the same
  `stencil::compile` + `stencil::run` entry points and asserts identical
  output for identical wire input.
- A second test asserting strict mode either supports both or rejects
  both with the same `StencilError::Unsupported { reason, … }`.

Exit criteria: a single test file (e.g.
`rust/binette/tests/parity.rs`) iterates the matrix above and is green.
Any row that cannot be made green stays in the test, is marked with the
reason, and is reported — not deleted.

## Step 6 — Honest support matrix and benchmark lanes

1. In `benches/codec.rs`, audit each lane. Any lane labeled "strict" or
   "JIT" that actually exercises an unsupported subtree via helper
   dispatch must be either:
   - relabeled as hybrid, or
   - hidden until step 5 makes it honest.
   (The existing commit `672ba41` "Hide unsupported strict encode
   benchmark lanes" is the right shape; extend it to decode and to all
   hybrid lanes that are secretly helper-dispatch.)
2. Produce a `SUPPORT.md` (or a generated section in an existing doc)
   listing, per shape, which of {strict, hybrid, interpreter} works for
   each of {Rust, Swift}. Generate it from the parity test in step 5 —
   do not hand-maintain it.

Exit criteria: every benchmark lane name accurately describes what it
runs. The support matrix is generated, not asserted by hand.

## Step 7 — Tighten invariants

1. Add a doc comment at the top of `src/stencil/mod.rs` quoting the
   invariants from `goal.md`. Reviewers can point to it.
2. Add a `#![deny(...)]` or a Clippy lint allowlist note that forbids
   importing `facet`-specific types into `src/stencil/**`. If module
   privacy doesn't already enforce this, add a `compile_fail` test.
3. Re-run the step-0 audit. It should now find zero violations.

Exit criteria: clean audit, paired fixtures green, no
`NicheString`/`RustOptionStringBytes`/Rust-typed execution branches
left.

## Order of commits (suggested)

1. Add audit (step 0). Red.
2. Generalize option representation in producer + descriptor (step 1).
   Audit drops one site.
3. Strip `NicheString` from stencil types + aarch64 emission (step 2a).
4. Replace `RustOptionStringBytes` runtime with generic three-write
   construction (step 2b).
5. Per-site sweep of `LocalBackend::RustFacet` execution checks (step 3),
   one logical group per commit.
6. Helper-to-thunk reduction (step 4), one helper variant per commit.
7. Parity fixtures, shape by shape (step 5), one shape per commit.
8. Bench lane and support matrix honesty (step 6).
9. Tighten doc + lints (step 7).

## What success looks like

- `goal.md` invariants 1–7 hold.
- `confession.md`'s "Concrete technical debt" list 1–8 are all done.
- The audit from step 0 is green and stays green.
- Strict mode strictly rejects unsupported shapes with no Rust-shortcut.
- Hybrid mode falls back only at subtree boundaries through generic
  thunk dispatch.
- For every supported shape, Rust and Swift drive the same compiled
  path, proven by a single parity test.

## What to do if a step turns out to be wrong

Commit the negative result and report. Do not revert. Do not start
disabling things to make a failure go away. If the descriptor model
needs to grow a fact you didn't expect, that is a finding, not a
failure — write it up, add the fact, update both producers, and keep
going.
