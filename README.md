# binette

binette is a compact binary value format with schemas, stable type identities,
compatibility tooling, and support for long-lived data.

The project is being split out of [vox](https://github.com/bearcove/vox) so
the value format, schema model, schema bundles, compatibility checks, and
translation planning can evolve as a standalone substrate for RPC, storage,
fixtures, and archives.

## Implementations

- `rust/binette`: Rust implementation with Facet schema extraction plus the
  local-access descriptor and stencil/JIT machinery.
- `typescript/binette`: TypeScript implementation, starting with the generic
  self-describing value codec.
- `swift/probes`: Swift probe fixtures and a descriptor-dump executable for
  producing process-local access descriptors consumed by the Rust/binette
  execution engine.

## Local Access

binette schemas and values are the portable wire contract. Runtime layouts are
process-local facts supplied by local access backends. Rust/Facet and Swift
probes are sibling descriptor producers; neither defines the binary format.

Strict optimized execution uses only direct descriptor facts. Hybrid optimized
execution compiles supported subtrees and uses explicit backend thunks only at
unsupported subtree boundaries.

The Swift probe handoff is a tagged descriptor tree. The Rust crate can decode
the JSON form of that handoff through Facet and lower it into the same local
descriptor model used by Rust/Facet.

## Benchmarks

The Rust codec benchmarks are grouped by data shape and execution path:

```bash
cargo bench -p binette --bench codec -- --list
```

To turn Divan output into a browsable report:

```bash
cargo bench -p binette --bench codec -- --color never > /tmp/binette-bench.txt
tools/bench_report.py /tmp/binette-bench.html < /tmp/binette-bench.txt
```
