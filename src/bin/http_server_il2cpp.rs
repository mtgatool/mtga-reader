//! IL2CPP (macOS) debug HTTP server for `debug-ui`.
//!
//! Mirrors `http_server_simple.rs` (Mono/Windows): same routes, same response
//! shapes, so the same `debug-ui` works against either platform and the typed
//! reads can be compared side by side.
//!
//! Build as your user, then run the binary as root:
//!   cargo build --bin http_server_il2cpp
//!   sudo ./target/debug/http_server_il2cpp

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("http_server_il2cpp is macOS-only; use http_server_simple on Windows/Linux");
}

#[cfg(target_os = "macos")]
use axum::{
    extract::Path,
    response::Json,
    routing::get,
    Router,
};
#[cfg(target_os = "macos")]
use serde::Serialize;
#[cfg(target_os = "macos")]
use serde_json::json;
#[cfg(target_os = "macos")]
use tower_http::cors::{Any, CorsLayer};

#[cfg(target_os = "macos")]
use mtga_reader::il2cpp::macos_runtime::{plausible, Il2Cpp};
#[cfg(target_os = "macos")]
use mtga_reader::queries_il2cpp as q;
#[cfg(target_os = "macos")]
use mtga_reader::session_il2cpp as session;

#[cfg(target_os = "macos")]
const PROCESS: &str = "MTGA";

// ---------------------------------------------------------------------------
// Response types (identical shapes to the Mono server)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct AssembliesResponse {
    assemblies: Vec<String>,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct ClassInfo {
    name: String,
    namespace: String,
    address: usize,
    is_static: bool,
    is_enum: bool,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct ClassesResponse {
    classes: Vec<ClassInfo>,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct FieldInfo {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    offset: i32,
    is_static: bool,
    is_const: bool,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct StaticInstanceInfo {
    field_name: String,
    address: usize,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct ClassDetailsResponse {
    name: String,
    namespace: String,
    address: usize,
    fields: Vec<FieldInfo>,
    static_instances: Vec<StaticInstanceInfo>,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct InstanceField {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    is_static: bool,
    value: serde_json::Value,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct InstanceResponse {
    class_name: String,
    namespace: String,
    address: usize,
    fields: Vec<InstanceField>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse `0x…` or decimal addresses coming from the UI.
#[cfg(target_os = "macos")]
fn parse_addr(s: &str) -> Option<usize> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok().or_else(|| usize::from_str_radix(s, 16).ok())
    }
}

#[cfg(target_os = "macos")]
fn type_name_of(rt: &Il2Cpp, f: &mtga_reader::il2cpp::macos_runtime::FieldRec) -> String {
    rt.type_name(f.type_ptr, f.type_code)
}

/// Run against the cached session (attaching on first use).
#[cfg(target_os = "macos")]
fn with_rt<F>(f: F) -> serde_json::Value
where
    F: Fn(&Il2Cpp) -> serde_json::Value,
{
    session::read_raw(PROCESS, f)
}

// ---------------------------------------------------------------------------
// Typed readers — delegate to the shared library so the server and the .node
// addon use one implementation.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
async fn get_decks() -> Json<serde_json::Value> {
    Json(session::read_decks(PROCESS))
}

#[cfg(target_os = "macos")]
async fn get_ranks() -> Json<serde_json::Value> {
    Json(session::read_ranks(PROCESS))
}

#[cfg(target_os = "macos")]
async fn get_account() -> Json<serde_json::Value> {
    Json(session::read_account(PROCESS))
}

#[cfg(target_os = "macos")]
async fn get_collection() -> Json<serde_json::Value> {
    Json(session::read_collection(PROCESS))
}

#[cfg(target_os = "macos")]
async fn get_inventory() -> Json<serde_json::Value> {
    Json(session::read_inventory(PROCESS))
}

/// GET /read?path=A,B,C — walk a `[Class, StaticField, field, ...]` path.
#[cfg(target_os = "macos")]
async fn read_path(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let path = params.get("path").cloned().unwrap_or_default();
    let fields: Vec<String> = path
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if fields.len() < 2 {
        return Json(json!({
            "error": "pass ?path=Class,StaticField[,field...] (at least two segments)"
        }));
    }

    Json(with_rt(|rt| q::read_data_path(rt, &fields)))
}

// ---------------------------------------------------------------------------
// Explorer endpoints (used by debug-ui)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
async fn get_assemblies() -> Json<AssembliesResponse> {
    // IL2CPP merges everything into GameAssembly; the type table is flat.
    Json(AssembliesResponse {
        assemblies: vec!["GameAssembly".to_string()],
    })
}

/// All classes in the type-info table.
#[cfg(target_os = "macos")]
async fn get_assembly_classes(Path(_name): Path<String>) -> Json<ClassesResponse> {
    let v = with_rt(|rt| {
        let table = rt
            .mem
            .meta_bytes(rt.type_info_table, rt.type_count * 8)
            .unwrap_or_default();

        let mut classes = Vec::new();
        for chunk in table.chunks_exact(8) {
            let class = usize::from_le_bytes(chunk.try_into().unwrap());
            if !plausible(class) || !rt.is_class(class) {
                continue;
            }
            let name = rt.class_name(class);
            if name.is_empty() {
                continue;
            }
            classes.push(json!({
                "name": name,
                "namespace": rt.class_namespace(class),
                "address": class,
                "is_static": false,
                "is_enum": false,
            }));
        }
        json!(classes)
    });

    let classes = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| ClassInfo {
                    name: c["name"].as_str().unwrap_or_default().to_string(),
                    namespace: c["namespace"].as_str().unwrap_or_default().to_string(),
                    address: c["address"].as_u64().unwrap_or(0) as usize,
                    is_static: false,
                    is_enum: false,
                })
                .collect()
        })
        .unwrap_or_default();

    Json(ClassesResponse { classes })
}

/// Substring search over class names — the table is large, so this is how the
/// UI finds anything without listing 39k entries.
#[cfg(target_os = "macos")]
async fn search_classes(Path(term): Path<String>) -> Json<ClassesResponse> {
    let needle = term.to_lowercase();
    let v = with_rt(|rt| {
        let table = rt
            .mem
            .meta_bytes(rt.type_info_table, rt.type_count * 8)
            .unwrap_or_default();

        let mut classes = Vec::new();
        for chunk in table.chunks_exact(8) {
            let class = usize::from_le_bytes(chunk.try_into().unwrap());
            if !plausible(class) || !rt.is_class(class) {
                continue;
            }
            let name = rt.class_name(class);
            if name.is_empty() || !name.to_lowercase().contains(&needle) {
                continue;
            }
            classes.push(json!({
                "name": name,
                "namespace": rt.class_namespace(class),
                "address": class,
            }));
            if classes.len() >= 500 {
                break;
            }
        }
        json!(classes)
    });

    let classes = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| ClassInfo {
                    name: c["name"].as_str().unwrap_or_default().to_string(),
                    namespace: c["namespace"].as_str().unwrap_or_default().to_string(),
                    address: c["address"].as_u64().unwrap_or(0) as usize,
                    is_static: false,
                    is_enum: false,
                })
                .collect()
        })
        .unwrap_or_default();

    Json(ClassesResponse { classes })
}

