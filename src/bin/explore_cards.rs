// One-shot exploration tool to locate the (relocated) card collection in the
// current MTGA build. Runs a bounded BFS from the WrapperController instance,
// following game-logic reference fields, and reports any path whose field or
// class name looks inventory/card/collection related.
//
// Run elevated: cargo run --bin explore_cards --no-default-features --features mono

use std::collections::{HashSet, VecDeque};

use mtga_reader::field_definition::FieldDefinition;
use mtga_reader::mono_reader::MonoReader;
use mtga_reader::type_definition::TypeDefinition;

const CLASS: u32 = 0x12;
const GENERICINST: u32 = 0x15;
const OBJECT: u32 = 0x1c;
const SZARRAY: u32 = 0x1d;

const MAX_DEPTH: usize = 5;
const MAX_NODES: usize = 12000;

fn is_ref_code(code: u32) -> bool {
    matches!(code, CLASS | GENERICINST | OBJECT | SZARRAY)
}

fn valid_ptr(p: usize) -> bool {
    p > 0x10000 && p < 0x7FFF_FFFF_FFFF
}

fn find_type(mono_reader: &mut MonoReader, class_name: &str) -> Option<usize> {
    let asm_image = mono_reader.read_assembly_image();
    if asm_image != 0 {
        let defs = mono_reader.create_type_definitions_for_image(asm_image);
        if let Some(a) = defs.iter().find_map(|d| {
            let td = TypeDefinition::new(*d, mono_reader);
            if td.name == class_name { Some(*d) } else { None }
        }) {
            return Some(a);
        }
    }
    let assemblies = mono_reader.get_all_assembly_names();
    for asm_name in assemblies {
        if asm_name == "Assembly-CSharp" {
            continue;
        }
        let img = mono_reader.read_assembly_image_by_name(&asm_name);
        if img == 0 {
            continue;
        }
        let defs = mono_reader.create_type_definitions_for_image(img);
        if let Some(a) = defs.iter().find_map(|d| {
            let td = TypeDefinition::new(*d, mono_reader);
            if td.name == class_name { Some(*d) } else { None }
        }) {
            return Some(a);
        }
    }
    None
}

fn class_of(r: &MonoReader, obj_ptr: usize) -> usize {
    let vtable = r.read_ptr(obj_ptr);
    if !valid_ptr(vtable) {
        return 0;
    }
    r.read_ptr(vtable)
}

/// Collect (name, offset, type_code) for own + inherited instance fields, bounded.
fn collect_fields(r: &MonoReader, class_addr: usize) -> Vec<(String, i32, u32)> {
    let mut out = Vec::new();
    let mut cur = class_addr;
    let mut depth = 0;
    while depth < 8 && valid_ptr(cur) {
        let td = TypeDefinition::new(cur, r);
        if td.name.is_empty() || td.name.len() > 200 {
            break;
        }
        if td.field_count > 0 && td.field_count < 2000 {
            for fa in td.get_fields() {
                let fd = FieldDefinition::new(fa, r);
                if fd.type_info.is_static || fd.type_info.is_const {
                    continue;
                }
                out.push((fd.name.clone(), fd.offset, fd.type_info.clone().type_code));
            }
        }
        cur = td.parent_addr;
        depth += 1;
    }
    out
}

/// Should we descend into an object of this class during BFS?
fn should_follow(name: &str, ns: &str) -> bool {
    if name.is_empty() || name.starts_with('<') {
        return false;
    }
    if ns.starts_with("UnityEngine")
        || ns.starts_with("System")
        || ns.starts_with("TMPro")
        || ns.starts_with("Unity.")
        || name.ends_with("Module")
    {
        return false;
    }
    true
}

fn interesting(field_name: &str, class_name: &str) -> bool {
    let f = field_name.to_lowercase();
    let c = class_name.to_lowercase();
    f.contains("card")
        || f.contains("inventory")
        || f.contains("collection")
        || c.contains("inventoryservicewrapper")
        || c.contains("cardsandquantity")
}

