//! End-to-end check of the macOS/IL2CPP typed readers.
//!
//!   cargo build --bin test_readers_macos
//!   sudo ./target/debug/test_readers_macos

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("test_readers_macos is macOS-only");
}

#[cfg(target_os = "macos")]
fn main() {
    use std::time::Instant;

    let name = std::env::args().nth(1).unwrap_or_else(|| "MTGA".to_string());

    let t = Instant::now();
    match mtga_reader::session_il2cpp::init(&name) {
        Ok(_) => println!("init ok in {:?}", t.elapsed()),
        Err(e) => {
            eprintln!("init failed: {e}");
            std::process::exit(1);
        }
    }

    let mut brief = |label: &str, v: &serde_json::Value| {
        let s = serde_json::to_string(v).unwrap_or_default();
        let cut: String = s.chars().take(400).collect();
        println!("\n=== {label} ===\n{cut}{}", if s.len() > 400 { " ..." } else { "" });
    };

    for (label, f) in [
        ("account", mtga_reader::session_il2cpp::read_account as fn(&str) -> serde_json::Value),
        ("inventory", mtga_reader::session_il2cpp::read_inventory),
        ("ranks", mtga_reader::session_il2cpp::read_ranks),
    ] {
        let t = Instant::now();
        let v = f(&name);
        let el = t.elapsed();
        brief(&format!("{label} ({el:?})"), &v);
    }

    // Big ones: report shape rather than dumping everything.
    let t = Instant::now();
    let coll = mtga_reader::session_il2cpp::read_collection(&name);
    println!(
        "\n=== collection ({:?}) ===\ncount = {}, total copies = {}",
        t.elapsed(),
        coll["count"],
        coll["cards"]
            .as_array()
            .map(|a| a.iter().filter_map(|c| c["qty"].as_i64()).sum::<i64>())
            .unwrap_or(0)
    );
    println!("first 3: {}", &coll["cards"].as_array().map(|a| serde_json::to_string(&a[..a.len().min(3)]).unwrap_or_default()).unwrap_or_default());

    let t = Instant::now();
    let decks = mtga_reader::session_il2cpp::read_decks(&name);
    println!("\n=== decks ({:?}) ===\ncount = {}", t.elapsed(), decks["count"]);
    if let Some(list) = decks["decks"].as_array() {
        let with_cards = list
            .iter()
            .filter(|d| {
                d["piles"]
                    .as_array()
                    .map(|p| !p.is_empty())
                    .unwrap_or(false)
            })
            .count();
        println!("decks with at least one non-empty pile: {with_cards}");
        for d in list.iter().filter(|d| !d["piles"].as_array().map(|p| p.is_empty()).unwrap_or(true)).take(2) {
            println!(
                "  {:?} id={:?} format={:?} piles={}",
                d["name"],
                d["deckId"],
                d["attributes"]["Format"],
                serde_json::to_string(
                    &d["piles"]
                        .as_array()
                        .map(|ps| ps
                            .iter()
                            .map(|p| serde_json::json!({
                                "pile": p["pileName"],
                                "total": p["total"],
                                "unique": p["cards"].as_array().map(|c| c.len()).unwrap_or(0)
                            }))
                            .collect::<Vec<_>>())
                        .unwrap_or_default()
                )
                .unwrap_or_default()
            );
        }
    }

    // Repeat polls must reuse the cached session.
    println!("\n=== repeat poll timings (cached session) ===");
    for i in 0..3 {
        let t = Instant::now();
        let v = mtga_reader::session_il2cpp::read_inventory(&name);
        println!(
            "  inventory #{i}: {:?}  gold={} gems={}",
            t.elapsed(),
            v["gold"],
            v["gems"]
        );
    }
    let t = Instant::now();
    let _ = mtga_reader::session_il2cpp::read_collection(&name);
    println!("  collection (2nd): {:?}", t.elapsed());
}
