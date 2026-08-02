# macOS / IL2CPP backend

MTGA uses two Unity runtimes depending on platform:

- **Windows / Linux (Wine) → Mono** (`mono-2.0-bdwgc.dll`) — `src/mono/`, `src/queries.rs`.
- **macOS → IL2CPP** (`GameAssembly.dylib`) — `src/il2cpp/`, `src/queries_il2cpp.rs`.

Both backends expose the same NAPI surface and return the **same JSON shapes**, so
callers never branch on platform.

## Status

All five typed readers work on macOS and are verified against a live build
(macOS 15 / arm64):

| reader | verified output |
|---|---|
| `readAccount` | displayName, accountId, personaId, gameId, email, externalId, countryCode, accessToken |
| `readInventory` | gems, gold, wildcards, wcTrackPosition, vaultProgress, land sets |
| `readCollection` | 9925 unique / 25407 copies |
| `readRanks` | constructed + limited class/level/step/record |
| `readDecks` | 176 decks with names, Guid ids, attributes, per-pile card lists |

`readData` / `readClass` / `getClassDetails` / `getDictionary` also run on the
IL2CPP runtime, so the low-level explorer APIs and the live-match paths
(`MatchSceneManager` → `Instance` → `_matchManager`) work too.

**Timings:** attach ~3.9 s once (scans `GameAssembly.__DATA` for the type-info
table); afterwards account/inventory/ranks ~13 ms, collection ~35 ms, decks ~70 ms.

## Requirements

Reading another process's memory needs `task_for_pid`, which fails with
KERN_FAILURE (5) unprivileged. Either:

1. **run as root** (`sudo node app.js`) — simplest for development; or
2. **sign the host binary with `com.apple.security.cs.debugger`** (Developer ID +
   notarization). This is the shipping path for an Electron app.

Option 2 works because MTGA itself is **not** hardened —
`codesign -d --verbose=2 MTGA.app` reports `flags=0x0(none)`. A hardened target
without `get-task-allow` could not be attached to without disabling SIP, so
**re-check this flag after client updates**.

Verified: even an **ad-hoc** signature is enough, so no Apple account is needed
just to develop. `entitlements.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.cs.debugger</key><true/>
  <key>com.apple.security.get-task-allow</key><true/>
  <key>com.apple.security.cs.disable-library-validation</key><true/>
</dict></plist>
```

For a plain binary:

```bash
codesign -s - -f --options runtime --entitlements entitlements.plist ./my-binary
```

For an **Electron** host (e.g. running the tracker in dev mode) — sign the whole
bundle with `--deep` and **without** `--options runtime`. Ad-hoc + hardened
runtime makes Electron's nested dylibs fail library validation and the app dies
with `SIGTRAP` on launch:

```bash
codesign -s - -f --deep --entitlements entitlements.plist \
  node_modules/electron/dist/Electron.app
```

The entitlement must reach the **renderer** helper too, not just the main
process — that's where the tracker calls the reader. `--deep` covers it.

Because of this, `isAdmin()` on macOS reports *"can we read game memory"* — it
returns true when running as root **or** when `task_for_pid` actually succeeds —
rather than `geteuid() == 0`. Callers gate the whole reader on it, so a uid check
would disable a perfectly capable entitled build.

## Design: metadata-driven, not offset-driven

`src/il2cpp/macos_runtime.rs` deliberately hardcodes as little as possible:

- **Images** come from dyld (`TASK_DYLD_INFO` → `dyld_all_image_infos`). Do *not*
  scan VM-region starts for the Mach-O magic — `mach_vm_region` coalesces adjacent
  regions with matching attributes, so a dylib header often isn't at a region start.
- **The type-info table** is found by scanning `GameAssembly.__DATA` for a pointer to
  an array of structurally-validated classes. Its offset changes per build, so it is
  never hardcoded.
- **The `Il2CppClass` layout** is detected by locating the `klass` self-pointer.
- **Fields** resolve by name from `FieldInfo`, walking the inheritance chain.
- **Container strides** come from metadata: dictionary `Entry` field offsets from the
  `Entry` class itself, array element sizes from `Il2CppClass.element_size`.

