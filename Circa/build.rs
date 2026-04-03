use std::{env, fs, path::Path};

fn main() {
    let std_dir = Path::new("std");
    let mut modules = Vec::new();

    if std_dir.exists() {
        let mut entries: Vec<_> = fs::read_dir(std_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "ca").unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let name = entry.path().file_stem().unwrap().to_string_lossy().to_string();
            let content = fs::read_to_string(entry.path()).unwrap();
            modules.push((name, content));
        }
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("stdlib.rs");

    let mut code = String::from("pub const STDLIB_MODULES: &[(&str, &str)] = &[\n");
    for (name, src) in &modules {
        code.push_str(&format!("    ({:?}, {:?}),\n", name, src));
    }
    code.push_str("];\n");

    fs::write(dest, code).unwrap();

    println!("cargo:rerun-if-changed=std");
}