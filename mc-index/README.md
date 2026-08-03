# mc-index

Extracts a structured index (package, class/interface/enum/record declarations, fields,
methods, `extends`/`implements`) from a decompiled `net.minecraft` source tree.

Used for two things:

- PatchQuilt Stage 4 ("mapped Minecraft surfaces"): a versioned reference of what the vanilla
  server actually exposes, without re-scraping raw decompiled source on every lookup.
- Pumpkin conformance checking: `conformance/map_coverage.py` currently only matches Rust
  struct/enum names against vanilla class names. A structured index makes signature-level
  comparison (method names, field types) possible.

## Usage

```sh
cargo run --release -- <path-to-decompiled-net/minecraft> <output.json> [version-label]
```

Example:

```sh
cargo run --release -- /tmp/pumpkin-vanilla-26.2/decompiled/net/minecraft net-minecraft-26.2.json 26.2
```

## Known limitations

This is a best-effort brace-depth scanner, not a full Java parser. It cannot see inside method
bodies (by design - that's how it avoids capturing local variables as fields), and it does not
capture:

- fields whose initializer itself contains a brace (`int[] a = {1, 2}`, anonymous class bodies)
- enum constant lists (only members declared after the constants are indexed)
- multiple classes/interfaces declared with the same simple name in different files sharing a
  qualified name collision (the index does not deduplicate across files)

These gaps only cause under-counting; they never crash the scan or produce a corrupted index.
