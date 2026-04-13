//! NAPI bindings for Node.js
//!
//! This module provides cross-platform Node.js bindings for reading MTGA memory.
//! - Windows: Uses Mono backend
//! - macOS: Uses IL2CPP backend

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[cfg(target_os = "windows")]
use sysinfo::{Pid, System};

// ============================================================================
// Response types matching the HTTP server (cross-platform)
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
// Windows Backend (Mono)
// ============================================================================

#[cfg(target_os = "windows")]
mod windows_backend {
    use super::*;
    use crate::{
        field_definition::FieldDefinition,
        mono_reader::MonoReader,
        type_code::TypeCode,
        type_definition::TypeDefinition,
    };

    pub struct ReaderWrapper(pub Option<MonoReader>);
    unsafe impl Send for ReaderWrapper {}
    unsafe impl Sync for ReaderWrapper {}

    pub static READER: Mutex<ReaderWrapper> = Mutex::new(ReaderWrapper(None));

    pub fn is_admin_impl() -> bool {
        MonoReader::is_admin()
    }

    pub fn find_process_impl(process_name: &str) -> bool {
        MonoReader::find_pid_by_name(process_name).is_some()
    }

    pub fn init_impl(process_name: &str) -> Result<bool> {
        let pid = MonoReader::find_pid_by_name(process_name)
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

    pub fn close_impl() -> Result<bool> {
        let mut wrapper = READER.lock().map_err(|_| Error::from_reason("Failed to lock reader"))?;
        wrapper.0 = None;
        Ok(true)
    }

    pub fn is_initialized_impl() -> bool {
        if let Ok(wrapper) = READER.lock() {
            wrapper.0.is_some()
        } else {
            false
        }
    }

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

    pub fn get_assemblies_impl() -> Result<Vec<String>> {
        with_reader(|reader| Ok(reader.get_all_assembly_names()))
    }

    pub fn get_assembly_classes_impl(assembly_name: &str) -> Result<Vec<ClassInfo>> {
        with_reader(|reader| {
            let image_addr = reader.read_assembly_image_by_name(assembly_name);
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

    pub fn get_class_details_impl(assembly_name: &str, class_name: &str) -> Result<ClassDetails> {
        with_reader(|reader| {
            let image_addr = reader.read_assembly_image_by_name(assembly_name);
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

    pub fn get_instance_impl(address: i64) -> Result<InstanceData> {
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

                // Skip static fields - their values are not stored in the instance
                if field.type_info.is_static {
                    continue;
                }

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

    pub fn get_instance_field_impl(address: i64, field_name: &str) -> Result<serde_json::Value> {
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

    pub fn get_static_field_impl(class_address: i64, field_name: &str) -> Result<serde_json::Value> {
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

    pub fn get_dictionary_impl(address: i64) -> Result<DictionaryData> {
        with_reader(|reader| {
            let dict_addr = address as usize;
            if dict_addr == 0 {
                return Err(Error::from_reason("Invalid address"));
            }

            let entries_ptr_0x18 = reader.read_ptr(dict_addr + 0x18);
            if entries_ptr_0x18 > 0x10000 {
                let array_length = reader.read_i32(entries_ptr_0x18 + 0x18);
                if array_length > 0 && array_length < 100000 {
                    return read_dict_entries(reader, entries_ptr_0x18, array_length);
                }
            }

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
        let entries_start = entries_ptr + crate::constants::SIZE_OF_PTR * 4;

        let mut entries = Vec::new();
        let max_read = std::cmp::min(count, 5000);

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

        // Use contains() for more robust type matching
        // Check UInt32 before Int32 since "UInt32" contains "Int32"
        if type_name.contains("UInt32") || type_name == "uint" {
            serde_json::json!(reader.read_u32(addr))
        } else if type_name.contains("Int32") || type_name == "int" {
            serde_json::json!(reader.read_i32(addr))
        } else if type_name.contains("UInt64") || type_name == "ulong" {
            serde_json::json!(reader.read_u64(addr))
        } else if type_name.contains("Int64") || type_name == "long" {
            serde_json::json!(reader.read_i64(addr))
        } else if type_name.contains("UInt16") || type_name == "ushort" {
            serde_json::json!(reader.read_u16(addr))
        } else if type_name.contains("Int16") || type_name == "short" {
            serde_json::json!(reader.read_i16(addr))
        } else if type_name.contains("Byte") && !type_name.contains("SByte") || type_name == "byte" {
            serde_json::json!(reader.read_u8(addr))
        } else if type_name.contains("SByte") || type_name == "sbyte" {
            serde_json::json!(reader.read_i8(addr))
        } else if type_name.contains("Single") || type_name == "float" {
            serde_json::json!(reader.read_f32(addr))
        } else if type_name.contains("Double") || type_name == "double" {
            serde_json::json!(reader.read_f64(addr))
        } else if type_name.contains("Boolean") || type_name == "bool" {
            serde_json::json!(reader.read_u8(addr) != 0)
        } else if type_name.contains("String") || type_name == "string" {
            let str_ptr = reader.read_ptr(addr);
            if str_ptr == 0 {
                serde_json::Value::Null
            } else {
                match reader.read_mono_string(str_ptr) {
                    Some(s) => serde_json::json!(s),
                    None => serde_json::Value::Null,
                }
            }
        } else {
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

    fn read_typed_value(
        reader: &MonoReader,
        field_location: usize,
        type_name: &str,
        field: &FieldDefinition,
    ) -> serde_json::Value {
        // Use contains() for more robust type matching
        // Check UInt32 before Int32 since "UInt32" contains "Int32"
        if type_name.contains("UInt32") || type_name == "uint" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint32",
                "value": reader.read_u32(field_location)
            })
        } else if type_name.contains("Int32") || type_name == "int" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "int32",
                "value": reader.read_i32(field_location)
            })
        } else if type_name.contains("UInt64") || type_name == "ulong" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint64",
                "value": reader.read_u64(field_location).to_string()
            })
        } else if type_name.contains("Int64") || type_name == "long" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "int64",
                "value": reader.read_i64(field_location)
            })
        } else if type_name.contains("UInt16") || type_name == "ushort" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint16",
                "value": reader.read_u16(field_location)
            })
        } else if type_name.contains("Int16") || type_name == "short" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "int16",
                "value": reader.read_i16(field_location)
            })
        } else if type_name.contains("Byte") && !type_name.contains("SByte") || type_name == "byte" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "byte",
                "value": reader.read_u8(field_location)
            })
        } else if type_name.contains("SByte") || type_name == "sbyte" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "sbyte",
                "value": reader.read_i8(field_location)
            })
        } else if type_name.contains("Single") || type_name == "float" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "float",
                "value": reader.read_f32(field_location)
            })
        } else if type_name.contains("Double") || type_name == "double" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "double",
                "value": reader.read_f64(field_location)
            })
        } else if type_name.contains("Boolean") || type_name == "bool" {
            serde_json::json!({
                "type": "primitive",
                "value_type": "boolean",
                "value": reader.read_u8(field_location) != 0
            })
        } else {
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

    pub fn read_data_impl(process_name: &str, fields: Vec<String>) -> serde_json::Value {
        crate::read_data(process_name.to_string(), fields)
    }

    pub fn read_class_impl(process_name: &str, address: i64) -> serde_json::Value {
        crate::read_class(process_name.to_string(), address)
    }

    pub fn read_generic_instance_impl(process_name: &str, address: i64) -> serde_json::Value {
        crate::read_generic_instance(process_name.to_string(), address)
    }
}

// ============================================================================
// macOS Backend (IL2CPP)
// ============================================================================

#[cfg(target_os = "macos")]
mod macos_backend {
    use super::*;
    use std::process::Command;

    mod offsets {
        pub const CLASS_NAME: usize = 0x10;
        pub const CLASS_NAMESPACE: usize = 0x18;
        pub const CLASS_FIELDS: usize = 0x80;
        pub const CLASS_STATIC_FIELDS: usize = 0xA8;
        pub const FIELD_INFO_SIZE: usize = 32;
        pub const FIELD_TYPE: usize = 0x08;
        pub const FIELD_OFFSET: usize = 0x18;
        pub const TYPE_ATTRS: usize = 0x08;
        pub const TYPE_INFO_TABLE_OFFSET: usize = 0x24360;
    }

    pub struct MemReader {
        task_port: u32,
    }

    impl MemReader {
        pub fn new(pid: u32) -> Self {
            let task_port = unsafe {
                let mut task: u32 = 0;
                mach2::traps::task_for_pid(mach2::traps::mach_task_self(), pid as i32, &mut task);
                task
            };
            MemReader { task_port }
        }

