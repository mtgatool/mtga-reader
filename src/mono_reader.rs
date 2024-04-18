use proc_mem::{ProcMemError, Process};
use process_memory::{DataMember, Memory, ProcessHandle, TryIntoProcessHandle};

use crate::constants;
use crate::pe_reader::PEReader;

pub struct MonoReader {
    pid: u32,
    handle: ProcessHandle,
    mono_root_domain: usize,
    assembly_image_address: usize,
}

impl MonoReader {
    pub fn new(pid: u32) -> Self {
        let handle = pid.try_into_process_handle().unwrap();
        MonoReader {
            pid,
            handle,
            mono_root_domain: 0,
            assembly_image_address: 0,
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

        let pe = PEReader::new(module.data().to_vec());
        let mono_root_offset = pe.get_function_offset("mono_get_root_domain").unwrap();

        // println!(
        //     "mono_root_domain addr: {:?}",
        //     module.base_address() + mono_root_offset as usize
        // );

        self.mono_root_domain = module.base_address() + mono_root_offset as usize;

        self.mono_root_domain
    }

    pub fn create_type_definitions(&mut self) -> Vec<usize> {
        // let type_definitions = Vec::new();

        let class_cache_size = self.read_u32(
            self.assembly_image_address
                + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_SIZE) as usize,
        );
        let class_cache_table_array = self.read_ptr(
            self.assembly_image_address
                + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize,
        );

        // println!("Class cache size: {:?}", class_cache_size);

        // println!("Class cache table array: {:?}", class_cache_table_array);

        let mut table_item = 0;
        // println!(
        //     "Class cache size: {:?}",
        //     class_cache_size * constants::SIZE_OF_PTR as u32
        // );

        let mut type_defs: Vec<usize> = Vec::new();

        while table_item < (class_cache_size * constants::SIZE_OF_PTR as u32) {
            //
            let mut definition = self.read_ptr(class_cache_table_array + table_item as usize);

            // If pointer is not null ?
            while definition != 0 {
                definition = self
                    .read_ptr(definition + constants::TYPE_DEFINITION_NEXT_CLASS_CACHE as usize);
                if definition != 0 {
                    // add its address to the list
                    type_defs.push(definition);
                }
            }

            table_item += constants::SIZE_OF_PTR as u32;
        }

        return type_defs;
    }

    pub fn read_assembly_image(&mut self) -> usize {
        let offset = self.read_i32(self.mono_root_domain + constants::RIP_PLUS_OFFSET_OFFSET)
            + constants::RIP_VALUE_OFFSET as i32;

        println!("offset: {:?}", offset);

        let domain = self.read_ptr(self.mono_root_domain + offset as usize);

        println!("Domain address: {:?}", domain);

        let assembly_array_address =
            self.read_ptr(domain + constants::REFERENCED_ASSEMBLIES as usize);

        let mut assembly_address = assembly_array_address;

        while assembly_address != 0 {
            let assembly = self.read_ptr(assembly_address);

            let assembly_name_address =
                self.read_ptr(assembly + (constants::SIZE_OF_PTR * 2 as usize));

            let assembly_name = self.read_ascii_string(assembly_name_address);

            if assembly_name == "Assembly-CSharp" {
                println!("Assembly name: {:?}", assembly_name);
                println!("  - {:?}", assembly_name_address);
                self.assembly_image_address =
                    self.read_ptr(assembly + constants::ASSEMBLY_IMAGE as usize);
                return self.assembly_image_address;
            }

            assembly_address = self.read_ptr(assembly_address + constants::SIZE_OF_PTR as usize);
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

    // All read_ methods should be wrapping a maybe_ method
    // Ideally we should only use the maybe_read_ methods
    pub fn maybe_read_u32(&self, addr: usize) -> Option<u32> {
        let mut member = DataMember::<u32>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => Some(val),
                Err(_e) => None,
            }
        };

        return val;
    }

    pub fn read_u32(&self, addr: usize) -> u32 {
        let val = match self.maybe_read_u32(addr) {
            Some(val) => val,
            None => {
                eprintln!("Error: {:?}", std::io::Error::last_os_error());
                0
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