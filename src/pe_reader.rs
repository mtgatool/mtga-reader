use core::fmt::Error;

use crate::mono_reader::MonoReader;

pub struct PEReader<'a> {
    reader: &'a MonoReader,
    address: usize,
}

const SIGNATURE: u32 = 0x3c;
// const EXPORT_DIRECTORY_INDEX_PE: u32 = 0x78; // 32bit
const EXPORT_DIRECTORY_INDEX_PE32_PLUS: u32 = 0x88; // 64bit
const NUMBER_OF_FUNCTIONS: u32 = 0x14;
const FUNCTION_ADDRESS_ARRAY_INDEX: u32 = 0x1c;
const FUNCTION_NAME_ARRAY_INDEX: u32 = 0x20;
const FUNCTION_ENTRY_SIZE: u32 = 4;

impl<'a> PEReader<'a> {
    pub fn new(reader: &'a MonoReader, address: usize) -> Self {
        PEReader { reader, address }
    }

    fn parse_u32(&self, offset: usize) -> u32 {
        let mut bytes: [u8; 4] = [0, 0, 0, 0];
        for i in 0..4 {
            let val = self.reader.read_u8(self.address + offset + i);
            bytes[i] = val;
        }
        u32::from_le_bytes(bytes)
    }

    fn parse_ascii_string(&self, offset: usize) -> String {
        self.reader.read_ascii_string(self.address + offset)
    }

    pub fn get_function_offset(&self, name: &str) -> Result<u32, Error> {
        let signature_offset = SIGNATURE as usize;
        let signature = self.parse_u32(signature_offset);

        let export_directory_offset =
            (signature + EXPORT_DIRECTORY_INDEX_PE32_PLUS as u32) as usize;

        let export_directory = self.parse_u32(export_directory_offset);

        let number_of_functions_offset = export_directory + NUMBER_OF_FUNCTIONS as u32;
        let number_of_functions = self.parse_u32(number_of_functions_offset as usize);

        let function_address_array_index_offset =
            export_directory + (FUNCTION_ADDRESS_ARRAY_INDEX as u32);

        let function_address_array = self.parse_u32(function_address_array_index_offset as usize);

        let function_name_array_index_offset =
            export_directory + (FUNCTION_NAME_ARRAY_INDEX as u32);

        let function_name_array = self.parse_u32(function_name_array_index_offset as usize);

        let mut root_domain_function_address = 0;
        let mut function_index: u32 = 0;
        while function_index < number_of_functions * FUNCTION_ENTRY_SIZE as u32 {
            function_index += FUNCTION_ENTRY_SIZE as u32;

            let function_name_index =
                self.parse_u32((function_name_array + function_index) as usize);

            let function_name = self.parse_ascii_string(function_name_index as usize);

            if function_name == name {
                root_domain_function_address =
                    self.parse_u32((function_address_array + function_index) as usize);
                break;
            }
        }

        Ok(root_domain_function_address)
    }
}
