//! Mono field definition implementation

use crate::backend::{FieldDef, TypeInfoData, MemoryReader, RuntimeBackend};
use crate::common::TypeCode;
use super::reader::MonoBackend;
use super::offsets::SIZE_OF_PTR;

/// Mono field definition
#[derive(Debug)]
pub struct MonoFieldDef<'a> {
    address: usize,
    backend: &'a MonoBackend,
    name: String,
    offset: i32,
    type_info: TypeInfoData,
    generic_type_args: Vec<TypeInfoData>,
}

impl<'a> MonoFieldDef<'a> {
    /// Create a new field definition from memory
    pub fn new(addr: usize, backend: &'a MonoBackend) -> Self {
        let type_ptr = backend.read_ptr(addr);
        let type_info = backend.read_type_info(type_ptr);

        let name_ptr = backend.read_ptr(addr + SIZE_OF_PTR);
        let name = backend.read_ascii_string(name_ptr);

        let offset = backend.read_i32(addr + SIZE_OF_PTR * 3);

        // Get generic type arguments
        let generic_type_args = Self::read_generic_args(backend, &type_info);

        MonoFieldDef {
            address: addr,
            backend,
            name,
            offset,
            type_info,
            generic_type_args,
        }
    }

    fn read_generic_args(backend: &MonoBackend, type_info: &TypeInfoData) -> Vec<TypeInfoData> {
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

    /// Get the address of this field definition
    pub fn address(&self) -> usize {
        self.address
    }
}

impl<'a> FieldDef for MonoFieldDef<'a> {
    fn name(&self) -> &str {
        &self.name
    }

    fn offset(&self) -> i32 {
        self.offset
    }

    fn type_info(&self) -> TypeInfoData {
        self.type_info.clone()
    }

    fn is_static(&self) -> bool {
        self.type_info.is_static
    }

    fn is_const(&self) -> bool {
        self.type_info.is_const
    }

    fn generic_type_args(&self) -> Vec<TypeInfoData> {
        self.generic_type_args.clone()
    }
}
