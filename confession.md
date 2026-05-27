# Confession: binette local access and JIT drift

This document records the ways my implementation work deviated from the stated
goal, so the next agent can build a corrective roadmap without having to infer
the failure modes from scattered conversation.

## The goal I was supposed to serve

The active goal was to refocus binette JIT around a language-neutral local
access architecture, with Rust and Swift as sibling backends.

The important architectural constraints were:

- The binette schema/value model is the wire contract.
- JIT, interpreter, and stencil execution consume binette plans plus explicit
  local type/access descriptors.
- Rust Facet is one producer of descriptors, not the implicit architecture.
- Swift probe metadata is a sibling producer of descriptors.
- Swift must feed descriptors and accessors into the same Rust/binette
  machinery, not into a separate codec.
- Strict mode means no helpers.
- Hybrid mode means compile supported subtrees and fall back only at unsupported
  subtree boundaries, through descriptor-provided functions.
- Runtime layout facts are process-local observations, not persistent ABI
  promises.

I did not hold those constraints tightly enough.

## Main deviations

### I let Rust remain the implicit architecture

I introduced and used a descriptor layer, but I did not consistently force the
execution paths to consume only descriptor facts. Several paths still encode
Rust-specific assumptions directly in the stencil/compiler/runtime layer.

The worst examples are:

- `LocalOptionRepresentation::NicheString`
- `StencilHelper::RustOptionStringBytes`
- runtime code that casts output memory to `Option<String>`
- Rust-specific naming and behavior in option/string decode helpers

Those are not backend-neutral concepts. They should either be facts produced by
the Rust descriptor producer or generic descriptor concepts usable by any
backend.

### I treated Swift as proof-of-concept coverage instead of a sibling backend

Swift support exists mostly as fixtures, imports, and selected tests. That is
not the same as carrying Swift as a first-class sibling through every supported
shape.

The desired model was simple:

- Rust hands binette descriptors.
- Swift hands binette descriptors.
- The compiler consumes descriptors.
- The compiler should not care which backend produced the descriptor, except
  when binding a thunk by backend/name.

I failed to enforce that model as the acceptance bar for each new supported
type shape. As a result, Swift lags behind Rust even when the runtime machinery
should only need different descriptors.

### I allowed type-specific hacks to masquerade as architecture

`NicheString` is the clearest example.

Many `Option<T>` values can have niche representations. The niche belongs to
the option representation, not to `String`. A string payload is just one child
descriptor that may itself use direct contiguous storage or thunks.

The architecture should describe generic option representation facts:

- direct tag or niche tag
- tag access
- tag width
- none value or none initialization recipe
- some payload access
- child descriptor for `T`
- optional backend thunks when direct construction is not available

Instead, I created a special case for `Option<String>` and let execution code
know about that Rust type directly. That violates both language neutrality and
descriptor-driven execution.

### I blurred helper semantics

The user clarified, and the goal says, that strict mode means no helpers. Hybrid
helpers are not language-specific escape hatches; they are descriptor-provided
functions used at unsupported subtree boundaries.

I did improve some paths toward recursive hybrid compilation, but I still left
helpers that are special compiler/runtime cases rather than ordinary
descriptor-provided functions. That makes it too easy to solve hard layout work
by calling back into Rust instead of making the descriptor model strong enough.

### I let naming hide architectural debt

Some renames were legitimate, such as moving direct contiguous sequence decode
away from Rust-specific names after the code stopped requiring `RustFacet`.

But I also tolerated names that expose real debt:

- `NicheString`
- `RustOptionStringBytes`
- Rust-specific helper paths in otherwise generic execution code

Names like that are not cosmetic. They show where the architecture is still
wrong.

### I did not keep the Swift and Rust support matrix honest enough

The code supports a range of Rust descriptor lowering:

- primitives
- structs
- tuples
- fixed arrays
- lists/`Vec<T>`
- options
- enums

Swift fixtures cover useful pieces:

- stored-field structs
- nested structs
- strings via thunks and at least one direct descriptor test
- arrays via thunks
- optionals in some direct/thunk forms
- enums through tag/project/construct thunks

But I did not make "Rust and Swift both drive the same compiler path" the
definition of done for each shape. That was the wrong bar.

### I let benchmarks get ahead of architecture clarity

Benchmark lanes are useful only if they mean what they claim.

Current broad shape:

- encode has wider strict/JIT coverage than decode
- decode strict lanes exist for fixed-ish shapes, enums, and nested structs
- decode list/tuple/option/mixed-struct paths still rely on hybrid/interpreter
  lanes in important cases
- set/map/dynamic are interpreter-only

That is not itself a sin. The sin was allowing "hybrid works" to blur whether
hybrid meant the intended recursive non-strict JIT model or merely helper
dispatch.

## Concrete technical debt to fix first

1. Remove `NicheString` as a descriptor concept.
2. Replace it with generic option niche/direct-tag representation facts.
3. Remove `RustOptionStringBytes` as a runtime/helper concept.
4. Make option decode construction use descriptor facts or descriptor-provided
   thunks.
5. Audit stencil/compiler/runtime for `LocalBackend::RustFacet` checks.
6. Keep backend-specific code inside descriptor/probe producers or thunk
   binding, not in execution architecture.
7. Add paired Rust and Swift descriptor fixtures for each supported shape.
8. Require each new support claim to answer: would this same compiler path work
   if Swift handed equivalent descriptors?

## Invariants for the next roadmap

- Descriptor facts are the boundary.
- Backends produce descriptors and thunks.
- Execution consumes descriptors and thunks.
- Strict mode rejects unsupported shapes.
- Hybrid mode falls back only at unsupported subtree boundaries.
- A helper is a descriptor-provided function, not a Rust shortcut.
- Rust-specific layout probing belongs in Rust descriptor production.
- Swift-specific layout probing belongs in Swift descriptor production.
- Shared execution code should not know Rust or Swift type identities.
- A type shape is not truly supported until Rust and Swift can exercise the same
  generic machinery, unless the roadmap explicitly marks it as Rust-only or
  Swift-only for a temporary reason.

## Bottom line

I built part of the intended skeleton, then repeatedly took Rust-shaped
shortcuts when the hard cases appeared. That directly violated the goal. The
next work should be a disciplined repair pass, not more feature expansion on
top of those shortcuts.
