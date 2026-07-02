// Validate the modular backend: detect runtime, create + initialize a backend
// via create_backend(), and read through the RuntimeBackend/TypeDef traits.
//
// Run elevated: cargo run --bin test_backend --no-default-features --features mono

use mtga_reader::backend::detection::find_process_by_name;
use mtga_reader::backend::{create_backend, detect_runtime, RuntimeBackend};

fn find_wrapper_controller(backend: &dyn RuntimeBackend) -> Option<usize> {
    // Search the default assembly first, then all other assemblies.
    let mut candidates = backend.get_type_definitions();
    if let Some(addr) = candidates.iter().copied().find(|a| {
        backend.create_type_def(*a).name() == "WrapperController"
    }) {
        return Some(addr);
    }

    for asm in backend.get_assembly_names() {
        if asm == "Assembly-CSharp" {
            continue;
        }
        if let Some(img) = backend.get_assembly_image(&asm) {
            candidates = backend.get_type_definitions_for_image(img);
            if let Some(addr) = candidates.iter().copied().find(|a| {
                backend.create_type_def(*a).name() == "WrapperController"
            }) {
                println!("  (WrapperController found in assembly '{}')", asm);
                return Some(addr);
            }
        }
    }
    None
}

fn main() {
    println!("=== modular backend smoke test ===\n");

    let pid = match find_process_by_name("MTGA") {
        Some(p) => p.as_u32() as u32,
        None => {
            println!("MTGA not found.");
            return;
        }
    };
    println!("PID: {}", pid);
    println!("detect_runtime: {}", detect_runtime(pid));

    let backend = match create_backend(pid) {
        Ok(b) => b,
        Err(e) => {
            println!("create_backend failed: {}", e);
            return;
        }
    };

    println!("runtime_name: {}", backend.runtime_name());
    println!("is_initialized: {}", backend.is_initialized());

    let assemblies = backend.get_assembly_names();
    println!("assemblies: {} loaded", assemblies.len());

    let defs = backend.get_type_definitions();
    println!("default-assembly type defs: {}", defs.len());

    match find_wrapper_controller(backend.as_ref()) {
        Some(addr) => {
            let td = backend.create_type_def(addr);
            println!(
                "\nWrapperController @ 0x{:x}: name='{}' ns='{}' fields={} vtable_size={}",
                addr,
                td.name(),
                td.namespace(),
                td.field_count(),
                td.vtable_size()
            );
            println!("\n✓ modular backend is functional");
        }
        None => println!("\n✗ WrapperController not found via backend"),
    }

    println!("\n=== done ===");
}
