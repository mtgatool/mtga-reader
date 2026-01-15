// Simple test to debug why TypeDefinition is returning empty names
use mtga_reader::constants;
use mtga_reader::mono_reader::MonoReader;
use mtga_reader::type_definition::TypeDefinition;

fn main() {
    println!("=== Simple TypeDefinition Test ===\n");

    let process_name = "MTGA";
    let pid = MonoReader::find_pid_by_name(&process_name).expect("MTGA not found");

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let _mono_root = mono_reader.read_mono_root_domain();
    let assembly_image = mono_reader.read_assembly_image();

    let class_cache_table = mono_reader.read_ptr(
        assembly_image + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize
    );

    // Get first valid type definition
    let mut type_def_addr = 0;
    for i in 0..100 {
        let def = mono_reader.read_ptr(class_cache_table + (i * 8));
        if def > 0x10000 && def < 0x7FFFFFFFFFFF {
            type_def_addr = def;
            break;
        }
    }

    println!("Type definition address: 0x{:x}\n", type_def_addr);

    // Read name manually
    println!("Manual reading:");
    let name_ptr_addr = type_def_addr + constants::TYPE_DEFINITION_NAME as usize;
    println!("  NAME pointer address: 0x{:x}", name_ptr_addr);

    let name_ptr = mono_reader.read_ptr(name_ptr_addr);
    println!("  NAME pointer value: 0x{:x}", name_ptr);

    if name_ptr > 0x10000 && name_ptr < 0x7FFFFFFFFFFF {
        let name = mono_reader.read_ascii_string(name_ptr);
        println!("  NAME string: \"{}\"", name);
    } else {
        println!("  NAME pointer is invalid!");
    }

    // Read using read_ptr_ascii_string
    println!("\nUsing read_ptr_ascii_string:");
    let name2 = mono_reader.read_ptr_ascii_string(name_ptr_addr);
    println!("  NAME: \"{}\"", name2);

    // Now use TypeDefinition
    println!("\nUsing TypeDefinition::new:");
    let typedef = TypeDefinition::new(type_def_addr, &mono_reader);
    println!("  NAME: \"{}\"", typedef.name);
    println!("  NAMESPACE: \"{}\"", typedef.namespace_name);

    // Test with a few more
    println!("\n=== Testing multiple type definitions ===\n");
    for i in 0..10 {
        let def = mono_reader.read_ptr(class_cache_table + (i * 8));
        if def > 0x10000 && def < 0x7FFFFFFFFFFF {
            let typedef = TypeDefinition::new(def, &mono_reader);
            println!("{}: {} [{}]", i, typedef.name, typedef.namespace_name);
        }
    }
}
