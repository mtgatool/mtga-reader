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

// IL2CPP offsets
mod offsets {
    pub const CLASS_NAME: usize = 0x10;
    pub const CLASS_NAMESPACE: usize = 0x18;
    pub const CLASS_FIELDS: usize = 0x80;
    pub const CLASS_STATIC_FIELDS: usize = 0xA8;
    pub const FIELD_INFO_SIZE: usize = 32;
    pub const FIELD_TYPE: usize = 0x08;
    pub const FIELD_OFFSET: usize = 0x18;
    pub const TYPE_ATTRS: usize = 0x08;
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
struct StaticInstanceInfo {
    field_name: String,
    address: usize,
}

#[derive(Serialize)]
struct ClassDetailsResponse {
    name: String,
    namespace: String,
    address: usize,
    fields: Vec<FieldInfo>,
    static_instances: Vec<StaticInstanceInfo>,
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
    papa_instance: usize,
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
    Path(assembly_name): Path<String>,
) -> Result<Json<ClassesResponse>, StatusCode> {
    let classes = with_state(|state| {
        let mut classes = Vec::new();

        // Return interesting classes based on assembly name
        let class_names: Vec<&str> = if assembly_name == "GameAssembly" || assembly_name == "MTGA-Classes" {
            vec![
                "PAPA",
                "WrapperController",
                "InventoryManager",
                "AwsInventoryServiceWrapper",
                "CardDatabase",
                "ClientPlayerInventory",
                "CardsAndQuantity",
            ]
        } else {
            vec![]
        };

        for name in class_names {
            if let Some(class_addr) = find_class_by_name(&state.reader, state.type_info_table, name) {
                let namespace = read_class_namespace(&state.reader, class_addr);
                classes.push(ClassInfo {
                    name: name.to_string(),
                    namespace,
                    address: class_addr,
                    is_static: false,
                    is_enum: false,
                });
            }
        }

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

        // Find static instances (for singleton patterns)
        let mut static_instances = Vec::new();

        // Special handling for PAPA - we have a known instance
        if class_name == "PAPA" && state.papa_instance != 0 {
            static_instances.push(StaticInstanceInfo {
                field_name: "_instance".to_string(),
                address: state.papa_instance,
            });
        }

        // Check static fields for instance patterns
        for field in &fields {
            if field.is_static && (field.name.contains("instance") || field.name.contains("Instance")) {
                let static_fields = state.reader.read_ptr(class_addr + offsets::CLASS_STATIC_FIELDS);
                if static_fields > 0x100000 {
                    let ptr = state.reader.read_ptr(static_fields + field.offset as usize);
                    if ptr > 0x100000 && ptr < 0x400000000 {
                        static_instances.push(StaticInstanceInfo {
                            field_name: field.name.clone(),
                            address: ptr,
                        });
                    }
                }
            }
        }

        Ok(ClassDetailsResponse {
            name,
            namespace,
            address: class_addr,
            fields,
            static_instances,
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

        let static_fields = state.reader.read_ptr(class_addr + offsets::CLASS_STATIC_FIELDS);
        if static_fields == 0 {
            return Ok(serde_json::Value::Null);
        }

        let field_addr = static_fields + field.offset as usize;

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
        let entries_ptr = state.reader.read_ptr(dict_addr + 0x18);
        let count = state.reader.read_i32(dict_addr + 0x20);

        if entries_ptr > 0x100000 && count > 0 && count < 100000 {
            let arr_len = state.reader.read_u32(entries_ptr + 0x18);
            if arr_len > 0 {
                return read_dict_entries(&state.reader, entries_ptr, count);
            }
        }

        // Pattern 2: entries at +0x10
        let entries_ptr = state.reader.read_ptr(dict_addr + 0x10);
        if entries_ptr > 0x100000 {
            let arr_len = state.reader.read_u32(entries_ptr + 0x18);
            if arr_len > 0 && arr_len < 200000 {
                let count = arr_len as i32;
                return read_dict_entries(&state.reader, entries_ptr, count);
            }
        }

        Err(StatusCode::NOT_FOUND)
    })?;

    Ok(Json(result))
}

// Special endpoint to get card collection
async fn get_cards() -> Result<Json<DictionaryResponse>, StatusCode> {
    let result = with_state(|state| -> Result<DictionaryResponse, StatusCode> {
        if state.papa_instance == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        // Navigate: PAPA (+224) -> InventoryManager (+56) -> AwsInventoryServiceWrapper (+72) -> Cards
        let inv_mgr_ptr = state.reader.read_ptr(state.papa_instance + 224);
        if inv_mgr_ptr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        let aws_wrapper_ptr = state.reader.read_ptr(inv_mgr_ptr + 56);
        if aws_wrapper_ptr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        let cards_ptr = state.reader.read_ptr(aws_wrapper_ptr + 72);
        if cards_ptr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        read_cards_and_quantity(&state.reader, cards_ptr)
    })?;

    Ok(Json(result))
}

// Special endpoint to get player inventory
#[derive(Serialize)]
struct InventoryResponse {
    wildcards_common: i32,
    wildcards_uncommon: i32,
    wildcards_rare: i32,
    wildcards_mythic: i32,
    gold: i32,
    gems: i32,
    vault_progress: f64,
    unique_cards: i32,
    total_cards: i64,
}

async fn get_inventory() -> Result<Json<InventoryResponse>, StatusCode> {
    let result = with_state(|state| -> Result<InventoryResponse, StatusCode> {
        if state.papa_instance == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        // Navigate to AwsInventoryServiceWrapper
        let inv_mgr_ptr = state.reader.read_ptr(state.papa_instance + 224);
        let aws_wrapper_ptr = state.reader.read_ptr(inv_mgr_ptr + 56);

        if aws_wrapper_ptr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        // Read m_inventory (ClientPlayerInventory) at +64
        let m_inventory = state.reader.read_ptr(aws_wrapper_ptr + 64);

        let wc_common = state.reader.read_i32(m_inventory + 16);
        let wc_uncommon = state.reader.read_i32(m_inventory + 20);
        let wc_rare = state.reader.read_i32(m_inventory + 24);
        let wc_mythic = state.reader.read_i32(m_inventory + 28);
        let gold = state.reader.read_i32(m_inventory + 32);
        let gems = state.reader.read_i32(m_inventory + 36);
        let vault_progress = state.reader.read_i32(m_inventory + 48) as f64 / 10.0;

        // Read cards count
        let cards_ptr = state.reader.read_ptr(aws_wrapper_ptr + 72);
        let (unique_cards, total_cards) = if cards_ptr > 0x100000 {
            let count = state.reader.read_i32(cards_ptr + 0x20);
            let entries_ptr = state.reader.read_ptr(cards_ptr + 0x18);

            let mut total = 0_i64;
            if entries_ptr > 0x100000 {
                for i in 0..count as usize {
                    let entry = entries_ptr + 0x20 + i * 16;
                    let hash = state.reader.read_i32(entry);
                    let quantity = state.reader.read_i32(entry + 12);
                    if hash >= 0 {
                        total += quantity as i64;
                    }
                }
            }
            (count, total)
        } else {
            (0, 0)
        };

        Ok(InventoryResponse {
            wildcards_common: wc_common,
            wildcards_uncommon: wc_uncommon,
            wildcards_rare: wc_rare,
            wildcards_mythic: wc_mythic,
            gold,
            gems,
            vault_progress,
            unique_cards,
            total_cards,
        })
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
    for i in 0..50000 {
        let class_ptr = reader.read_ptr(type_info_table + i * 8);
        if class_ptr == 0 {
            continue;
        }
        let name_ptr = reader.read_ptr(class_ptr + offsets::CLASS_NAME);
        if name_ptr == 0 {
            continue;
        }
        if reader.read_string(name_ptr) == name {
            return Some(class_ptr);
        }
    }
    None
}

fn read_class_name(reader: &MemReader, class: usize) -> String {
    if class == 0 || class < 0x100000 {
        return String::new();
    }
    let name_ptr = reader.read_ptr(class + offsets::CLASS_NAME);
    reader.read_string(name_ptr)
}

fn read_class_namespace(reader: &MemReader, class: usize) -> String {
    if class == 0 || class < 0x100000 {
        return String::new();
    }
    let ns_ptr = reader.read_ptr(class + offsets::CLASS_NAMESPACE);
    reader.read_string(ns_ptr)
}

fn get_class_fields(reader: &MemReader, class_addr: usize) -> Vec<FieldInfo> {
    let mut fields = Vec::new();
    let fields_ptr = reader.read_ptr(class_addr + offsets::CLASS_FIELDS);

    if fields_ptr == 0 || fields_ptr < 0x100000 {
        return fields;
    }

    for i in 0..50 {
        let field = fields_ptr + i * offsets::FIELD_INFO_SIZE;
        let name_ptr = reader.read_ptr(field);
        if name_ptr == 0 || name_ptr < 0x100000 {
            break;
        }

        let name = reader.read_string(name_ptr);
        let offset = reader.read_i32(field + offsets::FIELD_OFFSET);

        let type_ptr = reader.read_ptr(field + offsets::FIELD_TYPE);
        let type_attrs = reader.read_u32(type_ptr + offsets::TYPE_ATTRS);
        let is_static = (type_attrs & 0x10) != 0;

        // Try to get type name from type data
        let type_data = reader.read_ptr(type_ptr);
        let type_name = if type_data > 0x100000 {
            let tn = read_class_name(reader, type_data);
            if tn.is_empty() { "Unknown".to_string() } else { tn }
        } else {
            // Primitive type - guess from size/offset patterns
            "System.Object".to_string()
        };

        fields.push(FieldInfo {
            name,
            type_name,
            offset,
            is_static,
            is_const: false,
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
    let entries_ptr = reader.read_ptr(cards_addr + 0x18);
    let count = reader.read_i32(cards_addr + 0x20);

    if entries_ptr == 0 || count <= 0 || count > 100000 {
        return Err(StatusCode::NOT_FOUND);
    }

    read_dict_entries(reader, entries_ptr, count)
}

fn read_dict_entries(reader: &MemReader, entries_ptr: usize, count: i32) -> Result<DictionaryResponse, StatusCode> {
    let mut entries = Vec::new();
    let max_read = count.min(5000) as usize;  // Limit for performance

    for i in 0..max_read {
        let entry_addr = entries_ptr + 0x20 + i * 16;
        let hash = reader.read_i32(entry_addr);
        let key = reader.read_i32(entry_addr + 8);
        let value = reader.read_i32(entry_addr + 12);

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

fn find_papa_instance(reader: &MemReader, papa_class: usize) -> Option<usize> {
    // Scan heap regions for PAPA instance
    let heap_regions = [
        (0x15a000000_usize, 0x15b000000_usize),
        (0x158000000_usize, 0x16a000000_usize),
        (0x145000000_usize, 0x150000000_usize),
    ];

    for (start, end) in heap_regions {
        let step = 0x100000;
        for chunk_start in (start..end).step_by(step) {
            let bytes = reader.read_bytes(chunk_start, step);
            if bytes.is_empty() || bytes.iter().all(|&b| b == 0) {
                continue;
            }

            for i in (0..bytes.len() - 8).step_by(8) {
                let ptr = usize::from_le_bytes(bytes[i..i + 8].try_into().unwrap_or([0; 8]));
                if ptr == papa_class {
                    let obj_addr = chunk_start + i;
                    // Verify not a FieldInfo entry (check +16)
                    let val_at_16 = reader.read_ptr(obj_addr + 16);
                    if val_at_16 != papa_class && val_at_16 > 0x100000 {
                        // Verify it has an InventoryManager field at +224
                        let inv_mgr = reader.read_ptr(obj_addr + 224);
                        if inv_mgr > 0x100000 && inv_mgr < 0x400000000 {
                            let inv_class = reader.read_ptr(inv_mgr);
                            let inv_name = read_class_name(reader, inv_class);
                            if inv_name.contains("InventoryManager") {
                                return Some(obj_addr);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_second_data_segment(pid: u32) -> usize {
    let output = Command::new("vmmap")
        .args(["-wide", &pid.to_string()])
        .output()
        .expect("vmmap failed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut found_first = false;

    for line in stdout.lines() {
        if line.contains("GameAssembly") && line.contains("__DATA") && !line.contains("__DATA_CONST") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let addr_parts: Vec<&str> = parts[1].split('-').collect();
                if let Ok(start) = usize::from_str_radix(addr_parts[0], 16) {
                    if found_first {
                        return start;
                    }
                    found_first = true;
                }
            }
        }
    }
    0
}

// Memory reader (same as in examples)
struct MemReader {
    task_port: u32,
}

impl MemReader {
    fn new(pid: u32) -> Self {
        let task_port = unsafe {
            let mut task: u32 = 0;
            mach2::traps::task_for_pid(mach2::traps::mach_task_self(), pid as i32, &mut task);
            task
        };
        MemReader { task_port }
    }

    fn read_bytes(&self, addr: usize, size: usize) -> Vec<u8> {
        let mut buffer = vec![0u8; size];
        let mut out_size: u64 = 0;
        unsafe {
            mach2::vm::mach_vm_read_overwrite(
                self.task_port,
                addr as u64,
                size as u64,
                buffer.as_mut_ptr() as u64,
                &mut out_size,
            );
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

    let type_info_table = reader.read_ptr(data_base + 0x24360);
    println!("Type info table: 0x{:x}", type_info_table);

    // Find PAPA class
    let papa_class = find_class_by_name(&reader, type_info_table, "PAPA")
        .expect("PAPA class not found");
    println!("PAPA class: 0x{:x}", papa_class);

    // Find PAPA instance
    let papa_instance = find_papa_instance(&reader, papa_class).unwrap_or(0);
    if papa_instance == 0 {
        eprintln!("Warning: PAPA instance not found. Some features may not work.");
    } else {
        println!("PAPA instance: 0x{:x}", papa_instance);
    }

    // Initialize state
    {
        let mut state = IL2CPP_STATE.lock().unwrap();
        *state = Some(Il2CppState {
            reader,
            type_info_table,
            class_cache: HashMap::new(),
            papa_instance,
        });
    }

    println!("IL2CPP backend initialized");

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        // Standard API (compatible with existing UI)
        .route("/assemblies", get(get_assemblies))
        .route("/assembly/:name/classes", get(get_assembly_classes))
        .route("/assembly/:assembly/class/:class", get(get_class_details))
        .route("/instance/:address", get(get_instance))
        .route("/instance/:address/field/:field_name", get(read_instance_field))
        .route("/class/:address/field/:field_name", get(read_static_field))
        .route("/dictionary/:address", get(read_dictionary))
        // IL2CPP-specific endpoints
        .route("/cards", get(get_cards))
        .route("/inventory", get(get_inventory))
        .layer(cors);

    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("Failed to bind to port 8080");

    println!("\nServer listening on http://127.0.0.1:8080");
    println!("\nEndpoints:");
    println!("  GET /assemblies              - List assemblies (faked for IL2CPP)");
    println!("  GET /assembly/:name/classes  - List classes");
    println!("  GET /instance/:address       - Read instance at address");
    println!("  GET /dictionary/:address     - Read dictionary entries");
    println!("  GET /cards                   - Get card collection (IL2CPP specific)");
    println!("  GET /inventory               - Get player inventory (IL2CPP specific)");
    println!("\nPress Ctrl+C to stop");

    axum::serve(listener, app).await.expect("Server error");
}
