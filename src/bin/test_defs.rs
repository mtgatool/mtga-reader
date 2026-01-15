// Test what create_type_definitions returns
use mtga_reader::constants;
use mtga_reader::mono_reader::MonoReader;
use mtga_reader::type_definition::TypeDefinition;

fn main() {
    println!("=== Testing create_type_definitions ===\n");

    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name).expect("MTGA not found");

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let _mono_root = mono_reader.read_mono_root_domain();
    let _assembly_image = mono_reader.read_assembly_image();

    let defs = mono_reader.create_type_definitions();
    println!("create_type_definitions returned {} addresses\n", defs.len());

    // Check how many are valid pointers
    let valid_count = defs.iter().filter(|&&addr| addr > 0x10000 && addr < 0x7FFFFFFFFFFF).count();
    println!("{} appear to be valid pointers\n", valid_count);

    // Try to read names from first 20
    println!("First 20 type definitions:");
    for (i, &addr) in defs.iter().take(20).enumerate() {
        println!("  [{}] addr=0x{:x}", i, addr);

        if addr > 0x10000 && addr < 0x7FFFFFFFFFFF {
            // Try to read name manually
            let name_ptr = mono_reader.read_ptr(addr + constants::TYPE_DEFINITION_NAME as usize);
            if name_ptr > 0x10000 && name_ptr < 0x7FFFFFFFFFFF {
                if let Some(name) = mono_reader.maybe_read_ascii_string(name_ptr) {
                    if !name.is_empty() && name.len() < 100 {
                        println!("      name: \"{}\"", name);
                    } else {
                        println!("      name: (empty or too long)");
                    }
                } else {
                    println!("      name: (not readable)");
                }
            } else {
                println!("      name_ptr: 0x{:x} (invalid)", name_ptr);
            }

            // Now try TypeDefinition
            let typedef = TypeDefinition::new(addr, &mono_reader);
            if !typedef.name.is_empty() {
                println!("      TypeDef: {} [{}]", typedef.name, typedef.namespace_name);
            } else {
                println!("      TypeDef: (empty name)");
            }
        } else {
            println!("      (invalid address)");
        }
    }

    // Count how many have non-empty names
    println!("\nCounting non-empty names...");
    let mut non_empty = 0;
    let mut empty = 0;

    for &addr in &defs {
        if addr > 0x10000 && addr < 0x7FFFFFFFFFFF {
            let typedef = TypeDefinition::new(addr, &mono_reader);
            if !typedef.name.is_empty() {
                non_empty += 1;
            } else {
                empty += 1;
            }
        }
    }

    println!("Non-empty names: {}", non_empty);
    println!("Empty names: {}", empty);
}
