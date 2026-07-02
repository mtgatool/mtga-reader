use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
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
// ---- Typed readers: delegate to the shared library (src/queries.rs) so the
// server and the .node binary use one implementation. ----

/// GET /decks — saved decks (name, deckId, format/attributes, piles + cards).
async fn get_decks() -> Json<serde_json::Value> {
    Json(mtga_reader::read_decks("MTGA".to_string()))
}

/// GET /ranks — constructed + limited rank info.
async fn get_ranks() -> Json<serde_json::Value> {
    Json(mtga_reader::read_ranks("MTGA".to_string()))
}

/// GET /account — account identity (displayName, accountId, ...).
async fn get_account() -> Json<serde_json::Value> {
    Json(mtga_reader::read_account("MTGA".to_string()))
}

/// GET /collection — owned-card collection (grpId -> qty).
async fn get_collection() -> Json<serde_json::Value> {
    Json(mtga_reader::read_collection("MTGA".to_string()))
}

/// GET /inventory — wallet (gems, gold, wildcards, vault, ...).
async fn get_inventory() -> Json<serde_json::Value> {
    Json(mtga_reader::read_inventory("MTGA".to_string()))
}

#[derive(Serialize)]
struct SingletonHit {
    owner: String,
    owner_ns: String,
    field: String,
    instance: String,
    class: String,
    namespace: String,
}

/// Find classes that currently hold a live static instance whose class/name
/// matches a filter. This is how we locate roots that exist DURING a match
/// (the DuelScene singletons), when WrapperController is unloaded.
///
/// Usage: GET /singletons?filter=duel,game,match&assemblies=Core,SharedClientCore&max=400
async fn find_singletons(Query(params): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let filter: Vec<String> = params
        .get("filter")
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "duel,game,match,gre,board,player,scene".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let asm_list: Vec<String> = params
        .get("assemblies")
        .cloned()
        .unwrap_or_else(|| "Assembly-CSharp,Core,SharedClientCore".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let max: usize = params.get("max").and_then(|s| s.parse().ok()).unwrap_or(400);

    let mut owned = match mtga_reader::get_reader("MTGA".to_string()) {
        Some(r) => r,
        None => return Json(serde_json::json!({ "error": "could not open MTGA process (run elevated)" })),
    };
    let reader = &mut owned;

    let mut hits: Vec<SingletonHit> = Vec::new();
    'outer: for asm in &asm_list {
        let img = if asm == "Assembly-CSharp" {
            reader.read_assembly_image()
        } else {
            reader.read_assembly_image_by_name(asm)
        };
        if img == 0 {
            continue;
        }
        for d in reader.create_type_definitions_for_image(img) {
            let td = TypeDefinition::new(d, reader);
            if td.field_count <= 0 || td.field_count >= 4000 || td.v_table == 0 {
                continue;
            }
            // Static field storage area for this class.
            let static_base = reader.read_ptr(
                td.v_table
                    + mtga_reader::constants::V_TABLE as usize
                    + mtga_reader::constants::SIZE_OF_PTR * td.v_table_size as usize,
            );
            if !explore_valid_ptr(static_base) {
                continue;
            }
            for fa in td.get_fields() {
                let fd = FieldDefinition::new(fa, reader);
                if !fd.type_info.is_static || fd.type_info.is_const {
                    continue;
                }
                let code = fd.type_info.type_code;
                if code != 0x12 && code != 0x1c {
                    continue; // only CLASS / OBJECT static fields
                }
                let obj = reader.read_ptr(static_base + fd.offset as usize);
                if !explore_valid_ptr(obj) {
                    continue;
                }
                let cls = {
                    let vt = reader.read_ptr(obj);
                    if explore_valid_ptr(vt) { reader.read_ptr(vt) } else { 0 }
                };
                if !explore_valid_ptr(cls) {
                    continue;
                }
                let ctd = TypeDefinition::new(cls, reader);
                if ctd.name.is_empty() || ctd.name.len() > 200 {
                    continue;
                }
                let hay = format!("{} {} {}", ctd.name, ctd.namespace_name, td.name).to_lowercase();
                if filter.iter().any(|f| hay.contains(f.as_str())) {
                    hits.push(SingletonHit {
                        owner: td.name.clone(),
                        owner_ns: td.namespace_name.clone(),
                        field: fd.name.clone(),
                        instance: format!("0x{:x}", obj),
                        class: ctd.name.clone(),
                        namespace: ctd.namespace_name.clone(),
                    });
                    if hits.len() >= max {
                        break 'outer;
                    }
                }
            }
        }
    }

    Json(serde_json::to_value(hits).unwrap_or_else(|_| serde_json::json!([])))
}

