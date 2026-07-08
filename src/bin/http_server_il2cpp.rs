//! IL2CPP HTTP Server for debug-ui
//! Provides the same API as http_server_simple.rs but uses IL2CPP memory reading
//!
//! Run with: sudo cargo run --bin http_server_il2cpp --release

use axum::{
    extract::Path,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

// Use the existing IL2CPP offsets implementation
use mtga_reader::il2cpp::Il2CppOffsets;

// Helper to get shared offsets (inline for convenience)
fn get_offsets() -> Il2CppOffsets {
    Il2CppOffsets::unity_2022_3()
}

// Response types (same as http_server_simple for compatibility)
#[derive(Serialize)]
struct AssembliesResponse {
    assemblies: Vec<String>,
}

#[derive(Serialize, Clone)]
struct ClassInfo {
    name: String,
    namespace: String,
    address: usize,
    is_static: bool,
    is_enum: bool,
}

#[derive(Serialize)]
struct ClassesResponse {
    classes: Vec<ClassInfo>,
}

#[derive(Serialize, Clone)]
struct FieldInfo {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    offset: i32,
    is_static: bool,
    is_const: bool,
}

#[derive(Serialize)]
struct ClassDetailsResponse {
    name: String,
    namespace: String,
    address: usize,
    fields: Vec<FieldInfo>,
}

#[derive(Serialize)]
struct InstanceField {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    is_static: bool,
    value: serde_json::Value,
}

#[derive(Serialize)]
struct InstanceResponse {
    class_name: String,
    namespace: String,
    address: usize,
    fields: Vec<InstanceField>,
}

#[derive(Serialize)]
struct DictionaryEntry {
    key: serde_json::Value,
    value: serde_json::Value,
}

#[derive(Serialize)]
struct DictionaryResponse {
    count: i32,
    entries: Vec<DictionaryEntry>,
}

// Global state
struct Il2CppState {
    reader: MemReader,
    type_info_table: usize,
    class_cache: HashMap<String, usize>,  // name -> class address
    offsets: Il2CppOffsets,  // Use shared offsets
}

static IL2CPP_STATE: Mutex<Option<Il2CppState>> = Mutex::new(None);

fn with_state<F, R>(f: F) -> R
where
    F: FnOnce(&Il2CppState) -> R,
{
    let guard = IL2CPP_STATE.lock().unwrap();
    let state = guard.as_ref().expect("IL2CPP state not initialized");
    f(state)
}

// Handler functions
async fn get_assemblies() -> Json<AssembliesResponse> {
    // IL2CPP doesn't have assemblies like Mono, but we fake it for compatibility
    Json(AssembliesResponse {
        assemblies: vec![
            "GameAssembly".to_string(),
            "MTGA-Classes".to_string(),
        ],
    })
}

async fn get_assembly_classes(
    Path(_assembly_name): Path<String>,
) -> Result<Json<ClassesResponse>, StatusCode> {
    let classes = with_state(|state| {
        let mut classes = Vec::new();
        let mut seen_classes = std::collections::HashSet::new();

        // Scan type info table for all classes
        // Scan up to 50000 entries to catch more classes
        for i in 0..50000 {
            if let Some(entry_addr) = state.type_info_table.checked_add(i * 8) {
                let class_ptr = state.reader.read_ptr(entry_addr);
                if class_ptr == 0 || class_ptr < 0x100000 {
                    continue;
                }

                // Read class name
                if let Some(name_addr) = class_ptr.checked_add(get_offsets().class_name as usize) {
                    let name_ptr = state.reader.read_ptr(name_addr);
                    if name_ptr > 0 && name_ptr < 0x400000000 {
                        let name = state.reader.read_string(name_ptr);
                        if !name.is_empty() && name.len() < 200 {
                            // Filter out compiler-generated classes
                            // These start with '<' or contain "<>" or end with ">d"
                            let is_compiler_generated = name.starts_with('<')
                                || name.contains("<>")
                                || name.ends_with(">d");

                            if !is_compiler_generated {
                                // Avoid duplicates
                                if seen_classes.insert(name.clone()) {
                                    let namespace = read_class_namespace(&state.reader, class_ptr);
                                    classes.push(ClassInfo {
                                        name,
                                        namespace,
                                        address: class_ptr,
                                        is_static: false,
                                        is_enum: false,
                                    });

                                    // Higher limit since we're filtering out compiler classes
                                    if classes.len() >= 5000 {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort by name for easier browsing
        classes.sort_by(|a, b| a.name.cmp(&b.name));
        classes
    });

    Ok(Json(ClassesResponse { classes }))
}

// Search for classes by name pattern
async fn search_classes(
    Path(search_term): Path<String>,
) -> Result<Json<ClassesResponse>, StatusCode> {
    let classes = with_state(|state| {
        let mut classes = Vec::new();
        let mut seen_classes = std::collections::HashSet::new();
        let search_lower = search_term.to_lowercase();

        // Scan type info table for matching classes
        for i in 0..50000 {
            if let Some(entry_addr) = state.type_info_table.checked_add(i * 8) {
                let class_ptr = state.reader.read_ptr(entry_addr);
                if class_ptr == 0 || class_ptr < 0x100000 {
                    continue;
                }

                if let Some(name_addr) = class_ptr.checked_add(get_offsets().class_name as usize) {
                    let name_ptr = state.reader.read_ptr(name_addr);
                    if name_ptr > 0 && name_ptr < 0x400000000 {
                        let name = state.reader.read_string(name_ptr);
                        if !name.is_empty() && name.len() < 200 {
                            // Check if name matches search term (case-insensitive)
                            if name.to_lowercase().contains(&search_lower) {
                                if seen_classes.insert(name.clone()) {
                                    let namespace = read_class_namespace(&state.reader, class_ptr);
                                    classes.push(ClassInfo {
                                        name,
                                        namespace,
                                        address: class_ptr,
                                        is_static: false,
                                        is_enum: false,
                                    });

                                    // Limit search results
                                    if classes.len() >= 200 {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort by name
        classes.sort_by(|a, b| a.name.cmp(&b.name));
        classes
    });

    Ok(Json(ClassesResponse { classes }))
}

async fn get_class_details(
    Path((_assembly_name, class_name)): Path<(String, String)>,
) -> Result<Json<ClassDetailsResponse>, StatusCode> {
    let response = with_state(|state| -> Result<ClassDetailsResponse, StatusCode> {
        let class_addr = find_class_by_name(&state.reader, state.type_info_table, &class_name)
            .ok_or(StatusCode::NOT_FOUND)?;

        let name = read_class_name(&state.reader, class_addr);
        let namespace = read_class_namespace(&state.reader, class_addr);
        let fields = get_class_fields(&state.reader, class_addr);

        Ok(ClassDetailsResponse {
            name,
            namespace,
            address: class_addr,
            fields,
        })
    })?;

    Ok(Json(response))
}

async fn get_instance(
    Path(address_str): Path<String>,
) -> Result<Json<InstanceResponse>, StatusCode> {
    let response = with_state(|state| -> Result<InstanceResponse, StatusCode> {
        let address = parse_address(&address_str).ok_or(StatusCode::BAD_REQUEST)?;

        // In IL2CPP, the class pointer is directly at offset 0 (no vtable indirection like Mono)
        let class_ptr = state.reader.read_ptr(address);
        if class_ptr == 0 || class_ptr < 0x100000 {
            return Err(StatusCode::NOT_FOUND);
        }

        let class_name = read_class_name(&state.reader, class_ptr);
        let namespace = read_class_namespace(&state.reader, class_ptr);

        // Get fields and read values
        let field_defs = get_class_fields(&state.reader, class_ptr);
        let mut fields = Vec::new();

        for field_def in field_defs {
            if field_def.is_static || field_def.offset <= 0 {
                continue;
            }

            let value = read_field_value(&state.reader, address, &field_def);

            fields.push(InstanceField {
                name: field_def.name,
                type_name: field_def.type_name,
                is_static: false,
                value,
            });
        }

        Ok(InstanceResponse {
            class_name,
            namespace,
            address,
            fields,
        })
    })?;

    Ok(Json(response))
}

async fn read_instance_field(
    Path((instance_addr_str, field_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = with_state(|state| -> Result<serde_json::Value, StatusCode> {
        let instance_addr = parse_address(&instance_addr_str).ok_or(StatusCode::BAD_REQUEST)?;

        // Get class and find field
        let class_ptr = state.reader.read_ptr(instance_addr);
        if class_ptr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        let fields = get_class_fields(&state.reader, class_ptr);
        let field = fields.iter()
            .find(|f| f.name == field_name)
            .ok_or(StatusCode::NOT_FOUND)?;

        let value = read_field_value(&state.reader, instance_addr, field);

        Ok(match &value {
            serde_json::Value::Null => serde_json::json!({
                "type": "null",
                "address": 0
            }),
            serde_json::Value::Number(n) => serde_json::json!({
                "type": "primitive",
                "value_type": "int32",
                "value": n
            }),
            serde_json::Value::Bool(b) => serde_json::json!({
                "type": "primitive",
                "value_type": "boolean",
                "value": b
            }),
            serde_json::Value::Object(obj) if obj.contains_key("address") => {
                serde_json::json!({
                    "type": "pointer",
                    "address": obj["address"],
                    "field_name": field_name,
                    "class_name": field.type_name
                })
            },
            _ => serde_json::json!({
                "type": "primitive",
                "value": value
            })
        })
    })?;

    Ok(Json(result))
}

async fn read_static_field(
    Path((class_addr_str, field_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = with_state(|state| -> Result<serde_json::Value, StatusCode> {
        let class_addr = parse_address(&class_addr_str).ok_or(StatusCode::BAD_REQUEST)?;

        let fields = get_class_fields(&state.reader, class_addr);
        let field = fields.iter()
            .find(|f| f.name == field_name && f.is_static)
            .ok_or(StatusCode::NOT_FOUND)?;

        let static_fields = match class_addr.checked_add(get_offsets().class_static_fields as usize) {
            Some(addr) => state.reader.read_ptr(addr),
            None => return Ok(serde_json::Value::Null),
        };
        if static_fields == 0 {
            return Ok(serde_json::Value::Null);
        }

        let field_addr = match static_fields.checked_add(field.offset as usize) {
            Some(addr) => addr,
            None => return Ok(serde_json::Value::Null),
        };

        // Handle primitive types correctly (read as their actual size, not as pointer)
        let type_name = &field.type_name;
        if type_name.contains("UInt32") || type_name == "uint" {
            let value = state.reader.read_u32(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "uint32",
                "value": value
            }));
        }
        if type_name.contains("Int32") || type_name == "int" {
            let value = state.reader.read_i32(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "int32",
                "value": value
            }));
        }
        if type_name.contains("UInt64") || type_name == "ulong" {
            let value = state.reader.read_u64(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "uint64",
                "value": value
            }));
        }
        if type_name.contains("Int64") || type_name == "long" {
            let value = state.reader.read_i64(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "int64",
                "value": value
            }));
        }
        if type_name.contains("Single") || type_name == "float" {
            let value = state.reader.read_f32(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "float",
                "value": value
            }));
        }
        if type_name.contains("Double") || type_name == "double" {
            let value = state.reader.read_f64(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "double",
                "value": value
            }));
        }
        if type_name.contains("Boolean") || type_name == "bool" {
            let value = state.reader.read_u8(field_addr) != 0;
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "bool",
                "value": value
            }));
        }
        if type_name.contains("Byte") || type_name == "byte" {
            let value = state.reader.read_u8(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "byte",
                "value": value
            }));
        }
        if type_name.contains("SByte") || type_name == "sbyte" {
            let value = state.reader.read_i8(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "sbyte",
                "value": value
            }));
        }
        if type_name.contains("Int16") || type_name == "short" {
            let value = state.reader.read_i16(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "int16",
                "value": value
            }));
        }
        if type_name.contains("UInt16") || type_name == "ushort" {
            let value = state.reader.read_u16(field_addr);
            return Ok(serde_json::json!({
                "type": "primitive",
                "value_type": "uint16",
                "value": value
            }));
        }

        // For reference types, read as pointer
        let value = state.reader.read_ptr(field_addr);

        if value == 0 {
            Ok(serde_json::json!({
                "type": "null",
                "address": 0
            }))
        } else {
            Ok(serde_json::json!({
                "type": "pointer",
                "address": value,
                "field_name": field_name,
                "class_name": field.type_name
            }))
        }
    })?;

    Ok(Json(result))
}

