use axum::{
    extract::Path,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};

use mtga_reader::{
    field_definition::FieldDefinition,
    mono_reader::MonoReader,
    type_code::TypeCode,
    type_definition::TypeDefinition,
};

// Response types
#[derive(Serialize)]
struct AssembliesResponse {
    assemblies: Vec<String>,
}

#[derive(Serialize)]
struct ClassInfo {
    name: String,
    namespace: String,
    address: usize,
    is_static: bool,
    is_enum: bool,
}

#[derive(Serialize)]
struct ClassesResponse {
    classes: Vec<ClassInfo>,
}

#[derive(Serialize)]
struct FieldInfo {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    offset: i32,
    is_static: bool,
    is_const: bool,
}

#[derive(Serialize)]
struct StaticInstanceInfo {
    field_name: String,
    address: usize,
}

#[derive(Serialize)]
struct ClassDetailsResponse {
    name: String,
    namespace: String,
    address: usize,
    fields: Vec<FieldInfo>,
    static_instances: Vec<StaticInstanceInfo>,
}

#[derive(Serialize)]
struct InstanceField {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    is_static: bool,
    value: serde_json::Value,
}

#[derive(Serialize)]
struct InstanceResponse {
    class_name: String,
    namespace: String,
    address: usize,
    fields: Vec<InstanceField>,
}

// Global mono reader - UNSAFE but necessary for this use case
static mut MONO_READER: Option<MonoReader> = None;

// Helper function to create a new reader for each request
fn with_reader<F, R>(f: F) -> R
where
    F: FnOnce(&mut MonoReader) -> R,
{
    unsafe {
        if let Some(ref mut reader) = MONO_READER {
            f(reader)
        } else {
            panic!("MonoReader not initialized");
        }
    }
}

// Handler functions
async fn get_assemblies() -> Json<AssembliesResponse> {
    let assemblies = with_reader(|reader| reader.get_all_assembly_names());
    Json(AssembliesResponse { assemblies })
}

async fn get_assembly_classes(
    Path(assembly_name): Path<String>,
) -> Result<Json<ClassesResponse>, StatusCode> {
    let classes = with_reader(|reader| {
        // Load assembly image by name
        let image_addr = reader.read_assembly_image_by_name(&assembly_name);
        if image_addr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        // Create type definitions for this assembly
        let type_defs = reader.create_type_definitions_for_image(image_addr);

        let mut classes = Vec::new();
        for def_addr in type_defs {
            let typedef = TypeDefinition::new(def_addr, &reader);
            classes.push(ClassInfo {
                name: typedef.name.clone(),
                namespace: typedef.namespace_name.clone(),
                address: def_addr,
                is_static: false,
                is_enum: typedef.is_enum,
            });
        }

        Ok(classes)
    })?;

    Ok(Json(ClassesResponse { classes }))
}

