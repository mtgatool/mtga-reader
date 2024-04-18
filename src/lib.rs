use sysinfo::{Pid as SysPid, System};

pub mod constants;
pub mod field_definition;
pub mod managed;
pub mod mono_class_kind;
pub mod mono_reader;
pub mod pe_reader;
pub mod type_code;
pub mod type_definition;
pub mod type_info;

use managed::Managed;
use mono_reader::MonoReader;
use type_code::TypeCode;
use type_definition::TypeDefinition;
use type_info::TypeInfo;

use napi_derive::napi;

pub fn find_pid_by_name(name: &str) -> Option<SysPid> {
    let mut sys = System::new_all();
    sys.refresh_all();

    sys.processes()
        .iter()
        .find(|(_, process)| process.name().contains(name))
        .map(|(pid, _)| *pid)
}

pub fn get_def_by_name<'a>(
    defs: &'a Vec<usize>,
    name: &str,
    mono_reader: &MonoReader,
) -> Option<&'a usize> {
    defs.iter().find(|def| {
        let main_typedef = TypeDefinition::new(**def, &mono_reader);
        main_typedef.name == name
    })
}

#[napi]
pub fn read_data(process_name: String, fields: Vec<&str>) -> String {
    println!("Reading started...");

    let pid = find_pid_by_name(&process_name);

    if pid.is_none() {
        return String::from("Process not found");
    }

    let mut return_string: String = String::new();

    pid.iter().for_each(|pid| {
        let mut mono_reader = MonoReader::new(pid.as_u32());

        mono_reader.read_mono_root_domain();
        mono_reader.read_assembly_image();
        let defs = mono_reader.create_type_definitions();

        // get the type defs on the root of the assembly for the first loop
        let definition = get_def_by_name(&defs, fields[0], &mono_reader)
            .unwrap()
            .clone();

        // skipt the first item in the find array
        let find = &fields[1..];

        let mut field = (definition.clone(), TypeInfo::new(definition, &mono_reader));

        for (index, name) in find.iter().enumerate() {
            field = match index {
                0 => {
                    let class = TypeDefinition::new(definition, &mono_reader);
                    class.get_static_value(name)
                }
                _ => {
                    let managed = Managed::new(&mono_reader, field.0, None);
                    let ptr = mono_reader.read_ptr(field.0);
                    let code = field.1.clone().code();
                    let class = match code {
                        TypeCode::GENERICINST => managed.read_generic_instance(field.1.clone()),
                        _ => managed.read_class(),
                    };
                    class.get_value(name, ptr)
                }
            };
            let code = field.1.clone();
            println!("Find: {}: {} {}", name, code.code(), field.0);
        }

        let managed = Managed::new(&mono_reader, field.0, None);
        let ptr = mono_reader.read_ptr(field.0);
        let code = field.1.clone().code();

        let strout = match code {
            TypeCode::CLASS => {
                let mut class = managed.read_class();
                class.set_fields_base(ptr);
                class.to_string()
            }
            TypeCode::GENERICINST => {
                let mut class = managed.read_generic_instance(field.1.clone());
                class.set_fields_base(ptr);
                class.to_string()
            }
            TypeCode::SZARRAY => managed.read_managed_array(),
            _ => {
                println!("Code: {} strout not implemented", code);
                String::from("{}")
            }
        };

        return_string = strout.clone();
    });

    return_string
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
