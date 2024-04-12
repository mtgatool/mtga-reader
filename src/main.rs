use mtga_reader::{FieldDefinition, Managed, MonoReader, TypeCode, TypeDefinition, TypeInfo};
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
            "<Cards>k__BackingField",
        ];

        // get the type defs on the root of the assembly for the first loop
        let definition = get_def_by_name(&defs, find[0], &mono_reader)
            .unwrap()
            .clone();

        // skipt the first item in the find array
        let find = &find[1..];

        let mut field = (definition.clone(), TypeInfo::new(definition, &mono_reader));

        for (index, name) in find.iter().enumerate() {
            let code = field.1.clone();
            println!("Find: {}: {}", name, code.code());

            field = match index {
                0 => {
                    let class = TypeDefinition::new(definition, &mono_reader);
                    class.get_static_value(name)
                }
                _ => {
                    let managed = Managed::new(&mono_reader, field.0);
                    let class = managed.read_class();
                    let ptr = mono_reader.read_ptr(field.0);
                    class.get_value(name, ptr)
                }
            };
        }

        let code = field.1.clone().code();
        println!("  {}", code);

        let code = field.1.clone().code();

        let ptr = mono_reader.read_ptr(field.0);
        let managed = Managed::new(&mono_reader, field.0);
        let class = match code {
            TypeCode::CLASS => managed.read_class(),
            TypeCode::GENERICINST => managed.read_generic_instance(field.1.clone()),
            _ => managed.read_class(),
        };

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
                TypeCode::U => {
                    print!(" = {}", managed.read_u4());
                }
                TypeCode::R4 => {
                    print!(" = {}", managed.read_r4());
                }
                TypeCode::R8 => {
                    print!(" = {}", managed.read_r8());
                }
                TypeCode::I4 => {
                    print!(" = {}", managed.read_i4());
                }
                TypeCode::I => {
                    print!(" = {}", managed.read_i4());
                }
                TypeCode::I2 => {
                    print!(" = {}", managed.read_i2());
                }
                TypeCode::U2 => {
                    print!(" = {}", managed.read_u2());
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
