+++
title = "Schema Bundles"
description = "Self-contained schema documents for compact Binette values"
weight = 15
+++

Compact Binette bytes are meaningful only with a root type and the schemas
needed to interpret that root. A schema bundle is the portable document form
for that context: it can travel beside RPC messages, live in an archive, or be
stored as a compatibility snapshot.

# Bundle Model

> r[binette.bundle.model]
>
> A Binette schema bundle contains:
>
> - `schemas`: a set of Binette schemas
> - `root`: a type reference naming the value type described by the bundle
> - `attachments`: zero or more external attachment kind declarations
>
> The `root` type reference may point to a schema in the bundle, a primitive
> type ID, or a schema already known to the consumer.

> r[binette.bundle.self-contained]
>
> A self-contained bundle contains every non-primitive schema transitively
> referenced by `root`, except schemas explicitly declared to be supplied by an
> existing registry. Self-contained bundles are suitable for long-term storage,
> test fixtures, and schema snapshots because decoding does not depend on a
> connection-local cache.

> r[binette.bundle.registry]
>
> A consumer installs bundle schemas into a schema registry using
> `r[binette.schema.registry+2]` before decoding compact bytes that name the
> bundle root. Duplicate non-identical declarations for the same type ID are
> invalid.

# Encoding

> r[binette.bundle.format]
>
> A schema bundle encoded for interchange is a self-described Binette struct
> with exactly these fields, emitted in this order by canonical encoders:
>
> - `schemas`: list of schema values encoded by `r[binette.schema.format+2]`
> - `root`: type reference encoded by `r[binette.schema.format.type-ref+2]`
> - `attachments`: list of attachment-kind declarations
>
> Decoders identify fields by name using the self-describing struct rules in
> `r[binette.aggregate.struct.self-describing]`. Canonical bundle encoders MUST
> NOT emit extra fields.

> r[binette.bundle.attachments]
>
> An attachment-kind declaration contains:
>
> - `kind`: the external kind string used by `r[binette.schema.external]`
> - `metadata_schema`: optional type reference describing attachment metadata
>
> The declaration describes envelope-level attachment semantics. It does not
> alter any schema type ID.

# Dumps And Snapshots

> r[binette.bundle.dump]
>
> A schema dump is a bundle plus producer metadata that is not part of the core
> schema model. Tooling MAY attach producer metadata such as source-language
> names, field defaultability, documentation strings, or source locations.
> Producer metadata MUST NOT affect Binette type IDs.

> r[binette.bundle.snapshot]
>
> Compatibility tools compare schema snapshots, not live values. A snapshot
> records the bundle roots and producer metadata needed to answer whether data
> written with one schema set can be read with another schema set.
