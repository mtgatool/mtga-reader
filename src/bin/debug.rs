// Debug binary for testing WrapperController static fields and instance reading
// Run with: cargo run --bin debug

use mtga_reader::constants;
use mtga_reader::field_definition::FieldDefinition;
use mtga_reader::managed::Managed;
use mtga_reader::mono_reader::MonoReader;
use mtga_reader::type_code::TypeCode;
use mtga_reader::type_definition::TypeDefinition;

fn main() {
    println!("===========================================");
    println!("  MTGA Memory Reader - Debug Tool");
    println!("===========================================\n");

    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name).expect("MTGA not found");

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let _mono_root = mono_reader.read_mono_root_domain();

    // Step 1: Find WrapperController
    println!("=== Step 1: Finding WrapperController ===\n");

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
        println!("✗ Could not find WrapperController in Assembly-CSharp!");
        println!("  Searching in other assemblies...\n");

        // Get all assemblies
        let assemblies = mono_reader.get_all_assembly_names();
        println!("Available assemblies:");
        for (i, name) in assemblies.iter().enumerate() {
            println!("  {:2}. {}", i + 1, name);
        }

        // Search in common assemblies
        let search_assemblies = [
            "Core",
            "Wizards.Mtga",
            "Wizards.Mtga.FrontEnd",
            "SharedClientCore",
            "WizardsUI",
        ];

        let mut found_addr: Option<usize> = None;
        let mut found_assembly = String::new();

        for asm_name in &search_assemblies {
            if !assemblies.contains(&asm_name.to_string()) {
                continue;
            }

            println!("\nSearching in {}...", asm_name);
            let asm_image = mono_reader.read_assembly_image_by_name(asm_name);
            if asm_image == 0 {
                println!("  ✗ Could not read assembly image");
                continue;
            }

            // Use the new method that accepts an assembly image address
            let asm_defs = mono_reader.create_type_definitions_for_image(asm_image);
            println!("  Found {} type definitions", asm_defs.len());

            // Debug: show first 10 classes
            if *asm_name == "Core" {
                println!("  First 10 classes in Core:");
                for (i, def_addr) in asm_defs.iter().take(10).enumerate() {
                    let typedef = TypeDefinition::new(*def_addr, &mono_reader);
                    println!("    {}. {} [{}]", i + 1, typedef.name, typedef.namespace_name);
                }
            }

            for def_addr in &asm_defs {
                let typedef = TypeDefinition::new(*def_addr, &mono_reader);
                if typedef.name == "WrapperController" {
                    println!("  ✓ Found WrapperController!");
                    found_addr = Some(*def_addr);
                    found_assembly = asm_name.to_string();
                    break;
                }

                // Debug: show classes that contain "Wrapper" or "Controller"
                if *asm_name == "Core" && (typedef.name.contains("Wrapper") || typedef.name.contains("Controller")) {
                    println!("    Found candidate: {} [{}]", typedef.name, typedef.namespace_name);
                }
            }

            if found_addr.is_some() {
                break;
            }
        }

        if found_addr.is_none() {
            println!("\n✗ Could not find WrapperController in any assembly!");

            // List all classes that contain "Wrapper" or "Controller"
            println!("\nSearching for similar class names in Assembly-CSharp:");
            for def_addr in &defs {
                let typedef = TypeDefinition::new(*def_addr, &mono_reader);
                if typedef.name.contains("Wrapper") || typedef.name.contains("Controller") {
                    println!("  - {} [{}]", typedef.name, typedef.namespace_name);
                }
            }
            return;
        }

        println!("\n✓ Found WrapperController in {}", found_assembly);
        found_addr.unwrap()
    };

    println!("✓ Found WrapperController at 0x{:x}\n", wrapper_controller_addr);

    // Step 2: Test reading static wildcard fields from PAPA class (before creating WrapperController typedef)
    println!("=== Step 2: Finding PAPA Class for Wildcard Fields ===\n");

    // Find PAPA class in Assembly-CSharp
    let asm_csharp_image = mono_reader.read_assembly_image();  // Get Assembly-CSharp
    let defs_for_papa = mono_reader.create_type_definitions_for_image(asm_csharp_image);

    let papa_def = defs_for_papa.iter().find_map(|def_addr| {
        let typedef = TypeDefinition::new(*def_addr, &mono_reader);
        if typedef.name == "PAPA" {
            Some(*def_addr)
        } else {
            None
        }
    });

    if papa_def.is_none() {
        println!("✗ Could not find PAPA class - skipping wildcard test");
        println!("  Wildcard constants are in PAPA class, not WrapperController\n");
    } else {
        let papa_addr = papa_def.unwrap();
        let papa_typedef = TypeDefinition::new(papa_addr, &mono_reader);
        println!("✓ Found PAPA class at 0x{:x}\n", papa_addr);

        let wildcard_fields = [
            ("COMMON_WILDCARD_GRPID", 69747),      // Expected: 69747
            ("UNCOMMON_WILDCARD_GRPID", 69748),    // Expected: 69748
            ("RARE_WILDCARD_GRPID", 69749),        // Expected: 69749
            ("MYTHICRARE_WILDCARD_GRPID", 69750),  // Expected: 69750
        ];

        println!("Testing get_static_value() for wildcards in PAPA class:");
        for (field_name, expected_value) in &wildcard_fields {
            let (value_ptr, type_info) = papa_typedef.get_static_value(field_name);

            if value_ptr == 0 {
                println!("  ✗ {}: NOT FOUND", field_name);
                continue;
            }

            println!("  {}", field_name);
            println!("    Value pointer: 0x{:x}", value_ptr);
            let type_code = type_info.clone().code();
            println!("    Type: {}", type_code);

            // Read the actual value
            let managed = Managed::new(&mono_reader, value_ptr, None);
            let actual_value = match type_code {
                TypeCode::I4 => managed.read_i4(),
                TypeCode::U4 => managed.read_u4() as i32,
                TypeCode::I => managed.read_i4(),
                TypeCode::U => managed.read_u4() as i32,
                _ => {
                    println!("    ⚠ Unexpected type code");
                    0
                }
            };

            let matches = actual_value == *expected_value as i32;
            let status = if matches { "✓" } else { "✗" };
            println!("    Actual value: {} {} (expected: {})", actual_value, status, expected_value);

            if !matches {
                // Debug: show raw bytes
                let bytes = mono_reader.read_bytes(value_ptr, 8);
                println!("    Raw bytes: {:02x?}", bytes);
            }
        }
        println!();
    }

    // Now create the WrapperController typedef
    let wrapper_controller_typedef = TypeDefinition::new(wrapper_controller_addr, &mono_reader);

    println!("WrapperController Class info:");
    println!("  Name: {}", wrapper_controller_typedef.name);
    println!("  Namespace: {}", wrapper_controller_typedef.namespace_name);
    println!("  VTable: 0x{:x}", wrapper_controller_typedef.v_table);
    println!("  VTableSize: {}", wrapper_controller_typedef.v_table_size);

    // Step 3: Test reading <Instance>k__BackingField from WrapperController
    println!("\n=== Step 3: Reading <Instance>k__BackingField ===\n");

    let instance_field_name = "<Instance>k__BackingField";
    let (instance_ptr_addr, instance_type_info) = wrapper_controller_typedef.get_static_value(instance_field_name);

    if instance_ptr_addr == 0 {
        println!("✗ Could not find {} field", instance_field_name);

        // Try alternative names
        println!("\nTrying alternative instance field names:");
        for name in &["_instance", "Instance", "instance", "s_instance"] {
            let (ptr_addr, _) = wrapper_controller_typedef.get_static_value(name);
            if ptr_addr != 0 {
                println!("  ✓ Found using field: {}", name);
            }
        }
        return;
    }

    println!("✓ Found {} field", instance_field_name);
    println!("  Value pointer address: 0x{:x}", instance_ptr_addr);
    println!("  Type: {}", instance_type_info.code());

    // The static field contains a pointer to the WrapperController instance
    // Read that pointer
    let instance_ptr = mono_reader.read_ptr(instance_ptr_addr);

    println!("  Instance pointer (dereferenced): 0x{:x}", instance_ptr);

    if instance_ptr == 0 || instance_ptr < 0x10000 {
        println!("  ✗ Instance pointer is null or invalid!");
        println!("\n  Debug: Raw memory at value pointer:");
        let raw_bytes = mono_reader.read_bytes(instance_ptr_addr, 32);
        for i in (0..32).step_by(8) {
            println!("    +0x{:02x}: {:02x?}", i, &raw_bytes[i..i+8]);
        }
        return;
    }

    println!("  ✓ Instance pointer is valid");

    // Step 4: Read <InventoryManager>k__BackingField from the instance
    println!("\n=== Step 4: Reading <InventoryManager> from Instance ===\n");

    // Verify the instance's class
    let instance_vtable = mono_reader.read_ptr(instance_ptr);
    let instance_class = mono_reader.read_ptr(instance_vtable);
    let instance_typedef = TypeDefinition::new(instance_class, &mono_reader);

    println!("Instance class verification:");
    println!("  Instance VTable: 0x{:x}", instance_vtable);
    println!("  Instance Class: 0x{:x}", instance_class);
    println!("  Instance Class Name: {}", instance_typedef.name);

    if instance_typedef.name != "WrapperController" {
        println!("  ⚠ WARNING: Instance class name doesn't match! Expected 'WrapperController'");
    } else {
        println!("  ✓ Instance class matches WrapperController");
    }

    // List all instance (non-static) fields
    println!("\nAll instance fields in WrapperController:");
    let instance_fields = wrapper_controller_typedef.get_fields();
    let mut inventory_manager_offset: Option<i32> = None;

    for field_addr in &instance_fields {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if !field_def.type_info.is_static && !field_def.type_info.is_const {
            let type_code = field_def.type_info.clone().code();
            println!("  {} (offset: 0x{:x}, type: {})",
                field_def.name,
                field_def.offset,
                type_code
            );

            if field_def.name.contains("InventoryManager") {
                inventory_manager_offset = Some(field_def.offset);
            }
        }
    }

    if inventory_manager_offset.is_none() {
        println!("\n✗ Could not find InventoryManager field!");
        return;
    }

    let inv_mgr_offset = inventory_manager_offset.unwrap();
    println!("\n✓ Found InventoryManager field at offset: 0x{:x}", inv_mgr_offset);

    // Read the InventoryManager pointer from the instance
    let inventory_manager_ptr_addr = instance_ptr + inv_mgr_offset as usize;
    println!("  Reading from address: 0x{:x} (instance base + offset)", inventory_manager_ptr_addr);

    let inventory_manager_ptr = mono_reader.read_ptr(inventory_manager_ptr_addr);
    println!("  InventoryManager pointer value: 0x{:x}", inventory_manager_ptr);

    if inventory_manager_ptr == 0 || inventory_manager_ptr < 0x10000 {
        println!("  ✗ InventoryManager pointer is null or invalid!");

        println!("\n  Debug: Memory dump around instance:");
        println!("    Instance base: 0x{:x}", instance_ptr);
        for offset in (0..0x80).step_by(8) {
            let addr = instance_ptr + offset;
            let value = mono_reader.read_ptr(addr);
            let is_valid_ptr = value > 0x10000 && value < 0x7FFFFFFFFFFF;
            let marker = if offset == inv_mgr_offset as usize { " <- InventoryManager offset" } else { "" };
            println!("    +0x{:02x}: 0x{:016x} {}{}",
                offset, value,
                if is_valid_ptr { "✓" } else { " " },
                marker
            );
        }
        return;
    }

    println!("  ✓ InventoryManager pointer is valid!");

    // Verify the InventoryManager class
    let inv_mgr_vtable = mono_reader.read_ptr(inventory_manager_ptr);
    let inv_mgr_class = mono_reader.read_ptr(inv_mgr_vtable);
    let inv_mgr_typedef = TypeDefinition::new(inv_mgr_class, &mono_reader);

    println!("\nInventoryManager class info:");
    println!("  VTable: 0x{:x}", inv_mgr_vtable);
    println!("  Class: 0x{:x}", inv_mgr_class);
    println!("  Class Name: {}", inv_mgr_typedef.name);
    println!("  Namespace: {}", inv_mgr_typedef.namespace_name);

    // List some fields from InventoryManager
    println!("\nSample fields from InventoryManager:");
    let inv_mgr_fields = inv_mgr_typedef.get_fields();
    for (i, field_addr) in inv_mgr_fields.iter().take(30).enumerate() {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        let is_static = field_def.type_info.is_static;
        let type_code = field_def.type_info.clone().code();
        println!("  {:2}. {} (offset: 0x{:x}, static: {}, type: {})",
            i + 1,
            field_def.name,
            field_def.offset,
            is_static,
            type_code
        );
    }

    // Step 5: Read _inventoryServiceWrapper field from InventoryManager
    println!("\n=== Step 5: Reading _inventoryServiceWrapper from InventoryManager ===\n");
    let inventory_service_wrapper_field = inv_mgr_fields.iter().find_map(|field_addr| {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if field_def.name == "_inventoryServiceWrapper" {
            Some(field_def)
        } else {
            None
        }
    });
    if inventory_service_wrapper_field.is_none() {
        println!("✗ Could not find _inventoryServiceWrapper field!");
        return;
    }
    let inv_svc_wrapper_field = inventory_service_wrapper_field.unwrap();
    println!("✓ Found _inventoryServiceWrapper field at offset: 0x{:x}", inv_svc_wrapper_field.offset);
    let inv_svc_wrapper_ptr_addr = inventory_manager_ptr + inv_svc_wrapper_field.offset as usize;
    println!("  Reading from address: 0x{:x} (InventoryManager base + offset)", inv_svc_wrapper_ptr_addr);
    let inv_svc_wrapper_ptr = mono_reader.read_ptr(inv_svc_wrapper_ptr_addr);
    println!("  _inventoryServiceWrapper pointer value: 0x{:x}", inv_svc_wrapper_ptr);
    if inv_svc_wrapper_ptr == 0 || inv_svc_wrapper_ptr < 0x10000 {
        println!("  ✗ _inventoryServiceWrapper pointer is null or invalid!");
        return;
    }
    println!("  ✓ _inventoryServiceWrapper pointer is valid!");

    println!("  Reading _inventoryServiceWrapper class");
    let inv_svc_wrapper_vtable = mono_reader.read_ptr(inv_svc_wrapper_ptr);
    let inv_svc_wrapper_class = mono_reader.read_ptr(inv_svc_wrapper_vtable);
    let inv_svc_wrapper_typedef = TypeDefinition::new(inv_svc_wrapper_class, &mono_reader);
    println!("\n_inventoryServiceWrapper class info:");
    println!("  VTable: 0x{:x}", inv_svc_wrapper_vtable);
    println!("  Class: 0x{:x}", inv_svc_wrapper_class);
    println!("  Class Name: {}", inv_svc_wrapper_typedef.name);
    println!("  Namespace: {}", inv_svc_wrapper_typedef.namespace_name);

    // List fields from _inventoryServiceWrapper
    println!("\nFields in _inventoryServiceWrapper:");
    let inv_svc_wrapper_fields = inv_svc_wrapper_typedef.get_fields();
    for (i, field_addr) in inv_svc_wrapper_fields.iter().take(30).enumerate() {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        let is_static = field_def.type_info.is_static;
        let type_code = field_def.type_info.clone().code();
        println!("  {:2}. {} (offset: 0x{:x}, static: {}, type: {})",
            i + 1,
            field_def.name,
            field_def.offset,
            is_static,
            type_code
        );
    }

    // Step 6: Read <Cards>k__BackingField from _inventoryServiceWrapper
    println!("\n=== Step 6: Reading <Cards>k__BackingField ===\n");

    let cards_field = inv_svc_wrapper_fields.iter().find_map(|field_addr| {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if field_def.name == "<Cards>k__BackingField" {
            Some(field_def)
        } else {
            None
        }
    });

    if cards_field.is_none() {
        println!("✗ Could not find <Cards>k__BackingField field!");
        println!("  This field should contain the card collection");
    } else {
        let cards_field_def = cards_field.unwrap();
        println!("✓ Found <Cards>k__BackingField at offset: 0x{:x}", cards_field_def.offset);

        let cards_ptr_addr = inv_svc_wrapper_ptr + cards_field_def.offset as usize;
        println!("  Reading from address: 0x{:x}", cards_ptr_addr);

        let cards_ptr = mono_reader.read_ptr(cards_ptr_addr);
        println!("  Cards pointer: 0x{:x}", cards_ptr);

        if cards_ptr == 0 || cards_ptr < 0x10000 {
            println!("  ✗ Cards pointer is null or invalid!");
        } else {
            println!("  ✓ Cards pointer is valid!");

            // Read the Cards collection class
            let cards_vtable = mono_reader.read_ptr(cards_ptr);
            let cards_class = mono_reader.read_ptr(cards_vtable);
            let cards_typedef = TypeDefinition::new(cards_class, &mono_reader);

            println!("\n  Cards collection class info:");
            println!("    Class Name: {}", cards_typedef.name);
            println!("    Namespace: {}", cards_typedef.namespace_name);

            // Look for _entries field (Dictionary internals)
            let cards_fields = cards_typedef.get_fields();

            println!("    Exploring parent class hierarchy...");

            // Check parent classes for fields (CardsAndQuantity might inherit from Dictionary)
            let mut current_class = cards_class;
            let mut all_fields = cards_fields.clone();
            let mut depth = 0;

            while depth < 10 {
                // Validate current_class pointer before using it
                if current_class == 0 || current_class < 0x100000000 || current_class > 0x7FFFFFFFFFFF {
                    println!("      Invalid class pointer at depth {}: 0x{:x}", depth, current_class);
                    break;
                }

                let typedef = TypeDefinition::new(current_class, &mono_reader);

                // Validate class name before printing
                if typedef.name.is_empty() || typedef.name.len() > 200 {
                    println!("      Invalid class name at depth {}, stopping parent traversal", depth);
                    break;
                }

                // Use field_count to avoid hanging on classes with massive corrupted field_count values
                println!("      Class {}: {} [{}] - {} fields (field_count)",
                    depth,
                    typedef.name,
                    typedef.namespace_name,
                    typedef.field_count
                );

                // Get parent
                let parent_ptr = mono_reader.read_ptr(current_class + constants::TYPE_DEFINITION_PARENT as usize);
                if parent_ptr == 0 || parent_ptr < 0x100000000 || parent_ptr > 0x7FFFFFFFFFFF {
                    println!("      No valid parent class (ptr: 0x{:x})", parent_ptr);
                    break;
                }

                current_class = parent_ptr;
                let parent_typedef = TypeDefinition::new(parent_ptr, &mono_reader);

                // Validate parent name
                if parent_typedef.name.is_empty() || parent_typedef.name.len() > 200 {
                    println!("      Invalid parent class name, stopping");
                    break;
                }

                // Only enumerate fields if field_count is reasonable (< 1000 to avoid hangs)
                // The Dictionary class has a corrupted field_count of 16M+
                if parent_typedef.field_count > 0 && parent_typedef.field_count < 1000 {
                    let parent_fields = parent_typedef.get_fields();
                    all_fields.extend(parent_fields);
                } else {
                    println!("      Skipping field enumeration for parent class (field_count: {})", parent_typedef.field_count);
                }
                depth += 1;
            }

            println!("\n    Total fields including inheritance: {}", all_fields.len());

            // Now search in all fields (including inherited)
            let entries_field = all_fields.iter().find_map(|field_addr| {
                let field_def = FieldDefinition::new(*field_addr, &mono_reader);
                if field_def.name == "_entries" || field_def.name == "entries" {
                    Some(field_def)
                } else {
                    None
                }
            });

            if entries_field.is_some() {
                let entries_field_def = entries_field.unwrap();
                println!("    ✓ Found {} field at offset: 0x{:x}", entries_field_def.name, entries_field_def.offset);

                let entries_ptr_addr = cards_ptr + entries_field_def.offset as usize;
                let entries_ptr = mono_reader.read_ptr(entries_ptr_addr);
                println!("    _entries array pointer: 0x{:x}", entries_ptr);

                if entries_ptr != 0 && entries_ptr > 0x10000 {
                    // Read array length (at offset 0x18 in mono array)
                    let array_length = mono_reader.read_i32(entries_ptr + 0x18);
                    println!("    Array length: {}", array_length);

                    if array_length > 0 && array_length < 100000 {
                        println!("    ✓ Successfully found card collection with {} entries!", array_length);

                        // Read first few entries to show card data
                        println!("\n    Reading first 10 card entries:");
                        println!("    {:>6} | {:>12} | {:>8}", "Index", "Card ID", "Count");
                        println!("    {}", "-".repeat(35));

                        // Dictionary<uint, int> entry structure:
                        // struct Entry { int hashCode; int next; uint key; int value; }
                        // Size: 16 bytes (4+4+4+4)
                        let entry_size = 16usize;
                        let entries_start = entries_ptr + constants::SIZE_OF_PTR * 4; // Skip array header

                        let mut valid_count = 0;
                        for i in 0..std::cmp::min(array_length, 100) {
                            let entry_addr = entries_start + (i as usize * entry_size);

                            let hash_code = mono_reader.read_i32(entry_addr);
                            let _next = mono_reader.read_i32(entry_addr + 4);
                            let key = mono_reader.read_u32(entry_addr + 8);  // card ID
                            let value = mono_reader.read_i32(entry_addr + 12); // quantity

                            // Valid entries have hashCode >= 0
                            if hash_code >= 0 && key > 0 {
                                valid_count += 1;
                                if valid_count <= 10 {
                                    println!("    {:>6} | {:>12} | {:>8}", valid_count, key, value);
                                }
                            }
                        }

                        println!("\n    ✓ Read {} valid card entries (out of {} total)", valid_count, array_length);
                    }
                }
            } else {
                println!("    ✗ Could not find _entries field via parent enumeration");
                println!("    Trying known Dictionary<K,V> offsets directly...");
                println!("    Dumping first 64 bytes of CardsAndQuantity object:");

                // Dump memory to see the structure
                for offset in (0..64).step_by(8) {
                    let val = mono_reader.read_ptr(cards_ptr + offset);
                    println!("      +0x{:02x}: 0x{:016x}", offset, val);
                }

                // Try standard Dictionary<K,V> offsets
                // _entries is at offset 0x18, _count is at offset 0x20
                let entries_ptr_0x18 = mono_reader.read_ptr(cards_ptr + 0x18);
                let count_0x20 = mono_reader.read_i32(cards_ptr + 0x20);

                println!("\n    Standard offsets: _entries at 0x18 = 0x{:x}, _count at 0x20 = {}", entries_ptr_0x18, count_0x20);

                // Try alternative offsets (sometimes _entries is at 0x10)
                let entries_ptr_0x10 = mono_reader.read_ptr(cards_ptr + 0x10);
                let count_0x18 = mono_reader.read_i32(cards_ptr + 0x18);
                println!("    Alternative offsets: _entries at 0x10 = 0x{:x}, _count at 0x18 = {}", entries_ptr_0x10, count_0x18);

                // Find which offset has valid data
                let (entries_ptr, array_length, offset_desc) = if entries_ptr_0x18 > 0x10000 {
                    let len = mono_reader.read_i32(entries_ptr_0x18 + 0x18);
                    if len > 0 && len < 100000 {
                        (entries_ptr_0x18, len, "standard 0x18")
                    } else if entries_ptr_0x10 > 0x10000 {
                        let len = mono_reader.read_i32(entries_ptr_0x10 + 0x18);
                        (entries_ptr_0x10, len, "alternative 0x10")
                    } else {
                        (0, 0, "none")
                    }
                } else if entries_ptr_0x10 > 0x10000 {
                    let len = mono_reader.read_i32(entries_ptr_0x10 + 0x18);
                    (entries_ptr_0x10, len, "alternative 0x10")
                } else {
                    (0, 0, "none")
                };

                if entries_ptr > 0x10000 && array_length > 0 && array_length < 100000 {
                    println!("    ✓ Found Dictionary using {} offset!", offset_desc);
                    println!("    Array length: {}", array_length);
                    println!("    ✓ Successfully found card collection with {} entries!", array_length);

                    // Read first few entries to show card data
                    println!("\n    Reading first 10 card entries:");
                    println!("    {:>6} | {:>12} | {:>8}", "Index", "Card ID", "Count");
                    println!("    {}", "-".repeat(35));

                    // Dictionary<uint, int> entry structure:
                    // struct Entry { int hashCode; int next; uint key; int value; }
                    // Size: 16 bytes (4+4+4+4)
                    let entry_size = 16usize;
                    let entries_start = entries_ptr + constants::SIZE_OF_PTR * 4; // Skip array header

                    let mut valid_count = 0;
                    for i in 0..std::cmp::min(array_length, 100) {
                        let entry_addr = entries_start + (i as usize * entry_size);

                        let hash_code = mono_reader.read_i32(entry_addr);
                        let _next = mono_reader.read_i32(entry_addr + 4);
                        let key = mono_reader.read_u32(entry_addr + 8);  // card ID
                        let value = mono_reader.read_i32(entry_addr + 12); // quantity

                        // Valid entries have hashCode >= 0
                        if hash_code >= 0 && key > 0 {
                            valid_count += 1;
                            if valid_count <= 10 {
                                println!("    {:>6} | {:>12} | {:>8}", valid_count, key, value);
                            }
                        }
                    }

                    println!("\n    ✓ Read {} valid card entries (out of {} total)", valid_count, array_length);
                } else {
                    println!("    ✗ Could not find valid Dictionary structure at any known offset");
                    println!("    Total fields found (including parents): {}", all_fields.len());
                }
            }
        }
    }

    // Step 7: Read m_inventory from _inventoryServiceWrapper
    println!("\n=== Step 7: Reading m_inventory (gems/gold) ===\n");

    let inventory_field = inv_svc_wrapper_fields.iter().find_map(|field_addr| {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if field_def.name == "m_inventory" {
            Some(field_def)
        } else {
            None
        }
    });

    if inventory_field.is_none() {
        println!("✗ Could not find m_inventory field!");
    } else {
        let inventory_field_def = inventory_field.unwrap();
        println!("✓ Found m_inventory at offset: 0x{:x}", inventory_field_def.offset);

        let inventory_ptr_addr = inv_svc_wrapper_ptr + inventory_field_def.offset as usize;
        let inventory_ptr = mono_reader.read_ptr(inventory_ptr_addr);
        println!("  m_inventory pointer: 0x{:x}", inventory_ptr);

        if inventory_ptr != 0 && inventory_ptr > 0x10000 {
            println!("  ✓ m_inventory pointer is valid!");

            // Read inventory class
            let inventory_vtable = mono_reader.read_ptr(inventory_ptr);
            let inventory_class = mono_reader.read_ptr(inventory_vtable);
            let inventory_typedef = TypeDefinition::new(inventory_class, &mono_reader);

            println!("\n  Inventory class info:");
            println!("    Class Name: {}", inventory_typedef.name);
            println!("    Namespace: {}", inventory_typedef.namespace_name);

            // Look for gems and gold fields
            let inventory_fields = inventory_typedef.get_fields();

            println!("\n  Looking for gems and gold fields:");
            for field_addr in &inventory_fields {
                let field_def = FieldDefinition::new(*field_addr, &mono_reader);
                if field_def.name == "gems" || field_def.name == "gold" {
                    let field_ptr_addr = inventory_ptr + field_def.offset as usize;
                    let value = mono_reader.read_i32(field_ptr_addr);
                    println!("    {}: {} (offset: 0x{:x})", field_def.name, value, field_def.offset);
                }
            }
        }
    }

    // Step 8: Test string reading from AccountClient -> AccountInformation
    println!("\n=== Step 8: Testing string reading (AccountInformation) ===\n");

    // Find <AccountClient>k__BackingField from WrapperController
    let account_client_field = instance_fields.iter().find_map(|field_addr| {
        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
        if field_def.name.contains("AccountClient") {
            Some(field_def)
        } else {
            None
        }
    });

    if let Some(account_client_field_def) = account_client_field {
        println!("✓ Found {} at offset: 0x{:x}", account_client_field_def.name, account_client_field_def.offset);

        let account_client_ptr = mono_reader.read_ptr(instance_ptr + account_client_field_def.offset as usize);
        println!("  AccountClient pointer: 0x{:x}", account_client_ptr);

        if account_client_ptr != 0 && account_client_ptr > 0x10000 {
            // Read AccountClient fields
            let ac_vtable = mono_reader.read_ptr(account_client_ptr);
            let ac_class = mono_reader.read_ptr(ac_vtable);
            let ac_typedef = TypeDefinition::new(ac_class, &mono_reader);
            let ac_fields = ac_typedef.get_fields();

            println!("  AccountClient class: {} [{}]", ac_typedef.name, ac_typedef.namespace_name);

            // Find <AccountInformation>k__BackingField
            let account_info_field = ac_fields.iter().find_map(|field_addr| {
                let field_def = FieldDefinition::new(*field_addr, &mono_reader);
                if field_def.name.contains("AccountInformation") {
                    Some(field_def)
                } else {
                    None
                }
            });

            if let Some(account_info_field_def) = account_info_field {
                println!("\n  ✓ Found {} at offset: 0x{:x}", account_info_field_def.name, account_info_field_def.offset);

                let account_info_ptr = mono_reader.read_ptr(account_client_ptr + account_info_field_def.offset as usize);
                println!("    AccountInformation pointer: 0x{:x}", account_info_ptr);

                if account_info_ptr != 0 && account_info_ptr > 0x10000 {
                    // Read AccountInformation fields
                    let ai_vtable = mono_reader.read_ptr(account_info_ptr);
                    let ai_class = mono_reader.read_ptr(ai_vtable);
                    let ai_typedef = TypeDefinition::new(ai_class, &mono_reader);
                    let ai_fields = ai_typedef.get_fields();

                    println!("    AccountInformation class: {} [{}]", ai_typedef.name, ai_typedef.namespace_name);
                    println!("\n    Testing string field reading:");

                    // Read all string fields
                    for field_addr in &ai_fields {
                        let field_def = FieldDefinition::new(*field_addr, &mono_reader);
                        let type_code = field_def.type_info.clone().code();
                        if matches!(type_code, TypeCode::STRING) {
                            let string_ptr = mono_reader.read_ptr(account_info_ptr + field_def.offset as usize);

                            if string_ptr != 0 && string_ptr > 0x10000 {
                                match mono_reader.read_mono_string(string_ptr) {
                                    Some(value) => {
                                        println!("      {}: \"{}\" (offset: 0x{:x}, ptr: 0x{:x})",
                                            field_def.name,
                                            value,
                                            field_def.offset,
                                            string_ptr
                                        );
                                    }
                                    None => {
                                        println!("      {}: <failed to read string> (offset: 0x{:x}, ptr: 0x{:x})",
                                            field_def.name,
                                            field_def.offset,
                                            string_ptr
                                        );
                                    }
                                }
                            } else {
                                println!("      {}: null (offset: 0x{:x})", field_def.name, field_def.offset);
                            }
                        }
                    }
                } else {
                    println!("    ✗ AccountInformation pointer is null or invalid");
                }
            } else {
                println!("\n  ✗ Could not find AccountInformation field");
            }
        } else {
            println!("  ✗ AccountClient pointer is null or invalid");
        }
    } else {
        println!("✗ Could not find AccountClient field");
    }

    println!("\n===========================================");
    println!("  Debug complete!");
    println!("===========================================");
}
