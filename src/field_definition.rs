use crate::constants;
use crate::mono_reader::MonoReader;
use crate::type_code::TypeCode;
use crate::type_info::TypeInfo;

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

        let name = reader.read_ptr_ascii_string(addr + constants::SIZE_OF_PTR as usize);

        let offset = reader.read_i32(addr + constants::SIZE_OF_PTR * 3 as usize);

        // Get the generic type arguments
        let mut generic_type_args = Vec::new();
        let code = type_info.clone().code();
        match code {
            TypeCode::GENERICINST => {
                let mono_generic_class_address = type_info.clone().data;
                let mono_class_address = reader.read_ptr(mono_generic_class_address);
                // this.Image.GetTypeDefinition(mono_class_address);

                let mono_generic_container_ptr =
                    mono_class_address + constants::TYPE_DEFINITION_GENERIC_CONTAINER as usize;
                let mono_generic_container_address = reader.read_ptr(mono_generic_container_ptr);

                let mono_generic_context_ptr = mono_generic_class_address + constants::SIZE_OF_PTR;
                let mono_generic_ins_ptr = reader.read_ptr(mono_generic_context_ptr);

                // var argument_count = this.Process.ReadInt32(mono_generic_ins_ptr + 0x4);
                let argument_count =
                    reader.read_u32(mono_generic_container_address + (4 * constants::SIZE_OF_PTR));
                let type_arg_v_ptr = mono_generic_ins_ptr + 0x8;

                for i in 0..argument_count {
                    let generic_type_argument_ptr =
                        reader.read_ptr(type_arg_v_ptr + (i as usize * constants::SIZE_OF_PTR));
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
