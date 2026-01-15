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
// Platform-specific backend implementations
// ============================================================================

#[cfg(target_os = "windows")]
mod windows_backend {
    use super::*;
    use mtga_reader::{
        field_definition::FieldDefinition,
        mono_reader::MonoReader,
        type_code::TypeCode,
        type_definition::TypeDefinition,
    };

    // Wrapper to make MonoReader Send + Sync
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
        let entries_start = entries_ptr + mtga_reader::constants::SIZE_OF_PTR * 4;

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

        match type_name {
            "System.Int32" | "int" => serde_json::json!(reader.read_i32(addr)),
            "System.Int64" | "long" => serde_json::json!(reader.read_i64(addr)),
            "System.UInt32" | "uint" => serde_json::json!(reader.read_u32(addr)),
            "System.Boolean" | "bool" | "BOOLEAN" => serde_json::json!(reader.read_u8(addr) != 0),
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
            "System.Int32" => serde_json::json!({
                "type": "primitive",
                "value_type": "int32",
                "value": reader.read_i32(field_location)
            }),
            "System.UInt32" => serde_json::json!({
                "type": "primitive",
                "value_type": "uint32",
                "value": reader.read_u32(field_location)
            }),
            "System.Int64" => serde_json::json!({
                "type": "primitive",
                "value_type": "int64",
                "value": reader.read_i64(field_location)
            }),
            "System.UInt64" => serde_json::json!({
                "type": "primitive",
                "value_type": "uint64",
                "value": reader.read_u64(field_location).to_string()
            }),
            "System.Boolean" => serde_json::json!({
                "type": "primitive",
                "value_type": "boolean",
                "value": reader.read_u8(field_location) != 0
            }),
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
}

// ============================================================================
// macOS IL2CPP Backend
// ============================================================================

#[cfg(target_os = "macos")]
mod macos_backend {
    use super::*;
    use std::process::Command;

    // IL2CPP offsets for MTGA on macOS
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

    // Memory reader using mach2
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

        pub fn read_i64(&self, addr: usize) -> i64 {
            let bytes = self.read_bytes(addr, 8);
            i64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
        }

