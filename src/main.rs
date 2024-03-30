use mtga_reader::{MonoReader, TypeDefinition};
use sysinfo::{Pid as SysPid, System};

fn find_pid_by_name(name: &str) -> Option<SysPid> {
    let mut sys = System::new_all();
    sys.refresh_all();

    sys.processes()
        .iter()
        .find(|(_, process)| process.name().contains(name))
        .map(|(pid, _)| *pid)
}

fn main() {
    println!("Reading started...");

    let pid = find_pid_by_name("MTGA");

    if pid.is_none() {
        println!("MTGA not found");
        return;
    }

    pid.iter().for_each(|pid| {
        let mut mono_reader = MonoReader::new(pid.as_u32());

        mono_reader.read_mono_root_domain();
        mono_reader.read_assembly_image();

        let mut type_defs: Vec<(usize, TypeDefinition)> = Vec::new();
        let defs = mono_reader.create_type_definitions();

        let vec_size = defs.len();
        for i in 0..vec_size {
            let offset = i * 8;
            let definition = defs.get(i).unwrap();
            let typedef = TypeDefinition::new(definition.clone(), &mono_reader);

            if typedef.name == "PAPA" {
                println!("PAPA type: {}", typedef.type_info.type_code);
                let fields = typedef.get_fields();
                for field in fields {
                    println!("Field: {}", field);
                }
            }

            type_defs.push((offset, typedef));
        }
    });
}
