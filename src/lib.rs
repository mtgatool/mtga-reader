pub mod constants;
pub mod field_definition;
pub mod managed;
pub mod mono_class_kind;
pub mod mono_reader;
pub mod pe_reader;
pub mod type_code;
pub mod type_definition;
pub mod type_info;

/*
pub fn read_managed<T>(type_code: TypeCode) -> Option<T> {
    match type_code {
        // 1, b => b[0] != 0
        TypeCode::BOOLEAN => Some(self.read_ptr_u8(addr) != 0),

        // char -> char
        TypeCode::CHAR => Some(self.read_ptr_u16(addr)),

        // sizeof(byte), b => b[0]
        TypeCode::I1 => Some(self.read_ptr_i8(addr)),

        // sizeof(sbyte), b => unchecked((sbyte)b[0])
        TypeCode::U1 => Some(self.read_ptr_u8(addr)),

        // short size -> int16
        TypeCode::I2 => Some(self.read_ptr_i16(addr)),

        // ushort size -> uint16
        TypeCode::U2 => Some(self.read_ptr_u16(addr)),

        // int32
        TypeCode::I => Some(self.read_i32(addr)),
        TypeCode::I4 => Some(self.read_i32(addr)),

        // unsigned int32
        TypeCode::U => Some(self.read_u32(addr)),
        TypeCode::U4 => Some(self.read_u32(addr)),

        // char size -> int64
        TypeCode::I8 => Some(self.read_ptr_i64(addr)),

        // char size -> uint64
        TypeCode::U8 => Some(self.read_ptr_u64(addr)),

        // char size -> single
        TypeCode::R4 => Some(self.read_ptr_u32(addr)),
        // char size -> double
        TypeCode::R8 => Some(self.read_i64(addr)),

        TypeCode::STRING => Some(self.read_ascii_string(addr)),

        // ReadManagedArray
        TypeCode::SZARRAY => Some(self.read_ptr_ptr(addr)),

        // try ReadManagedStructInstance
        TypeCode::VALUETYPE => Some(self.read_i32(addr)),

        // ReadManagedClassInstance
        TypeCode::CLASS => Some(self.read_ptr_ptr(addr)),

        // ReadManagedGenericObject
        TypeCode::GENERICINST => Some(self.read_ptr_ptr(addr)),

        // ReadManagedGenericObject
        TypeCode::OBJECT => Some(self.read_ptr_ptr(addr)),

        // ReadManagedVar
        TypeCode::VAR => Some(self.read_ptr_i32(addr)),

        // Junk
        TypeCode::END => Some(0),
        TypeCode::VOID => Some(0),
        TypeCode::PTR => Some(0),
        TypeCode::BYREF => Some(0),
        TypeCode::TYPEDBYREF => Some(0),
        TypeCode::FNPTR => Some(0),
        TypeCode::CMOD_REQD => Some(0),
        TypeCode::CMOD_OPT => Some(0),
        TypeCode::INTERNAL => Some(0),
        TypeCode::MODIFIER => Some(0),
        TypeCode::SENTINEL => Some(0),
        TypeCode::PINNED => Some(0),

        // May need support
        TypeCode::ARRAY => Some(0),
        TypeCode::ENUM => Some(0),
        TypeCode::MVAR => Some(0),
        _ => None,
    }
}
*/
