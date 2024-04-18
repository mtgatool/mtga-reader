use crate::constants;
use crate::field_definition::FieldDefinition;
use crate::mono_reader::MonoReader;
use crate::type_code::TypeCode;
use crate::type_definition::TypeDefinition;
use crate::type_info::TypeInfo;
use std::cmp;

pub struct Managed<'a> {
    reader: &'a MonoReader,
    pub addr: usize,
    pub generic_type_args: Vec<TypeInfo>,
}

impl<'a> Managed<'a> {
    pub fn new(
        reader: &'a MonoReader,
        addr: usize,
        generic_type_args: Option<Vec<TypeInfo>>,
    ) -> Self {
        Managed {
            reader,
            addr,
            generic_type_args: generic_type_args.unwrap_or(Vec::new()),
        }
    }

    pub fn read_boolean(&self) -> bool {
        self.reader.read_u8(self.addr) != 0x0
    }

    pub fn read_char(&self) -> char {
        self.reader.read_u16(self.addr).to_string().parse().unwrap()
    }

    // read_u
    pub fn read_u4(&self) -> u32 {
        self.reader.read_u32(self.addr)
    }

    pub fn read_r4(&self) -> i32 {
        self.reader.read_i32(self.addr)
    }

    pub fn read_r8(&self) -> i64 {
        self.reader.read_i64(self.addr)
    }

    // read_i
    pub fn read_i4(&self) -> i32 {
        self.reader.read_i32(self.addr)
    }

    pub fn read_i2(&self) -> i16 {
        self.reader.read_i16(self.addr)
    }

    pub fn read_u2(&self) -> u16 {
        self.reader.read_u16(self.addr)
    }

    pub fn read_string(&self) -> String {
        let ptr = self.reader.read_ptr(self.addr);
        let length = self
            .reader
            .read_u32(ptr + (constants::SIZE_OF_PTR * 2 as usize));

        let mut str = Vec::new();

        let cap_length = cmp::min(length as u64 * 2, 1024);

        for i in 0..(cap_length) {
            let val = self
                .reader
                .read_u16(ptr + (constants::SIZE_OF_PTR * 2 as usize) + 4 + (i as usize));

            str.push(val);
        }

        // Convert the vector to a string
        let string: String = str.iter().map(|&c| c as u8 as char).collect::<String>();

        return string;
    }

    pub fn read_valuetype(&self) -> i32 {
        self.reader.read_i32(self.addr)
    }

    pub fn read_class_address(&self) -> usize {
        let ptr: usize = self.reader.read_ptr(self.addr);
        let vtable = self.reader.read_ptr(ptr);
        let definition_addr = self.reader.read_ptr(vtable);

        //   println!("ptr: {:?}", ptr);
        //   println!("self.addr: {:?}", self.addr);
        //   println!("vtable: {:?}", vtable);
        //   println!("definition_addr: {:?}", definition_addr);

        return definition_addr;
    }

    pub fn read_class(&self) -> TypeDefinition {
        let address = self.read_class_address();
        return TypeDefinition::new(address, self.reader);
    }

    pub fn read_raw_class(&self) -> TypeDefinition {
        return TypeDefinition::new(self.addr, self.reader);
    }

    pub fn read_generic_instance(&self, type_info: TypeInfo) -> TypeDefinition {
        let ptr = self.reader.read_ptr(type_info.data);
        let mut td = TypeDefinition::new(ptr, self.reader);
        td.set_generic_type_args(self.generic_type_args.clone());

        if td.is_value_type {
            return td;
        }

        return td;
    }

    // pub fn read_managed_array<T>(&self) -> Option<T>

