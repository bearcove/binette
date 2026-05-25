# binette

binette is a compact binary value format with schemas, stable type identities,
compatibility tooling, and support for long-lived data.

The crate currently implements Facet-to-binette schema extraction, stable type
identities, compact encode/decode with reader translation plans, and a
self-describing generic value codec used by schema and dynamic-value work.
