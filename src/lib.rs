use proc_mem::{ProcMemError, Process};
use process_memory::{DataMember, Memory, ProcessHandle, TryIntoProcessHandle};

pub mod constants;

mod pe_reader;

pub struct MonoReader {
    pid: u32,
    handle: ProcessHandle,
    mono_root_domain: usize,
    assembly_image_address: usize,
    type_definitions: Vec<(usize, TypeDefinition)>,
}

impl MonoReader {
    pub fn new(pid: u32) -> Self {
        let handle = pid.try_into_process_handle().unwrap();
        MonoReader {
            pid,
            handle,
            mono_root_domain: 0,
            assembly_image_address: 0,
            type_definitions: Vec::new(),
        }
    }

    pub fn read_mono_root_domain(&mut self) -> usize {
        let mtga_process = match Process::with_pid(*&self.pid) {
            Ok(process) => Some(process),
            Err(e) => {
                eprintln!("Error obtaining process data: {:?}", e);
                None
            }
        }
        .unwrap();

        let module = match mtga_process.module("mono-2.0-bdwgc.dll") {
            Ok(module) => Some(module),
            Err(ProcMemError::ModuleNotFound) => None,
            Err(e) => {
                eprintln!("Error obtaining mono dll: {:?}", e);
                None
            }
        }
        .unwrap();

        // println!("mono-2.0-bdwgc.dll Base addr: {:?}", module.base_address());

        let pe = pe_reader::PEReader::new(module.data().to_vec());
        let mono_root_offset = pe.get_function_offset("mono_get_root_domain").unwrap();

        // println!(
        //     "mono_root_domain addr: {:?}",
        //     module.base_address() + mono_root_offset as usize
        // );

        self.mono_root_domain = module.base_address() + mono_root_offset as usize;

        self.mono_root_domain
    }

    pub fn create_type_definitions(&mut self) {
        // let type_definitions = Vec::new();

        let class_cache_size = self.read_u32(
            self.assembly_image_address
                + (crate::constants::IMAGE_CLASS_CACHE + crate::constants::HASH_TABLE_SIZE)
                    as usize,
        );
        let class_cache_table_array = self.read_ptr(
            self.assembly_image_address
                + (crate::constants::IMAGE_CLASS_CACHE + crate::constants::HASH_TABLE_TABLE)
                    as usize,
        );

        println!("Class cache size: {:?}", class_cache_size);

        println!("Class cache table array: {:?}", class_cache_table_array);

        let mut table_item = 0;
        println!(
            "Class cache size: {:?}",
            class_cache_size * crate::constants::SIZE_OF_PTR as u32
        );

        while table_item < (class_cache_size * crate::constants::SIZE_OF_PTR as u32) {
            //
            let mut definition = self.read_ptr(class_cache_table_array + table_item as usize);

            // If pointer is not null ?
            while definition != 0 {
                definition = self.read_ptr(
                    definition + crate::constants::TYPE_DEFINITION_NEXT_CLASS_CACHE as usize,
                );
                if definition != 0 {
                    // let ptr = self.read_ptr(definition + crate::constants::TYPE_DEFINITION_NAME as usize);
                    // let name = self.read_ascii_string(ptr);
                    // println!("Name: {:?}", name);

                    let type_def = TypeDefinition::new(definition, self);
                    self.type_definitions
                        .push((definition, type_def.clone() as TypeDefinition));
                    let name = type_def.name.clone();
                    println!("Name: {:?}", name);
                }
                // add definition
            }

            table_item += crate::constants::SIZE_OF_PTR as u32;
        }
    }

    pub fn read_assembly_image(&mut self) -> usize {
        let offset = self
            .read_i32(self.mono_root_domain + crate::constants::RIP_PLUS_OFFSET_OFFSET)
            + crate::constants::RIP_VALUE_OFFSET as i32;

        println!("offset: {:?}", offset);

        let domain = self.read_ptr(self.mono_root_domain + offset as usize);

        println!("Domain address: {:?}", domain);

        let assembly_array_address =
            self.read_ptr(domain + crate::constants::REFERENCED_ASSEMBLIES as usize);

        let mut assembly_address = assembly_array_address;

        while assembly_address != 0 {
            let assembly = self.read_ptr(assembly_address);

            let assembly_name_address =
                self.read_ptr(assembly + (crate::constants::SIZE_OF_PTR * 2 as usize));

            let assembly_name = self.read_ascii_string(assembly_name_address);

            if assembly_name == "Assembly-CSharp" {
                println!("Assembly name: {:?}", assembly_name);
                println!("  - {:?}", assembly_name_address);
                self.assembly_image_address =
                    self.read_ptr(assembly + crate::constants::ASSEMBLY_IMAGE as usize);
                return self.assembly_image_address;
            }

            assembly_address =
                self.read_ptr(assembly_address + crate::constants::SIZE_OF_PTR as usize);
        }

        return self.assembly_image_address;
    }

