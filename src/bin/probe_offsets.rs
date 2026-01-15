// Offset probing tool for finding correct MonoClass field offsets
// This tool tests various offset values to find which ones produce valid class names

use mtga_reader::constants;
use mtga_reader::mono_reader::MonoReader;
use std::collections::HashMap;

fn main() {
    println!("===========================================");
    println!("  Mono Offset Probing Tool");
    println!("===========================================\n");

    // Find and attach to MTGA
    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name);

    if pid.is_none() {
        println!("✗ Process '{}' not found!", process_name);
        println!("  Make sure MTGA is running.");
        return;
    }

    let pid = pid.iter().next().unwrap();
    println!("✓ Found MTGA process (PID: {:?})\n", pid);

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let mono_root = mono_reader.read_mono_root_domain();

    if mono_root == 0 {
        println!("✗ Failed to read mono_root_domain");
        return;
    }

    println!("✓ mono_root_domain: 0x{:x}\n", mono_root);

    // Read Assembly-CSharp
    let assembly_image = mono_reader.read_assembly_image();
    if assembly_image == 0 {
        println!("✗ Failed to find Assembly-CSharp!");
        return;
    }

    println!("✓ Assembly-CSharp image: 0x{:x}\n", assembly_image);

    // Get a few type definition addresses to test
    let class_cache_size = mono_reader.read_u32(
        assembly_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_SIZE) as usize
    );
    let class_cache_table = mono_reader.read_ptr(
        assembly_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize
    );

    println!("Class cache size: {}", class_cache_size);
    println!("Class cache table: 0x{:x}\n", class_cache_table);

    // Collect some type definition addresses
    let mut test_addresses: Vec<usize> = Vec::new();

    for i in 0..class_cache_size.min(20) {
        let mut definition = mono_reader.read_ptr(class_cache_table + (i as usize * 8));

        let mut count = 0;
        while definition != 0 && count < 5 {
            if definition > 0x10000 && definition < 0x7FFFFFFFFFFF {
                test_addresses.push(definition);
            }

            definition = mono_reader.read_ptr(definition + constants::TYPE_DEFINITION_NEXT_CLASS_CACHE as usize);
            count += 1;
        }

        if test_addresses.len() >= 20 {
            break;
        }
    }

    println!("Collected {} type definition addresses to probe\n", test_addresses.len());

    if test_addresses.is_empty() {
        println!("✗ No type definitions found!");
        return;
    }

    // Probe different offsets for TYPE_DEFINITION_NAME
    println!("=== Probing TYPE_DEFINITION_NAME offset ===\n");
    println!("Current offset: 0x{:x}\n", constants::TYPE_DEFINITION_NAME);

    let name_offsets_to_test: Vec<u32> = vec![
        0x30, 0x38, 0x40, 0x48, 0x50, 0x58, 0x60, 0x68,
        0x70, 0x78, 0x80, 0x88, 0x90, 0x98, 0xa0, 0xa8,
    ];

    let mut name_results: HashMap<u32, Vec<String>> = HashMap::new();

    for offset in &name_offsets_to_test {
        let mut names = Vec::new();

        for &addr in test_addresses.iter().take(10) {
            let name_ptr = mono_reader.read_ptr(addr + *offset as usize);

            if name_ptr > 0x10000 && name_ptr < 0x7FFFFFFFFFFF {
                if let Some(name) = mono_reader.maybe_read_ascii_string(name_ptr) {
                    if !name.is_empty() && name.len() < 100 && is_valid_class_name(&name) {
                        names.push(name);
                    }
                }
            }
        }

        name_results.insert(*offset, names);
    }

    // Print results
    for offset in &name_offsets_to_test {
        let names = &name_results[offset];
        let current = if *offset == constants::TYPE_DEFINITION_NAME { " (current)" } else { "" };

        print!("  0x{:02x}: ", offset);

        if names.is_empty() {
            println!("No valid names found{}", current);
        } else {
            println!("{} valid names found{}", names.len(), current);
            for (i, name) in names.iter().take(5).enumerate() {
                println!("      {}. {}", i + 1, name);
            }
            if names.len() > 5 {
                println!("      ... and {} more", names.len() - 5);
            }
        }
    }

    // Find best candidate for NAME offset
    let best_name_offset = name_offsets_to_test.iter()
        .max_by_key(|&&offset| name_results[&offset].len())
        .copied();

    if let Some(best_offset) = best_name_offset {
        if !name_results[&best_offset].is_empty() {
            println!("\n✓ Best NAME offset candidate: 0x{:x} ({} valid names)",
                best_offset, name_results[&best_offset].len());
        }
    }

    // Probe different offsets for TYPE_DEFINITION_NAMESPACE
    println!("\n=== Probing TYPE_DEFINITION_NAMESPACE offset ===\n");
    println!("Current offset: 0x{:x}\n", constants::TYPE_DEFINITION_NAMESPACE);

    let namespace_offsets_to_test: Vec<u32> = vec![
        0x38, 0x40, 0x48, 0x50, 0x58, 0x60, 0x68, 0x70,
        0x78, 0x80, 0x88, 0x90, 0x98, 0xa0, 0xa8, 0xb0,
    ];

    let mut namespace_results: HashMap<u32, Vec<String>> = HashMap::new();

    for offset in &namespace_offsets_to_test {
        let mut namespaces = Vec::new();

        for &addr in test_addresses.iter().take(10) {
            let ns_ptr = mono_reader.read_ptr(addr + *offset as usize);

            if ns_ptr > 0x10000 && ns_ptr < 0x7FFFFFFFFFFF {
                if let Some(ns) = mono_reader.maybe_read_ascii_string(ns_ptr) {
                    // Namespaces can be empty or contain dots/alphanumeric
                    if ns.len() < 200 && (ns.is_empty() || is_valid_namespace(&ns)) {
                        namespaces.push(ns);
                    }
                }
            }
        }

        namespace_results.insert(*offset, namespaces);
    }

    // Print results
    for offset in &namespace_offsets_to_test {
        let namespaces = &namespace_results[offset];
        let current = if *offset == constants::TYPE_DEFINITION_NAMESPACE { " (current)" } else { "" };

        print!("  0x{:02x}: ", offset);

        if namespaces.is_empty() {
            println!("No valid namespaces found{}", current);
        } else {
            println!("{} valid namespaces found{}", namespaces.len(), current);
            for (i, ns) in namespaces.iter().take(5).enumerate() {
                if ns.is_empty() {
                    println!("      {}. (empty)", i + 1);
                } else {
                    println!("      {}. {}", i + 1, ns);
                }
            }
            if namespaces.len() > 5 {
                println!("      ... and {} more", namespaces.len() - 5);
            }
        }
    }

    // Find best candidate for NAMESPACE offset
    let best_namespace_offset = namespace_offsets_to_test.iter()
        .max_by_key(|&&offset| namespace_results[&offset].len())
        .copied();

    if let Some(best_offset) = best_namespace_offset {
        if !namespace_results[&best_offset].is_empty() {
            println!("\n✓ Best NAMESPACE offset candidate: 0x{:x} ({} valid namespaces)",
                best_offset, namespace_results[&best_offset].len());
        }
    }

    // Test the best combination
    if let (Some(name_off), Some(ns_off)) = (best_name_offset, best_namespace_offset) {
        println!("\n=== Testing Best Combination ===\n");
        println!("NAME offset: 0x{:x}", name_off);
        println!("NAMESPACE offset: 0x{:x}\n", ns_off);

        println!("Sample classes with both name and namespace:");
        for (i, &addr) in test_addresses.iter().take(15).enumerate() {
            let name_ptr = mono_reader.read_ptr(addr + name_off as usize);
            let ns_ptr = mono_reader.read_ptr(addr + ns_off as usize);

            let name = if name_ptr > 0x10000 && name_ptr < 0x7FFFFFFFFFFF {
                mono_reader.maybe_read_ascii_string(name_ptr).unwrap_or_default()
            } else {
                String::new()
            };

            let namespace = if ns_ptr > 0x10000 && ns_ptr < 0x7FFFFFFFFFFF {
                mono_reader.maybe_read_ascii_string(ns_ptr).unwrap_or_default()
            } else {
                String::new()
            };

            if !name.is_empty() && is_valid_class_name(&name) {
                let ns_display = if namespace.is_empty() { "(no namespace)" } else { &namespace };
                println!("  {:2}. {} [{}]", i + 1, name, ns_display);
            }
        }
    }

    println!("\n===========================================");
    println!("  Probing complete");
    println!("===========================================");
}

fn is_valid_class_name(s: &str) -> bool {
    // Class names should be ASCII alphanumeric plus underscore, angle brackets, backtick, dollar
    // They often contain < > for generics, ` for nested types, $ for compiler-generated
    if s.is_empty() {
        return false;
    }

    s.chars().all(|c| {
        c.is_ascii_alphanumeric() ||
        c == '_' ||
        c == '<' ||
        c == '>' ||
        c == '`' ||
        c == '$' ||
        c == '.' ||
        c == '+' ||
        c == '[' ||
        c == ']' ||
        c == ',' ||
        c == ' '
    })
}

fn is_valid_namespace(s: &str) -> bool {
    // Namespaces can be empty or contain dots and alphanumeric
    if s.is_empty() {
        return true;
    }

    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}
