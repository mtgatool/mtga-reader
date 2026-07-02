# macOS / IL2CPP handoff

This document is for an agent or developer picking up the **macOS (IL2CPP) backend**.
Everything below was built and verified on **Windows (Mono)** during a prior session;
the goal is to bring the macOS backend to parity.

MTGA uses two Unity runtimes depending on platform:
- **Windows / Linux → Mono** (`mono-2.0-bdwgc.dll`). Fully implemented & verified.
- **macOS → IL2CPP** (`GameAssembly.dylib`). Low-level reads exist; the high-level
  typed readers are **not implemented** (stubs).

## What "done" looks like

Implement these five typed readers for IL2CPP so the `.node` addon returns real
data on macOS (today they return `{"error":"read_* not implemented for the IL2CPP backend"}`):

| napi function | returns |
|---|---|
| `readAccount(processName)`   | displayName, accountId, personaId, gameId, email, externalId, countryCode, accessToken |
| `readCollection(processName)`| `{ count, cards:[{grpId, qty}] }` |
| `readInventory(processName)` | gems, gold, wildcards, wcTrackPosition, vaultProgress, land sets |
| `readRanks(processName)`     | constructed + limited class/level/step/record/percentile |
| `readDecks(processName)`     | name, deckId (Guid), Format/attributes, per-pile card lists |

The **Windows/Mono reference implementation is `src/queries.rs`** — read it first.
It's the canonical spec: the object-graph navigation and the data shapes to reproduce.

## The stubs to fill in

`src/napi/mod.rs` → `mod macos_backend` → these functions (search for
"not implemented for the IL2CPP backend"):

```
read_account_impl / read_collection_impl / read_inventory_impl
read_ranks_impl / read_decks_impl
```

The `#[napi]` wrappers already dispatch to them by `cfg(target_os = "macos")`, and
`index.js`/`index.d.ts` already export the functions — no napi surface changes needed.

## Two implementation options

1. **Mirror `queries.rs` for IL2CPP** — create `src/queries_il2cpp.rs`
   (`#![cfg(target_os = "macos")]`) that walks the same object graph using the IL2CPP
   reader + `Il2CppOffsets`, and have the macos `*_impl` call it. Fastest path to parity.
2. **Generalize over the backend trait** (cleaner, bigger) — `src/backend/traits.rs`
   defines `RuntimeBackend` / `TypeDef` / `FieldDef` / `MemoryReader`, and BOTH
   `MonoBackend` and `Il2CppBackend` implement `RuntimeBackend`. Rewrite `queries.rs`
   against `&dyn RuntimeBackend` so one implementation serves both runtimes. More work
   up front; removes the duplication permanently.

Recommended: start with (1) to get it working and verified, then consider (2).

## Existing IL2CPP infrastructure to build on

- `src/il2cpp/` — `Il2CppBackend` (implements `RuntimeBackend`), `offsets.rs`
  (`Il2CppOffsets`, MTGA-tuned for Unity 2021), `metadata.rs`, `macho_reader.rs`,
  `macos_memory.rs` (mach `task_for_pid` + `mach_vm_read`).
- `src/napi/mod.rs` `mod macos_backend` — already has a working `MemReader` (mach),
  `find_class_by_name`, `read_class_name`, `get_class_fields`, `read_dict_entries_il2cpp`,
  and a global `IL2CPP_STATE` (reader + `type_info_table` + class cache). The low-level
  napi functions (`init`, `getInstance`, `getDictionary`, `readData`, …) are wired for macOS.
- Debug binaries (macOS): `src/bin/http_server_il2cpp.rs` (HTTP server, mirror of the
  Mono `http_server_simple`) and `src/bin/test_il2cpp_offsets.rs` (offset verification).
  Both are gated to compile as stubs off-macOS.

## CRITICAL differences: Mono vs IL2CPP

Do **not** assume the Windows offsets transfer. The C# *source* is the same (so field
**names** match), but memory layout differs:

