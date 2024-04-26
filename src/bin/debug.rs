use mtga_reader::read_data;

pub fn main() {
    let path = vec![
        "PAPA".to_string(),
        "_instance".to_string(),
        "_formatManager".to_string(),
        "_formats".to_string(),
        "_items".to_string(),
    ];

    let data = read_data("MTGA".to_string(), path);
    println!("{}", data.to_string());
    assert_eq!(data.is_object(), true);
}
