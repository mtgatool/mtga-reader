use mtga_reader::{FieldDefinition, Managed, MonoReader, TypeCode, TypeDefinition};
use sysinfo::{Pid as SysPid, System};

fn find_pid_by_name(name: &str) -> Option<SysPid> {
    let mut sys = System::new_all();
    sys.refresh_all();

    sys.processes()
        .iter()
        .find(|(_, process)| process.name().contains(name))
        .map(|(pid, _)| *pid)
}

fn get_def_by_name<'a>(
    defs: &'a Vec<usize>,
    name: &str,
    mono_reader: &MonoReader,
) -> Option<&'a usize> {
    defs.iter().find(|def| {
        let main_typedef = TypeDefinition::new(**def, &mono_reader);
        main_typedef.name == name
    })
}

fn get_def_by_addr<'a>(
    defs: &'a Vec<usize>,
    addr: usize,
    mono_reader: &MonoReader,
) -> Option<&'a usize> {
    defs.iter().find(|def| {
        let main_typedef = TypeDefinition::new(**def, &mono_reader);
        main_typedef.type_info.addr == addr
    })
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
        let defs = mono_reader.create_type_definitions();

        let find = [
            "WrapperController",
            "<Instance>k__BackingField",
            "<InventoryManager>k__BackingField",
            "_inventoryServiceWrapper",
            "m_inventory",
        ];

        // get the type defs on the root of the assembly for the first loop
        let definition = get_def_by_name(&defs, find[0], &mono_reader)
            .unwrap()
            .clone();

        let td = TypeDefinition::new(definition, &mono_reader);
        let static_field_addr = td.get_static_value(find[1]);

        println!("static_field_addr: {}", static_field_addr);

        let managed = Managed::new(&mono_reader, static_field_addr);
        let class = managed.read_class();
        let field = class.get_static_value(find[1]);

        println!("Found 1 {} {}", find[1], field);

        // read a class (ManagedClassInstance, non static field)
        let managed = Managed::new(&mono_reader, field);
        let class = managed.read_class();
        let ptr = mono_reader.read_ptr(field);
        let field = class.get_value(find[2], ptr);

        println!("Found 2 {} {}", find[2], field);

        let managed = Managed::new(&mono_reader, field);
        let class = managed.read_class();
        let ptr = mono_reader.read_ptr(field);
        let field = class.get_value(find[3], ptr);

        println!("Found 3 {} {} {}", find[3], ptr, field);

        let managed = Managed::new(&mono_reader, field);
        let class = managed.read_class();
        let ptr = mono_reader.read_ptr(field);
        let field = class.get_value(find[4], ptr);

        println!("Found 4 {} {} {}", find[4], ptr, field);

        let managed = Managed::new(&mono_reader, field);
        let class = managed.read_class();
        let ptr = mono_reader.read_ptr(field);

        for field in class.get_fields() {
            let field_def = FieldDefinition::new(field, &mono_reader);

            let code = field_def.type_info.clone().code();
            print!("   {}, {} ", field_def.name, code);
            let managed = Managed::new(&mono_reader, ptr + field_def.offset as usize);
            match code {
                TypeCode::BOOLEAN => {
                    print!(" = {}", managed.read_boolean());
                }
                TypeCode::U4 => {
                    print!(" = {}", managed.read_u4());
                }
                TypeCode::R4 => {
                    print!(" = {}", managed.read_r4());
                }
                TypeCode::I4 => {
                    print!(" = {}", managed.read_i4());
                }
                TypeCode::STRING => {
                    print!(" = {}", managed.read_string());
                }
                TypeCode::VALUETYPE => {
                    print!(" = {}", managed.read_valuetype());
                }
                _ => {
                    //
                }
            }
            println!("")
        }
    });
}
