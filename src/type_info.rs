use crate::{constants, MonoReader};

use crate::type_code::TypeCode;

#[derive(Clone)]
pub struct TypeInfo {
    pub addr: usize,
    pub data: usize,
    pub attrs: u32,
    pub is_static: bool,
    pub is_const: bool,
    pub type_code: u32,
}

impl TypeInfo {
    pub fn new(addr: usize, reader: &MonoReader) -> Self {
        let data = reader.read_ptr(addr);
        let attrs = reader.read_u32(addr + constants::SIZE_OF_PTR);
        let is_static = (attrs & 0x10) == 0x10;
        let is_const = (attrs & 0x40) == 0x40;
        let type_code = 0xff & (attrs >> 16);

        TypeInfo {
            addr,
            data,
            attrs,
            is_static,
            is_const,
            type_code,
        }
    }

    pub fn code(self) -> TypeCode {
        // return the appropiate TypeCode enum based on self.type_code
        match self.type_code {
            0x00 => TypeCode::END,
            0x01 => TypeCode::VOID,
            0x02 => TypeCode::BOOLEAN,
            0x03 => TypeCode::CHAR,
            0x04 => TypeCode::I1,
            0x05 => TypeCode::U1,
            0x06 => TypeCode::I2,
            0x07 => TypeCode::U2,
            0x08 => TypeCode::I4,
            0x09 => TypeCode::U4,
            0x0a => TypeCode::I8,
            0x0b => TypeCode::U8,
            0x0c => TypeCode::R4,
            0x0d => TypeCode::R8,
            0x0e => TypeCode::STRING,
            0x0f => TypeCode::PTR,
            0x10 => TypeCode::BYREF,
            0x11 => TypeCode::VALUETYPE,
            0x12 => TypeCode::CLASS,
            0x13 => TypeCode::VAR,
            0x14 => TypeCode::ARRAY,
            0x15 => TypeCode::GENERICINST,
            0x16 => TypeCode::TYPEDBYREF,
            0x18 => TypeCode::I,
            0x19 => TypeCode::U,
            0x1b => TypeCode::FNPTR,
            0x1c => TypeCode::OBJECT,
            0x1d => TypeCode::SZARRAY,
            0x1e => TypeCode::MVAR,
            0x1f => TypeCode::CMODREQD,
            0x20 => TypeCode::CMODOPT,
            0x21 => TypeCode::INTERNAL,
            0x40 => TypeCode::MODIFIER,
            0x41 => TypeCode::SENTINEL,
            0x45 => TypeCode::PINNED,
            0x55 => TypeCode::ENUM,
            _ => TypeCode::END,
        }
    }
}