async fn read_dictionary(
    Path(dict_addr_str): Path<String>,
) -> Result<Json<DictionaryResponse>, StatusCode> {
    let result = with_state(|state| -> Result<DictionaryResponse, StatusCode> {
        let dict_addr = parse_address(&dict_addr_str).ok_or(StatusCode::BAD_REQUEST)?;

        if dict_addr == 0 {
            return Err(StatusCode::BAD_REQUEST);
        }

        // Check class name to determine how to read
        let class_ptr = state.reader.read_ptr(dict_addr);
        let class_name = read_class_name(&state.reader, class_ptr);

        // For CardsAndQuantity (custom class used by MTGA)
        if class_name == "CardsAndQuantity" {
            return read_cards_and_quantity(&state.reader, dict_addr);
        }

        // Try standard Dictionary layouts
        // Pattern 1: entries at +0x18, count at +0x20
        let entries_ptr = match dict_addr.checked_add(0x18) {
            Some(addr) => state.reader.read_ptr(addr),
            None => 0,
        };
        let count = match dict_addr.checked_add(0x20) {
            Some(addr) => state.reader.read_i32(addr),
            None => 0,
        };

        if entries_ptr > 0x100000 && count > 0 && count < 100000 {
            let arr_len = match entries_ptr.checked_add(0x18) {
                Some(addr) => state.reader.read_u32(addr),
                None => 0,
            };
            if arr_len > 0 {
                return read_dict_entries(&state.reader, entries_ptr, count);
            }
        }

        // Pattern 2: entries at +0x10
        let entries_ptr = match dict_addr.checked_add(0x10) {
            Some(addr) => state.reader.read_ptr(addr),
            None => 0,
        };
        if entries_ptr > 0x100000 {
            let arr_len = match entries_ptr.checked_add(0x18) {
                Some(addr) => state.reader.read_u32(addr),
                None => 0,
            };
            if arr_len > 0 && arr_len < 200000 {
                let count = arr_len as i32;
                return read_dict_entries(&state.reader, entries_ptr, count);
            }
        }

        Err(StatusCode::NOT_FOUND)
    })?;

    Ok(Json(result))
}


