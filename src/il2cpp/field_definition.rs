//! IL2CPP field definition implementation

use crate::backend::{FieldDef, TypeInfoData, MemoryReader, RuntimeBackend};
use super::reader::Il2CppBackend;

/// IL2CPP field definition (Il2CppFieldInfo in memory)
#[derive(Debug)]
pub struct Il2CppFieldDef<'a> {
    address: usize,
    backend: &'a Il2CppBackend,
    name: String,
    offset: i32,
    type_info: TypeInfoData,
    generic_type_args: Vec<TypeInfoData>,
}

impl<'a> Il2CppFieldDef<'a> {
    /// Create a new field definition from memory
    pub fn new(addr: usize, backend: &'a Il2CppBackend) -> Self {
        let offsets = backend.offsets();

        // Read field name
        let name_ptr = backend.read_ptr(addr + offsets.field_name as usize);
        let name = backend.read_ascii_string(name_ptr);

        // Read field type
        let type_ptr = backend.read_ptr(addr + offsets.field_type as usize);
        let type_info = if type_ptr != 0 {
            backend.read_type_info(type_ptr)
        } else {
            TypeInfoData::empty()
        };

        // Read field offset
        let offset = backend.read_i32(addr + offsets.field_offset as usize);

        // TODO: Read generic type arguments for generic fields
        let generic_type_args = Vec::new();

        Il2CppFieldDef {
            address: addr,
            backend,
            name,
            offset,
            type_info,
            generic_type_args,
        }
    }

    /// Get the address of this field definition
    pub fn address(&self) -> usize {
        self.address
    }
}

impl<'a> FieldDef for Il2CppFieldDef<'a> {
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
