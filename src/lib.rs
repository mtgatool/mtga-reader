use core::fmt;
use core::fmt::Debug;
use core::fmt::Formatter;
use proc_mem::{ProcMemError, Process};
use process_memory::{DataMember, Memory, ProcessHandle, TryIntoProcessHandle};
use std::fmt::Display;

pub mod constants;

mod pe_reader;

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

        let pe = pe_reader::PEReader::new(module.data().to_vec());
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
                + (crate::constants::IMAGE_CLASS_CACHE + crate::constants::HASH_TABLE_SIZE)
                    as usize,
        );
        let class_cache_table_array = self.read_ptr(
            self.assembly_image_address
                + (crate::constants::IMAGE_CLASS_CACHE + crate::constants::HASH_TABLE_TABLE)
                    as usize,
        );

        // println!("Class cache size: {:?}", class_cache_size);

        // println!("Class cache table array: {:?}", class_cache_table_array);

        let mut table_item = 0;
        // println!(
        //     "Class cache size: {:?}",
        //     class_cache_size * crate::constants::SIZE_OF_PTR as u32
        // );

        let mut type_defs: Vec<usize> = Vec::new();

        while table_item < (class_cache_size * crate::constants::SIZE_OF_PTR as u32) {
            //
            let mut definition = self.read_ptr(class_cache_table_array + table_item as usize);

            // If pointer is not null ?
            while definition != 0 {
                definition = self.read_ptr(
                    definition + crate::constants::TYPE_DEFINITION_NEXT_CLASS_CACHE as usize,
                );
                if definition != 0 {
                    // add its address to the list
                    type_defs.push(definition);
                }
            }

            table_item += crate::constants::SIZE_OF_PTR as u32;
        }

        return type_defs;
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
pub enum MonoClassKind {
    Def = 1,
    GTg = 2,
    GInst = 3,
    GParam = 4,
    Array = 5,
    Pointer = 6,
}

impl Debug for MonoClassKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MonoClassKind::Def => write!(f, "Def"),
            MonoClassKind::GTg => write!(f, "GTg"),
            MonoClassKind::GInst => write!(f, "GInst"),
            MonoClassKind::GParam => write!(f, "GParam"),
            MonoClassKind::Array => write!(f, "Array"),
            MonoClassKind::Pointer => write!(f, "Pointer"),
        }
    }
}

fn match_class_kind(value: u8) -> MonoClassKind {
    match value {
        1 => MonoClassKind::Def,
        2 => MonoClassKind::GTg,
        3 => MonoClassKind::GInst,
        4 => MonoClassKind::GParam,
        5 => MonoClassKind::Array,
        6 => MonoClassKind::Pointer,
        _ => MonoClassKind::Def,
    }
}

pub enum TypeCode {
    END = 0x00, /* End of List */
    VOID = 0x01,
    BOOLEAN = 0x02,

    // [Description("char")]
    CHAR = 0x03,

    // [Description("byte")]
    I1 = 0x04,

    // [Description("sbyte")]
    U1 = 0x05,

    // [Description("short")]
    I2 = 0x06,

    // [Description("ushort")]
    U2 = 0x07,

    // [Description("int")]
    I4 = 0x08,

    // [Description("uint")]
    U4 = 0x09,

    // [Description("long")]
    I8 = 0x0a,

    // [Description("ulong")]
    U8 = 0x0b,

    // [Description("float")]
    R4 = 0x0c,

    // [Description("double")]
    R8 = 0x0d,

    // [Description("string")]
    STRING = 0x0e,

    PTR = 0x0f,         /* arg: <type> token */
    BYREF = 0x10,       /* arg: <type> token */
    VALUETYPE = 0x11,   /* arg: <type> token */
    CLASS = 0x12,       /* arg: <type> token */
    VAR = 0x13,         /* number */
    ARRAY = 0x14,       /* type, rank, boundsCount, bound1, loCount, lo1 */
    GENERICINST = 0x15, /* <type> <type-arg-count> <type-1> \x{2026} <type-n> */
    TYPEDBYREF = 0x16,

    // [Description("int")]
    I = 0x18,