The practical payoff: a game update that reorders fields or moves the table doesn't
require new offsets.

## Verified layout constants

```
Il2CppClass:  name 0x10   namespace 0x18   element_class 0x40   cast_class 0x48
              parent 0x58   klass(self) 0x78   fields 0x80   static_fields 0xB8
              instance_size 0xF8   element_size 0x104
FieldInfo (32 bytes): name 0x00  type 0x08  parent 0x10  offset 0x18  token 0x1C
Il2CppType:   attrs = read_u32(type+0x08) & 0xFFFF
              type_code = (read_u32(type+0x08) >> 16) & 0xFF
Object header 0x10.  String: len 0x10, chars 0x14.  Array: len 0x18, data 0x20.
```

Two of these are easy to get wrong, and both fail *silently*:

- `static_fields` is **0xB8**, not `0xA8`. `0xA8` is `implementedInterfaces`, which
  returns a plausible non-null pointer — reads "succeed" and yield garbage. Getting
  this wrong is what forced the original code to brute-force scan the heap for a
  `PAPA` instance instead of simply reading `WrapperController.<Instance>k__BackingField`.
- `element_size` is **0x104**, not `0x100` (which reads 0). Note `0xFC` reads a
  constant `8` for *every* array class, so a "first non-zero word" search silently
  makes every stride 8. `array_element_size` therefore only accepts a value that
  matches either a pointer (8) or the element class's inline size.

Also note `element_class` (0x40) and `cast_class` (0x48) point back at the class for
ordinary types, so "first offset that self-references" finds 0x40 — not the `klass`
slot. Disambiguate by requiring the pointer right after the anchor to be a `FieldInfo`
array whose `parent` points back at the class.

## Object graph

Root: `WrapperController.<Instance>k__BackingField` (same root Windows uses;
`PAPA._instance` also works and is kept as a fallback). Null during a match, since
the wrapper scene is unloaded — the typed readers return a "home screen" error, and
live-match data comes from `MatchSceneManager` instead.

- **account** → `<AccountClient>k__BackingField` → `<AccountInformation>k__BackingField`
- **inventory/collection** → `<InventoryManager>k__BackingField` → `InventoryServiceWrapper`
  (declared on a *base* class, concrete type `AwsInventoryServiceWrapper`) →
  `m_inventory` / `<Cards>k__BackingField`
- **ranks** → `<PlayerRankServiceWrapper>k__BackingField` → `_combinedRankInfo`
- **decks** → `DecksManager` → `_deckDataProvider` → `_allDecks`

Observed container layouts (all read from metadata at runtime — listed for reference):

| container | layout |
|---|---|
| `CardsAndQuantity : Dictionary<uint,int>` | hashCode 0 / next 4 / key 8 / value 12, stride 16 |
| `_allDecks : Dictionary<Guid, Client_Deck>` | key 8 (16-byte Guid) / value 24, stride 32 |
| `Piles : Dictionary<EDeckPile, List<Client_DeckCard>>` | enum key, `List` value |
| `Client_DeckCard` | `{ Id u32 @0, Quantity u32 @4 }`, stride 8 |

Dictionary iteration is bounded by `_count` (used high-water mark), not capacity.

> Note: `_allDecks` is keyed by **Guid**, not `uint` as earlier notes claimed.
> `constructedPercentile`/`limitedPercentile` are **floats** on this build, not
> strings; they are read type-aware so either works.

## Re-verifying after a game update

`src/bin/probe_il2cpp.rs` is the tool for this — it dumps the detected layout, the
root singleton, and the full field table at every hop of the object graph.

```bash
cargo build --bin probe_il2cpp          # build as your user...
sudo ./target/debug/probe_il2cpp        # ...run the binary as root
```

Then check the typed readers end to end:

```bash
cargo build --bin test_readers_macos
sudo ./target/debug/test_readers_macos
```

Or through the Node addon: `npm run build:debug && node scripts/macos-smoke.js`.

**Never run `sudo cargo run`** — it leaves root-owned artifacts in `target/`, and
later user-level builds then fail with a misleading
`ld: can't write output file: .../libmtga_reader.dylib`.