// Helper functions
fn parse_address(s: &str) -> Option<usize> {
    if s.starts_with("0x") {
        usize::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse().ok()
    }
}

fn find_class_by_name(reader: &MemReader, type_info_table: usize, name: &str) -> Option<usize> {
    let mut found_classes = Vec::new();
    let mut valid_count = 0;

    for i in 0..50000 {
        let class_ptr = match type_info_table.checked_add(i * 8) {
            Some(addr) => reader.read_ptr(addr),
            None => continue,
        };
        if class_ptr == 0 {
            continue;
        }
        let name_ptr = match class_ptr.checked_add(get_offsets().class_name as usize) {
            Some(addr) => reader.read_ptr(addr),
            None => continue,
        };
        if name_ptr == 0 {
            continue;
        }
        let class_name = reader.read_string(name_ptr);
        if !class_name.is_empty() {
            valid_count += 1;
            if valid_count <= 10 {
                found_classes.push(class_name.clone());
            }
            if class_name == name {
                return Some(class_ptr);
            }
        }
    }

    // Print debug info if class not found
    if !found_classes.is_empty() {
        eprintln!("Found {} valid classes. First 10:", valid_count);
        for class in &found_classes {
            eprintln!("  - {}", class);
        }
    } else {
        eprintln!("No valid classes found. Type info table might be incorrect.");
    }

    None
}

