// New modular backend architecture
pub mod common;
pub mod backend;

#[cfg(feature = "mono")]
pub mod mono;

#[cfg(feature = "il2cpp")]
pub mod il2cpp;

// NAPI bindings for Node.js (only when napi-bindings feature is enabled)
#[cfg(feature = "napi-bindings")]
pub mod napi;

// Legacy modules (kept for backward compatibility)
pub mod constants;
pub mod field_definition;
pub mod managed;
pub mod mono_class_kind;
pub mod mono_reader;
pub mod pe_reader;
pub mod type_code;
pub mod type_definition;
pub mod type_info;
pub mod unity_version;

use managed::Managed;
use mono_reader::MonoReader;
use type_code::TypeCode;
use type_definition::TypeDefinition;
use type_info::TypeInfo;

use serde_json::json;

// Utility fn to get the reader and initialize it (used by Windows backend)
#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
pub fn read_data(process_name: String, fields: Vec<String>) -> serde_json::Value {
    println!("Reading started...");

    let reader = get_reader(process_name);

    match reader {
        None => return json!({ "error": "Process not found" }),
        Some(mut mono_reader) => {
            // First try Assembly-CSharp
            let defs = mono_reader.create_type_definitions();

            // get the type defs on the root of the assembly for the first loop
            let definition = match get_def_by_name(&defs, fields[0].clone(), &mono_reader) {
                Some(def) => def.clone(),
                None => {
                    // Try searching in other assemblies
                    let assemblies = mono_reader.get_all_assembly_names();
                    let mut found_def: Option<usize> = None;

                    for asm_name in assemblies {
                        if asm_name == "Assembly-CSharp" {
                            continue; // Already searched
                        }
                        let asm_image = mono_reader.read_assembly_image_by_name(&asm_name);
                        if asm_image == 0 {
                            continue;
                        }
                        let asm_defs = mono_reader.create_type_definitions_for_image(asm_image);
                        if let Some(def) = get_def_by_name(&asm_defs, fields[0].clone(), &mono_reader) {
                            println!("Found '{}' in assembly '{}'", fields[0], asm_name);
                            found_def = Some(def.clone());
                            break;
                        }
                    }

                    match found_def {
                        Some(def) => def,
                        None => return json!({ "error": format!("Class '{}' not found in any assembly", fields[0]) }),
                    }
                }
            };

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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
