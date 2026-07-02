//! High-level structured queries read from MTGA memory (decks, ranks).
//!
//! These walk the live object graph and return ready-to-use JSON, so callers
//! (the NAPI `.node` binary and the debug HTTP server) don't have to chain
//! low-level `read_data` paths. Windows/Mono only for now.

use serde_json::{json, Value};

use crate::field_definition::FieldDefinition;
use crate::mono_reader::MonoReader;
use crate::type_definition::TypeDefinition;

fn dvalid(p: usize) -> bool {
    p > 0x10000 && p < 0x7FFF_FFFF_FFFF
}

fn class_of(r: &MonoReader, obj: usize) -> usize {
    let vt = r.read_ptr(obj);
    if dvalid(vt) { r.read_ptr(vt) } else { 0 }
}

fn class_name(r: &MonoReader, obj: usize) -> String {
    let c = class_of(r, obj);
    if dvalid(c) {
        let td = TypeDefinition::new(c, r);
        if td.name.len() <= 200 { td.name } else { String::new() }
    } else {
        String::new()
    }
}

/// (field_storage_addr, type_code) for a field on `obj` by name (walks inheritance).
fn field_addr(r: &MonoReader, obj: usize, name: &str) -> Option<(usize, u32)> {
    let cls = class_of(r, obj);
    if !dvalid(cls) {
        return None;
    }
    let td = TypeDefinition::new(cls, r);
    let (fa, _ti) = td.get_field(name);
    if fa == 0 {
        return None;
    }
    let fd = FieldDefinition::new(fa, r);
    Some((obj + fd.offset as usize, fd.type_info.type_code))
}

fn ref_field(r: &MonoReader, obj: usize, name: &str) -> Option<usize> {
    let (addr, _c) = field_addr(r, obj, name)?;
    let child = r.read_ptr(addr);
    if dvalid(child) { Some(child) } else { None }
}

fn string_field(r: &MonoReader, obj: usize, name: &str) -> Option<String> {
    let (addr, _c) = field_addr(r, obj, name)?;
    r.read_mono_string(r.read_ptr(addr))
}

fn u32_field(r: &MonoReader, obj: usize, name: &str) -> Option<u32> {
    field_addr(r, obj, name).map(|(addr, _)| r.read_u32(addr))
}

fn i32_field(r: &MonoReader, obj: usize, name: &str) -> Option<i32> {
    field_addr(r, obj, name).map(|(addr, _)| r.read_i32(addr))
}

/// Read a System.Guid value type (16 bytes) and format it canonically.
fn read_guid(r: &MonoReader, addr: usize) -> String {
    let b = r.read_bytes(addr, 16);
    if b.len() < 16 {
        return String::new();
    }
    let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let d2 = u16::from_le_bytes([b[4], b[5]]);
    let d3 = u16::from_le_bytes([b[6], b[7]]);
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        d1, d2, d3, b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Read a Dictionary<string,string> into (key,value) pairs.
fn read_string_dict(r: &MonoReader, dict: usize) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let entries = r.read_ptr(dict + 0x18);
    if !dvalid(entries) {
        return out;
    }
    let cap = r.read_i32(entries + 0x18);
    if cap <= 0 || cap > 10_000 {
        return out;
    }
    let data = entries + 0x20;
    for i in 0..cap {
        let e = data + i as usize * 24;
        if r.read_i32(e) < 0 {
            continue;
        }
        if let Some(k) = r.read_mono_string(r.read_ptr(e + 0x08)) {
            let v = r.read_mono_string(r.read_ptr(e + 0x10)).unwrap_or_default();
            out.push((k, v));
        }
    }
    out
}

/// Read a pile's List<{uint grpId, int qty}> value-type array.
fn read_pile_list(r: &MonoReader, list_ptr: usize) -> Vec<(u32, i32)> {
    let mut out = Vec::new();
    let items = r.read_ptr(list_ptr + 0x10);
    let size = r.read_i32(list_ptr + 0x18);
    if !dvalid(items) || size <= 0 || size > 5000 {
        return out;
    }
    let data = items + 0x20;
    for i in 0..size {
        let base = data + i as usize * 8;
        let grp = r.read_u32(base);
        let qty = r.read_i32(base + 4);
        if grp > 0 && qty > 0 && qty < 1000 {
            out.push((grp, qty));
        }
    }
    out
}