fn read_class_name(reader: &MemReader, class: usize) -> String {
    if class == 0 || class < 0x100000 {
        return String::new();
    }
    let name_ptr = match class.checked_add(get_offsets().class_name as usize) {
        Some(addr) => reader.read_ptr(addr),
        None => return String::new(),
    };
    reader.read_string(name_ptr)
}

fn read_class_namespace(reader: &MemReader, class: usize) -> String {
    if class == 0 || class < 0x100000 {
        return String::new();
    }
    let ns_ptr = match class.checked_add(get_offsets().class_namespace as usize) {
        Some(addr) => reader.read_ptr(addr),
        None => return String::new(),
    };
    reader.read_string(ns_ptr)
}

fn get_class_fields(reader: &MemReader, class_addr: usize) -> Vec<FieldInfo> {
    // Wrapper to get type_info_table from state
    with_state(|state| -> Result<Vec<FieldInfo>, StatusCode> {
        Ok(get_class_fields_internal(reader, class_addr, state.type_info_table))
    }).unwrap_or_else(|_| Vec::new())
}

fn read_type_name(reader: &MemReader, type_ptr: usize) -> String {
    read_type_name_with_table(reader, type_ptr, 0)
}

fn read_type_name_with_table(reader: &MemReader, type_ptr: usize, type_info_table: usize) -> String {
    if type_ptr == 0 || type_ptr < 0x100000 {
        return "unknown".to_string();
    }

    // Read Il2CppType structure
    // attrs field contains: type code in low byte, attribute flags in upper bits
    let attrs = match type_ptr.checked_add(get_offsets().type_attrs as usize) {
        Some(addr) => reader.read_u32(addr),
        None => return "unknown".to_string(),
    };
    let type_enum = (attrs & 0xFF) as u8;  // Type code is in lowest byte

    // Debug first few reads
    static DEBUG_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let count = DEBUG_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if count < 15 {
        // Read what's in the Il2CppType structure
        let data_at_0 = reader.read_ptr(type_ptr);
        let u32_at_8 = reader.read_u32(type_ptr + 8);
        let u32_at_c = reader.read_u32(type_ptr + 12);
        eprintln!("    read_type_name: type_ptr=0x{:x}", type_ptr);
        eprintln!("      Il2CppType.data (+0x0): 0x{:x}", data_at_0);
        eprintln!("      Il2CppType.attrs (+0x8): 0x{:08x} (type_enum=0x{:02x})", attrs, type_enum);
        eprintln!("      Il2CppType.??? (+0xc): 0x{:08x}", u32_at_c);
    }

    // Handle primitive types
    match type_enum {
        0x01 => {
            // SPECIAL CASE: In IL2CPP, instance fields often have type_enum=0x01 but
            // still have valid type_data. Try to read it.
            let type_data_test = match type_ptr.checked_add(get_offsets().type_data as usize) {
                Some(addr) => reader.read_ptr(addr),
                None => 0,
            };
            if type_data_test > 0x100000 && type_data_test < 0x400000000 {
                if count < 15 {
                    eprintln!("      VOID with type_data=0x{:x}, will try to read as class", type_data_test);
                }
                // Fall through to try reading type_data as a class pointer
            } else {
                return "void".to_string();
            }
        }
        0x02 => return "bool".to_string(),
        0x03 => return "char".to_string(),
        0x04 => return "sbyte".to_string(),
        0x05 => return "byte".to_string(),
        0x06 => return "short".to_string(),
        0x07 => return "ushort".to_string(),
        0x08 => return "int".to_string(),
        0x09 => return "uint".to_string(),
        0x0a => return "long".to_string(),
        0x0b => return "ulong".to_string(),
        0x0c => return "float".to_string(),
        0x0d => return "double".to_string(),
        0x0e => return "string".to_string(),
        0x18 => return "nint".to_string(),
        0x19 => return "nuint".to_string(),
        0x1c => return "object".to_string(),
        _ => {}
    }

    // For reference types (CLASS, VALUETYPE, etc.), read the data pointer
    let type_data = match type_ptr.checked_add(get_offsets().type_data as usize) {
        Some(addr) => reader.read_ptr(addr),
        None => return "Unknown".to_string(),
    };

    if count < 15 {
        eprintln!("      type_data=0x{:x}", type_data);
    }

    if type_data == 0 || type_data < 0x100000 {
        if count < 15 {
            eprintln!("      -> Invalid type_data, returning Type_{:x}", type_enum);
        }
        return format!("Type_{:x}", type_enum);
    }

    // IL2CPP Type Data Interpretation:
    // In IL2CPP metadata v29+ (Unity 2021+), type_data is a union that can be:
    // - klassIndex (TypeDefinitionIndex) for CLASS/VALUETYPE
    // - Il2CppClass* pointer in some cases
    // Try multiple interpretations:

    // 1. Try type_data as klassIndex (treating as small integer index)
    if type_info_table != 0 && type_data < 100000 {
        let class_ptr = reader.read_ptr(type_info_table + type_data * 8);
        if class_ptr > 0x100000 && class_ptr < 0x400000000 {
            let name = read_class_name(reader, class_ptr);
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_graphic() || c == '_' || c == '<' || c == '>') {
                if count < 15 {
                    eprintln!("      -> type_info_table[{}] = 0x{:x} -> name='{}'", type_data, class_ptr, name);
                }
                let namespace = read_class_namespace(reader, class_ptr);
                return if namespace.is_empty() { name } else { format!("{}.{}", namespace, name) };
            }
        }
    }

    // 2. Try type_data as direct Il2CppClass pointer
    // Scan multiple possible class_name offsets since structure might vary
    for name_offset in [0x10, 0x18, 0x20, 0x28, 0x30] {
        if let Some(test_addr) = type_data.checked_add(name_offset) {
            let name_ptr = reader.read_ptr(test_addr);
            if name_ptr > 0x100000 && name_ptr < 0x400000000 {
                let name = reader.read_string(name_ptr);
                if !name.is_empty() && name.len() < 200 && name.chars().all(|c| c.is_ascii_graphic() || c == '_' || c == '<' || c == '>') {
                    if count < 15 {
                        eprintln!("      -> Direct Il2CppClass* at type_data+0x{:x} worked: '{}'", name_offset, name);
                    }
                    // Found valid name! Use this offset for namespace too
                    let ns_offset = get_offsets().class_namespace as usize;
                    let namespace = if let Some(ns_addr) = type_data.checked_add(ns_offset) {
                        let ns_ptr = reader.read_ptr(ns_addr);
                        if ns_ptr > 0x100000 { reader.read_string(ns_ptr) } else { String::new() }
                    } else {
                        String::new()
                    };
                    return if namespace.is_empty() { name } else { format!("{}.{}", namespace, name) };
                }
            }
        }
    }

    // Fallback: format based on type_enum and type_data address
    if count < 15 {
        eprintln!("      -> All lookups failed, using fallback format");
    }

    match type_enum {
        0x12 => format!("Class_0x{:x}", type_data),
        0x11 => format!("ValueType_0x{:x}", type_data),
        0x1d => format!("Unknown[]"),
        0x14 => format!("Unknown[...]"),
        0x0f => format!("Unknown*"),
        0x15 => format!("Generic<...>"),
        _ => format!("Type_{:x}", type_enum),
    }
}

