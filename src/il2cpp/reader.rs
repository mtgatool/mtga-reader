//! IL2CPP runtime backend reader
//!
//! This is the main entry point for reading IL2CPP runtime structures.
//! Implements the RuntimeBackend trait for IL2CPP-based Unity games.

use crate::backend::{RuntimeBackend, MemoryReader, BackendError, TypeDef, FieldDef, TypeInfoData};
use crate::common::TypeCode;
use super::offsets::{Il2CppOffsets, SIZE_OF_PTR};
use super::metadata::MetadataParser;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use process_memory::{DataMember, Memory, ProcessHandle, TryIntoProcessHandle};

#[cfg(target_os = "macos")]
use super::macos_memory::MacOsMemoryReader;

/// IL2CPP runtime backend
pub struct Il2CppBackend {
    pid: u32,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    handle: process_memory::ProcessHandle,
    #[cfg(target_os = "macos")]
    macos_reader: Option<MacOsMemoryReader>,
    offsets: Il2CppOffsets,
    game_assembly_base: usize,
    /// Base address of the second __DATA segment (where globals live)
    data_segment_base: usize,
    /// Cached s_TypeInfoTable address
    type_info_table: usize,
    /// Number of types (from metadata)
    type_count: usize,
    metadata: Option<MetadataParser>,
    initialized: bool,
}

impl std::fmt::Debug for Il2CppBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Il2CppBackend")
            .field("pid", &self.pid)
            .field("game_assembly_base", &format!("0x{:x}", self.game_assembly_base))
            .field("initialized", &self.initialized)
            .finish()
    }
}

// Safety: ProcessHandle on Windows/Linux is safe to send between threads.
// On macOS, MacOsMemoryReader uses mach ports which are also thread-safe.
unsafe impl Send for Il2CppBackend {}
unsafe impl Sync for Il2CppBackend {}

impl Il2CppBackend {
    /// Create a new IL2CPP backend for the given process ID
    pub fn new(pid: u32) -> Self {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        let handle = (pid as process_memory::Pid)
            .try_into_process_handle()
            .expect("Failed to get process handle");

        Il2CppBackend {
            pid,
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            handle,
            #[cfg(target_os = "macos")]
            macos_reader: None,
            offsets: Il2CppOffsets::default(),
            game_assembly_base: 0,
            data_segment_base: 0,
            type_info_table: 0,
            type_count: 0,
            metadata: None,
            initialized: false,
        }
    }

    /// Create a new IL2CPP backend for macOS with mach memory reader
    #[cfg(target_os = "macos")]
    pub fn new_macos(pid: u32) -> Result<Self, BackendError> {
        let macos_reader = MacOsMemoryReader::new(pid)?;

        Ok(Il2CppBackend {
            pid,
            macos_reader: Some(macos_reader),
            offsets: Il2CppOffsets::default(),
            game_assembly_base: 0,
            data_segment_base: 0,
            type_info_table: 0,
            type_count: 0,
            metadata: None,
            initialized: false,
        })
    }

    /// Set custom offsets (for different Unity versions)
    pub fn with_offsets(mut self, offsets: Il2CppOffsets) -> Self {
        self.offsets = offsets;
        self
    }

    /// Find the GameAssembly module in the process
    #[cfg(target_os = "windows")]
    fn find_game_assembly(&mut self) -> Result<usize, BackendError> {
        use proc_mem::Process;
        use super::offsets::IL2CPP_LIBRARY;

        let process = Process::with_pid(self.pid)
            .map_err(|e| BackendError::ProcessNotFound(format!("{:?}", e)))?;

        let module = process.module(IL2CPP_LIBRARY)
            .map_err(|_| BackendError::InitializationFailed(
                "GameAssembly.dll not found".to_string()
            ))?;

        self.game_assembly_base = module.base_address() as usize;
        Ok(self.game_assembly_base)
    }

    #[cfg(target_os = "linux")]
    fn find_game_assembly(&mut self) -> Result<usize, BackendError> {
        use super::offsets::IL2CPP_LIBRARY;

        // Parse /proc/<pid>/maps to find GameAssembly.so
        let maps_path = format!("/proc/{}/maps", self.pid);
        let maps = std::fs::read_to_string(&maps_path)
            .map_err(|e| BackendError::InitializationFailed(format!("Failed to read maps: {}", e)))?;

        for line in maps.lines() {
            if line.contains(IL2CPP_LIBRARY) {
                // Parse the base address from the line
                if let Some(addr_str) = line.split('-').next() {
                    if let Ok(addr) = usize::from_str_radix(addr_str, 16) {
                        self.game_assembly_base = addr;
                        return Ok(addr);
                    }
                }
            }
        }

        Err(BackendError::InitializationFailed(
            "GameAssembly.so not found in process maps".to_string()
        ))
    }

