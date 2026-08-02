//! NAPI bindings for Node.js
//!
//! This module provides cross-platform Node.js bindings for reading MTGA memory.
//! - Windows: Uses Mono backend
//! - Linux: Uses the Mono backend too (MTGA runs the Windows binary under Wine)
//! - macOS: Uses IL2CPP backend
//!
//! Every function that touches the game process (init, find_process and the
//! read_* family) runs on libuv's threadpool via AsyncTask and returns a
//! Promise, so a slow memory read never blocks the caller's JS event loop —
//! critical in Electron, where the renderer hosting the GRE parser would
//! otherwise freeze. Trivial in-process checks (is_admin, is_initialized,
//! close) and the session-based debug explorer APIs (get_*) stay synchronous.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use std::sync::Mutex;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use sysinfo::{Pid, System};

// ============================================================================
// Threadpool task plumbing: wraps a blocking closure so #[napi] functions can
// return AsyncTask<...> (a JS Promise) with the work off the JS thread.
// ============================================================================

pub struct JsonTask {
    run: Option<Box<dyn FnOnce() -> serde_json::Value + Send>>,
}

impl JsonTask {
    fn spawn(f: impl FnOnce() -> serde_json::Value + Send + 'static) -> AsyncTask<JsonTask> {
        AsyncTask::new(JsonTask {
            run: Some(Box::new(f)),
        })
    }
}

impl napi::Task for JsonTask {
    type Output = serde_json::Value;
    // serde_json::Value implements ToNapiValue but not TypeName, so resolve
    // through JsUnknown (converted on the JS thread via serde).
    type JsValue = napi::JsUnknown;

    fn compute(&mut self) -> Result<Self::Output> {
        let run = self
            .run
            .take()
            .ok_or_else(|| Error::from_reason("Task already ran"))?;
        Ok(run())
    }

    fn resolve(&mut self, env: napi::Env, output: Self::Output) -> Result<Self::JsValue> {
        env.to_js_value(&output)
    }
}

pub struct BoolTask {
    run: Option<Box<dyn FnOnce() -> Result<bool> + Send>>,
}

impl BoolTask {
    fn spawn(f: impl FnOnce() -> Result<bool> + Send + 'static) -> AsyncTask<BoolTask> {
        AsyncTask::new(BoolTask {
            run: Some(Box::new(f)),
        })
    }
}

impl napi::Task for BoolTask {
    type Output = bool;
    type JsValue = bool;

