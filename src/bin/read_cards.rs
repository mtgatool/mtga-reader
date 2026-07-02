// Validate the production read_data() path (the same one the Node bindings use)
// against the corrected card-collection path.
//
// Run elevated: cargo run --bin read_cards --no-default-features --features mono

#[cfg(target_os = "windows")]
fn main() {
    let path: Vec<String> = [
        "WrapperController",
        "<Instance>k__BackingField",
        "<InventoryManager>k__BackingField",
        "InventoryServiceWrapper",
        "<Cards>k__BackingField",
        "_entries",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    println!("Calling read_data with path:\n  {}\n", path.join(" -> "));

    let data = mtga_reader::read_data("MTGA".to_string(), path);

    match &data {
        serde_json::Value::Array(arr) => {
            println!("Got ARRAY with {} elements. First 10:", arr.len());
            for v in arr.iter().take(10) {
                println!("  {}", v);
            }
        }
        serde_json::Value::Object(_) => {
            let s = serde_json::to_string(&data).unwrap_or_default();
            let preview: String = s.chars().take(600).collect();
            println!("Got OBJECT (preview): {}", preview);
        }
        other => println!("Got: {}", other),
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("read_cards is Windows-only.");
}