#[derive(Serialize)]
struct ExploreHit {
    depth: usize,
    path: String,
    class: String,
    namespace: String,
    code: u32,
    address: String,
}

#[derive(Serialize)]
struct ExploreResponse {
    root: String,
    nodes_visited: usize,
    hits: Vec<ExploreHit>,
}

const REF_CODES: [u32; 4] = [0x12 /*CLASS*/, 0x15 /*GENERICINST*/, 0x1c /*OBJECT*/, 0x1d /*SZARRAY*/];

fn explore_valid_ptr(p: usize) -> bool {
    p > 0x10000 && p < 0x7FFF_FFFF_FFFF
}

/// Bounded BFS over the object graph, rooted at WrapperController.Instance,
/// reporting reference fields whose field- or class-name matches a filter.
///
/// Usage: GET /explore?filter=match,player,life&depth=5&max=20000
/// If `filter` is omitted a match-oriented default set is used. This is the
/// tool for mapping live in-match structures without re-elevating.
async fn read_explore(Query(params): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let depth_limit: usize = params.get("depth").and_then(|s| s.parse().ok()).unwrap_or(5);
    let max_nodes: usize = params.get("max").and_then(|s| s.parse().ok()).unwrap_or(20000);
    let filters: Vec<String> = params
        .get("filter")
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| {
            "match,player,life,hand,zone,turn,phase,mana,battlefield,opponent,gamestate,duel,graveyard,library".to_string()
        })
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Use a fresh, fully-initialized reader (same path as read_data). The global
    // MONO_READER is not initialized for static-field reads.
    let mut owned = match mtga_reader::get_reader("MTGA".to_string()) {
        Some(r) => r,
        None => {
            return Json(serde_json::json!({ "error": "could not open MTGA process (run elevated)" }))
        }
    };
    let reader = &mut owned;
    {
        // Resolve BFS root: explicit ?root_addr=0x.. (for in-match / arbitrary
        // objects) or default to WrapperController.Instance (home screen).
        let (instance, root_class, root_label): (usize, usize, String) = match params.get("root_addr") {
            Some(ra) => {
                let addr = ra
                    .strip_prefix("0x")
                    .map(|h| usize::from_str_radix(h, 16).unwrap_or(0))
                    .unwrap_or_else(|| ra.parse().unwrap_or(0));
                if !explore_valid_ptr(addr) {
                    return Json(serde_json::json!({ "error": "invalid root_addr" }));
                }
                let cls = {
                    let vt = reader.read_ptr(addr);
                    if explore_valid_ptr(vt) { reader.read_ptr(vt) } else { 0 }
                };
                let label = params.get("root_name").cloned().unwrap_or_else(|| {
                    if explore_valid_ptr(cls) { TypeDefinition::new(cls, reader).name } else { "root".to_string() }
                });
                (addr, cls, label)
            }
            None => {
                let wc_addr = {
                    let mut found = None;
                    let img = reader.read_assembly_image();
                    if img != 0 {
                        for d in reader.create_type_definitions_for_image(img) {
                            if TypeDefinition::new(d, reader).name == "WrapperController" { found = Some(d); break; }
                        }
                    }
                    if found.is_none() {
                        for asm in reader.get_all_assembly_names() {
                            if asm == "Assembly-CSharp" { continue; }
                            let img = reader.read_assembly_image_by_name(&asm);
                            if img == 0 { continue; }
                            if let Some(d) = reader
                                .create_type_definitions_for_image(img)
                                .into_iter()
                                .find(|d| TypeDefinition::new(*d, reader).name == "WrapperController")
                            {
                                found = Some(d);
                                break;
                            }
                        }
                    }
                    found
                };
                let wc_addr = match wc_addr {
                    Some(a) => a,
                    None => return Json(serde_json::json!({ "error": "WrapperController not found" })),
                };
                let wc_td = TypeDefinition::new(wc_addr, reader);
                let (inst_addr, _ti) = wc_td.get_static_value("<Instance>k__BackingField");
                let inst = reader.read_ptr(inst_addr);
                if !explore_valid_ptr(inst) {
                    return Json(serde_json::json!({ "error": "WrapperController.Instance is null (in a match? use /singletons then /explore?root_addr=)" }));
                }
                (inst, wc_addr, "WrapperController".to_string())
            }
        };

        // Collect own + inherited fields for a class, bounded.
        let collect_fields = |reader: &MonoReader, class_addr: usize| -> Vec<(String, i32, u32)> {
            let mut out = Vec::new();
            let mut cur = class_addr;
            let mut d = 0;
            while d < 8 && explore_valid_ptr(cur) {
                let td = TypeDefinition::new(cur, reader);
                if td.name.is_empty() || td.name.len() > 200 { break; }
                if td.field_count > 0 && td.field_count < 4000 {
                    for fa in td.get_fields() {
                        let fd = FieldDefinition::new(fa, reader);
                        if fd.type_info.is_static || fd.type_info.is_const { continue; }
                        out.push((fd.name.clone(), fd.offset, fd.type_info.clone().type_code));
                    }
                }
                cur = td.parent_addr;
                d += 1;
            }
            out
        };

        let mut visited: HashSet<usize> = HashSet::new();
        let mut q: VecDeque<(usize, usize, String, usize)> = VecDeque::new();
        visited.insert(instance);
        q.push_back((instance, root_class, root_label, 0));

        let mut hits = Vec::new();
        let mut nodes = 0;

        while let Some((obj, class_addr, path, depth)) = q.pop_front() {
            nodes += 1;
            if nodes > max_nodes { break; }

            for (fname, foff, fcode) in collect_fields(reader, class_addr) {
                if !REF_CODES.contains(&fcode) { continue; }
                let child = reader.read_ptr(obj + foff as usize);
                if !explore_valid_ptr(child) { continue; }

                let child_class = {
                    let vt = reader.read_ptr(child);
                    if explore_valid_ptr(vt) { reader.read_ptr(vt) } else { 0 }
                };
                let (cname, cns) = if explore_valid_ptr(child_class) {
                    let td = TypeDefinition::new(child_class, reader);
                    (td.name.clone(), td.namespace_name.clone())
                } else {
                    (String::new(), String::new())
                };

                let child_path = format!("{}.{}", path, fname);
                let fl = fname.to_lowercase();
                let cl = cname.to_lowercase();
                if filters.iter().any(|f| fl.contains(f.as_str()) || cl.contains(f.as_str())) {
                    hits.push(ExploreHit {
                        depth: depth + 1,
                        path: child_path.clone(),
                        class: cname.clone(),
                        namespace: cns.clone(),
                        code: fcode,
                        address: format!("0x{:x}", child),
                    });
                }

                // Follow into game-logic objects only.
                let follow = !cname.is_empty()
                    && !cname.starts_with('<')
                    && !cns.starts_with("UnityEngine")
                    && !cns.starts_with("System")
                    && !cns.starts_with("TMPro")
                    && !cns.starts_with("Unity.")
                    && !cname.ends_with("Module");
                if depth + 1 < depth_limit && fcode != 0x1d && follow && !visited.contains(&child) {
                    visited.insert(child);
                    q.push_back((child, child_class, child_path, depth + 1));
                }
            }
        }

        Json(serde_json::to_value(ExploreResponse {
            root: format!("0x{:x}", instance),
            nodes_visited: nodes,
            hits,
        }).unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" })))
    }
}