fn get_class_fields_internal(reader: &MemReader, class_addr: usize, type_info_table: usize) -> Vec<FieldInfo> {
    let mut fields = Vec::new();
    let offsets = get_offsets();

    // Read field count using the correct offset from offsets table
    let field_count_addr = match class_addr.checked_add(offsets.class_field_count as usize) {
        Some(addr) => addr,
        None => {
            eprintln!("DEBUG get_class_fields: Failed to add field_count offset to class_addr");
            return fields;
        }
    };

    let field_count = reader.read_i32(field_count_addr);

    let fields_ptr = match class_addr.checked_add(offsets.class_fields as usize) {
        Some(addr) => reader.read_ptr(addr),
        None => {
            eprintln!("DEBUG get_class_fields: Failed to add class_fields offset to class_addr");
            return fields;
        }
    };

    eprintln!("DEBUG get_class_fields: class_addr=0x{:x}, field_count={}, fields_ptr=0x{:x}",
              class_addr, field_count, fields_ptr);

    if fields_ptr == 0 || fields_ptr < 0x100000 {
        eprintln!("DEBUG get_class_fields: Invalid fields_ptr");
        return fields;
    }

    // Determine how many fields to scan
    // Some IL2CPP class types (MonoBehaviours, singletons, etc.) may have field_count
    // at different offsets. If field_count is unreasonable, scan until we hit invalid fields.
    let max_fields = if field_count > 0 && field_count < 500 {
        field_count as usize
    } else {
        eprintln!("DEBUG get_class_fields: Unreasonable field_count: {}, will scan up to 200 fields", field_count);
        200  // Scan until we hit invalid field names
    };

    for i in 0..max_fields {
        let field = match fields_ptr.checked_add(i * 0x20) {
            Some(addr) => addr,
            None => break,
        };
        let name_ptr = reader.read_ptr(field);
        if name_ptr == 0 || name_ptr < 0x100000 {
            eprintln!("DEBUG: field {} has invalid name_ptr: 0x{:x}", i, name_ptr);
            break;
        }

        let name = reader.read_string(name_ptr);

        // Stop if we hit garbage field names (when scanning without valid field_count)
        if name.is_empty() || name.len() > 200 || !name.chars().all(|c| c.is_ascii_graphic() || c == '_') {
            eprintln!("DEBUG: field {} has invalid name: {:?}, stopping scan", i, name);
            break;
        }
        let offset = match field.checked_add(get_offsets().field_offset as usize) {
            Some(addr) => reader.read_i32(addr),
            None => break,
        };

        let type_ptr = match field.checked_add(get_offsets().field_type as usize) {
            Some(addr) => reader.read_ptr(addr),
            None => break,
        };

        // Debug: Read all pointers in field structure to see what's there
        if i < 5 {
            eprintln!("  Field[{}] '{}' at field_addr=0x{:x}:", i, name, field);
            for off in [0x0, 0x8, 0x10, 0x18] {
                let val = reader.read_ptr(field + off);
                eprintln!("    +0x{:02x}: 0x{:x}", off, val);
            }
        }

        // Read type attributes to determine static/const flags
        let (is_static, is_const) = if type_ptr > 0 && type_ptr < 0x400000000 {
            match type_ptr.checked_add(get_offsets().type_attrs as usize) {
                Some(addr) => {
                    let attrs = reader.read_u32(addr);
                    let is_static = (attrs & 0x10) != 0;
                    let is_const = (attrs & 0x40) != 0;

                    if i < 5 {
                        eprintln!("    type_ptr=0x{:x}, attrs=0x{:08x}, type_enum=0x{:02x}, is_static={}, is_const={}",
                                 type_ptr, attrs, attrs & 0xFF, is_static, is_const);
                    }

                    (is_static, is_const)
                }
                None => (false, false),
            }
        } else {
            (false, false)
        };

        // Read type name using proper IL2CPP type decoding
        let type_name = read_type_name_with_table(reader, type_ptr, type_info_table);

        fields.push(FieldInfo {
            name,
            type_name,
            offset,
            is_static,
            is_const,
        });
    }

    fields
}

