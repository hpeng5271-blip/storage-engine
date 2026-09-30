use std::collections::BTreeMap;

pub const TOMBSTONE: &str = "__TOMBSTONE__";

pub struct MemTable {
    data: BTreeMap<String, String>,
}

impl MemTable {
    pub fn new() -> Self {
        MemTable { data: BTreeMap::new() }
    }

    pub fn put(&mut self, key: String, value: String) {
        self.data.insert(key, value);
    }

    pub fn delete(&mut self, key: String) {
        self.data.insert(key, TOMBSTONE.to_string());
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn data(&self) -> &BTreeMap<String, String> {
        &self.data
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// 范围扫描 [start, end]
    pub fn scan(&self, start: &str, end: &str) -> Vec<(String, String)> {
        self.data
            .range(start.to_string()..=end.to_string())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}