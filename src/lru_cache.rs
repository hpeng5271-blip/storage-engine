use std::collections::{HashMap, VecDeque};

/// 简单的 LRU 缓存
pub struct LruCache<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    order: VecDeque<K>,  // 队首最旧，队尾最新
    pub evictions: usize,  // 被淘汰的次数
}

impl<K: Clone + Eq + std::hash::Hash, V: Clone> LruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        LruCache {
            capacity,
            map: HashMap::new(),
            order: VecDeque::new(),
            evictions: 0,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<V> {
        if !self.map.contains_key(key) {
            return None;
        }
        // 移到队尾（标记为最近使用）
        self.order.retain(|k| k != key);
        self.order.push_back(key.clone());
        self.map.get(key).cloned()
    }

    pub fn put(&mut self, key: K, value: V) {
        // 已存在 → 更新并移到队尾
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            self.order.retain(|k| k != &key);
            self.order.push_back(key);
            return;
        }

        // 缓存满 → 淘汰队首（最旧）
        if self.map.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
                self.evictions += 1;
            }
        }

        self.map.insert(key.clone(), value);
        self.order.push_back(key);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}