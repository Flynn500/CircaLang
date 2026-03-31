use crate::value::Value;

#[derive(Debug, Clone)]
pub struct Env {
    entries: Vec<(String, Value)>,
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

    pub fn define(&mut self, name: String, val: Value) {
        self.entries.push((name, val));
    }

    pub fn assign(&mut self, name: &str, val: Value) -> bool {
        for entry in self.entries.iter_mut().rev() {
            if entry.0 == name {
                entry.1 = val;
                return true;
            }
        }
        false
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        for entry in self.entries.iter().rev() {
            if entry.0 == name {
                return Some(&entry.1);
            }
        }
        None
    }
}