fn read_field_value(reader: &MemReader, instance_addr: usize, field: &FieldInfo) -> serde_json::Value {
    let field_addr = instance_addr + field.offset as usize;
    let type_name = &field.type_name;

    // Handle primitive types correctly (read as their actual size)
    // Note: Check UInt32 before Int32 since "UInt32" contains "Int32"
    if type_name.contains("UInt32") || type_name == "uint" {
        return serde_json::json!(reader.read_u32(field_addr));
    }
    if type_name.contains("Int32") || type_name == "int" {
        return serde_json::json!(reader.read_i32(field_addr));
    }
    if type_name.contains("UInt64") || type_name == "ulong" {
        return serde_json::json!(reader.read_u64(field_addr));
    }
    if type_name.contains("Int64") || type_name == "long" {
        return serde_json::json!(reader.read_i64(field_addr));
    }
    if type_name.contains("Single") || type_name == "float" {
        return serde_json::json!(reader.read_f32(field_addr));
    }
    if type_name.contains("Double") || type_name == "double" {
        return serde_json::json!(reader.read_f64(field_addr));
    }
    if type_name.contains("Boolean") || type_name == "bool" {
        return serde_json::json!(reader.read_u8(field_addr) != 0);
    }
    if type_name.contains("Byte") || type_name == "byte" {
        return serde_json::json!(reader.read_u8(field_addr));
    }
    if type_name.contains("SByte") || type_name == "sbyte" {
        return serde_json::json!(reader.read_i8(field_addr));
    }
    if type_name.contains("Int16") || type_name == "short" {
        return serde_json::json!(reader.read_i16(field_addr));
    }
    if type_name.contains("UInt16") || type_name == "ushort" {
        return serde_json::json!(reader.read_u16(field_addr));
    }

    // Try reading as pointer for reference types
    let ptr = reader.read_ptr(field_addr);
    if ptr == 0 {
        return serde_json::Value::Null;
    }

    // Check if it's a valid object pointer
    if ptr > 0x100000 && ptr < 0x400000000 {
        let class_ptr = reader.read_ptr(ptr);
        let class_name = read_class_name(reader, class_ptr);

        return serde_json::json!({
            "type": "pointer",
            "address": ptr,
            "class_name": if class_name.is_empty() { field.type_name.clone() } else { class_name }
        });
    }

    // Fallback - might be a small integer stored directly
    serde_json::json!(reader.read_i32(field_addr))
}

fn read_cards_and_quantity(reader: &MemReader, cards_addr: usize) -> Result<DictionaryResponse, StatusCode> {
    // CardsAndQuantity structure:
    // +0x18: entries (Entry[])
    // +0x20: count
    let entries_ptr = match cards_addr.checked_add(0x18) {
        Some(addr) => reader.read_ptr(addr),
        None => return Err(StatusCode::NOT_FOUND),
    };
    let count = match cards_addr.checked_add(0x20) {
        Some(addr) => reader.read_i32(addr),
        None => return Err(StatusCode::NOT_FOUND),
    };

    if entries_ptr == 0 || count <= 0 || count > 100000 {
        return Err(StatusCode::NOT_FOUND);
    }

    read_dict_entries(reader, entries_ptr, count)
}

fn read_dict_entries(reader: &MemReader, entries_ptr: usize, count: i32) -> Result<DictionaryResponse, StatusCode> {
    let mut entries = Vec::new();
    let max_read = count.min(5000) as usize;  // Limit for performance

    for i in 0..max_read {
        let entry_addr = match entries_ptr.checked_add(0x20) {
            Some(addr) => match addr.checked_add(i * 16) {
                Some(e) => e,
                None => break,
            },
            None => break,
        };
        let hash = reader.read_i32(entry_addr);
        let key = match entry_addr.checked_add(8) {
            Some(addr) => reader.read_i32(addr),
            None => break,
        };
        let value = match entry_addr.checked_add(12) {
            Some(addr) => reader.read_i32(addr),
            None => break,
        };

        if hash >= 0 && key > 0 {
            entries.push(DictionaryEntry {
                key: serde_json::json!(key),
                value: serde_json::json!(value),
            });
        }
    }

    Ok(DictionaryResponse {
        count: entries.len() as i32,
        entries,
    })
}


