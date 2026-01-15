//! Mono structure offsets
//!
//! These offsets are for reading Mono runtime structures from memory.
//! They vary between Unity versions due to changes in Mono struct layouts.

/// Pointer size for 64-bit processes
pub const SIZE_OF_PTR: usize = 8;

/// Name of the Mono library on Windows
pub const MONO_LIBRARY: &str = "mono-2.0-bdwgc.dll";

/// Offsets used to find mono_get_root_domain from the DLL
pub const RIP_PLUS_OFFSET_OFFSET: usize = 0x3;
pub const RIP_VALUE_OFFSET: usize = 0x7;

/// Mono structure offsets
/// These are for Unity 2021.3.14 / 2022.3 (MTGA versions)
#[derive(Debug, Clone)]
pub struct MonoOffsets {
    /// Offset in _MonoAssembly to field 'image' (Type MonoImage*)
    pub assembly_image: u32,
    /// Field 'domain_assemblies' in _MonoDomain
    pub referenced_assemblies: u32,
    /// Field 'class_cache' in _MonoImage
    pub image_class_cache: u32,
    /// Hash table size field offset
    pub hash_table_size: u32,
    /// Hash table array pointer offset
    pub hash_table_table: u32,

    // _MonoClass offsets
    /// Field size in type definition
    pub type_def_field_size: u32,
    /// Bit fields (size_inited, valuetype, enumtype)
    pub type_def_bit_fields: u32,
    /// Class kind byte
    pub type_def_class_kind: u32,
    /// Parent class pointer
    pub type_def_parent: u32,
    /// Nested in class pointer
    pub type_def_nested_in: u32,
    /// Type name string pointer
    pub type_def_name: u32,
    /// Namespace string pointer
    pub type_def_namespace: u32,
    /// VTable size
    pub type_def_vtable_size: u32,
    /// Instance size
    pub type_def_size: u32,
    /// Fields array pointer
    pub type_def_fields: u32,
    /// ByVal arg type info
    pub type_def_by_val_arg: u32,
    /// Runtime info pointer
    pub type_def_runtime_info: u32,
    /// Field count
    pub type_def_field_count: u32,
    /// Next class in cache linked list
    pub type_def_next_class_cache: u32,
    /// Mono generic class
    pub type_def_mono_generic_class: u32,
    /// Generic container
    pub type_def_generic_container: u32,
    /// Runtime info domain vtables offset
    pub runtime_info_domain_vtables: u32,
    /// VTable offset within MonoVTable
    pub vtable: u32,
}

impl Default for MonoOffsets {
    fn default() -> Self {
        Self::unity_2021_3()
    }
}

impl MonoOffsets {
    /// Offsets for Unity 2021.3.x (current MTGA version)
    pub fn unity_2021_3() -> Self {
        MonoOffsets {
            assembly_image: 0x10 + 0x50, // 0x60
            referenced_assemblies: 0xa0,
            image_class_cache: 0x4d0,
            hash_table_size: 0xc + 0xc,   // 0x18
            hash_table_table: 0x14 + 0xc, // 0x20

            type_def_field_size: 0x10 + 0x10,        // 0x20
            type_def_bit_fields: 0x14 + 0xc,         // 0x20
            type_def_class_kind: 0x1b,
            type_def_parent: 0x30,
            type_def_nested_in: 0x38,
            type_def_name: 0x48,
            type_def_namespace: 0x50,
            type_def_vtable_size: 0x5C,
            type_def_size: 0x90,
            type_def_fields: 0x98,
            type_def_by_val_arg: 0xB8,
            type_def_runtime_info: 0x84 + 0x34 + 0x18, // 0xD0
            type_def_field_count: 0xa4 + 0x34 + 0x18 + 0x10, // 0xE0
            type_def_next_class_cache: 0x108,
            type_def_mono_generic_class: 0x94 + 0x34 + 0x18 + 0x10, // 0xE0
            type_def_generic_container: 0x110,
            runtime_info_domain_vtables: 0x2 + 0x6, // 0x8
            vtable: 0x48,
        }
    }

    /// Offsets for Unity 2022.3.x
    pub fn unity_2022_3() -> Self {
        // Currently same as 2021.3 - adjust if needed
        Self::unity_2021_3()
    }

    /// Offsets for Unity 2019/2020 (older versions)
    pub fn unity_2019_2020() -> Self {
        MonoOffsets {
            assembly_image: 0x60,
            referenced_assemblies: 0x98,
            image_class_cache: 0x4c0,
            hash_table_size: 0x18,
            hash_table_table: 0x20,

            type_def_field_size: 0x20,
            type_def_bit_fields: 0x20,
            type_def_class_kind: 0x1b,
            type_def_parent: 0x30,
            type_def_nested_in: 0x38,
            type_def_name: 0x48,
            type_def_namespace: 0x50,
            type_def_vtable_size: 0x5C,
            type_def_size: 0x88,
            type_def_fields: 0x90,
            type_def_by_val_arg: 0xB0,
            type_def_runtime_info: 0xC8,
            type_def_field_count: 0xD8,
            type_def_next_class_cache: 0x100,
            type_def_mono_generic_class: 0xD8,
            type_def_generic_container: 0x108,
            runtime_info_domain_vtables: 0x8,
            vtable: 0x48,
        }
    }

    /// Get offsets for a specific Unity version string
    pub fn for_version(version: &str) -> Self {
        if version.starts_with("2022") {
            Self::unity_2022_3()
        } else if version.starts_with("2021") {
            Self::unity_2021_3()
        } else if version.starts_with("2019") || version.starts_with("2020") {
            Self::unity_2019_2020()
        } else {
            // Default to latest
            Self::unity_2021_3()
        }
    }
}
