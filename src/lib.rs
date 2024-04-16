use core::fmt;
use core::fmt::Debug;
use core::fmt::Formatter;
use proc_mem::{ProcMemError, Process};
use process_memory::{DataMember, Memory, ProcessHandle, TryIntoProcessHandle};
use std::cmp;
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

pub struct Managed<'a> {
    reader: &'a MonoReader,
    addr: usize,
    generic_type_args: Vec<TypeInfo>,
}

impl<'a> Managed<'a> {
    pub fn new(
        reader: &'a MonoReader,
        addr: usize,
        generic_type_args: Option<Vec<TypeInfo>>,
    ) -> Self {
        Managed {
            reader,
            addr,
            generic_type_args: generic_type_args.unwrap_or(Vec::new()),
        }
    }

    pub fn read_boolean(&self) -> bool {
        self.reader.read_u8(self.addr) != 0x0
    }

    pub fn read_char(&self) -> char {
        self.reader.read_u16(self.addr).to_string().parse().unwrap()
    }

    // read_u
    pub fn read_u4(&self) -> u16 {
        self.reader.read_u16(self.addr)
    }

    pub fn read_r4(&self) -> i32 {
        self.reader.read_i32(self.addr)
    }

    pub fn read_r8(&self) -> i64 {
        self.reader.read_i64(self.addr)
    }

    // read_i
    pub fn read_i4(&self) -> u32 {
        self.reader.read_u32(self.addr)
    }

    pub fn read_i2(&self) -> i16 {
        self.reader.read_i16(self.addr)
    }

    pub fn read_u2(&self) -> u16 {
        self.reader.read_u16(self.addr)
    }

    pub fn read_string(&self) -> String {
        let ptr = self.reader.read_ptr(self.addr);
        let length = self
            .reader
            .read_u32(ptr + (crate::constants::SIZE_OF_PTR * 2 as usize));

        let mut str = Vec::new();

        let cap_length = cmp::min(length as u64 * 2, 1024);

        for i in 0..(cap_length) {
            let val = self
                .reader
                .read_u16(ptr + (crate::constants::SIZE_OF_PTR * 2 as usize) + 4 + (i as usize));

            str.push(val);
        }

        // Convert the vector to a string
        let string: String = str.iter().map(|&c| c as u8 as char).collect::<String>();

        return string;
    }

    pub fn read_valuetype(&self) -> i32 {
        self.reader.read_i32(self.addr)
    }

    pub fn read_class_address(&self) -> usize {
        let ptr = self.reader.read_ptr(self.addr);
        let vtable = self.reader.read_ptr(ptr);
        let definition_addr = self.reader.read_ptr(vtable);

        // println!("ptr: {:?}", ptr);
        // println!("self.addr: {:?}", self.addr);
        // println!("vtable: {:?}", vtable);
        // println!("definition_addr: {:?}", definition_addr);

        return definition_addr;
    }

    pub fn read_class(&self) -> TypeDefinition {
        let address = self.read_class_address();
        return TypeDefinition::new(address, self.reader);
    }

    pub fn read_raw_class(&self) -> TypeDefinition {
        return TypeDefinition::new(self.addr, self.reader);
    }

    pub fn read_generic_instance(&self, type_info: TypeInfo) -> TypeDefinition {
        let ptr = self.reader.read_ptr(type_info.data);
        let td = TypeDefinition::new(ptr, self.reader);

        if td.is_value_type {
            println!("Generic instance {}", self.addr);
            return TypeDefinition::new(self.addr, self.reader);
        }

        return td;
    }

    // pub fn read_managed_array<T>(&self) -> Option<T>

