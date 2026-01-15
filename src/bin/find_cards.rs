// Find and read Cards data via WrapperController -> InventoryManager
// Path from mtga-tracker-daemon: WrapperController.<Instance> -> <InventoryManager> -> _inventoryServiceWrapper -> <Cards> -> _entries
// Run with: cargo run --bin find_cards

use mtga_reader::constants;
use mtga_reader::field_definition::FieldDefinition;
use mtga_reader::mono_reader::MonoReader;
use mtga_reader::type_definition::TypeDefinition;

fn main() {
    println!("=== Finding Cards Data via WrapperController ===\n");

    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name).expect("MTGA not found");

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let _mono_root = mono_reader.read_mono_root_domain();

    // Step 1: Find WrapperController in Assembly-CSharp
    println!("Step 1: Finding WrapperController in Assembly-CSharp...\n");

    let assembly_image = mono_reader.read_assembly_image();
    if assembly_image == 0 {
        println!("✗ Failed to find Assembly-CSharp!");
        return;
    }
    println!("✓ Assembly-CSharp image: 0x{:x}", assembly_image);

    let defs = mono_reader.create_type_definitions();
    println!("  Found {} type definitions\n", defs.len());

    let wrapper_controller_def = defs.iter().find_map(|def_addr| {
        let typedef = TypeDefinition::new(*def_addr, &mono_reader);
        if typedef.name == "WrapperController" {
            Some(*def_addr)
        } else {
            None
        }
    });

    let wrapper_controller_addr = if let Some(addr) = wrapper_controller_def {
        addr
    } else {
        println!("✗ Could not find WrapperController class in Assembly-CSharp!");
        println!("  Trying Core assembly...");

        // Try searching in Core assembly
        let core_image = mono_reader.read_assembly_image_by_name("Core");
        let core_defs = mono_reader.create_type_definitions_for_image(core_image);

        let core_wrapper_controller = core_defs.iter().find_map(|def_addr| {
            let typedef = TypeDefinition::new(*def_addr, &mono_reader);
            if typedef.name.contains("WrapperController") || typedef.name.contains("Wrapper") {
                println!("  Found candidate: {} [{}]", typedef.name, typedef.namespace_name);
                if typedef.name == "WrapperController" {
                    return Some(*def_addr);
                }
            }
            None
        });

        if core_wrapper_controller.is_none() {
            println!("✗ Could not find WrapperController in Core either!");
            return;
        }

        core_wrapper_controller.unwrap()
    };
    println!("✓ Found WrapperController at 0x{:x}", wrapper_controller_addr);

    // Step 2: Read WrapperController.<Instance> singleton
    println!("\nStep 2: Reading WrapperController.<Instance>...\n");

    let wrapper_controller_typedef = TypeDefinition::new(wrapper_controller_addr, &mono_reader);

    // Try to get the static <Instance>k__BackingField
    // get_static_value returns the ADDRESS of the field, we need to dereference it
    let (instance_field_addr, _) = wrapper_controller_typedef.get_static_value("<Instance>k__BackingField");

    if instance_field_addr == 0 || instance_field_addr < 0x10000 {
        println!("✗ WrapperController <Instance> field address is null or invalid: 0x{:x}", instance_field_addr);
        println!("  Trying alternative names...");

        // Try other common singleton names
        for name in &["_instance", "Instance", "instance", "s_instance"] {
            let (field_addr, _) = wrapper_controller_typedef.get_static_value(name);
            if field_addr != 0 && field_addr > 0x10000 {
                let instance_ptr = mono_reader.read_ptr(field_addr);
                if instance_ptr != 0 && instance_ptr > 0x10000 {
                    println!("✓ Found instance using field: {}", name);
                    println!("✓ WrapperController instance pointer: 0x{:x}", instance_ptr);
                    read_inventory_from_wrapper_controller(&mono_reader, instance_ptr, wrapper_controller_addr);
                    return;
                }
            }
        }

        println!("✗ Could not find any valid instance field!");
        return;
    }

    // Dereference the static field to get the actual instance pointer
    let instance_ptr = mono_reader.read_ptr(instance_field_addr);

    if instance_ptr == 0 || instance_ptr < 0x10000 {
        println!("✗ WrapperController instance pointer (dereferenced) is null: 0x{:x}", instance_ptr);
        return;
    }

    println!("✓ WrapperController instance pointer: 0x{:x}", instance_ptr);
    read_inventory_from_wrapper_controller(&mono_reader, instance_ptr, wrapper_controller_addr);
}