    // [Description("uint")]
    U = 0x19,

    FNPTR = 0x1b, /* arg: full method signature */
    OBJECT = 0x1c,
    SZARRAY = 0x1d,   /* 0-based one-dim-array */
    MVAR = 0x1e,      /* number */
    CMOD_REQD = 0x1f, /* arg: typedef or typeref token */
    CMOD_OPT = 0x20,  /* optional arg: typedef or typref token */
    INTERNAL = 0x21,  /* CLR internal type */
    MODIFIER = 0x40,  /* Or with the following types */
    SENTINEL = 0x41,  /* Sentinel for varargs method signature */
    PINNED = 0x45,    /* Local var that points to pinned object */
    ENUM = 0x55,      /* an enumeration */
}

impl Display for TypeCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TypeCode::END => write!(f, "END"),
            TypeCode::VOID => write!(f, "VOID"),
            TypeCode::BOOLEAN => write!(f, "BOOLEAN"),
            TypeCode::CHAR => write!(f, "CHAR"),
            TypeCode::I1 => write!(f, "I1"),
            TypeCode::U1 => write!(f, "U1"),
            TypeCode::I2 => write!(f, "I2"),
            TypeCode::U2 => write!(f, "U2"),
            TypeCode::I4 => write!(f, "I4"),
            TypeCode::U4 => write!(f, "U4"),
            TypeCode::I8 => write!(f, "I8"),
            TypeCode::U8 => write!(f, "U8"),
            TypeCode::R4 => write!(f, "R4"),
            TypeCode::R8 => write!(f, "R8"),
            TypeCode::STRING => write!(f, "STRING"),
            TypeCode::PTR => write!(f, "PTR"),
            TypeCode::BYREF => write!(f, "BYREF"),
            TypeCode::VALUETYPE => write!(f, "VALUETYPE"),
            TypeCode::CLASS => write!(f, "CLASS"),
            TypeCode::VAR => write!(f, "VAR"),
            TypeCode::ARRAY => write!(f, "ARRAY"),
            TypeCode::GENERICINST => write!(f, "GENERICINST"),
            TypeCode::TYPEDBYREF => write!(f, "TYPEDBYREF"),
            TypeCode::I => write!(f, "I"),
            TypeCode::U => write!(f, "U"),
            TypeCode::FNPTR => write!(f, "FNPTR"),
            TypeCode::OBJECT => write!(f, "OBJECT"),
            TypeCode::SZARRAY => write!(f, "SZARRAY"),
            TypeCode::MVAR => write!(f, "MVAR"),
            TypeCode::CMOD_REQD => write!(f, "CMOD_REQD"),
            TypeCode::CMOD_OPT => write!(f, "CMOD_OPT"),
            TypeCode::INTERNAL => write!(f, "INTERNAL"),
            TypeCode::MODIFIER => write!(f, "MODIFIER"),
            TypeCode::SENTINEL => write!(f, "SENTINEL"),
            TypeCode::PINNED => write!(f, "PINNED"),
            TypeCode::ENUM => write!(f, "ENUM"),
            _ => write!(f, "UNKNOWN"),
        }
    }
}

pub struct TypeDefinition<'a> {
    reader: &'a MonoReader,
    address: usize,
    pub bit_fields: u32,
    pub field_count: i32,
    // lazy_parent: usize,
    // lazy_nested_in: usize,
    // lazy_full_name: usize,
    // lazy_fields: usize,
    // lazy_generic: usize,
    pub name: String,
    pub namespace_name: String,
    pub size: i32,
    pub v_table: usize,
    pub v_table_size: i32,
    pub type_info: TypeInfo,
    pub class_kind: MonoClassKind,
}

impl<'a> TypeDefinition<'a> {
    pub fn new(definition_addr: usize, reader: &'a MonoReader) -> Self {
        let bit_fields = reader
            .read_u32(definition_addr + crate::constants::TYPE_DEFINITION_BIT_FIELDS as usize);

        let field_count = reader
            .read_i32(definition_addr + crate::constants::TYPE_DEFINITION_FIELD_COUNT as usize);

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
            &reader,
        );

