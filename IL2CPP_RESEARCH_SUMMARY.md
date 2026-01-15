# IL2CPP Backend Research Summary

This document summarizes the research and findings for implementing IL2CPP support in mtga-reader for macOS.

## Project Goal

Add IL2CPP backend support to mtga-reader alongside existing Mono support, enabling cross-platform memory reading (especially macOS). The goal is to create a unified API where the same paths work on both platforms.

## STATUS: WORKING!

**Successfully reading card collection and player inventory from MTGA on macOS!**

Example output:
```
=== Card Collection ===
  Unique cards: 9695
  Total cards: 25117

=== Player Resources ===
  Wildcards - Common: 174, Uncommon: 72, Rare: 14, Mythic: 2
  Gold: 1750
  Gems: 140
```

## Working Path

The complete navigation path to read card data:

```
PAPA instance (found by scanning heap for class pointer)
  └─ +224: <InventoryManager>k__BackingField
              └─ +56: _inventoryServiceWrapper (AwsInventoryServiceWrapper)
                        ├─ +64: m_inventory (ClientPlayerInventory - wildcards, gold, gems)
                        └─ +72: <Cards>k__BackingField (CardsAndQuantity)
                                  └─ +0x18: entries (Entry[] - Dictionary entries)
```

## Key Discoveries

### 1. IL2CPP Class Structure Offsets (Corrected)
```rust
// Il2CppClass offsets for MTGA:
class_name = 0x10          // const char* name
class_namespace = 0x18     // const char* namespace
class_fields = 0x80        // FieldInfo* fields (NOT 0x70!)
class_static_fields = 0xA8 // void* static_fields
```

### 2. FieldInfo Structure (32 bytes)
```rust
// Each FieldInfo entry:
+0x00: name_ptr (8 bytes)   // Pointer to field name string
+0x08: type_ptr (8 bytes)   // Pointer to Il2CppType
+0x10: parent (8 bytes)     // Pointer to parent class
+0x18: offset (4 bytes)     // Field offset in instance
+0x1C: token (4 bytes)      // Metadata token
```

### 3. Type Attributes
```rust
// At Il2CppType + 0x08:
let type_attrs = read_u32(type_ptr + 0x08);
let is_static = (type_attrs & 0x10) != 0;
```

### 4. CardsAndQuantity Structure
The Cards field is NOT a Dictionary, but a custom `CardsAndQuantity` class:
```rust
// CardsAndQuantity layout:
+0x00: class_ptr
+0x10: buckets (Int32[])     // Hash buckets for lookup
+0x18: entries (Entry[])     // Dictionary entries with card data
+0x20: count (i32)           // Number of unique cards
```

### 5. Dictionary Entry Structure
```rust
// Each Entry in the entries array (16 bytes):
+0x00: hashCode (i32)
+0x04: next (i32)
+0x08: key (i32)      // Card ID
+0x0C: value (i32)    // Quantity
```

### 6. ClientPlayerInventory Layout
```rust
// Player resources at m_inventory:
+16: wcCommon (i32)
+20: wcUncommon (i32)
+24: wcRare (i32)
+28: wcMythic (i32)
+32: gold (i32)
+36: gems (i32)
+48: vaultProgress (i32)
```

### 7. Memory Address Ranges
```
Code/metadata:    0x100... - 0x128...
Heap objects:     0x145... - 0x170...
Alternate heap:   0x305... - 0x340... (also valid!)
```

## Implementation Details

### 1. macOS Memory Reading
Using `mach2` crate:
```rust
let task_port = task_for_pid(mach_task_self(), pid as i32, &mut task);
mach_vm_read_overwrite(task_port, addr, size, buffer, &mut out_size);
```

### 2. Finding Type Info Table
```rust
let data_base = get_second_data_segment(pid);  // Second __DATA segment
let type_info_table = read_ptr(data_base + 0x24360);
```

### 3. Finding PAPA Instance
Scan heap regions (0x15a...) for objects where the class pointer matches PAPA class:
```rust
for addr in heap_range.step_by(8) {
    if read_ptr(addr) == papa_class && read_ptr(addr + 16) != papa_class {
        return Some(addr);  // Skip FieldInfo entries
    }
}
```

### 4. Reading Card Data
```rust
let entries_ptr = read_ptr(cards_ptr + 0x18);
let count = read_i32(cards_ptr + 0x20);

for i in 0..count {
    let entry = entries_ptr + 0x20 + i * 16;
    let hash = read_i32(entry);
    let card_id = read_i32(entry + 8);
    let quantity = read_i32(entry + 12);

    if hash >= 0 && card_id > 0 {
        cards.push((card_id, quantity));
    }
}
```

## Test Files

Key test files that demonstrate working functionality:

| File | Purpose |
|------|---------|
| `test_read_collection.rs` | **Full working card reader** |
| `test_heap_instances.rs` | Find real object instances in heap |
| `test_cards_quantity.rs` | Decode CardsAndQuantity structure |
| `test_read_dict.rs` | Explore dictionary-like structures |

## Running Tests

```bash
# Run the card collection reader
sudo cargo run --example test_read_collection --release
```

## Code Structure

The IL2CPP backend implementation:
- `src/il2cpp/mod.rs` - Module exports
- `src/il2cpp/reader.rs` - Main Il2CppBackend implementation
- `src/il2cpp/macos_memory.rs` - macOS memory reading
- `src/il2cpp/metadata.rs` - Metadata parser (v31)
- `src/il2cpp/offsets.rs` - Structure offsets (needs updating to use 0x80 for fields)
- `src/il2cpp/type_definition.rs` - Type definition wrapper
- `src/il2cpp/field_definition.rs` - Field definition wrapper

## Next Steps

1. **Update `src/il2cpp/offsets.rs`** - Change `class_fields` from 0x70 to 0x80
2. **Implement PAPA instance finder** - Scan heap at startup to find current instance
3. **Add dynamic path resolution** - Navigate from PAPA to cards using field names
4. **Integrate with main library** - Add IL2CPP backend to the unified API

## Important Notes

- PAPA instance address changes between game restarts (need to scan each time)
- Addresses in 0x03... range are valid on this macOS version
- FieldInfo entries in metadata region have class pointer at +16, use this to filter
- The Cards data uses a custom CardsAndQuantity class, not a standard Dictionary
