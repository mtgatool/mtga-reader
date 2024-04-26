// Should enable [lib] crate-type = ["lib"] in Cargo.toml to use this .rs
// im pretty sure there is a way to make this work trough parameters but i dont know how
use mono_reader::MonoReader;

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
