pub const RIP_PLUS_OFFSET_OFFSET: usize = 0x3;
pub const RIP_VALUE_OFFSET: usize = 0x7;

pub const SIZE_OF_PTR: usize = 8; // for 32 bit it's 4

pub const MONO_LIBRARY: &str = "mono-2.0-bdwgc.dll";
// offset in _MonoAssembly to field 'image' (Type MonoImage*)
pub const ASSEMBLY_IMAGE: u32 = 0x10 + 0x50;
// field 'domain_assemblies' in _MonoDomain (domain-internals.h)
pub const REFERENCED_ASSEMBLIES: u32 = 0xa0;

// field 'class_cache' in _MonoImage
pub const IMAGE_CLASS_CACHE: u32 = 0x4d0;
pub const HASH_TABLE_SIZE: u32 = 0xc + 0xc;
pub const HASH_TABLE_TABLE: u32 = 0x14 + 0xc;

// _MonoClass
// instance_size
pub const TYPE_DEFINITION_FIELD_SIZE: u32 = 0x10 + 0x10;

// starting from size_inited, valuetype, enumtype
pub const TYPE_DEFINITION_BIT_FIELDS: u32 = 0x14 + 0xc;

// class_kind
pub const TYPE_DEFINITION_CLASS_KIND: u32 = 0x1b; // alt: 0x1e + 0xc

// parent
pub const TYPE_DEFINITION_PARENT: u32 = 0x30;
// nested_in
pub const TYPE_DEFINITION_NESTED_IN: u32 = 0x38;
// name (Unity 2022.3 - confirmed via memory probing)
pub const TYPE_DEFINITION_NAME: u32 = 0x48;
// name_space (Unity 2022.3 - confirmed via memory probing)
pub const TYPE_DEFINITION_NAMESPACE: u32 = 0x50; // 0x48 + 0x8

// vtable_size
pub const TYPE_DEFINITION_V_TABLE_SIZE: u32 = 0x5C; // 0x50 + 0x8 + 0x4

// sizes
pub const TYPE_DEFINITION_SIZE: u32 = 0x90; // Static Fields / Array Element Count / Generic Param Types

// fields
pub const TYPE_DEFINITION_FIELDS: u32 = 0x98; // 0x98

// _byval_arg
pub const TYPE_DEFINITION_BY_VAL_ARG: u32 = 0xB8; // 0x98 + 0x10 (2 ptr) + 0x10 (sizeof(MonoType))

// runtime_info
pub const TYPE_DEFINITION_RUNTIME_INFO: u32 = 0x84 + 0x34 + 0x18; // 0xD0

// MonoClassDef
// field_count
pub const TYPE_DEFINITION_FIELD_COUNT: u32 = 0xa4 + 0x34 + 0x18 + 0x10; // 0xE0

// next_class_cache
// Unity 2021.3.14 & 2022.3: 0xa8 + 0x34 + 0x18 + 0x10 + 0x4 = 0x108
// NOTE: The C# comment says 0xE4 but the actual calculation is 0x108!
// Verified: 168 + 52 + 24 + 16 + 4 = 264 = 0x108
pub const TYPE_DEFINITION_NEXT_CLASS_CACHE: u32 = 0x108;
pub const TYPE_DEFINITION_MONO_GENERIC_CLASS: u32 = 0x94 + 0x34 + 0x18 + 0x10;
pub const TYPE_DEFINITION_GENERIC_CONTAINER: u32 = 0x110;

pub const TYPE_DEFINITION_RUNTIME_INFO_DOMAIN_V_TABLES: u32 = 0x2 + 0x6; // 2 byte 'max_domain' + allignment to pointer size

// MonoVTable.vtable
// 5 ptr + 8 byte (max_interface_id -> gc_bits) + 8 bytes (4 + 4 padding) + 2 ptr
// 0x28 + 0x8 + 0x8 + 0x10
pub const V_TABLE: u32 = 0x48;
