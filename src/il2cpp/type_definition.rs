//! IL2CPP type definition implementation

use crate::backend::{TypeDef, TypeInfoData, MemoryReader, RuntimeBackend};
use crate::common::TypeCode;
use super::reader::Il2CppBackend;
use super::offsets::SIZE_OF_PTR;

/// IL2CPP type definition (Il2CppClass in memory)
#[derive(Debug)]
pub struct Il2CppTypeDef<'a> {
    address: usize,
    backend: &'a Il2CppBackend,
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

impl<'a> Il2CppTypeDef<'a> {
    /// Create a new type definition from memory
    pub fn new(addr: usize, backend: &'a Il2CppBackend) -> Self {
        let offsets = backend.offsets();

        // Read class flags
        let flags = backend.read_u32(addr + offsets.class_flags as usize);
        let is_value_type = (flags & 0x4) != 0;
        let is_enum = (flags & 0x8) != 0;

        // Read field count
        let field_count = backend.read_i32(addr + offsets.class_field_count as usize);

        // Read parent
        let parent_addr = backend.read_ptr(addr + offsets.class_parent as usize);

        // Read name
        let name_ptr = backend.read_ptr(addr + offsets.class_name as usize);
        let name = backend.read_ascii_string(name_ptr);

        // Read namespace
        let namespace_ptr = backend.read_ptr(addr + offsets.class_namespace as usize);
        let namespace = backend.read_ascii_string(namespace_ptr);

        // Read size
        let size = backend.read_i32(addr + offsets.class_instance_size as usize);

        // Read vtable info (IL2CPP stores it differently)
        // In IL2CPP, the vtable is part of the class structure
        let vtable = 0; // TODO: Implement proper vtable reading
        let vtable_size = 0;

        // Create type info
        let type_info = TypeInfoData {
            addr,
            data: addr,
            attrs: flags,
            is_static: false,
            is_const: false,
            type_code: if is_value_type { TypeCode::VALUETYPE } else { TypeCode::CLASS },
        };

        // Get generic type arguments
        let generic_type_args = Self::read_generic_args(addr, backend);

        Il2CppTypeDef {
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

    fn read_generic_args(addr: usize, backend: &Il2CppBackend) -> Vec<TypeInfoData> {
        let offsets = backend.offsets();
        let mut args = Vec::new();

        // Check if this is a generic instance
        let generic_class_ptr = backend.read_ptr(addr + offsets.class_generic_class as usize);
        if generic_class_ptr == 0 {
            return args;
        }

        // Read generic context
        let context_ptr = backend.read_ptr(
            generic_class_ptr + offsets.generic_class_context as usize
        );
        if context_ptr == 0 {
            return args;
        }

        // Read class type arguments
        let class_inst_ptr = backend.read_ptr(context_ptr);
        if class_inst_ptr == 0 {
            return args;
        }

        // Read argument count
        let argc = backend.read_u32(class_inst_ptr + offsets.generic_inst_argc as usize);
        let argv = backend.read_ptr(class_inst_ptr + offsets.generic_inst_argv as usize);

        for i in 0..argc {
            let type_ptr = backend.read_ptr(argv + (i as usize * SIZE_OF_PTR));
            if type_ptr != 0 {
                let type_info = backend.read_type_info(type_ptr);
                args.push(type_info);
            }
        }

        args
    }

    /// Get the address of this type definition
    pub fn address(&self) -> usize {
        self.address
    }
}

impl<'a> TypeDef for Il2CppTypeDef<'a> {
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
        let fields_ptr = self.backend.read_ptr(self.address + offsets.class_fields as usize);

        if fields_ptr == 0 {
            return Vec::new();
        }

        // IL2CPP stores fields as FieldInfo structures
        // Size of Il2CppFieldInfo varies by version but is typically 0x20 (32 bytes)
        const FIELD_INFO_SIZE: usize = 0x20;

        let mut fields = Vec::new();
        for i in 0..self.field_count {
            let field_addr = fields_ptr + (i as usize * FIELD_INFO_SIZE);
            fields.push(field_addr);
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
