use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use mtga_reader::{
    field_definition::FieldDefinition,
    mono_reader::MonoReader,
    type_code::TypeCode,
    type_definition::TypeDefinition,
};

// Wrapper to make MonoReader Send + Sync
// This is safe because Node.js addon calls are single-threaded from the JS main thread
struct ReaderWrapper(Option<MonoReader>);
unsafe impl Send for ReaderWrapper {}
unsafe impl Sync for ReaderWrapper {}

// Global reader state - protected by mutex
static READER: Mutex<ReaderWrapper> = Mutex::new(ReaderWrapper(None));

// ============================================================================
// Response types matching the HTTP server
// ============================================================================

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct ClassInfo {
    pub name: String,
    pub namespace: String,
    pub address: i64,
    pub is_static: bool,
    pub is_enum: bool,
}

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct FieldInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub offset: i32,
    pub is_static: bool,
    pub is_const: bool,
}

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct StaticInstanceInfo {
    pub field_name: String,
    pub address: i64,
}

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct ClassDetails {
    pub name: String,
    pub namespace: String,
    pub address: i64,
    pub fields: Vec<FieldInfo>,
    pub static_instances: Vec<StaticInstanceInfo>,
}

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct InstanceField {
    pub name: String,
    pub type_name: String,
    pub is_static: bool,
    pub value: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct InstanceData {
    pub class_name: String,
    pub namespace: String,
    pub address: i64,
    pub fields: Vec<InstanceField>,
}

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct DictionaryEntry {
    pub key: serde_json::Value,
    pub value: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
#[napi(object)]
pub struct DictionaryData {
    pub count: i32,
    pub entries: Vec<DictionaryEntry>,
}

// ============================================================================
// Utility functions
// ============================================================================

/// Check if the current process has administrator privileges
#[napi]
pub fn is_admin() -> bool {
    MonoReader::is_admin()
}

/// Find a process by name and return true if found
#[napi]
pub fn find_process(process_name: String) -> bool {
    MonoReader::find_pid_by_name(&process_name).is_some()
}

/// Initialize connection to the target process
/// Must be called before using any other reader functions
#[napi]
pub fn init(process_name: String) -> Result<bool> {
    let pid = MonoReader::find_pid_by_name(&process_name)
        .ok_or_else(|| Error::from_reason("Process not found"))?;

    let mut mono_reader = MonoReader::new(pid.as_u32());
    let mono_root = mono_reader.read_mono_root_domain();

    if mono_root == 0 {
        return Err(Error::from_reason("Failed to read mono root domain"));
    }

    let mut wrapper = READER.lock().map_err(|_| Error::from_reason("Failed to lock reader"))?;
    wrapper.0 = Some(mono_reader);

    Ok(true)
}

/// Close the connection to the target process
#[napi]
pub fn close() -> Result<bool> {
    let mut wrapper = READER.lock().map_err(|_| Error::from_reason("Failed to lock reader"))?;
    wrapper.0 = None;
    Ok(true)
}

/// Check if the reader is initialized
#[napi]
pub fn is_initialized() -> bool {
    if let Ok(wrapper) = READER.lock() {
        wrapper.0.is_some()
    } else {
        false
    }
}

// Helper to run operations with the reader
fn with_reader<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&mut MonoReader) -> Result<T>,
{
    let mut wrapper = READER.lock().map_err(|_| Error::from_reason("Failed to lock reader"))?;
    let reader = wrapper.0
        .as_mut()
        .ok_or_else(|| Error::from_reason("Reader not initialized. Call init() first."))?;
    f(reader)
}

// ============================================================================
// Assembly functions
// ============================================================================

/// Get all loaded assembly names
#[napi]
pub fn get_assemblies() -> Result<Vec<String>> {
    with_reader(|reader| Ok(reader.get_all_assembly_names()))
}

/// Get all classes in an assembly
#[napi]
pub fn get_assembly_classes(assembly_name: String) -> Result<Vec<ClassInfo>> {
    with_reader(|reader| {
        let image_addr = reader.read_assembly_image_by_name(&assembly_name);
        if image_addr == 0 {
            return Err(Error::from_reason("Assembly not found"));
        }

        let type_defs = reader.create_type_definitions_for_image(image_addr);
        let mut classes = Vec::new();

        for def_addr in type_defs {
            let typedef = TypeDefinition::new(def_addr, reader);
            classes.push(ClassInfo {
                name: typedef.name.clone(),
                namespace: typedef.namespace_name.clone(),
                address: def_addr as i64,
                is_static: false,
                is_enum: typedef.is_enum,
            });
        }

        Ok(classes)
    })
}

/// Get detailed information about a class
#[napi]
pub fn get_class_details(assembly_name: String, class_name: String) -> Result<ClassDetails> {
    with_reader(|reader| {
        let image_addr = reader.read_assembly_image_by_name(&assembly_name);
        if image_addr == 0 {
            return Err(Error::from_reason("Assembly not found"));
        }

        let type_defs = reader.create_type_definitions_for_image(image_addr);
        let class_addr = type_defs
            .iter()
            .find(|&&def_addr| {
                let typedef = TypeDefinition::new(def_addr, reader);
                typedef.name == class_name
            })
            .ok_or_else(|| Error::from_reason("Class not found"))?;

        let typedef = TypeDefinition::new(*class_addr, reader);
        let field_addrs = typedef.get_fields();

        // Get fields
        let mut fields = Vec::new();
        for field_addr in &field_addrs {
            let field = FieldDefinition::new(*field_addr, reader);
            let type_name = get_type_name(&field, reader);
            fields.push(FieldInfo {
                name: field.name.clone(),
                type_name,
                offset: field.offset,
                is_static: field.type_info.is_static,
                is_const: field.type_info.is_const,
            });
        }

        // Get static instances
        let mut static_instances = Vec::new();
        for field_addr in &field_addrs {
            let field = FieldDefinition::new(*field_addr, reader);
            if (field.name.contains("instance") || field.name.contains("Instance"))
                && field.type_info.is_static
            {
                let (static_field_addr, _) = typedef.get_static_value(&field.name);
                let instance_ptr = if static_field_addr != 0 {
                    reader.read_ptr(static_field_addr)
                } else {
                    0
                };

                static_instances.push(StaticInstanceInfo {
                    field_name: field.name.clone(),
                    address: instance_ptr as i64,
                });
            }
        }

        Ok(ClassDetails {
            name: typedef.name.clone(),
            namespace: typedef.namespace_name.clone(),
            address: *class_addr as i64,
            fields,
            static_instances,
        })
    })
}

// ============================================================================
// Instance reading functions
// ============================================================================

/// Read an instance at a given memory address
#[napi]
pub fn get_instance(address: i64) -> Result<InstanceData> {
    with_reader(|reader| {
        let address = address as usize;
        if address == 0 {
            return Err(Error::from_reason("Invalid address"));
        }

        let vtable_ptr = reader.read_ptr(address);
        let class_ptr = reader.read_ptr(vtable_ptr);
        let typedef = TypeDefinition::new(class_ptr, reader);

        let field_addrs = typedef.get_fields();
        let mut fields = Vec::new();

        for field_addr in &field_addrs {
            let field = FieldDefinition::new(*field_addr, reader);
            let type_name = get_instance_type_name(&field, reader);
            let value = read_field_value(reader, address, &field, &type_name);

            fields.push(InstanceField {
                name: field.name.clone(),
                type_name,
                is_static: false,
                value,
            });
        }

        Ok(InstanceData {
            class_name: typedef.name.clone(),
            namespace: typedef.namespace_name.clone(),
            address: address as i64,
            fields,
        })
    })
}

/// Read a specific field from an instance
#[napi]
pub fn get_instance_field(address: i64, field_name: String) -> Result<serde_json::Value> {
    with_reader(|reader| {
        let instance_addr = address as usize;
        if instance_addr == 0 {
            return Err(Error::from_reason("Invalid address"));
        }

        let vtable_ptr = reader.read_ptr(instance_addr);
        if vtable_ptr == 0 {
            return Err(Error::from_reason("Invalid instance"));
        }

        let class_ptr = reader.read_ptr(vtable_ptr);
        let typedef = TypeDefinition::new(class_ptr, reader);

        let field_addrs = typedef.get_fields();
        let field_addr = field_addrs
            .iter()
            .find(|&&addr| {
                let field = FieldDefinition::new(addr, reader);
                field.name == field_name
            })
            .ok_or_else(|| Error::from_reason("Field not found"))?;

        let field = FieldDefinition::new(*field_addr, reader);
        let field_location = instance_addr + field.offset as usize;
        let type_name = get_type_name(&field, reader);

        Ok(read_typed_value(reader, field_location, &type_name, &field))
    })
}

/// Read a static field from a class
#[napi]
pub fn get_static_field(class_address: i64, field_name: String) -> Result<serde_json::Value> {
    with_reader(|reader| {
        let class_addr = class_address as usize;
        let typedef = TypeDefinition::new(class_addr, reader);

        let field_addrs = typedef.get_fields();
        let field_addr = field_addrs
            .iter()
            .find(|&&addr| {
                let field = FieldDefinition::new(addr, reader);
                field.name == field_name
            })
            .ok_or_else(|| Error::from_reason("Field not found"))?;

        let field = FieldDefinition::new(*field_addr, reader);
        if !field.type_info.is_static {
            return Err(Error::from_reason("Field is not static"));
        }

        let (field_location, _) = typedef.get_static_value(&field.name);
        if field_location == 0 {
            return Ok(serde_json::Value::Null);
        }

        let type_name = get_type_name(&field, reader);
        Ok(read_typed_value(reader, field_location, &type_name, &field))
    })
}

// ============================================================================
// Dictionary reading
// ============================================================================

/// Read a dictionary at a given memory address
#[napi]
pub fn get_dictionary(address: i64) -> Result<DictionaryData> {
    with_reader(|reader| {
        let dict_addr = address as usize;
        if dict_addr == 0 {
            return Err(Error::from_reason("Invalid address"));
        }

        // Try standard Dictionary offsets first (_entries at 0x18)
        let entries_ptr_0x18 = reader.read_ptr(dict_addr + 0x18);
        if entries_ptr_0x18 > 0x10000 {
            let array_length = reader.read_i32(entries_ptr_0x18 + 0x18);
            if array_length > 0 && array_length < 100000 {
                return read_dict_entries(reader, entries_ptr_0x18, array_length);
            }
        }

        // Try alternative offsets (_entries at 0x10)
        let entries_ptr_0x10 = reader.read_ptr(dict_addr + 0x10);
        if entries_ptr_0x10 > 0x10000 {
            let array_length = reader.read_i32(entries_ptr_0x10 + 0x18);
            if array_length > 0 && array_length < 100000 {
                return read_dict_entries(reader, entries_ptr_0x10, array_length);
            }
        }

        Err(Error::from_reason("Could not read dictionary entries"))
    })
}

fn read_dict_entries(reader: &MonoReader, entries_ptr: usize, count: i32) -> Result<DictionaryData> {
    let entry_size = 16usize;
    let entries_start = entries_ptr + mtga_reader::constants::SIZE_OF_PTR * 4;

    let mut entries = Vec::new();
    let max_read = std::cmp::min(count, 1000);

    for i in 0..max_read {
        let entry_addr = entries_start + (i as usize * entry_size);

        let hash_code = reader.read_i32(entry_addr);
        let key = reader.read_u32(entry_addr + 8);
        let value = reader.read_i32(entry_addr + 12);

        if hash_code >= 0 && key > 0 {
            entries.push(DictionaryEntry {
                key: serde_json::json!(key),
                value: serde_json::json!(value),
            });
        }
    }

    Ok(DictionaryData {
        count: entries.len() as i32,
        entries,
    })
}

// ============================================================================
// High-level data reading (from original lib.rs)
// ============================================================================

/// Read nested data by traversing a path of field names
/// The first element is the root class name, subsequent elements are field names
#[napi]
pub fn read_data(process_name: String, fields: Vec<String>) -> serde_json::Value {
    mtga_reader::read_data(process_name, fields)
}

/// Read a managed class at a given address
#[napi]
pub fn read_class(process_name: String, address: i64) -> serde_json::Value {
    mtga_reader::read_class(process_name, address)
}

/// Read a generic instance at a given address
#[napi]
pub fn read_generic_instance(process_name: String, address: i64) -> serde_json::Value {
    mtga_reader::read_generic_instance(process_name, address)
}

// ============================================================================
// Helper functions
// ============================================================================

fn get_type_name(field: &FieldDefinition, reader: &MonoReader) -> String {
    match field.type_info.clone().code() {
        TypeCode::CLASS | TypeCode::VALUETYPE => {
            let typedef = TypeDefinition::new(field.type_info.data, reader);
            format!("{}.{}", typedef.namespace_name, typedef.name)
        }
        TypeCode::I4 => "System.Int32".to_string(),
        TypeCode::U4 => "System.UInt32".to_string(),
        TypeCode::I8 => "System.Int64".to_string(),
        TypeCode::U8 => "System.UInt64".to_string(),
        TypeCode::BOOLEAN => "System.Boolean".to_string(),
        TypeCode::STRING => "System.String".to_string(),
        _ => format!("TypeCode({})", field.type_info.type_code),
    }
}

fn get_instance_type_name(field: &FieldDefinition, reader: &MonoReader) -> String {
    match field.type_info.clone().code() {
        TypeCode::CLASS | TypeCode::VALUETYPE | TypeCode::GENERICINST => {
            let typedef = TypeDefinition::new(field.type_info.data, reader);
            if typedef.namespace_name.is_empty() {
                typedef.name.clone()
            } else {
                format!("{}.{}", typedef.namespace_name, typedef.name)
            }
        }
        TypeCode::SZARRAY => "Array (SZARRAY)".to_string(),
        TypeCode::ARRAY => "Array (multi-dim)".to_string(),
        TypeCode::I4 => "System.Int32".to_string(),
        TypeCode::U4 => "System.UInt32".to_string(),
        TypeCode::I8 => "System.Int64".to_string(),
        TypeCode::U8 => "System.UInt64".to_string(),
        TypeCode::BOOLEAN => "System.Boolean".to_string(),
        TypeCode::STRING => "System.String".to_string(),
        TypeCode::OBJECT => "System.Object".to_string(),
        TypeCode::PTR => "Pointer".to_string(),
        _ => format!("TypeCode({})", field.type_info.type_code),
    }
}

fn read_field_value(
    reader: &MonoReader,
    base_addr: usize,
    field: &FieldDefinition,
    type_name: &str,
) -> serde_json::Value {
    let addr = base_addr + field.offset as usize;

    match type_name {
        "System.Int32" | "int" => {
            let val = reader.read_i32(addr);
            serde_json::json!(val)
        }
        "System.Int64" | "long" => {
            let val = reader.read_i64(addr);
            serde_json::json!(val)
        }
        "System.UInt32" | "uint" => {
            let val = reader.read_u32(addr);
            serde_json::json!(val)
        }
        "System.Boolean" | "bool" | "BOOLEAN" => {
            let val = reader.read_u8(addr);
            serde_json::json!(val != 0)
        }
        "System.String" | "string" => {
            let str_ptr = reader.read_ptr(addr);
            if str_ptr == 0 {
                serde_json::Value::Null
            } else {
                match reader.read_mono_string(str_ptr) {
                    Some(s) => serde_json::json!(s),
                    None => serde_json::Value::Null,
                }
            }
        }
        _ => {
            let ptr = reader.read_ptr(addr);
            if ptr == 0 {
                serde_json::Value::Null
            } else {
                serde_json::json!({
                    "type": "pointer",
                    "address": ptr,
                    "class_name": type_name
                })
            }
        }
    }
}

fn read_typed_value(
    reader: &MonoReader,
    field_location: usize,
    type_name: &str,
    field: &FieldDefinition,
) -> serde_json::Value {
    match type_name {
        "System.Int32" => {
            let val = reader.read_i32(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "int32",
                "value": val
            })
        }
        "System.UInt32" => {
            let val = reader.read_u32(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint32",
                "value": val
            })
        }
        "System.Int64" => {
            let val = reader.read_i64(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "int64",
                "value": val
            })
        }
        "System.UInt64" => {
            let val = reader.read_u64(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint64",
                "value": val.to_string()
            })
        }
        "System.Boolean" => {
            let val = reader.read_u8(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "boolean",
                "value": val != 0
            })
        }
        _ => {
            let ptr = reader.read_ptr(field_location);
            if ptr == 0 {
                serde_json::json!({
                    "type": "null",
                    "address": 0
                })
            } else {
                serde_json::json!({
                    "type": "pointer",
                    "address": ptr,
                    "field_name": field.name,
                    "class_name": type_name
                })
            }
        }
    }
}