    pub fn read_managed_array(&self) -> Option<Vec<String>> {
        let ptr = self.reader.read_ptr(self.addr);
        if ptr == 0 {
            return None;
        }

        let vtable = self.reader.read_ptr(ptr);

        let array_definition_ptr = self.reader.read_ptr(vtable);

        let array_definition = TypeDefinition::new(array_definition_ptr, self.reader);

        let element_definition =
            TypeDefinition::new(self.reader.read_ptr(array_definition_ptr), self.reader);

        let count = 1 as i32;
        // self
        // .reader
        // .read_i32(ptr + (crate::constants::SIZE_OF_PTR * 3));

        let start = ptr + crate::constants::SIZE_OF_PTR * 4;

        let mut result = Vec::new();

        println!("Array ptr: {:?}", ptr);
        println!("Array vtable: {:?}", vtable);
        println!(
            "Array array_definition.address: {:?}",
            array_definition.address
        );
        println!(
            "Array element_definition.address: {:?}",
            element_definition.address
        );
        println!(
            "Array element_definition type: {}",
            element_definition.type_info.clone().code()
        );

        let code = element_definition.type_info.clone().code();

        for i in 0..count {
            let managed = Managed::new(
                self.reader,
                start + (i as usize * array_definition.size as usize),
                Some(element_definition.generic_type_args.clone()),
            );

            let strout = match code {
                TypeCode::CLASS => managed.read_class().to_string(),
                TypeCode::GENERICINST => {
                    let m = managed.read_generic_instance(element_definition.type_info.clone());

                    String::from("")
                }
                _ => {
                    // println!("Code: {} strout not implemented", code);
                    String::from("{}")
                }
            };

            result.push(strout);
        }

        return Some(result);
    }

    pub fn read_var(&self) -> u32 {
        let ptr = self.reader.read_u32(self.addr);

        return ptr;
    }

    // pub fn read_managed_struct_instance<T>(&self) -> Option<T>

    // pub fn read_managed_class_instance<T>(&self) -> Option<T>

    // pub fn read_managed_generic_object<T>(&self) -> Option<T>

    // pub fn read_managed_var<T>(&self) -> Option<T>

    // pub fn read_managed_object<T>(&self) -> Option<T>
}

/*
pub fn read_managed<T>(type_code: TypeCode) -> Option<T> {
    match type_code {
        // 1, b => b[0] != 0
        TypeCode::BOOLEAN => Some(self.read_ptr_u8(addr) != 0),

        // char -> char
        TypeCode::CHAR => Some(self.read_ptr_u16(addr)),

        // sizeof(byte), b => b[0]
        TypeCode::I1 => Some(self.read_ptr_i8(addr)),

        // sizeof(sbyte), b => unchecked((sbyte)b[0])
        TypeCode::U1 => Some(self.read_ptr_u8(addr)),

        // short size -> int16
        TypeCode::I2 => Some(self.read_ptr_i16(addr)),

        // ushort size -> uint16
        TypeCode::U2 => Some(self.read_ptr_u16(addr)),

        // int32
        TypeCode::I => Some(self.read_i32(addr)),
        TypeCode::I4 => Some(self.read_i32(addr)),

        // unsigned int32
        TypeCode::U => Some(self.read_u32(addr)),
        TypeCode::U4 => Some(self.read_u32(addr)),

        // char size -> int64
        TypeCode::I8 => Some(self.read_ptr_i64(addr)),

        // char size -> uint64
        TypeCode::U8 => Some(self.read_ptr_u64(addr)),

        // char size -> single
        TypeCode::R4 => Some(self.read_ptr_u32(addr)),
        // char size -> double
        TypeCode::R8 => Some(self.read_i64(addr)),

        TypeCode::STRING => Some(self.read_ascii_string(addr)),

        // ReadManagedArray
        TypeCode::SZARRAY => Some(self.read_ptr_ptr(addr)),

        // try ReadManagedStructInstance
        TypeCode::VALUETYPE => Some(self.read_i32(addr)),

        // ReadManagedClassInstance
        TypeCode::CLASS => Some(self.read_ptr_ptr(addr)),

        // ReadManagedGenericObject
        TypeCode::GENERICINST => Some(self.read_ptr_ptr(addr)),

        // ReadManagedGenericObject
        TypeCode::OBJECT => Some(self.read_ptr_ptr(addr)),

        // ReadManagedVar
        TypeCode::VAR => Some(self.read_ptr_i32(addr)),

        // Junk
        TypeCode::END => Some(0),
        TypeCode::VOID => Some(0),
        TypeCode::PTR => Some(0),
        TypeCode::BYREF => Some(0),
        TypeCode::TYPEDBYREF => Some(0),
        TypeCode::FNPTR => Some(0),
        TypeCode::CMOD_REQD => Some(0),
        TypeCode::CMOD_OPT => Some(0),
        TypeCode::INTERNAL => Some(0),
        TypeCode::MODIFIER => Some(0),
        TypeCode::SENTINEL => Some(0),
        TypeCode::PINNED => Some(0),

        // May need support
        TypeCode::ARRAY => Some(0),
        TypeCode::ENUM => Some(0),
        TypeCode::MVAR => Some(0),
        _ => None,
    }
}
*/

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
    END = 0x00,
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
    SZARRAY = 0x1d,  /* 0-based one-dim-array */
    MVAR = 0x1e,     /* number */
    CMODREQD = 0x1f, /* arg: typedef or typeref token */
    CMODOPT = 0x20,  /* optional arg: typedef or typref token */
    INTERNAL = 0x21, /* CLR internal type */
    MODIFIER = 0x40, /* Or with the following types */
    SENTINEL = 0x41, /* Sentinel for varargs method signature */
    PINNED = 0x45,   /* Local var that points to pinned object */
    ENUM = 0x55,     /* an enumeration */
}