        let class_kind_value =
            reader.read_u8(definition_addr + crate::constants::TYPE_DEFINITION_CLASS_KIND as usize);
        let class_kind = match_class_kind(class_kind_value);

        TypeDefinition {
            address: definition_addr,
            reader,
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

    pub fn get_fields(&self) -> Vec<usize> {
        let first_field = self
            .reader
            .read_ptr(self.address + crate::constants::TYPE_DEFINITION_FIELDS as usize);

        let mut fields = Vec::new();

        if first_field == 0 {
            return fields;
        } else {
            for field_index in 0..self.field_count {
                let field = first_field
                    + (field_index as usize
                        * crate::constants::TYPE_DEFINITION_FIELD_SIZE as usize);
                let ptr = self.reader.read_ptr(field);
                if ptr == 0 {
                    continue;
                }
                fields.push(field);
            }
        }

        return fields;
    }
}

#[derive(Clone)]
pub struct TypeInfo {
    pub data: usize,
    pub attrs: u32,
    pub is_static: bool,
    pub is_const: bool,
    pub type_code: u32,
}

impl TypeInfo {
    fn new(addr: usize, reader: &MonoReader) -> Self {
        let data = reader.read_ptr(addr);
        let attrs = reader.read_u32(addr + crate::constants::SIZE_OF_PTR);
        let is_static = (attrs & 0x10) == 0x10;
        let is_const = (attrs & 0x40) == 0x40;
        let type_code = 0xff & (attrs >> 16);

        TypeInfo {
            data,
            attrs,
            is_static,
            is_const,
            type_code,
        }
    }

    pub fn code(self) -> TypeCode {
        // return the appropiate TypeCode enum based on self.type_code
        match self.type_code {
            0x00 => TypeCode::END,
            0x01 => TypeCode::VOID,
            0x02 => TypeCode::BOOLEAN,
            0x03 => TypeCode::CHAR,
            0x04 => TypeCode::I1,
            0x05 => TypeCode::U1,
            0x06 => TypeCode::I2,
            0x07 => TypeCode::U2,
            0x08 => TypeCode::I4,
            0x09 => TypeCode::U4,
            0x0a => TypeCode::I8,
            0x0b => TypeCode::U8,
            0x0c => TypeCode::R4,
            0x0d => TypeCode::R8,
            0x0e => TypeCode::STRING,
            0x0f => TypeCode::PTR,
            0x10 => TypeCode::BYREF,
            0x11 => TypeCode::VALUETYPE,
            0x12 => TypeCode::CLASS,
            0x13 => TypeCode::VAR,
            0x14 => TypeCode::ARRAY,
            0x15 => TypeCode::GENERICINST,
            0x16 => TypeCode::TYPEDBYREF,
            0x18 => TypeCode::I,
            0x19 => TypeCode::U,
            0x1b => TypeCode::FNPTR,
            0x1c => TypeCode::OBJECT,
            0x1d => TypeCode::SZARRAY,
            0x1e => TypeCode::MVAR,
            0x1f => TypeCode::CMOD_REQD,
            0x20 => TypeCode::CMOD_OPT,
            0x21 => TypeCode::INTERNAL,
            0x40 => TypeCode::MODIFIER,
            0x41 => TypeCode::SENTINEL,
            0x45 => TypeCode::PINNED,
            0x55 => TypeCode::ENUM,
            _ => TypeCode::END,
        }
    }
}

pub struct FieldDefinition {
    pub type_info: TypeInfo,
    pub name: String,
    pub offset: i32,
}

impl FieldDefinition {
    pub fn new(addr: usize, reader: &MonoReader) -> Self {
        let type_ptr = reader.read_ptr(addr);
        let type_info = TypeInfo::new(type_ptr, reader);

        let name = reader.read_ptr_ascii_string(addr + crate::constants::SIZE_OF_PTR as usize);

        let offset = reader.read_i32(addr + crate::constants::SIZE_OF_PTR * 3 as usize);

        FieldDefinition {
            type_info,
            name,
            offset,
        }
    }
}