async fn get_class_details(
    Path((assembly_name, class_name)): Path<(String, String)>,
) -> Result<Json<ClassDetailsResponse>, StatusCode> {
    let response = with_reader(|reader| {
        // Load assembly image by name
        let image_addr = reader.read_assembly_image_by_name(&assembly_name);
        if image_addr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        // Find class
        let type_defs = reader.create_type_definitions_for_image(image_addr);
        let class_addr = type_defs
            .iter()
            .find(|&&def_addr| {
                let typedef = TypeDefinition::new(def_addr, &reader);
                typedef.name == class_name
            })
            .ok_or(StatusCode::NOT_FOUND)?;

        let typedef = TypeDefinition::new(*class_addr, &reader);

        // Get fields
        let field_addrs = typedef.get_fields();
        let mut fields = Vec::new();
        for field_addr in &field_addrs {
            let field = FieldDefinition::new(*field_addr, &reader);
            // Get type name from TypeDefinition if it's a class/valuetype
            let type_name = match field.type_info.clone().code() {
                TypeCode::CLASS | TypeCode::VALUETYPE => {
                    let typedef = TypeDefinition::new(field.type_info.data, &reader);
                    format!("{}.{}", typedef.namespace_name, typedef.name)
                }
                TypeCode::I4 => "System.Int32".to_string(),
                TypeCode::U4 => "System.UInt32".to_string(),
                TypeCode::I8 => "System.Int64".to_string(),
                TypeCode::U8 => "System.UInt64".to_string(),
                TypeCode::BOOLEAN => "System.Boolean".to_string(),
                TypeCode::STRING => "System.String".to_string(),
                _ => format!("TypeCode({})", field.type_info.type_code)
            };
            fields.push(FieldInfo {
                name: field.name.clone(),
                type_name,
                offset: field.offset,
                is_static: field.type_info.is_static,
                is_const: field.type_info.is_const,
            });
        }

        // Get static instances
        let mut static_instances = Vec::new();
        for field_addr in &field_addrs {
            let field = FieldDefinition::new(*field_addr, &reader);
            if (field.name.contains("instance") || field.name.contains("Instance")) && field.type_info.is_static {
                // Use get_static_value to read the static field address
                let (static_field_addr, _) = typedef.get_static_value(&field.name);

                // Dereference to get the actual instance pointer
                let instance_ptr = if static_field_addr != 0 {
                    reader.read_ptr(static_field_addr)
                } else {
                    0
                };

                static_instances.push(StaticInstanceInfo {
                    field_name: field.name.clone(),
                    address: instance_ptr,
                });
            }
        }

        Ok(ClassDetailsResponse {
            name: typedef.name.clone(),
            namespace: typedef.namespace_name.clone(),
            address: *class_addr,
            fields,
            static_instances,
        })
    })?;

    Ok(Json(response))
}

async fn get_instance(
    Path(address_str): Path<String>,
) -> Result<Json<InstanceResponse>, StatusCode> {
    let response = with_reader(|reader| -> Result<InstanceResponse, StatusCode> {
        // Parse address
        let address = if address_str.starts_with("0x") {
            usize::from_str_radix(&address_str[2..], 16)
        } else {
            address_str.parse::<usize>()
        }
        .map_err(|_| StatusCode::BAD_REQUEST)?;

        // Read instance vtable and class
        let vtable_ptr = reader.read_ptr(address);
        let class_ptr = reader.read_ptr(vtable_ptr);
        let typedef = TypeDefinition::new(class_ptr, &reader);

        // Read field values
        let field_addrs = typedef.get_fields();
        let mut fields = Vec::new();
        for field_addr in &field_addrs {
            let field = FieldDefinition::new(*field_addr, &reader);
            // Get type name
            let type_name = match field.type_info.clone().code() {
                TypeCode::CLASS | TypeCode::VALUETYPE | TypeCode::GENERICINST => {
                    let typedef = TypeDefinition::new(field.type_info.data, &reader);
                    let base_name = if typedef.namespace_name.is_empty() {
                        typedef.name.clone()
                    } else {
                        format!("{}.{}", typedef.namespace_name, typedef.name)
                    };
                    base_name
                }
                TypeCode::SZARRAY => {
                    // Single-dimension array
                    format!("Array (SZARRAY)")
                }
                TypeCode::ARRAY => {
                    format!("Array (multi-dim)")
                }
                TypeCode::I4 => "System.Int32".to_string(),
                TypeCode::U4 => "System.UInt32".to_string(),
                TypeCode::I8 => "System.Int64".to_string(),
                TypeCode::U8 => "System.UInt64".to_string(),
                TypeCode::BOOLEAN => "System.Boolean".to_string(),
                TypeCode::STRING => "System.String".to_string(),
                TypeCode::OBJECT => "System.Object".to_string(),
                TypeCode::PTR => "Pointer".to_string(),
                _ => format!("TypeCode({})", field.type_info.type_code)
            };

            // Read field value based on type
            let value = match type_name.as_str() {
                "System.Int32" | "int" => {
                    let val = reader.read_i32(address + field.offset as usize);
                    serde_json::json!(val)
                }
                "System.Int64" | "long" => {
                    let val = reader.read_i64(address + field.offset as usize);
                    serde_json::json!(val)
                }
                "System.UInt32" | "uint" => {
                    let val = reader.read_u32(address + field.offset as usize);
                    serde_json::json!(val)
                }
                "System.Boolean" | "bool" | "BOOLEAN" => {
                    let val = reader.read_u8(address + field.offset as usize);
                    serde_json::json!(val != 0)
                }
                "System.String" | "string" => {
                    let str_ptr = reader.read_ptr(address + field.offset as usize);
                    if str_ptr == 0 {
                        serde_json::Value::Null
                    } else {
                        match reader.read_mono_string(str_ptr) {
                            Some(s) => serde_json::json!(s),
                            None => serde_json::Value::Null,
                        }
                    }
                }
                _ => {
                    // Assume it's a pointer/reference type
                    let ptr = reader.read_ptr(address + field.offset as usize);
                    if ptr == 0 {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!({
                            "type": "pointer",
                            "address": ptr,
                            "class_name": type_name
                        })
                    }
                }
            };

            fields.push(InstanceField {
                name: field.name.clone(),
                type_name: type_name.clone(),
                is_static: false,
                value,
            });
        }

        Ok(InstanceResponse {
            class_name: typedef.name.clone(),
            namespace: typedef.namespace_name.clone(),
            address,
            fields,
        })
    })?;

    Ok(Json(response))
}