impl Display for TypeCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TypeCode::END => write!(f, "END"),
            TypeCode::VOID => write!(f, "VOID"),
            TypeCode::BOOLEAN => write!(f, "BOOLEAN"),
            TypeCode::CHAR => write!(f, "CHAR"),
            TypeCode::I1 => write!(f, "BYTE (I1)"),
            TypeCode::U1 => write!(f, "UBYTE (U1)"),
            TypeCode::I2 => write!(f, "SHORT (I2)"),
            TypeCode::U2 => write!(f, "USHORT (U2)"),
            TypeCode::I4 => write!(f, "INT (I4)"),
            TypeCode::U4 => write!(f, "UINT (U4)"),
            TypeCode::I8 => write!(f, "LONG (I8)"),
            TypeCode::U8 => write!(f, "ULONG (U8)"),
            TypeCode::R4 => write!(f, "FLOAT (R4)"),
            TypeCode::R8 => write!(f, "DOUBLE (R8)"),
            TypeCode::STRING => write!(f, "STRING"),
            TypeCode::PTR => write!(f, "PTR"),
            TypeCode::BYREF => write!(f, "BYREF"),
            TypeCode::VALUETYPE => write!(f, "VALUETYPE"),
            TypeCode::CLASS => write!(f, "CLASS"),
            TypeCode::VAR => write!(f, "VAR"),
            TypeCode::ARRAY => write!(f, "ARRAY"),
            TypeCode::GENERICINST => write!(f, "GENERICINST"),
            TypeCode::TYPEDBYREF => write!(f, "TYPEDBYREF"),
            TypeCode::I => write!(f, "INT (I)"),
            TypeCode::U => write!(f, "UINT (U)"),
            TypeCode::FNPTR => write!(f, "FNPTR"),
            TypeCode::OBJECT => write!(f, "OBJECT"),
            TypeCode::SZARRAY => write!(f, "SZARRAY"),
            TypeCode::MVAR => write!(f, "MVAR"),
            TypeCode::CMODREQD => write!(f, "CMOD_REQD"),
            TypeCode::CMODOPT => write!(f, "CMOD_OPT"),
            TypeCode::INTERNAL => write!(f, "INTERNAL"),
            TypeCode::MODIFIER => write!(f, "MODIFIER"),
            TypeCode::SENTINEL => write!(f, "SENTINEL"),
            TypeCode::PINNED => write!(f, "PINNED"),
            TypeCode::ENUM => write!(f, "ENUM"),
        }
    }
}

