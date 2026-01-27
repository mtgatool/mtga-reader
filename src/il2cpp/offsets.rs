//! IL2CPP structure offsets
//!
//! These offsets are for reading IL2CPP runtime structures from memory.
//! They vary between Unity/IL2CPP versions.

/// Pointer size for 64-bit processes
pub const SIZE_OF_PTR: usize = 8;

/// Name of the IL2CPP library on different platforms
#[cfg(target_os = "windows")]
pub const IL2CPP_LIBRARY: &str = "GameAssembly.dll";

#[cfg(target_os = "linux")]
pub const IL2CPP_LIBRARY: &str = "GameAssembly.so";

#[cfg(target_os = "macos")]
pub const IL2CPP_LIBRARY: &str = "GameAssembly.dylib";

/// Global pointer offsets in GameAssembly __DATA segment
/// These are offsets from the second __DATA segment base address
#[derive(Debug, Clone)]
pub struct GlobalPointerOffsets {
    /// Offset to s_Il2CppMetadataRegistration pointer
    pub metadata_registration: usize,
    /// Offset to s_Il2CppCodeRegistration pointer
    pub code_registration: usize,
    /// Offset to s_GlobalMetadata pointer
    pub global_metadata: usize,
    /// Offset to s_TypeInfoTable pointer (Il2CppClass**)
    pub type_info_table: usize,
}

impl Default for GlobalPointerOffsets {
    fn default() -> Self {
        Self::mtga()
    }
}

impl GlobalPointerOffsets {
    /// Offsets discovered for MTGA (Magic: The Gathering Arena)
    pub fn mtga() -> Self {
        GlobalPointerOffsets {
            metadata_registration: 0x24330,
            code_registration: 0x24338,
            global_metadata: 0x24340,
            type_info_table: 0x24360,
        }
    }
}

/// IL2CPP structure offsets
#[derive(Debug, Clone)]
pub struct Il2CppOffsets {
    /// Version identifier
    pub version_name: String,

    // Il2CppClass offsets
    /// Pointer to Il2CppImage
    pub class_image: u32,
    /// Pointer to name string
    pub class_name: u32,
    /// Pointer to namespace string
    pub class_namespace: u32,
    /// Parent class pointer
    pub class_parent: u32,
    /// Pointer to FieldInfo array
    pub class_fields: u32,
    /// Number of fields
    pub class_field_count: u32,
    /// Pointer to static field data
    pub class_static_fields: u32,
    /// Pointer to MethodInfo array
    pub class_methods: u32,
    /// Instance size in bytes
    pub class_instance_size: u32,
    /// Class flags (value type, enum, etc.)
    pub class_flags: u32,
    /// Type definition index
    pub class_type_definition: u32,
    /// Generic class pointer (for generic instances)
    pub class_generic_class: u32,

    /// Global pointer offsets
    pub global_offsets: GlobalPointerOffsets,

    // Il2CppFieldInfo offsets
    /// Field name pointer
    pub field_name: u32,
    /// Il2CppType pointer
    pub field_type: u32,
    /// Parent class pointer
    pub field_parent: u32,
    /// Field offset in instance
    pub field_offset: u32,

    // Il2CppType offsets
    /// Data union (classIndex, genericClass, etc.)
    pub type_data: u32,
    /// Type attributes
    pub type_attrs: u32,

    // Il2CppGenericClass offsets
    /// Type definition pointer
    pub generic_class_type: u32,
    /// Context (type arguments)
    pub generic_class_context: u32,

    // Il2CppGenericInst offsets
    /// Number of type arguments
    pub generic_inst_argc: u32,
    /// Type arguments array
    pub generic_inst_argv: u32,

    // String offsets (Il2CppString)
    /// String length
    pub string_length: u32,
    /// String characters (UTF-16)
    pub string_chars: u32,

    // Array offsets (Il2CppArray)
    /// Array length
    pub array_length: u32,
    /// Array elements start
    pub array_elements: u32,
}

impl Default for Il2CppOffsets {
    fn default() -> Self {
        Self::unity_2021()
    }
}

impl Il2CppOffsets {
    /// Offsets for Unity 2021.x IL2CPP (metadata v29+)
    /// Updated with MTGA-discovered offsets (2026-01-24)
    pub fn unity_2021() -> Self {
        Il2CppOffsets {
            version_name: "Unity 2021.x".to_string(),

            // Il2CppClass - verified from MTGA offset testing
            class_image: 0x0,
            class_name: 0x10,
            class_namespace: 0x18,
            class_parent: 0x48,
            class_fields: 0x80,      // CORRECTED: Was 0x70, verified at 0x80
            class_field_count: 0x124, // CORRECTED: Was 0x11C, verified at 0x124
            class_static_fields: 0xA8, // Verified correct
            class_methods: 0x88,
            class_instance_size: 0xF8,
            class_flags: 0xFC,
            class_type_definition: 0x68,
            class_generic_class: 0x50,

            global_offsets: GlobalPointerOffsets::mtga(),

            // Il2CppFieldInfo
            field_name: 0x0,
            field_type: 0x8,
            field_parent: 0x10,
            field_offset: 0x18,

            // Il2CppType
            type_data: 0x0,
            type_attrs: 0x8,

            // Il2CppGenericClass
            generic_class_type: 0x0,
            generic_class_context: 0x8,

            // Il2CppGenericInst
            generic_inst_argc: 0x0,
            generic_inst_argv: 0x8,

            // Il2CppString
            string_length: 0x10,
            string_chars: 0x14,

            // Il2CppArray
            array_length: 0x18,
            array_elements: 0x20,
        }
    }

    /// Offsets for Unity 2022.x IL2CPP
    pub fn unity_2022() -> Self {
        // Same as 2021 for now
        let mut offsets = Self::unity_2021();
        offsets.version_name = "Unity 2022.x".to_string();
        offsets
    }

    /// Offsets for Unity 2019/2020 IL2CPP (older metadata versions)
    pub fn unity_2019_2020() -> Self {
        Il2CppOffsets {
            version_name: "Unity 2019/2020".to_string(),

            // Older IL2CPP versions have slightly different layouts
            class_image: 0x0,
            class_name: 0x10,
            class_namespace: 0x18,
            class_parent: 0x50,
            class_fields: 0x78,
            class_field_count: 0x114,
            class_static_fields: 0xB0,
            class_methods: 0x80,
            class_instance_size: 0xF8,
            class_flags: 0xF4,
            class_type_definition: 0x60,
            class_generic_class: 0x8,

            global_offsets: GlobalPointerOffsets::default(),

            field_name: 0x0,
            field_type: 0x8,
            field_parent: 0x10,
            field_offset: 0x18,

            type_data: 0x0,
            type_attrs: 0x8,

            generic_class_type: 0x0,
            generic_class_context: 0x8,

            generic_inst_argc: 0x0,
            generic_inst_argv: 0x8,

            string_length: 0x10,
            string_chars: 0x14,

            array_length: 0x18,
            array_elements: 0x20,
        }
    }

    /// Get offsets for a specific Unity version string
    pub fn for_version(version: &str) -> Self {
        if version.starts_with("2022") {
            Self::unity_2022()
        } else if version.starts_with("2021") {
            Self::unity_2021()
        } else if version.starts_with("2019") || version.starts_with("2020") {
            Self::unity_2019_2020()
        } else {
            // Default to latest
            Self::unity_2021()
        }
    }
}
