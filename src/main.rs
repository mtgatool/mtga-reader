use mtga_reader::read_data;

fn main() {
    // let find = [
    //     "WrapperController",
    //     "<Instance>k__BackingField",
    //     "<InventoryManager>k__BackingField",
    //     "_inventoryServiceWrapper",
    //     "<Cards>k__BackingField",
    //     "_entries",
    // ];
    
    let find = vec![
        "PAPA",
        "_instance",
        "_inventoryManager",
        "_inventoryServiceWrapper",
        "m_inventory",
    ];

    let data = read_data(
        "MTGA".to_string(),
        find,
    );

    println!("{}", data);
}