fn get_type_size(type_code: TypeCode) -> usize {
    match type_code {
        TypeCode::BOOLEAN => 1,
        TypeCode::CHAR => 2,
        TypeCode::I1 => 1,
        TypeCode::U1 => 1,
        TypeCode::I2 => 2,
        TypeCode::U2 => 2,
        TypeCode::I4 => 4,
        TypeCode::U4 => 4,
        TypeCode::I8 => 8,
        TypeCode::U8 => 8,
        TypeCode::R4 => 4,
        TypeCode::R8 => 8,
        TypeCode::PTR => 4,
        TypeCode::BYREF => 4,
        TypeCode::VALUETYPE => 4,
        TypeCode::CLASS => 4,
        TypeCode::VAR => 4,
        TypeCode::ARRAY => 4,
        TypeCode::GENERICINST => 4,
        TypeCode::TYPEDBYREF => 4,
        TypeCode::I => 4,
        TypeCode::U => 4,
        TypeCode::FNPTR => 4,
        TypeCode::OBJECT => 4,
        TypeCode::SZARRAY => 4,
        TypeCode::MVAR => 4,
        TypeCode::CMODREQD => 4,
        TypeCode::CMODOPT => 4,
        TypeCode::INTERNAL => 4,
        TypeCode::MODIFIER => 4,
        TypeCode::SENTINEL => 4,
        TypeCode::PINNED => 4,
        TypeCode::ENUM => 4,
        _ => 0,
    }
}

pub struct TypeDefinition<'a> {
    reader: &'a MonoReader,
    address: usize,
    pub bit_fields: u32,
    pub field_count: i32,
    pub parent_addr: usize,
    pub nested_in_addr: usize,
    pub name: String,
    pub namespace_name: String,
    pub size: i32,
    pub vtable_ptr: usize,
    pub v_table: usize,
    pub v_table_size: i32,
    pub type_info: TypeInfo,
    pub class_kind: MonoClassKind,
    pub is_enum: bool,
    pub is_value_type: bool,
    pub generic_type_args: Vec<TypeInfo>,
}