    fn compute(&mut self) -> Result<Self::Output> {
        let run = self
            .run
            .take()
            .ok_or_else(|| Error::from_reason("Task already ran"))?;
        run()
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

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

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod windows_backend {
    use super::*;
    use crate::{
        field_definition::FieldDefinition,
        mono_reader::MonoReader,
        type_code::TypeCode,
        type_definition::TypeDefinition,
    };

    pub fn is_admin_impl() -> bool {
        MonoReader::is_admin()
    }

    pub fn find_process_impl(process_name: &str) -> bool {
        MonoReader::find_pid_by_name(process_name).is_some()
    }

    pub fn init_impl(process_name: &str) -> Result<bool> {
        crate::session::init(process_name).map_err(Error::from_reason)
    }

    pub fn close_impl() -> Result<bool> {
        crate::session::close().map_err(Error::from_reason)
    }

    pub fn is_initialized_impl() -> bool {
        crate::session::is_initialized()
    }

    fn with_reader<F, T>(f: F) -> Result<T>
    where
        F: FnOnce(&mut MonoReader) -> Result<T>,
    {
        let mut wrapper = crate::session::READER
            .lock()
            .map_err(|_| Error::from_reason("Failed to lock reader"))?;
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

    pub fn read_decks_impl(process_name: &str) -> serde_json::Value {
        crate::session::read_decks(process_name)
    }

    pub fn read_ranks_impl(process_name: &str) -> serde_json::Value {
        crate::session::read_ranks(process_name)
    }

    pub fn read_account_impl(process_name: &str) -> serde_json::Value {
        crate::session::read_account(process_name)
    }

    pub fn read_collection_impl(process_name: &str) -> serde_json::Value {
        crate::session::read_collection(process_name)
    }

    pub fn read_inventory_impl(process_name: &str) -> serde_json::Value {
        crate::session::read_inventory(process_name)
    }
}

// ============================================================================
// macOS Backend (IL2CPP)
// ============================================================================

#[cfg(target_os = "macos")]
mod macos_backend {
    use super::*;

    use crate::il2cpp::macos_runtime::{can_attach, find_pid, plausible, Il2Cpp};
    use crate::queries_il2cpp as q;
    use crate::session_il2cpp::SESSION;

    // Everything here runs against the shared cached session in
    // `session_il2cpp`, which owns the attached `Il2Cpp` runtime. Class and
    // field resolution is metadata-driven — see `il2cpp::macos_runtime`.

    /// On macOS this answers "can we read game memory?", which is the question
    /// callers actually gate on. Being root is *one* way to get there; a host
    /// signed with `com.apple.security.cs.debugger` can attach as a normal
    /// user, so reporting `geteuid() == 0` would wrongly disable the reader.
    pub fn is_admin_impl() -> bool {
        if unsafe { libc::geteuid() } == 0 {
            return true;
        }
        if is_initialized_impl() {
            return true; // already attached, so we demonstrably can
        }
        match find_pid("MTGA") {
            Some(pid) => can_attach(pid),
            // Can't tell without a target — let the caller's "is MTGA running"
            // check report the real problem rather than claiming no privileges.
            None => true,
        }
    }

    pub fn find_process_impl(process_name: &str) -> bool {
        find_pid(process_name).is_some()
    }

    pub fn init_impl(process_name: &str) -> Result<bool> {
        crate::session_il2cpp::init(process_name).map_err(Error::from_reason)
    }

    pub fn close_impl() -> Result<bool> {
        crate::session_il2cpp::close().map_err(Error::from_reason)
    }

    pub fn is_initialized_impl() -> bool {
        crate::session_il2cpp::is_initialized()
    }

    /// Run `f` against the attached runtime, attaching first if needed.
    fn with_runtime<F, T>(f: F) -> Result<T>
    where
        F: FnOnce(&Il2Cpp) -> Result<T>,
    {
        let guard = SESSION
            .lock()
            .map_err(|_| Error::from_reason("Failed to lock the IL2CPP session"))?;
        let rt = guard
            .0
            .as_ref()
            .ok_or_else(|| Error::from_reason("Reader not initialized. Call init() first."))?;
        rt.mem.refresh_regions();
        f(rt)
    }

    fn type_name_of(rt: &Il2Cpp, f: &crate::il2cpp::macos_runtime::FieldRec) -> String {
        rt.type_name(f.type_ptr, f.type_code)
    }

    fn field_infos(rt: &Il2Cpp, class: usize) -> Vec<FieldInfo> {
        let mut out = Vec::new();
        let mut cur = class;
        for _ in 0..16 {
            if cur == 0 {
                break;
            }
            for f in rt.class_fields(cur).iter() {
                if out.iter().any(|e: &FieldInfo| e.name == f.name) {
                    continue;
                }
                out.push(FieldInfo {
                    name: f.name.clone(),
                    type_name: type_name_of(rt, f),
                    offset: f.offset,
                    is_static: f.is_static,
                    is_const: false,
                });
            }
            cur = rt.class_parent(cur);
        }
        out
    }

    pub fn get_assemblies_impl() -> Result<Vec<String>> {
        // IL2CPP merges everything into GameAssembly; the type table is flat.
        Ok(vec!["GameAssembly".to_string()])
    }

    pub fn get_assembly_classes_impl(_assembly_name: &str) -> Result<Vec<ClassInfo>> {
        with_runtime(|rt| {
            let mut out = Vec::new();
            let table = rt
                .mem
                .meta_bytes(rt.type_info_table, rt.type_count * 8)
                .unwrap_or_default();

            for chunk in table.chunks_exact(8) {
                let class = usize::from_le_bytes(chunk.try_into().unwrap());
                if !plausible(class) || !rt.is_class(class) {
                    continue;
                }
                let name = rt.class_name(class);
                if name.is_empty() {
                    continue;
                }
                out.push(ClassInfo {
                    namespace: rt.class_namespace(class),
                    name,
                    address: class as i64,
                    is_static: false,
                    is_enum: false,
                });
            }
            Ok(out)
        })
    }

    pub fn get_class_details_impl(_assembly_name: &str, class_name: &str) -> Result<ClassDetails> {
        with_runtime(|rt| {
            let class = rt
                .find_class(class_name)
                .ok_or_else(|| Error::from_reason(format!("Class '{class_name}' not found")))?;

            let fields = field_infos(rt, class);

            // Surface any static field that currently holds a live object, so
            // the debug UI can jump straight into the graph.
            let mut static_instances = Vec::new();
            for f in rt.class_fields(class).iter().filter(|f| f.is_static) {
                if let Some((addr, _)) = rt.static_field_addr(class, &f.name) {
                    let ptr = rt.mem.read_ptr(addr);
                    if plausible(ptr) && rt.class_of(ptr) != 0 {
                        static_instances.push(StaticInstanceInfo {
                            field_name: f.name.clone(),
                            address: ptr as i64,
                        });
                    }
                }
            }

            Ok(ClassDetails {
                name: rt.class_name(class),
                namespace: rt.class_namespace(class),
                address: class as i64,
                fields,
                static_instances,
            })
        })
    }

    pub fn get_instance_impl(address: i64) -> Result<InstanceData> {
        with_runtime(|rt| {
            let obj = address as usize;
            let class = rt.class_of(obj);
            if class == 0 {
                return Err(Error::from_reason("Not a managed object address"));
            }

            let mut fields = Vec::new();
            let mut cur = class;
            for _ in 0..16 {
                if cur == 0 {
                    break;
                }
                for f in rt.class_fields(cur).iter() {
                    if f.is_static
                        || f.is_thread_static
                        || fields.iter().any(|e: &InstanceField| e.name == f.name)
                    {
                        continue;
                    }
                    fields.push(InstanceField {
                        name: f.name.clone(),
                        type_name: type_name_of(rt, f),
                        is_static: false,
                        value: q::value_json(rt, obj + f.offset as usize, f.type_code, f.type_ptr, 1),
                    });
                }
                cur = rt.class_parent(cur);
            }

            Ok(InstanceData {
                class_name: rt.class_name(class),
                namespace: rt.class_namespace(class),
                address,
                fields,
            })
        })
    }

    pub fn get_instance_field_impl(address: i64, field_name: &str) -> Result<serde_json::Value> {
        with_runtime(|rt| {
            let obj = address as usize;
            let (addr, f) = rt
                .field_addr(obj, field_name)
                .ok_or_else(|| Error::from_reason(format!("Field '{field_name}' not found")))?;
            Ok(q::value_json(rt, addr, f.type_code, f.type_ptr, 2))
        })
    }

    pub fn get_static_field_impl(class_address: i64, field_name: &str) -> Result<serde_json::Value> {
        with_runtime(|rt| {
            let class = class_address as usize;
            let (addr, f) = rt.static_field_addr(class, field_name).ok_or_else(|| {
                Error::from_reason(format!("Static field '{field_name}' not found"))
            })?;
            Ok(q::value_json(rt, addr, f.type_code, f.type_ptr, 2))
        })
    }

    pub fn get_dictionary_impl(address: i64) -> Result<DictionaryData> {
        with_runtime(|rt| {
            let dict = address as usize;
            if rt.class_of(dict) == 0 {
                return Err(Error::from_reason("Not a managed object address"));
            }

            let entries: Vec<DictionaryEntry> = rt
                .dict_entries(dict, 200_000)
                .into_iter()
                .map(|(ka, kc, va, vc)| DictionaryEntry {
                    key: q::value_json(rt, ka, kc, 0, 1),
                    value: q::value_json(rt, va, vc, 0, 1),
                })
                .collect();

            Ok(DictionaryData {
                count: entries.len() as i32,
                entries,
            })
        })
    }

    /// Walk a `[RootClass, StaticField, field, ...]` path, matching the Mono
    /// backend's semantics (arbitrary root class, not just the wrapper).
    pub fn read_data_impl(process_name: &str, fields: Vec<String>) -> serde_json::Value {
        crate::session_il2cpp::read_raw(process_name, |rt| q::read_data_path(rt, &fields))
    }

    pub fn read_class_impl(process_name: &str, address: i64) -> serde_json::Value {
        crate::session_il2cpp::read_raw(process_name, |rt| {
            q::object_json(rt, address as usize, 3)
        })
    }

    pub fn read_generic_instance_impl(process_name: &str, address: i64) -> serde_json::Value {
        read_class_impl(process_name, address)
    }

    // Typed readers run against the cached IL2CPP session (see
    // `session_il2cpp`), which mirrors the Mono/Windows behaviour: attach once,
    // then re-read only the root singleton per poll.

    pub fn read_decks_impl(process_name: &str) -> serde_json::Value {
        crate::session_il2cpp::read_decks(process_name)
    }

    pub fn read_ranks_impl(process_name: &str) -> serde_json::Value {
        crate::session_il2cpp::read_ranks(process_name)
    }

    pub fn read_account_impl(process_name: &str) -> serde_json::Value {
        crate::session_il2cpp::read_account(process_name)
    }

    pub fn read_collection_impl(process_name: &str) -> serde_json::Value {
        crate::session_il2cpp::read_collection(process_name)
    }

    pub fn read_inventory_impl(process_name: &str) -> serde_json::Value {
        crate::session_il2cpp::read_inventory(process_name)
    }
}

// ============================================================================
// Public NAPI API
// ============================================================================

#[napi]
pub fn is_admin() -> bool {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::is_admin_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::is_admin_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { false }
}

/// Async: process enumeration runs on the threadpool, resolves to a boolean.
#[napi]
pub fn find_process(process_name: String) -> AsyncTask<BoolTask> {
    BoolTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { Ok(windows_backend::find_process_impl(&process_name)) }

        #[cfg(target_os = "macos")]
        { Ok(macos_backend::find_process_impl(&process_name)) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { Ok(false) }
    })
}

/// Async: session init scans the game's loaded assemblies (the expensive,
/// multi-second step) — it runs on the threadpool and resolves when cached.
#[napi]
pub fn init(process_name: String) -> AsyncTask<BoolTask> {
    BoolTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::init_impl(&process_name) }

        #[cfg(target_os = "macos")]
        { macos_backend::init_impl(&process_name) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { Err(Error::from_reason("Platform not supported")) }
    })
}