    pub fn read_u8(&self, addr: usize) -> u8 {
        let mut member = DataMember::<u8>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_u16(&self, addr: usize) -> u16 {
        let mut member = DataMember::<u16>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_u32(&self, addr: usize) -> u32 {
        let mut member = DataMember::<u32>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_u64(&self, addr: usize) -> u64 {
        let mut member = DataMember::<u64>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_i8(&self, addr: usize) -> i8 {
        let mut member = DataMember::<i8>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_i16(&self, addr: usize) -> i16 {
        let mut member = DataMember::<i16>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_i32(&self, addr: usize) -> i32 {
        let mut member = DataMember::<i32>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_i64(&self, addr: usize) -> i64 {
        let mut member = DataMember::<i64>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_ptr(&self, addr: usize) -> usize {
        let mut member = DataMember::<usize>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => val,
                Err(_e) => {
                    eprintln!("Error: {:?}", std::io::Error::last_os_error());
                    0
                }
            }
        };

        return val;
    }

    pub fn read_ascii_string(&self, addr: usize) -> String {
        let mut string = String::new();
        let mut index = addr;
        loop {
            let val = self.read_u8(index);
            if val == 0 {
                break;
            }
            string.push(val as char);
            index += 1;
        }
        string
    }

    pub fn read_ptr_u8(&self, addr: usize) -> u8 {
        let ptr = self.read_ptr(addr);
        self.read_u8(ptr)
    }

    pub fn read_ptr_u16(&self, addr: usize) -> u16 {
        let ptr = self.read_ptr(addr);
        self.read_u16(ptr)
    }

    pub fn read_ptr_u32(&self, addr: usize) -> u32 {
        let ptr = self.read_ptr(addr);
        self.read_u32(ptr)
    }

    pub fn read_ptr_u64(&self, addr: usize) -> u64 {
        let ptr = self.read_ptr(addr);
        self.read_u64(ptr)
    }

    pub fn read_ptr_i8(&self, addr: usize) -> i8 {
        let ptr = self.read_ptr(addr);
        self.read_i8(ptr)
    }

    pub fn read_ptr_i16(&self, addr: usize) -> i16 {
        let ptr = self.read_ptr(addr);
        self.read_i16(ptr)
    }

    pub fn read_ptr_i32(&self, addr: usize) -> i32 {
        let ptr = self.read_ptr(addr);
        self.read_i32(ptr)
    }

    pub fn read_ptr_i64(&self, addr: usize) -> i64 {
        let ptr = self.read_ptr(addr);
        self.read_i64(ptr)
    }

    pub fn read_ptr_ascii_string(&self, addr: usize) -> String {
        let ptr = self.read_ptr(addr);
        self.read_ascii_string(ptr)
    }

    pub fn read_ptr_ptr(&self, addr: usize) -> usize {
        let ptr = self.read_ptr(addr);
        self.read_ptr(ptr)
    }
}

#[derive(Clone)]
pub struct TypeDefinition {
    bit_fields: u32,
    field_count: i32,
    // lazy_parent: usize,
    // lazy_nested_in: usize,
    // lazy_full_name: usize,
    // lazy_fields: usize,
    // lazy_generic: usize,
    name: String,
    namespace_name: String,
    size: i32,
    v_table: usize,
    v_table_size: i32,
    type_info: TypeInfo,
    class_kind: u32,
}

impl TypeDefinition {
    pub fn new(definition_addr: usize, reader: &MonoReader) -> Self {
        let bit_fields = reader
            .read_u32(definition_addr + crate::constants::TYPE_DEFINITION_BIT_FIELDS as usize);

        let field_count = reader
            .read_i32(definition_addr + crate::constants::TYPE_DEFINITION_FIELD_COUNT as usize);

        // let lazy_parent = 0;
        // let lazy_nested_in = 0;
        // let lazy_full_name = 0;
        // let lazy_fields = 0;
        // let lazy_generic = 0;

        let name = reader.read_ptr_ascii_string(
            definition_addr + crate::constants::TYPE_DEFINITION_NAME as usize,
        );

        let namespace_name = reader.read_ptr_ascii_string(
            definition_addr + crate::constants::TYPE_DEFINITION_NAMESPACE as usize,
        );

        let size =
            reader.read_i32(definition_addr + crate::constants::TYPE_DEFINITION_SIZE as usize);

        let vtable_ptr = reader
            .read_ptr(definition_addr + crate::constants::TYPE_DEFINITION_RUNTIME_INFO as usize);

        let v_table = if vtable_ptr != 0 {
            reader.read_ptr(
                vtable_ptr
                    + crate::constants::TYPE_DEFINITION_RUNTIME_INFO_DOMAIN_V_TABLES as usize,
            )
        } else {
            0
        };

        let v_table_size = if v_table != 0 {
            reader.read_i32(v_table + crate::constants::TYPE_DEFINITION_V_TABLE_SIZE as usize)
        } else {
            0
        };

        let type_info = TypeInfo::new(
            definition_addr + crate::constants::TYPE_DEFINITION_BY_VAL_ARG as usize,
            reader,
        );
        let class_kind = 0;

        TypeDefinition {
            bit_fields,
            field_count,
            name,
            namespace_name,
            size,
            v_table,
            v_table_size,
            type_info,
            class_kind,
        }
    }
}

#[derive(Clone)]
pub struct TypeInfo {
    data: usize,
    attrs: u32,
    is_static: bool,
    is_const: bool,
    type_code: u32,
}

impl TypeInfo {
    fn new(addr: usize, reader: &MonoReader) -> Self {
        let data = reader.read_ptr(addr);
        let attrs = reader.read_u32(addr + crate::constants::SIZE_OF_PTR);
        let is_static = (attrs & 0x1) != 0;
        let is_const = (attrs & 0x4) != 0;
        let type_code = 0xff & (attrs >> 16);

        TypeInfo {
            data,
            attrs,
            is_static,
            is_const,
            type_code,
        }
    }
}
