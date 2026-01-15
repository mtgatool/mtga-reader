# Mono Generic Types Memory Layout Notes

## Problem: Generic Types Have Corrupted field_count

When reading generic type definitions like `Dictionary<TKey, TValue>` from Mono memory, the `field_count` value is often corrupted, showing values like 138,610,192 instead of the actual ~8 fields.

### Root Cause

Generic types in Mono have a different memory layout than regular class definitions. The `TYPE_DEFINITION_FIELD_COUNT` offset (used for regular classes) points to incorrect data when applied to generic type definitions.

## Solutions

### 1. Check field_count Before Enumeration

Always validate `field_count` before calling `get_fields()`:

```rust
// Check if field_count is reasonable before enumerating
if typedef.field_count > 0 && typedef.field_count < 1000 {
    let fields = typedef.get_fields();
    // Process fields...
} else {
    println!("Skipping field enumeration (field_count: {})", typedef.field_count);
}
```

**Why 1000?** Most classes have fewer than 100 fields. A threshold of 1000 catches corrupted values while allowing legitimate large classes.

### 2. Use Known Structure Offsets

For common structures like Dictionary, use known memory layouts directly:

```rust
// Dictionary<K,V> standard layout:
// +0x00: VTable pointer
// +0x08: Monitor/sync block
// +0x10: _buckets (sometimes)
// +0x18: _entries array pointer (standard)
// +0x20: _count (often 0 for inherited types)

let entries_ptr = reader.read_ptr(dict_addr + 0x18);

// Verify by reading array header
if entries_ptr > 0x10000 {
    let array_length = reader.read_i32(entries_ptr + 0x18);
    if array_length > 0 && array_length < 100000 {
        // Valid Dictionary!
    }
}
```

### 3. Read Array Length from Array Header

Don't rely on Dictionary's `_count` field - it may be 0 for inherited Dictionary types. Instead, read the array length from the `_entries` array header:

```rust
// Mono array structure:
// +0x00: VTable pointer (8 bytes)
// +0x08: Monitor (8 bytes)
// +0x10: Bounds pointer (8 bytes)
// +0x18: Length (4 bytes i32)
// +0x20: Data starts...

let entries_array_ptr = reader.read_ptr(dict_addr + 0x18);
let actual_length = reader.read_i32(entries_array_ptr + 0x18);
```

## Dictionary<K,V> Entry Structure

Dictionary entries are stored as an array of structs:

```csharp
struct Entry {
    int hashCode;  // +0x00 (4 bytes) - negative = empty slot
    int next;      // +0x04 (4 bytes) - next entry in bucket chain
    TKey key;      // +0x08 (size of TKey)
    TValue value;  // +0x08 + sizeof(TKey)
}
```

For `Dictionary<uint, int>`:
- Entry size: 16 bytes (4 + 4 + 4 + 4)
- Valid entries have `hashCode >= 0`
- Empty slots have `hashCode < 0`

```rust
let entry_size = 16usize;
let entries_start = entries_ptr + SIZE_OF_PTR * 4; // Skip array header

for i in 0..array_length {
    let entry_addr = entries_start + (i as usize * entry_size);

    let hash_code = reader.read_i32(entry_addr);
    let key = reader.read_u32(entry_addr + 8);
    let value = reader.read_i32(entry_addr + 12);

    if hash_code >= 0 && key > 0 {
        // Valid entry
    }
}
```

## Example: CardsAndQuantity

The MTGA `CardsAndQuantity` class inherits from `Dictionary<uint, int>`:

```
Class hierarchy:
  CardsAndQuantity (0 fields)
    └─ Dictionary<uint, int> (corrupted field_count: 138M+)
         └─ Object (0 fields)
```

**Memory layout at instance address:**
```
+0x00: VTable pointer
+0x08: Monitor/sync block
+0x10: _buckets pointer (alternative location)
+0x18: _entries array pointer ✓ (17,519 card entries)
+0x20: _count (0 - not reliable for inherited types)
```

## Implementation Examples

### debug.rs Pattern

```rust
// Try to enumerate fields from parent classes
if parent_typedef.field_count > 0 && parent_typedef.field_count < 1000 {
    let parent_fields = parent_typedef.get_fields();
    all_fields.extend(parent_fields);
} else {
    println!("Skipping enumeration (field_count: {})", parent_typedef.field_count);
}

// Fallback: use known Dictionary offsets
let entries_ptr = mono_reader.read_ptr(dict_addr + 0x18);
if entries_ptr > 0x10000 {
    let array_length = mono_reader.read_i32(entries_ptr + 0x18);
    if array_length > 0 && array_length < 100000 {
        // Successfully found Dictionary data!
    }
}
```

### http_server_simple.rs Pattern

```rust
// Try standard Dictionary offset
let entries_ptr_0x18 = reader.read_ptr(dict_addr + 0x18);
if entries_ptr_0x18 > 0x10000 {
    let array_length = reader.read_i32(entries_ptr_0x18 + 0x18);
    if array_length > 0 && array_length < 100000 {
        return read_dict_entries(reader, entries_ptr_0x18, array_length);
    }
}

// Try alternative offset
let entries_ptr_0x10 = reader.read_ptr(dict_addr + 0x10);
if entries_ptr_0x10 > 0x10000 {
    let array_length = reader.read_i32(entries_ptr_0x10 + 0x18);
    if array_length > 0 && array_length < 100000 {
        return read_dict_entries(reader, entries_ptr_0x10, array_length);
    }
}
```

## Key Takeaways

1. **Never trust field_count for generic types** - it's often corrupted
2. **Validate before enumerating** - check field_count < 1000 before calling get_fields()
3. **Use known offsets for common structures** - Dictionary has consistent memory layout
4. **Read array length from array header** - more reliable than Dictionary's _count field
5. **Verify pointer validity** - check ptr > 0x10000 before dereferencing
6. **Validate array bounds** - ensure length > 0 && length < 100000 for sanity

## Testing

Successfully tested with MTGA:
- **Cards collection**: 17,519 entries read successfully
- **Gems/Gold**: Read correctly (140 gems, 1750 gold)
- **Strings**: UTF-16 reading works (DisplayName: "Manuel777#63494")

## References

- Mono source: https://github.com/Unity-Technologies/mono/blob/unity-master/mono/metadata/class-internals.h
- .NET Dictionary source: https://github.com/dotnet/runtime/blob/main/src/libraries/System.Private.CoreLib/src/System/Collections/Generic/Dictionary.cs
