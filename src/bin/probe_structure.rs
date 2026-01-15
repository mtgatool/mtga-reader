// Advanced structure probing - dumps memory around MonoClass to find NAME and NAMESPACE
use mtga_reader::constants;
use mtga_reader::mono_reader::MonoReader;

fn main() {
    println!("=== MonoClass Structure Memory Dump ===\n");

    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name).expect("MTGA not found");

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let mono_root = mono_reader.read_mono_root_domain();
    let assembly_image = mono_reader.read_assembly_image();

    let class_cache_table = mono_reader.read_ptr(
        assembly_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize
    );

    // Get first valid type definition
    let mut type_def = 0;
    for i in 0..100 {
        let def = mono_reader.read_ptr(class_cache_table + (i * 8));
        if def > 0x10000 && def < 0x7FFFFFFFFFFF {
            type_def = def;
            break;
        }
    }

    if type_def == 0 {
        println!("No type definition found!");
        return;
    }

    println!("Analyzing MonoClass at: 0x{:x}\n", type_def);

    // Dump pointer fields from 0x00 to 0xC0
    println!("Offset | Pointer Value     | Dereferenced String (if valid)");
    println!("-------|-------------------|------------------------------------------");

    for offset in (0x00..0xC0).step_by(8) {
        let ptr_value = mono_reader.read_ptr(type_def + offset);

        print!("0x{:02x}  | 0x{:016x} | ", offset, ptr_value);

        // Try to read as string pointer
        if ptr_value > 0x10000 && ptr_value < 0x7FFFFFFFFFFF {
            if let Some(string) = mono_reader.maybe_read_ascii_string(ptr_value) {
                if !string.is_empty() && string.len() < 100 {
                    // Check if it looks like a valid name/namespace
                    let is_valid = string.chars().all(|c| {
                        c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '<' ||
                        c == '>' || c == '`' || c == '$' || c == '+' || c == '[' || c == ']'
                    });

                    if is_valid {
                        println!("\"{}\"", string);
                    } else {
                        println!("(invalid string)");
                    }
                } else {
                    println!("(empty or too long)");
                }
            } else {
                println!("(not readable)");
            }
        } else {
            println!("(not a valid pointer)");
        }
    }

    println!("\n=== Testing More Type Definitions ===\n");

    // Test multiple type definitions
    let mut defs = Vec::new();
    for i in 0..100 {
        let def = mono_reader.read_ptr(class_cache_table + (i * 8));
        if def > 0x10000 && def < 0x7FFFFFFFFFFF {
            defs.push(def);
            if defs.len() >= 10 {
                break;
            }
        }
    }

    // For each offset, show what we read from multiple type defs
    let test_offsets = vec![0x40, 0x48, 0x50, 0x58, 0x60];

    for &offset in &test_offsets {
        println!("Offset 0x{:02x}:", offset);
        for (i, &def) in defs.iter().enumerate() {
            let ptr = mono_reader.read_ptr(def + offset);
            if ptr > 0x10000 && ptr < 0x7FFFFFFFFFFF {
                if let Some(s) = mono_reader.maybe_read_ascii_string(ptr) {
                    if !s.is_empty() && s.len() < 100 {
                        println!("  [{}] \"{}\"", i, s);
                    }
                }
            }
        }
        println!();
    }

    // Also test against a known assembly with more classes
    println!("=== Testing with Wizards.Arena.Models assembly ===\n");

    let models_image = mono_reader.read_assembly_image_by_name("Wizards.Arena.Models");
    if models_image != 0 {
        println!("Found Wizards.Arena.Models at: 0x{:x}", models_image);

        let class_cache_table2 = mono_reader.read_ptr(
            models_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize
        );

        // Get first type def from this assembly
        let mut type_def2 = 0;
        for i in 0..100 {
            let def = mono_reader.read_ptr(class_cache_table2 + (i * 8));
            if def > 0x10000 && def < 0x7FFFFFFFFFFF {
                type_def2 = def;
                break;
            }
        }

        if type_def2 != 0 {
            println!("Type definition at: 0x{:x}\n", type_def2);

            println!("Offset | String");
            println!("-------|------------------------------------------");
            for offset in (0x40..0x68).step_by(8) {
                let ptr = mono_reader.read_ptr(type_def2 + offset);
                if ptr > 0x10000 && ptr < 0x7FFFFFFFFFFF {
                    if let Some(s) = mono_reader.maybe_read_ascii_string(ptr) {
                        if !s.is_empty() && s.len() < 100 {
                            println!("0x{:02x}  | \"{}\"", offset, s);
                        }
                    }
                }
            }
        }
    }
}