#[napi]
pub fn close() -> Result<bool> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::close_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::close_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Ok(true) }
}

#[napi]
pub fn is_initialized() -> bool {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::is_initialized_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::is_initialized_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { false }
}

#[napi]
pub fn get_assemblies() -> Result<Vec<String>> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::get_assemblies_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::get_assemblies_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_assembly_classes(assembly_name: String) -> Result<Vec<ClassInfo>> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::get_assembly_classes_impl(&assembly_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_assembly_classes_impl(&assembly_name) }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_class_details(assembly_name: String, class_name: String) -> Result<ClassDetails> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::get_class_details_impl(&assembly_name, &class_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_class_details_impl(&assembly_name, &class_name) }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_instance(address: i64) -> Result<InstanceData> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::get_instance_impl(address) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_instance_impl(address) }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_instance_field(address: i64, field_name: String) -> Result<serde_json::Value> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::get_instance_field_impl(address, &field_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_instance_field_impl(address, &field_name) }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_static_field(class_address: i64, field_name: String) -> Result<serde_json::Value> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::get_static_field_impl(class_address, &field_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_static_field_impl(class_address, &field_name) }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn get_dictionary(address: i64) -> Result<DictionaryData> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    { windows_backend::get_dictionary_impl(address) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_dictionary_impl(address) }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