1. **Root singleton differs.**
   - Mono: `WrapperController` static `<Instance>k__BackingField`.
   - IL2CPP (historical): **`PAPA`** is used as the root (see `node-test/demo.js` and the
     `PAPA (+224) -> InventoryManager (+56) -> ...` comment in `http_server_il2cpp.rs`).
   - **Verify on the current build.** Both `PAPA` and `WrapperController` exist; find which
     persistent singleton actually holds `DecksManager` / `InventoryManager` /
     `AccountClient` / `PlayerRankServiceWrapper` under IL2CPP. Scanning the
     `type_info_table` for a class with a live static instance (the macOS analogue of the
     Windows `/singletons` endpoint) is the way to find it.

2. **Field navigation may be by NAME or by OFFSET.** The Mono side resolves fields by
   name (walking the class hierarchy). The existing IL2CPP code partly uses hardcoded
   offsets (`+224`, `+56`, `+72`). Prefer name-based resolution via `Il2CppFieldInfo`
   (`field_name` / `field_offset` in `Il2CppOffsets`) so it survives game updates —
   mirror `queries.rs::field_addr`.

3. **The IL2CPP path arrays in the repo are STALE.** `node-test/demo.js` and
   `http_server_il2cpp.rs` still use `_inventoryServiceWrapper`. On the *current* build
   that field was renamed to **`InventoryServiceWrapper`** (and it now lives on a *base
   class* of `InventoryManager`, so you must walk inheritance). Update these.

4. **Re-derive collection struct offsets.** Object header size and the internal layout of
   `Dictionary<K,V>` / `List<T>` can differ between Mono and IL2CPP. `Il2CppString`
   (length @0x10, chars @0x14) and `Il2CppArray` (length @0x18, elements @0x20) happen to
   match Mono, but **verify Dictionary `_entries`/`_count` offsets and entry strides by
   probing** (dump object heads, like the Windows exploration did).

## The target: navigation paths + layouts (from the verified Mono build)

These are the *semantic* paths (field names are source-level, so they should match on
IL2CPP). Offsets/strides in **(parens)** are the **Mono** values — re-verify each for IL2CPP.

**Root:** `WrapperController.Instance` (Mono) / likely `PAPA` (IL2CPP). Home screen only —
the wrapper/root is unloaded during a match, so these reads return null mid-game.

- **account:** root → `<AccountClient>k__BackingField` → `<AccountInformation>k__BackingField`
  → string fields: `DisplayName`, `AccountID`, `PersonaID`, `GameID`, `Email`, `ExternalID`,
  `CountryCode`, `AccessToken`.

- **inventory:** root → `<InventoryManager>k__BackingField` → `InventoryServiceWrapper`
  (inherited; concrete class `AwsInventoryServiceWrapper`) → `m_inventory`
  (`ClientPlayerInventory`). Read scalars by name: `gems`, `gold`, `wcCommon`, `wcUncommon`,
  `wcRare`, `wcMythic`, `wcTrackPosition` (all i32); `vaultProgress` (**f64/double**);
  `basicLandSet`, `latestBasicLandSet` (strings).

- **collection:** root → `<InventoryManager>k__BackingField` → `InventoryServiceWrapper`
  → `<Cards>k__BackingField` (`CardsAndQuantity` : `Dictionary<uint,int>`). Read dict entries
  → `[{grpId, qty}]`. (Mono: `_entries` @dict+0x18; array len @entries+0x18; data @entries+0x20;
  **16-byte** entries: hashCode@+0, next@+4, key(uint)@+8, value(int)@+12; skip hashCode<0.)

- **ranks:** root → `<PlayerRankServiceWrapper>k__BackingField` → `_combinedRankInfo`
  (`CombinedRankInfo`, `Wizards.Mtga.FrontDoorModels`). Flat fields per format `constructed*`/`limited*`:
  `SeasonOrdinal`(i32), `Class`(enum `RankingClassType`), `Level`, `Step`,
  `MatchesWon/Lost/Drawn`, `Percentile`(string), `LeaderboardPlace`; plus top-level `playerId`.
  `RankingClassType`: 0 None, 1 Spark, 2 Bronze, 3 Silver, 4 Gold, 5 Platinum, 6 Diamond, 7 Master, 8 Mythic.