impl<'a> TypeDefinition<'a> {
    pub fn new(definition_addr: usize, reader: &'a MonoReader) -> Self {
        let bit_fields = reader
            .read_u32(definition_addr + crate::constants::TYPE_DEFINITION_BIT_FIELDS as usize);

        let is_enum = (bit_fields & 0x8) == 0x8;

        let is_value_type = (bit_fields & 0x4) == 0x4;

        let field_count = reader
            .read_i32(definition_addr + crate::constants::TYPE_DEFINITION_FIELD_COUNT as usize);

        let nested_in_addr =
            reader.read_ptr(definition_addr + crate::constants::TYPE_DEFINITION_NESTED_IN as usize);

        let parent_addr =
            reader.read_ptr(definition_addr + crate::constants::TYPE_DEFINITION_PARENT as usize);

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
            reader
                .read_i32(definition_addr + crate::constants::TYPE_DEFINITION_V_TABLE_SIZE as usize)
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

        // Get the generic type arguments
        let mut generic_type_args = Vec::new();
        let code = type_info.clone().code();

        println!("Type code: {}", code);

        match code {
            TypeCode::GENERICINST => {
                let mono_generic_class_address = type_info.clone().data;
                let mono_class_address = reader.read_ptr(mono_generic_class_address);
                // this.Image.GetTypeDefinition(mono_class_address);

                let mono_generic_container_ptr = mono_class_address
                    + crate::constants::TYPE_DEFINITION_GENERIC_CONTAINER as usize;
                let mono_generic_container_address = reader.read_ptr(mono_generic_container_ptr);

                let mono_generic_context_ptr =
                    mono_generic_class_address + crate::constants::SIZE_OF_PTR;
                let mono_generic_ins_ptr = reader.read_ptr(mono_generic_context_ptr);

                // var argument_count = this.Process.ReadInt32(mono_generic_ins_ptr + 0x4);
                let argument_count = reader
                    .read_u32(mono_generic_container_address + (4 * crate::constants::SIZE_OF_PTR));
                let type_arg_v_ptr = mono_generic_ins_ptr + 0x8;

                for i in 0..argument_count {
                    let generic_type_argument_ptr = reader
                        .read_ptr(type_arg_v_ptr + (i as usize * crate::constants::SIZE_OF_PTR));
                    let t = TypeInfo::new(generic_type_argument_ptr, reader);
                    generic_type_args.push(t);
                }
            }
            _ => {}
        }

        TypeDefinition {
            address: definition_addr,
            reader,
            bit_fields,
            field_count,
            nested_in_addr,
            parent_addr,
            name,
            namespace_name,
            size,
            vtable_ptr,
            v_table,
            v_table_size,
            type_info,
            class_kind,
            is_enum,
            is_value_type,
            generic_type_args,
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

    pub fn get_static_value(&self, field_name: &str) -> (usize, TypeInfo) {
        // println!("get_static_value: {:?}", field_name);
        let fields = self.get_fields();
        for field in fields {
            let field_def = FieldDefinition::new(field, self.reader);
            if !field_def.type_info.is_const && field_def.type_info.is_static {
                // let field_addr = field + field_def.offset as usize;
                // println!("  {}: {:?}", field_def.name, field);

                if field_def.name == field_name {
                    let v_table_memory_size =
                        crate::constants::SIZE_OF_PTR * self.v_table_size as usize;

                    let value_ptr = self.reader.read_ptr(
                        self.v_table + (crate::constants::V_TABLE as usize) + v_table_memory_size,
                    );

                    return (value_ptr, field_def.type_info);
                }
            }
        }
        return (0, TypeInfo::new(0, self.reader));
    }

    pub fn get_field(&self, field_name: &str) -> (usize, TypeInfo) {
        let fields = self.get_fields();
        for field in fields {
            let field_def = FieldDefinition::new(field, self.reader);
            let type_info = field_def.type_info.clone();
            // let code = field_def.type_info.code();
            // println!("  field: {}, {}", field_def.name, code);
            if field_def.name == field_name {
                return (field, type_info);
            }
        }
        return (0, TypeInfo::new(0, self.reader));
    }

    pub fn get_value(&self, field_name: &str, ptr: usize) -> (usize, TypeInfo) {
        let field = self.get_field(field_name);
        let def = FieldDefinition::new(field.0, self.reader);

        return (def.offset as usize + ptr, def.type_info);
    }
}

impl fmt::Display for TypeDefinition<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fields_str: Vec<String> = Vec::new();
        for _field in self.get_fields() {
            let ptr = &self.reader.read_ptr(self.address);
            let field_def = FieldDefinition::new(_field, &self.reader);
            if field_def.type_info.clone().is_const {
                continue;
            }

            let code = field_def.type_info.clone().code();

            let managed = Managed::new(&self.reader, ptr + field_def.offset as usize, None);

            // println!("  {}: {}", field_def.name, field_def.type_info.code());

            match code {
                TypeCode::BOOLEAN => {
                    fields_str.push(format!(
                        "\"{}\": {}",
                        field_def.name,
                        managed.read_boolean()
                    ));
                }
                TypeCode::U4 => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_u4()));
                }
                TypeCode::U => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_u4()));
                }
                TypeCode::R4 => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_r4()));
                }
                TypeCode::R8 => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_r8()));
                }
                TypeCode::I4 => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_i4()));
                }
                TypeCode::I => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_i4()));
                }
                TypeCode::I2 => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_i2()));
                }
                TypeCode::U2 => {
                    fields_str.push(format!("\"{}\": {}", field_def.name, managed.read_u2()));
                }
                TypeCode::STRING => {
                    fields_str.push(format!(
                        "\"{}\": \"{}\"",
                        field_def.name,
                        managed.read_string()
                    ));
                }
                TypeCode::VALUETYPE => {
                    fields_str.push(format!(
                        "\"{}\": {}",
                        field_def.name,
                        managed.read_valuetype()
                    ));
                }
                TypeCode::VAR => {
                    let number_of_generic_argument = self
                        .reader
                        .read_u32(field_def.type_info.clone().data + crate::constants::SIZE_OF_PTR);

                    // println!("number_of_generic_argument: {}", number_of_generic_argument);
                    // for arg in self.generic_type_args.iter() {
                    //     println!("- code: {}", arg.clone().code());
                    // }

                    let mut offset: i32 = 0;
                    for _i in 0..number_of_generic_argument {
                        // let arg = self.generic_type_args[i as usize].clone().code();
                        offset += get_type_size(TypeCode::U4) as i32
                            - crate::constants::SIZE_OF_PTR as i32;
                    }

                    let managed =
                        Managed::new(&self.reader, ptr + field_def.offset as usize - 4, None);
                    let var = managed.read_var();

                    fields_str.push(format!("\"{}\": {}", field_def.name, ptr));
                }
                _ => {
                    fields_str.push(format!("\"{}\": {}, {}", field_def.name, code, "null"));
                }
            }
        }
        write!(f, "{{ {} }}", fields_str.join(", "))
    }
}

