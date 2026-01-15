// Probe NEXT_CLASS_CACHE with multiple buckets
use mtga_reader::constants;
use mtga_reader::mono_reader::MonoReader;
use mtga_reader::type_definition::TypeDefinition;

fn main() {
    println!("=== Probing Multiple Hash Buckets ===\n");

    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name).expect("MTGA not found");

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let _mono_root = mono_reader.read_mono_root_domain();
    let assembly_image = mono_reader.read_assembly_image();

    let class_cache_size = mono_reader.read_u32(
        assembly_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_SIZE) as usize
    );
    let class_cache_table = mono_reader.read_ptr(
        assembly_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize
    );

    println!("Class cache has {} buckets\n", class_cache_size);

    // Find a bucket with multiple entries for testing
    println!("Looking for bucket with multiple classes...\n");

    let test_offsets = vec![0xe0u32, 0xe8, 0xf0, 0x100, 0x108];

    for bucket in 0..class_cache_size.min(20) {
        let first_def = mono_reader.read_ptr(class_cache_table + (bucket as usize * 8));

        if first_def == 0 || first_def < 0x100000000 {
            continue;
        }

        println!("Bucket {}: first=0x{:x}", bucket, first_def);

        // Check each offset to see which gives a valid chain
        for &offset in &test_offsets {
            let mut current = first_def;
            let mut classes = Vec::new();
            let mut max_iter = 10;

            while current != 0 && max_iter > 0 {
                // Try to read name
                let typedef = TypeDefinition::new(current, &mono_reader);
                if !typedef.name.is_empty() {
                    classes.push(typedef.name.clone());
                }

                // Get next
                let next = mono_reader.read_ptr(current + offset as usize);

                // Validate next
                if next != 0 && (next < 0x100000000 || next > 0x7FFFFFFFFFFF) {
                    break; // Invalid pointer
                }

                current = next;
                max_iter -= 1;
            }

            if !classes.is_empty() {
                print!("  0x{:x}: {} classes", offset, classes.len());
                if classes.len() > 1 {
                    print!(" ✓ [{}]", classes.join(", "));
                } else {
                    print!(" [{}]", classes[0]);
                }
                println!();
            }
        }

        println!();
    }

    // Now do a comprehensive test with each offset
    println!("\n=== Comprehensive Test: Total Valid Classes Found ===\n");

    for &offset in &test_offsets {
        println!("Testing offset 0x{:x}:", offset);

        let mut total_classes = 0;
        let mut total_valid_names = 0;

        for bucket in 0..class_cache_size {
            let mut current = mono_reader.read_ptr(class_cache_table + (bucket as usize * 8));

            let mut max_iter = 50;
            while current != 0 && max_iter > 0 {
                // Validate address
                if current < 0x100000000 || current > 0x7FFFFFFFFFFF {
                    break;
                }

                total_classes += 1;

                // Check if it has a valid name
                let typedef = TypeDefinition::new(current, &mono_reader);
                if !typedef.name.is_empty() {
                    total_valid_names += 1;
                }

                // Get next
                let next = mono_reader.read_ptr(current + offset as usize);

                // Validate next
                if next != 0 && (next < 0x100000000 || next > 0x7FFFFFFFFFFF) {
                    break;
                }

                current = next;
                max_iter -= 1;
            }
        }

        let marker = if offset == constants::TYPE_DEFINITION_NEXT_CLASS_CACHE { " (current)" } else { "" };
        println!("  Total: {} classes, {} with valid names{}", total_classes, total_valid_names, marker);

        // Check if this matches expected count
        if total_classes == total_valid_names && total_valid_names > 100 {
            println!("  ✓ LOOKS GOOD - all classes have valid names!");
        }
    }
}
