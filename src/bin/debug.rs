// test main fn

use mtga_reader::read_data;

fn main() {
    let path = vec![
        "PAPA".to_string(),
        "_instance".to_string(),
        "_matchManager".to_string(),
        "<OpponentInfo>k__BackingField".to_string(),
    ];

    let data = read_data("MTGA".to_string(), path);
    println!("{:?}", data);
}
