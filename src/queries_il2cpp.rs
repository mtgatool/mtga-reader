//! High-level structured queries for the macOS / IL2CPP backend.
//!
//! This is the IL2CPP counterpart of `queries.rs` (Windows/Mono) and returns
//! the **same JSON shapes**, so consumers don't branch on platform.
//!
//! Unlike the Mono side, nothing here hardcodes a struct offset: field
//! addresses, dictionary entry strides and list element layouts all come from
//! IL2CPP metadata via `Il2Cpp`. Verified against a live MTGA build on
//! macOS/arm64 (see `src/bin/probe_il2cpp.rs`).

#![cfg(target_os = "macos")]

use serde_json::{json, Value};

use crate::il2cpp::macos_runtime::{plausible, type_enum, Il2Cpp};

/// The root singleton. `WrapperController.Instance` is the same root the
/// Windows backend uses; `PAPA._instance` is the historical IL2CPP fallback.
const ROOTS: [(&str, &str); 2] = [
    ("WrapperController", "<Instance>k__BackingField"),
    ("PAPA", "_instance"),
];

/// Locate the live root object. Null during a match (the wrapper scene is
/// unloaded), which is why callers surface a "home screen" error.
pub fn find_root(rt: &Il2Cpp) -> Option<usize> {
    for (class_name, field) in ROOTS {
        let class = match rt.find_class(class_name) {
            Some(c) => c,
            None => continue,
        };
        if let Some((addr, _)) = rt.static_field_addr(class, field) {
            let inst = rt.mem.read_ptr(addr);
            // The static may be stale after a scene unload; require that it
            // still points at an instance of the expected class.
            if plausible(inst) && rt.class_of(inst) == class {
                return Some(inst);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Generic object serialisation (used by readData / readClass)
// ---------------------------------------------------------------------------

/// Max elements pulled out of an array when serialising generically.
const MAX_ARRAY_ELEMS: usize = 1024;

/// The `Il2CppClass` a field's type points at (for CLASS / VALUETYPE fields).
fn field_class(rt: &Il2Cpp, type_ptr: usize) -> usize {
    rt.type_class(type_ptr)
}

/// .NET enums have exactly one instance field named `value__`.
fn enum_backing(rt: &Il2Cpp, class: usize) -> Option<u8> {
    if class == 0 {
        return None;
    }
    rt.class_fields(class)
        .iter()
        .find(|f| f.name == "value__")
        .map(|f| f.type_code)
}

/// Serialise the value stored at `addr` according to its metadata type code.
pub fn value_json(rt: &Il2Cpp, addr: usize, code: u8, type_ptr: usize, depth: u32) -> Value {
    use type_enum::*;

    match code {
        STRING => rt
            .mem
            .read_managed_string(rt.mem.read_ptr(addr))
            .map(Value::String)
            .unwrap_or(Value::Null),

        CLASS | OBJECT | GENERICINST => {
            let ptr = rt.mem.read_ptr(addr);
            if !plausible(ptr) {
                return Value::Null;
            }
            if depth == 0 {
                return json!({ "address": ptr, "class": rt.class_name(rt.class_of(ptr)) });
            }
            // A generic instance may be a collection; strings arrive here too
            // when boxed as object.
            object_json(rt, ptr, depth - 1)
        }

        SZARRAY | ARRAY => {
            let ptr = rt.mem.read_ptr(addr);
            if !plausible(ptr) {
                return Value::Null;
            }
            array_json(rt, ptr, depth)
        }

        VALUETYPE => {
            let fc = field_class(rt, type_ptr);
            // Enum: emit the underlying number, like the Mono backend does.
            if let Some(backing) = enum_backing(rt, fc) {
                return rt.read_number(addr, backing);
            }
            if depth == 0 || fc == 0 {
                return rt.read_number(addr, I4);
            }
            // Inline struct: value-type field offsets include the object
            // header, so it is subtracted for the unboxed storage.
            struct_json(rt, addr, fc, depth - 1)
        }

        _ => rt.read_number(addr, code),
    }
}

/// Serialise an unboxed value type stored at `addr`.
fn struct_json(rt: &Il2Cpp, addr: usize, class: usize, depth: u32) -> Value {
    let mut map = serde_json::Map::new();
    for f in rt.class_fields(class).iter() {
        if f.is_static || f.is_thread_static {
            continue;
        }
        let fa = addr + (f.offset as usize).saturating_sub(0x10);
        map.insert(f.name.clone(), value_json(rt, fa, f.type_code, f.type_ptr, depth));
    }
    Value::Object(map)
}

fn array_json(rt: &Il2Cpp, array: usize, depth: u32) -> Value {
    let (elem_class, elem_size, len, data) = match rt.array_info(array) {
        Some(a) => a,
        None => return Value::Null,
    };
    let n = (len.max(0) as usize).min(MAX_ARRAY_ELEMS);
    let mut out = Vec::with_capacity(n);

    // Element type code: read it from the element class's own byval_arg.
    let code = ((rt.mem.meta_u32(elem_class + 0x28) >> 16) & 0xFF) as u8;

    for i in 0..n {
        let addr = data + i * elem_size;
        out.push(value_json(rt, addr, code, elem_class + 0x20, depth));
    }
    Value::Array(out)
}

/// Serialise a managed object's instance fields, walking its inheritance chain
/// (a derived field shadows a base field of the same name).
pub fn object_json(rt: &Il2Cpp, obj: usize, depth: u32) -> Value {
    let class = rt.class_of(obj);
    if class == 0 {
        return Value::Null;
    }

    // Strings and arrays reaching here should serialise as themselves.
    let class_name = rt.class_name(class);
    if class_name == "String" {
        return rt
            .mem
            .read_managed_string(obj)
            .map(Value::String)
            .unwrap_or(Value::Null);
    }
    if class_name.ends_with("[]") {
        return array_json(rt, obj, depth);
    }

    let mut map = serde_json::Map::new();
    let mut cur = class;
    for _ in 0..16 {
        if cur == 0 {
            break;
        }
        for f in rt.class_fields(cur).iter() {
            if f.is_static || f.is_thread_static || map.contains_key(&f.name) {
                continue;
            }
            let addr = obj + f.offset as usize;
            map.insert(
                f.name.clone(),
                value_json(rt, addr, f.type_code, f.type_ptr, depth),
            );
        }
        cur = rt.class_parent(cur);
    }

    Value::Object(map)
}

/// `readData` path walk: `[RootClass, StaticField, field, field, ...]`.
///
/// Matches the Mono backend's semantics — the first hop is a static field on the
/// named class, the rest are instance fields resolved by name.
pub fn read_data_path(rt: &Il2Cpp, fields: &[String]) -> Value {
    if fields.len() < 2 {
        return json!({ "error": "path needs at least a class and a static field" });
    }

    let class = match rt.find_class(&fields[0]) {
        Some(c) => c,
        None => return json!({ "error": format!("class '{}' not found", fields[0]) }),
    };

    let (static_addr, static_f) = match rt.static_field_addr(class, &fields[1]) {
        Some(s) => s,
        None => {
            return json!({
                "error": format!("static field '{}' not found on '{}'", fields[1], fields[0])
            })
        }
    };

    if fields.len() == 2 {
        return value_json(rt, static_addr, static_f.type_code, static_f.type_ptr, 3);
    }

    let mut cur = rt.mem.read_ptr(static_addr);
    if !plausible(cur) {
        return json!({
            "error": format!("{}.{} is null (scene not loaded?)", fields[0], fields[1])
        });
    }

    let rest = &fields[2..];
    for (i, name) in rest.iter().enumerate() {
        let (addr, f) = match rt.field_addr(cur, name) {
            Some(x) => x,
            None => return json!({ "error": format!("field '{name}' not found") }),
        };

        if i + 1 == rest.len() {
            return value_json(rt, addr, f.type_code, f.type_ptr, 3);
        }

        cur = rt.mem.read_ptr(addr);
        if !plausible(cur) {
            return json!({ "error": format!("field '{name}' is null") });
        }
    }

    object_json(rt, cur, 3)
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
fn rank_class_name(v: i64) -> &'static str {
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

/// Read a field that may be either a string or a number on a given build
/// (`Percentile` is a float here but a string on other builds).
fn text_or_number(rt: &Il2Cpp, obj: usize, name: &str) -> Option<Value> {
    let (addr, f) = rt.field_addr(obj, name)?;
    if f.type_code == type_enum::STRING {
        return rt
            .mem
            .read_managed_string(rt.mem.read_ptr(addr))
            .map(Value::String);
    }
    Some(rt.read_number(addr, f.type_code))
}

// ---------------------------------------------------------------------------
// Readers
// ---------------------------------------------------------------------------

/// Read the player's account identity (from AccountInformation).
pub fn account_from(rt: &Il2Cpp, root: usize) -> Value {
    let ai = rt
        .ref_field(root, "<AccountClient>k__BackingField")
        .and_then(|ac| rt.ref_field(ac, "<AccountInformation>k__BackingField"));
    let ai = match ai {
        Some(a) => a,
        None => return json!({ "error": "could not reach AccountClient.AccountInformation" }),
    };

    json!({
        "displayName": rt.string_field(ai, "DisplayName"),
        "accountId": rt.string_field(ai, "AccountID"),
        "personaId": rt.string_field(ai, "PersonaID"),
        "gameId": rt.string_field(ai, "GameID"),
        "email": rt.string_field(ai, "Email"),
        "externalId": rt.string_field(ai, "ExternalID"),
        "countryCode": rt.string_field(ai, "CountryCode"),
        "accessToken": rt.string_field(ai, "AccessToken"),
    })
}

/// Walk to the inventory service wrapper, which lives on a base class of
/// InventoryManager (concrete type `AwsInventoryServiceWrapper`).
fn inventory_wrapper(rt: &Il2Cpp, root: usize) -> Option<usize> {
    let im = rt.ref_field(root, "<InventoryManager>k__BackingField")?;
    rt.ref_field(im, "InventoryServiceWrapper")
        // Older builds named the field with a leading underscore.
        .or_else(|| rt.ref_field(im, "_inventoryServiceWrapper"))
}

/// Read the player's wallet/inventory (gems, gold, wildcards, vault, ...).
pub fn inventory_from(rt: &Il2Cpp, root: usize) -> Value {
    let inv = inventory_wrapper(rt, root).and_then(|w| rt.ref_field(w, "m_inventory"));
    let inv = match inv {
        Some(i) => i,
        None => return json!({ "error": "could not reach InventoryServiceWrapper.m_inventory" }),
    };

    json!({
        "gems": rt.number_field(inv, "gems"),
        "gold": rt.number_field(inv, "gold"),
        "wildcards": {
            "common": rt.number_field(inv, "wcCommon"),
            "uncommon": rt.number_field(inv, "wcUncommon"),
            "rare": rt.number_field(inv, "wcRare"),
            "mythic": rt.number_field(inv, "wcMythic"),
        },
        "wcTrackPosition": rt.number_field(inv, "wcTrackPosition"),
        "vaultProgress": rt.number_field(inv, "vaultProgress"),
        "basicLandSet": rt.string_field(inv, "basicLandSet"),
        "latestBasicLandSet": rt.string_field(inv, "latestBasicLandSet"),
    })
}

/// Read the player's owned-card collection (grpId -> quantity).
pub fn collection_from(rt: &Il2Cpp, root: usize) -> Value {
    let cards = inventory_wrapper(rt, root).and_then(|w| rt.ref_field(w, "<Cards>k__BackingField"));
    let cards = match cards {
        Some(c) => c,
        None => return json!({ "error": "could not reach InventoryServiceWrapper.Cards" }),
    };

    // Decode straight out of the bulk-read entry blob: ~10k cards would
    // otherwise cost two syscalls each.
    let (blob, stride, _data, layout) = match rt.dict_pairs_bulk(cards, 1_000_000) {
        Some(b) => b,
        None => return json!({ "error": "could not read the Cards dictionary entries" }),
    };

    let mut arr: Vec<Value> = Vec::new();
    let mut i = 0usize;
    while (i + 1) * stride <= blob.len() {
        let base = i * stride;
        i += 1;

        let hash = i32::from_le_bytes(
            blob[base + layout.hash..base + layout.hash + 4]
                .try_into()
                .unwrap_or([0; 4]),
        );
        if hash < 0 {
            continue; // free slot
        }

        let grp = Il2Cpp::decode_number(&blob[base + layout.key..], layout.key_code)
            .as_u64()
            .unwrap_or(0);
        let qty = Il2Cpp::decode_number(&blob[base + layout.value..], layout.value_code)
            .as_i64()
            .unwrap_or(0);

        if grp > 0 {
            arr.push(json!({ "grpId": grp, "qty": qty }));
        }
    }

    json!({ "count": arr.len(), "cards": arr })
}

/// Read the player's constructed + limited rank info.
pub fn ranks_from(rt: &Il2Cpp, root: usize) -> Value {
    let cri = rt
        .ref_field(root, "<PlayerRankServiceWrapper>k__BackingField")
        .and_then(|w| rt.ref_field(w, "_combinedRankInfo"));
    let cri = match cri {
        Some(c) => c,
        None => {
            return json!({ "error": "could not reach PlayerRankServiceWrapper._combinedRankInfo" })
        }
    };

    let one = |prefix: &str| -> Value {
        let class_v = rt
            .number_field(cri, &format!("{prefix}Class"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        json!({
            "seasonOrdinal": rt.number_field(cri, &format!("{prefix}SeasonOrdinal")),
            "class": rank_class_name(class_v),
            "classValue": class_v,
            "level": rt.number_field(cri, &format!("{prefix}Level")),
            "step": rt.number_field(cri, &format!("{prefix}Step")),
            "wins": rt.number_field(cri, &format!("{prefix}MatchesWon")),
            "losses": rt.number_field(cri, &format!("{prefix}MatchesLost")),
            "draws": rt.number_field(cri, &format!("{prefix}MatchesDrawn")),
            "percentile": text_or_number(rt, cri, &format!("{prefix}Percentile")),
            "leaderboardPlace": rt.number_field(cri, &format!("{prefix}LeaderboardPlace")),
        })
    };

    json!({
        "playerId": rt.string_field(cri, "playerId"),
        "constructed": one("constructed"),
        "limited": one("limited"),
    })
}

/// Read a pile's `List<Client_DeckCard>` of `{ Id, Quantity }` value types.
fn read_pile_list(rt: &Il2Cpp, list_ptr: usize) -> Vec<(u64, i64)> {
    let mut out = Vec::new();
    let (data, elem_size, size, elem_class) = match rt.list_info(list_ptr) {
        Some(i) => i,
        None => return out,
    };
    if size <= 0 || size > 100_000 || elem_size == 0 {
        return out;
    }

    // Element field offsets come from metadata; value-type offsets include the
    // object header, so it is subtracted for unboxed array elements.
    let fields = rt.class_fields(elem_class);
    let field = |names: [&str; 2]| {
        fields
            .iter()
            .find(|f| names.contains(&f.name.as_str()))
            .map(|f| ((f.offset as usize).saturating_sub(0x10), f.type_code))
    };
    let (id_off, id_code) = field(["Id", "grpId"]).unwrap_or((0, type_enum::U4));
    let (qty_off, qty_code) = field(["Quantity", "qty"]).unwrap_or((4, type_enum::I4));

    let blob = rt.mem.read_bytes(data, size as usize * elem_size);
    if blob.len() < size as usize * elem_size {
        return out;
    }

    for i in 0..size as usize {
        let base = i * elem_size;
        let grp = Il2Cpp::decode_number(&blob[base + id_off..], id_code)
            .as_u64()
            .unwrap_or(0);
        let qty = Il2Cpp::decode_number(&blob[base + qty_off..], qty_code)
            .as_i64()
            .unwrap_or(0);
        if grp > 0 && qty > 0 {
            out.push((grp, qty));
        }
    }
    out
}

/// Read `Piles = Dictionary<EDeckPile, List<Client_DeckCard>>`.
fn read_piles(rt: &Il2Cpp, dict: usize) -> Vec<Value> {
    let mut out = Vec::new();
    for (key_addr, key_code, val_addr, _) in rt.dict_entries(dict, 64) {
        let key = rt.read_number(key_addr, key_code).as_i64().unwrap_or(-1) as i32;
        let list = rt.mem.read_ptr(val_addr);
        if !plausible(list) {
            continue;
        }
        let cards = read_pile_list(rt, list);
        if cards.is_empty() {
            continue;
        }
        out.push(json!({
            "pile": key,
            "pileName": pile_name(key),
            "total": cards.iter().map(|(_, q)| *q).sum::<i64>(),
            "cards": cards.iter().map(|(g, q)| json!({ "grpId": g, "qty": q })).collect::<Vec<_>>(),
        }));
    }
    out
}

/// Read all saved decks (name, deckId, format/attributes, per-pile card lists).
pub fn decks_from(rt: &Il2Cpp, root: usize) -> Value {
    let all_decks = rt
        .ref_field(root, "DecksManager")
        .or_else(|| rt.ref_field(root, "<DecksManager>k__BackingField"))
        .and_then(|dm| rt.ref_field(dm, "_deckDataProvider"))
        .and_then(|dp| rt.ref_field(dp, "_allDecks"));
    let all_decks = match all_decks {
        Some(a) => a,
        None => {
            return json!({ "error": "could not reach DecksManager._deckDataProvider._allDecks" })
        }
    };

    let mut decks: Vec<Value> = Vec::new();

    for (_key_addr, _key_code, val_addr, _) in rt.dict_entries(all_decks, 100_000) {
        let deck_ptr = rt.mem.read_ptr(val_addr);
        if !plausible(deck_ptr) {
            continue;
        }
        let summary = match rt.ref_field(deck_ptr, "_summary") {
            Some(s) => s,
            None => continue, // not a deck-like object
        };

        let name = rt.string_field(summary, "Name").unwrap_or_default();
        let deck_id = rt
            .field_addr(summary, "DeckId")
            .map(|(a, _)| rt.read_guid(a));
        let tile_id = rt.number_field(summary, "DeckTileId");
        let description = rt
            .string_field(summary, "Description")
            .filter(|s| !s.is_empty());

        let attributes: serde_json::Map<String, Value> = rt
            .ref_field(summary, "Attributes")
            .map(|d| {
                rt.dict_entries(d, 128)
                    .into_iter()
                    .filter_map(|(ka, _, va, _)| {
                        let k = rt.mem.read_managed_string(rt.mem.read_ptr(ka))?;
                        let v = rt
                            .mem
                            .read_managed_string(rt.mem.read_ptr(va))
                            .unwrap_or_default();
                        Some((k, Value::String(v)))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let piles = rt
            .ref_field(deck_ptr, "_contents")
            .and_then(|c| rt.ref_field(c, "Piles"))
            .map(|p| read_piles(rt, p))
            .unwrap_or_default();

        decks.push(json!({
            "name": name,
            "deckId": deck_id,
            "description": description,
            "tileId": tile_id,
            "attributes": attributes,
            "piles": piles,
        }));
    }

    json!({ "count": decks.len(), "decks": decks })
}
