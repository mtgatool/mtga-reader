//! macOS IL2CPP discovery probe.
//!
//! Verifies, against the live MTGA process, every assumption the typed readers
//! rely on: the `Il2CppClass` layout, the root singleton, the object graph
//! down to decks/collection/inventory/ranks, and the container strides.
//!
//! Build as your user, then run as root:
//!   cargo build --bin probe_il2cpp
//!   sudo ./target/debug/probe_il2cpp

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe_il2cpp is macOS-only");
}

#[cfg(target_os = "macos")]
fn main() {
    use mtga_reader::il2cpp::macos_runtime::{
        find_images, find_pid, plausible, ClassLayout, Il2Cpp, Mem,
    };
    use std::time::Instant;

    let t0 = Instant::now();

    let pid = match find_pid("MTGA") {
        Some(p) => p,
        None => {
            eprintln!("MTGA process not found");
            std::process::exit(1);
        }
    };
    println!("MTGA pid = {pid}");

    // --- raw access ---------------------------------------------------------
    let probe_mem = match Mem::open(pid) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };
    let regions = probe_mem.regions();
    println!("mapped regions: {}", regions.len());

    println!("\n=== mach-o images ===");
    let t_img = Instant::now();
    let images = find_images(&probe_mem);
    println!("found {} images in {:?}", images.len(), t_img.elapsed());
    let mut ranked: Vec<_> = images.iter().collect();
    ranked.sort_by_key(|i| {
        std::cmp::Reverse(i.data_segments.iter().map(|(_, _, z)| *z).sum::<usize>())
    });
    for img in ranked.iter().take(8) {
        println!("  base 0x{:x} {:?}", img.base, img.name);
        for (seg, addr, size) in &img.data_segments {
            println!("      {seg:16} 0x{addr:x} size 0x{size:x}");
        }
    }
    if !images.iter().any(|i| i.name.contains("GameAssembly")) {
        println!("  !! no image named GameAssembly among {} images", images.len());
    }
    drop(probe_mem);

    // --- attach -------------------------------------------------------------
    let rt = match Il2Cpp::attach(pid) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "GameAssembly __DATA = 0x{:x} (size 0x{:x})",
        rt.data_segment.0, rt.data_segment.1
    );
    println!(
        "type-info table = 0x{:x} at __DATA+0x{:x}, {} entries",
        rt.type_info_table, rt.table_ptr_offset, rt.type_count
    );
    println!("attach took {:?}", t0.elapsed());

    let l: ClassLayout = rt.layout;
    println!(
        "\nIl2CppClass layout: name=0x{:x} ns=0x{:x} elem_class=0x{:x} parent=0x{:x} \
         klass_self=0x{:x} fields=0x{:x} static_fields=0x{:x} inst_size=0x{:x} elem_size=0x{:x}",
        l.name, l.namespace, l.element_class, l.parent, l.klass_self, l.fields,
        l.static_fields, l.instance_size, l.element_size
    );

    // --- root singleton -----------------------------------------------------
    println!("\n=== root singleton ===");
    let mut root = 0usize;
    let mut root_via = String::new();

    for (cls_name, field) in [
        ("WrapperController", "<Instance>k__BackingField"),
        ("PAPA", "_instance"),
    ] {
        let t = Instant::now();
        let class = match rt.find_class(cls_name) {
            Some(c) => c,
            None => {
                println!("{cls_name}: class NOT FOUND");
                continue;
            }
        };
        println!(
            "{cls_name}: class 0x{:x} (ns='{}', parent='{}') [lookup {:?}]",
            class,
            rt.class_namespace(class),
            rt.class_name(rt.class_parent(class)),
            t.elapsed()
        );

        let statics = rt.class_static_storage(class);
        println!("  static storage = 0x{statics:x}");
        for f in rt.class_fields(class).iter().filter(|f| f.is_static) {
            println!(
                "    static field {:40} off {:6} code 0x{:02x}{}",
                f.name,
                f.offset,
                f.type_code,
                if f.is_thread_static { " (thread-static)" } else { "" }
            );
        }

        match rt.static_field_addr(class, field) {
            Some((addr, _)) => {
                let inst = rt.mem.read_ptr(addr);
                let inst_class = rt.class_of(inst);
                println!(
                    "  {field} @0x{addr:x} -> instance 0x{inst:x} (class '{}')",
                    rt.class_name(inst_class)
                );
                if plausible(inst) && inst_class == class && root == 0 {
                    root = inst;
                    root_via = format!("{cls_name}.{field}");
                }
            }
            None => println!("  {field}: not found / no static storage"),
        }
    }

    if root == 0 {
        eprintln!("\nno live root singleton — is the game on the home screen (not in a match)?");
        std::process::exit(2);
    }
    println!("\nUSING ROOT: 0x{root:x} via {root_via}");

    // --- helper -------------------------------------------------------------
    let dump = |label: &str, obj: usize| {
        let class = rt.class_of(obj);
        println!(
            "\n--- {label} @0x{obj:x} class '{}' (ns '{}', size {}) ---",
            rt.class_name(class),
            rt.class_namespace(class),
            rt.class_instance_size(class)
        );
        let mut cur = class;
        let mut depth = 0;
        while cur != 0 && depth < 6 {
            let fields = rt.class_fields(cur);
            if !fields.is_empty() {
                println!("  [{}]", rt.class_name(cur));
                for f in fields.iter() {
                    if f.is_static || f.is_thread_static {
                        continue;
                    }
                    let addr = obj + f.offset as usize;
                    let raw = rt.mem.read_ptr(addr);
                    let hint = if f.type_code == 0x0e {
                        rt.mem
                            .read_managed_string(raw)
                            .map(|s| format!("\"{}\"", s.chars().take(48).collect::<String>()))
                            .unwrap_or_else(|| "null".into())
                    } else if plausible(raw) && rt.class_of(raw) != 0 {
                        format!("-> {}", rt.class_name(rt.class_of(raw)))
                    } else {
                        format!("{}", rt.mem.read_i32(addr))
                    };
                    println!(
                        "    {:44} off {:5} code 0x{:02x}  {hint}",
                        f.name, f.offset, f.type_code
                    );
                }
            }
            cur = rt.class_parent(cur);
            depth += 1;
        }
    };

    // Step-by-step dictionary diagnosis: which stage of the generic
    // Entry[] resolution fails.
    let diag_dict = |label: &str, dict: usize| {
        println!("\n[dict] {label} @0x{dict:x} class '{}'", rt.class_name(rt.class_of(dict)));
        let entries = match rt.ref_field(dict, "_entries") {
            Some(e) => e,
            None => {
                println!("  _entries: FIELD NOT RESOLVED");
                return;
            }
        };
        println!("  _entries ptr = 0x{entries:x}");
        println!("  _count = {:?}", rt.i32_field(dict, "_count"));

        let arr_class = rt.class_of(entries);
        println!(
            "  array class = 0x{arr_class:x} '{}'  (is_class={})",
            rt.class_name(arr_class),
            arr_class != 0
        );
        if arr_class == 0 {
            println!("  raw first ptr at entries = 0x{:x}", rt.mem.read_ptr(entries));
            return;
        }
        let ec = rt.class_element_class(arr_class);
        println!(
            "  element_class = 0x{ec:x} '{}' element_size = {} array_len(@0x18) = {}",
            rt.class_name(ec),
            rt.class_element_size(arr_class),
            rt.mem.read_i32(entries + 0x18)
        );
        // Where does element_size actually live? Dump the size block of the
        // array class alongside the element class's own instance size.
        print!("  array class size block:");
        for off in [0xF0usize, 0xF4, 0xF8, 0xFC, 0x100, 0x104, 0x108] {
            print!(" 0x{off:x}={}", rt.mem.meta_u32(arr_class + off));
        }
        println!();
        println!(
            "  element class instance_size(0xF8) = {}  (unboxed {})",
            rt.class_instance_size(ec),
            rt.class_instance_size(ec).saturating_sub(0x10)
        );

        let ecf = rt.class_fields(ec);
        println!("  element class declares {} fields:", ecf.len());
        for f in ecf.iter() {
            println!(
                "    {:20} off {:4} code 0x{:02x} static={}",
                f.name, f.offset, f.type_code, f.is_static
            );
        }
        if ecf.is_empty() {
            println!(
                "  !! no fields; fields ptr = 0x{:x}, parent of that = 0x{:x}",
                rt.mem.read_ptr(ec + 0x80),
                rt.mem.read_ptr(rt.mem.read_ptr(ec + 0x80) + 0x10)
            );
            println!("  element class parent = '{}'", rt.class_name(rt.class_parent(ec)));
        }
        let n = rt.dict_entries(dict, 20).len();
        println!("  dict_entries(first 20) -> {n}");
    };

    // How does Il2CppType.data identify a class on this build?
    println!("\n=== Il2CppType.data resolution ===");
    if let Some(wc) = rt.find_class("WrapperController") {
        let im = rt.find_class("InventoryManager").unwrap_or(0);
        println!(
            "  InventoryManager class = 0x{im:x}  typeDefinition(class+0x68) = 0x{:x}",
            rt.mem.meta_ptr(im + 0x68)
        );
        for f in rt.class_fields(wc).iter().take(6) {
            let data = rt.mem.meta_ptr(f.type_ptr);
            println!(
                "  {:46} type_ptr=0x{:x} data=0x{:x} is_class={} < type_count={}",
                f.name,
                f.type_ptr,
                data,
                rt.is_class(data),
                data < rt.type_count
            );
        }
    }

    dump("root", root);

    // --- account ------------------------------------------------------------
    println!("\n=== account ===");
    match rt
        .ref_field(root, "<AccountClient>k__BackingField")
        .and_then(|ac| rt.ref_field(ac, "<AccountInformation>k__BackingField"))
    {
        Some(ai) => {
            dump("AccountInformation", ai);
            for f in [
                "DisplayName", "AccountID", "PersonaID", "GameID", "Email", "ExternalID",
                "CountryCode",
            ] {
                println!("  {f} = {:?}", rt.string_field(ai, f));
            }
            println!(
                "  AccessToken = {:?}",
                rt.string_field(ai, "AccessToken").map(|s| format!("<{} chars>", s.len()))
            );

            // A reference-element array: element_size here MUST be 8, which
            // pins down which offset really holds element_size.
            if let Some(roles) = rt.ref_field(ai, "Roles") {
                let ac = rt.class_of(roles);
                print!(
                    "  Roles String[] class 0x{ac:x} '{}' len {} size block:",
                    rt.class_name(ac),
                    rt.mem.read_i32(roles + 0x18)
                );
                for off in [0xF0usize, 0xF4, 0xF8, 0xFC, 0x100, 0x104, 0x108] {
                    print!(" 0x{off:x}={}", rt.mem.meta_u32(ac + off));
                }
                println!();
            }
        }
        None => println!("  could not reach AccountClient.AccountInformation"),
    }

    // --- inventory / collection --------------------------------------------
    println!("\n=== inventory manager ===");
    let inv_mgr = rt.ref_field(root, "<InventoryManager>k__BackingField");
    println!("  InventoryManager = {inv_mgr:x?}");

    let wrapper = inv_mgr.and_then(|im| {
        rt.ref_field(im, "InventoryServiceWrapper")
            .or_else(|| rt.ref_field(im, "_inventoryServiceWrapper"))
    });
    match wrapper {
        Some(w) => {
            dump("InventoryServiceWrapper", w);

            if let Some(inv) = rt.ref_field(w, "m_inventory") {
                dump("ClientPlayerInventory", inv);
                for f in [
                    "gems", "gold", "wcCommon", "wcUncommon", "wcRare", "wcMythic",
                    "wcTrackPosition",
                ] {
                    println!("  {f} = {:?}", rt.number_field(inv, f));
                }
                println!("  vaultProgress = {:?}", rt.number_field(inv, "vaultProgress"));
                println!("  basicLandSet = {:?}", rt.string_field(inv, "basicLandSet"));
                println!(
                    "  latestBasicLandSet = {:?}",
                    rt.string_field(inv, "latestBasicLandSet")
                );
            } else {
                println!("  m_inventory NOT FOUND");
            }

            if let Some(cards) = rt.ref_field(w, "<Cards>k__BackingField") {
                let cls = rt.class_of(cards);
                println!(
                    "\n  Cards @0x{cards:x} class '{}' ns '{}'",
                    rt.class_name(cls),
                    rt.class_namespace(cls)
                );
                diag_dict("Cards", cards);

                let t = Instant::now();
                let entries = rt.dict_entries(cards, 200_000);
                println!("  dict_entries -> {} live entries in {:?}", entries.len(), t.elapsed());
                let mut total = 0i64;
                for (ka, kc, va, vc) in entries.iter().take(5) {
                    println!(
                        "    key(code 0x{kc:02x}) = {}  value(code 0x{vc:02x}) = {}",
                        rt.read_number(*ka, *kc),
                        rt.read_number(*va, *vc)
                    );
                }
                for (_, _, va, vc) in entries.iter() {
                    total += rt.read_number(*va, *vc).as_i64().unwrap_or(0);
                }
                println!("    unique = {}, total copies = {}", entries.len(), total);
            } else {
                println!("  <Cards>k__BackingField NOT FOUND");
            }
        }
        None => println!("  InventoryServiceWrapper NOT FOUND on InventoryManager"),
    }

    // --- ranks --------------------------------------------------------------
    println!("\n=== ranks ===");
    match rt
        .ref_field(root, "<PlayerRankServiceWrapper>k__BackingField")
        .and_then(|w| rt.ref_field(w, "_combinedRankInfo"))
    {
        Some(cri) => {
            dump("CombinedRankInfo", cri);
            for p in ["constructed", "limited"] {
                println!(
                    "  {p}: class={:?} level={:?} step={:?} won={:?} lost={:?} pct={:?}",
                    rt.number_field(cri, &format!("{p}Class")),
                    rt.number_field(cri, &format!("{p}Level")),
                    rt.number_field(cri, &format!("{p}Step")),
                    rt.number_field(cri, &format!("{p}MatchesWon")),
                    rt.number_field(cri, &format!("{p}MatchesLost")),
                    rt.string_field(cri, &format!("{p}Percentile")),
                );
            }
        }
        None => println!("  could not reach PlayerRankServiceWrapper._combinedRankInfo"),
    }

    // --- decks --------------------------------------------------------------
    println!("\n=== decks ===");
    match rt
        .ref_field(root, "DecksManager")
        .or_else(|| rt.ref_field(root, "<DecksManager>k__BackingField"))
        .and_then(|dm| rt.ref_field(dm, "_deckDataProvider"))
        .and_then(|dp| rt.ref_field(dp, "_allDecks"))
    {
        Some(all) => {
            let cls = rt.class_of(all);
            println!("  _allDecks @0x{all:x} class '{}'", rt.class_name(cls));
            diag_dict("_allDecks", all);

            if let Some(entries) = rt.ref_field(all, "_entries") {
                if let Some((ec, es, len, _data)) = rt.array_info(entries) {
                    println!(
                        "  entries array: elem_class '{}' stride {} capacity {}",
                        rt.class_name(ec),
                        es,
                        len
                    );
                    println!("  Entry fields (offsets include the 0x10 header):");
                    for f in rt.class_fields(ec).iter() {
                        println!("    {:20} off {:4} code 0x{:02x}", f.name, f.offset, f.type_code);
                    }
                }
            }

            let items = rt.dict_entries(all, 10_000);
            println!("  {} live deck entries", items.len());

            for (i, (ka, kc, va, _vc)) in items.iter().enumerate().take(3) {
                let deck = rt.mem.read_ptr(*va);
                println!(
                    "\n  deck[{i}] key(code 0x{kc:02x})={} ptr=0x{deck:x} class '{}'",
                    rt.read_number(*ka, *kc),
                    rt.class_name(rt.class_of(deck))
                );
                if !plausible(deck) {
                    continue;
                }
                if let Some(sum) = rt.ref_field(deck, "_summary") {
                    dump("Client_DeckSummary", sum);
                    println!("    Name = {:?}", rt.string_field(sum, "Name"));
                    if let Some((a, _)) = rt.field_addr(sum, "DeckId") {
                        println!("    DeckId = {}", rt.read_guid(a));
                    }
                    if let Some(attrs) = rt.ref_field(sum, "Attributes") {
                        let pairs = rt.dict_entries(attrs, 100);
                        for (ka, _, va, _) in pairs {
                            println!(
                                "    attr {:?} = {:?}",
                                rt.mem.read_managed_string(rt.mem.read_ptr(ka)),
                                rt.mem.read_managed_string(rt.mem.read_ptr(va))
                            );
                        }
                    }
                }
                if let Some(contents) = rt.ref_field(deck, "_contents") {
                    dump("Client_DeckContents", contents);
                    if let Some(piles) = rt.ref_field(contents, "Piles") {
                        for (ka, kc, va, _) in rt.dict_entries(piles, 32) {
                            let list = rt.mem.read_ptr(va);
                            let info = rt.list_info(list);
                            println!(
                                "    pile {} -> list 0x{list:x} info {:?} (elem class '{}')",
                                rt.read_number(ka, kc),
                                info.map(|(d, es, n, _)| (format!("0x{d:x}"), es, n)),
                                info.map(|(_, _, _, ec)| rt.class_name(ec)).unwrap_or_default()
                            );
                            if let Some((data, es, n, ec)) = info {
                                println!("      element fields:");
                                for f in rt.class_fields(ec).iter() {
                                    println!(
                                        "        {:16} off {:4} code 0x{:02x}",
                                        f.name, f.offset, f.type_code
                                    );
                                }
                                for k in 0..n.min(4) {
                                    let base = data + k as usize * es;
                                    println!(
                                        "      [{k}] raw {:?}",
                                        rt.mem.read_bytes(base, es.min(16))
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        None => println!("  could not reach DecksManager._deckDataProvider._allDecks"),
    }

    println!("\ntotal {:?}", t0.elapsed());
}
