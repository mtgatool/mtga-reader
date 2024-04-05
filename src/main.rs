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
        ];
        // let find = [
        //     "PAPA",
        //     "_instance",
        //     "_eventManager",
        //     "_eventsServiceWrapper",
        //     "_cachedEvents",
        //     "_items",
        // ];

        // get the type defs on the root of the assembly for the first loop
        let definition = get_def_by_name(&defs, find[0], &mono_reader)
            .unwrap()
            .clone();
        let main_typedef = TypeDefinition::new(definition, &mono_reader);

        // Not really needed now, but this is how you can check if a class has static fields
        // Usually we would navigate these fields and then down to the next class or static field
        // let mut has_statics = false;
        // let fields = main_typedef.get_fields();
        // for field in fields {
        //     let field_def = FieldDefinition::new(field, &mono_reader);
        //     if field_def.type_info.is_static {
        //         has_statics = true;
        //     }
        // }
        // if has_statics {
        //     println!("{}", main_typedef.name);
        // }

        if main_typedef.name == find[0] {
            println!("{} definition addr: {}", find[0], definition);
            let mut definition_address = 0;

            for find in find.iter().skip(1) {
                println!("-----------------");
                println!("-----------------");
                let definition_addr_clone = definition_address.clone();
                println!("definition_address: {}", definition_addr_clone);

                let managed = Managed::new(&mono_reader, definition_addr_clone);

                let managed_def: TypeDefinition = match definition_addr_clone {
                    0 => TypeDefinition::new(definition.clone(), &mono_reader),
                    _ => managed.read_class(),
                };

                println!(
                    "In > {} - {} - {:?}",
                    managed_def.name,
                    managed_def.type_info.clone().code(),
                    managed_def.class_kind
                );
                println!("find: {}", find);

                let fields = managed_def.get_fields();
                for field in fields {
                    let field_def = FieldDefinition::new(field, &mono_reader);
                    if !field_def.type_info.is_const && field_def.type_info.is_static {
                        println!("    {} - {}", field_def.name, field);
                        let code = field_def.type_info.clone().code();
                        match code {
                            TypeCode::BOOLEAN => {
                                let managed =
                                    Managed::new(&mono_reader, field + field_def.offset as usize);
                                println!("    {}", managed.read_boolean());
                            }
                            TypeCode::U4 => {
                                let managed =
                                    Managed::new(&mono_reader, field + field_def.offset as usize);
                                println!("    {}", managed.read_u4());
                            }
                            TypeCode::R4 => {
                                let managed =
                                    Managed::new(&mono_reader, field + field_def.offset as usize);
                                println!("    {}", managed.read_r4());
                            }
                            TypeCode::VALUETYPE => {
                                let managed =
                                    Managed::new(&mono_reader, field + field_def.offset as usize);
                                println!("    {}", managed.read_valuetype());
                            }
                            _ => {
                                //
                            }
                        }
                    }
                    if &field_def.name == find {
                        println!("FOUND at: {}", field);
                        definition_address = field + field_def.offset as usize;
                        // break;
                    }
                }
            }
        }
    });
}
