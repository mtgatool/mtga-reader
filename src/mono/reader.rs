//! Mono runtime backend reader
//!
//! This is the main entry point for reading Mono runtime structures.
//! Implements the RuntimeBackend trait for Mono-based Unity games.

use process_memory::{DataMember, Memory, ProcessHandle, TryIntoProcessHandle};
use crate::backend::{RuntimeBackend, MemoryReader, BackendError, TypeDef, FieldDef, TypeInfoData};
use super::offsets::{MonoOffsets, SIZE_OF_PTR, RIP_PLUS_OFFSET_OFFSET, RIP_VALUE_OFFSET};

/// Mono runtime backend
#[derive(Debug)]
pub struct MonoBackend {
    pid: u32,
    handle: ProcessHandle,
    offsets: MonoOffsets,
    mono_root_domain: usize,
    assembly_image_address: usize,
    initialized: bool,
}

// Safety: ProcessHandle on Windows is a HANDLE which is safe to send between threads.
// The Windows API guarantees that process handles can be used from any thread.
unsafe impl Send for MonoBackend {}
unsafe impl Sync for MonoBackend {}

impl MonoBackend {
    /// Create a new Mono backend for the given process ID
    pub fn new(pid: u32) -> Self {
        let handle = (pid as process_memory::Pid)
            .try_into_process_handle()
            .expect("Failed to get process handle");

        MonoBackend {
            pid,
            handle,
            offsets: MonoOffsets::default(),
            mono_root_domain: 0,
            assembly_image_address: 0,
            initialized: false,
        }
    }

    /// Set custom offsets (for different Unity versions)
    pub fn with_offsets(mut self, offsets: MonoOffsets) -> Self {
        self.offsets = offsets;
        self
    }

    /// Read the mono root domain address
    #[cfg(target_os = "windows")]
    fn read_mono_root_domain(&mut self) -> Result<usize, BackendError> {
        use proc_mem::{ProcMemError, Process};
        use super::offsets::MONO_LIBRARY;
        use crate::mono::pe_reader::PEReader;

        let process = Process::with_pid(self.pid)
            .map_err(|e| BackendError::ProcessNotFound(format!("{:?}", e)))?;

        let module = match process.module(MONO_LIBRARY) {
            Ok(m) => m,
            Err(ProcMemError::ModuleNotFound) => {
                return Err(BackendError::InitializationFailed(
                    "Mono module not found in process".to_string()
                ));
            }
            Err(e) => {
                return Err(BackendError::InitializationFailed(format!("{:?}", e)));
            }
        };

        let pe = PEReader::new(self, module.base_address() as usize);
        let offset = pe.get_function_offset("mono_get_root_domain")
            .map_err(|e| BackendError::InitializationFailed(format!("{:?}", e)))?;

        self.mono_root_domain = module.base_address() as usize + offset as usize;
        Ok(self.mono_root_domain)
    }

    #[cfg(target_os = "linux")]
    fn read_mono_root_domain(&mut self) -> Result<usize, BackendError> {
        use crate::mono::pe_reader::PEReader;

        let mut addr: usize = 0;
        let mut managed = DataMember::<u16>::new(self.handle);

        // Scan memory for PE header (MZ magic)
        loop {
            let val = unsafe {
                managed.set_offset(vec![addr]);
                managed.read().unwrap_or(0)
            };

            if val == 0x5a4d {
                // MZ header found
                let pe = PEReader::new(self, addr);
                if let Ok(offset) = pe.get_function_offset("mono_get_root_domain") {
                    self.mono_root_domain = addr + offset as usize;
                    return Ok(self.mono_root_domain);
                }
            }

            addr += 4096;
            if addr > 0x7FFF_FFFF_FFFF {
                break;
            }
        }

        Err(BackendError::InitializationFailed(
            "Could not find mono root domain".to_string()
        ))
    }

    #[cfg(target_os = "macos")]
    fn read_mono_root_domain(&mut self) -> Result<usize, BackendError> {
        // macOS Mono support not implemented - use IL2CPP backend instead
        Err(BackendError::InitializationFailed(
            "Mono backend not supported on macOS - use IL2CPP".to_string()
        ))
    }

    /// Read the Assembly-CSharp image address
    fn read_assembly_image(&mut self) -> Result<usize, BackendError> {
        let offset = self.read_i32(self.mono_root_domain + RIP_PLUS_OFFSET_OFFSET)
            + RIP_VALUE_OFFSET as i32;

        let domain = self.read_ptr(self.mono_root_domain + offset as usize);
        if domain == 0 {
            return Err(BackendError::InitializationFailed(
                "Failed to read domain address".to_string()
            ));
        }

        let assembly_array = self.read_ptr(domain + self.offsets.referenced_assemblies as usize);
        let mut assembly_addr = assembly_array;

        while assembly_addr != 0 {
            let assembly = self.read_ptr(assembly_addr);
            let name_addr = self.read_ptr(assembly + SIZE_OF_PTR * 2);

            if let Some(name) = self.maybe_read_ascii_string(name_addr) {
                if name == "Assembly-CSharp" {
                    self.assembly_image_address =
                        self.read_ptr(assembly + self.offsets.assembly_image as usize);
                    return Ok(self.assembly_image_address);
                }
            }

            assembly_addr = self.read_ptr(assembly_addr + SIZE_OF_PTR);
        }

        Err(BackendError::AssemblyNotFound("Assembly-CSharp".to_string()))
    }

    /// Get the offsets being used
    pub fn offsets(&self) -> &MonoOffsets {
        &self.offsets
    }

    /// Get the mono root domain address
    pub fn mono_root_domain(&self) -> usize {
        self.mono_root_domain
    }
}

