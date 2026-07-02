// Focused reader: navigate WrapperController -> Instance -> AccountClient -> AccountInformation
// and print all string fields (username lives here). This path is independent of the
// (currently outdated) InventoryManager/Cards path in the debug binary.
//
// Run elevated with: cargo run --bin read_account --no-default-features --features mono

use mtga_reader::field_definition::FieldDefinition;
use mtga_reader::mono_reader::MonoReader;
use mtga_reader::type_code::TypeCode;
use mtga_reader::type_definition::TypeDefinition;

/// Find a type definition by name, searching Assembly-CSharp first then all other assemblies.
fn find_type(mono_reader: &mut MonoReader, class_name: &str) -> Option<usize> {
    let asm_image = mono_reader.read_assembly_image();
    if asm_image != 0 {
        let defs = mono_reader.create_type_definitions_for_image(asm_image);
        if let Some(addr) = defs.iter().find_map(|d| {
            let td = TypeDefinition::new(*d, mono_reader);
            if td.name == class_name { Some(*d) } else { None }
        }) {
            return Some(addr);
        }
    }

    let assemblies = mono_reader.get_all_assembly_names();
    for asm_name in assemblies {
        if asm_name == "Assembly-CSharp" {
            continue;
        }
        let img = mono_reader.read_assembly_image_by_name(&asm_name);
        if img == 0 {
            continue;
        }
        let defs = mono_reader.create_type_definitions_for_image(img);
        if let Some(addr) = defs.iter().find_map(|d| {
            let td = TypeDefinition::new(*d, mono_reader);
            if td.name == class_name { Some(*d) } else { None }
        }) {
            println!("  (found '{}' in assembly '{}')", class_name, asm_name);
            return Some(addr);
        }
    }
    None
}

/// Given an object pointer, return the TypeDefinition address of its class (via vtable).
fn class_of(mono_reader: &MonoReader, obj_ptr: usize) -> usize {
    let vtable = mono_reader.read_ptr(obj_ptr);
    mono_reader.read_ptr(vtable)
}

/// Read a reference-type field (class pointer) from an object by field name.
fn read_ref_field(mono_reader: &MonoReader, obj_ptr: usize, class_addr: usize, needle: &str) -> Option<usize> {
    let td = TypeDefinition::new(class_addr, mono_reader);
    for field_addr in td.get_fields() {
        let fd = FieldDefinition::new(field_addr, mono_reader);
        if fd.name.contains(needle) {
            let ptr = mono_reader.read_ptr(obj_ptr + fd.offset as usize);
            println!("  {} (offset 0x{:x}) -> 0x{:x}", fd.name, fd.offset, ptr);
            if ptr > 0x10000 {
                return Some(ptr);
            }
        }
    }
    None
}

fn main() {
    println!("=== MTGA Account Reader ===\n");

    let process_name = "MTGA";
    let pid = match MonoReader::find_pid_by_name(&process_name) {
        Some(p) => p,
        None => {
            println!("MTGA process not found.");
            return;
        }
    };

    let mut mono_reader = match MonoReader::new(pid.as_u32()) {
        Ok(reader) => reader,
        Err(e) => {
            println!("Failed to open MTGA process (run elevated/as admin): {}", e);
            return;
        }
    };
    mono_reader.read_mono_root_domain();
    mono_reader.read_assembly_image();

    // 1. WrapperController type + static <Instance>
    let wc_addr = match find_type(&mut mono_reader, "WrapperController") {
        Some(a) => a,
        None => {
            println!("Could not find WrapperController.");
            return;
        }
    };
    let wc_td = TypeDefinition::new(wc_addr, &mono_reader);
    let (instance_ptr_addr, _ti) = wc_td.get_static_value("<Instance>k__BackingField");
    if instance_ptr_addr == 0 {
        println!("Could not find <Instance>k__BackingField.");
        return;
    }
    let instance_ptr = mono_reader.read_ptr(instance_ptr_addr);
    println!("WrapperController.Instance -> 0x{:x}", instance_ptr);
    if instance_ptr <= 0x10000 {
        println!("Instance is null - is the client past the loading screen?");
        return;
    }

    // 2. AccountClient
    println!("\n[AccountClient]");
    let account_client_ptr = match read_ref_field(&mono_reader, instance_ptr, wc_addr, "AccountClient") {
        Some(p) => p,
        None => {
            println!("Could not read AccountClient.");
            return;
        }
    };

    // 3. AccountInformation
    println!("\n[AccountInformation]");
    let ac_class = class_of(&mono_reader, account_client_ptr);
    let account_info_ptr = match read_ref_field(&mono_reader, account_client_ptr, ac_class, "AccountInformation") {
        Some(p) => p,
        None => {
            println!("Could not read AccountInformation.");
            return;
        }
    };

    // 4. Dump all string fields (username, etc.)
    println!("\n[AccountInformation string fields]");
    let ai_class = class_of(&mono_reader, account_info_ptr);
    let ai_td = TypeDefinition::new(ai_class, &mono_reader);
    println!("  class: {} [{}]\n", ai_td.name, ai_td.namespace_name);

    let mut printed = 0;
    for field_addr in ai_td.get_fields() {
        let fd = FieldDefinition::new(field_addr, &mono_reader);
        if fd.type_info.is_static || fd.type_info.is_const {
            continue;
        }
        let code = fd.type_info.clone().code();
        if matches!(code, TypeCode::STRING) {
            let string_ptr = mono_reader.read_ptr(account_info_ptr + fd.offset as usize);
            if string_ptr > 0x10000 {
                match mono_reader.read_mono_string(string_ptr) {
                    Some(v) => println!("  {:<28} = \"{}\"", fd.name, v),
                    None => println!("  {:<28} = <unreadable>", fd.name),
                }
            } else {
                println!("  {:<28} = null", fd.name);
            }
            printed += 1;
        }
    }
    if printed == 0 {
        println!("  (no string fields found - dumping all field names for inspection)");
        for field_addr in ai_td.get_fields() {
            let fd = FieldDefinition::new(field_addr, &mono_reader);
            println!("    {} (offset 0x{:x}, type {})", fd.name, fd.offset, fd.type_info.clone().code());
        }
    }

    println!("\n=== done ===");
}
