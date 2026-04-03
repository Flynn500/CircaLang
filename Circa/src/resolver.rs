use std::collections::HashSet;
use std::path::Path;
use std::fs;

use crate::ast::{Program, Stmt};
use crate::parser;

include!(concat!(env!("OUT_DIR"), "/stdlib.rs"));

/// Result of resolving all imports for a program.
pub struct Resolved {
    /// Flattened program with Import statements replaced by their contents.
    pub program: Program,
    /// All module names that were imported (for builtin registration).
    pub imported_modules: HashSet<String>,
}

/// Look up a stdlib module source by name.
fn find_stdlib(name: &str) -> Option<&'static str> {
    STDLIB_MODULES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, src)| *src)
}

/// Resolve all imports in a parsed program. Entry point for main.
pub fn resolve(program: Program, base_dir: &Path) -> Result<Resolved, String> {
    let mut imported = HashSet::new();
    resolve_inner(program, base_dir, &mut imported)
}

/// Recursive inner resolver.
fn resolve_inner(
    program: Program,
    base_dir: &Path,
    imported: &mut HashSet<String>,
) -> Result<Resolved, String> {
    let mut out = Vec::new();
    let mut modules = HashSet::new();

    for stmt in program {
        match stmt {
            Stmt::Import { ref name } => {
                // Skip if already imported
                if imported.contains(name) {
                    continue;
                }
                imported.insert(name.clone());
                modules.insert(name.clone());

                // Resolve source: stdlib first, then local file
                let (src, child_dir) = if let Some(src) = find_stdlib(name) {
                    (src.to_string(), base_dir.to_path_buf())
                } else {
                    let path = base_dir.join(format!("{}.ca", name));
                    let content = fs::read_to_string(&path).map_err(|e| {
                        format!("import {}: cannot read {}: {}", name, path.display(), e)
                    })?;
                    (content, base_dir.to_path_buf())
                };

                // Parse the imported source
                let tokens = parser::lex(&src)
                    .map_err(|errs| format!("import {}: lex error: {:?}", name, errs))?;
                let child_program = parser::parse(tokens)
                    .map_err(|errs| format!("import {}: parse error: {:?}", name, errs))?;

                // Recursively resolve imports within the imported module
                let child = resolve_inner(child_program, &child_dir, imported)?;
                modules.extend(child.imported_modules);
                out.extend(child.program);
            }
            other => out.push(other),
        }
    }

    Ok(Resolved {
        program: out,
        imported_modules: modules,
    })
}