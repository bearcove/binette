# binette

binette is a compact binary value format with schemas, stable type identities,
compatibility tooling, and support for long-lived data.

The crate currently implements Facet-to-binette schema extraction, stable type
identities, compact encode/decode with reader translation plans, a
self-describing generic value codec used by schema and dynamic-value work, and
local access descriptors consumed by the stencil/JIT execution path.

Facet is the Rust backend for local access descriptors. Swift feeds equivalent
plain C descriptor structs and callable thunks into the same binette planning
and stencil machinery rather than defining a separate codec. JSON descriptor
dumps still exist as inspection fixtures, not as the execution boundary.