#[cfg(target_os = "macos")]
async fn get_class_details(
    Path((_assembly, class_name)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    Json(with_rt(|rt| {
        let class = match rt.find_class(&class_name) {
            Some(c) => c,
            None => return json!({ "error": format!("class '{class_name}' not found") }),
        };

        // Own + inherited fields, derived shadowing base.
        let mut fields: Vec<FieldInfo> = Vec::new();
        let mut cur = class;
        for _ in 0..16 {
            if cur == 0 {
                break;
            }
            for f in rt.class_fields(cur).iter() {
                if fields.iter().any(|e| e.name == f.name) {
                    continue;
                }
                fields.push(FieldInfo {
                    name: f.name.clone(),
                    type_name: type_name_of(rt, f),
                    offset: f.offset,
                    is_static: f.is_static,
                    is_const: false,
                });
            }
            cur = rt.class_parent(cur);
        }

        // Statics that currently hold a live object, so the UI can dive in.
        let mut static_instances = Vec::new();
        for f in rt.class_fields(class).iter().filter(|f| f.is_static) {
            if let Some((addr, _)) = rt.static_field_addr(class, &f.name) {
                let ptr = rt.mem.read_ptr(addr);
                if plausible(ptr) && rt.class_of(ptr) != 0 {
                    static_instances.push(StaticInstanceInfo {
                        field_name: f.name.clone(),
                        address: ptr,
                    });
                }
            }
        }

        serde_json::to_value(ClassDetailsResponse {
            name: rt.class_name(class),
            namespace: rt.class_namespace(class),
            address: class,
            fields,
            static_instances,
        })
        .unwrap_or_else(|e| json!({ "error": e.to_string() }))
    }))
}

#[cfg(target_os = "macos")]
async fn get_instance(Path(address): Path<String>) -> Json<serde_json::Value> {
    let addr = match parse_addr(&address) {
        Some(a) => a,
        None => return Json(json!({ "error": format!("bad address '{address}'") })),
    };

    Json(with_rt(move |rt| {
        let class = rt.class_of(addr);
        if class == 0 {
            return json!({ "error": format!("0x{addr:x} is not a managed object") });
        }

        let mut fields: Vec<InstanceField> = Vec::new();
        let mut cur = class;
        for _ in 0..16 {
            if cur == 0 {
                break;
            }
            for f in rt.class_fields(cur).iter() {
                if f.is_static || f.is_thread_static || fields.iter().any(|e| e.name == f.name) {
                    continue;
                }
                fields.push(InstanceField {
                    name: f.name.clone(),
                    type_name: type_name_of(rt, f),
                    is_static: false,
                    value: q::value_json(rt, addr + f.offset as usize, f.type_code, f.type_ptr, 1),
                });
            }
            cur = rt.class_parent(cur);
        }

        serde_json::to_value(InstanceResponse {
            class_name: rt.class_name(class),
            namespace: rt.class_namespace(class),
            address: addr,
            fields,
        })
        .unwrap_or_else(|e| json!({ "error": e.to_string() }))
    }))
}

#[cfg(target_os = "macos")]
async fn read_instance_field(
    Path((address, field_name)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let addr = match parse_addr(&address) {
        Some(a) => a,
        None => return Json(json!({ "error": format!("bad address '{address}'") })),
    };

    Json(with_rt(move |rt| {
        match rt.field_addr(addr, &field_name) {
            Some((fa, f)) => {
                let value = q::value_json(rt, fa, f.type_code, f.type_ptr, 2);
                // debug-ui follows `address` to drill into reference fields.
                let ptr = rt.mem.read_ptr(fa);
                json!({
                    "name": field_name,
                    "type": type_name_of(rt, &f),
                    "value": value,
                    "address": if plausible(ptr) && rt.class_of(ptr) != 0 { ptr } else { 0 },
                })
            }
            None => json!({ "error": format!("field '{field_name}' not found") }),
        }
    }))
}

#[cfg(target_os = "macos")]
async fn read_static_field(
    Path((address, field_name)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let class = match parse_addr(&address) {
        Some(a) => a,
        None => return Json(json!({ "error": format!("bad address '{address}'") })),
    };

    Json(with_rt(move |rt| {
        match rt.static_field_addr(class, &field_name) {
            Some((fa, f)) => {
                let ptr = rt.mem.read_ptr(fa);
                json!({
                    "name": field_name,
                    "type": type_name_of(rt, &f),
                    "value": q::value_json(rt, fa, f.type_code, f.type_ptr, 2),
                    "address": if plausible(ptr) && rt.class_of(ptr) != 0 { ptr } else { 0 },
                })
            }
            None => json!({ "error": format!("static field '{field_name}' not found") }),
        }
    }))
}

#[cfg(target_os = "macos")]
async fn read_dictionary(Path(address): Path<String>) -> Json<serde_json::Value> {
    let addr = match parse_addr(&address) {
        Some(a) => a,
        None => return Json(json!({ "error": format!("bad address '{address}'") })),
    };

    Json(with_rt(move |rt| {
        if rt.class_of(addr) == 0 {
            return json!({ "error": format!("0x{addr:x} is not a managed object") });
        }
        let entries: Vec<serde_json::Value> = rt
            .dict_entries(addr, 100_000)
            .into_iter()
            .map(|(ka, kc, va, vc)| {
                json!({
                    "key": q::value_json(rt, ka, kc, 0, 1),
                    "value": q::value_json(rt, va, vc, 0, 1),
                })
            })
            .collect();

        json!({ "count": entries.len(), "entries": entries })
    }))
}

/// GET /singletons — classes whose statics currently hold a live instance.
/// The macOS analogue of the Mono server's singleton scan; this is how you find
/// a new root after a game update.
#[cfg(target_os = "macos")]
async fn find_singletons() -> Json<serde_json::Value> {
    Json(with_rt(|rt| {
        let table = rt
            .mem
            .meta_bytes(rt.type_info_table, rt.type_count * 8)
            .unwrap_or_default();

        let mut found = Vec::new();
        for chunk in table.chunks_exact(8) {
            let class = usize::from_le_bytes(chunk.try_into().unwrap());
            if !plausible(class) || !rt.is_class(class) {
                continue;
            }
            let statics = rt.class_static_storage(class);
            if !plausible(statics) {
                continue;
            }

            for f in rt.class_fields(class).iter() {
                if !f.is_static || f.is_thread_static {
                    continue;
                }
                let lname = f.name.to_lowercase();
                if !lname.contains("instance") {
                    continue;
                }
                let ptr = rt.mem.read_ptr(statics + f.offset as usize);
                if !plausible(ptr) {
                    continue;
                }
                // Only report when the static really points at an instance of
                // its own class — that's what makes it a usable root.
                if rt.class_of(ptr) == class {
                    found.push(json!({
                        "class": rt.class_name(class),
                        "namespace": rt.class_namespace(class),
                        "classAddress": class,
                        "field": f.name,
                        "address": ptr,
                    }));
                }
            }
            if found.len() >= 400 {
                break;
            }
        }

        json!({ "count": found.len(), "singletons": found })
    }))
}

/// GET /debug/probe/:address — raw words, for eyeballing an unknown object.
#[cfg(target_os = "macos")]
async fn debug_probe(Path(address): Path<String>) -> Json<serde_json::Value> {
    probe_impl(address, 128).await
}

#[cfg(target_os = "macos")]
async fn debug_probe_size(Path((address, size)): Path<(String, usize)>) -> Json<serde_json::Value> {
    probe_impl(address, size.min(4096)).await
}

#[cfg(target_os = "macos")]
async fn probe_impl(address: String, size: usize) -> Json<serde_json::Value> {
    let addr = match parse_addr(&address) {
        Some(a) => a,
        None => return Json(json!({ "error": format!("bad address '{address}'") })),
    };

    Json(with_rt(move |rt| {
        let bytes = rt.mem.read_bytes(addr, size);
        if bytes.is_empty() {
            return json!({ "error": format!("could not read 0x{addr:x}") });
        }
        let words: Vec<serde_json::Value> = bytes
            .chunks_exact(8)
            .enumerate()
            .map(|(i, w)| {
                let v = usize::from_le_bytes(w.try_into().unwrap());
                let cls = if plausible(v) { rt.class_name(rt.class_of(v)) } else { String::new() };
                json!({
                    "offset": i * 8,
                    "hex": format!("0x{v:x}"),
                    "i64": v as i64,
                    "i32": i32::from_le_bytes(w[..4].try_into().unwrap()),
                    "class": if cls.is_empty() { serde_json::Value::Null } else { json!(cls) },
                })
            })
            .collect();

        json!({
            "address": addr,
            "class": rt.class_name(rt.class_of(addr)),
            "words": words,
        })
    }))
}

/// GET /status — attach state, for a quick sanity check.
#[cfg(target_os = "macos")]
async fn status() -> Json<serde_json::Value> {
    Json(with_rt(|rt| {
        json!({
            "pid": rt.mem.pid,
            "typeInfoTable": format!("0x{:x}", rt.type_info_table),
            "typeCount": rt.type_count,
            "dataSegment": format!("0x{:x}", rt.data_segment.0),
            "root": q::find_root(rt).map(|r| format!("0x{r:x}")),
        })
    }))
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!(
            "warning: not running as root — task_for_pid will fail.\n\
             Build as your user, then: sudo ./target/debug/http_server_il2cpp"
        );
    }

    let app = Router::new()
        .route("/status", get(status))
        .route("/read", get(read_path))
        .route("/decks", get(get_decks))
        .route("/ranks", get(get_ranks))
        .route("/account", get(get_account))
        .route("/collection", get(get_collection))
        .route("/inventory", get(get_inventory))
        .route("/singletons", get(find_singletons))
        .route("/assemblies", get(get_assemblies))
        .route("/search/:term", get(search_classes))
        .route("/assembly/:name/classes", get(get_assembly_classes))
        .route("/assembly/:assembly/class/:class", get(get_class_details))
        .route("/instance/:address", get(get_instance))
        .route("/instance/:address/field/:field_name", get(read_instance_field))
        .route("/class/:address/field/:field_name", get(read_static_field))
        .route("/dictionary/:address", get(read_dictionary))
        .route("/debug/probe/:address", get(debug_probe))
        .route("/debug/probe/:address/:size", get(debug_probe_size))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("failed to bind 127.0.0.1:8080");

    println!("IL2CPP debug server (macOS) on http://localhost:8080");
    println!("  /status /account /inventory /collection /ranks /decks");
    println!("  /singletons /search/:term /read?path=Class,StaticField,field");
    println!("  /assemblies /assembly/:n/classes /assembly/:a/class/:c");
    println!("  /instance/:addr /instance/:addr/field/:f /class/:addr/field/:f");
    println!("  /dictionary/:addr /debug/probe/:addr[/:size]");

    axum::serve(listener, app).await.unwrap();
}
