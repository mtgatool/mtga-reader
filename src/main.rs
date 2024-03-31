use mtga_reader::{FieldDefinition, MonoReader, TypeDefinition};
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

            if typedef.name == "WrapperController" {
                println!(
                    "namespace_name: {}, {}",
                    typedef.namespace_name, typedef.name
                );
                println!("type: {}", typedef.type_info.clone().code());
                println!("field count: {}", typedef.field_count);
                let fields = typedef.get_fields();
                for field in fields {
                    let field_def = FieldDefinition::new(field, &mono_reader);
                    println!("Field name: {}", field_def.name);
                    println!("Field type: {}", field_def.type_info.code());
                }
            }

            type_defs.push((offset, typedef));
        }
    });
}
