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
            "<AccountClient>k__BackingField",
            "<InventoryManager>k__BackingField",
        ];
        // let find = ["PAPA","_instance","_eventManager","_eventsServiceWrapper","_cachedEvents","_items"];

        let vec_size = defs.len();
        for i in 0..vec_size {
            // get the type defs on the root of the assembly for the first loop
            let definition = defs.get(i).unwrap();
            let main_typedef = TypeDefinition::new(definition.clone(), &mono_reader);

            if main_typedef.name == find[0] {
                let mut definition_address = 0;

                for find in find.iter().skip(1) {
                    println!("-----------------");
                    println!("-----------------");
                    let definition_addr_clone = definition_address.clone();
                    println!("definition_address: {}", definition_addr_clone);

                    let managed = Managed::new(&mono_reader, definition_addr_clone);

                    let managed_def: TypeDefinition = match definition_addr_clone {
                        0 => TypeDefinition::new(definition.clone(), &mono_reader),
                        _ => managed.read_class().unwrap(),
                    };

                    println!(
                        "In > {} - {}",
                        managed_def.name,
                        managed_def.type_info.clone().code()
                    );
                    println!("find: {}", find);

                    let fields = managed_def.get_fields();
                    for field in fields {
                        let field_def = FieldDefinition::new(field, &mono_reader);
                        println!(
                            "   - {} - {}",
                            field_def.name,
                            field_def.type_info.clone().code()
                        );
                        if &field_def.name == find {
                            println!("FOUND at: {}", field_def.type_info.addr);
                            definition_address = field_def.type_info.addr;
                            break;
                        }
                    }
                }
            }

            // if typedef.name == find[0] {
            //     println!(
            //         "namespace_name: {}, {}",
            //         typedef.namespace_name, typedef.name
            //     );
            //     println!("type: {}", typedef.type_info.clone().code());
            //     println!("field count: {}", typedef.field_count);
            //     let fields = typedef.get_fields();

            //     for field in fields {
            //         let field_def = FieldDefinition::new(field, &mono_reader);
            //         // println!("Field name: {}", field_def.name);
            //         let code = field_def.type_info.clone().code();
            //         // println!("Field type: {}", code);

            //         match code {
            //             TypeCode::CLASS => {
            //                 if field_def.name == "_inventoryManager" {
            //                     println!("_inventoryManager found");
            //                     println!("field_def.type_info.addr: {}", field_def.type_info.addr);
            //                     println!("field_def.offset: {}", field_def.offset);

            //                     // if this.readXXXX(type, genericTypeArguments, address) the base offset is type_info.addr
            //                     let managed = Managed::new(&mono_reader, field_def.type_info.addr);
            //                     let inventory_manager = managed.read_class();
            //                     inventory_manager.iter().for_each(|td| {
            //                         let fields = td.get_fields();
            //                         println!("field count: {}", td.field_count);
            //                         for field in fields {
            //                             let field_def = FieldDefinition::new(field, &mono_reader);
            //                             println!("Field name: {}", field_def.name);
            //                             let code = field_def.type_info.clone().code();
            //                             println!("Field type: {}", code);
            //                         }
            //                     });
            //                 }
            //             }
            //             _ => {}
            //         }

            // match code {
            //     TypeCode::BOOLEAN => {
            //         let managed =
            //             Managed::new(&mono_reader, field + field_def.offset as usize);
            //         println!("Field value: {}", managed.read_boolean());
            //     }
            //     TypeCode::U4 => {
            //         let managed =
            //             Managed::new(&mono_reader, field + field_def.offset as usize);
            //         println!("Field value: {}", managed.read_u4());
            //     }
            //     TypeCode::R4 => {
            //         let managed =
            //             Managed::new(&mono_reader, field + field_def.offset as usize);
            //         println!("Field value: {}", managed.read_r4());
            //     }
            //     TypeCode::VALUETYPE => {
            //         let managed =
            //             Managed::new(&mono_reader, field + field_def.offset as usize);
            //         println!("Field value: {}", managed.read_valuetype());
            //     }
            //     _ => {}
            // }
            //     }
            // }

            // type_defs.push((offset, typedef));
        }
    });
}
