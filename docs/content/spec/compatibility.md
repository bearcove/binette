+++
title = "Compatibility"
description = "Schema evolution and translation planning for binette values"
weight = 16
+++

binette schemas are stable enough to store compact bytes for a long time, but
schemas still evolve. Compatibility is the question: can a value written using
one schema be read using another schema without losing the value's meaning?

# Translation Plans

> r[binette.compat.plan]
>
> A translation plan maps a writer schema to a reader schema. It is built before
> compact payload decode. If a plan cannot be built, the payload is incompatible
> with the requested reader type and decoding MUST NOT begin.

> r[binette.compat.field-matching]
>
> Struct fields are matched by field name, not by declaration position. The
> translation plan maps writer field positions to reader field positions before
> compact bytes are decoded.

> r[binette.compat.skip-unknown]
>
> Fields present in the writer schema but absent from the reader schema are
> skipped during decode by walking the writer schema for that field. Compact
> binette does not add per-field length wrappers solely to make field skipping
> trivial.

> r[binette.compat.fill-defaults]
>
> Fields present in the reader schema but absent from the writer schema require
> a reader-side default provider. Default providers are supplied by the
> language mapping or embedding tool; the writer schema does not decide whether
> the reader can fill a missing field.

> r[binette.compat.defaultability-metadata]
>
> Schema dumps MAY carry defaultability metadata for compatibility analysis.
> Defaultability metadata is not part of the binette schema model and is not
> type-ID hash input. Tooling SHOULD distinguish at least these states:
>
> - no default provider
> - default provider exists but is opaque to portable tooling
> - literal default value is available as a binette value
>
> Opaque defaults can make local decoding possible without making generated
> cross-language code able to reproduce the default.

> r[binette.compat.type-compat]
>
> Matched field types are compatible only when a more specific compatibility
> rule says they are compatible. Numeric widening is not implicit compatibility
> unless a future rule adds an explicit widening conversion.

> r[binette.compat.type-compat.basic]
>
> Non-enum matched field types are compatible when they have the same primitive
> type, the same container kind with compatible child types, compatible struct
> plans, or the same tuple arity with compatible element types.

# Enum And Tuple Evolution

> r[binette.compat.enum]
>
> Enum variants are matched by variant name, not by variant index. The
> translation plan maps writer variant indices to reader variant indices.

> r[binette.compat.enum.unknown-variant]
>
> A writer enum variant absent from the reader schema is skippable as schema
> structure, but receiving that variant at runtime is a decode error for that
> value.

> r[binette.compat.enum.missing-variant]
>
> A reader enum variant absent from the writer schema is compatible: the writer
> cannot produce that variant.

> r[binette.compat.enum.payload]
>
> Variants present in both schemas must have compatible payload shapes: unit
> with unit, newtype with newtype, tuple with tuple, and struct with struct.

> r[binette.compat.tuple]
>
> Tuples are positional. Writer and reader tuple schemas are compatible only
> when they have the same arity and pairwise-compatible element types.

# Reports

> r[binette.compat.report]
>
> A compatibility checker reports direction. For two schema snapshots `old` and
> `new`, it distinguishes at least:
>
> - backward compatible: `new` can read `old`
> - forward compatible: `old` can read `new`
> - bidirectionally compatible: both directions work
> - incompatible: at least one required translation plan cannot be built
>
> Reports SHOULD identify the schema path and reason for each incompatibility.
