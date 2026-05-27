+++
title = "local access"
description = "Process-local type access descriptors for binette execution engines"
weight = 17
+++

binette schemas and binette values are the wire contract. A programming
language runtime still needs a way to read and write its own local values when
encoding, decoding, interpreting, or generating code. That bridge is a local
access backend.

A local access backend is not a new serialization format. It is process-local
metadata that describes how one runtime representation maps to a binette schema
inside the current process.

# Boundary

> r[binette.local-access.boundary]
>
> binette execution engines consume two independent inputs:
>
> - a binette schema and compatibility plan, which define the byte-level value
>   contract
> - a local access descriptor, which defines how to read or write the current
>   process's runtime representation for that schema
>
> The schema/value model is portable. The local access descriptor is not
> portable and is not part of the encoded bytes, schema hash, schema bundle, or
> compatibility result.

> r[binette.local-access.backends]
>
> Rust/Facet metadata and Swift runtime probes are sibling producers of local
> access descriptors. Neither backend defines the architecture of the execution
> engine.
>
> A backend may produce direct memory facts, accessor thunks, or a mixture of
> both. Execution engines consume those facts through the descriptor model
> rather than by depending directly on the source-language reflection API.
>
> Backends that are not linked into the Rust type system, such as Swift, feed an
> owned descriptor tree into binette. binette validates that the tree is
> internally consistent for its declared backend before any interpreter, hybrid,
> or strict optimized engine consumes it.

# Descriptor model

> r[binette.local-access.descriptor]
>
> A local access descriptor for a type contains:
>
> - the binette schema reference it maps to
> - the producing backend
> - local layout facts such as size, alignment, and stride when relevant
> - the value kind's local access shape: scalar, struct fields, enum variants,
>   sequence elements, option representation, external attachment, or opaque
>   fallback
> - direct memory access facts where validated
> - backend-provided accessor or thunk fallbacks where direct memory is not
>   available or not worth assuming
> - child descriptors for nested local values
>
> Direct memory facts are offsets, strides, tags, or pointers that have been
> observed or proven for this process. Accessor thunks are explicit calls owned
> by the backend; an engine may use them in interpreted or hybrid execution but
> not in strict JIT execution.
>
> A descriptor names required thunks, but executable hybrid code also requires
> the embedding backend to bind those names to callable process-local function
> pointers before compilation succeeds. An unbound thunk is not a valid implicit
> fallback.
>
> Every descriptor node consumed by an execution engine must agree with the
> schema or plan node it is lowering. A backend-provided helper is not allowed
> to change the binette value kind of a subtree; for example, a descriptor for a
> local byte buffer cannot satisfy a string plan node merely because both use
> length-prefixed bytes on the wire.
>
> Decode-side fallback thunks construct or write the local representation for
> the unsupported subtree. Encode-side fallback thunks project local bytes or
> elements from the same subtree. Both directions are explicit backend calls.
> Text and byte-string primitives remain scalar descriptors; their local access
> shape may still include byte-sequence storage facts or thunks. They are not
> reclassified as aggregate sequences merely because the backend projects their
> bytes through sequence-like accessors.
> For a sequence subtree, a backend may provide count and element projection
> thunks; hybrid execution may then encode the sequence by combining those
> process-local projections with the descriptor-derived element encoding.
> Decode-side sequence construction may similarly use a backend write thunk
> after the engine decodes fixed-width elements into process-local element
> layout.
> For an optional subtree, a backend may provide direct tag and payload access
> facts, or presence/projector thunks for encode and construction thunks for
> decode. Direct optional access identifies the tag location, the local none and
> some tag values, and the payload location for the some case. Hybrid execution
> treats the optional node as the fallback boundary unless the engine has proven
> direct local layout facts for that optional representation.
> For an enum subtree, a backend may provide a tag thunk plus per-variant
> payload projectors; hybrid execution writes the binette variant index and
> encodes the projected payload using the variant payload descriptor.
> Decode may use per-variant constructor thunks at the same enum subtree
> boundary, after binette has identified the writer variant and prepared the
> payload representation expected by that backend constructor.

> r[binette.local-access.runtime-facts]
>
> Runtime layout facts are process-local observations. They MUST NOT be cached
> on disk, embedded into portable artifacts, or treated as promises made by a
> compiler or language ABI. A new process constructs or validates its own local
> access descriptors before using direct memory access.

# Execution modes

> r[binette.local-access.strict-hybrid]
>
> Strict optimized execution emits only code that is covered by schema facts,
> plan facts, and direct local access descriptor facts. If any required subtree
> needs a backend helper, accessor thunk, or interpreter call, strict optimized
> construction fails before execution.
>
> A strict engine may be built from a compatibility or writer plan plus a local
> descriptor without requiring the descriptor producer's reflection API at code
> generation time. Executing such code is only valid for live values that match
> the descriptor used to compile it.
>
> Hybrid optimized execution is recursive non-strict execution. It attempts to
> compile each node or subtree normally. If a subtree cannot be compiled from
> direct descriptor facts, the engine may emit one explicit backend-provided
> fallback for that unsupported subtree, then continue compiling supported
> siblings and children. The fallback boundary is the unsupported subtree itself;
> supported siblings before and after that subtree remain native code.
>
> A hybrid report distinguishes native subtrees from fallback subtrees so
> benchmark results can show which work is still capable of becoming faster.

# Swift probes

> r[binette.local-access.swift-probes]
>
> A Swift backend produces local access descriptors by probing and validating
> the current process's Swift runtime representation. Representative probe
> coverage includes stored-field structs, nested structs, enums with payloads,
> optionals, arrays, strings, and fallback accessor/thunk cases.
>
> Swift support in binette does not define a separate Swift-native binette
> codec. Swift feeds local descriptors and accessors into the same binette
> schema, planning, interpreter, and code-generation machinery as other
> backends.
>
> A Swift descriptor handoff includes the same information as any other local
> access descriptor: schema reference, local layout, stored-field offsets where
> available, enum variant projectors, sequence/optional access, and explicit
> thunk names for cases that require Swift-owned accessors. The handoff is
> rejected if Swift descriptor nodes contain thunks from another backend. A
> Swift backend may export this handoff as a tagged descriptor tree. If the
> handoff crosses an FFI or process boundary as JSON, Rust/binette decodes it as
> typed Facet data before lowering it into runtime descriptors. That export is
> metadata for binette engines, not encoded binette data and not a Swift codec.
