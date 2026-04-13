# mtga-reader local fork — investigation notes

This is a **local fork** of [`mtgatool/mtga-reader`](https://github.com/mtgatool/mtga-reader), built from HEAD
(`v0.1.6` unreleased as of **2026-04-10**) with local patches so it compiles
and runs on macOS arm64. Originally forked into `/tmp/mtga-reader-head` during
a `commander-tuner` `mtga-import` integration investigation, then moved here
for durability.

The fork is **not currently wired into anything**. `commander-tuner`'s
`mtga-import` removed its `--collection-source mtga-reader` option because the
`--collection-source untapped-csv` path ended up being a cleaner,
no-privileges-required alternative. These notes exist so the work is
recoverable if someone wants to pick it back up later.

## UPDATE 2026-04-11: arena_id → set/collector_number now works

`readMtgaCardDatabase("MTGA")` (new napi export) returns **23,694 rows** of
`{ grpId, set, collectorNumber, titleId }` in ~11 seconds, covering Arena's
entire in-process card database. Every grp_id from `readMtgaCards` resolves
to a real set code + collector number. From there, downstream name
resolution can hit `https://api.scryfall.com/cards/{set}/{number}` which
returns cards even when Scryfall's `arena_id` field is null — closing the
gap that originally drove us to the `untapped-csv` workaround.

The rest of this document has large sections that were written before this
work landed and are now stale. See the dedicated "UPDATE 2026-04-11"
subsections below for the corrections. The headline change: we now get
set + collector number directly from Arena's own memory and resolve names
via Scryfall's `/cards/{set}/{number}` endpoint (which works even for
cards where Scryfall's `arena_id` field is null), so the old framing
around needing a third-party name database no longer applies.

## What works

- Builds cleanly on `darwin-arm64` with `npm install && npm run build`
  (produces `mtga-reader.darwin-arm64.node`, already present in the repo root
  from the last build).
- `readMtgaCards("MTGA")` (custom napi function added in this fork) scans
  Arena's live process memory, finds the `Cards` dictionary via a signature
  scan, and returns `{ cards: [{cardId: int, quantity: int}, ...] }`.
- `readMtgaCardDatabase("MTGA")` (**new 2026-04-11**) returns the full
  in-process card database as
  `{ cards: [{grpId, set, collectorNumber, titleId}, ...] }`, ~23.7k rows.
  Combine with `readMtgaCards` for a complete `grp_id → (quantity, set,
  collector_number)` mapping, then resolve names via Scryfall
  `/cards/{set}/{number}`. Set `MTGA_DEBUG_CARD_DB=1` to enable verbose
  stderr diagnostics (field dumps, byte hexdump of the first entry, etc.).
- `readMtgaInventory("MTGA")` (**new 2026-04-11**) returns the current
  player's wildcard counts plus gold, gems, and vault progress from the
  `ClientPlayerInventory` singleton:
  `{ wcCommon, wcUncommon, wcRare, wcMythic, gold, gems, vaultProgress }`.
  Ground-truth verified against Arena's UI (37/11/1/1 wildcards,
  825 gold, 610 gems, 58.9% vault). `vaultProgress` is a decimal
  percentage in the `0.0 – 100.0` range matching the UI exactly — DO NOT
  multiply or divide. Set `MTGA_DEBUG_INVENTORY=1` for verbose
  diagnostics (class variant enumeration, candidate dump, raw vault
  bytes under multiple type interpretations).
- Both scans are fast (<12s wall clock) and deterministic against a running
  Arena process.
- Requires `sudo` to run because `task_for_pid` on macOS needs elevated
  privileges unless the calling binary is signed with
  `com.apple.security.cs.debugger`, which requires an Apple developer
  entitlement we don't have. Sudo is effectively mandatory for local use.

## What doesn't

- **The returned dict is per-printing (grp_id), not per-oracle.** A card like
  Lightning Strike with 6 printings has 6 separate entries, each at whatever
  physical-copy count the user acquired for that printing. Downstream code
  needs to aggregate by oracle name and cap at 4 for deckbuilding use. (The
  Python side of `mtga-import` handles this in `_resolve_collection` — see the
  `sum + cap at 4` comment block.)