fn read_inventory_from_wrapper_controller(mono_reader: &MonoReader, wrapper_controller_ptr: usize, wrapper_controller_class_def: usize) {
    // Step 3: Read <InventoryManager>k__BackingField from WrapperController instance
    println!("\nStep 3: Reading <InventoryManager> from WrapperController...\n");

    // Use the class definition we already found, not the one from the instance vtable
    let wrapper_controller_typedef = TypeDefinition::new(wrapper_controller_class_def, &mono_reader);

    println!("  Using WrapperController class def at: 0x{:x}", wrapper_controller_class_def);
    println!("  WrapperController class name: \"{}\" [{}]",
        wrapper_controller_typedef.name,
        wrapper_controller_typedef.namespace_name);
    let wrapper_controller_fields = wrapper_controller_typedef.get_fields();

    let mut inventory_manager_offset: Option<i32> = None;

    for field_addr in &wrapper_controller_fields {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if field_def.name.contains("InventoryManager") || field_def.name.contains("inventoryManager") {
            println!("    Found: {} (offset: 0x{:x}, is_static: {})",
                field_def.name,
                field_def.offset,
                field_def.type_info.is_static
            );

            if !field_def.type_info.is_static {
                inventory_manager_offset = Some(field_def.offset);
            }
        }
    }

    if inventory_manager_offset.is_none() {
        println!("\n✗ Could not find InventoryManager field!");
        println!("  Available fields:");
        for (i, field_addr) in wrapper_controller_fields.iter().take(20).enumerate() {
            let field_def = FieldDefinition::new(*field_addr, &mono_reader);
            println!("    {}: {}", i + 1, field_def.name);
        }
        return;
    }

    let inv_mgr_offset = inventory_manager_offset.unwrap();
    let inventory_manager_ptr = mono_reader.read_ptr(wrapper_controller_ptr + inv_mgr_offset as usize);

    if inventory_manager_ptr == 0 || inventory_manager_ptr < 0x10000 {
        println!("✗ InventoryManager pointer is null: 0x{:x}", inventory_manager_ptr);
        return;
    }

    println!("✓ InventoryManager instance pointer: 0x{:x}", inventory_manager_ptr);

    // Step 4: Read _inventoryServiceWrapper from InventoryManager
    read_cards_from_inventory_manager(mono_reader, inventory_manager_ptr);
}

fn read_cards_from_inventory_manager(mono_reader: &MonoReader, inventory_manager_ptr: usize) {
    println!("\nStep 4: Reading _inventoryServiceWrapper from InventoryManager...\n");

    // Get InventoryManager class definition
    let vtable = mono_reader.read_ptr(inventory_manager_ptr);
    let inv_mgr_class = mono_reader.read_ptr(vtable);
    let inv_mgr_typedef = TypeDefinition::new(inv_mgr_class, &mono_reader);

    println!("  InventoryManager class: {}", inv_mgr_typedef.name);
    let inv_mgr_fields = inv_mgr_typedef.get_fields();

    let wrapper_field = inv_mgr_fields.iter().find_map(|field_addr| {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if field_def.name == "_inventoryServiceWrapper" {
            Some(field_def.offset)
        } else {
            None
        }
    });

    if wrapper_field.is_none() {
        println!("✗ Could not find _inventoryServiceWrapper field!");
        return;
    }

    let wrapper_offset = wrapper_field.unwrap();
    let wrapper_ptr = mono_reader.read_ptr(inventory_manager_ptr + wrapper_offset as usize);

    if wrapper_ptr == 0 || wrapper_ptr < 0x10000 {
        println!("✗ _inventoryServiceWrapper pointer is null: 0x{:x}", wrapper_ptr);
        return;
    }

    println!("✓ Wrapper pointer: 0x{:x}", wrapper_ptr);

    // Step 5: Read <Cards>k__BackingField from wrapper
    println!("\nStep 5: Reading <Cards> dictionary from wrapper...\n");

    let wrapper_vtable = mono_reader.read_ptr(wrapper_ptr);
    let wrapper_class = mono_reader.read_ptr(wrapper_vtable);
    let wrapper_typedef = TypeDefinition::new(wrapper_class, &mono_reader);

    println!("  Wrapper class: {} [{}]", wrapper_typedef.name, wrapper_typedef.namespace_name);
    let wrapper_fields = wrapper_typedef.get_fields();

    let mut cards_offset: Option<i32> = None;

    for field_addr in &wrapper_fields {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if field_def.name.contains("Cards") || field_def.name.contains("cards") {
            println!("    Found: {} (offset: 0x{:x})", field_def.name, field_def.offset);
            cards_offset = Some(field_def.offset);
        }
    }

    if cards_offset.is_none() {
        println!("✗ Could not find Cards field!");
        return;
    }

    let cards_ptr = mono_reader.read_ptr(wrapper_ptr + cards_offset.unwrap() as usize);

    if cards_ptr == 0 || cards_ptr < 0x10000 {
        println!("✗ Cards dictionary pointer is null: 0x{:x}", cards_ptr);
        return;
    }

    println!("✓ Cards dictionary pointer: 0x{:x}", cards_ptr);

    // Step 6: Read Dictionary structure directly
    // Don't try to examine the generic Dictionary class - just read it directly
    println!("\n  Reading Dictionary<uint, int> directly (skipping class examination)...");
    read_dictionary_entries(mono_reader, cards_ptr);
}