    #[cfg(target_os = "macos")]
    fn find_game_assembly(&mut self) -> Result<usize, BackendError> {
        use super::macos_memory::find_game_assembly_base;

        if let Some(base) = find_game_assembly_base(self.pid) {
            self.game_assembly_base = base;
            Ok(base)
        } else {
            Err(BackendError::InitializationFailed(
                "GameAssembly.dylib not found in process".to_string()
            ))
        }
    }

    /// Load metadata from global-metadata.dat
    pub fn load_metadata(&mut self, path: &std::path::Path) -> Result<(), BackendError> {
        let parser = MetadataParser::from_file(path)
            .map_err(|e| BackendError::InitializationFailed(format!("Failed to load metadata: {}", e)))?;
        self.metadata = Some(parser);
        Ok(())
    }

    /// Get the offsets being used
    pub fn offsets(&self) -> &Il2CppOffsets {
        &self.offsets
    }

    /// Find the second __DATA segment (where IL2CPP globals are stored) on macOS
    #[cfg(target_os = "macos")]
    fn find_data_segment(&mut self) -> Result<usize, BackendError> {
        use std::process::Command;

        let output = Command::new("vmmap")
            .args(["-wide", &self.pid.to_string()])
            .output()
            .map_err(|e| BackendError::InitializationFailed(format!("vmmap failed: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut found_first = false;
        for line in stdout.lines() {
            if line.contains("GameAssembly") && line.contains("__DATA") && !line.contains("__DATA_CONST") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let addr_parts: Vec<&str> = parts[1].split('-').collect();
                    if addr_parts.len() == 2 {
                        if let Ok(start) = usize::from_str_radix(addr_parts[0], 16) {
                            if found_first {
                                self.data_segment_base = start;
                                return Ok(start);
                            }
                            found_first = true;
                        }
                    }
                }
            }
        }

        Err(BackendError::InitializationFailed(
            "Could not find GameAssembly __DATA segment".to_string()
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn find_data_segment(&mut self) -> Result<usize, BackendError> {
        // TODO: Implement for Windows/Linux
        Err(BackendError::InitializationFailed(
            "Data segment finding not implemented for this platform".to_string()
        ))
    }

    /// Initialize the type info table pointer
    fn init_type_info_table(&mut self) -> Result<(), BackendError> {
        if self.data_segment_base == 0 {
            self.find_data_segment()?;
        }

        let table_ptr_addr = self.data_segment_base + self.offsets.global_offsets.type_info_table;
        self.type_info_table = self.read_ptr(table_ptr_addr);

        if self.type_info_table == 0 {
            return Err(BackendError::InitializationFailed(
                "s_TypeInfoTable is null".to_string()
            ));
        }

        // Get type count from metadata if loaded
        if let Some(ref metadata) = self.metadata {
            self.type_count = metadata.type_definition_count();
        }

        Ok(())
    }

    /// Find an Il2CppClass by name
    /// Returns the address of the Il2CppClass structure if found
    pub fn find_class(&self, name: &str) -> Option<usize> {
        if self.type_info_table == 0 {
            return None;
        }

        let max_types = if self.type_count > 0 { self.type_count } else { 50000 };

        for i in 0..max_types {
            let class_ptr = self.read_ptr(self.type_info_table + i * SIZE_OF_PTR);
            if class_ptr == 0 {
                continue;
            }

            let name_ptr = self.read_ptr(class_ptr + self.offsets.class_name as usize);
            if name_ptr == 0 {
                continue;
            }

            let class_name = self.read_ascii_string(name_ptr);
            if class_name == name {
                return Some(class_ptr);
            }
        }

        None
    }

    /// Find an Il2CppClass by full name (namespace.name)
    pub fn find_class_by_full_name(&self, namespace: &str, name: &str) -> Option<usize> {
        if self.type_info_table == 0 {
            return None;
        }

        let max_types = if self.type_count > 0 { self.type_count } else { 50000 };

        for i in 0..max_types {
            let class_ptr = self.read_ptr(self.type_info_table + i * SIZE_OF_PTR);
            if class_ptr == 0 {
                continue;
            }

            let name_ptr = self.read_ptr(class_ptr + self.offsets.class_name as usize);
            let ns_ptr = self.read_ptr(class_ptr + self.offsets.class_namespace as usize);

            if name_ptr == 0 {
                continue;
            }

            let class_name = self.read_ascii_string(name_ptr);
            let class_ns = if ns_ptr != 0 { self.read_ascii_string(ns_ptr) } else { String::new() };

            if class_name == name && class_ns == namespace {
                return Some(class_ptr);
            }
        }

        None
    }

    /// Get the static fields pointer for a class
    pub fn get_static_fields(&self, class_ptr: usize) -> usize {
        self.read_ptr(class_ptr + self.offsets.class_static_fields as usize)
    }

    /// Read a value from a class's static fields at the given offset
    pub fn read_static_field<T: Copy + Default>(&self, class_ptr: usize, offset: usize) -> T {
        let static_fields = self.get_static_fields(class_ptr);
        if static_fields == 0 {
            return T::default();
        }

        // Read the value - this is a simplified implementation
        // Real implementation would need type-specific reading
        let bytes = self.read_bytes(static_fields + offset, std::mem::size_of::<T>());
        if bytes.len() == std::mem::size_of::<T>() {
            unsafe { std::ptr::read(bytes.as_ptr() as *const T) }
        } else {
            T::default()
        }
    }

    /// Get class name from an Il2CppClass pointer
    pub fn get_class_name(&self, class_ptr: usize) -> String {
        let name_ptr = self.read_ptr(class_ptr + self.offsets.class_name as usize);
        if name_ptr == 0 {
            return String::new();
        }
        self.read_ascii_string(name_ptr)
    }

    /// Get class namespace from an Il2CppClass pointer
    pub fn get_class_namespace(&self, class_ptr: usize) -> String {
        let ns_ptr = self.read_ptr(class_ptr + self.offsets.class_namespace as usize);
        if ns_ptr == 0 {
            return String::new();
        }
        self.read_ascii_string(ns_ptr)
    }

    /// Get parent class pointer
    pub fn get_class_parent(&self, class_ptr: usize) -> usize {
        self.read_ptr(class_ptr + self.offsets.class_parent as usize)
    }

    /// Get the type info table address
    pub fn type_info_table(&self) -> usize {
        self.type_info_table
    }

    /// Get the data segment base address
    pub fn data_segment_base(&self) -> usize {
        self.data_segment_base
    }
}

impl MemoryReader for Il2CppBackend {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_u8(&self, addr: usize) -> u8 {
        use process_memory::DataMember;
        let mut member = DataMember::<u8>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_u8(&self, addr: usize) -> u8 {
        self.macos_reader.as_ref().map(|r| r.read_u8(addr)).unwrap_or(0)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_u16(&self, addr: usize) -> u16 {
        use process_memory::DataMember;
        let mut member = DataMember::<u16>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_u16(&self, addr: usize) -> u16 {
        self.macos_reader.as_ref().map(|r| r.read_u16(addr)).unwrap_or(0)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_u32(&self, addr: usize) -> u32 {
        use process_memory::DataMember;
        let mut member = DataMember::<u32>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_u32(&self, addr: usize) -> u32 {
        self.macos_reader.as_ref().map(|r| r.read_u32(addr)).unwrap_or(0)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_u64(&self, addr: usize) -> u64 {
        use process_memory::DataMember;
        let mut member = DataMember::<u64>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_u64(&self, addr: usize) -> u64 {
        self.macos_reader.as_ref().map(|r| r.read_u64(addr)).unwrap_or(0)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_i8(&self, addr: usize) -> i8 {
        use process_memory::DataMember;
        let mut member = DataMember::<i8>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_i8(&self, addr: usize) -> i8 {
        self.read_u8(addr) as i8
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_i16(&self, addr: usize) -> i16 {
        use process_memory::DataMember;
        let mut member = DataMember::<i16>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_i16(&self, addr: usize) -> i16 {
        self.read_u16(addr) as i16
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_i32(&self, addr: usize) -> i32 {
        use process_memory::DataMember;
        let mut member = DataMember::<i32>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_i32(&self, addr: usize) -> i32 {
        self.macos_reader.as_ref().map(|r| r.read_i32(addr)).unwrap_or(0)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_i64(&self, addr: usize) -> i64 {
        use process_memory::DataMember;
        let mut member = DataMember::<i64>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_i64(&self, addr: usize) -> i64 {
        self.macos_reader.as_ref().map(|r| r.read_i64(addr)).unwrap_or(0)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_f32(&self, addr: usize) -> f32 {
        use process_memory::DataMember;
        let mut member = DataMember::<f32>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0.0) }
    }

    #[cfg(target_os = "macos")]
    fn read_f32(&self, addr: usize) -> f32 {
        // Read as u32 and transmute to f32
        f32::from_bits(self.read_u32(addr))
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_f64(&self, addr: usize) -> f64 {
        use process_memory::DataMember;
        let mut member = DataMember::<f64>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0.0) }
    }

    #[cfg(target_os = "macos")]
    fn read_f64(&self, addr: usize) -> f64 {
        // Read as u64 and transmute to f64
        f64::from_bits(self.read_u64(addr))
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn read_ptr(&self, addr: usize) -> usize {
        use process_memory::DataMember;
        let mut member = DataMember::<usize>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    #[cfg(target_os = "macos")]
    fn read_ptr(&self, addr: usize) -> usize {
        self.macos_reader.as_ref().map(|r| r.read_ptr(addr)).unwrap_or(0)
    }

    fn read_bytes(&self, addr: usize, len: usize) -> Vec<u8> {
        let mut result = vec![0u8; len];
        for i in 0..len {
            result[i] = self.read_u8(addr + i);
        }
        result
    }

    fn read_ascii_string(&self, addr: usize) -> String {
        self.maybe_read_ascii_string(addr).unwrap_or_default()
    }

    fn maybe_read_ascii_string(&self, addr: usize) -> Option<String> {
        if addr == 0 {
            return None;
        }

        let mut string = String::new();
        let mut index = 0;
        loop {
            let val = self.read_u8(addr + index);
            if val == 0 || index > 1024 {
                break;
            }
            string.push(val as char);
            index += 1;
        }
        Some(string)
    }

    fn read_managed_string(&self, string_ptr: usize) -> Option<String> {
        if string_ptr == 0 || string_ptr < 0x10000 {
            return None;
        }

        let length = self.read_u32(string_ptr + self.offsets.string_length as usize);
        if length == 0 || length > 10000 {
            return None;
        }

        let mut utf16_chars = Vec::new();
        let chars_offset = string_ptr + self.offsets.string_chars as usize;

        for i in 0..length {
            let char_val = self.read_u16(chars_offset + (i as usize * 2));
            utf16_chars.push(char_val);
        }

        String::from_utf16(&utf16_chars).ok()
    }

    fn ptr_size(&self) -> usize {
        SIZE_OF_PTR
    }
}

impl RuntimeBackend for Il2CppBackend {
    fn initialize(&mut self) -> Result<(), BackendError> {
        self.find_game_assembly()?;
        self.find_data_segment()?;
        self.init_type_info_table()?;
        self.initialized = true;
        Ok(())
    }

    fn get_type_definitions(&self) -> Vec<usize> {
        if self.type_info_table == 0 {
            return Vec::new();
        }

        let max_types = if self.type_count > 0 { self.type_count } else { 50000 };
        let mut result = Vec::with_capacity(max_types);

        for i in 0..max_types {
            let class_ptr = self.read_ptr(self.type_info_table + i * SIZE_OF_PTR);
            if class_ptr != 0 {
                result.push(class_ptr);
            }
        }

        result
    }

    fn get_type_definitions_for_image(&self, _image_addr: usize) -> Vec<usize> {
        // TODO: Implement
        Vec::new()
    }

    fn get_assembly_names(&self) -> Vec<String> {
        // TODO: Read from metadata
        if let Some(ref metadata) = self.metadata {
            metadata.get_assembly_names()
        } else {
            Vec::new()
        }
    }

    fn get_assembly_image(&self, _name: &str) -> Option<usize> {
        // TODO: Implement
        None
    }

    fn create_type_def(&self, addr: usize) -> Box<dyn TypeDef + '_> {
        Box::new(super::type_definition::Il2CppTypeDef::new(addr, self))
    }

    fn create_field_def(&self, addr: usize) -> Box<dyn FieldDef + '_> {
        Box::new(super::field_definition::Il2CppFieldDef::new(addr, self))
    }

    fn read_type_info(&self, addr: usize) -> TypeInfoData {
        let data = self.read_ptr(addr + self.offsets.type_data as usize);
        let attrs = self.read_u32(addr + self.offsets.type_attrs as usize);

        // IL2CPP type encoding is slightly different
        let type_code_raw = attrs & 0xFF;
        let is_static = (attrs & 0x10) != 0;
        let is_const = (attrs & 0x40) != 0;

        TypeInfoData {
            addr,
            data,
            attrs,
            is_static,
            is_const,
            type_code: TypeCode::from_raw(type_code_raw),
        }
    }

    fn runtime_name(&self) -> &'static str {
        "IL2CPP"
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }
}
