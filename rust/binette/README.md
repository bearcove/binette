# binette

binette is a compact binary value format with schemas, stable type identities,
compatibility tooling, and support for long-lived data.

The crate currently implements Facet-to-binette schema extraction, stable type
identities, compact encode/decode with reader translation plans, a
self-describing generic value codec used by schema and dynamic-value work, and
local access descriptors consumed by the stencil/JIT execution path.

Facet is the Rust backend for local access descriptors. Other runtimes, such as
Swift probe fixtures in this repository, feed equivalent descriptors and thunks
into the same binette planning and stencil machinery rather than defining a
separate codec.

Swift descriptor dumps use a tagged, snake_case JSON handoff. The Rust crate
decodes that handoff through Facet into `LocalDescriptorExport` values, then
validates and lowers them into `LocalTypeDescriptor` trees before any stencil
or JIT path consumes them.