- **Scryfall's `default_cards.json` has null `arena_id` for many recent Arena
  printings** (Alchemy Y-sets, Avatar TLA/TLE, Final Fantasy, Lorwyn Eclipsed,
  etc.) so downstream name resolution misses ~1500 cards when going through
  `arena_id` alone. **This no longer blocks us** — `readMtgaCardDatabase`
  now returns `set` + `collector_number` for every card, and
  `/cards/{set}/{number}` on Scryfall resolves cards even when `arena_id`
  is null.
- **The PAPA walker is broken on current Arena builds, but it no longer
  matters.** Both `readMtgaCards` and `readMtgaCardDatabase` reach their
  target dicts via direct heap signature scans, bypassing PAPA entirely.
  The walker itself (`find_papa_instance_by_field_verification` in
  `src/napi/mod.rs:2149`) finds ~200 slots where the klass pointer matches
  PAPA_class but verifies 0 as real PAPA instances — either the scanned
  heap regions don't cover the GC-managed region where the real singleton
  lives, or the verification strategy (comparing class pointers instead of
  class names, which `find_wrapper_controller_instance` documents as the
  robust approach) is wrong. Fixing it would unlock walking
  `InventoryManager` / `EventManager` / `MatchManager` fields from PAPA for
  game-state reading, but it is not on the critical path for card data.

### UPDATE 2026-04-11: the CardPrintingRecord layout claim was wrong

The field-layout table below ("Replicating option (a)…") originally said
`CardPrintingRecord` is the runtime class the card DB dictionary holds,
with `GrpId@0x10`, `TitleId@0x20`, `ExpansionCode@0x50`,
`CollectorNumber@0x70`. That's only half right:

1. **The runtime dict value class is `CardPrintingData`, not
   `CardPrintingRecord`.** `CardPrintingData` is a wrapper with ~47 fields
   for cached computed values (`_convertedManaCost`, `_isLand`, etc.) and
   an **embedded CardPrintingRecord struct** at offset `0xC0` under a field
   literally named `Record`.
2. **`Record` is a value-type struct, not a pointer.** Its 352-byte
   footprint (offset `0xC0` to `0x220`) matches CardPrintingRecord's own
   field layout (`Blank@0x0`..`AdditionalFrameDetails@0x150`). There is no
   indirection to dereference.
3. **When embedded as a struct, Il2CppObject header size (16 bytes) drops
   out.** The class-level field offsets from `get_class_fields(cpr_class)`
   include the header (that's why `GrpId` reads as `0x10` on the standalone
   class — `0x10` = the 16-byte header). When the struct is inlined, there
   is no header; so the effective offset on the wrapper is
   `record_offset + (class_field_offset - 0x10)`. For the current build:
   - `GrpId` → `0xC0 + (0x10 - 0x10)` = `0xC0`
   - `TitleId` → `0xC0 + (0x20 - 0x10)` = `0xD0`
   - `ExpansionCode` → `0xC0 + (0x50 - 0x10)` = `0x100`
   - `CollectorNumber` → `0xC0 + (0x70 - 0x10)` = `0x120`
4. **`ExpansionCode` and `CollectorNumber` are `Il2CppString*`, not C
   strings.** Layout: `klass(8) + monitor(8) + length(i32) + utf16_chars`.
   NOTES originally described them as "pointer to a string like `'tle'`"
   which is true but under-specified — you need to decode them as UTF-16
   managed strings. The new `read_il2cpp_string` helper in
   `src/napi/mod.rs` handles this.
5. **Heap-instance classes are a different `Il2CppClass*` than the metadata
   variant `find_class_by_direct_scan` returns.** The metadata class for
   `CardPrintingRecord` (what we find by scanning `__DATA`) is at a
   different address than the runtime class `CardPrintingData` instances
   reference. Comparing by class POINTER fails; comparing by class NAME
   works. This confirms the comment in `find_wrapper_controller_instance`
   about IL2CPP keeping separate structs for "metadata table entry" vs
   "runtime vtable owner."
