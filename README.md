# PatchQuilt

PatchQuilt is an experimental compatibility layer for loading Quilt server mods beside Pumpkin.

The current implementation uses Quilt Loader 0.30.0 for `quilt.mod.json` discovery, dependency resolution, class loading, mixin initialization, and entrypoint construction. Pumpkin is exposed as the built-in `minecraft` mod at version 26.2. Quilt `init` and `server_init` entrypoints and Fabric `main` and `server` entrypoints are invoked in a dedicated Java runtime managed by a Pumpkin native plugin.

## Current compatibility

Tier 1 is implemented and tested:

- Quilt metadata discovery
- Quilt dependency validation
- Quilt and Fabric initialization entrypoints
- mixin transformation of PatchQuilt-owned game classes
- deterministic startup readiness and shutdown
- Java runtime lifecycle controlled by Pumpkin

Tier 2 is planned:

- a typed local bridge for logging, commands, configuration, scheduling, and server lifecycle events
- stable wrapper objects for players, worlds, entities, blocks, items, and registries
- compatibility tests for server-only utility mods that do not access Minecraft internals

Tier 3 requires substantial compatibility work:

- Quilt Standard Libraries backed by Pumpkin services
- custom registries and network synchronization
- saved-data and resource-pack bridges
- safe server-thread execution for mod callbacks

Tier 4 is not currently supported:

- arbitrary access to `net.minecraft` implementation classes
- mixins targeting Minecraft classes
- bytecode assumptions tied to Mojang server internals
- client-required content mods

## Build

Java 25 and Rust stable are required.

```bash
./gradlew clean build :host:installDist
cargo build --manifest-path rust/Cargo.toml
```

Copy the native library into Pumpkin's `plugins` directory. Copy the contents of `host/build/install/host` into `patchquilt/runtime`. Place Quilt mod jars in `patchquilt/mods`.

Set `PATCHQUILT_JAVA` to the Java executable if `java` is not available on `PATH`.

## Rust supervisor prototype

`supervisor/` contains the no-embedded-Java direction. It launches one Pumpkin
child, attaches to that child with Linux `PTRACE_SEIZE`, resumes it, and relays
its exit status. The supervisor never searches for or attaches to an unrelated
process. This is an interposition boundary for the future Rust bridge; ptrace
does not itself implement a JVM, Quilt Loader, Java bytecode execution, or
Mixin semantics, so those compatibility layers remain explicit follow-up work.

```bash
cargo test --manifest-path supervisor/Cargo.toml
cargo run --manifest-path supervisor/Cargo.toml -- ./pumpkin
```

## Conformance test

```bash
./gradlew :host:installDist :test-mod:jar
mkdir -p run/mods
cp test-mod/build/libs/test-mod-0.1.0.jar run/mods/
printf 'STOP\n' | JAVA_OPTS='-Dpatchquilt.marker=run/lifecycle.marker' host/build/install/host/bin/host --gameDir run
```

The run succeeds when `run/lifecycle.marker` contains `patchquilt_lifecycle_test=1.0.0`.
