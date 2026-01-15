//! Mono type definition implementation

use crate::backend::{TypeDef, TypeInfoData, MemoryReader, RuntimeBackend};
use crate::common::TypeCode;
use super::reader::MonoBackend;
use super::offsets::SIZE_OF_PTR;

/// Mono type definition
#[derive(Debug)]
pub struct MonoTypeDef<'a> {
    address: usize,
    backend: &'a MonoBackend,
    name: String,
    namespace: String,
    size: i32,
    field_count: i32,
    is_enum: bool,
    is_value_type: bool,
    vtable: usize,
    vtable_size: i32,
    parent_addr: usize,
    type_info: TypeInfoData,
    generic_type_args: Vec<TypeInfoData>,
}

impl<'a> MonoTypeDef<'a> {
    /// Create a new type definition from memory
    pub fn new(addr: usize, backend: &'a MonoBackend) -> Self {
        let offsets = backend.offsets();

        let bit_fields = backend.read_u32(addr + offsets.type_def_bit_fields as usize);
        let is_enum = (bit_fields & 0x8) == 0x8;
        let is_value_type = (bit_fields & 0x4) == 0x4;

        let field_count = backend.read_i32(addr + offsets.type_def_field_count as usize);
        let parent_addr = backend.read_ptr(addr + offsets.type_def_parent as usize);

        let name_ptr = backend.read_ptr(addr + offsets.type_def_name as usize);
        let name = backend.read_ascii_string(name_ptr);

        let namespace_ptr = backend.read_ptr(addr + offsets.type_def_namespace as usize);
        let namespace = backend.read_ascii_string(namespace_ptr);

        let size = backend.read_i32(addr + offsets.type_def_size as usize);

        let vtable_ptr = backend.read_ptr(addr + offsets.type_def_runtime_info as usize);
        let vtable = if vtable_ptr != 0 {
            backend.read_ptr(vtable_ptr + offsets.runtime_info_domain_vtables as usize)
        } else {
            0
        };

        let vtable_size = if vtable != 0 {
            backend.read_i32(addr + offsets.type_def_vtable_size as usize)
        } else {
            0
        };

        let type_info = backend.read_type_info(addr + offsets.type_def_by_val_arg as usize);

        // Get generic type arguments
        let generic_type_args = Self::read_generic_args(addr, backend, &type_info);

        MonoTypeDef {
            address: addr,
            backend,
            name,
            namespace,
            size,
            field_count,
            is_enum,
            is_value_type,
            vtable,
            vtable_size,
            parent_addr,
            type_info,
            generic_type_args,
        }
    }

    fn read_generic_args(_addr: usize, backend: &MonoBackend, type_info: &TypeInfoData) -> Vec<TypeInfoData> {
        let mut args = Vec::new();
        let offsets = backend.offsets();

        if type_info.type_code == TypeCode::GENERICINST {
            let mono_generic_class = type_info.data;
            let mono_class = backend.read_ptr(mono_generic_class);

            let container_ptr = mono_class + offsets.type_def_generic_container as usize;
            let container = backend.read_ptr(container_ptr);

            let context_ptr = mono_generic_class + SIZE_OF_PTR;
            let inst_ptr = backend.read_ptr(context_ptr);

            let arg_count = backend.read_u32(container + 4 * SIZE_OF_PTR);
            let type_arg_ptr = inst_ptr + 0x8;

            for i in 0..arg_count {
                let arg_ptr = backend.read_ptr(type_arg_ptr + (i as usize * SIZE_OF_PTR));
                let arg_info = backend.read_type_info(arg_ptr);
                args.push(arg_info);
            }
        }

        args
    }

    /// Get the address of this type definition
    pub fn address(&self) -> usize {
        self.address
    }
}

impl<'a> TypeDef for MonoTypeDef<'a> {
    fn name(&self) -> &str {
        &self.name
    }

    fn namespace(&self) -> &str {
        &self.namespace
    }

    fn size(&self) -> i32 {
        self.size
    }

    fn is_enum(&self) -> bool {
        self.is_enum
    }

    fn is_value_type(&self) -> bool {
        self.is_value_type
    }

    fn field_count(&self) -> i32 {
        self.field_count
    }

    fn get_field_addresses(&self) -> Vec<usize> {
        let offsets = self.backend.offsets();
        let first_field = self.backend.read_ptr(self.address + offsets.type_def_fields as usize);

        if first_field == 0 {
            return Vec::new();
        }

        let mut fields = Vec::new();
        for i in 0..self.field_count {
            let field = first_field + (i as usize * offsets.type_def_field_size as usize);
            let ptr = self.backend.read_ptr(field);
            if ptr != 0 {
                fields.push(field);
            }
        }

        fields
    }

    fn parent_address(&self) -> usize {
        self.parent_addr
    }

    fn type_info(&self) -> TypeInfoData {
        self.type_info.clone()
    }

    fn vtable(&self) -> usize {
        self.vtable
    }

    fn vtable_size(&self) -> i32 {
        self.vtable_size
    }

    fn generic_type_args(&self) -> Vec<TypeInfoData> {
        self.generic_type_args.clone()
    }
}
