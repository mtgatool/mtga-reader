use crate::field_definition::FieldDefinition;
use crate::managed::Managed;
use crate::mono_class_kind::{match_class_kind, MonoClassKind};
use crate::{constants, MonoReader, TypeCode, TypeInfo};

use core::fmt;

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
    pub fields_base: usize,
}

impl<'a> TypeDefinition<'a> {
    pub fn new(definition_addr: usize, reader: &'a MonoReader) -> Self {
        let bit_fields =
            reader.read_u32(definition_addr + constants::TYPE_DEFINITION_BIT_FIELDS as usize);

        let is_enum = (bit_fields & 0x8) == 0x8;

        let is_value_type = (bit_fields & 0x4) == 0x4;

        let field_count =
            reader.read_i32(definition_addr + constants::TYPE_DEFINITION_FIELD_COUNT as usize);

        let nested_in_addr =
            reader.read_ptr(definition_addr + constants::TYPE_DEFINITION_NESTED_IN as usize);

        let parent_addr =
            reader.read_ptr(definition_addr + constants::TYPE_DEFINITION_PARENT as usize);

        let name = reader
            .read_ptr_ascii_string(definition_addr + constants::TYPE_DEFINITION_NAME as usize);

        let namespace_name = reader
            .read_ptr_ascii_string(definition_addr + constants::TYPE_DEFINITION_NAMESPACE as usize);

        let size = reader.read_i32(definition_addr + constants::TYPE_DEFINITION_SIZE as usize);

        let vtable_ptr =
            reader.read_ptr(definition_addr + constants::TYPE_DEFINITION_RUNTIME_INFO as usize);

        let v_table = if vtable_ptr != 0 {
            reader.read_ptr(
                vtable_ptr + constants::TYPE_DEFINITION_RUNTIME_INFO_DOMAIN_V_TABLES as usize,
            )
        } else {
            0
        };

        let v_table_size = if v_table != 0 {
            reader.read_i32(definition_addr + constants::TYPE_DEFINITION_V_TABLE_SIZE as usize)
        } else {
            0
        };

        let type_info = TypeInfo::new(
            definition_addr + constants::TYPE_DEFINITION_BY_VAL_ARG as usize,
            &reader,
        );

        let class_kind_value =
            reader.read_u8(definition_addr + constants::TYPE_DEFINITION_CLASS_KIND as usize);
        let class_kind = match_class_kind(class_kind_value);

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

                    // println!(" {}: {}", i, t.clone().code());

                    generic_type_args.push(t);
                }
            }
            _ => {}
        }

        let fields_base = definition_addr;

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
            fields_base,
        }
    }

    pub fn get_fields(&self) -> Vec<usize> {
        let first_field = self
            .reader
            .read_ptr(self.address + constants::TYPE_DEFINITION_FIELDS as usize);

        let mut fields = Vec::new();

        if first_field == 0 {
            return fields;
        } else {
            for field_index in 0..self.field_count {
                let field = first_field
                    + (field_index as usize * constants::TYPE_DEFINITION_FIELD_SIZE as usize);
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
                    let v_table_memory_size = constants::SIZE_OF_PTR * self.v_table_size as usize;

                    let value_ptr = self.reader.read_ptr(
                        self.v_table + (constants::V_TABLE as usize) + v_table_memory_size,
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

    pub fn set_generic_type_args(&mut self, generic_type_args: Vec<TypeInfo>) {
        self.generic_type_args = generic_type_args;
    }

    pub fn set_fields_base(&mut self, addr: usize) {
        self.fields_base = addr;
    }
}

impl fmt::Display for TypeDefinition<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fields_str: Vec<String> = Vec::new();

        for _field in self.get_fields() {
            let field_def = FieldDefinition::new(_field, &self.reader);
            if !field_def.type_info.clone().is_const && !field_def.type_info.clone().is_static {
                let code = field_def.type_info.clone().code();

                let offset_a = field_def.offset;

                let offset_b = field_def.offset - (constants::SIZE_OF_PTR as i32 * 2);

                let offset = if self.is_value_type {
                    offset_b
                } else {
                    offset_a
                };

                let managed = Managed::new(&self.reader, self.fields_base + offset as usize, None);

                let val = match code {
                    TypeCode::BOOLEAN => managed.read_boolean().to_string(),
                    TypeCode::U4 => managed.read_u4().to_string(),
                    TypeCode::U => managed.read_u4().to_string(),
                    TypeCode::R4 => managed.read_r4().to_string(),
                    TypeCode::R8 => managed.read_r8().to_string(),
                    TypeCode::I4 => managed.read_i4().to_string(),
                    TypeCode::I => managed.read_i4().to_string(),
                    TypeCode::I2 => managed.read_i2().to_string(),
                    TypeCode::U2 => managed.read_u2().to_string(),
                    TypeCode::STRING => format!("\"{}\"", managed.read_string().to_string()),
                    TypeCode::VALUETYPE => managed.read_valuetype().to_string(),
                    _ => "null".to_string(),
                };

                // println!(
                //     " - {} {} {} => {} {} {}",
                //     self.fields_base + offset as usize,
                //     field_def.name,
                //     field_def.type_info.clone().is_const,
                //     field_def.type_info.clone().is_static,
                //     field_def.type_info.clone().code(),
                //     val
                // );

                fields_str.push(format!("\"{}\": {}", field_def.name, val));
            }
        }
        write!(f, "{{ {} }}", fields_str.join(", "))
    }
}
