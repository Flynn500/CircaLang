use crate::ast::TypeAnno;
use crate::value::{value_matches_type, Value};

#[derive(Debug, Clone)]
pub struct Binding {
    pub value: Value,
    pub declared_type: TypeAnno,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct Env {
    entries: Vec<(String, Binding)>,
    scope_starts: Vec<usize>,
}

impl Env {
    pub fn new() -> Self {
        Env {
            entries: Vec::new(),
            scope_starts: vec![0],
        }
    }

    pub fn push_scope(&mut self) {
        self.scope_starts.push(self.entries.len());
    }

    pub fn pop_scope(&mut self) {
        let start = self.scope_starts.pop().expect("cannot pop global scope");
        self.entries.truncate(start);
    }

    pub fn define(&mut self, name: String, binding: Binding) {
        self.entries.push((name, binding));
    }

    pub fn define_value(&mut self, name: String, value: Value, declared_type: TypeAnno, mutable: bool) {
        self.define(
            name,
            Binding {
                value,
                declared_type,
                mutable,
            },
        );
    }

    pub fn assign(&mut self, name: &str, val: Value) -> Result<(), String> {
        for entry in self.entries.iter_mut().rev() {
            if entry.0 == name {
                if !entry.1.mutable {
                    return Err(format!("cannot assign to const '{}'", name));
                }

                if !value_matches_type(&val, &entry.1.declared_type) {
                    return Err(format!(
                        "type mismatch: '{}' is declared as {:?}, cannot assign value of type {:?}",
                        name,
                        entry.1.declared_type,
                        val.runtime_type(),
                    ));
                }

                entry.1.value = val;
                return Ok(());
            }
        }
        Err(format!("undefined variable: {}", name))
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        for entry in self.entries.iter().rev() {
            if entry.0 == name {
                return Some(&entry.1.value);
            }
        }
        None
    }

    pub fn get_binding(&self, name: &str) -> Option<&Binding> {
        for entry in self.entries.iter().rev() {
            if entry.0 == name {
                return Some(&entry.1);
            }
        }
        None
    }
}