        pub fn read_bytes(&self, addr: usize, size: usize) -> Vec<u8> {
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

        pub fn read_ptr(&self, addr: usize) -> usize {
            let bytes = self.read_bytes(addr, 8);
            usize::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
        }

        pub fn read_i32(&self, addr: usize) -> i32 {
            let bytes = self.read_bytes(addr, 4);
            i32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
        }

        pub fn read_u32(&self, addr: usize) -> u32 {
            let bytes = self.read_bytes(addr, 4);
            u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
        }

        pub fn read_u8(&self, addr: usize) -> u8 {
            let bytes = self.read_bytes(addr, 1);
            bytes.first().copied().unwrap_or(0)
        }

        pub fn read_i8(&self, addr: usize) -> i8 {
            let bytes = self.read_bytes(addr, 1);
            i8::from_le_bytes(bytes.try_into().unwrap_or([0; 1]))
        }

        pub fn read_u16(&self, addr: usize) -> u16 {
            let bytes = self.read_bytes(addr, 2);
            u16::from_le_bytes(bytes.try_into().unwrap_or([0; 2]))
        }

        pub fn read_i16(&self, addr: usize) -> i16 {
            let bytes = self.read_bytes(addr, 2);
            i16::from_le_bytes(bytes.try_into().unwrap_or([0; 2]))
        }

        pub fn read_u64(&self, addr: usize) -> u64 {
            let bytes = self.read_bytes(addr, 8);
            u64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
        }

        pub fn read_i64(&self, addr: usize) -> i64 {
            let bytes = self.read_bytes(addr, 8);
            i64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
        }

        pub fn read_f32(&self, addr: usize) -> f32 {
            let bytes = self.read_bytes(addr, 4);
            f32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
        }

        pub fn read_f64(&self, addr: usize) -> f64 {
            let bytes = self.read_bytes(addr, 8);
            f64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
        }

        pub fn read_string(&self, addr: usize) -> String {
            if addr == 0 {
                return String::new();
            }
            let bytes = self.read_bytes(addr, 256);
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..end]).to_string()
        }
    }

    pub struct Il2CppState {
        pub reader: MemReader,
        pub pid: u32,
        pub type_info_table: usize,
        pub papa_class: usize,
        pub papa_instance: usize,
    }

    pub struct StateWrapper(pub Option<Il2CppState>);
    unsafe impl Send for StateWrapper {}
    unsafe impl Sync for StateWrapper {}

    pub static STATE: Mutex<StateWrapper> = Mutex::new(StateWrapper(None));

    fn find_second_data_segment(pid: u32) -> usize {
        // Historical name kept for compatibility. Upstream hardcoded
        // the "second __DATA segment of GameAssembly" pattern with
        // the assumption that a fixed offset inside that segment
        // held the IL2CPP type info table. Both assumptions drifted:
        // the real table lives in the first __DATA segment on current
        // MTGA builds, and even there the offset `0x24360` is wrong.
        // Returning any segment start here is now just a sentinel so
        // init_impl knows whether vmmap parsing succeeded at all;
        // init_impl does its own scan via `scan_for_type_info_table`.
        find_all_data_segments(pid)
            .into_iter()
            .next()
            .map(|(s, _e)| s)
            .unwrap_or(0)
    }

    fn find_all_data_segments(pid: u32) -> Vec<(usize, usize)> {
        let output = Command::new("vmmap")
            .args(["-wide", &pid.to_string()])
            .output()
            .ok();

        let mut result: Vec<(usize, usize)> = Vec::new();
        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("GameAssembly")
                    && line.contains("__DATA")
                    && !line.contains("__DATA_CONST")
                {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let addr_parts: Vec<&str> = parts[1].split('-').collect();
                        if addr_parts.len() >= 2 {
                            if let (Ok(start), Ok(end)) = (
                                usize::from_str_radix(addr_parts[0], 16),
                                usize::from_str_radix(addr_parts[1], 16),
                            ) {
                                result.push((start, end));
                            }
                        }
                    }
                }
            }
        }
        // vmmap output ordering isn't guaranteed, so sort so callers
        // can reliably walk segments low-to-high.
        result.sort();
        result
    }

    /// Scan GameAssembly's __DATA segments for an IL2CPP type info
    /// table. Hardcoded offsets in upstream rot every time Unity
    /// reshuffles its metadata layout, so we find the table
    /// heuristically each time init runs.
    ///
    /// Strategy:
    ///  1. For each __DATA segment, read the whole segment in ONE
    ///     `mach_vm_read_overwrite` call into a local buffer (avoids
    ///     doing 720k syscalls one slot at a time).
    ///  2. Phase 1 (in-memory): walk the buffer 8 bytes at a time
    ///     looking for positions where the next 20 aligned slots are
    ///     all either zero or look like pointers (in the coarse
    ///     macOS arm64 userspace range).
    ///  3. Phase 2 (syscalls): for each candidate position, take the
    ///     first non-zero slot, treat it as a class pointer, read
    ///     `slot + CLASS_NAME` to get a name-pointer, read a string
    ///     from there, and check that the result is a plausible class
    ///     name (non-empty, short, ASCII-graphic). If 10+ slots in a
    ///     row yield valid names, the position is the table base.
    ///
    /// Returns the absolute address of the discovered type info table,
    /// or 0 if no plausible table was found in any __DATA segment.
    fn scan_for_type_info_table(reader: &MemReader, pid: u32) -> usize {
        // Coarse bounds for "looks like a loaded-library pointer on
        // macOS arm64 userspace". Real pointers into GameAssembly are
        // all in [0x100000000, 0x200000000] on current MTGA builds —
        // and the scanner is only a heuristic anyway, so a permissive
        // range is fine.
        const MIN_PTR: usize = 0x1_0000_0000;
        const MAX_PTR: usize = 0x2_0000_0000;
        const WINDOW_SLOTS: usize = 20;
        const PLAUSIBLE_THRESHOLD: usize = 18; // out of 20
        const NAME_VALIDATION_STREAK: usize = 10;

        let is_ptr = |v: usize| v == 0 || (v >= MIN_PTR && v <= MAX_PTR);
        // Any plausibly-decoded C# / IL2CPP identifier — including generic
        // placeholders like `$i1` — passes this check. Used in phase 2 to
        // filter garbage memory reads, but NOT enough on its own to pick
        // the right table (the IL2CPP generic-instance table is full of
        // `$i1`-style placeholders and will pass this check).
        let is_valid_name = |s: &str| {
            !s.is_empty()
                && s.len() <= 128
                && s.chars().all(|c| {
                    c.is_ascii_graphic() || c == '_' || c == '.' || c == '`' || c == '<' || c == '>'
                })
        };
        // A "rich" name is one that is almost certainly a real C# type
        // definition from user code (as opposed to a generic placeholder
        // or metadata marker): starts with letter/underscore, has
        // enough length to be meaningful, and either contains a
        // namespace dot or is capitalized (a PascalCase type name).
        // Counting these in candidate tables lets us distinguish the
        // main type_info_table (many rich names) from the generic
        // instance table (mostly $i1 placeholders).
        let is_rich_name = |s: &str| {
            if s.len() < 4 || s.len() > 128 {
                return false;
            }
            let first = match s.chars().next() {
                Some(c) => c,
                None => return false,
            };
            if !(first.is_ascii_alphabetic() || first == '_') {
                return false;
            }
            // All chars ASCII-identifier-ish
            if !s.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || c == '_'
                    || c == '.'
                    || c == '`'
                    || c == '<'
                    || c == '>'
                    || c == ','
                    || c == ' '
                    || c == '['
                    || c == ']'
            }) {
                return false;
            }
            // Either has a namespace dot OR starts with an uppercase
            // PascalCase-style identifier
            s.contains('.') || first.is_ascii_uppercase()
        };

        let segments = find_all_data_segments(pid);
        eprintln!(
            "scan_for_type_info_table: {} __DATA segments to scan: {:?}",
            segments.len(),
            segments
                .iter()
                .map(|(s, e)| format!("0x{:x}-0x{:x} ({}K)", s, e, (e - s) / 1024))
                .collect::<Vec<_>>(),
        );

        for (seg_start, seg_end) in segments {
            let seg_size = seg_end - seg_start;
            // Read the whole segment into a local buffer in one syscall.
            // MTGA's __DATA segments are single contiguous VM regions
            // per vmmap, so this call is cheap and doesn't span holes.
            let buf = reader.read_bytes(seg_start, seg_size);
            if buf.len() != seg_size {
                eprintln!(
                    "scan_for_type_info_table: segment 0x{:x}-0x{:x} short read ({} of {} bytes), skipping",
                    seg_start, seg_end, buf.len(), seg_size,
                );
                continue;
            }

            let slot_count = seg_size / 8;
            // Phase 1: collect in-memory candidates.
            let mut candidates: Vec<usize> = Vec::new();
            let mut i = 0;
            while i + WINDOW_SLOTS < slot_count {
                // Read window of 20 aligned slots directly from buffer.
                let mut plausible = 0usize;
                let mut any_nonzero = false;
                for j in 0..WINDOW_SLOTS {
                    let off = (i + j) * 8;
                    let slot = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
                        as usize;
                    if slot != 0 {
                        any_nonzero = true;
                    }
                    if is_ptr(slot) {
                        plausible += 1;
                    }
                }
                if plausible >= PLAUSIBLE_THRESHOLD && any_nonzero {
                    candidates.push(i);
                    // Skip ahead past this window to avoid flooding
                    // candidates with every shifted copy of the same
                    // hit. Phase 2 will verify more rigorously.
                    i += WINDOW_SLOTS;
                    continue;
                }
                i += 1;
            }
            eprintln!(
                "scan_for_type_info_table: segment 0x{:x}: {} phase-1 candidates",
                seg_start, candidates.len(),
            );

            // Phase 2 + 3: score candidates by the number of UNIQUE
            // rich class names they contain in their first VERIFY_DEPTH
            // slots. The main type_info_table has thousands of unique
            // classes (each C# type definition gets one slot); IL2CPP's
            // generic-specialization tables have repeats where every
            // slot shares the base generic name (e.g., 31 slots all
            // reading `AltAssetReference\`1`). Scoring by uniqueness
            // separates the two. We also require a meaningful floor of
            // unique rich names so a candidate with 3 unique names and
            // 100 zeros doesn't accidentally win.
            const VERIFY_DEPTH: usize = 300;
            const MIN_UNIQUE_RICH: usize = 20;

            use std::collections::HashSet;
            let mut best: Option<(usize, usize, Vec<String>)> = None;

            for ci in candidates {
                let off = ci * 8;
                let first_slot = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
                    as usize;
                if first_slot == 0 || !is_ptr(first_slot) {
                    continue;
                }
                // Does slot[0] look like a class?
                let name_ptr = reader.read_ptr(first_slot + offsets::CLASS_NAME);
                if name_ptr == 0 || !is_ptr(name_ptr) {
                    continue;
                }
                let first_name = reader.read_string(name_ptr);
                if !is_valid_name(&first_name) {
                    continue;
                }

                // Deep verify: count valid names + UNIQUE rich names
                // over VERIFY_DEPTH slots. Bail early if the walker
                // wanders off the end of the table into garbage.
                let mut valid_names = 0usize;
                let mut unique_rich: HashSet<String> = HashSet::new();
                let mut out_of_band = 0usize;

                for k in 0..VERIFY_DEPTH {
                    if ci + k >= slot_count {
                        break;
                    }
                    let off_k = (ci + k) * 8;
                    let slot_k = u64::from_le_bytes(
                        buf[off_k..off_k + 8].try_into().unwrap_or([0; 8]),
                    ) as usize;
                    if slot_k == 0 {
                        continue;
                    }
                    if !is_ptr(slot_k) {
                        // Non-pointer garbage: 3 in a row means we walked
                        // past the table's end, stop scanning.
                        out_of_band += 1;
                        if out_of_band >= 3 {
                            break;
                        }
                        continue;
                    }
                    out_of_band = 0;
                    let nk_ptr = reader.read_ptr(slot_k + offsets::CLASS_NAME);
                    if nk_ptr == 0 || !is_ptr(nk_ptr) {
                        continue;
                    }
                    let nk = reader.read_string(nk_ptr);
                    if !is_valid_name(&nk) {
                        continue;
                    }
                    valid_names += 1;
                    if is_rich_name(&nk) {
                        unique_rich.insert(nk);
                    }
                }

                let unique_count = unique_rich.len();
                if valid_names < NAME_VALIDATION_STREAK || unique_count < MIN_UNIQUE_RICH {
                    continue;
                }

                let addr = seg_start + off;
                let is_new_best = match &best {
                    Some((_, best_unique, _)) => unique_count > *best_unique,
                    None => true,
                };
                if is_new_best {
                    let mut samples: Vec<String> = unique_rich.iter().take(10).cloned().collect();
                    samples.sort();
                    eprintln!(
                        "scan_for_type_info_table: candidate at 0x{:x} valid={}, unique_rich={}, sample={:?}",
                        addr, valid_names, unique_count, samples,
                    );
                    best = Some((addr, unique_count, samples));
                }
            }

            if let Some((addr, unique, samples)) = best {
                eprintln!(
                    "scan_for_type_info_table: FOUND at 0x{:x} (best candidate in segment 0x{:x}), unique_rich_names={}, samples={:?}",
                    addr, seg_start, unique, samples,
                );
                return addr;
            }
        }
        0
    }

    fn find_class_by_name(reader: &MemReader, type_info_table: usize, name: &str) -> Option<usize> {
        // Unused when the caller prefers find_class_by_direct_scan
        // (which is more robust across table-layout drift). Kept for
        // compatibility with upstream code paths that still treat
        // `state.type_info_table` as authoritative.
        for i in 0..50000 {
            let class_ptr = reader.read_ptr(type_info_table + i * 8);
            if class_ptr == 0 {
                continue;
            }
            let name_ptr = reader.read_ptr(class_ptr + offsets::CLASS_NAME);
            if name_ptr == 0 {
                continue;
            }
            let class_name = reader.read_string(name_ptr);
            if class_name.is_empty() {
                continue;
            }
            if class_name == name {
                return Some(class_ptr);
            }
        }
        None
    }

    /// Scan both __DATA segments and dump every unique class name
    /// containing the given substring. Diagnostic only — used to
    /// discover the right class names when upstream's hardcoded
    /// names drift. Caps at `limit` results per call to avoid
    /// flooding stderr.
    fn dump_class_names_matching(reader: &MemReader, pid: u32, needle: &str, limit: usize) {
        use std::collections::HashSet;
        const MIN_PTR: usize = 0x1_0000_0000;
        const MAX_PTR: usize = 0x2_0000_0000;

        let segments = find_all_data_segments(pid);
        let mut seen: HashSet<usize> = HashSet::new();
        let mut names_seen: HashSet<String> = HashSet::new();
        let mut matches: Vec<String> = Vec::new();

        'outer: for (seg_start, seg_end) in segments {
            let size = seg_end - seg_start;
            let buf = reader.read_bytes(seg_start, size);
            if buf.len() != size {
                continue;
            }
            let slot_count = size / 8;
            for i in 0..slot_count {
                let off = i * 8;
                let p = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
                    as usize;
                if p < MIN_PTR || p > MAX_PTR {
                    continue;
                }
                if !seen.insert(p) {
                    continue;
                }
                let name_ptr = reader.read_ptr(p + offsets::CLASS_NAME);
                if name_ptr < MIN_PTR || name_ptr > MAX_PTR {
                    continue;
                }
                let class_name = reader.read_string(name_ptr);
                if class_name.is_empty() || class_name.len() > 128 {
                    continue;
                }
                // Must look like a real C# identifier, not garbage.
                if !class_name.chars().next().map_or(false, |c| c.is_ascii_alphabetic() || c == '_' || c == '<') {
                    continue;
                }
                if class_name.contains(needle) && names_seen.insert(class_name.clone()) {
                    matches.push(class_name);
                    if matches.len() >= limit {
                        break 'outer;
                    }
                }
            }
        }
        matches.sort();
        eprintln!(
            "dump_class_names_matching({:?}): {} unique match(es): {:?}",
            needle, matches.len(), matches,
        );
    }

    /// Scan all __DATA segments of GameAssembly.dylib for an
    /// `Il2CppClass*` whose `CLASS_NAME` string equals `name`.
    ///
    /// This is a more robust alternative to
    /// `find_class_by_name(type_info_table, ...)` because it does not
    /// depend on locating "the" type info table — IL2CPP's metadata
    /// layout has enough sub-tables (generic instantiations,
    /// per-assembly name lookups, interface method tables, etc.) that
    /// picking a specific one by heuristic is fragile. Every class
    /// pointer that matters for this importer is referenced from
    /// somewhere in __DATA at least once, so a direct pointer scan
    /// finds them regardless of which sub-table holds them.
    ///
    /// Algorithm:
    ///  1. Read both __DATA segments into memory in one syscall each.
    ///  2. Walk the buffer 8 bytes at a time, collect every
    ///     pointer-shaped value within the GameAssembly mapping range.
    ///  3. Deduplicate pointers via a HashSet so we only dereference
    ///     each candidate once.
    ///  4. For each unique candidate, read `ptr + CLASS_NAME` then
    ///     read the name string; compare to target.
    fn find_class_by_direct_scan(
        reader: &MemReader,
        pid: u32,
        name: &str,
    ) -> Option<usize> {
        use std::collections::HashSet;
        const MIN_PTR: usize = 0x1_0000_0000;
        const MAX_PTR: usize = 0x2_0000_0000;

        let segments = find_all_data_segments(pid);
        let mut seen: HashSet<usize> = HashSet::new();
        let mut checked: usize = 0;
        let mut matched: Option<usize> = None;

        for (seg_start, seg_end) in segments {
            let size = seg_end - seg_start;
            let buf = reader.read_bytes(seg_start, size);
            if buf.len() != size {
                continue;
            }
            let slot_count = size / 8;
            for i in 0..slot_count {
                let off = i * 8;
                let p = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
                    as usize;
                if p < MIN_PTR || p > MAX_PTR {
                    continue;
                }
                if !seen.insert(p) {
                    continue;
                }
                let name_ptr = reader.read_ptr(p + offsets::CLASS_NAME);
                if name_ptr < MIN_PTR || name_ptr > MAX_PTR {
                    continue;
                }
                let class_name = reader.read_string(name_ptr);
                if class_name.is_empty() || class_name.len() > 128 {
                    continue;
                }
                checked += 1;
                if class_name == name {
                    matched = Some(p);
                    break;
                }
            }
            if matched.is_some() {
                break;
            }
        }
        eprintln!(
            "find_class_by_direct_scan: target={:?}, unique_candidates_checked={}, found={}",
            name,
            checked,
            matched.is_some(),
        );
        matched
    }

    /// Enumerate all `Il2CppClass*` addresses in `__DATA` whose
    /// class-name field **contains** the given substring. Useful
    /// for discovery — e.g. `"Inventory"` will surface
    /// `ClientPlayerInventory`, `AwsInventoryServiceWrapper`,
    /// `InventoryManager`, etc. Returns `(class_ptr, class_name)`
    /// pairs.
    fn find_classes_by_name_substr(
        reader: &MemReader,
        pid: u32,
        substr: &str,
    ) -> Vec<(usize, String)> {
        use std::collections::HashSet;
        const MIN_PTR: usize = 0x1_0000_0000;
        const MAX_PTR: usize = 0x2_0000_0000;
        let segments = find_all_data_segments(pid);
        let mut seen: HashSet<usize> = HashSet::new();
        let mut matches: Vec<(usize, String)> = Vec::new();
        for (seg_start, seg_end) in segments {
            let size = seg_end - seg_start;
            let buf = reader.read_bytes(seg_start, size);
            if buf.len() != size {
                continue;
            }
            for i in 0..size / 8 {
                let off = i * 8;
                let p = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
                    as usize;
                if p < MIN_PTR || p > MAX_PTR || !seen.insert(p) {
                    continue;
                }
                let name_ptr = reader.read_ptr(p + offsets::CLASS_NAME);
                if name_ptr < MIN_PTR || name_ptr > MAX_PTR {
                    continue;
                }
                let class_name = reader.read_string(name_ptr);
                if class_name.is_empty() || class_name.len() > 128 {
                    continue;
                }
                if class_name.contains(substr) {
                    matches.push((p, class_name));
                }
            }
        }
        matches
    }

    /// Count how many 8-byte-aligned occurrences of `target` exist
    /// in the scannable heap regions. Diagnostic helper for
    /// confirming whether a given class pointer is even referenced
    /// anywhere in the heap we're scanning.
    fn count_pointer_occurrences_in_heap(
        reader: &MemReader,
        pid: u32,
        target: usize,
    ) -> (usize, Vec<usize>) {
        // Returns (count, first_few_addresses).
        let regions = find_scannable_heap_regions(pid);
        let mut count = 0usize;
        let mut sample: Vec<usize> = Vec::new();
        for (start, end) in regions {
            let size = end - start;
            let buf = reader.read_bytes(start, size);
            if buf.len() != size {
                continue;
            }
            let slot_count = size / 8;
            for i in 0..slot_count {
                let off = i * 8;
                let p = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
                    as usize;
                if p == target {
                    count += 1;
                    if sample.len() < 10 {
                        sample.push(start + off);
                    }
                }
            }
        }
        (count, sample)
    }

    /// Enumerate ALL `Il2CppClass*` addresses in `__DATA` whose
    /// class-name field matches the given name. `find_class_by_direct_scan`
    /// returns the first match, but IL2CPP often keeps multiple
    /// `Il2CppClass` structs for the same logical type (metadata
    /// table entry + one or more runtime vtable owners) at different
    /// addresses. For heap-scan use cases we need all of them so the
    /// instance filter accepts whichever variant the GC-managed
    /// objects actually reference.
    fn find_all_classes_by_name(
        reader: &MemReader,
        pid: u32,
        name: &str,
    ) -> Vec<usize> {
        use std::collections::HashSet;
        const MIN_PTR: usize = 0x1_0000_0000;
        const MAX_PTR: usize = 0x2_0000_0000;

        let segments = find_all_data_segments(pid);
        let mut seen: HashSet<usize> = HashSet::new();
        let mut matches: Vec<usize> = Vec::new();

        for (seg_start, seg_end) in segments {
            let size = seg_end - seg_start;
            let buf = reader.read_bytes(seg_start, size);
            if buf.len() != size {
                continue;
            }
            let slot_count = size / 8;
            for i in 0..slot_count {
                let off = i * 8;
                let p = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
                    as usize;
                if p < MIN_PTR || p > MAX_PTR {
                    continue;
                }
                if !seen.insert(p) {
                    continue;
                }
                let name_ptr = reader.read_ptr(p + offsets::CLASS_NAME);
                if name_ptr < MIN_PTR || name_ptr > MAX_PTR {
                    continue;
                }
                let class_name = reader.read_string(name_ptr);
                if class_name.is_empty() || class_name.len() > 128 {
                    continue;
                }
                if class_name == name {
                    matches.push(p);
                }
            }
        }
        if std::env::var("MTGA_DEBUG_INVENTORY").is_ok() {
            eprintln!(
                "find_all_classes_by_name: target={:?}, matches={}",
                name,
                matches.len(),
            );
        }
        matches
    }

    pub fn read_class_name(reader: &MemReader, class: usize) -> String {
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

    /// Find PAPA's singleton instance by reading a static field
    /// directly out of the class's static-fields region, rather than
    /// heap-scanning. This is much more reliable: every C#
    /// `public static Instance { get; }` compiles to a backing field
    /// on the declaring class whose value is the singleton pointer.
    /// We don't have to guess object layouts or scan gigabytes of
    /// heap.
    ///
    /// Returns the first non-null pointer found in a static field of
    /// PAPA whose name contains `"instance"` (case-insensitive). Also
    /// dumps the full field list of PAPA to stderr for debugging
    /// when the lookup fails — that's how we'll chase down any
    /// future renames of the backing field.
    fn find_papa_instance_via_static_field(
        reader: &MemReader,
        papa_class: usize,
    ) -> Option<usize> {
        let fields = get_class_fields(reader, papa_class);
        eprintln!(
            "find_papa_instance_via_static_field: PAPA has {} field(s)",
            fields.len(),
        );

        let static_fields_base = reader.read_ptr(papa_class + offsets::CLASS_STATIC_FIELDS);
        eprintln!(
            "find_papa_instance_via_static_field: CLASS_STATIC_FIELDS base = 0x{:x}",
            static_fields_base,
        );

        // Pass 1: report every field so we can see the layout.
        for (i, field) in fields.iter().enumerate() {
            let value = if field.is_static && static_fields_base > 0x100000 {
                reader.read_ptr(static_fields_base + field.offset as usize)
            } else {
                0
            };
            eprintln!(
                "  field[{}] name={:?} type={:?} offset=0x{:x} is_static={} static_value=0x{:x}",
                i, field.name, field.type_name, field.offset, field.is_static, value,
            );
        }

        if static_fields_base < 0x100000 {
            return None;
        }

        // Pass 2: return the first plausible static instance pointer.
        // Prefer a field explicitly named `<Instance>k__BackingField`
        // (the compiler-generated backing field for a standard C#
        // `public static Instance { get; }` auto-property), then fall
        // back to any static field whose name contains "instance".
        let mut preferred: Option<usize> = None;
        let mut fallback: Option<usize> = None;
        for field in &fields {
            if !field.is_static {
                continue;
            }
            let value = reader.read_ptr(static_fields_base + field.offset as usize);
            if value < 0x100000 {
                continue;
            }
            let name_lower = field.name.to_ascii_lowercase();
            if field.name == "<Instance>k__BackingField" {
                preferred = Some(value);
                break;
            }
            if name_lower.contains("instance") && fallback.is_none() {
                fallback = Some(value);
            }
        }
        preferred.or(fallback)
    }

    /// Find PAPA's singleton instance by cross-verifying two
    /// independently discovered class pointers: the object's own
    /// class pointer (which must equal `papa_class`) AND the
    /// InventoryManager field at offset `<InventoryManager>k__BackingField`
    /// on PAPA (which must dereference to an object whose class
    /// pointer equals InventoryManager's class). The combination
    /// uniquely identifies the real PAPA instance — random heap
    /// pointers matching one check are common, but matching BOTH
    /// simultaneously is astronomically unlikely.
    ///
    /// This sidesteps every static-field / object-layout offset
    /// assumption. We only need:
    ///  - `papa_class` (found via direct scan)
    ///  - The offset of `<InventoryManager>k__BackingField` in PAPA
    ///    (read from `get_class_fields` at runtime — the field name
    ///    matches exactly on current MTGA builds)
    ///  - `InventoryManager` class pointer (also found via direct scan)
    ///
    /// Scans writable heap regions and returns the first match where
    /// both checks pass. If InventoryManager class isn't found via
    /// direct scan OR the field enumeration for PAPA doesn't turn
    /// up `<InventoryManager>k__BackingField`, returns None and lets
    /// the caller fall through to the next strategy.
    /// Read an object-typed static field from a class. Returns the
    /// pointer value stored in the static field, or 0 if anything
    /// goes wrong. Used to resolve singleton `Instance` fields like
    /// `WrapperController.<Instance>k__BackingField` which are the
    /// documented C# singleton pattern.
    fn read_static_object_field(
        reader: &MemReader,
        class_addr: usize,
        field_name: &str,
    ) -> usize {
        let fields = get_class_fields(reader, class_addr);
        let field = match fields.iter().find(|f| f.name == field_name) {
            Some(f) => f,
            None => {
                eprintln!(
                    "read_static_object_field: class 0x{:x} has no field named {:?}",
                    class_addr, field_name,
                );
                return 0;
            }
        };
        if !field.is_static {
            eprintln!(
                "read_static_object_field: field {:?} on class 0x{:x} is not marked static (offset=0x{:x})",
                field_name, class_addr, field.offset,
            );
            return 0;
        }
        let static_base = reader.read_ptr(class_addr + offsets::CLASS_STATIC_FIELDS);
        if static_base < 0x100000 {
            eprintln!(
                "read_static_object_field: CLASS_STATIC_FIELDS for class 0x{:x} is 0x{:x}, unusable",
                class_addr, static_base,
            );
            return 0;
        }
        let value = reader.read_ptr(static_base + field.offset as usize);
        eprintln!(
            "read_static_object_field: class 0x{:x} field {:?} static_base=0x{:x} field_offset=0x{:x} value=0x{:x}",
            class_addr, field_name, static_base, field.offset, value,
        );
        value
    }

    /// Locate the WrapperController singleton instance using the
    /// documented C# singleton pattern (`WrapperController.Instance`
    /// auto-property → `<Instance>k__BackingField`). Falls back to
    /// scanning the heap for any object whose class pointer equals
    /// `WrapperController` if the static field read fails.
    ///
    /// On success returns a pointer to a real WrapperController
    /// instance; the caller can use it as `state.papa_instance`
    /// (misnomer kept for API compatibility — the walker only cares
    /// that the value is a real object whose class the reader can
    /// resolve).
    /// Signature-scan the heap for a `Dictionary<int, int>` whose
    /// contents look like an Arena card collection: many entries,
    /// keys in the card-id range, values in the quantity range.
    ///
    /// This deliberately avoids every IL2CPP class/metadata offset
    /// (`CLASS_NAME`, `CLASS_FIELDS`, `CLASS_STATIC_FIELDS`) because
    /// those have been unreliable at every level above runtime
    /// instance data on current MTGA builds. The only layout
    /// assumption is the documented .NET `Dictionary<TKey, TValue>`
    /// object layout:
    ///
    /// ```text
    /// +0x00  klass pointer       (Il2CppObject header)
    /// +0x08  monitor pointer     (Il2CppObject header)
    /// +0x10  buckets[] pointer
    /// +0x18  entries[] pointer   <-- what we read
    /// +0x20  count (int32)       <-- what we read
    /// ```
    ///
    /// And the documented `Dictionary<int, int>.Entry` layout, which
    /// upstream's existing dictionary-reader code already uses:
    ///
    /// ```text
    /// Entry[] array header is 0x20 bytes (klass + monitor + length)
    /// Each entry is 16 bytes starting at entries[] + 0x20:
    ///   +0x00  hash   (int32; -1 means empty slot)
    ///   +0x04  next   (int32; unused here)
    ///   +0x08  key    (int32)   <-- cardId
    ///   +0x0c  value  (int32)   <-- quantity
    /// ```
    ///
    /// Arena card IDs are small positive integers (typically
    /// 1..200_000), and quantities are 1..4 (we accept up to 99 to
    /// tolerate weird edge cases like event rewards or currency
    /// counters). A card collection dictionary has thousands of
    /// entries, so we require at least `MIN_COUNT` to drop noise from
    /// small runtime dictionaries.
    ///
    /// Returns the address of the best-scoring dictionary (most
    /// entries where the first 10 sampled entries all validate), or
    /// 0 if nothing passed.
    /// Parse the `MTGA_KNOWN_CARD_IDS` environment variable into a
    /// set of arena_ids the caller knows should appear in the real
    /// collection dict. Format: comma-separated decimal integers,
    /// e.g., `"90881,90804,91088"`. Empty or unset → empty set,
    /// which disables the known-ids cross-check.
    fn parse_known_card_ids_env() -> std::collections::HashSet<i32> {
        let raw = std::env::var("MTGA_KNOWN_CARD_IDS").unwrap_or_default();
        raw.split(',')
            .filter_map(|s| s.trim().parse::<i32>().ok())
            .collect()
    }

    /// Byte-pattern scan: find every 8-byte-aligned position in any
    /// writable heap region where the int32 at +0 equals the int32
    /// at +8 (the defining hash==key shape of a .NET
    /// Dictionary<int, TValue>.Entry for any value type TValue). For
    /// each hit, dump the surrounding 32 bytes so we can recognize
    /// entry stride from context.
    ///
    /// Targeted mode: if `target_key` is Some, only report hits
    /// whose hash/key matches the target. Used to confirm whether a
    /// specific (cardId, quantity) pair exists anywhere in Arena's
    /// memory without assuming entry size.
    fn scan_for_dict_entry_pattern(
        reader: &MemReader,
        pid: u32,
        target_key: Option<i32>,
        expected_value: Option<i32>,
        max_hits: usize,
    ) {
        eprintln!(
            "scan_for_dict_entry_pattern: target_key={:?} expected_value={:?}",
            target_key, expected_value,
        );
        let heap_regions = find_scannable_heap_regions(pid);
        let mut hits: Vec<(usize, Vec<u8>)> = Vec::new();

        for (start, end) in heap_regions {
            let size = end - start;
            let buf = reader.read_bytes(start, size);
            if buf.len() != size {
                continue;
            }
            // Walk every 4-byte-aligned offset looking for int at +0
            // matching int at +8.
            let mut i = 0usize;
            while i + 16 <= buf.len() {
                let a = i32::from_le_bytes(buf[i..i + 4].try_into().unwrap_or([0; 4]));
                let b = i32::from_le_bytes(buf[i + 8..i + 12].try_into().unwrap_or([0; 4]));
                if a != 0 && a == b {
                    // Potential hash == key
                    let matches_target = match target_key {
                        Some(t) => a == t,
                        None => a > 1000 && a < 200_000, // plausible card id
                    };
                    if matches_target {
                        // Record surrounding 32 bytes for context (if in bounds)
                        let ctx_start = i.saturating_sub(0);
                        let ctx_end = (i + 32).min(buf.len());
                        let ctx_bytes = buf[ctx_start..ctx_end].to_vec();
                        hits.push((start + i, ctx_bytes));
                        if hits.len() >= max_hits {
                            break;
                        }
                    }
                }
                i += 4;
            }
            if hits.len() >= max_hits {
                break;
            }
        }

        eprintln!(
            "scan_for_dict_entry_pattern: {} hits for target={:?}",
            hits.len(), target_key,
        );
        for (addr, bytes) in hits.iter().take(20) {
            let b32: Vec<String> = (0..bytes.len() / 4)
                .map(|k| {
                    let v = i32::from_le_bytes(
                        bytes[k * 4..k * 4 + 4].try_into().unwrap_or([0; 4]),
                    );
                    format!("{}", v)
                })
                .collect();
            eprintln!(
                "  0x{:x}: [{}]",
                addr,
                b32.join(" "),
            );
        }
    }

    /// Parse `MTGA_VERIFY_QTYS` as a map of `arena_id → expected_quantity`.
    /// Format: comma-separated `<id>:<qty>` pairs, e.g.,
    /// `"98307:4,98487:3,90804:2"`. Used by the scanner to
    /// distinguish between multiple dicts that all pass the base
    /// signature — the correct collection dict will have the
    /// expected quantities for these specific cards, while stale
    /// caches or format-filtered subsets will have different
    /// (smaller or absent) values.
    fn parse_verify_qtys_env() -> std::collections::HashMap<i32, i32> {
        let raw = std::env::var("MTGA_VERIFY_QTYS").unwrap_or_default();
        let mut out = std::collections::HashMap::new();
        for pair in raw.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let mut parts = pair.splitn(2, ':');
            let id_part = parts.next().unwrap_or("");
            let qty_part = parts.next().unwrap_or("");
            if let (Ok(id), Ok(qty)) = (id_part.parse::<i32>(), qty_part.parse::<i32>()) {
                out.insert(id, qty);
            }
        }
        out
    }

    fn scan_heap_for_cards_dictionary(reader: &MemReader, pid: u32) -> usize {
        // Arena collections have hundreds to low tens-of-thousands
        // of entries. `hash == key` below is the load-bearing
        // signature; the count range is a secondary filter that
        // mostly drops small on-demand dicts created by game state.
        // Arena collections: 500-50000 entries. Tight upper bound
        // matters — without env-var ground-truth (known_ids /
        // verify_qtys), the selection falls through to
        // "biggest count wins", and letting 50k+ dicts through
        // means junk game-state dicts (e.g., hash=3/key=3 counter
        // arrays) can beat real collections. 50k is plenty for
        // a live card collection; anything bigger is noise.
        const MIN_COUNT: i32 = 500;
        const MAX_COUNT: i32 = 50_000;
        // Arena card IDs (internal "grp_id" / Arena IDs) are always
        // small positive integers in the observed range
        // ~60_000..110_000 but the range slowly extends as new sets
        // release. Keep a generous upper bound.
        const MIN_CARD_ID: i32 = 1;
        const MAX_CARD_ID: i32 = 200_000;
        // Arena's internal card-ownership model caps quantities at
        // 4. Any card with "any number allowed" rules text (Hare
        // Apparent, Persistent Petitioners, Seven Dwarves, Rat
        // Colony, Relentless Rats, Shadowborn Apostle) is still
        // capped at 4 internally. With MAX_QUANTITY=4 the real
        // card collection is essentially the only Dictionary<int,
        // int> that passes; relaxing this bound lets junk
        // counter-shaped dicts through.
        const MIN_QUANTITY: i32 = 1;
        const MAX_QUANTITY: i32 = 4;
        // Sample more entries to handle hash buckets that happen to
        // have many empty slots at the start of the entries array.
        const SAMPLE_ENTRIES: usize = 30;
        const MIN_VALID_SAMPLES: usize = 12;
        const MIN_PTR: usize = 0x1_0000_0000;
        const MAX_PTR: usize = 0x4_0000_0000;

        let known_ids = parse_known_card_ids_env();
        let verify_qtys = parse_verify_qtys_env();
        let heap_regions = find_scannable_heap_regions(pid);
        eprintln!(
            "scan_heap_for_cards_dictionary: scanning {} heap regions for Dictionary<int,int> with {}-{} entries, values in [{}..{}], known_ids={:?}, verify_qtys={:?}",
            heap_regions.len(), MIN_COUNT, MAX_COUNT, MIN_QUANTITY, MAX_QUANTITY, known_ids, verify_qtys,
        );

        // Collect EVERY candidate that passes validation rather than
        // only the largest. Multiple int→int dictionaries can coexist
        // (counters, progress, state, cards), and the "biggest that
        // looks valid" heuristic can still land on the wrong one.
        // Printing them all lets us tell which is the real collection.
        let mut candidates: Vec<(usize, i32, Vec<(i32, i32, i32)>)> = Vec::new(); // (addr, count, first_valid_samples)
        let mut candidates_examined = 0usize;

        for (start, end) in heap_regions {
            let size = end - start;
            let buf = reader.read_bytes(start, size);
            if buf.len() != size {
                continue;
            }
            let slot_count = size / 8;
            let mut i = 0;
            while i + 5 < slot_count {
                let base = i * 8;
                let buckets_ptr = u64::from_le_bytes(
                    buf[base + 0x10..base + 0x18].try_into().unwrap_or([0; 8]),
                ) as usize;
                let entries_ptr = u64::from_le_bytes(
                    buf[base + 0x18..base + 0x20].try_into().unwrap_or([0; 8]),
                ) as usize;
                let count = i32::from_le_bytes(
                    buf[base + 0x20..base + 0x24].try_into().unwrap_or([0; 4]),
                );

                if count < MIN_COUNT
                    || count > MAX_COUNT
                    || buckets_ptr < MIN_PTR
                    || buckets_ptr > MAX_PTR
                    || entries_ptr < MIN_PTR
                    || entries_ptr > MAX_PTR
                {
                    i += 1;
                    continue;
                }

                candidates_examined += 1;

                let mut valid = 0usize;
                let mut sample_valid_entries: Vec<(i32, i32, i32)> = Vec::new(); // (hash, key, value)
                for entry_idx in 0..SAMPLE_ENTRIES {
                    let entry_addr = entries_ptr + 0x20 + entry_idx * 16;
                    let entry_bytes = reader.read_bytes(entry_addr, 16);
                    if entry_bytes.len() != 16 {
                        break;
                    }
                    let hash = i32::from_le_bytes(entry_bytes[0..4].try_into().unwrap_or([0; 4]));
                    let key = i32::from_le_bytes(entry_bytes[8..12].try_into().unwrap_or([0; 4]));
                    let value = i32::from_le_bytes(entry_bytes[12..16].try_into().unwrap_or([0; 4]));

                    if hash == -1 {
                        // .NET dict empty-slot marker.
                        continue;
                    }
                    // Defining signature of a real Dictionary<int, int>
                    // with default equality comparer:
                    //    hash == key
                    // because EqualityComparer<int>.Default.GetHashCode(x) == x.
                    // No other int-keyed dictionary in the Arena process
                    // has this property — counter / stats / rarity
                    // dicts either use non-default comparers or store
                    // data in structs where `hash` at offset 0 is
                    // something else entirely. This is the tight
                    // check that distinguishes the real card
                    // collection from every other Dictionary<int,
                    // int>-shaped thing in the heap.
                    if hash == key
                        && key >= MIN_CARD_ID
                        && key <= MAX_CARD_ID
                        && value >= MIN_QUANTITY
                        && value <= MAX_QUANTITY
                    {
                        valid += 1;
                        if sample_valid_entries.len() < 5 {
                            sample_valid_entries.push((hash, key, value));
                        }
                    }
                }

                if valid >= MIN_VALID_SAMPLES {
                    let dict_addr = start + base;
                    candidates.push((dict_addr, count, sample_valid_entries));
                }
                i += 1;
            }
        }

        candidates.sort_by_key(|(_, count, _)| std::cmp::Reverse(*count));
        eprintln!(
            "scan_heap_for_cards_dictionary: examined {} pre-filter candidates, {} passed validation",
            candidates_examined, candidates.len(),
        );
        for (i, (addr, count, samples)) in candidates.iter().take(10).enumerate() {
            let sample_strs: Vec<String> = samples
                .iter()
                .map(|(h, k, v)| format!("(hash={},key={},val={})", h, k, v))
                .collect();
            eprintln!(
                "  [{}] 0x{:x} count={} samples=[{}]",
                i, addr, count, sample_strs.join(", "),
            );
        }

        // Score each candidate by:
        //   1. Number of `known_ids` present (membership check)
        //   2. Number of `verify_qtys` whose quantity matches exactly
        //      (verification — this distinguishes stale/cached
        //      dicts from the live collection dict because the
        //      quantities will differ)
        //
        // Tiebreakers: prefer the candidate with more extracted
        // entries, then the bigger `count` field.
        //
        // If neither env var is set, we fall back to "biggest count
        // wins" — still wrong in the general case but it's the
        // best we can do without ground truth.
        let best = if !known_ids.is_empty() || !verify_qtys.is_empty() {
            #[allow(clippy::type_complexity)]
            let mut scored: Vec<(usize, i32, usize, usize, usize)> = Vec::new(); // (addr, count, matched_known, matched_qtys, total_valid)
            for (addr, count, _) in &candidates {
                let entries = read_cards_dictionary_entries(reader, *addr);
                let by_id: std::collections::HashMap<i32, i32> =
                    entries.iter().copied().collect();
                let matched_known: usize = known_ids
                    .iter()
                    .filter(|id| by_id.contains_key(id))
                    .count();
                let matched_qtys: usize = verify_qtys
                    .iter()
                    .filter(|(id, expected)| by_id.get(*id) == Some(*expected))
                    .count();
                scored.push((*addr, *count, matched_known, matched_qtys, entries.len()));
                eprintln!(
                    "  scoring 0x{:x}: count={} extracted={} known_ids={}/{} verify_qtys={}/{}",
                    addr, count, entries.len(),
                    matched_known, known_ids.len(),
                    matched_qtys, verify_qtys.len(),
                );
            }
            // Rank: verify_qtys is the strictest signal (only the
            // TRUE live collection has exactly-matching quantities),
            // then known_ids presence, then total extracted count.
            scored.sort_by(|a, b| {
                b.3.cmp(&a.3)
                    .then_with(|| b.2.cmp(&a.2))
                    .then_with(|| b.4.cmp(&a.4))
            });
            scored.first().and_then(|(addr, _, _, _, _)| {
                candidates
                    .iter()
                    .find(|(a, _, _)| a == addr)
                    .cloned()
            })
        } else {
            candidates.first().cloned()
        };

        match best {
            Some((addr, count, samples)) => {
                let sample_strs: Vec<String> = samples
                    .iter()
                    .map(|(h, k, v)| format!("(h={},k={},v={})", h, k, v))
                    .collect();
                eprintln!(
                    "scan_heap_for_cards_dictionary: SELECTED 0x{:x} count={} samples=[{}]",
                    addr, count, sample_strs.join(", "),
                );
                addr
            }
            None => 0,
        }
    }

    /// Read the card entries out of a previously-discovered
    /// Dictionary<int, int> object.
    ///
    /// Applies the SAME filter that `scan_heap_for_cards_dictionary`
    /// uses to identify the dict in the first place: only accept
    /// entries where `hash == key` (the defining signature of
    /// `Dictionary<int, int>` with the default equality comparer,
    /// since `EqualityComparer<int>.Default.GetHashCode(x) == x`),
    /// the key is a plausible Arena card id, and the value is in
    /// the 1..4 ownership range. Entries that fail these checks are
    /// skipped rather than returned as garbage rows: they represent
    /// either deleted/rehashed slots (common in any `Dictionary<K,V>`
    /// that has seen removals) or array-tail padding past the count.
    /// Without this filter we would emit hundreds of ghost rows that
    /// downstream Arena-id → name resolution has no hope of mapping
    /// to real cards.
    fn read_cards_dictionary_entries(
        reader: &MemReader,
        dict_addr: usize,
    ) -> Vec<(i32, i32)> {
        const MIN_CARD_ID: i32 = 1;
        const MAX_CARD_ID: i32 = 200_000;
        // Matches the tight value range in
        // scan_heap_for_cards_dictionary (see comment there):
        // Arena's internal card-ownership cap is 4, so any entry
        // with value > 4 is almost certainly not a card collection
        // entry.
        const MIN_QUANTITY: i32 = 1;
        const MAX_QUANTITY: i32 = 4;

        let entries_ptr = reader.read_ptr(dict_addr + 0x18);
        let count = reader.read_i32(dict_addr + 0x20);
        if entries_ptr < 0x100000 || count <= 0 {
            return Vec::new();
        }
        let mut entries = Vec::new();
        let mut skipped_empty = 0usize;
        let mut skipped_mismatched_hash = 0usize;
        let mut skipped_out_of_range = 0usize;
        for i in 0..count.min(50_000) as usize {
            let entry_addr = entries_ptr + 0x20 + i * 16;
            let hash = reader.read_i32(entry_addr);
            let key = reader.read_i32(entry_addr + 8);
            let value = reader.read_i32(entry_addr + 12);
            if hash == -1 {
                skipped_empty += 1;
                continue;
            }
            if hash != key {
                skipped_mismatched_hash += 1;
                continue;
            }
            if key < MIN_CARD_ID
                || key > MAX_CARD_ID
                || value < MIN_QUANTITY
                || value > MAX_QUANTITY
            {
                skipped_out_of_range += 1;
                continue;
            }
            entries.push((key, value));
        }
        eprintln!(
            "read_cards_dictionary_entries: count={} kept={} skipped(empty={}, hash!=key={}, out_of_range={})",
            count,
            entries.len(),
            skipped_empty,
            skipped_mismatched_hash,
            skipped_out_of_range,
        );
        entries
    }


    /// Field offsets on the `ClientPlayerInventory` class, resolved
    /// at runtime by name via `get_class_fields`.
    ///
    /// The C# source's property names are `wcCommon` / `wcUncommon`
    /// / `wcRare` / `wcMythic` / `gold` / `gems` / `vaultProgress`,
    /// but Arena's serialized log format sometimes uses
    /// `WildCardCommons` etc. instead — we try several candidate
    /// names per logical field and accept the first match.
    #[derive(Debug, Clone)]
    struct InventoryFieldOffsets {
        wc_common: usize,
        wc_uncommon: usize,
        wc_rare: usize,
        wc_mythic: usize,
        gold: usize,
        gems: usize,
        vault_progress: usize,
    }

    fn resolve_inventory_field_offsets(
        fields: &[FieldInfo],
    ) -> Option<InventoryFieldOffsets> {
        // Try each candidate name in order; first non-static match wins.
        let find = |candidates: &[&str]| -> Option<usize> {
            for name in candidates {
                if let Some(f) = fields.iter().find(|f| !f.is_static && f.name == *name) {
                    return Some(f.offset as usize);
                }
            }
            None
        };
        Some(InventoryFieldOffsets {
            wc_common: find(&[
                "wcCommon",
                "<wcCommon>k__BackingField",
                "WildCardCommons",
                "<WildCardCommons>k__BackingField",
                "_wcCommon",
            ])?,
            wc_uncommon: find(&[
                "wcUncommon",
                "<wcUncommon>k__BackingField",
                "WildCardUnCommons",
                "WildCardUncommons",
                "<WildCardUnCommons>k__BackingField",
                "_wcUncommon",
            ])?,
            wc_rare: find(&[
                "wcRare",
                "<wcRare>k__BackingField",
                "WildCardRares",
                "<WildCardRares>k__BackingField",
                "_wcRare",
            ])?,
            wc_mythic: find(&[
                "wcMythic",
                "<wcMythic>k__BackingField",
                "WildCardMythics",
                "<WildCardMythics>k__BackingField",
                "_wcMythic",
            ])?,
            gold: find(&[
                "gold",
                "<gold>k__BackingField",
                "Gold",
                "_gold",
            ])?,
            gems: find(&[
                "gems",
                "<gems>k__BackingField",
                "Gems",
                "_gems",
            ])?,
            vault_progress: find(&[
                "vaultProgress",
                "<vaultProgress>k__BackingField",
                "VaultProgress",
                "<VaultProgress>k__BackingField",
                "_vaultProgress",
            ])?,
        })
    }

    /// Plausibility check on an inventory-shaped object's field
    /// values. Used during the heap scan to filter candidates.
    ///
    /// Range constraints (deliberately generous):
    /// - Wildcards: `[0, 99_999]` — Untapped Premium accounts can
    ///   accumulate thousands; allow headroom.
    /// - Gold: `[0, 10^9]` — nobody actually hits a billion, but
    ///   int32 max is 2.1B so anything non-negative is plausible.
    /// - Gems: `[0, 10^7]`
    /// - VaultProgress: **not range-checked**. Observed live values
    ///   are `0x33333333 = 858_993_459` which is neither a clean
    ///   int percentage nor a plausible IEEE float; either the
    ///   field is stored as a fixed-point representation we don't
    ///   understand yet or the field is actually 8 bytes (a
    ///   `double`/`long`) with the low 4 bytes being a poison-like
    ///   pattern. Field spacing in the class struct
    ///   (`vaultProgress @ 0x30`, `boosters @ 0x38`) supports the
    ///   8-byte theory. We report the raw i32 and let callers decide.
    ///
    /// Plus a **non-triviality requirement**: at least one of
    /// {wildcards, gold, gems} must be non-zero. A player logged in
    /// to Arena has completed the NPE tutorial, which grants gold
    /// and wildcards; an ALL-ZERO inventory is either uninitialized
    /// or a metadata false positive. vault_progress is excluded
    /// from the non-zero check because its encoding is unclear.
    fn inventory_fields_look_plausible(
        wc_common: i32,
        wc_uncommon: i32,
        wc_rare: i32,
        wc_mythic: i32,
        gold: i32,
        gems: i32,
        _vault_progress: i32,
    ) -> bool {
        let in_range = (0..=99_999).contains(&wc_common)
            && (0..=99_999).contains(&wc_uncommon)
            && (0..=99_999).contains(&wc_rare)
            && (0..=99_999).contains(&wc_mythic)
            && (0..=1_000_000_000).contains(&gold)
            && (0..=10_000_000).contains(&gems);
        if !in_range {
            return false;
        }
        (wc_common | wc_uncommon | wc_rare | wc_mythic | gold | gems) != 0
    }

    /// Priority score for an inventory candidate. Higher means
    /// "more like a live, populated inventory." Used to break ties
    /// when multiple heap slots pass the plausibility filter AND
    /// have a class that resolves to `ClientPlayerInventory`.
    /// Excludes vault_progress because its int32 interpretation is
    /// unreliable.
    fn inventory_activity_score(
        wc_common: i32,
        wc_uncommon: i32,
        wc_rare: i32,
        wc_mythic: i32,
        gold: i32,
        gems: i32,
        _vault_progress: i32,
    ) -> i64 {
        wc_common as i64
            + wc_uncommon as i64
            + wc_rare as i64
            + wc_mythic as i64
            + gold as i64
            + gems as i64
    }

    /// Heap-scan for a `ClientPlayerInventory` instance.
    ///
    /// **Pre-filter**: enumerate every `Il2CppClass*` in `__DATA`
    /// whose name is `ClientPlayerInventory` (1 or more — IL2CPP
    /// keeps multiple variants). Only accept heap slots whose klass
    /// pointer is in this set. This collapses the "is it an
    /// inventory?" check to a hash lookup instead of reading+string-
    /// comparing 2M random class pointers, which was burning most
    /// of the scan time without finding anything.
    ///
    /// **Pass 1**: for each 8-byte-aligned slot whose `+0` matches a
    /// known ClientPlayerInventory klass pointer, read the seven
    /// inventory fields at their resolved offsets. Keep only
    /// candidates whose field values pass the plausibility check
    /// (wildcards in range, currency in range, and at least one
    /// field non-zero).
    ///
    /// **Pass 2**: among surviving candidates, pick the one with the
    /// highest activity score. Multiple ClientPlayerInventory
    /// instances can coexist (live + cached + pending update); the
    /// "most populated" one is the live account state.
    fn scan_heap_for_client_player_inventory(
        reader: &MemReader,
        pid: u32,
        offsets: &InventoryFieldOffsets,
        cpi_classes: &[usize],
    ) -> Option<usize> {
        use std::collections::HashSet;
        let debug = std::env::var("MTGA_DEBUG_INVENTORY").is_ok();

        if cpi_classes.is_empty() {
            eprintln!(
                "scan_heap_for_client_player_inventory: caller passed empty class set, nothing to scan for",
            );
            return None;
        }
        let class_set: HashSet<usize> = cpi_classes.iter().copied().collect();

        // Required tail read: the highest field offset plus 4 bytes
        // (i32 width).
        let max_off = [
            offsets.wc_common,
            offsets.wc_uncommon,
            offsets.wc_rare,
            offsets.wc_mythic,
            offsets.gold,
            offsets.gems,
            offsets.vault_progress,
        ]
        .into_iter()
        .max()
        .unwrap_or(0);
        let min_obj_size = max_off + 4;

        let heap_regions = find_scannable_heap_regions(pid);
        if debug {
            eprintln!(
                "scan_heap_for_client_player_inventory: scanning {} heap regions (field span = {} bytes, {} known cpi class ptrs: {:?})",
                heap_regions.len(),
                min_obj_size,
                cpi_classes.len(),
                cpi_classes.iter().map(|p| format!("0x{:x}", p)).collect::<Vec<_>>(),
            );
        }

        // (obj_addr, klass_ptr, activity_score, (fields))
        let mut candidates: Vec<(usize, usize, i64, [i32; 7])> = Vec::new();
        for (start, end) in heap_regions {
            let size = end - start;
            let buf = reader.read_bytes(start, size);
            if buf.len() != size {
                continue;
            }
            let mut i = 0usize;
            while i + min_obj_size <= buf.len() {
                let klass = u64::from_le_bytes(
                    buf[i..i + 8].try_into().unwrap_or([0; 8]),
                ) as usize;
                if !class_set.contains(&klass) {
                    i += 8;
                    continue;
                }
                let read_i32_at = |field_off: usize| -> i32 {
                    let s = i + field_off;
                    i32::from_le_bytes(buf[s..s + 4].try_into().unwrap_or([0; 4]))
                };
                let wc_common = read_i32_at(offsets.wc_common);
                let wc_uncommon = read_i32_at(offsets.wc_uncommon);
                let wc_rare = read_i32_at(offsets.wc_rare);
                let wc_mythic = read_i32_at(offsets.wc_mythic);
                let gold = read_i32_at(offsets.gold);
                let gems = read_i32_at(offsets.gems);
                let vault = read_i32_at(offsets.vault_progress);
                if !inventory_fields_look_plausible(
                    wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault,
                ) {
                    i += 8;
                    continue;
                }
                let score = inventory_activity_score(
                    wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault,
                );
                candidates.push((
                    start + i,
                    klass,
                    score,
                    [wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault],
                ));
                i += 8;
            }
        }

        candidates.sort_by_key(|(_, _, s, _)| std::cmp::Reverse(*s));
        if debug {
            eprintln!(
                "scan_heap_for_client_player_inventory: {} candidates passed (klass-set + plausibility)",
                candidates.len(),
            );
            for (addr, klass, score, fields) in candidates.iter().take(20) {
                eprintln!(
                    "  0x{:x} klass=0x{:x} score={} wc=[{},{},{},{}] gold={} gems={} vault={}",
                    addr, klass, score,
                    fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], fields[6],
                );
            }
        }
        candidates.first().map(|(addr, _, _, _)| *addr)
    }

    /// Public entry point for the inventory reader. Returns wildcard
    /// counts plus gold / gems / vault progress for the currently
    /// logged-in Arena player. All values come from a live memory
    /// read of the `ClientPlayerInventory` singleton — no Arena log
    /// tailing, no Untapped CSV, no network.
    ///
    /// `vault_progress` is read as an `f64` from offset 0x30 and
    /// holds the percentage directly (e.g. `58.9` for "Vault: 58.9%"
    /// in Arena's UI). Don't multiply or divide it — the stored
    /// value matches the UI exactly.
    pub fn read_mtga_inventory_impl(
        process_name: &str,
    ) -> Result<(i32, i32, i32, i32, i32, i32, f64)> {
        // Returns (wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault_progress)
        let pid = find_pid_by_name(process_name)
            .ok_or_else(|| Error::from_reason(format!("Process '{}' not found", process_name)))?;
        let reader = MemReader::new(pid);

        let cpi_classes = find_all_classes_by_name(&reader, pid, "ClientPlayerInventory");
        if cpi_classes.is_empty() {
            return Err(Error::from_reason(
                "ClientPlayerInventory class not found via direct __DATA scan. \
                 Either MTGA isn't fully loaded or the class has been renamed.",
            ));
        }
        // Pick the first class for field-offset resolution. All
        // variants share the same logical layout so any of them
        // works for metadata lookup.
        let cpi_class = cpi_classes[0];

        let debug = std::env::var("MTGA_DEBUG_INVENTORY").is_ok();
        if debug {
            eprintln!(
                "read_mtga_inventory_impl: found {} ClientPlayerInventory class variants: {:?}",
                cpi_classes.len(),
                cpi_classes.iter().map(|p| format!("0x{:x}", p)).collect::<Vec<_>>(),
            );
        }

        let fields = get_class_fields(&reader, cpi_class);
        if debug {
            eprintln!(
                "read_mtga_inventory_impl: ClientPlayerInventory has {} fields:",
                fields.len(),
            );
            for f in &fields {
                eprintln!(
                    "  {:?} @ 0x{:x} (type: {}, static: {})",
                    f.name, f.offset, f.type_name, f.is_static,
                );
            }
        }
        let offsets = resolve_inventory_field_offsets(&fields).ok_or_else(|| {
            // Dump the field list unconditionally on failure so the
            // user can see what's actually present.
            eprintln!(
                "resolve_inventory_field_offsets: failed to find all required fields. Available non-static fields:",
            );
            for f in &fields {
                if !f.is_static {
                    eprintln!("  {:?} @ 0x{:x} (type: {})", f.name, f.offset, f.type_name);
                }
            }
            Error::from_reason(
                "Could not resolve ClientPlayerInventory field offsets. Required \
                 fields: wcCommon, wcUncommon, wcRare, wcMythic, gold, gems, \
                 vaultProgress. See stderr for the field dump.",
            )
        })?;
        if debug {
            eprintln!(
                "read_mtga_inventory_impl: resolved offsets wcCommon=0x{:x} wcUncommon=0x{:x} wcRare=0x{:x} wcMythic=0x{:x} gold=0x{:x} gems=0x{:x} vaultProgress=0x{:x}",
                offsets.wc_common, offsets.wc_uncommon, offsets.wc_rare, offsets.wc_mythic,
                offsets.gold, offsets.gems, offsets.vault_progress,
            );
        }

        let inst = match scan_heap_for_client_player_inventory(
            &reader, pid, &offsets, &cpi_classes,
        ) {
            Some(addr) => addr,
            None => {
                // Diagnostic cascade to help pinpoint why we're not
                // finding an instance:
                //
                // 1. How many times does the class pointer appear in
                //    heap regions at all? Zero → real instance isn't
                //    in a region `find_scannable_heap_regions`
                //    returns. Nonzero but the plausibility filter
                //    dropped them → offsets are wrong.
                // 2. What OTHER classes in the process contain
                //    "Inventory" in their name? Maybe Arena renamed
                //    `ClientPlayerInventory` or wraps it in
                //    something else.
                for cpi_class in &cpi_classes {
                    let (count, sample) =
                        count_pointer_occurrences_in_heap(&reader, pid, *cpi_class);
                    eprintln!(
                        "diagnostic: cpi_class 0x{:x} appears {} times in scannable heap regions; first {} at: {:?}",
                        cpi_class,
                        count,
                        sample.len(),
                        sample.iter().map(|a| format!("0x{:x}", a)).collect::<Vec<_>>(),
                    );
                    // Dump field values at each sampled address so
                    // we can see why the plausibility filter rejects
                    // them. The "object" interpretation starts at
                    // the address where the class pointer was found.
                    for (idx, addr) in sample.iter().enumerate() {
                        let wc_common = reader.read_i32(addr + offsets.wc_common);
                        let wc_uncommon = reader.read_i32(addr + offsets.wc_uncommon);
                        let wc_rare = reader.read_i32(addr + offsets.wc_rare);
                        let wc_mythic = reader.read_i32(addr + offsets.wc_mythic);
                        let gold = reader.read_i32(addr + offsets.gold);
                        let gems = reader.read_i32(addr + offsets.gems);
                        let vault = reader.read_i32(addr + offsets.vault_progress);
                        eprintln!(
                            "    [{}] 0x{:x} wc=[{},{},{},{}] gold={} gems={} vault={}",
                            idx, addr, wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault,
                        );
                    }
                }
                let inventory_classes =
                    find_classes_by_name_substr(&reader, pid, "Inventory");
                eprintln!(
                    "diagnostic: classes whose name contains \"Inventory\":",
                );
                for (class_ptr, class_name) in &inventory_classes {
                    eprintln!("  0x{:x} {:?}", class_ptr, class_name);
                }
                return Err(Error::from_reason(
                    "ClientPlayerInventory instance not found in heap. See \
                     the diagnostic output above: if the class pointer appears \
                     zero times in heap, the real instance is outside the \
                     scanned regions or wrapped in a different class; if it \
                     appears many times, the field offsets may be wrong.",
                ));
            }
        };

        let wc_common = reader.read_i32(inst + offsets.wc_common);
        let wc_uncommon = reader.read_i32(inst + offsets.wc_uncommon);
        let wc_rare = reader.read_i32(inst + offsets.wc_rare);
        let wc_mythic = reader.read_i32(inst + offsets.wc_mythic);
        let gold = reader.read_i32(inst + offsets.gold);
        let gems = reader.read_i32(inst + offsets.gems);
        // vaultProgress is an 8-byte `double` in the C# class
        // layout (field spacing 0x30→0x38 confirms 8 bytes wide),
        // NOT an int32 like the old IL2CPP research summary claimed.
        // The stored value is the UI percentage directly (58.9 in
        // decimal = 0x404d733333333333 as little-endian double).
        let vault_progress = reader.read_f64(inst + offsets.vault_progress);

        if debug {
            eprintln!(
                "read_mtga_inventory_impl: inst=0x{:x} wc={{C:{}, U:{}, R:{}, M:{}}} gold={} gems={} vault_pct={}",
                inst, wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault_progress,
            );
        }
        Ok((
            wc_common,
            wc_uncommon,
            wc_rare,
            wc_mythic,
            gold,
            gems,
            vault_progress,
        ))
    }

    /// Diagnostic: find the `CardPrintingRecord` class and dump its
    /// fields, then scan the heap for the first few instances and
    /// show what's at each field offset. Used to reverse-engineer
    /// Arena's in-process card database layout so we can build our
    /// own arena_id → card_name lookup table without depending on
    /// Scryfall having populated arena_id values.
    ///
    /// Untapped's companion app doesn't read this dictionary — they
    /// download their own pre-built arena_id → card metadata
    /// mapping from their server. We're reconstructing it from
    /// Arena's memory directly instead, which gives us an
    /// authoritative source that works offline and doesn't lag
    /// behind Scryfall's data ingestion.
    fn probe_card_printing_record(reader: &MemReader, pid: u32) {
        eprintln!("probe_card_printing_record: looking for CardPrintingRecord class...");
        let cpr_class = match find_class_by_direct_scan(reader, pid, "CardPrintingRecord") {
            Some(addr) => addr,
            None => {
                eprintln!("probe_card_printing_record: CardPrintingRecord class not found — bail");
                return;
            }
        };
        eprintln!("probe_card_printing_record: class = 0x{:x}", cpr_class);

        let fields = get_class_fields(reader, cpr_class);
        eprintln!("probe_card_printing_record: {} fields:", fields.len());
        for (i, f) in fields.iter().enumerate() {
            eprintln!(
                "  field[{}] name={:?} type={:?} offset=0x{:x} is_static={}",
                i, f.name, f.type_name, f.offset, f.is_static,
            );
        }

        // Scan heap for instances: objects whose first 8 bytes equal
        // cpr_class. For each of the first few hits, dump 256 bytes
        // of the instance so we can see the raw field values.
        eprintln!(
            "probe_card_printing_record: scanning heap for instances (first 5)...",
        );
        let heap_regions = find_scannable_heap_regions(pid);
        let mut hits = 0usize;
        for (start, end) in heap_regions {
            if hits >= 5 {
                break;
            }
            let size = end - start;
            let buf = reader.read_bytes(start, size);
            if buf.len() != size {
                continue;
            }
            let mut i = 0;
            while i + 256 <= buf.len() {
                let ptr = u64::from_le_bytes(buf[i..i + 8].try_into().unwrap_or([0; 8])) as usize;
                if ptr == cpr_class {
                    let obj_addr = start + i;
                    eprintln!("  instance at 0x{:x}:", obj_addr);
                    // Dump first 20 int32 slots and 20 pointer slots
                    let mut i32s: Vec<String> = Vec::new();
                    let mut ptrs: Vec<String> = Vec::new();
                    for k in 0..20 {
                        let i32_off = k * 4;
                        if i + i32_off + 4 <= buf.len() {
                            let v = i32::from_le_bytes(
                                buf[i + i32_off..i + i32_off + 4].try_into().unwrap_or([0; 4]),
                            );
                            i32s.push(format!("+{:02x}:{}", i32_off, v));
                        }
                        let ptr_off = k * 8;
                        if i + ptr_off + 8 <= buf.len() {
                            let p = u64::from_le_bytes(
                                buf[i + ptr_off..i + ptr_off + 8].try_into().unwrap_or([0; 8]),
                            ) as usize;
                            ptrs.push(format!("+{:02x}:0x{:x}", ptr_off, p));
                        }
                    }
                    eprintln!("    i32s: {}", i32s.join(" "));
                    eprintln!("    ptrs: {}", ptrs.join(" "));
                    // For each field in the metadata, try to read
                    // its value from the instance and display.
                    eprintln!("    field values (from metadata offsets):");
                    for f in fields.iter().take(30) {
                        if f.is_static {
                            continue;
                        }
                        let field_off = f.offset as usize;
                        if i + field_off + 8 > buf.len() {
                            continue;
                        }
                        let as_int = i32::from_le_bytes(
                            buf[i + field_off..i + field_off + 4].try_into().unwrap_or([0; 4]),
                        );
                        let as_ptr = u64::from_le_bytes(
                            buf[i + field_off..i + field_off + 8].try_into().unwrap_or([0; 8]),
                        ) as usize;
                        let as_string = if as_ptr >= 0x100000 && as_ptr < 0x400000000 {
                            reader.read_string(as_ptr)
                        } else {
                            String::new()
                        };
                        let string_display = if as_string.is_empty() || as_string.len() > 40 {
                            String::new()
                        } else {
                            format!(" str={:?}", as_string)
                        };
                        eprintln!(
                            "      {}: int={} ptr=0x{:x}{}",
                            f.name, as_int, as_ptr, string_display,
                        );
                    }
                    hits += 1;
                    if hits >= 5 {
                        break;
                    }
                    i += 8;
                    continue;
                }
                i += 8;
            }
        }
        eprintln!("probe_card_printing_record: {} instances examined", hits);
    }

    /// Public entry point for the signature-based card collection
    /// reader. Bypasses the entire PAPA/WrapperController/InventoryManager
    /// walker. Called from `read_mtga_cards` below.
    pub fn read_mtga_cards_impl(process_name: &str) -> Result<Vec<(i32, i32)>> {
        let pid = find_pid_by_name(process_name)
            .ok_or_else(|| Error::from_reason(format!("Process '{}' not found", process_name)))?;
        let reader = MemReader::new(pid);

        // Diagnostic byte-pattern scan — find every location in heap
        // where an int equals the int 8 bytes later AND equals a
        // specific target arena_id. This confirms whether the
        // (cardId, quantity) pair we're looking for actually exists
        // in Arena's memory at all, independent of the dict-header
        // signature scan below. If the target key is set via
        // MTGA_PROBE_CARD_ID, run it before the normal scan so we
        // see the diagnostic output regardless of what the scan
        // ultimately returns.
        if let Ok(probe_str) = std::env::var("MTGA_PROBE_CARD_ID") {
            if let Ok(target_key) = probe_str.trim().parse::<i32>() {
                scan_for_dict_entry_pattern(&reader, pid, Some(target_key), None, 50);
            }
        }

        // Diagnostic: probe CardPrintingRecord class and dump its
        // fields so we can figure out how to map grp_id → card name
        // directly from Arena's in-process card database. Gated
        // behind MTGA_PROBE_CARD_DB env var so it doesn't always run.
        if std::env::var("MTGA_PROBE_CARD_DB").is_ok() {
            probe_card_printing_record(&reader, pid);
        }

        let dict_addr = scan_heap_for_cards_dictionary(&reader, pid);
        if dict_addr == 0 {
            return Err(Error::from_reason(
                "Cards dictionary not found via heap signature scan. \
                 Either the MTGA player is not logged in yet, the card \
                 collection is empty, or the Dictionary<int,int> layout \
                 has changed in a way the scanner doesn't recognize.",
            ));
        }
        let entries = read_cards_dictionary_entries(&reader, dict_addr);
        if entries.is_empty() {
            return Err(Error::from_reason(format!(
                "Found Cards dictionary at 0x{:x} but it had no valid entries. \
                 This usually means the collection is still loading.",
                dict_addr,
            )));
        }
        eprintln!(
            "read_mtga_cards_impl: extracted {} cards from dictionary at 0x{:x}",
            entries.len(), dict_addr,
        );
        Ok(entries)
    }

    fn find_wrapper_controller_instance(
        reader: &MemReader,
        pid: u32,
        wrapper_controller_class: usize,
    ) -> Option<usize> {
        // Strategy 1: read the static <Instance>k__BackingField.
        // Only trust it if the returned pointer dereferences to an
        // object whose class is WrapperController — the static field
        // parser has been observed returning stale / shared values
        // (offset 0 of PAPA and WrapperController both report the
        // same static_base, which can't be right).
        let inst = read_static_object_field(
            reader,
            wrapper_controller_class,
            "<Instance>k__BackingField",
        );
        if inst >= 0x100000 {
            let obj_class = reader.read_ptr(inst);
            if obj_class == wrapper_controller_class {
                eprintln!(
                    "find_wrapper_controller_instance: static <Instance> = 0x{:x} verified (obj->class matches)",
                    inst,
                );
                return Some(inst);
            }
            eprintln!(
                "find_wrapper_controller_instance: static <Instance> = 0x{:x} but obj->class = 0x{:x} != 0x{:x}, rejecting",
                inst, obj_class, wrapper_controller_class,
            );
        }

        // Strategy 2: heap scan + cross-verified field walk. Scan for
        // any object whose first 8 bytes equal the WrapperController
        // class pointer, then verify by reading its
        // `<InventoryManager>k__BackingField` field and checking that
        // the referenced object's class pointer equals the
        // InventoryManager class. A false positive would have to hit
        // BOTH conditions simultaneously, which is astronomically
        // unlikely for non-instance heap data.
        eprintln!(
            "find_wrapper_controller_instance: static read failed, scanning heap with field verification for instances of class 0x{:x}",
            wrapper_controller_class,
        );

        // Cross-verify by CLASS NAME rather than class pointer. In
        // IL2CPP there can be multiple `Il2CppClass*` variants for
        // the same logical type — a static metadata entry in __DATA
        // and a runtime class struct that actual heap instances
        // reference. These have different addresses but both have a
        // CLASS_NAME offset pointing to the same string literal.
        // Comparing by name is the robust check.
        let im_field_offset = get_class_fields(reader, wrapper_controller_class)
            .iter()
            .find(|f| f.name == "<InventoryManager>k__BackingField")
            .map(|f| f.offset as usize);
        if im_field_offset.is_none() {
            eprintln!("find_wrapper_controller_instance: WrapperController has no <InventoryManager>k__BackingField field!");
        }
        eprintln!(
            "find_wrapper_controller_instance: im_field_offset = {:?}",
            im_field_offset.map(|v| format!("0x{:x}", v)),
        );

        let heap_regions = find_scannable_heap_regions(pid);
        let mut total_raw_matches = 0usize;
        let mut first_raw_match: Option<usize> = None;
        let mut sample_raw_matches: Vec<(usize, usize, usize, usize, String)> = Vec::new(); // (addr, +0, im_ptr, im_ptr_class, im_ptr_class_name)

        for (start, end) in heap_regions {
            let step = 0x100000;
            let mut chunk_start = start;
            while chunk_start < end {
                let chunk_size = step.min(end - chunk_start);
                let bytes = reader.read_bytes(chunk_start, chunk_size);
                if bytes.is_empty() || bytes.iter().all(|&b| b == 0) {
                    chunk_start += chunk_size;
                    continue;
                }
                let mut i = 0;
                while i + 16 <= bytes.len() {
                    let ptr = usize::from_le_bytes(bytes[i..i + 8].try_into().unwrap_or([0; 8]));
                    if ptr == wrapper_controller_class {
                        let obj_addr = chunk_start + i;
                        total_raw_matches += 1;
                        if first_raw_match.is_none() {
                            first_raw_match = Some(obj_addr);
                        }

                        if let Some(im_off) = im_field_offset {
                            let im_ptr = reader.read_ptr(obj_addr + im_off);
                            if im_ptr > 0x100000 && im_ptr < 0x400_000_000 {
                                let im_ptr_class = reader.read_ptr(im_ptr);
                                let im_ptr_class_name = if im_ptr_class > 0x100000 {
                                    read_class_name(reader, im_ptr_class)
                                } else {
                                    String::new()
                                };
                                if sample_raw_matches.len() < 10 {
                                    sample_raw_matches.push((
                                        obj_addr,
                                        ptr,
                                        im_ptr,
                                        im_ptr_class,
                                        im_ptr_class_name.clone(),
                                    ));
                                }
                                if im_ptr_class_name == "InventoryManager" {
                                    eprintln!(
                                        "find_wrapper_controller_instance: VERIFIED by class name at 0x{:x} (im_ptr=0x{:x}, im_ptr_class=0x{:x})",
                                        obj_addr, im_ptr, im_ptr_class,
                                    );
                                    return Some(obj_addr);
                                }
                            } else if sample_raw_matches.len() < 10 {
                                sample_raw_matches.push((
                                    obj_addr,
                                    ptr,
                                    im_ptr,
                                    0,
                                    String::new(),
                                ));
                            }
                        }
                    }
                    i += 8;
                }
                chunk_start += chunk_size;
            }
        }

        eprintln!(
            "find_wrapper_controller_instance: {} raw matches scanned, none verified by class name.",
            total_raw_matches,
        );
        if !sample_raw_matches.is_empty() {
            eprintln!("find_wrapper_controller_instance: first 10 samples (obj_addr, +0, im_ptr, im_ptr_class, im_ptr_class_name):");
            for (a, v0, imp, imc, imn) in &sample_raw_matches {
                eprintln!(
                    "  0x{:x}  +0=0x{:x}  im_ptr=0x{:x}  im_ptr_class=0x{:x}  name={:?}",
                    a, v0, imp, imc, imn,
                );
            }
        }
        None
    }

    fn find_papa_instance_by_field_verification(
        reader: &MemReader,
        pid: u32,
        papa_class: usize,
    ) -> Option<usize> {
        let im_class = find_class_by_direct_scan(reader, pid, "InventoryManager")?;
        eprintln!(
            "find_papa_instance_by_field_verification: InventoryManager class = 0x{:x}",
            im_class,
        );

        let papa_fields = get_class_fields(reader, papa_class);
        let im_field_offset = papa_fields
            .iter()
            .find(|f| f.name == "<InventoryManager>k__BackingField")
            .map(|f| f.offset as usize)?;
        eprintln!(
            "find_papa_instance_by_field_verification: PAPA.<InventoryManager>k__BackingField at offset 0x{:x}",
            im_field_offset,
        );

        let heap_regions = find_scannable_heap_regions(pid);
        eprintln!(
            "find_papa_instance_by_field_verification: scanning {} heap regions",
            heap_regions.len(),
        );

        let mut total_papa_matches = 0usize;
        let mut verified_matches: Vec<usize> = Vec::new();

        for (start, end) in heap_regions {
            let step = 0x100000;
            let mut chunk_start = start;
            while chunk_start < end {
                let chunk_size = step.min(end - chunk_start);
                let bytes = reader.read_bytes(chunk_start, chunk_size);
                if bytes.is_empty() || bytes.iter().all(|&b| b == 0) {
                    chunk_start += chunk_size;
                    continue;
                }

                let mut i = 0;
                while i + 8 <= bytes.len() {
                    let ptr = usize::from_le_bytes(bytes[i..i + 8].try_into().unwrap_or([0; 8]));
                    if ptr == papa_class {
                        total_papa_matches += 1;
                        let obj_addr = chunk_start + i;
                        let im_ptr = reader.read_ptr(obj_addr + im_field_offset);
                        if im_ptr > 0x100000 && im_ptr < 0x400000000 {
                            let im_obj_class = reader.read_ptr(im_ptr);
                            if im_obj_class == im_class {
                                verified_matches.push(obj_addr);
                                if verified_matches.len() >= 5 {
                                    break;
                                }
                            }
                        }
                    }
                    i += 8;
                }
                if verified_matches.len() >= 5 {
                    break;
                }
                chunk_start += chunk_size;
            }
            if verified_matches.len() >= 5 {
                break;
            }
        }

        eprintln!(
            "find_papa_instance_by_field_verification: papa_class matched in {} slots, {} verified as real PAPA instances",
            total_papa_matches, verified_matches.len(),
        );
        if !verified_matches.is_empty() {
            eprintln!(
                "find_papa_instance_by_field_verification: verified PAPA instances: {:?}",
                verified_matches
                    .iter()
                    .map(|a| format!("0x{:x}", a))
                    .collect::<Vec<_>>(),
            );
            return Some(verified_matches[0]);
        }
        None
    }

    fn find_papa_instance(reader: &MemReader, pid: u32, papa_class: usize) -> Option<usize> {
        // Strategy 1: field-verified heap scan. Cross-checks the
        // candidate's class pointer AND its InventoryManager backing
        // field. Astronomically unlikely to false-positive.
        if let Some(inst) = find_papa_instance_by_field_verification(reader, pid, papa_class) {
            eprintln!(
                "find_papa_instance: using field-verified instance = 0x{:x}",
                inst,
            );
            return Some(inst);
        }

        // Strategy 2: static-field lookup. Only works if PAPA exposes
        // its singleton via a conventional C# `static Instance` — on
        // current MTGA builds the `_instance` static field reads as
        // null even though Arena is fully initialized, so this rarely
        // helps, but it stays as a fallback for older/older MTGA
        // versions.
        if let Some(inst) = find_papa_instance_via_static_field(reader, papa_class) {
            eprintln!(
                "find_papa_instance: using static-field instance = 0x{:x}",
                inst,
            );
            return Some(inst);
        }

        // Strategy 3 (last resort): upstream's hardcoded-heap-range
        // scan with its brittle `+16` / `+224` verification. This
        // path is effectively dead on current Arena builds — the
        // heap ranges are wrong AND the field offsets are wrong — but
        // it's kept so we have a clear error path if the first two
        // strategies both fail.
        //
        // Local patch — upstream used three hardcoded heap ranges
        // (`0x15a000000..0x15b000000`, `0x158000000..0x16a000000`,
        // `0x145000000..0x150000000`) that happened to be where the
        // PAPA instance lived on whatever macOS build was tested.
        // macOS arm64 heap addresses drift between OS versions and
        // even between Arena restarts, so hardcoded ranges rot.
        // Instead, enumerate writable (`rw-`) regions of reasonable
        // size from `vmmap` and scan each of them.
        let heap_regions = find_scannable_heap_regions(pid);
        eprintln!(
            "find_papa_instance: scanning {} heap regions for papa_class=0x{:x}",
            heap_regions.len(), papa_class,
        );

        // Diagnostics: count total slots where `ptr == papa_class`.
        // If that count is zero, either our papa_class pointer is
        // wrong or the scannable heap regions miss the GC heap.
        // If the count is non-zero but verification fails every time,
        // the +16 / +224 object layout offsets have drifted.
        let mut total_matches: usize = 0;
        let mut sample_matches: Vec<(usize, usize, usize, String)> = Vec::new();

        for (start, end) in heap_regions {
            let step = 0x100000;
            let mut chunk_start = start;
            while chunk_start < end {
                let chunk_size = step.min(end - chunk_start);
                let bytes = reader.read_bytes(chunk_start, chunk_size);
                if bytes.is_empty() || bytes.iter().all(|&b| b == 0) {
                    chunk_start += chunk_size;
                    continue;
                }

                let mut i = 0;
                while i + 8 <= bytes.len() {
                    let ptr = usize::from_le_bytes(bytes[i..i + 8].try_into().unwrap_or([0; 8]));
                    if ptr == papa_class {
                        let obj_addr = chunk_start + i;
                        total_matches += 1;
                        let val_at_16 = reader.read_ptr(obj_addr + 16);
                        let inv_mgr_224 = reader.read_ptr(obj_addr + 224);
                        if sample_matches.len() < 10 {
                            let inv_mgr_class = if inv_mgr_224 > 0x100000 {
                                reader.read_ptr(inv_mgr_224)
                            } else {
                                0
                            };
                            let inv_name = if inv_mgr_class > 0x100000 {
                                read_class_name(reader, inv_mgr_class)
                            } else {
                                String::new()
                            };
                            sample_matches.push((obj_addr, val_at_16, inv_mgr_224, inv_name));
                        }

                        // Upstream verification: +16 looks like a
                        // non-self pointer, +224 points to something
                        // whose class name contains "InventoryManager".
                        if val_at_16 != papa_class && val_at_16 > 0x100000 {
                            if inv_mgr_224 > 0x100000 && inv_mgr_224 < 0x400000000 {
                                let inv_class = reader.read_ptr(inv_mgr_224);
                                let inv_name = read_class_name(reader, inv_class);
                                if inv_name.contains("InventoryManager") {
                                    eprintln!(
                                        "find_papa_instance: FOUND (strict) at 0x{:x} after {} match(es)",
                                        obj_addr, total_matches,
                                    );
                                    return Some(obj_addr);
                                }
                            }
                        }
                    }
                    i += 8;
                }
                chunk_start += chunk_size;
            }
        }
        eprintln!(
            "find_papa_instance: total slots matching papa_class = {}. Strict InventoryManager check did not pass for any of them.",
            total_matches,
        );
        if !sample_matches.is_empty() {
            eprintln!("find_papa_instance: first {} matches (obj_addr, +16 ptr, +224 ptr, class_name_at_+224):", sample_matches.len());
            for (a, v16, v224, name) in &sample_matches {
                eprintln!("  0x{:x}  +16=0x{:x}  +224=0x{:x}  name={:?}", a, v16, v224, name);
            }
            // Loose fallback: if we found a match where +16 is a
            // valid non-self pointer (i.e., it looks like a typical
            // Il2CppObject header), return the first one. This is
            // less certain than the InventoryManager-verified hit
            // but lets downstream code try the field walk anyway
            // and produce a more actionable error if field offsets
            // have drifted.
            for (obj_addr, val_at_16, _v224, _name) in &sample_matches {
                if *val_at_16 != papa_class && *val_at_16 > 0x100000 {
                    eprintln!(
                        "find_papa_instance: using LOOSE fallback at 0x{:x} (first match with plausible +16 header)",
                        obj_addr,
                    );
                    return Some(*obj_addr);
                }
            }
        }
        None
    }

    /// Parse vmmap output for writable, reasonably-sized VM regions
    /// that a Unity GC-managed heap might live in. Returns
    /// `(start, end)` pairs sorted by address, filtered to exclude
    /// the GameAssembly dylib's own segments (which we already know
    /// are code and static metadata, not C# object instances) and
    /// any region smaller than 1MB (too small to hold the managed
    /// heap) or larger than 4GB (to avoid reading the entire VM if
    /// vmmap reports some weird very-large mapping).
    fn find_scannable_heap_regions(pid: u32) -> Vec<(usize, usize)> {
        let output = Command::new("vmmap")
            .args(["-wide", &pid.to_string()])
            .output()
            .ok();

        let mut result: Vec<(usize, usize)> = Vec::new();
        const MIN_SIZE: usize = 1 << 20; // 1 MB
        const MAX_SIZE: usize = 4usize << 30; // 4 GB

        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                // Skip obvious non-heap regions. We want the IL2CPP
                // GC heap (Boehm) and the managed heap Unity allocates
                // for C# objects — both are `rw-` `SM=PRV` or
                // `SM=ZER` mappings in the anonymous-mapping range.
                // We exclude the GameAssembly dylib segments because
                // the PAPA instance is a heap-allocated C# object
                // whose pointer lives in GC-managed memory, not in
                // the dylib.
                if line.contains("GameAssembly") {
                    continue;
                }
                // Only rw- regions can hold mutable C# object data.
                if !line.contains("rw-") {
                    continue;
                }
                // Parse "0xstart-0xend" or "start-end" from the
                // second whitespace-separated column. vmmap lines
                // look like:
                //   "MALLOC_LARGE  142000000-142100000  [  1024K  ...] rw-/rwx SM=PRV"
                let parts: Vec<&str> = line.split_whitespace().collect();
                let addr_field_idx = parts.iter().position(|p| p.contains('-') && p.split('-').count() == 2 && p.chars().next().map_or(false, |c| c.is_ascii_hexdigit()));
                let idx = match addr_field_idx {
                    Some(i) => i,
                    None => continue,
                };
                let addr_parts: Vec<&str> = parts[idx].split('-').collect();
                if addr_parts.len() != 2 {
                    continue;
                }
                let start = match usize::from_str_radix(addr_parts[0], 16) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let end = match usize::from_str_radix(addr_parts[1], 16) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if end <= start {
                    continue;
                }
                let size = end - start;
                if size < MIN_SIZE || size > MAX_SIZE {
                    continue;
                }
                result.push((start, end));
            }
        }
        result.sort();
        // De-dup overlapping regions.
        result.dedup();
        result
    }

    pub fn is_admin_impl() -> bool {
        unsafe { libc::geteuid() == 0 }
    }

    pub fn find_process_impl(process_name: &str) -> bool {
        find_pid_by_name(process_name).is_some()
    }

    fn find_pid_by_name(process_name: &str) -> Option<u32> {
        let output = Command::new("pgrep")
            .arg(process_name)
            .output()
            .ok()?;

        String::from_utf8_lossy(&output.stdout)
            .trim()
            .lines()
            .next()
            .and_then(|s| s.parse().ok())
    }

    pub fn init_impl(process_name: &str) -> Result<bool> {
        let pid = find_pid_by_name(process_name)
            .ok_or_else(|| Error::from_reason("Process not found"))?;

        let reader = MemReader::new(pid);

        // Sanity-check that vmmap can see GameAssembly at all. The
        // returned segment address isn't used for the table walk
        // anymore — we scan directly for class pointers — but if it's
        // 0, neither the direct scan nor the table scan has any data
        // to work with.
        let data_base = find_second_data_segment(pid);
        eprintln!("init_impl: vmmap data_base = 0x{:x}", data_base);
        if data_base == 0 {
            return Err(Error::from_reason("Could not find GameAssembly __DATA segment"));
        }

        // Find PAPA by scanning __DATA for pointers that dereference
        // to a class named "PAPA". This bypasses the fragile
        // "find the type info table" step — IL2CPP has many
        // sub-tables that all pass rich-name heuristics, and picking
        // the "right" one is version-dependent. Direct scan doesn't
        // care which table the class pointer lives in.
        let papa_class = find_class_by_direct_scan(&reader, pid, "PAPA")
            .ok_or_else(|| Error::from_reason(
                "PAPA class not found via direct __DATA scan. Either the \
                 top-level singleton has been renamed in this MTGA version, \
                 or GameAssembly's __DATA segments are structured differently \
                 than expected. Check mtga-reader debug output above.",
            ))?;
        eprintln!("init_impl: direct-scan papa_class = 0x{:x}", papa_class);

        // Find WrapperController class — present on both macOS and
        // Windows Arena builds. On Windows this is the singleton
        // entry point that holds `<Instance>k__BackingField` →
        // `<InventoryManager>k__BackingField` → ... path. On macOS
        // the upstream code preferred PAPA, but PAPA's heap layout
        // and static-field state are hostile (see
        // find_papa_instance_by_field_verification and
        // find_papa_instance_via_static_field both failing).
        // WrapperController is worth trying as an alternative root.
        let wrapper_controller_class = find_class_by_direct_scan(&reader, pid, "WrapperController");
        eprintln!(
            "init_impl: WrapperController class = {}",
            wrapper_controller_class.map(|v| format!("0x{:x}", v)).unwrap_or_else(|| "not found".to_string()),
        );

        // type_info_table is kept in state for API compatibility with
        // the rest of the module (some read paths still reference it).
        // Use the best table we can find, but don't fail init if we
        // can't — the direct-scanned papa_class is enough for readData.
        let type_info_table = scan_for_type_info_table(&reader, pid);
        eprintln!("init_impl: scan_for_type_info_table result = 0x{:x}", type_info_table);

        // Try WrapperController first — it's the Windows-proven
        // singleton entry point and exists on macOS too. If we get a
        // real instance this way, we store it as `papa_instance`
        // (misnomer kept for API compat) and the walker starts
        // from WrapperController instead of PAPA.
        let wrapper_instance_opt = wrapper_controller_class.and_then(|wc_class| {
            find_wrapper_controller_instance(&reader, pid, wc_class)
        });
        if let Some(wc_inst) = wrapper_instance_opt {
            eprintln!(
                "init_impl: using WrapperController instance 0x{:x} as papa_instance",
                wc_inst,
            );
            let mut wrapper = STATE.lock().map_err(|_| Error::from_reason("Failed to lock state"))?;
            wrapper.0 = Some(Il2CppState {
                reader,
                pid,
                type_info_table,
                papa_class,
                papa_instance: wc_inst,
            });
            return Ok(true);
        }

        let papa_instance = find_papa_instance(&reader, pid, papa_class).unwrap_or(0);

        let mut wrapper = STATE.lock().map_err(|_| Error::from_reason("Failed to lock state"))?;
        wrapper.0 = Some(Il2CppState {
            reader,
            pid,
            type_info_table,
            papa_class,
            papa_instance,
        });

        Ok(true)
    }

    pub fn close_impl() -> Result<bool> {
        let mut wrapper = STATE.lock().map_err(|_| Error::from_reason("Failed to lock state"))?;
        wrapper.0 = None;
        Ok(true)
    }

    pub fn is_initialized_impl() -> bool {
        if let Ok(wrapper) = STATE.lock() {
            wrapper.0.is_some()
        } else {
            false
        }
    }

    fn with_state<F, T>(f: F) -> Result<T>
    where
        F: FnOnce(&Il2CppState) -> Result<T>,
    {
        let wrapper = STATE.lock().map_err(|_| Error::from_reason("Failed to lock state"))?;
        let state = wrapper.0
            .as_ref()
            .ok_or_else(|| Error::from_reason("Reader not initialized. Call init() first."))?;
        f(state)
    }

    pub fn get_assemblies_impl() -> Result<Vec<String>> {
        Ok(vec![
            "GameAssembly".to_string(),
            "MTGA-Classes".to_string(),
        ])
    }

    pub fn get_assembly_classes_impl(_assembly_name: &str) -> Result<Vec<ClassInfo>> {
        with_state(|state| {
            let class_names = vec![
                "PAPA",
                "WrapperController",
                "InventoryManager",
                "AwsInventoryServiceWrapper",
                "CardDatabase",
                "ClientPlayerInventory",
                "CardsAndQuantity",
            ];

            let mut classes = Vec::new();
            for name in class_names {
                if let Some(class_addr) = find_class_by_name(&state.reader, state.type_info_table, name) {
                    let namespace = read_class_namespace(&state.reader, class_addr);
                    classes.push(ClassInfo {
                        name: name.to_string(),
                        namespace,
                        address: class_addr as i64,
                        is_static: false,
                        is_enum: false,
                    });
                }
            }

            Ok(classes)
        })
    }

    pub fn get_class_fields(reader: &MemReader, class_addr: usize) -> Vec<FieldInfo> {
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

            let type_data = reader.read_ptr(type_ptr);
            let type_name = if type_data > 0x100000 {
                let tn = read_class_name(reader, type_data);
                if tn.is_empty() { "Unknown".to_string() } else { tn }
            } else {
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

    pub fn get_class_details_impl(_assembly_name: &str, class_name: &str) -> Result<ClassDetails> {
        with_state(|state| {
            let class_addr = find_class_by_name(&state.reader, state.type_info_table, class_name)
                .ok_or_else(|| Error::from_reason("Class not found"))?;

            let name = read_class_name(&state.reader, class_addr);
            let namespace = read_class_namespace(&state.reader, class_addr);
            let fields = get_class_fields(&state.reader, class_addr);

            let mut static_instances = Vec::new();

            if class_name == "PAPA" && state.papa_instance != 0 {
                static_instances.push(StaticInstanceInfo {
                    field_name: "_instance".to_string(),
                    address: state.papa_instance as i64,
                });
            }

            for field in &fields {
                if field.is_static && (field.name.contains("instance") || field.name.contains("Instance")) {
                    let static_fields = state.reader.read_ptr(class_addr + offsets::CLASS_STATIC_FIELDS);
                    if static_fields > 0x100000 {
                        let ptr = state.reader.read_ptr(static_fields + field.offset as usize);
                        if ptr > 0x100000 && ptr < 0x400000000 {
                            static_instances.push(StaticInstanceInfo {
                                field_name: field.name.clone(),
                                address: ptr as i64,
                            });
                        }
                    }
                }
            }

            Ok(ClassDetails {
                name,
                namespace,
                address: class_addr as i64,
                fields,
                static_instances,
            })
        })
    }

    fn read_field_value(reader: &MemReader, instance_addr: usize, field: &FieldInfo) -> serde_json::Value {
        let field_addr = instance_addr + field.offset as usize;
        let type_name = &field.type_name;

        // Use contains() for more robust type matching
        // Check UInt32 before Int32 since "UInt32" contains "Int32"
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
        if type_name.contains("UInt16") || type_name == "ushort" {
            return serde_json::json!(reader.read_u16(field_addr));
        }
        if type_name.contains("Int16") || type_name == "short" {
            return serde_json::json!(reader.read_i16(field_addr));
        }
        if type_name.contains("Byte") && !type_name.contains("SByte") || type_name == "byte" {
            return serde_json::json!(reader.read_u8(field_addr));
        }
        if type_name.contains("SByte") || type_name == "sbyte" {
            return serde_json::json!(reader.read_i8(field_addr));
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

        // For reference types, read as pointer
        let ptr = reader.read_ptr(field_addr);
        if ptr == 0 {
            return serde_json::Value::Null;
        }

        if ptr > 0x100000 && ptr < 0x400000000 {
            let class_ptr = reader.read_ptr(ptr);
            let class_name = read_class_name(reader, class_ptr);

            return serde_json::json!({
                "type": "pointer",
                "address": ptr,
                "class_name": if class_name.is_empty() { field.type_name.clone() } else { class_name }
            });
        }

        // Fallback
        serde_json::json!(reader.read_i32(field_addr))
    }

    pub fn get_instance_impl(address: i64) -> Result<InstanceData> {
        with_state(|state| {
            let address = address as usize;
            if address == 0 {
                return Err(Error::from_reason("Invalid address"));
            }

            let class_ptr = state.reader.read_ptr(address);
            if class_ptr == 0 || class_ptr < 0x100000 {
                return Err(Error::from_reason("Invalid instance"));
            }

            let class_name = read_class_name(&state.reader, class_ptr);
            let namespace = read_class_namespace(&state.reader, class_ptr);

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

            Ok(InstanceData {
                class_name,
                namespace,
                address: address as i64,
                fields,
            })
        })
    }

    pub fn get_instance_field_impl(address: i64, field_name: &str) -> Result<serde_json::Value> {
        with_state(|state| {
            let instance_addr = address as usize;
            let class_ptr = state.reader.read_ptr(instance_addr);
            if class_ptr == 0 {
                return Err(Error::from_reason("Invalid instance"));
            }

            let fields = get_class_fields(&state.reader, class_ptr);
            let field = fields.iter()
                .find(|f| f.name == field_name)
                .ok_or_else(|| Error::from_reason("Field not found"))?;

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
        })
    }

    pub fn get_static_field_impl(class_address: i64, field_name: &str) -> Result<serde_json::Value> {
        with_state(|state| {
            let class_addr = class_address as usize;

            let fields = get_class_fields(&state.reader, class_addr);
            let field = fields.iter()
                .find(|f| f.name == field_name && f.is_static)
                .ok_or_else(|| Error::from_reason("Static field not found"))?;

            let static_fields = state.reader.read_ptr(class_addr + offsets::CLASS_STATIC_FIELDS);
            if static_fields == 0 {
                return Ok(serde_json::Value::Null);
            }

            let field_addr = static_fields + field.offset as usize;
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
        })
    }

    fn read_dict_entries_il2cpp(reader: &MemReader, entries_ptr: usize, count: i32) -> Result<DictionaryData> {
        let mut entries = Vec::new();
        let max_read = count.min(5000) as usize;

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

        Ok(DictionaryData {
            count: entries.len() as i32,
            entries,
        })
    }

    pub fn get_dictionary_impl(address: i64) -> Result<DictionaryData> {
        with_state(|state| {
            let dict_addr = address as usize;
            if dict_addr == 0 {
                return Err(Error::from_reason("Invalid address"));
            }

            let class_ptr = state.reader.read_ptr(dict_addr);
            let class_name = read_class_name(&state.reader, class_ptr);

            if class_name == "CardsAndQuantity" {
                let entries_ptr = state.reader.read_ptr(dict_addr + 0x18);
                let count = state.reader.read_i32(dict_addr + 0x20);

                if entries_ptr > 0x100000 && count > 0 && count < 100000 {
                    return read_dict_entries_il2cpp(&state.reader, entries_ptr, count);
                }
            }

            let entries_ptr = state.reader.read_ptr(dict_addr + 0x18);
            let count = state.reader.read_i32(dict_addr + 0x20);

            if entries_ptr > 0x100000 && count > 0 && count < 100000 {
                return read_dict_entries_il2cpp(&state.reader, entries_ptr, count);
            }

            Err(Error::from_reason("Could not read dictionary"))
        })
    }

    pub fn read_data_impl(process_name: &str, fields: Vec<String>) -> serde_json::Value {
        let result = (|| -> Result<serde_json::Value> {
            if fields.is_empty() {
                return Err(Error::from_reason("No path specified"));
            }

            if !is_initialized_impl() {
                init_impl(process_name)?;
            }

            let wrapper = STATE.lock().map_err(|_| Error::from_reason("Failed to lock state"))?;
            let state = wrapper.0.as_ref()
                .ok_or_else(|| Error::from_reason("Reader not initialized"))?;

            let root_name = &fields[0];
            if root_name != "PAPA" && root_name != "WrapperController" {
                return Err(Error::from_reason(format!("Root class '{}' not supported on macOS IL2CPP. Use 'PAPA' or 'WrapperController'.", root_name)));
            }

            if state.papa_instance == 0 {
                return Err(Error::from_reason("PAPA instance not found"));
            }

            let mut current_addr = state.papa_instance;
            let mut current_class = state.reader.read_ptr(current_addr);

            for field_name in &fields[1..] {
                let fields_list = get_class_fields(&state.reader, current_class);

                let field = fields_list.iter()
                    .find(|f| f.name == *field_name)
                    .ok_or_else(|| Error::from_reason(format!("Field '{}' not found", field_name)))?;

                let field_addr = current_addr + field.offset as usize;
                let ptr = state.reader.read_ptr(field_addr);

                if ptr == 0 {
                    return Ok(serde_json::Value::Null);
                }

                current_addr = ptr;
                current_class = state.reader.read_ptr(current_addr);
            }

            let class_name = read_class_name(&state.reader, current_class);

            if class_name == "CardsAndQuantity" || class_name.contains("Dictionary") {
                let entries_ptr = state.reader.read_ptr(current_addr + 0x18);
                let count = state.reader.read_i32(current_addr + 0x20);

                if entries_ptr > 0x100000 && count > 0 {
                    let mut entries = Vec::new();
                    for i in 0..count.min(5000) as usize {
                        let entry_addr = entries_ptr + 0x20 + i * 16;
                        let hash = state.reader.read_i32(entry_addr);
                        let key = state.reader.read_i32(entry_addr + 8);
                        let value = state.reader.read_i32(entry_addr + 12);

                        if hash >= 0 && key > 0 {
                            entries.push(serde_json::json!({
                                "key": key,
                                "value": value
                            }));
                        }
                    }
                    return Ok(serde_json::json!(entries));
                }
            }

            Ok(serde_json::json!({
                "address": current_addr,
                "class": class_name
            }))
        })();

        result.unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
    }

    pub fn read_class_impl(_process_name: &str, address: i64) -> serde_json::Value {
        match get_instance_impl(address) {
            Ok(data) => serde_json::to_value(data).unwrap_or(serde_json::json!({"error": "Serialization failed"})),
            Err(e) => serde_json::json!({ "error": e.to_string() })
        }
    }

    pub fn read_generic_instance_impl(_process_name: &str, address: i64) -> serde_json::Value {
        match get_instance_impl(address) {
            Ok(data) => serde_json::to_value(data).unwrap_or(serde_json::json!({"error": "Serialization failed"})),
            Err(e) => serde_json::json!({ "error": e.to_string() })
        }
    }
}

// ============================================================================
// Public NAPI API
// ============================================================================

#[napi]
pub fn is_admin() -> bool {
    #[cfg(target_os = "windows")]
    { windows_backend::is_admin_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::is_admin_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { false }
}

#[napi]
pub fn find_process(process_name: String) -> bool {
    #[cfg(target_os = "windows")]
    { windows_backend::find_process_impl(&process_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::find_process_impl(&process_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { false }
}

#[napi]
pub fn init(process_name: String) -> Result<bool> {
    #[cfg(target_os = "windows")]
    { windows_backend::init_impl(&process_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::init_impl(&process_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn close() -> Result<bool> {
    #[cfg(target_os = "windows")]
    { windows_backend::close_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::close_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Ok(true) }
}

#[napi]
pub fn is_initialized() -> bool {
    #[cfg(target_os = "windows")]
    { windows_backend::is_initialized_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::is_initialized_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { false }
}

#[napi]
pub fn get_assemblies() -> Result<Vec<String>> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_assemblies_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::get_assemblies_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_assembly_classes(assembly_name: String) -> Result<Vec<ClassInfo>> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_assembly_classes_impl(&assembly_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_assembly_classes_impl(&assembly_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_class_details(assembly_name: String, class_name: String) -> Result<ClassDetails> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_class_details_impl(&assembly_name, &class_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_class_details_impl(&assembly_name, &class_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_instance(address: i64) -> Result<InstanceData> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_instance_impl(address) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_instance_impl(address) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_instance_field(address: i64, field_name: String) -> Result<serde_json::Value> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_instance_field_impl(address, &field_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_instance_field_impl(address, &field_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_static_field(class_address: i64, field_name: String) -> Result<serde_json::Value> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_static_field_impl(class_address, &field_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_static_field_impl(class_address, &field_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_dictionary(address: i64) -> Result<DictionaryData> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_dictionary_impl(address) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_dictionary_impl(address) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn read_data(process_name: String, fields: Vec<String>) -> serde_json::Value {
    #[cfg(target_os = "windows")]
    { windows_backend::read_data_impl(&process_name, fields) }

    #[cfg(target_os = "macos")]
    { macos_backend::read_data_impl(&process_name, fields) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { serde_json::json!({ "error": "Platform not supported" }) }
}

/// Signature-based card-collection reader. Scans the MTGA process
/// heap for a `Dictionary<int, int>` object whose contents match
/// the shape of an Arena player collection (enough entries, keys in
/// the Arena card-id range, values in the quantity range) and
/// returns the list of (cardId, quantity) entries.
///
/// This is a macOS-only path added as a local patch: the
/// `readData` walker starting from PAPA / WrapperController turned
/// out to be too fragile against current Arena builds (IL2CPP
/// metadata layout drift, runtime-class-vs-metadata-class
/// indirection, inconsistent CLASS_NAME offsets on runtime-allocated
/// class structs). The signature scan sidesteps every one of those
/// by searching for the only dictionary in the process whose entries
/// all look like real card entries.
///
/// Returns a JSON array of `{ "cardId": int, "quantity": int }`
/// objects on success, or `{ "error": string }` on any failure.
#[napi]
pub fn read_mtga_cards(process_name: String) -> serde_json::Value {
    #[cfg(target_os = "macos")]
    {
        match macos_backend::read_mtga_cards_impl(&process_name) {
            Ok(entries) => {
                let cards: Vec<serde_json::Value> = entries
                    .into_iter()
                    .map(|(key, value)| serde_json::json!({ "cardId": key, "quantity": value }))
                    .collect();
                serde_json::json!({ "cards": cards })
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = process_name;
        serde_json::json!({ "error": "readMtgaCards is macOS-only in this local fork" })
    }
}

/// Inventory reader. Returns the current player's wildcard counts
/// plus currency and vault progress, read directly from the
/// `ClientPlayerInventory` singleton in Arena's memory.
///
/// Returns `{ wcCommon, wcUncommon, wcRare, wcMythic, gold, gems,
/// vaultProgress }` on success, `{ error }` on failure.
///
/// `vaultProgress` is a number in `0.0 – 100.0` matching Arena's UI
/// exactly (e.g. `58.9` when the UI shows "Vault: 58.9%"). The raw
/// field is stored as an 8-byte `double` in the C# class, not an
/// int — NOTES / IL2CPP_RESEARCH_SUMMARY.md were wrong about this.
///
/// Set `MTGA_DEBUG_INVENTORY=1` for verbose stderr diagnostics (class
/// location, field dump, candidate counts).
#[napi]
pub fn read_mtga_inventory(process_name: String) -> serde_json::Value {
    #[cfg(target_os = "macos")]
    {
        match macos_backend::read_mtga_inventory_impl(&process_name) {
            Ok((wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault_progress)) => {
                serde_json::json!({
                    "wcCommon": wc_common,
                    "wcUncommon": wc_uncommon,
                    "wcRare": wc_rare,
                    "wcMythic": wc_mythic,
                    "gold": gold,
                    "gems": gems,
                    "vaultProgress": vault_progress,
                })
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = process_name;
        serde_json::json!({ "error": "readMtgaInventory is macOS-only in this local fork" })
    }
}


/// Mono-backend card-collection reader. Targets Arena processes running
/// the Mono scripting backend (Windows native or Wine). Pass the process
/// name or path fragment (e.g. "MTGA.exe" for Wine).
#[napi]
pub fn read_mtga_cards_mono(process_name: String) -> serde_json::Value {
    match crate::mono::scanner::read_mtga_cards_mono(&process_name) {
        Ok(entries) => {
            let cards: Vec<serde_json::Value> = entries
                .into_iter()
                .map(|(key, value)| serde_json::json!({ "cardId": key, "quantity": value }))
                .collect();
            serde_json::json!({ "cards": cards })
        }
        Err(e) => serde_json::json!({ "error": e }),
    }
}


/// Mono-backend inventory reader.
/// Pass known_gold and known_gems (visible in Arena's UI) for exact
/// anchoring. Pass 0 for both to use the generic scanner (less reliable).
#[napi]
pub fn read_mtga_inventory_mono(process_name: String, known_gold: i32, known_gems: i32) -> serde_json::Value {
    match crate::mono::scanner::read_mtga_inventory_mono(&process_name, known_gold, known_gems) {
        Ok((wc, wu, wr, wm, gold, gems, vault)) => {
            serde_json::json!({
                "wcCommon": wc,
                "wcUncommon": wu,
                "wcRare": wr,
                "wcMythic": wm,
                "gold": gold,
                "gems": gems,
                "vaultProgress": vault,
            })
        }
        Err(e) => serde_json::json!({ "error": e }),
    }
}

/// Debug: probe a MonoClass struct to find the name field offset.
#[napi]
pub fn probe_mono_class(process_name: String, class_address: String) -> serde_json::Value {
    let addr = u64::from_str_radix(class_address.trim_start_matches("0x"), 16)
        .unwrap_or(0) as usize;
    match crate::mono::scanner::probe_mono_class_name_offset(&process_name, addr) {
        Ok(result) => serde_json::json!({ "result": result }),
        Err(e) => serde_json::json!({ "error": e }),
    }
}

/// Debug: read raw bytes from a Mono Arena process at a given address.
/// Returns hex string. Used for discovering Mono struct layouts.
#[napi]
pub fn read_mono_bytes(process_name: String, address: String, length: i32) -> serde_json::Value {
    let addr = u64::from_str_radix(address.trim_start_matches("0x"), 16)
        .unwrap_or(0) as usize;
    if addr == 0 || length <= 0 {
        return serde_json::json!({ "error": "invalid address or length" });
    }
    match crate::mono::scanner::read_bytes_at(&process_name, addr, length as usize) {
        Ok(hex) => serde_json::json!({ "hex": hex }),
        Err(e) => serde_json::json!({ "error": e }),
    }
}

/// Debug probe: search heap for two adjacent i32 values and dump context.
/// Use to discover field offsets on Mono.
/// Example: probeHeapForI32Pair("MTGA.exe", 1825, 610) finds gold+gems.
#[napi]
pub fn probe_heap_for_i32_pair(process_name: String, val_a: i32, val_b: i32) -> serde_json::Value {
    match crate::mono::scanner::probe_heap_for_i32_pair(&process_name, val_a, val_b) {
        Ok(result) => serde_json::json!({ "result": result }),
        Err(e) => serde_json::json!({ "error": e }),
    }
}

#[napi]
pub fn read_class(process_name: String, address: i64) -> serde_json::Value {
    #[cfg(target_os = "windows")]
    { windows_backend::read_class_impl(&process_name, address) }

    #[cfg(target_os = "macos")]
    { macos_backend::read_class_impl(&process_name, address) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { serde_json::json!({ "error": "Platform not supported" }) }
}

#[napi]
pub fn read_generic_instance(process_name: String, address: i64) -> serde_json::Value {
    #[cfg(target_os = "windows")]
    { windows_backend::read_generic_instance_impl(&process_name, address) }

    #[cfg(target_os = "macos")]
    { macos_backend::read_generic_instance_impl(&process_name, address) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { serde_json::json!({ "error": "Platform not supported" }) }
}