#[napi]
pub fn read_data(process_name: String, fields: Vec<String>) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_data_impl(&process_name, fields) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_data_impl(&process_name, fields) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}

#[napi]
pub fn read_class(process_name: String, address: i64) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_class_impl(&process_name, address) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_class_impl(&process_name, address) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}

#[napi]
pub fn read_generic_instance(process_name: String, address: i64) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_generic_instance_impl(&process_name, address) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_generic_instance_impl(&process_name, address) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}

/// Read all saved decks (name, deckId, format/attributes, per-pile card lists).
/// Home screen only — returns an error object during a match.
#[napi]
pub fn read_decks(process_name: String) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_decks_impl(&process_name) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_decks_impl(&process_name) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}

/// Read the player's constructed + limited rank info.
#[napi]
pub fn read_ranks(process_name: String) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_ranks_impl(&process_name) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_ranks_impl(&process_name) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}

/// Read the player's account identity (displayName, accountId, personaId, ...).
#[napi]
pub fn read_account(process_name: String) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_account_impl(&process_name) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_account_impl(&process_name) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}

/// Read the player's owned-card collection (grpId -> quantity).
#[napi]
pub fn read_collection(process_name: String) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_collection_impl(&process_name) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_collection_impl(&process_name) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}

/// Read the player's wallet/inventory (gems, gold, wildcards, vault, ...).
#[napi]
pub fn read_inventory(process_name: String) -> AsyncTask<JsonTask> {
    JsonTask::spawn(move || {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        { windows_backend::read_inventory_impl(&process_name) }

        #[cfg(target_os = "macos")]
        { macos_backend::read_inventory_impl(&process_name) }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { serde_json::json!({ "error": "Platform not supported" }) }
    })
}