fn read_dictionary_entries(reader: &MonoReader, dict_ptr: usize) {
    println!("\nStep 6: Reading Dictionary<uint, int> entries...\n");

    // First, let's dump the memory to see what's actually there
    println!("  Memory dump around Dictionary pointer:");
    for offset in (0..0x60).step_by(4) {
        let value_i32 = reader.read_i32(dict_ptr + offset);
        let value_ptr = if offset % 8 == 0 {
            reader.read_ptr(dict_ptr + offset)
        } else {
            0
        };
        let is_valid_ptr = value_ptr > 0x10000 && value_ptr < 0x7FFFFFFFFFFF;

        if offset % 8 == 0 {
            println!("    +0x{:02x}: {:10} (ptr: 0x{:016x}) {}",
                offset, value_i32, value_ptr,
                if is_valid_ptr { "<- valid ptr" } else { "" }
            );
        } else {
            println!("    +0x{:02x}: {:10}",
                offset, value_i32
            );
        }
    }

    // Try both with and without underscore prefix (newer vs older C# versions)
    // Standard Dictionary<K,V> layout (newer Unity):
    // +0x10: _buckets (int[])
    // +0x18: _entries (Entry[])
    // +0x20: _count (int)
    // +0x24: _version (int)
    // +0x28: _freeList (int)
    // +0x2c: _freeCount (int)

    println!("\n  Trying standard Dictionary offsets:");
    let buckets_ptr = reader.read_ptr(dict_ptr + 0x10);
    let entries_ptr = reader.read_ptr(dict_ptr + 0x18);
    let count = reader.read_i32(dict_ptr + 0x20);
    let version = reader.read_i32(dict_ptr + 0x24);

    println!("    _buckets: 0x{:x}", buckets_ptr);
    println!("    _entries: 0x{:x}", entries_ptr);
    println!("    _count: {}", count);
    println!("    _version: {}", version);

    if count > 0 && count < 100000 && entries_ptr > 0x10000 {
        println!("\n  ✓ Found valid Dictionary structure!");
        read_entries_array(reader, entries_ptr, count);
        return;
    }

    // Try older offset layout (without underscores or different structure)
    println!("\n  Trying alternative Dictionary offsets:");
    let alt_entries = reader.read_ptr(dict_ptr + 0x10);
    let alt_count = reader.read_i32(dict_ptr + 0x18);

    println!("    entries: 0x{:x}", alt_entries);
    println!("    count: {}", alt_count);

    if alt_count > 0 && alt_count < 100000 && alt_entries > 0x10000 {
        println!("\n  ✓ Found valid Dictionary structure (alt offsets)!");
        read_entries_array(reader, alt_entries, alt_count);
        return;
    }

    println!("\n✗ Could not determine valid dictionary structure!");
}

fn read_entries_array(reader: &MonoReader, entries_ptr: usize, count: i32) {
    println!("\nStep 7: Reading {} dictionary entries...\n", count);

    // Entry structure: { int hashCode; int next; uint key; int value; } = 16 bytes
    let entry_size = 16usize;
    let entries_start = entries_ptr + constants::SIZE_OF_PTR * 4; // Skip array header

    let max_display = std::cmp::min(count, 100);

    println!("  Card Collection (first {} of {}):", max_display, count);
    println!("  {:>6} | {:>12} | {:>8}", "Index", "Card ID", "Count");
    println!("  {}", "-".repeat(35));

    let mut valid_entries = 0;

    for i in 0..count {
        let entry_addr = entries_start + (i as usize * entry_size);

        let hash_code = reader.read_i32(entry_addr);
        let key = reader.read_u32(entry_addr + 8);
        let value = reader.read_i32(entry_addr + 12);

        if hash_code >= 0 && key > 0 {
            valid_entries += 1;
            if valid_entries <= max_display {
                println!("  {:>6} | {:>12} | {:>8}", valid_entries, key, value);
            }
        }
    }

    if valid_entries > max_display {
        println!("\n  ... and {} more cards", valid_entries - max_display);
    }

    println!("\n✓ Successfully read {} cards from collection!", valid_entries);
}
