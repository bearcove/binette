+++
title = "local execution"
description = "Optimized execution modes for local access descriptors"
weight = 18
+++

Local access descriptors are the process-local facts used by binette execution
engines. This specification defines how optimized engines use those descriptors.

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
