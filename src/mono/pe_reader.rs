//! PE (Portable Executable) header reader for finding Mono exports

use core::fmt::Error;
use crate::backend::MemoryReader;

const SIGNATURE: u32 = 0x3c;
const EXPORT_DIRECTORY_INDEX_PE32_PLUS: u32 = 0x88; // 64bit
const NUMBER_OF_FUNCTIONS: u32 = 0x14;
const FUNCTION_ADDRESS_ARRAY_INDEX: u32 = 0x1c;
const FUNCTION_NAME_ARRAY_INDEX: u32 = 0x20;
const FUNCTION_ENTRY_SIZE: u32 = 4;

/// PE reader for parsing Windows PE headers
pub struct PEReader<'a, R: MemoryReader> {
    reader: &'a R,
    address: usize,
}

impl<'a, R: MemoryReader> PEReader<'a, R> {
    /// Create a new PE reader at the given base address
    pub fn new(reader: &'a R, address: usize) -> Self {
        PEReader { reader, address }
    }

    /// Get the offset of an exported function by name
    pub fn get_function_offset(&self, name: &str) -> Result<u32, Error> {
        let signature = self.reader.read_u32(self.address + SIGNATURE as usize);

        if signature == 0x0 {
            return Err(Error::default());
        }

        let export_directory_offset = (signature + EXPORT_DIRECTORY_INDEX_PE32_PLUS) as usize;
        let export_directory = self.reader.read_u32(self.address + export_directory_offset);

        let number_of_functions_offset = export_directory + NUMBER_OF_FUNCTIONS;
        let number_of_functions = self
            .reader
            .read_u32(self.address + number_of_functions_offset as usize);

        let function_address_array = self
            .reader
            .read_u32(self.address + (export_directory + FUNCTION_ADDRESS_ARRAY_INDEX) as usize);

        let function_name_array = self
            .reader
            .read_u32(self.address + (export_directory + FUNCTION_NAME_ARRAY_INDEX) as usize);

        let mut function_index: u32 = 0;

        while function_index < number_of_functions * FUNCTION_ENTRY_SIZE {
            function_index += FUNCTION_ENTRY_SIZE;

            let function_name_index = self
                .reader
                .read_u32(self.address + (function_name_array + function_index) as usize);

            if let Some(function_name) = self
                .reader
                .maybe_read_ascii_string(self.address + function_name_index as usize)
            {
                if name == function_name {
                    let offset = self.reader.read_u32(
                        self.address + (function_address_array + function_index) as usize,
                    );
                    return Ok(offset);
                }
            }
        }

        Err(Error::default())
    }
}