#[derive(Clone)]
pub struct TypeInfo {
    pub addr: usize,
    pub data: usize,
    pub attrs: u32,
    pub is_static: bool,
    pub is_const: bool,
    pub type_code: u32,
}

impl TypeInfo {
    pub fn new(addr: usize, reader: &MonoReader) -> Self {
        let data = reader.read_ptr(addr);
        let attrs = reader.read_u32(addr + crate::constants::SIZE_OF_PTR);
        let is_static = (attrs & 0x10) == 0x10;
        let is_const = (attrs & 0x40) == 0x40;
        let type_code = 0xff & (attrs >> 16);

        TypeInfo {
            addr,
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
            0x1f => TypeCode::CMODREQD,
            0x20 => TypeCode::CMODOPT,
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
    pub generic_type_args: Vec<TypeInfo>,
}

impl FieldDefinition {
    pub fn new(addr: usize, reader: &MonoReader) -> Self {
        let type_ptr = reader.read_ptr(addr);
        let type_info = TypeInfo::new(type_ptr, reader);

        let name = reader.read_ptr_ascii_string(addr + crate::constants::SIZE_OF_PTR as usize);

        let offset = reader.read_i32(addr + crate::constants::SIZE_OF_PTR * 3 as usize);

        // Get the generic type arguments
        let mut generic_type_args = Vec::new();
        let code = type_info.clone().code();
        match code {
            TypeCode::GENERICINST => {
                let mono_generic_class_address = type_info.clone().data;
                let mono_class_address = reader.read_ptr(mono_generic_class_address);
                // this.Image.GetTypeDefinition(mono_class_address);

                let mono_generic_container_ptr = mono_class_address
                    + crate::constants::TYPE_DEFINITION_GENERIC_CONTAINER as usize;
                let mono_generic_container_address = reader.read_ptr(mono_generic_container_ptr);

                let mono_generic_context_ptr =
                    mono_generic_class_address + crate::constants::SIZE_OF_PTR;
                let mono_generic_ins_ptr = reader.read_ptr(mono_generic_context_ptr);

                // var argument_count = this.Process.ReadInt32(mono_generic_ins_ptr + 0x4);
                let argument_count = reader
                    .read_u32(mono_generic_container_address + (4 * crate::constants::SIZE_OF_PTR));
                let type_arg_v_ptr = mono_generic_ins_ptr + 0x8;

                for i in 0..argument_count {
                    let generic_type_argument_ptr = reader
                        .read_ptr(type_arg_v_ptr + (i as usize * crate::constants::SIZE_OF_PTR));
                    let t = TypeInfo::new(generic_type_argument_ptr, reader);
                    generic_type_args.push(t);
                }
            }
            _ => {}
        }

        FieldDefinition {
            type_info,
            name,
            offset,
            generic_type_args,
        }
    }
}