fn find_second_data_segment(pid: u32) -> usize {
    let output = Command::new("vmmap")
        .args(["-wide", &pid.to_string()])
        .output()
        .expect("vmmap failed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut found_first = false;
    let mut count = 0;

    eprintln!("Searching for GameAssembly __DATA segments:");
    for line in stdout.lines() {
        if line.contains("GameAssembly") && line.contains("__DATA") && !line.contains("__DATA_CONST") {
            count += 1;
            eprintln!("  Found __DATA segment #{}: {}", count, line.trim());
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let addr_parts: Vec<&str> = parts[1].split('-').collect();
                if let Ok(start) = usize::from_str_radix(addr_parts[0], 16) {
                    if found_first {
                        eprintln!("  Using second segment at: 0x{:x}", start);
                        return start;
                    }
                    found_first = true;
                }
            }
        }
    }
    eprintln!("  Total __DATA segments found: {}", count);
    0
}

// Memory reader (same as in examples)
struct MemReader {
    task_port: u32,
}

impl MemReader {
    fn new(pid: u32) -> Self {
        // This IL2CPP server relies on macOS mach APIs; on other platforms it
        // compiles to a non-functional stub so `cargo build --bins` succeeds.
        #[cfg(target_os = "macos")]
        let task_port = unsafe {
            let mut task: u32 = 0;
            mach2::traps::task_for_pid(mach2::traps::mach_task_self(), pid as i32, &mut task);
            task
        };
        #[cfg(not(target_os = "macos"))]
        let task_port = {
            let _ = pid;
            0u32
        };
        MemReader { task_port }
    }

    fn read_bytes(&self, addr: usize, size: usize) -> Vec<u8> {
        let mut buffer = vec![0u8; size];
        #[cfg(target_os = "macos")]
        unsafe {
            let mut out_size: u64 = 0;
            mach2::vm::mach_vm_read_overwrite(
                self.task_port,
                addr as u64,
                size as u64,
                buffer.as_mut_ptr() as u64,
                &mut out_size,
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = addr;
        }
        buffer
    }

    fn read_ptr(&self, addr: usize) -> usize {
        let bytes = self.read_bytes(addr, 8);
        usize::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
    }

    fn read_i32(&self, addr: usize) -> i32 {
        let bytes = self.read_bytes(addr, 4);
        i32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
    }

    fn read_u32(&self, addr: usize) -> u32 {
        let bytes = self.read_bytes(addr, 4);
        u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
    }

    fn read_u8(&self, addr: usize) -> u8 {
        let bytes = self.read_bytes(addr, 1);
        bytes.first().copied().unwrap_or(0)
    }

    fn read_i8(&self, addr: usize) -> i8 {
        self.read_u8(addr) as i8
    }

    fn read_u16(&self, addr: usize) -> u16 {
        let bytes = self.read_bytes(addr, 2);
        u16::from_le_bytes(bytes.try_into().unwrap_or([0; 2]))
    }

    fn read_i16(&self, addr: usize) -> i16 {
        let bytes = self.read_bytes(addr, 2);
        i16::from_le_bytes(bytes.try_into().unwrap_or([0; 2]))
    }

    fn read_u64(&self, addr: usize) -> u64 {
        let bytes = self.read_bytes(addr, 8);
        u64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
    }

    fn read_i64(&self, addr: usize) -> i64 {
        let bytes = self.read_bytes(addr, 8);
        i64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
    }

    fn read_f32(&self, addr: usize) -> f32 {
        let bytes = self.read_bytes(addr, 4);
        f32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
    }

    fn read_f64(&self, addr: usize) -> f64 {
        let bytes = self.read_bytes(addr, 8);
        f64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
    }

    fn read_string(&self, addr: usize) -> String {
        if addr == 0 {
            return String::new();
        }
        let bytes = self.read_bytes(addr, 256);
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).to_string()
    }
}

