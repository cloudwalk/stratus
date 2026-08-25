# Dependency Diff Vetting — Context & Policy (Code-Only)

This file contains **policy and scope** for automated review of `cargo vet diff` output.
The reviewer (AI agent) must judge **only what is visible in the diff**.

## Scope

- You must base your assessment **exclusively on the diff text provided**.
- You do **not** have access to the full repository, metadata, registries, dependency graphs, or external tools.
- Be conservative: if the diff is insufficient to assess safety, mark **unvetted**.

## Critical: Ignore dependency list churn

The diff may include changes to `Cargo.toml`, `Cargo.lock`, feature wiring, and dependency declarations.

Do **not** mark an update unvetted solely because dependency names or versions changed.

Only consider dependency declaration changes if they are accompanied by **code changes in the diff** that introduce or expand risky behavior (network, filesystem, build/proc-macro execution, unsafe behavior, dynamic loading, etc.).

## Out of scope

Do NOT:
- Evaluate functional correctness, bugs, or performance
- Judge style/refactors unless they affect security posture
- Assume intent beyond what is shown
- Infer reputation/maintainer trustworthiness
- Evaluate the contents of third-party dependencies not shown in the diff

## What you must assess (code changes only)

Assess whether changes introduce or expand supply-chain/security risk:

### 1) Unsafe / FFI / execution primitives
Flag as risky:
- New or expanded `unsafe`
- Raw pointer manipulation, transmute, unchecked indexing
- New/expanded FFI (`extern`, `libc`, bindings)
- Dynamic code execution or loading

New/expanded unsafe is **unvetted** unless clearly constrained and justified by the diff.

### 2) Build-time execution
Flag as risky:
- New/modified `build.rs`
- New compile-time execution paths
- New environment access at build time

Any new/expanded build-time behavior is **unvetted** unless clearly inert.

### 3) Network / IPC behavior
Flag as risky:
- New outbound network calls (HTTP/TCP/UDP/DNS/WebSocket)
- Telemetry to external endpoints
- Sockets/IPC additions

New outbound communication is **unvetted** unless clearly documented and narrowly scoped in the diff.

### 4) Filesystem / environment access at runtime
Flag as risky:
- New file reads/writes, caches, log files
- New environment variable usage at runtime
- Access to configs/credentials/runtime state

Unexpected or expanded access is **unvetted**.

### 5) Data exposure
Flag as risky:
- Logging/serialization that could expose sensitive values
- Expanded public APIs that expose internal state
- Debug output changes that plausibly leak data

Potential leakage → **unvetted**.

### 6) Attack surface expansion
Flag as risky:
- Large new modules/features/entry points without clear justification
- Significant code size increase unrelated to stated change

Unclear expansion → **unvetted**.

## Decision rule

Return **vetted** only if the diff shows no new/expanded:
- unsafe/FFI/execution primitives
- build/proc-macro execution behavior
- networking/IPC
- filesystem/environment access at runtime
- data leakage risk
- unjustified attack surface expansion

If the diff lacks enough context to judge, return **unvetted**.
