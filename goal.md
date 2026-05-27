# Goal: language-neutral local access in binette

Binette is a wire-level schema/value system. JIT, interpreter, and stencil
execution must be driven entirely by binette plans plus **local type and
access descriptors**. Rust Facet and Swift probes are two **sibling
producers** of those descriptors; nothing else.

## What "done" means

A change is in service of this goal iff all of the following hold:

1. **The wire contract is unchanged.** Binette schema/value semantics stay
   the way they are. Only local access machinery is in scope.
2. **Execution code is backend-blind.** Stencil, compiler, runtime, and
   interpreter modules do not mention `RustFacet`, `Swift`, `String`,
   `Option<String>`, or any other backend-specific type identity. They
   consume `LocalTypeDescriptor` / `LocalAccess` / `LocalThunk` only.
3. **Backend-specific knowledge lives in backend producers.** Anything that
   needs to know about `Option<String>` niche packing, `Vec<T>` layout,
   Swift existential boxes, etc. lives in either:
   - the Rust descriptor producer (`facet`-driven path), or
   - the Swift descriptor producer (probe-driven path).
4. **Strict mode means no helpers.** Strict JIT either compiles a shape
   directly from descriptor facts or rejects it. There is no Rust-shaped
   shortcut.
5. **Hybrid mode means recursive compilation with thunk boundaries.**
   Supported subtrees compile; at unsupported subtree boundaries the
   compiler emits a call to a **descriptor-provided thunk** named by
   `(backend, name)`. A helper is just a thunk binding, not a special
   compiler/runtime case.
6. **Rust and Swift exercise the same compiler path.** For each supported
   shape there is a paired fixture: Rust-produced descriptors and
   Swift-produced descriptors flow through the same stencil/compiler/runtime
   code and produce equivalent results.
7. **Runtime layout facts are local observations.** Anything probed at
   runtime (offsets, niches, tag widths) is treated as a process-local
   measurement, not as a stable ABI promise. Probes belong in producers.

## Non-goals

- New supported shapes beyond what the producers already describe.
- Encoder/decoder format changes.
- Performance work that requires backend-specific code in shared modules.
- Backwards compatibility with the current `NicheString` /
  `RustOptionStringBytes` names. They are scheduled for removal.

## Anti-goals (things that will get reverted on sight)

- Any new `match` on `LocalBackend::RustFacet` inside `src/stencil/**`,
  `src/local_access.rs` execution paths, or interpreter code.
- Any new variant named after a Rust type (`*RustOption*`, `*String*`,
  `*Vec*`) in shared execution enums.
- Adding "hybrid helpers" that compile to anything other than a generic
  thunk call dispatched by `(backend, name)`.
- Disabling tests or fixtures to make the audit pass. If a shape regresses,
  commit it and report.

## Invariants the next agent must hold

- Descriptor facts are the boundary.
- Backends produce descriptors and thunks; execution consumes them.
- Strict rejects unsupported shapes; hybrid falls back only at subtree
  boundaries via descriptor-provided thunks.
- Shared execution code does not know Rust or Swift type identities.
- A shape is "supported" only when **both** Rust and Swift descriptor
  fixtures drive the same compiler path through it, unless explicitly
  marked Rust-only or Swift-only with a written reason.
