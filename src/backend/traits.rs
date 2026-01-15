//! Core traits for runtime backend abstraction
//!
//! These traits define the common interface between Mono and IL2CPP backends,
//! allowing the rest of the codebase to work with either runtime transparently.

use crate::common::TypeCode;
use std::fmt::Debug;

/// Error type for backend operations
#[derive(Debug, Clone)]
pub enum BackendError {
    /// Process not found
    ProcessNotFound(String),
    /// Failed to initialize the runtime
    InitializationFailed(String),
    /// Failed to read memory
    MemoryReadError(String),
    /// Type or class not found
    TypeNotFound(String),
    /// Assembly not found
    AssemblyNotFound(String),
    /// Generic backend error
    Other(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::ProcessNotFound(msg) => write!(f, "Process not found: {}", msg),
            BackendError::InitializationFailed(msg) => write!(f, "Initialization failed: {}", msg),
            BackendError::MemoryReadError(msg) => write!(f, "Memory read error: {}", msg),
            BackendError::TypeNotFound(msg) => write!(f, "Type not found: {}", msg),
            BackendError::AssemblyNotFound(msg) => write!(f, "Assembly not found: {}", msg),
            BackendError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for BackendError {}

/// Type information for a field or variable
#[derive(Debug, Clone)]
pub struct TypeInfoData {
    /// Address of the type metadata
    pub addr: usize,
    /// Data pointer (e.g., class address for CLASS type)
    pub data: usize,
    /// Raw attributes
    pub attrs: u32,
    /// Whether this is a static field
    pub is_static: bool,
    /// Whether this is a constant
    pub is_const: bool,
    /// The type code
    pub type_code: TypeCode,
}

impl TypeInfoData {
    /// Create a new TypeInfoData with default values
    pub fn empty() -> Self {
        TypeInfoData {
            addr: 0,
            data: 0,
            attrs: 0,
            is_static: false,
            is_const: false,
            type_code: TypeCode::END,
        }
    }
}

/// Trait for field definition abstraction
pub trait FieldDef: Debug {
    /// Get the field name
    fn name(&self) -> &str;

    /// Get the field offset within the instance
    fn offset(&self) -> i32;

    /// Get type information for this field
    fn type_info(&self) -> TypeInfoData;

    /// Check if this is a static field
    fn is_static(&self) -> bool;

    /// Check if this is a constant
    fn is_const(&self) -> bool;

    /// Get generic type arguments if this is a generic field
    fn generic_type_args(&self) -> Vec<TypeInfoData>;
}

/// Trait for type/class definition abstraction
pub trait TypeDef: Debug {
    /// Get the type name
    fn name(&self) -> &str;

    /// Get the namespace
    fn namespace(&self) -> &str;

    /// Get the instance size in bytes
    fn size(&self) -> i32;

    /// Check if this is an enum type
    fn is_enum(&self) -> bool;

    /// Check if this is a value type
    fn is_value_type(&self) -> bool;

    /// Get the number of fields
    fn field_count(&self) -> i32;

    /// Get addresses of all fields
    fn get_field_addresses(&self) -> Vec<usize>;

    /// Get the parent class address
    fn parent_address(&self) -> usize;

    /// Get type info for this type
    fn type_info(&self) -> TypeInfoData;

    /// Get the vtable address
    fn vtable(&self) -> usize;

    /// Get the vtable size
    fn vtable_size(&self) -> i32;

    /// Get generic type arguments
    fn generic_type_args(&self) -> Vec<TypeInfoData>;
}

/// Main trait for memory reading operations
/// This provides a common interface for reading process memory
pub trait MemoryReader {
    /// Read a single byte
    fn read_u8(&self, addr: usize) -> u8;

    /// Read a 16-bit unsigned integer
    fn read_u16(&self, addr: usize) -> u16;

    /// Read a 32-bit unsigned integer
    fn read_u32(&self, addr: usize) -> u32;

    /// Read a 64-bit unsigned integer
    fn read_u64(&self, addr: usize) -> u64;

    /// Read a signed 8-bit integer
    fn read_i8(&self, addr: usize) -> i8;

    /// Read a signed 16-bit integer
    fn read_i16(&self, addr: usize) -> i16;

    /// Read a signed 32-bit integer
    fn read_i32(&self, addr: usize) -> i32;

    /// Read a signed 64-bit integer
    fn read_i64(&self, addr: usize) -> i64;

    /// Read a pointer (platform-sized)
    fn read_ptr(&self, addr: usize) -> usize;

    /// Read a sequence of bytes
    fn read_bytes(&self, addr: usize, len: usize) -> Vec<u8>;

    /// Read an ASCII null-terminated string
    fn read_ascii_string(&self, addr: usize) -> String;

    /// Try to read an ASCII string, returning None on error
    fn maybe_read_ascii_string(&self, addr: usize) -> Option<String>;

    /// Read a .NET string (UTF-16 encoded)
    fn read_managed_string(&self, addr: usize) -> Option<String>;

    /// Get pointer size for this process (4 or 8 bytes)
    fn ptr_size(&self) -> usize {
        8 // Default to 64-bit
    }
}

/// Main runtime backend trait
/// Implementations exist for Mono and IL2CPP
pub trait RuntimeBackend: MemoryReader + Send + Sync {
    /// Initialize the backend by finding the runtime in memory
    fn initialize(&mut self) -> Result<(), BackendError>;

    /// Get all type definition addresses for the default assembly (Assembly-CSharp)
    fn get_type_definitions(&self) -> Vec<usize>;

    /// Get type definitions for a specific assembly image
    fn get_type_definitions_for_image(&self, image_addr: usize) -> Vec<usize>;

    /// Get all loaded assembly names
    fn get_assembly_names(&self) -> Vec<String>;

    /// Get the image address for a named assembly
    fn get_assembly_image(&self, name: &str) -> Option<usize>;

    /// Create a type definition from an address
    fn create_type_def(&self, addr: usize) -> Box<dyn TypeDef + '_>;

    /// Create a field definition from an address
    fn create_field_def(&self, addr: usize) -> Box<dyn FieldDef + '_>;

    /// Read type info from an address
    fn read_type_info(&self, addr: usize) -> TypeInfoData;

    /// Get the runtime type name (e.g., "Mono", "IL2CPP")
    fn runtime_name(&self) -> &'static str;

    /// Check if the backend is properly initialized
    fn is_initialized(&self) -> bool;
}

/// Boxed backend type for dynamic dispatch
pub type BoxedBackend = Box<dyn RuntimeBackend>;
