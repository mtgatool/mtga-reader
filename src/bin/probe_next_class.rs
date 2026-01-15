// Probe for correct TYPE_DEFINITION_NEXT_CLASS_CACHE offset
use mtga_reader::constants;
use mtga_reader::mono_reader::MonoReader;

fn main() {
    println!("=== Probing TYPE_DEFINITION_NEXT_CLASS_CACHE offset ===\n");

    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name).expect("MTGA not found");

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let _mono_root = mono_reader.read_mono_root_domain();
    let assembly_image = mono_reader.read_assembly_image();

    let class_cache_table = mono_reader.read_ptr(
        assembly_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize
    );

    // Get first valid type definition from bucket 0
    let first_def = mono_reader.read_ptr(class_cache_table);
    println!("First type definition in bucket 0: 0x{:x}\n", first_def);

    // Read its name to verify it's valid
    let name_ptr = mono_reader.read_ptr(first_def + constants::TYPE_DEFINITION_NAME as usize);
    if let Some(name) = mono_reader.maybe_read_ascii_string(name_ptr) {
        println!("Name: \"{}\"\n", name);
    }

    println!("Current TYPE_DEFINITION_NEXT_CLASS_CACHE: 0x{:x}", constants::TYPE_DEFINITION_NEXT_CLASS_CACHE);
    println!("Reading next pointer at: 0x{:x} + 0x{:x} = 0x{:x}\n",
        first_def, constants::TYPE_DEFINITION_NEXT_CLASS_CACHE,
        first_def + constants::TYPE_DEFINITION_NEXT_CLASS_CACHE as usize);

    let current_next = mono_reader.read_ptr(first_def + constants::TYPE_DEFINITION_NEXT_CLASS_CACHE as usize);
    println!("Current next pointer: 0x{:x}\n", current_next);

    // Probe different offsets
    println!("Probing offsets from 0xD0 to 0x120...\n");
    println!("Offset | Next Pointer      | Valid? | Notes");
    println!("-------|-------------------|--------|------------------");

    let test_offsets = vec![
        0xd0u32, 0xd8, 0xe0, 0xe8, 0xf0, 0xf8, 0x100, 0x108,
        0x110, 0x118, 0x120,
    ];

    for offset in test_offsets {
        let next_ptr = mono_reader.read_ptr(first_def + offset as usize);
        let is_valid = next_ptr == 0 || (next_ptr > 0x100000000 && next_ptr < 0x7FFFFFFFFFFF);
        let is_current = offset == constants::TYPE_DEFINITION_NEXT_CLASS_CACHE;

        let mut notes = String::new();
        if is_current {
            notes.push_str(" (current)");
        }

        if next_ptr == 0 {
            notes.push_str(" NULL - end of chain");
        } else if is_valid {
            // Try to read name to see if it's a valid MonoClass
            let name_ptr = mono_reader.read_ptr(next_ptr + constants::TYPE_DEFINITION_NAME as usize);
            if name_ptr > 0x10000 && name_ptr < 0x7FFFFFFFFFFF {
                if let Some(name) = mono_reader.maybe_read_ascii_string(name_ptr) {
                    if !name.is_empty() && name.len() < 100 {
                        notes.push_str(&format!(" -> \"{}\"", name));
                    }
                }
            }
        }

        println!("0x{:03x} | 0x{:016x} | {:6} | {}",
            offset, next_ptr,
            if is_valid { "YES" } else { "NO" },
            notes
        );
    }

    // Now test which offset gives us a complete, valid chain
    println!("\n=== Testing Complete Chains ===\n");

    for offset in &[0xe0u32, 0xe8, 0xf0, 0x100, 0x108] {
        print!("Offset 0x{:x}: ", offset);

        let mut current = first_def;
        let mut count = 0;
        let mut valid_count = 0;
        let mut max_iterations = 50; // Safety limit

        while current != 0 && max_iterations > 0 {
            count += 1;

            // Check if this is a valid class
            let name_ptr = mono_reader.read_ptr(current + constants::TYPE_DEFINITION_NAME as usize);
            if name_ptr > 0x10000 && name_ptr < 0x7FFFFFFFFFFF {
                if let Some(name) = mono_reader.maybe_read_ascii_string(name_ptr) {
                    if !name.is_empty() && name.len() < 100 {
                        valid_count += 1;
                    }
                }
            }

            // Get next
            current = mono_reader.read_ptr(current + *offset as usize);

            // Sanity check
            if current != 0 && (current < 0x100000000 || current > 0x7FFFFFFFFFFF) {
                print!("INVALID next pointer 0x{:x}, ", current);
                break;
            }

            max_iterations -= 1;
        }

        println!("{} total, {} valid classes", count, valid_count);
    }
}
