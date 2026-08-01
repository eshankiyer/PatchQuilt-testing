# PatchQuilt compatibility plan

PatchQuilt is developed in compatibility stages. A stage advances only after its automated conformance tests and an isolated Pumpkin integration test pass on the NUC.

## Stage 1: Loader lifecycle

Implemented in the initial patch:

- discover Quilt mods and validate their metadata and dependencies
- expose Pumpkin as the built-in Minecraft 26.2 game provider
- invoke Quilt `init` and `server_init` entrypoints
- invoke Fabric `main` and `server` compatibility entrypoints
- start and stop the Java runtime with the Pumpkin plugin lifecycle
- verify the lifecycle using a deterministic test mod

This stage supports loader-only initialization code. It does not provide Minecraft implementation classes or Pumpkin server APIs to mods.

## Stage 2: Pumpkin service bridge

- define a versioned local protocol with capability negotiation
- bridge logs, configuration, commands, scheduling, and server lifecycle events
- enforce server-thread affinity for callbacks that mutate game state
- add stable wrappers for players, worlds, entities, blocks, items, and registries
- test disconnects, timeouts, malformed requests, shutdown, and reload behavior

The target for this stage is server utility mods written specifically against the PatchQuilt bridge.

## Stage 3: Quilt libraries

- implement selected Quilt Standard Libraries on top of the service bridge
- add saved-data, resources, events, registries, and networking adapters
- publish an explicit support matrix for each API module
- add representative upstream mod fixtures for every supported module

The target for this stage is server-only Quilt mods that stay within supported public APIs.

## Stage 4: Minecraft compatibility

- provide mapped `net.minecraft` surfaces required by selected mods
- remap compatible mod jars into the PatchQuilt runtime
- translate registry and packet objects across the process boundary
- evaluate mixin targets individually and reject unsafe transformations

This stage is required for most existing Quilt mods and is expected to be the largest part of the project.

## Stage 5: Custom content

- synchronize custom registries and resources with compatible clients
- bridge custom blocks, items, entities, recipes, and data components
- validate persistence and protocol compatibility across upgrades

Client-required content will only be considered supported when both server and client behavior have automated compatibility coverage.

## Acceptance gates

Every compatibility addition must include a minimized mod fixture, deterministic assertions, strict Java and Rust builds, and an isolated NUC run through Pumpkin. Changes that regress an established fixture do not advance the compatibility level.