fn main() {
    println!("=== MTGA card-collection explorer ===\n");

    let pid = match MonoReader::find_pid_by_name("MTGA") {
        Some(p) => p,
        None => {
            println!("MTGA not found.");
            return;
        }
    };
    let mut r = match MonoReader::new(pid.as_u32()) {
        Ok(x) => x,
        Err(e) => {
            println!("Failed to open MTGA (run elevated): {}", e);
            return;
        }
    };
    r.read_mono_root_domain();
    r.read_assembly_image();

    // --- Global class-name search for the relocated wrapper types ---
    println!("[global] classes whose name mentions Inventory/Cards service:");
    for asm in ["Assembly-CSharp", "Core", "SharedClientCore"] {
        let img = r.read_assembly_image_by_name(asm);
        if img == 0 {
            continue;
        }
        let defs = r.create_type_definitions_for_image(img);
        for d in &defs {
            let td = TypeDefinition::new(*d, &r);
            let n = td.name.to_lowercase();
            if (n.contains("inventory") && n.contains("wrapper"))
                || n.contains("inventoryservice")
                || n == "cardsandquantity"
            {
                println!("  [{}] {} [{}]", asm, td.name, td.namespace_name);
            }
        }
    }
    println!();

    // --- BFS from WrapperController instance ---
    let wc_addr = match find_type(&mut r, "WrapperController") {
        Some(a) => a,
        None => {
            println!("WrapperController not found.");
            return;
        }
    };
    let wc_td = TypeDefinition::new(wc_addr, &r);
    let (inst_addr, _ti) = wc_td.get_static_value("<Instance>k__BackingField");
    let instance = r.read_ptr(inst_addr);
    if !valid_ptr(instance) {
        println!("WrapperController.Instance is null.");
        return;
    }
    println!("[bfs] from WrapperController.Instance = 0x{:x}\n", instance);

    let mut visited: HashSet<usize> = HashSet::new();
    let mut q: VecDeque<(usize, usize, String, usize)> = VecDeque::new();
    visited.insert(instance);
    q.push_back((instance, wc_addr, "WrapperController".to_string(), 0));

    let mut nodes = 0;
    let mut hits = 0;

    while let Some((obj, class_addr, path, depth)) = q.pop_front() {
        nodes += 1;
        if nodes > MAX_NODES {
            println!("\n[bfs] node budget ({}) exhausted.", MAX_NODES);
            break;
        }
        let fields = collect_fields(&r, class_addr);
        for (fname, foff, fcode) in fields {
            if !is_ref_code(fcode) {
                continue;
            }
            let child = r.read_ptr(obj + foff as usize);
            if !valid_ptr(child) {
                continue;
            }
            let child_class = class_of(&r, child);
            let (cname, cns) = if valid_ptr(child_class) {
                let td = TypeDefinition::new(child_class, &r);
                (td.name.clone(), td.namespace_name.clone())
            } else {
                (String::new(), String::new())
            };

            let child_path = format!("{}.{}", path, fname);

            if interesting(&fname, &cname) {
                hits += 1;
                let arr = if fcode == SZARRAY { " [array]" } else { "" };
                println!(
                    "  HIT d{} {}\n        -> class '{}' [{}] code=0x{:x}{}",
                    depth + 1, child_path, cname, cns, fcode, arr
                );
                // If it looks like a dictionary, report its _entries/_count.
                report_dictionary(&r, child, child_class);
            }

            if depth + 1 < MAX_DEPTH
                && !visited.contains(&child)
                && fcode != SZARRAY
                && should_follow(&cname, &cns)
            {
                visited.insert(child);
                q.push_back((child, child_class, child_path, depth + 1));
            }
        }
    }

    println!("\n[bfs] visited {} nodes, {} interesting hits.", nodes, hits);
    println!("\n=== done ===");
}

/// If the object has a `_count`/`_entries` field pair (Dictionary/List), print it.
fn report_dictionary(r: &MonoReader, obj: usize, class_addr: usize) {
    let fields = collect_fields(r, class_addr);
    let mut count_off: Option<i32> = None;
    let mut entries_off: Option<i32> = None;
    for (n, off, _c) in &fields {
        if n == "_count" || n == "count" {
            count_off = Some(*off);
        }
        if n == "_entries" || n == "entries" {
            entries_off = Some(*off);
        }
    }
    if let Some(co) = count_off {
        let count = r.read_i32(obj + co as usize);
        print!("        dict _count={}", count);
        if let Some(eo) = entries_off {
            let entries = r.read_ptr(obj + eo as usize);
            let len = if valid_ptr(entries) {
                r.read_i32(entries + 0x18)
            } else {
                0
            };
            println!(", _entries=0x{:x} (array len {})", entries, len);
        } else {
            println!();
        }
    }
}