    pub fn read_managed_array(&self) -> String {
        let ptr = self.reader.read_ptr(self.addr);
        if ptr == 0 {
            return String::from("null");
        }

        let vtable = self.reader.read_ptr(ptr);

        let array_definition_ptr = self.reader.read_ptr(vtable);

        let array_definition = TypeDefinition::new(array_definition_ptr, self.reader);

        let element_definition =
            TypeDefinition::new(self.reader.read_ptr(array_definition_ptr), self.reader);

        let count = 2 as u32;
        // self
        //     .reader
        //     .read_u32(ptr + (constants::SIZE_OF_PTR * 3));

        let start = ptr + constants::SIZE_OF_PTR * 4;

        let mut result = Vec::new();

        // println!("Array vtable: {:?}", vtable);
        // println!(
        //     "Array array_definition.address: {:?}",
        //     array_definition.address
        // );
        // println!(
        //     "Array element_definition.address: {:?}",
        //     element_definition.address
        // );
        // println!(
        //     "Array element_definition type: {}",
        //     element_definition.type_info.clone().code()
        // );

        let type_args = element_definition.generic_type_args.clone();

        let code = element_definition.type_info.clone().code();

        for i in 0..count {
            let managed = Managed::new(
                self.reader,
                start + (i as usize * array_definition.size as usize),
                Some(type_args.clone()),
            );

            let strout = match code {
                TypeCode::CLASS => managed.read_class().to_string(),
                TypeCode::GENERICINST => {
                    let m = managed.read_generic_instance(element_definition.type_info.clone());

                    let el_fields = m.get_fields();

                    let mut fields_str: Vec<String> = Vec::new();
                    for field in el_fields {
                        let field_def = FieldDefinition::new(field, &self.reader);

                        let number_of_generic_argument = self.reader.maybe_read_u32(
                            field_def.type_info.clone().data + constants::SIZE_OF_PTR,
                        );

                        let mut offset: i32 = 0;

                        let gen_type = match number_of_generic_argument {
                            Some(number_of_generic_argument) => {
                                // get the offset for this arg
                                for i in 0..(number_of_generic_argument as i32) {
                                    let arg = type_args[i as usize].clone().code();
                                    offset +=
                                        get_type_size(arg) as i32 - constants::SIZE_OF_PTR as i32;
                                }
                                // get the type of the value
                                type_args[number_of_generic_argument as usize].clone()
                            }
                            None => field_def.type_info.clone(),
                        };

                        let managed_var = Managed::new(
                            self.reader,
                            managed.addr + (field_def.offset + offset) as usize,
                            None,
                        );

                        let var = match gen_type.clone().code() {
                            TypeCode::I4 => managed_var.read_i4().to_string(),
                            TypeCode::U4 => managed_var.read_u4().to_string(),
                            TypeCode::R4 => managed_var.read_r4().to_string(),
                            TypeCode::R8 => managed_var.read_r8().to_string(),
                            TypeCode::I => managed_var.read_i4().to_string(),
                            TypeCode::U => managed_var.read_u4().to_string(),
                            TypeCode::I2 => managed_var.read_i2().to_string(),
                            TypeCode::U2 => managed_var.read_u2().to_string(),
                            TypeCode::STRING => managed_var.read_string(),
                            TypeCode::CLASS => {
                                let mut class = managed_var.read_class();
                                let ptr = self.reader.read_ptr(managed_var.addr);
                                class.set_fields_base(ptr);
                                class.to_string()
                            },
                            // (field_def.type_info.code()).to_string(),
                            _ => "null".to_string()
                        };

                        fields_str.push(format!("\"{}\": {}", field_def.name, var));
                    }

                    format!("{{{}}}", fields_str.join(","))
                }
                _ => {
                    // println!("Code: {} strout not implemented", code);
                    String::from("{}")
                }
            };

            result.push(strout);
        }

        return format!("[{}]", result.join(", "));
    }

    pub fn read_var(&self) -> u32 {
        let ptr = self.reader.read_u32(self.addr);

        return ptr;
    }
}

fn get_type_size(type_code: TypeCode) -> usize {
    match type_code {
        TypeCode::BOOLEAN => 1,
        TypeCode::CHAR => 2,
        TypeCode::I1 => 1,
        TypeCode::U1 => 1,
        TypeCode::I2 => 2,
        TypeCode::U2 => 2,
        TypeCode::I4 => 4,
        TypeCode::U4 => 4,
        TypeCode::I8 => 8,
        TypeCode::U8 => 8,
        TypeCode::R4 => 4,
        TypeCode::R8 => 8,
        TypeCode::PTR => 4,
        TypeCode::BYREF => 4,
        TypeCode::VALUETYPE => 4,
        TypeCode::CLASS => 4,
        TypeCode::VAR => 4,
        TypeCode::ARRAY => 4,
        TypeCode::GENERICINST => 4,
        TypeCode::TYPEDBYREF => 4,
        TypeCode::I => 4,
        TypeCode::U => 4,
        TypeCode::FNPTR => 4,
        TypeCode::OBJECT => 4,
        TypeCode::SZARRAY => 4,
        TypeCode::MVAR => 4,
        TypeCode::CMODREQD => 4,
        TypeCode::CMODOPT => 4,
        TypeCode::INTERNAL => 4,
        TypeCode::MODIFIER => 4,
        TypeCode::SENTINEL => 4,
        TypeCode::PINNED => 4,
        TypeCode::ENUM => 4,
        _ => 0,
    }
}