#[tokio::main]
async fn main() {
    println!("Starting MTGA Reader HTTP Server (IL2CPP)...");

    // Find MTGA process
    let output = Command::new("pgrep").arg("MTGA").output().expect("pgrep failed");
    let pid: u32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .lines()
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);

    if pid == 0 {
        eprintln!("Error: MTGA process not found. Please start MTGA first.");
        std::process::exit(1);
    }

    println!("Found MTGA process with PID: {}", pid);

    // Initialize memory reader
    let reader = MemReader::new(pid);

    // Find type info table
    let data_base = find_second_data_segment(pid);
    if data_base == 0 {
        eprintln!("Error: Could not find GameAssembly __DATA segment");
        std::process::exit(1);
    }

    println!("Data base: 0x{:x}", data_base);
    println!("Scanning for type info table...");

    // First, try known offsets
    let known_offsets = [0x24360, 0x24350, 0x24370, 0x24340, 0x24380, 0x243A0, 0x243C0, 0x24400];
    let mut type_info_table = 0;

    for offset in known_offsets {
        if let Some(addr) = data_base.checked_add(offset) {
            let table = reader.read_ptr(addr);

            if table > 0x100000 && table < 0x400000000 {
                // Try to validate by reading entries
                let mut valid_count = 0;
                for i in 0..20 {
                    if let Some(entry_addr) = table.checked_add(i * 8) {
                        let class_ptr = reader.read_ptr(entry_addr);
                        if class_ptr > 0x100000 && class_ptr < 0x400000000 {
                            if let Some(name_addr) = class_ptr.checked_add(get_offsets().class_name as usize) {
                                let name_ptr = reader.read_ptr(name_addr);
                                if name_ptr > 0 && name_ptr < 0x400000000 {
                                    let name = reader.read_string(name_ptr);
                                    if !name.is_empty() && name.len() < 100 && name.chars().all(|c| c.is_ascii_graphic() || c == '_') {
                                        valid_count += 1;
                                    }
                                }
                            }
                        }
                    }
                }

                if valid_count >= 3 {
                    println!("✓ Offset 0x{:x} -> table: 0x{:x} ({} valid entries)", offset, table, valid_count);
                    type_info_table = table;
                    break;
                } else {
                    println!("  Offset 0x{:x} -> table: 0x{:x} (only {} valid entries, skipping)", offset, table, valid_count);
                }
            }
        }
    }

    // If not found, scan the first 256KB of the DATA segment
    if type_info_table == 0 {
        println!("Known offsets failed. Scanning DATA segment for type info table...");
        println!("This may take a moment...");

        for offset in (0..0x40000).step_by(8) {
            if let Some(addr) = data_base.checked_add(offset) {
                let table = reader.read_ptr(addr);

                if table > 0x100000 && table < 0x400000000 {
                    let mut valid_count = 0;
                    let mut sample_names = Vec::new();

                    for i in 0..30 {
                        if let Some(entry_addr) = table.checked_add(i * 8) {
                            let class_ptr = reader.read_ptr(entry_addr);
                            if class_ptr > 0x100000 && class_ptr < 0x400000000 {
                                if let Some(name_addr) = class_ptr.checked_add(get_offsets().class_name as usize) {
                                    let name_ptr = reader.read_ptr(name_addr);
                                    if name_ptr > 0 && name_ptr < 0x400000000 {
                                        let name = reader.read_string(name_ptr);
                                        if !name.is_empty() && name.len() < 100 && name.chars().all(|c| c.is_ascii_graphic() || c == '_') {
                                            valid_count += 1;
                                            if sample_names.len() < 3 {
                                                sample_names.push(name);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if valid_count >= 10 {
                        println!("✓ Found potential table at offset 0x{:x} -> 0x{:x} ({} valid entries)", offset, table, valid_count);
                        println!("  Sample classes: {:?}", sample_names);
                        type_info_table = table;
                        break;
                    }
                }
            }
        }
    }

    if type_info_table == 0 {
        eprintln!("\nError: Could not find valid type info table");
        eprintln!("This could mean:");
        eprintln!("  1. The game uses a different IL2CPP version");
        eprintln!("  2. The class structure offsets have changed");
        eprintln!("  3. The game is protected/obfuscated");
        std::process::exit(1);
    }

    println!("Using type info table: 0x{:x}", type_info_table);

    // Initialize state with shared offsets
    let offsets = get_offsets();
    println!("\nUsing IL2CPP Offsets (Unity 2021.x):");
    println!("  class_fields: 0x{:x}", offsets.class_fields);
    println!("  class_static_fields: 0x{:x}", offsets.class_static_fields);
    println!("  field_offset: 0x{:x}", offsets.field_offset);
    println!("  field_type: 0x{:x}", offsets.field_type);
    println!("  type_attrs: 0x{:x}", offsets.type_attrs);

    {
        let mut state = IL2CPP_STATE.lock().unwrap();
        *state = Some(Il2CppState {
            reader,
            type_info_table,
            class_cache: HashMap::new(),
            offsets,
        });
    }

    println!("\nIL2CPP backend initialized");

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        // Generic IL2CPP memory exploration API
        .route("/assemblies", get(get_assemblies))
        .route("/assembly/:name/classes", get(get_assembly_classes))
        .route("/search/:term", get(search_classes))
        .route("/assembly/:assembly/class/:class", get(get_class_details))
        .route("/instance/:address", get(get_instance))
        .route("/instance/:address/field/:field_name", get(read_instance_field))
        .route("/class/:address/field/:field_name", get(read_static_field))
        .route("/dictionary/:address", get(read_dictionary))
        .layer(cors);

    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("Failed to bind to port 8080");

    println!("\nServer listening on http://127.0.0.1:8080");
    println!("\nGeneric IL2CPP Memory Explorer API:");
    println!("  GET /assemblies                           - List assemblies");
    println!("  GET /assembly/:name/classes               - List all classes (filters compiler-generated)");
    println!("  GET /search/:term                         - Search classes by name (case-insensitive)");
    println!("  GET /assembly/:assembly/class/:class      - Get class details with fields & types");
    println!("  GET /instance/:address                    - Read instance fields at address");
    println!("  GET /instance/:address/field/:field_name  - Read specific instance field");
    println!("  GET /class/:address/field/:field_name     - Read static field value");
    println!("  GET /dictionary/:address                  - Read dictionary entries");
    println!("\nPress Ctrl+C to stop");

    axum::serve(listener, app).await.expect("Server error");
}
