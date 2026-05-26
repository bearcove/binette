# binette

binette is a compact binary value format with schemas, stable type identities,
compatibility tooling, and support for long-lived data.

The project is being split out of [vox](https://github.com/bearcove/vox) so
the value format, schema model, schema bundles, compatibility checks, and
translation planning can evolve as a standalone substrate for RPC, storage,
fixtures, and archives.

## Implementations

- `rust/binette`: Rust implementation with Facet schema extraction.
- `typescript/binette`: TypeScript implementation, starting with the generic
  self-describing value codec.