async fn read_instance_field(
    Path((instance_addr_str, field_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = with_reader(|reader| -> Result<serde_json::Value, StatusCode> {
        // Parse instance address
        let instance_addr = if instance_addr_str.starts_with("0x") {
            usize::from_str_radix(&instance_addr_str[2..], 16)
        } else {
            instance_addr_str.parse::<usize>()
        }
        .map_err(|_| StatusCode::BAD_REQUEST)?;

        // Read instance vtable and class to get field definitions
        let vtable_ptr = reader.read_ptr(instance_addr);
        if vtable_ptr == 0 {
            return Err(StatusCode::NOT_FOUND);
        }

        let class_ptr = reader.read_ptr(vtable_ptr);
        let typedef = TypeDefinition::new(class_ptr, &reader);

        // Find the field
        let field_addrs = typedef.get_fields();
        let field_addr = field_addrs
            .iter()
            .find(|&&addr| {
                let field = FieldDefinition::new(addr, &reader);
                field.name == field_name
            })
            .ok_or(StatusCode::NOT_FOUND)?;

        let field = FieldDefinition::new(*field_addr, &reader);

        // Calculate field address (instance base + field offset)
        let field_location = instance_addr + field.offset as usize;

        // Get type name
        let type_name = match field.type_info.clone().code() {
            TypeCode::CLASS | TypeCode::VALUETYPE => {
                let typedef = TypeDefinition::new(field.type_info.data, &reader);
                format!("{}.{}", typedef.namespace_name, typedef.name)
            }
            TypeCode::I4 => "System.Int32".to_string(),
            TypeCode::U4 => "System.UInt32".to_string(),
            TypeCode::I8 => "System.Int64".to_string(),
            TypeCode::U8 => "System.UInt64".to_string(),
            TypeCode::BOOLEAN => "System.Boolean".to_string(),
            TypeCode::STRING => "System.String".to_string(),
            _ => format!("TypeCode({})", field.type_info.type_code)
        };

        // Read value based on type
        let result = match type_name.as_str() {
            "System.Int32" => {
                let val = reader.read_i32(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "int32",
                    "value": val
                })
            }
            "System.UInt32" => {
                let val = reader.read_u32(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "uint32",
                    "value": val
                })
            }
            "System.Int64" => {
                let val = reader.read_i64(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "int64",
                    "value": val
                })
            }
            "System.UInt64" => {
                let val = reader.read_u64(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "uint64",
                    "value": val.to_string()
                })
            }
            "System.Boolean" => {
                let val = reader.read_u8(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "boolean",
                    "value": val != 0
                })
            }
            _ => {
                // For reference types, read as pointer
                let ptr = reader.read_ptr(field_location);
                if ptr == 0 {
                    serde_json::json!({
                        "type": "null",
                        "address": 0
                    })
                } else {
                    serde_json::json!({
                        "type": "pointer",
                        "address": ptr,
                        "field_name": field.name,
                        "class_name": type_name
                    })
                }
            }
        };

        Ok(result)
    })?;

    Ok(Json(result))
}

