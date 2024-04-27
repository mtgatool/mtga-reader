pub mod constants;
pub mod field_definition;
pub mod managed;
pub mod mono_class_kind;
pub mod mono_reader;
pub mod pe_reader;
pub mod type_code;
pub mod type_definition;
pub mod type_info;

use managed::Managed;
use mono_reader::MonoReader;
use type_code::TypeCode;
use type_definition::TypeDefinition;
use type_info::TypeInfo;

use serde_json::json;

use napi_derive::napi;

// Utility fn to get the reader and initialize it
pub fn get_reader(process_name: String) -> Option<MonoReader> {
    let pid = MonoReader::find_pid_by_name(&process_name);

    if pid.is_none() {
        return None;
    }

    let pid = pid.iter().next().unwrap();

    let mut mono_reader = MonoReader::new(pid.as_u32());
    mono_reader.read_mono_root_domain();
    mono_reader.read_assembly_image();
    return Some(mono_reader);
}

pub fn get_def_by_name<'a>(
    defs: &'a Vec<usize>,
    name: String,
    mono_reader: &MonoReader,
) -> Option<&'a usize> {
    defs.iter().find(|def| {
        let main_typedef = TypeDefinition::new(**def, &mono_reader);
        main_typedef.name == name
    })
}

#[napi]
pub fn read_data(process_name: String, fields: Vec<String>) -> serde_json::Value {
    println!("Reading started...");

    let reader = get_reader(process_name);

    match reader {
        None => return json!({ "error": "Process not found" }),
        Some(mut mono_reader) => {
            let defs = mono_reader.create_type_definitions();

            // get the type defs on the root of the assembly for the first loop
            let definition = get_def_by_name(&defs, fields[0].clone(), &mono_reader)
                .unwrap()
                .clone();

            // skipt the first item in the find array
            let find = &fields[1..];

            let mut field = (definition.clone(), TypeInfo::new(definition, &mono_reader));

            for (index, name) in find.iter().enumerate() {
                field = match index {
                    0 => {
                        let class = TypeDefinition::new(definition, &mono_reader);
                        class.get_static_value(name)
                    }
                    _ => {
                        let managed = Managed::new(&mono_reader, field.0, None);
                        let ptr = mono_reader.read_ptr(field.0);
                        let code = field.1.clone().code();
                        let class = match code {
                            TypeCode::GENERICINST => managed.read_generic_instance(field.1.clone()),
                            _ => managed.read_class(),
                        };
                        class.get_value(name, ptr)
                    }
                };
                let code = field.1.clone();
                println!("Find: {}: {} {}", name, code.code(), field.0);
            }

            let managed = Managed::new(&mono_reader, field.0, None);
            let ptr = mono_reader.read_ptr(field.0);
            let code = field.1.clone().code();

            let strout = match code {
                TypeCode::CLASS => {
                    let mut class = managed.read_class();
                    class.set_fields_base(ptr);
                    class.to_string()
                }
                TypeCode::GENERICINST => {
                    let mut class = managed.read_generic_instance(field.1.clone());
                    class.set_fields_base(ptr);
                    class.to_string()
                }
                TypeCode::SZARRAY => managed.read_managed_array(),
                _ => {
                    println!("Code: {} strout not implemented", code);
                    String::from("{}")
                }
            };

            let return_string = strout.clone();

            let clean_str = return_string
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>();
            let json = serde_json::from_str(&clean_str);
            return match json {
                Ok(j) => j,
                Err(e) => {
                    println!("Error: {}", e);
                    serde_json::from_str(&format!("{{ \"error\": \"{}\" }}", e)).unwrap()
                }
            };
        }
    }
}

#[napi]
pub fn read_class(process_name: String, address: i64) -> serde_json::Value {
    let reader = get_reader(process_name);

    match reader {
        None => return json!({ "error": "Process not found" }),
        Some(mono_reader) => {
            let managed = Managed::new(&mono_reader, address as usize, None);
            let ptr = mono_reader.read_ptr(address as usize);

            let mut class = managed.read_class();
            class.set_fields_base(ptr);
            let return_string = class.to_string();

            let clean_str = return_string
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>();
            let json = serde_json::from_str(&clean_str);
            return match json {
                Ok(j) => j,
                Err(e) => {
                    println!("Error: {}", e);
                    serde_json::from_str(&format!("{{ \"error\": \"{}\" }}", e)).unwrap()
                }
            };
        }
    }
}

#[napi]
pub fn read_generic_instance(process_name: String, address: i64) -> serde_json::Value {
    let reader = get_reader(process_name);

    match reader {
        None => return json!({ "error": "Process not found" }),
        Some(mono_reader) => {
            let managed = Managed::new(&mono_reader, address as usize, None);
            let ptr = mono_reader.read_ptr(address as usize);

            let mut class = managed.read_generic_instance(TypeInfo::new(ptr, &mono_reader));
            class.set_fields_base(ptr);
            let return_string = class.to_string();

            let clean_str = return_string
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>();
            let json = serde_json::from_str(&clean_str);
            return match json {
                Ok(j) => j,
                Err(e) => {
                    println!("Error: {}", e);
                    serde_json::from_str(&format!("{{ \"error\": \"{}\" }}", e)).unwrap()
                }
            };
        }
    }
}