impl MemoryReader for MonoBackend {
    fn read_u8(&self, addr: usize) -> u8 {
        let mut member = DataMember::<u8>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_u16(&self, addr: usize) -> u16 {
        let mut member = DataMember::<u16>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_u32(&self, addr: usize) -> u32 {
        let mut member = DataMember::<u32>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_u64(&self, addr: usize) -> u64 {
        let mut member = DataMember::<u64>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_i8(&self, addr: usize) -> i8 {
        let mut member = DataMember::<i8>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_i16(&self, addr: usize) -> i16 {
        let mut member = DataMember::<i16>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_i32(&self, addr: usize) -> i32 {
        let mut member = DataMember::<i32>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_i64(&self, addr: usize) -> i64 {
        let mut member = DataMember::<i64>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
    }

    fn read_f32(&self, addr: usize) -> f32 {
        let mut member = DataMember::<f32>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0.0) }
    }

    fn read_f64(&self, addr: usize) -> f64 {
        let mut member = DataMember::<f64>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0.0) }
    }

    fn read_ptr(&self, addr: usize) -> usize {
        let mut member = DataMember::<usize>::new(self.handle);
        member.set_offset(vec![addr]);
        unsafe { member.read().unwrap_or(0) }
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
        let mut string = String::new();
        let mut index = 0;
        loop {
            let mut member = DataMember::<u8>::new(self.handle);
            member.set_offset(vec![addr + index]);
            let val = unsafe { member.read().ok()? };

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

        let length = self.read_u32(string_ptr + SIZE_OF_PTR * 2);
        if length == 0 || length > 10000 {
            return None;
        }

        let mut utf16_chars = Vec::new();
        let chars_offset = string_ptr + SIZE_OF_PTR * 2 + 4;

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

impl RuntimeBackend for MonoBackend {
    fn initialize(&mut self) -> Result<(), BackendError> {
        self.read_mono_root_domain()?;
        self.read_assembly_image()?;
        self.initialized = true;
        Ok(())
    }

    fn get_type_definitions(&self) -> Vec<usize> {
        self.get_type_definitions_for_image(self.assembly_image_address)
    }

    fn get_type_definitions_for_image(&self, image_addr: usize) -> Vec<usize> {
        let class_cache_size = self.read_u32(
            image_addr + self.offsets.image_class_cache as usize + self.offsets.hash_table_size as usize
        );
        let class_cache_table = self.read_ptr(
            image_addr + self.offsets.image_class_cache as usize + self.offsets.hash_table_table as usize
        );

        let mut type_defs = Vec::new();
        let mut table_item = 0u32;

        while table_item < class_cache_size * SIZE_OF_PTR as u32 {
            let mut definition = self.read_ptr(class_cache_table + table_item as usize);

            while definition != 0 {
                type_defs.push(definition);
                definition = self.read_ptr(
                    definition + self.offsets.type_def_next_class_cache as usize
                );
            }

            table_item += SIZE_OF_PTR as u32;
        }

        type_defs
    }

    fn get_assembly_names(&self) -> Vec<String> {
        let mut names = Vec::new();

        let offset = self.read_i32(self.mono_root_domain + RIP_PLUS_OFFSET_OFFSET)
            + RIP_VALUE_OFFSET as i32;
        let domain = self.read_ptr(self.mono_root_domain + offset as usize);
        let assembly_array = self.read_ptr(domain + self.offsets.referenced_assemblies as usize);

        let mut assembly_addr = assembly_array;
        while assembly_addr != 0 {
            let assembly = self.read_ptr(assembly_addr);
            let name_addr = self.read_ptr(assembly + SIZE_OF_PTR * 2);

            if let Some(name) = self.maybe_read_ascii_string(name_addr) {
                if !name.is_empty() {
                    names.push(name);
                }
            }

            assembly_addr = self.read_ptr(assembly_addr + SIZE_OF_PTR);
        }

        names
    }

    fn get_assembly_image(&self, target_name: &str) -> Option<usize> {
        let offset = self.read_i32(self.mono_root_domain + RIP_PLUS_OFFSET_OFFSET)
            + RIP_VALUE_OFFSET as i32;
        let domain = self.read_ptr(self.mono_root_domain + offset as usize);
        let assembly_array = self.read_ptr(domain + self.offsets.referenced_assemblies as usize);

        let mut assembly_addr = assembly_array;
        while assembly_addr != 0 {
            let assembly = self.read_ptr(assembly_addr);
            let name_addr = self.read_ptr(assembly + SIZE_OF_PTR * 2);

            if let Some(name) = self.maybe_read_ascii_string(name_addr) {
                if name == target_name {
                    return Some(self.read_ptr(assembly + self.offsets.assembly_image as usize));
                }
            }

            assembly_addr = self.read_ptr(assembly_addr + SIZE_OF_PTR);
        }

        None
    }

    fn create_type_def(&self, addr: usize) -> Box<dyn TypeDef + '_> {
        Box::new(super::type_definition::MonoTypeDef::new(addr, self))
    }

    fn create_field_def(&self, addr: usize) -> Box<dyn FieldDef + '_> {
        Box::new(super::field_definition::MonoFieldDef::new(addr, self))
    }

    fn read_type_info(&self, addr: usize) -> TypeInfoData {
        let data = self.read_ptr(addr);
        let attrs = self.read_u32(addr + SIZE_OF_PTR);
        let is_static = (attrs & 0x10) == 0x10;
        let is_const = (attrs & 0x40) == 0x40;
        let type_code_raw = 0xff & (attrs >> 16);

        TypeInfoData {
            addr,
            data,
            attrs,
            is_static,
            is_const,
            type_code: crate::common::TypeCode::from_raw(type_code_raw),
        }
    }

    fn runtime_name(&self) -> &'static str {
        "Mono"
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }
}