async fn read_static_field(
    Path((class_addr_str, field_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = with_reader(|reader| -> Result<serde_json::Value, StatusCode> {
        // Parse class address
        let class_addr = if class_addr_str.starts_with("0x") {
            usize::from_str_radix(&class_addr_str[2..], 16)
        } else {
            class_addr_str.parse::<usize>()
        }
        .map_err(|_| StatusCode::BAD_REQUEST)?;

        let typedef = TypeDefinition::new(class_addr, &reader);

        // Find the field
        let field_addrs = typedef.get_fields();
        let field_addr = field_addrs
            .iter()
            .find(|&&addr| {
                let field = FieldDefinition::new(addr, &reader);
                field.name == field_name
            })
            .ok_or(StatusCode::NOT_FOUND)?;

        let field = FieldDefinition::new(*field_addr, &reader);

        // Read static field value using get_static_value (which applies the correct offset)
        if !field.type_info.is_static {
            return Err(StatusCode::BAD_REQUEST); // Not a static field
        }

        // Use the fixed get_static_value method
        let (field_location, _) = typedef.get_static_value(&field.name);

        if field_location == 0 {
            return Ok(serde_json::Value::Null);
        }

        // Determine the type and read the appropriate value
        let type_name = match field.type_info.clone().code() {
            TypeCode::CLASS | TypeCode::VALUETYPE | TypeCode::GENERICINST => {
                let typedef = TypeDefinition::new(field.type_info.data, &reader);
                format!("{}.{}", typedef.namespace_name, typedef.name)
            }
            TypeCode::I4 => "System.Int32".to_string(),
            TypeCode::U4 => "System.UInt32".to_string(),
            TypeCode::I8 => "System.Int64".to_string(),
            TypeCode::U8 => "System.UInt64".to_string(),
            TypeCode::BOOLEAN => "System.Boolean".to_string(),
            TypeCode::STRING => "System.String".to_string(),
            _ => format!("TypeCode({})", field.type_info.type_code)
        };

        // Read value based on type
        let result = match type_name.as_str() {
            "System.Int32" => {
                let val = reader.read_i32(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "int32",
                    "value": val
                })
            }
            "System.UInt32" => {
                let val = reader.read_u32(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "uint32",
                    "value": val
                })
            }
            "System.Int64" => {
                let val = reader.read_i64(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "int64",
                    "value": val
                })
            }
            "System.UInt64" => {
                let val = reader.read_u64(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "uint64",
                    "value": val.to_string() // Convert to string to avoid JSON precision issues
                })
            }
            "System.Boolean" => {
                let val = reader.read_u8(field_location);
                serde_json::json!({
                    "type": "primitive",
                    "value_type": "boolean",
                    "value": val != 0
                })
            }
            _ => {
                // For reference types, read as pointer
                let field_value_addr = reader.read_ptr(field_location);
                if field_value_addr == 0 {
                    serde_json::json!({
                        "type": "null",
                        "address": 0
                    })
                } else {
                    serde_json::json!({
                        "type": "pointer",
                        "address": field_value_addr,
                        "field_name": field.name,
                        "class_name": type_name
                    })
                }
            }
        };

        Ok(result)
    })?;

    Ok(Json(result))
}

#[derive(Serialize)]
struct DictionaryEntry {
    key: serde_json::Value,
    value: serde_json::Value,
}

#[derive(Serialize)]
struct DictionaryResponse {
    count: i32,
    entries: Vec<DictionaryEntry>,
}