#[napi]
pub fn find_pid_by_name(process_name: String) -> bool {
    let results = MonoReader::find_pid_by_name(&process_name);

    return match results {
        Some(_pid) => true,
        None => false,
    };
}

#[test]
fn test_find_no_process() {
    let process_name = "_____test";

    let results = MonoReader::find_pid_by_name(&process_name);

    assert_eq!(results.is_none(), true);
}

#[test]
fn test_find_mtga() {
    let process_name = "MTGA";

    let results = MonoReader::find_pid_by_name(&process_name);

    assert_eq!(results.is_some(), true);
}

#[test]
fn test_read_cards() {
    let path = vec![
        "WrapperController".to_string(),
        "<Instance>k__BackingField".to_string(),
        "<InventoryManager>k__BackingField".to_string(),
        "_inventoryServiceWrapper".to_string(),
        "<Cards>k__BackingField".to_string(),
        "_entries".to_string(),
    ];

    let data = read_data("MTGA".to_string(), path);
    assert_eq!(data.is_array(), true);

    let any_entry = data.get(0).unwrap();
    assert_eq!(any_entry.is_object(), true);
    println!("{:?}", any_entry);
    assert_eq!(any_entry.get("key").unwrap().is_number(), true);
    assert_eq!(any_entry.get("value").unwrap().is_number(), true);
}

#[test]
fn test_read_formats() {
    let path = vec![
        "PAPA".to_string(),
        "_instance".to_string(),
        "_formatManager".to_string(),
        "_formats".to_string(),
        "_items".to_string(),
    ];

    let data = read_data("MTGA".to_string(), path);
    println!("{}", data.to_string());
    assert_eq!(data.is_object(), true);
}

/*
pub fn read_managed<T>(type_code: TypeCode) -> Option<T> {
    match type_code {
        // 1, b => b[0] != 0
        TypeCode::BOOLEAN => Some(self.read_ptr_u8(addr) != 0),

        // char -> char
        TypeCode::CHAR => Some(self.read_ptr_u16(addr)),

        // sizeof(byte), b => b[0]
        TypeCode::I1 => Some(self.read_ptr_i8(addr)),

        // sizeof(sbyte), b => unchecked((sbyte)b[0])
        TypeCode::U1 => Some(self.read_ptr_u8(addr)),

        // short size -> int16
        TypeCode::I2 => Some(self.read_ptr_i16(addr)),

        // ushort size -> uint16
        TypeCode::U2 => Some(self.read_ptr_u16(addr)),

        // int32
        TypeCode::I => Some(self.read_i32(addr)),
        TypeCode::I4 => Some(self.read_i32(addr)),

        // unsigned int32
        TypeCode::U => Some(self.read_u32(addr)),
        TypeCode::U4 => Some(self.read_u32(addr)),

        // char size -> int64
        TypeCode::I8 => Some(self.read_ptr_i64(addr)),

        // char size -> uint64
        TypeCode::U8 => Some(self.read_ptr_u64(addr)),

        // char size -> single
        TypeCode::R4 => Some(self.read_ptr_u32(addr)),
        // char size -> double
        TypeCode::R8 => Some(self.read_i64(addr)),

        TypeCode::STRING => Some(self.read_ascii_string(addr)),

        // ReadManagedArray
        TypeCode::SZARRAY => Some(self.read_ptr_ptr(addr)),

        // try ReadManagedStructInstance
        TypeCode::VALUETYPE => Some(self.read_i32(addr)),

        // ReadManagedClassInstance
        TypeCode::CLASS => Some(self.read_ptr_ptr(addr)),

        // ReadManagedGenericObject
        TypeCode::GENERICINST => Some(self.read_ptr_ptr(addr)),

        // ReadManagedGenericObject
        TypeCode::OBJECT => Some(self.read_ptr_ptr(addr)),

        // ReadManagedVar
        TypeCode::VAR => Some(self.read_ptr_i32(addr)),

        // Junk
        TypeCode::END => Some(0),
        TypeCode::VOID => Some(0),
        TypeCode::PTR => Some(0),
        TypeCode::BYREF => Some(0),
        TypeCode::TYPEDBYREF => Some(0),
        TypeCode::FNPTR => Some(0),
        TypeCode::CMOD_REQD => Some(0),
        TypeCode::CMOD_OPT => Some(0),
        TypeCode::INTERNAL => Some(0),
        TypeCode::MODIFIER => Some(0),
        TypeCode::SENTINEL => Some(0),
        TypeCode::PINNED => Some(0),

        // May need support
        TypeCode::ARRAY => Some(0),
        TypeCode::ENUM => Some(0),
        TypeCode::MVAR => Some(0),
        _ => None,
    }
}
*/