- **decks:** root → `DecksManager` → `_deckDataProvider` → `_allDecks`
  (`Dictionary<uint, Client_Deck>`; Mono: **32-byte** entries, `Client_Deck*` @entry+0x18).
  Per `Client_Deck`:
  - `_summary` (`Client_DeckSummary`): `Name`(string), `DeckId`(`System.Guid`, 16-byte value type),
    `DeckTileId`(uint), `Description`(string), `Attributes`(`Dictionary<string,string>` —
    holds `Format` e.g. Historic/Timeless/HistoricSingleton100/TraditionalHistoric, plus
    lastPlayed/LastUpdated/favorite/Version).
  - `_contents` (`Client_DeckContents`): `Piles`
    (`Dictionary<EDeckPile, List<{uint grpId, int qty}>>`). Mono: Piles dict **24-byte**
    entries (key(int)@+8, value(List*)@+0x10); each pile `List`: `_items`@+0x10, `_size`@+0x18,
    array data @items+0x20, **8 bytes/elem** {grpId(u32)@+0, qty(i32)@+4}.
  - `EDeckPile` (`Wizards.Mtga.Decks`): 0 Invalid, 1 Main, 2 Sideboard, 3 CommandZone, 4 Companions.

`grpId` is the Arena card id — leave it as-is (the consuming tracker maps ids → card data;
each id is a distinct printing/art, so do NOT collapse them).

## Shared session (match the Mono behavior)

Mono `init()` caches the reader **and** the root class address so polling skips the
expensive assembly/type scan (see `windows_backend::ReaderWrapper` +
`session_or_fresh`). Do the equivalent on macOS: have `macos_backend::init_impl` cache the
IL2CPP root singleton, and route the typed reads through it with a fresh-read fallback.
On Mono this took the light reads from ~3.8s to ~10–20ms.

## How to build & test on macOS

1. Get MTGA running on the Mac, on the **home screen** (not in a match).
2. Build the addon: `npm run build:debug` (debug) — produces `mtga-reader.darwin-*.node`.
   Release: `npm run build`.
3. Reading another process's memory needs `task_for_pid`. Options: run the test as **root**
   (`sudo node test.js`), or sign the host binary with the
   `com.apple.security.cs.debugger` entitlement. SIP may need consideration. Node running
   under `sudo` is the simplest path for iteration.
4. Smoke test the low-level reader first: `init('MTGA')`, then `getAssemblies()` /
   `readData('MTGA', ['PAPA', ...])` to confirm the IL2CPP reader connects and resolves classes.
5. Iterate like the Windows session did: use `http_server_il2cpp` (add `/decks` etc., or a
   generic explore/probe endpoint) so you can walk memory without re-launching each time.
   Dump object heads to re-derive the Dictionary/List offsets for IL2CPP.
6. Verify against known-good values from the account (displayName, gems/gold, deck count,
   collection size) — cross-check with the same account read on Windows if possible.

## Suggested order of work

`account` (strings only) → `inventory` (scalars) → `collection` (one dict) →
`ranks` (flat struct + enum) → `decks` (nested dicts/lists). Each builds on the previous.

## Quick file map

- `src/queries.rs` — **the spec** (Mono impl of all five readers).
- `src/napi/mod.rs` — `mod macos_backend` holds the stubs to implement + IL2CPP low-level reads.
- `src/il2cpp/` — IL2CPP backend, `offsets.rs`, mach memory reader.
- `src/bin/http_server_il2cpp.rs`, `src/bin/test_il2cpp_offsets.rs` — macOS debug tools.
- `node-test/demo.js`, `node-test/test.js` — example calls (their IL2CPP paths are stale — fix them).
- `README.md` — general build/run notes.