async fn read_dictionary(
    Path(dict_addr_str): Path<String>,
) -> Result<Json<DictionaryResponse>, StatusCode> {
    let result = with_reader(|reader| -> Result<DictionaryResponse, StatusCode> {
        // Parse dictionary address
        let dict_addr = if dict_addr_str.starts_with("0x") {
            usize::from_str_radix(&dict_addr_str[2..], 16)
        } else {
            dict_addr_str.parse::<usize>()
        }
        .map_err(|_| StatusCode::BAD_REQUEST)?;

        if dict_addr == 0 {
            return Err(StatusCode::BAD_REQUEST);
        }

        // Try standard Dictionary offsets first (_entries at 0x18)
        let entries_ptr_0x18 = reader.read_ptr(dict_addr + 0x18);

        // Check if this is a valid array by reading its length at offset 0x18 from array base
        if entries_ptr_0x18 > 0x10000 {
            let array_length = reader.read_i32(entries_ptr_0x18 + 0x18);
            if array_length > 0 && array_length < 100000 {
                return read_dict_entries(reader, entries_ptr_0x18, array_length);
            }
        }

        // Try alternative offsets (_entries at 0x10)
        let entries_ptr_0x10 = reader.read_ptr(dict_addr + 0x10);
        if entries_ptr_0x10 > 0x10000 {
            let array_length = reader.read_i32(entries_ptr_0x10 + 0x18);
            if array_length > 0 && array_length < 100000 {
                return read_dict_entries(reader, entries_ptr_0x10, array_length);
            }
        }

        Err(StatusCode::NOT_FOUND)
    })?;

    Ok(Json(result))
}

fn read_dict_entries(reader: &MonoReader, entries_ptr: usize, count: i32) -> Result<DictionaryResponse, StatusCode> {
    // Entry structure: { int hashCode; int next; TKey key; TValue value; }
    // For Dictionary<uint, int>: 16 bytes (4 + 4 + 4 + 4)
    let entry_size = 16usize;
    let entries_start = entries_ptr + mtga_reader::constants::SIZE_OF_PTR * 4; // Skip array header

    let mut entries = Vec::new();
    let max_read = std::cmp::min(count, 1000); // Limit to prevent huge responses

    for i in 0..max_read {
        let entry_addr = entries_start + (i as usize * entry_size);

        let hash_code = reader.read_i32(entry_addr);
        let key = reader.read_u32(entry_addr + 8);
        let value = reader.read_i32(entry_addr + 12);

        // Only include valid entries (hashCode >= 0 means occupied slot)
        if hash_code >= 0 && key > 0 {
            entries.push(DictionaryEntry {
                key: serde_json::json!(key),
                value: serde_json::json!(value),
            });
        }
    }

    Ok(DictionaryResponse {
        count: entries.len() as i32,
        entries,
    })
}

#[tokio::main]
async fn main() {
    println!("Starting MTGA Reader HTTP Server...");

    // Check for admin privileges
    if !MonoReader::is_admin() {
        eprintln!("Error: This program requires administrator privileges");
        std::process::exit(1);
    }

    // Find MTGA process
    let pid = match MonoReader::find_pid_by_name("MTGA") {
        Some(pid) => pid,
        None => {
            eprintln!("Error: MTGA process not found. Please start MTGA first.");
            std::process::exit(1);
        }
    };

    println!("Found MTGA process with PID: {}", pid);

    // Initialize MonoReader
    let mut mono_reader = MonoReader::new(pid.as_u32());
    let mono_root = mono_reader.read_mono_root_domain();

    if mono_root == 0 {
        eprintln!("Error: Failed to read mono root domain");
        std::process::exit(1);
    }

    println!("Connected to MTGA process (Mono root: 0x{:x})", mono_root);

    // Store in global (unsafe but necessary for this single-threaded use case)
    unsafe {
        MONO_READER = Some(mono_reader);
    }

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        .route("/assemblies", get(get_assemblies))
        .route("/assembly/:name/classes", get(get_assembly_classes))
        .route("/assembly/:assembly/class/:class", get(get_class_details))
        .route("/instance/:address", get(get_instance))
        .route("/instance/:address/field/:field_name", get(read_instance_field))
        .route("/class/:address/field/:field_name", get(read_static_field))
        .route("/dictionary/:address", get(read_dictionary))
        .layer(cors);

    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("Failed to bind to port 8080");

    println!("Server listening on http://127.0.0.1:8080");
    println!("Press Ctrl+C to stop");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