6. **`get_class_fields` has a 50-entry hard limit and will read off the
   end of classes with >50 fields into adjacent class metadata.** Not a
   problem for CardPrintingRecord (exactly 50 fields, all valid), but on
   `CardPrintingData` (which has 47 real fields plus the Record struct
   plus two tail pointers) the last few entries returned by
   `get_class_fields` are garbage names picked up from an adjacent class.
   Work around it by looking fields up by NAME — the first match is
   always the real one because class-internal fields come before the
   out-of-bounds overflow.

## Local patches on top of upstream HEAD

All in `src/napi/mod.rs` and `src/mono_reader.rs`. None have been sent upstream.

1. **`MonoReader::is_admin` — macOS branch added**
   (`src/mono_reader.rs:48-75`). Upstream's function had branches for Windows
   (via `is_elevated` crate) and Linux (via `sudo::check()`) but no macOS
   branch, so the function body is `()` on macOS and doesn't match the
   declared `-> bool` return type. The crate doesn't compile on macOS as
   published. Our patch adds a `#[cfg(target_os = "macos")]` branch that
   uses `libc::geteuid() == 0`.

2. **`MemReader::read_{u64,i64,u16,i16,i8,f32,f64}` methods added**
   (`src/napi/mod.rs` in the `macos_backend` module's `MemReader` impl). The
   local `MemReader` struct upstream only defines `read_u8/u32/i32/ptr`, but
   `read_field_value` further down in the file calls all the missing ones.
   Seven missing methods = seven build errors on a cold `cargo build`. Each
   fix follows the same `from_le_bytes` pattern as the existing methods.

3. **`scan_heap_for_cards_dictionary` + `read_cards_dictionary_entries`**
   (`src/napi/mod.rs` in `macos_backend`). Signature scan that walks writable
   heap regions looking for a `Dictionary<int, int>`-shaped object with:
   - `count` in `[500, 50_000]`
   - `buckets_ptr` and `entries_ptr` in a plausible heap range
   - First 30 entries have `hash == key` (the defining signature of a .NET
     `Dictionary<int, TValue>` with the default `EqualityComparer<int>`,
     since `GetHashCode(x) == x` for int)
   - Keys in Arena card-id range `[1, 200_000]`
   - Values in `[1, 4]` (Arena's internal cap)
   This uniquely identifies the live card collection dict across 200k+
   candidate positions. Scoring supports optional `MTGA_KNOWN_CARD_IDS` and
   `MTGA_VERIFY_QTYS` env vars for cross-validation if you're debugging.

4. **`read_mtga_cards_impl` + `readMtgaCards` napi export**
   (`src/napi/mod.rs`, napi export section at the bottom of the file). Public
   entry point that calls the scanner and returns the Cards dictionary's
   contents. Bypasses all of upstream's PAPA walker / WrapperController
   walker / InventoryManager walker / field-walk machinery.

4c. **`read_mtga_inventory_impl` + `readMtgaInventory` napi export**
   (`src/napi/mod.rs`, added 2026-04-11). Heap-signature-scan reader
   for `ClientPlayerInventory`. Strategy:
   - `find_all_classes_by_name("ClientPlayerInventory")` enumerates
     every `Il2CppClass*` in `__DATA` with that name (handles the
     metadata-vs-runtime class duplication preemptively).
   - `resolve_inventory_field_offsets` looks up `wcCommon`,
     `wcUncommon`, `wcRare`, `wcMythic`, `gold`, `gems`,
     `vaultProgress` by name with multiple candidate-name fallbacks
     (bare name, `<…>k__BackingField`, WildCard-prefixed log-style,
     underscore-prefixed).
   - `scan_heap_for_client_player_inventory` uses the class-pointer
     set as a strong pre-filter (no per-slot `read_class_name` cost,
     which is what made the naive approach unusable) and then
     applies `inventory_fields_look_plausible` — wildcards in
     `[0, 99_999]`, gold `[0, 10^9]`, gems `[0, 10^7]`, with a
     non-zero signal requirement to reject uninitialized / metadata
     false positives. `inventory_activity_score` breaks ties in
     favor of the live instance over cached/backup copies.
   - Multiple ClientPlayerInventory objects usually exist on the
     heap (active + cached); the activity score correctly identifies
     the live one (e.g. in testing, the winning instance had score
     1485 vs a stale zombie at score 76).

   **Key layout correction** vs. `IL2CPP_RESEARCH_SUMMARY.md`:
   `vaultProgress` is an **8-byte `double`**, not an `int32`. Field
   spacing in the class metadata (`vaultProgress @ 0x30`,
   `boosters @ 0x38`) confirms 8 bytes. The stored value is the UI
   percentage directly (e.g. `58.9` for "Vault: 58.9%"). Reading it
   as `int32` gives `0x33333333 = 858_993_459`, which is just the
   low half of the `double` bit pattern
   `0x404d733333333333`. Other fields on the class have shifted too:
   `wcTrackPosition @ 0x28` is 8 bytes wide in the current build
   (the summary said 32), and class now has 18 fields total vs. the
   older shorter layout.

4b. **`read_mtga_card_database_impl` + `readMtgaCardDatabase` napi export**
   (`src/napi/mod.rs`, added 2026-04-11). Same heap-signature-scan approach
   as `readMtgaCards` but looking for a `Dictionary<int,
   CardPrintingData*>` instead of `Dictionary<int, int>`. Six new helpers:
   - `read_il2cpp_string` — UTF-16 `Il2CppString*` decoder
   - `RuntimeCardFieldOffsets` + `resolve_runtime_card_field_offsets` —
     resolves the absolute field offsets on whichever class the dict
     actually holds (handles both `CardPrintingRecord` directly and
     `CardPrintingData` with its embedded-struct Record field)
   - `find_card_database_instance` (fallback, unused when heap scan
     succeeds) — PAPA-walker-based CardDatabase locator
   - `find_card_printing_dictionary` (fallback) — enumerates
     `CardDatabase` fields to find the printing dict
   - `scan_heap_for_card_printing_dictionary` (**primary path**) —
     heap-scans for a Dictionary whose value class NAME matches
     `CardPrintingData` or `CardPrintingRecord`. Two-pass: first filters
     by `hash==key` Dictionary invariant with stride 24
     (`hash+next+key+pad+value_ptr` = `4+4+4+4+8`), then resolves the
     observed value classes by name to work around the IL2CPP metadata-
     vs-runtime class duplication.
   - `read_card_printing_entries` — walks the found dict and returns
     `(grp_id, value_ptr)` pairs.
   All gated behind `MTGA_DEBUG_CARD_DB=1` for verbose stderr output.

5. **Various diagnostic functions — `scan_for_type_info_table`,
   `find_class_by_direct_scan`, `dump_class_names_matching`,
   `find_papa_instance_via_static_field`, `find_wrapper_controller_instance`,
   `find_papa_instance_by_field_verification`, `probe_card_printing_record`,
   `scan_for_dict_entry_pattern`.** All dead code now — used during the
   reverse-engineering to learn about Arena's memory layout. Feel free to
   delete if you're cleaning up for an upstream PR, but they're useful
   reference for how to probe specific aspects of Arena's in-process state.

## CardPrintingRecord field layout (historical reference)

Captured via our own `probe_card_printing_record` function in
`src/napi/mod.rs`, which calls `get_class_fields(cpr_class)` on the live
running Arena process. Reading IL2CPP metadata at the class's
`class_fields` offset walks the `FieldInfo[]` array Arena itself populates
at startup. This is authoritative for whatever Arena build is currently
running.

> **⚠️ Stale on the current build.** The dict that downstream code walks
> no longer holds `CardPrintingRecord*` directly — it holds
> `CardPrintingData*` which embeds a `CardPrintingRecord` struct at offset
> `0xC0` (under a field literally named `Record`). The field offsets
> inside the embedded struct are still accurate; what changed is the
> wrapper. See the `readMtgaCardDatabase` UPDATE subsection for how the
> live code finds and walks the dict. This table is retained because the
> field names and semantic types (which fields are int vs string vs
> array vs dict) are still correct for the embedded struct.

**Class**: `CardPrintingRecord` in Assembly-CSharp. 50 fields,
confirmed via `get_class_fields()` on current Arena:

| Offset | Name | Notes |
|---|---|---|
| `0x00` | `Blank` | static sentinel |
| `0x10` | **`GrpId`** | **int — this is Scryfall's `arena_id`** |
| `0x14` | `ArtId` | int |
| `0x18` | `ArtPath` | pointer to string |
| `0x20` | **`TitleId`** | **int — NOT a string; index into a localization table** |
| `0x24` | `InterchangeableTitleId` | int |
| `0x28` | `AltTitleId` | int |
| `0x2c` | `FlavorTextId` | int |
| `0x30` | `ReminderTextId` | int |
| `0x34` | `TypeTextId` | int |
| `0x38` | `SubtypeTextId` | int |
| `0x40` | `ArtistCredit` | pointer to string |
| `0x48` | `ArtSize` | ? |
| `0x4c` | `Rarity` | enum int |
| `0x50` | **`ExpansionCode`** | **pointer to Il2CppString like `"tle"`** |
| `0x58` | `DigitalReleaseSet` | pointer |
| `0x60` | `IsToken` | bool |
| `0x61` | `IsPrimaryCard` | bool |
| `0x62` | `IsDigitalOnly` | bool |
| `0x63` | `IsRebalanced` | bool |
| `0x64` | `RebalancedCardGrpId` | int |
| `0x68` | `DefunctRebalancedCardGrpId` | int |
| `0x6c` | `AlternateDeckLimit` | int |
| `0x70` | **`CollectorNumber`** | **pointer to Il2CppString like `"162"`** |
| `0x78` | `CollectorMax` | pointer |
| `0x80` | `CollectorSuffix` | pointer |
| `0x88` | `DraftContent` | bool |
| `0x8a` | `UsesSideboard` | bool |
| `0x90` | `OldSchoolManaText` | pointer |
| `0x98` | `LinkedFaceType` | ? |
| `0xa0` | `RawFrameDetail` | ? |
| `0xa8` | `Watermark` | pointer |
| `0xb0` | `TextChangeData` | ? |
| `0xc0` | `Power` | pointer to string |
| `0xd0` | `Toughness` | pointer to string |
| `0xe0` | `Colors` | array |
| `0xe8` | `ColorIdentity` | array |
| `0xf0` | `FrameColors` | array |
| `0xf8` | `IndicatorColors` | array |
| `0x100` | `Types` | array |
| `0x108` | `Subtypes` | array |
| `0x110` | `Supertypes` | array |
| `0x118` | `AbilityIds` | array |
| `0x120` | `HiddenAbilityIds` | array |
| `0x128` | `LinkedFaceGrpIds` | array |
| `0x130` | `LinkedAbilityTemplateCardGrpIds` | array |
| `0x138` | `AbilityIdToLinkedTokenGrpId` | dict |
| `0x140` | `AbilityIdToLinkedConjurations` | dict |
| `0x148` | `KnownSupportedStyles` | array |
| `0x150` | `AdditionalFrameDetails` | ? |

### Paths to resolve a grp_id to a card name

We shipped Path B. Path A is untried.

**Path A — via `TitleId` + localization table (offline, untried)**:
`TitleId` at offset `0x20` is an int ID into Arena's localization database,
not a direct string pointer. Resolving to English text requires walking a
localization data structure we haven't explored — likely keyed first by
language code and then by TitleId, with some form of fallback handling.
Possibly lazy-loaded. The entry class is somewhere in the
`Wotc.Mtga.Loc` namespace but the walker hasn't been written.

**Path B — via `ExpansionCode` + `CollectorNumber` (online, shipped)**:
`ExpansionCode` at `0x50` and `CollectorNumber` at `0x70` are both
`Il2CppString*` (UTF-16 managed strings, decoded by the `read_il2cpp_string`
helper). With both, hit
`https://api.scryfall.com/cards/{set}/{number}` which **returns cards
even when Scryfall's `arena_id` field is null** — verified against
Alchemy / Universes Beyond sets where `arena_id` is absent. This is
what `readMtgaCardDatabase` exposes and what downstream callers use.
It introduces a network dependency but that's cacheable on disk.

### Finding CardPrintingRecord instances (stale approach)

The older approach described here — heap-scanning for `obj[0] == cpr_class`
and filtering by field shape — was abandoned because single-field matches
produce too many false positives (FieldInfo entries in dylib data, zeroed
slots, and unrelated objects whose first 8 bytes happen to equal the class
pointer).

The approach that actually works: heap-scan for a `Dictionary<int, T>`
object whose entries have the `Dictionary<int, V>.Entry` layout **with
stride 24** (`hash + next + key + padding + 8-byte value pointer`), whose
`count` field is in the Arena card-database range (`5_000–100_000`), and
whose sampled entry value pointers dereference to objects of a known
card-printing class name. This is `scan_heap_for_card_printing_dictionary`
in `src/napi/mod.rs` — see the `readMtgaCardDatabase` update subsection
for the detailed walk-through.

## Resume / rebuild instructions

```sh
cd ~/repos/mtga-reader

# Make sure Rust is on PATH (rustup lives in ~/.cargo by default)
. "$HOME/.cargo/env"

# devDeps (napi-rs CLI etc.) — only needed on first checkout
npm install

# Full release build. Produces mtga-reader.darwin-arm64.node in the repo
# root, which Node's native loader picks up via ./index.js.
npm run build

# Optional: expose as a global package for experiments
npm link

# Run against live MTGA (needs sudo for task_for_pid). Example using
# our custom readMtgaCards function:
sudo node -e 'console.log(require("mtga-reader").readMtgaCards("MTGA"))'
```

`MTGA_KNOWN_CARD_IDS` and `MTGA_VERIFY_QTYS` env vars are read by the
`scan_heap_for_cards_dictionary` function for ground-truth validation when
there are multiple passing candidates:

```sh
MTGA_KNOWN_CARD_IDS="90881,90804,91088" \
MTGA_VERIFY_QTYS="98307:4,98487:3" \
sudo node -e 'console.log(require("mtga-reader").readMtgaCards("MTGA"))'
```

## Decision log

- **2026-04-10: Forked from upstream HEAD.** `v0.1.5` on npm doesn't build
  on macOS at all (no cfg(target_os="macos") deps in `Cargo.toml`, no macOS
  branch in `is_admin`, stale API surface).

- **2026-04-10: Bypassed the PAPA walker entirely.** Upstream's code path
  walks `PAPA._instance → InventoryManager → _inventoryServiceWrapper →
  Cards`, but `PAPA._instance` reads as 0 on current Arena builds. Replaced
  with a signature scan (`scan_heap_for_cards_dictionary`) that finds the
  Cards dict directly by `hash == key` + value-range signature.

- **2026-04-10: Accepted that Scryfall arena_id coverage is incomplete.**
  Scryfall's `default_cards.json` has ~16,500 entries with populated
  `arena_id` but Arena has ~17,466 cards total; the gap is newer Alchemy and
  Universes Beyond sets where Scryfall's upstream data hasn't caught up.
  Adding `commander-tuner/mtga-import --untapped-csv` as a fallback /
  primary source proved to be the cheapest and most reliable fix. The
  CardDatabase walker was NOT implemented.

- **2026-04-11: Removed mtga-reader support from `mtga-import`.** The
  `untapped-csv` source is simpler (no sudo, no Arena running, no Rust
  toolchain, no Scryfall coverage gap), and the mtga-reader code path hit
  enough macOS-specific friction that it wasn't worth maintaining in the
  commander-tuner repo. This fork stays in `~/repos/mtga-reader` as a
  standalone project for future experimentation.