/// Read an arbitrary field path via the library's read_data().
///
/// Usage: GET /read?path=WrapperController,<Instance>k__BackingField,...,_entries
/// The comma-separated `path` is the same array the Node `readData` API takes.
async fn read_path(Query(params): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let raw = match params.get("path") {
        Some(p) if !p.is_empty() => p.clone(),
        _ => {
            return Json(serde_json::json!({
                "error": "missing 'path' query param (comma-separated field names)"
            }))
        }
    };

    let fields: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // read_data opens its own (elevated) handle per call, so this works even
    // though the browsing endpoints use the global MONO_READER.
    Json(mtga_reader::read_data("MTGA".to_string(), fields))
}

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

            // Skip static fields - their values are not stored in the instance
            // They need to be read from the static data area via read_static_field
            if field.type_info.is_static {
                continue;
            }

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
            // Use contains() for more robust type matching
            let field_addr = address + field.offset as usize;
            let value = if type_name.contains("UInt32") || type_name == "uint" {
                // Check UInt32 before Int32 since "UInt32" contains "Int32"
                let val = reader.read_u32(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("Int32") || type_name == "int" {
                let val = reader.read_i32(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("UInt64") || type_name == "ulong" {
                let val = reader.read_u64(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("Int64") || type_name == "long" {
                let val = reader.read_i64(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("UInt16") || type_name == "ushort" {
                let val = reader.read_u16(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("Int16") || type_name == "short" {
                let val = reader.read_i16(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("Byte") && !type_name.contains("SByte") || type_name == "byte" {
                let val = reader.read_u8(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("SByte") || type_name == "sbyte" {
                let val = reader.read_i8(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("Single") || type_name == "float" {
                let val = reader.read_f32(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("Double") || type_name == "double" {
                let val = reader.read_f64(field_addr);
                serde_json::json!(val)
            } else if type_name.contains("Boolean") || type_name == "bool" {
                let val = reader.read_u8(field_addr);
                serde_json::json!(val != 0)
            } else if type_name.contains("String") || type_name == "string" {
                let str_ptr = reader.read_ptr(field_addr);
                if str_ptr == 0 {
                    serde_json::Value::Null
                } else {
                    match reader.read_mono_string(str_ptr) {
                        Some(s) => serde_json::json!(s),
                        None => serde_json::Value::Null,
                    }
                }
            } else {
                // Assume it's a pointer/reference type
                let ptr = reader.read_ptr(field_addr);
                if ptr == 0 {
                    serde_json::Value::Null
                } else {
                    serde_json::json!({
                        "type": "pointer",
                        "address": ptr,
                        "class_name": type_name
                    })
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

        // Read value based on type (use contains() for more robust matching)
        let result = if type_name.contains("UInt32") || type_name == "uint" {
            // Check UInt32 before Int32 since "UInt32" contains "Int32"
            let val = reader.read_u32(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint32",
                "value": val
            })
        } else if type_name.contains("Int32") || type_name == "int" {
            let val = reader.read_i32(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "int32",
                "value": val
            })
        } else if type_name.contains("UInt64") || type_name == "ulong" {
            let val = reader.read_u64(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint64",
                "value": val.to_string()
            })
        } else if type_name.contains("Int64") || type_name == "long" {
            let val = reader.read_i64(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "int64",
                "value": val
            })
        } else if type_name.contains("UInt16") || type_name == "ushort" {
            let val = reader.read_u16(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "uint16",
                "value": val
            })
        } else if type_name.contains("Int16") || type_name == "short" {
            let val = reader.read_i16(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "int16",
                "value": val
            })
        } else if type_name.contains("Byte") && !type_name.contains("SByte") || type_name == "byte" {
            let val = reader.read_u8(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "byte",
                "value": val
            })
        } else if type_name.contains("SByte") || type_name == "sbyte" {
            let val = reader.read_i8(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "sbyte",
                "value": val
            })
        } else if type_name.contains("Single") || type_name == "float" {
            let val = reader.read_f32(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "float",
                "value": val
            })
        } else if type_name.contains("Double") || type_name == "double" {
            let val = reader.read_f64(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "double",
                "value": val
            })
        } else if type_name.contains("Boolean") || type_name == "bool" {
            let val = reader.read_u8(field_location);
            serde_json::json!({
                "type": "primitive",
                "value_type": "boolean",
                "value": val != 0
            })
        } else {
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

        // For const fields, the value is stored in the assembly metadata (IL constant table),
        // not in runtime memory like static fields. Reading const values requires parsing
        // the assembly's metadata tables, which is complex.
        //
        // Const values are compile-time constants embedded in the DLL - they don't exist
        // as runtime memory locations that can be read directly.
        if field.type_info.is_const {
            let type_name = match field.type_info.clone().code() {
                TypeCode::I4 => "System.Int32",
                TypeCode::U4 => "System.UInt32",
                TypeCode::I8 => "System.Int64",
                TypeCode::U8 => "System.UInt64",
                TypeCode::BOOLEAN => "System.Boolean",
                TypeCode::STRING => "System.String",
                _ => "unknown"
            };

            return Ok(serde_json::json!({
                "type": "const",
                "value_type": type_name,
                "is_const": true,
                "field_addr": format!("0x{:x}", *field_addr),
                "message": "Const values are stored in assembly metadata, not runtime memory. Use dnSpy or similar tools to read const values from the DLL."
            }));
        }

        // Use the fixed get_static_value method for non-const static fields
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

// Debug probe endpoint - dump memory at address
async fn debug_probe(
    Path(address_str): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    debug_probe_size(Path((address_str, "64".to_string()))).await
}

async fn debug_probe_size(
    Path((address_str, size_str)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = with_reader(|reader| -> Result<serde_json::Value, StatusCode> {
        let address = if address_str.starts_with("0x") {
            usize::from_str_radix(&address_str[2..], 16)
        } else {
            address_str.parse::<usize>()
        }
        .map_err(|_| StatusCode::BAD_REQUEST)?;

        let size: usize = size_str.parse().unwrap_or(64);
        let size = size.min(256); // Max 256 bytes

        let mut data = Vec::new();
        for i in (0..size).step_by(8) {
            let ptr_val = reader.read_ptr(address + i);
            let u32_val = reader.read_u32(address + i);
            let i32_val = reader.read_i32(address + i);
            data.push(serde_json::json!({
                "offset": format!("0x{:02x}", i),
                "ptr": format!("0x{:016x}", ptr_val),
                "u32": u32_val,
                "i32": i32_val,
            }));
        }

        Ok(serde_json::json!({
            "address": format!("0x{:x}", address),
            "size": size,
            "data": data
        }))
    })?;

    Ok(Json(result))
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
    let mut mono_reader = match MonoReader::new(pid.as_u32()) {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("Error: Failed to open MTGA process (run elevated/as admin): {}", e);
            std::process::exit(1);
        }
    };
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
        .route("/read", get(read_path))
        .route("/explore", get(read_explore))
        .route("/decks", get(get_decks))
        .route("/ranks", get(get_ranks))
        .route("/account", get(get_account))
        .route("/collection", get(get_collection))
        .route("/inventory", get(get_inventory))
        .route("/singletons", get(find_singletons))
        .route("/assemblies", get(get_assemblies))
        .route("/assembly/:name/classes", get(get_assembly_classes))
        .route("/assembly/:assembly/class/:class", get(get_class_details))
        .route("/instance/:address", get(get_instance))
        .route("/instance/:address/field/:field_name", get(read_instance_field))
        .route("/class/:address/field/:field_name", get(read_static_field))
        .route("/dictionary/:address", get(read_dictionary))
        .route("/debug/probe/:address", get(debug_probe))
        .route("/debug/probe/:address/:size", get(debug_probe_size))
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