        pub fn read_u64(&self, addr: usize) -> u64 {
            let bytes = self.read_bytes(addr, 8);
            u64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
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

    // IL2CPP State
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
        let output = Command::new("vmmap")
            .args(["-wide", &pid.to_string()])
            .output()
            .ok();

        if let Some(output) = output {
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
        }
        0
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

    fn find_papa_instance(reader: &MemReader, papa_class: usize) -> Option<usize> {
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
                        let val_at_16 = reader.read_ptr(obj_addr + 16);
                        if val_at_16 != papa_class && val_at_16 > 0x100000 {
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

    pub fn is_admin_impl() -> bool {
        // On macOS, we need to run with sudo
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

        // Find data segment and type info table
        let data_base = find_second_data_segment(pid);
        if data_base == 0 {
            return Err(Error::from_reason("Could not find GameAssembly __DATA segment"));
        }

        let type_info_table = reader.read_ptr(data_base + offsets::TYPE_INFO_TABLE_OFFSET);
        if type_info_table == 0 {
            return Err(Error::from_reason("Could not find type info table"));
        }

        // Find PAPA class
        let papa_class = find_class_by_name(&reader, type_info_table, "PAPA")
            .ok_or_else(|| Error::from_reason("PAPA class not found"))?;

        // Find PAPA instance
        let papa_instance = find_papa_instance(&reader, papa_class).unwrap_or(0);

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
        // IL2CPP doesn't have assemblies like Mono - return fake list for compatibility
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

            // Try to get type name from type data
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

            // Find static instances
            let mut static_instances = Vec::new();

            // Special handling for PAPA - we have a known instance
            if class_name == "PAPA" && state.papa_instance != 0 {
                static_instances.push(StaticInstanceInfo {
                    field_name: "_instance".to_string(),
                    address: state.papa_instance as i64,
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

        // For small offsets, might be a primitive
        if field.type_name.contains("Int32") || field.type_name.contains("int") {
            return serde_json::json!(reader.read_i32(field_addr));
        }
        if field.type_name.contains("Boolean") || field.type_name.contains("bool") {
            return serde_json::json!(reader.read_u8(field_addr) != 0);
        }

        // Try reading as pointer
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

        // Might be a small integer stored directly
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

    fn read_cards_and_quantity(reader: &MemReader, cards_addr: usize) -> Result<DictionaryData> {
        let entries_ptr = reader.read_ptr(cards_addr + 0x18);
        let count = reader.read_i32(cards_addr + 0x20);

        if entries_ptr == 0 || count <= 0 || count > 100000 {
            return Err(Error::from_reason("Invalid CardsAndQuantity structure"));
        }

        read_dict_entries_il2cpp(reader, entries_ptr, count)
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

            // Check class name to determine how to read
            let class_ptr = state.reader.read_ptr(dict_addr);
            let class_name = read_class_name(&state.reader, class_ptr);

            // For CardsAndQuantity
            if class_name == "CardsAndQuantity" {
                return read_cards_and_quantity(&state.reader, dict_addr);
            }

            // Try standard Dictionary layouts
            let entries_ptr = state.reader.read_ptr(dict_addr + 0x18);
            let count = state.reader.read_i32(dict_addr + 0x20);

            if entries_ptr > 0x100000 && count > 0 && count < 100000 {
                let arr_len = state.reader.read_u32(entries_ptr + 0x18);
                if arr_len > 0 {
                    return read_dict_entries_il2cpp(&state.reader, entries_ptr, count);
                }
            }

            // Try alternative offset
            let entries_ptr = state.reader.read_ptr(dict_addr + 0x10);
            if entries_ptr > 0x100000 {
                let arr_len = state.reader.read_u32(entries_ptr + 0x18);
                if arr_len > 0 && arr_len < 200000 {
                    let count = arr_len as i32;
                    return read_dict_entries_il2cpp(&state.reader, entries_ptr, count);
                }
            }

            Err(Error::from_reason("Could not read dictionary"))
        })
    }
}

// ============================================================================
// Public API - delegates to platform-specific implementations
// ============================================================================

/// Check if the current process has administrator privileges
#[napi]
pub fn is_admin() -> bool {
    #[cfg(target_os = "windows")]
    { windows_backend::is_admin_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::is_admin_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { false }
}

/// Find a process by name and return true if found
#[napi]
pub fn find_process(process_name: String) -> bool {
    #[cfg(target_os = "windows")]
    { windows_backend::find_process_impl(&process_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::find_process_impl(&process_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { false }
}

/// Initialize connection to the target process
/// Must be called before using any other reader functions
#[napi]
pub fn init(process_name: String) -> Result<bool> {
    #[cfg(target_os = "windows")]
    { windows_backend::init_impl(&process_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::init_impl(&process_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

/// Close the connection to the target process
#[napi]
pub fn close() -> Result<bool> {
    #[cfg(target_os = "windows")]
    { windows_backend::close_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::close_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Ok(true) }
}

/// Check if the reader is initialized
#[napi]
pub fn is_initialized() -> bool {
    #[cfg(target_os = "windows")]
    { windows_backend::is_initialized_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::is_initialized_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { false }
}

/// Get all loaded assembly names
#[napi]
pub fn get_assemblies() -> Result<Vec<String>> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_assemblies_impl() }

    #[cfg(target_os = "macos")]
    { macos_backend::get_assemblies_impl() }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

/// Get all classes in an assembly
#[napi]
pub fn get_assembly_classes(assembly_name: String) -> Result<Vec<ClassInfo>> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_assembly_classes_impl(&assembly_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_assembly_classes_impl(&assembly_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

/// Get detailed information about a class
#[napi]
pub fn get_class_details(assembly_name: String, class_name: String) -> Result<ClassDetails> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_class_details_impl(&assembly_name, &class_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_class_details_impl(&assembly_name, &class_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

/// Read an instance at a given memory address
#[napi]
pub fn get_instance(address: i64) -> Result<InstanceData> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_instance_impl(address) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_instance_impl(address) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

/// Read a specific field from an instance
#[napi]
pub fn get_instance_field(address: i64, field_name: String) -> Result<serde_json::Value> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_instance_field_impl(address, &field_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_instance_field_impl(address, &field_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

/// Read a static field from a class
#[napi]
pub fn get_static_field(class_address: i64, field_name: String) -> Result<serde_json::Value> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_static_field_impl(class_address, &field_name) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_static_field_impl(class_address, &field_name) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

/// Read a dictionary at a given memory address
#[napi]
pub fn get_dictionary(address: i64) -> Result<DictionaryData> {
    #[cfg(target_os = "windows")]
    { windows_backend::get_dictionary_impl(address) }

    #[cfg(target_os = "macos")]
    { macos_backend::get_dictionary_impl(address) }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { Err(Error::from_reason("Platform not supported")) }
}

// ============================================================================
// High-level data reading functions
// These use the main mtga-reader library for Windows (Mono)
// For macOS (IL2CPP), we provide simplified implementations
// ============================================================================

/// Read nested data by traversing a path of field names
/// The first element is the root class name, subsequent elements are field names
#[napi]
pub fn read_data(process_name: String, fields: Vec<String>) -> serde_json::Value {
    #[cfg(target_os = "windows")]
    {
        mtga_reader::read_data(process_name, fields)
    }

    #[cfg(target_os = "macos")]
    {
        // For macOS IL2CPP, we need a simplified implementation
        // that navigates through the known PAPA path
        use macos_backend::*;

        let result = (|| -> Result<serde_json::Value> {
            if fields.is_empty() {
                return Err(Error::from_reason("No path specified"));
            }

            // Initialize if needed
            if !is_initialized_impl() {
                init_impl(&process_name)?;
            }

            let wrapper = STATE.lock().map_err(|_| Error::from_reason("Failed to lock state"))?;
            let state = wrapper.0.as_ref()
                .ok_or_else(|| Error::from_reason("Reader not initialized"))?;

            // Start from PAPA or WrapperController
            let root_name = &fields[0];
            if root_name != "PAPA" && root_name != "WrapperController" {
                return Err(Error::from_reason(format!("Root class '{}' not supported on macOS IL2CPP. Use 'PAPA' or 'WrapperController'.", root_name)));
            }

            // Navigate from PAPA instance
            if state.papa_instance == 0 {
                return Err(Error::from_reason("PAPA instance not found"));
            }

            let mut current_addr = state.papa_instance;
            let mut current_class = state.reader.read_ptr(current_addr);

            // Skip the root class name, navigate through field path
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

            // Return the final object's data
            let class_name = read_class_name(&state.reader, current_class);

            // If it's a dictionary-like structure, return its entries
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

            // Return basic object info
            Ok(serde_json::json!({
                "address": current_addr,
                "class": class_name
            }))
        })();

        result.unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        serde_json::json!({ "error": "Platform not supported" })
    }
}

/// Read a managed class at a given address
#[napi]
pub fn read_class(process_name: String, address: i64) -> serde_json::Value {
    #[cfg(target_os = "windows")]
    {
        mtga_reader::read_class(process_name, address)
    }

    #[cfg(target_os = "macos")]
    {
        // For macOS, just use get_instance
        match get_instance(address) {
            Ok(data) => serde_json::to_value(data).unwrap_or(serde_json::json!({"error": "Serialization failed"})),
            Err(e) => serde_json::json!({ "error": e.to_string() })
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        serde_json::json!({ "error": "Platform not supported" })
    }
}

/// Read a generic instance at a given address
#[napi]
pub fn read_generic_instance(process_name: String, address: i64) -> serde_json::Value {
    #[cfg(target_os = "windows")]
    {
        mtga_reader::read_generic_instance(process_name, address)
    }

    #[cfg(target_os = "macos")]
    {
        // For macOS, just use get_instance
        match get_instance(address) {
            Ok(data) => serde_json::to_value(data).unwrap_or(serde_json::json!({"error": "Serialization failed"})),
            Err(e) => serde_json::json!({ "error": e.to_string() })
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        serde_json::json!({ "error": "Platform not supported" })
    }
}
