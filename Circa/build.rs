use std::{env, fs, path::Path};

fn main() {
    let std_dir = Path::new("std");
    let mut combined = String::new();

    if std_dir.exists() {
        let mut entries: Vec<_> = fs::read_dir(std_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "ca").unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let content = fs::read_to_string(entry.path()).unwrap();
            combined.push_str(&content);
            combined.push('\n');
        }
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("stdlib.rs");
    fs::write(dest, format!("pub const STDLIB_SRC: &str = {:?};", combined)).unwrap();

    println!("cargo:rerun-if-changed=std");
}