/// Read Piles = Dictionary<EDeckPile, List<..>> into (pileKey, cards).
fn read_piles(r: &MonoReader, dict: usize) -> Vec<(i32, Vec<(u32, i32)>)> {
    let mut out = Vec::new();
    let entries = r.read_ptr(dict + 0x18);
    if !dvalid(entries) {
        return out;
    }
    let cap = r.read_i32(entries + 0x18);
    if cap <= 0 || cap > 100_000 {
        return out;
    }
    let data = entries + 0x20;
    for i in 0..cap {
        let e = data + i as usize * 24;
        if r.read_i32(e) < 0 {
            continue;
        }
        let key = r.read_i32(e + 0x08);
        let val = r.read_ptr(e + 0x10);
        if !dvalid(val) {
            continue;
        }
        let cards = read_pile_list(r, val);
        if !cards.is_empty() {
            out.push((key, cards));
        }
    }
    out
}

/// EDeckPile (Wizards.Mtga.Decks) value -> name.
fn pile_name(key: i32) -> &'static str {
    match key {
        0 => "Invalid",
        1 => "Main",
        2 => "Sideboard",
        3 => "CommandZone",
        4 => "Companions",
        _ => "Unknown",
    }
}

/// RankingClassType (Wizards.Mtga.FrontDoorModels) value -> name.
fn rank_class_name(v: u32) -> &'static str {
    match v {
        0 => "None",
        1 => "Spark",
        2 => "Bronze",
        3 => "Silver",
        4 => "Gold",
        5 => "Platinum",
        6 => "Diamond",
        7 => "Master",
        8 => "Mythic",
        _ => "Unknown",
    }
}

/// Locate WrapperController.Instance (home screen; null during a match).
fn wrapper_instance(reader: &mut MonoReader) -> Option<usize> {
    let mut wc = None;
    let img = reader.read_assembly_image();
    if img != 0 {
        for d in reader.create_type_definitions_for_image(img) {
            if TypeDefinition::new(d, reader).name == "WrapperController" {
                wc = Some(d);
                break;
            }
        }
    }
    if wc.is_none() {
        for asm in reader.get_all_assembly_names() {
            if asm == "Assembly-CSharp" {
                continue;
            }
            let img = reader.read_assembly_image_by_name(&asm);
            if img == 0 {
                continue;
            }
            if let Some(d) = reader
                .create_type_definitions_for_image(img)
                .into_iter()
                .find(|d| TypeDefinition::new(*d, reader).name == "WrapperController")
            {
                wc = Some(d);
                break;
            }
        }
    }
    let wc = wc?;
    let wc_td = TypeDefinition::new(wc, reader);
    let (inst_addr, _ti) = wc_td.get_static_value("<Instance>k__BackingField");
    let inst = reader.read_ptr(inst_addr);
    if dvalid(inst) { Some(inst) } else { None }
}

