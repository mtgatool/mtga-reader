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

## What works

- Builds cleanly on `darwin-arm64` with `npm install && npm run build`
  (produces `mtga-reader.darwin-arm64.node`, already present in the repo root
  from the last build).
- `readMtgaCards("MTGA")` (our custom napi function added to this fork) scans
  Arena's live process memory, finds the `Cards` dictionary via a signature
  scan, and returns `{ cards: [{cardId: int, quantity: int}, ...] }`.
- The scan is fast (<1s wall clock) and deterministic against a running Arena
  process.
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
  etc.) so downstream name resolution misses ~1500 cards when using this
  reader alone. This is a Scryfall-side upstream data ingestion lag, not a
  local bulk staleness issue. Running `download-bulk` doesn't help.
- **`PAPA._instance` reads as `0`** on current Arena builds. Upstream's code
  path walks `PAPA._instance.InventoryManager._inventoryServiceWrapper.Cards`
  and that first step is broken. Unknown root cause — probably a
  GC-static-vs-value-static layout difference in the IL2CPP class struct, or a
  stale `CLASS_STATIC_FIELDS` offset that happens to work for some value-type
  statics but not for reference-type statics. **Our signature scan bypasses
  this entirely** by finding the Cards dict directly rather than walking to
  it from PAPA, so the broken static read doesn't block card extraction, but
  it prevents walking the other fields on PAPA (InventoryManager, EventManager,
  MatchManager, etc.) that might be interesting to read in the future.

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

5. **Various diagnostic functions — `scan_for_type_info_table`,
   `find_class_by_direct_scan`, `dump_class_names_matching`,
   `find_papa_instance_via_static_field`, `find_wrapper_controller_instance`,
   `find_papa_instance_by_field_verification`, `probe_card_printing_record`,
   `scan_for_dict_entry_pattern`.** All dead code now — used during the
   reverse-engineering to learn about Arena's memory layout. Feel free to
   delete if you're cleaning up for an upstream PR, but they're useful
   reference for how to probe specific aspects of Arena's in-process state.

## CardPrintingRecord field layout

Captured by running `probe_card_printing_record()` from our own napi
module against a live Arena process. The function calls
`get_class_fields(cpr_class)`, which walks the `FieldInfo[]` array
stored on Arena's own IL2CPP class metadata at startup — so it's
authoritative for whatever Arena build is currently running. We did
not consult any third-party reader to derive this table.

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

**Path A — via `TitleId` + Arena's localization table (offline, untried)**:
`TitleId` at offset `0x20` is an int ID into Arena's localization database,
not a direct string pointer. Resolving to English text requires walking a
localization data structure we haven't explored — likely keyed first by
language code and then by TitleId, with some form of fallback handling,
possibly lazy-loaded. The walker hasn't been written.

**Path B — via `ExpansionCode` + `CollectorNumber` (online, simpler)**:
`ExpansionCode` at `0x50` and `CollectorNumber` at `0x70` are both
Il2CppString pointers (UTF-16 managed strings). With both, hit
`https://api.scryfall.com/cards/{set}/{number}` — which **returns cards
even when Scryfall's `arena_id` field is null** (verified during the
investigation, e.g. `/cards/tle/162` returns `Diresight` with
`arena_id: None`). Introduces a network dependency but that's
cacheable on disk.

### Finding CardPrintingRecord instances

Our direct heap scan for `obj[0] == cpr_class` produces **mostly false
positives**. The sample we captured (`probe_card_printing_record` output)
showed:

- **Instance 1** at `0x1036a9910`: `GrpId=75, TitleId=1`, rest mostly zero.
  Looked like a tiny token or placeholder slot.
- **Instance 2** at `0x10399d1a8`: all zeros. Uninitialized.
- **Instance 3** at `0x103a320d8`: **GrpId=71806704** (a pointer value, not
  an int) — the "class pointer" at offset 0 coincidentally matched
  `cpr_class` but the struct at that address is some other type.
- **Instances 4-5** at `0x10ff09b98`/`0x10ff09ba8`: have fields reading as
  strings `"_count"`, `"_entries"`, `"_freeList"`, `"_buckets"` — **they're
  actually Dictionary internal field-name string literals** that happened to
  land at addresses whose first 8 bytes equal `cpr_class`.

**The real instances must be inside a container** — probably Arena has a
`Dictionary<int, CardPrintingRecord*>` or similar. To find it, scan for a
dictionary with these properties:

- `hash == key` at the standard Dictionary<int,V> layout (hash at +0, key at
  +8 of each entry)
- Entry stride `24` bytes (not 16) because the value is an 8-byte pointer,
  plus 4 bytes alignment padding between `key` (int, 4 bytes) and `value`
  (ptr, 8 bytes)
- `count` around **17,000** — roughly how many cards Arena ships with
- Keys (grp_ids) in the Arena range `[1, 200_000]`
- Values pointing to objects whose first 8 bytes equal `cpr_class`

Once found, iterate the entries and for each valid (key, value_ptr) pair,
the value_ptr is a real `CardPrintingRecord*`. Read the fields we care about
(`GrpId`, `ExpansionCode`, `CollectorNumber` — or `TitleId` if going the
localization-table route).

### Improving static field reading (alternative approach)

If we wanted to fix the broken `papa._instance` read instead of
bypassing it, some ideas that haven't been explored:

1. **Dump the raw bytes of the `Il2CppClass` struct for PAPA** and compare
   against the layout our code assumes (`CLASS_STATIC_FIELDS` at `0xA8`).
   Look for another pointer field nearby that might be the GC-tracked static
   area.
2. **Cross-reference against the actual IL2CPP source** at
   `https://github.com/Unity-Technologies/il2cpp` or similar. The `Il2CppClass`
   struct layout is public, but it varies by Unity version.
3. **Use `il2cpp-dumper`** or `Il2CppInspector` against Arena's
   `GameAssembly.dylib` to get a definitive dump of every class's metadata.
   Those tools are open source and specifically target reverse-engineering
   IL2CPP binaries. They'd tell us exactly what offset `static_fields` is at
   and whether there's a separate GC-static-fields pointer.

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