/// Read all saved decks (name, deckId, format/attributes, per-pile card lists).
pub fn read_decks(process_name: String) -> Value {
    let mut reader = match crate::get_reader(process_name) {
        Some(r) => r,
        None => return json!({ "error": "could not open MTGA process (run elevated)" }),
    };
    let instance = match wrapper_instance(&mut reader) {
        Some(i) => i,
        None => return json!({ "error": "WrapperController.Instance is null (must be on the home screen, not in a match)" }),
    };
    let reader: &MonoReader = &reader;

    let all_decks = ref_field(reader, instance, "DecksManager")
        .and_then(|dm| ref_field(reader, dm, "_deckDataProvider"))
        .and_then(|dp| ref_field(reader, dp, "_allDecks"));
    let all_decks = match all_decks {
        Some(a) => a,
        None => return json!({ "error": "could not reach DecksManager._deckDataProvider._allDecks" }),
    };

    let entries = reader.read_ptr(all_decks + 0x18);
    if !dvalid(entries) {
        return json!({ "error": "_allDecks._entries not found" });
    }
    let cap = reader.read_i32(entries + 0x18);
    if cap <= 0 || cap > 100_000 {
        return json!({ "error": format!("implausible entries length {}", cap) });
    }
    let data = entries + 0x20;

    let mut decks: Vec<Value> = Vec::new();
    for i in 0..cap {
        let e = data + i as usize * 32;
        if reader.read_i32(e) < 0 {
            continue;
        }
        let deck_ptr = reader.read_ptr(e + 0x18);
        if !dvalid(deck_ptr) {
            continue;
        }
        let summary = match ref_field(reader, deck_ptr, "_summary") {
            Some(s) => s,
            None => continue, // not a deck-like object
        };

        let name = string_field(reader, summary, "Name").unwrap_or_default();
        let deck_id = field_addr(reader, summary, "DeckId").map(|(a, _)| read_guid(reader, a));
        let tile_id = u32_field(reader, summary, "DeckTileId");
        let description = string_field(reader, summary, "Description").filter(|s| !s.is_empty());
        let attributes: serde_json::Map<String, Value> = ref_field(reader, summary, "Attributes")
            .map(|d| {
                read_string_dict(reader, d)
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect()
            })
            .unwrap_or_default();

        let piles = ref_field(reader, deck_ptr, "_contents").and_then(|c| ref_field(reader, c, "Piles"));
        let pile_data = piles.map(|p| read_piles(reader, p)).unwrap_or_default();
        let piles_json: Vec<Value> = pile_data
            .iter()
            .map(|(key, cards)| {
                json!({
                    "pile": key,
                    "pileName": pile_name(*key),
                    "total": cards.iter().map(|(_, q)| *q).sum::<i32>(),
                    "cards": cards.iter().map(|(g, q)| json!({ "grpId": g, "qty": q })).collect::<Vec<_>>(),
                })
            })
            .collect();

        decks.push(json!({
            "name": name,
            "deckId": deck_id,
            "description": description,
            "tileId": tile_id,
            "attributes": attributes,
            "piles": piles_json,
        }));
    }

    json!({ "count": decks.len(), "decks": decks })
}

/// Read the player's constructed + limited rank info.
pub fn read_ranks(process_name: String) -> Value {
    let mut reader = match crate::get_reader(process_name) {
        Some(r) => r,
        None => return json!({ "error": "could not open MTGA process (run elevated)" }),
    };
    let instance = match wrapper_instance(&mut reader) {
        Some(i) => i,
        None => return json!({ "error": "WrapperController.Instance is null (must be on the home screen, not in a match)" }),
    };
    let reader: &MonoReader = &reader;

    let cri = ref_field(reader, instance, "<PlayerRankServiceWrapper>k__BackingField")
        .and_then(|w| ref_field(reader, w, "_combinedRankInfo"));
    let cri = match cri {
        Some(c) => c,
        None => return json!({ "error": "could not reach PlayerRankServiceWrapper._combinedRankInfo" }),
    };

    let one = |prefix: &str| -> Value {
        let class_v = u32_field(reader, cri, &format!("{}Class", prefix)).unwrap_or(0);
        json!({
            "seasonOrdinal": i32_field(reader, cri, &format!("{}SeasonOrdinal", prefix)),
            "class": rank_class_name(class_v),
            "classValue": class_v,
            "level": i32_field(reader, cri, &format!("{}Level", prefix)),
            "step": i32_field(reader, cri, &format!("{}Step", prefix)),
            "wins": i32_field(reader, cri, &format!("{}MatchesWon", prefix)),
            "losses": i32_field(reader, cri, &format!("{}MatchesLost", prefix)),
            "draws": i32_field(reader, cri, &format!("{}MatchesDrawn", prefix)),
            "percentile": string_field(reader, cri, &format!("{}Percentile", prefix)),
            "leaderboardPlace": i32_field(reader, cri, &format!("{}LeaderboardPlace", prefix)),
        })
    };

    json!({
        "playerId": string_field(reader, cri, "playerId"),
        "constructed": one("constructed"),
        "limited": one("limited"),
    })
}
