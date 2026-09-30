use std::collections::BTreeMap;
use std::fs;
use crate::memtable::{MemTable, TOMBSTONE};
use crate::sstable::SSTable;
use crate::wal::{Wal, OP_PUT, OP_DELETE};

/// 层数：L0 + L1 + L2 + L3
const NUM_LEVELS: usize = 4;

pub struct Storage {
    memtable: MemTable,
    wal: Wal,
    /// levels[0] = L0，levels[1] = L1，以此类推
    /// L0 允许 key 重叠，L1+ 不重叠
    levels: Vec<Vec<SSTable>>,
    /// 每层触发向下合并的 SSTable 数量阈值
    level_thresholds: Vec<usize>,
    flush_threshold: usize,
    sstable_counter: u32,
    data_dir: String,
}

impl Storage {
    pub fn open(data_dir: &str, flush_threshold: usize) -> std::io::Result<Self> {
        fs::create_dir_all(data_dir)?;

        // WAL
        let wal_path = format!("{}/wal.log", data_dir);
        let mut wal = Wal::open(&wal_path)?;

        // WAL 恢复
        let mut memtable = MemTable::new();
        for (op, k, v) in wal.replay()? {
            if op == OP_PUT {
                memtable.put(k, v);
            } else if op == OP_DELETE {
                memtable.delete(k);
            }
        }

        // 扫描已有 SSTable 文件，按层级归位
        let mut levels: Vec<Vec<SSTable>> = (0..NUM_LEVELS).map(|_| Vec::new()).collect();
        let mut counter: u32 = 0;

        for entry in fs::read_dir(data_dir)? {
            let entry = entry?;
            let name = entry.file_name().into_string().unwrap();

            if !name.ends_with(".sst") { continue; }

            // 文件名格式：L{level}_{counter}.sst
            let level = match name.strip_prefix('L')
                .and_then(|s| s.split_once('_'))
                .and_then(|(lvl, _)| lvl.parse::<usize>().ok())
            {
                Some(l) if l < NUM_LEVELS => l,
                _ => continue,
            };

            let path = format!("{}/{}", data_dir, name);
            if let Ok(sst) = SSTable::open(&path, 4096) {
                levels[level].push(sst);
            }

            // 从文件名提取 counter
            if let Some(rest) = name.strip_prefix(&format!("L{}_", level)) {
                if let Some(num_str) = rest.strip_suffix(".sst") {
                    if let Ok(n) = num_str.parse::<u32>() {
                        if n + 1 > counter { counter = n + 1; }
                    }
                }
            }
        }

        // 每层按文件名排序（旧 → 新）
        for level in levels.iter_mut() {
            level.sort_by(|a, b| a.path.cmp(&b.path));
        }

        Ok(Storage {
            memtable,
            wal,
            levels,
            // L0 累积 3 个就向下合并；L1/L2 累积 2 个；L3 是最后一层不合并
            level_thresholds: vec![3, 2, 2, usize::MAX],
            flush_threshold,
            sstable_counter: counter,
            data_dir: data_dir.to_string(),
        })
    }

    pub fn put(&mut self, key: &str, value: &str) -> std::io::Result<()> {
        self.wal.append_put(key, value)?;
        self.memtable.put(key.to_string(), value.to_string());

        if self.memtable.len() >= self.flush_threshold {
            self.flush()?;
        }
        Ok(())
    }

    pub fn delete(&mut self, key: &str) -> std::io::Result<()> {
        self.wal.append_delete(key)?;
        self.memtable.delete(key.to_string());

        if self.memtable.len() >= self.flush_threshold {
            self.flush()?;
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> std::io::Result<Option<String>> {
        // 1. MemTable（最新）
        if let Some(v) = self.memtable.get(key) {
            if v == TOMBSTONE { return Ok(None); }
            return Ok(Some(v.clone()));
        }

        // 2. 从 L0 到 L3，每层从新到旧
        for level in self.levels.iter() {
            for sst in level.iter().rev() {
                if let Some(v) = sst.get(key)? {
                    if v == TOMBSTONE { return Ok(None); }
                    return Ok(Some(v));
                }
            }
        }

        Ok(None)
    }

    pub fn scan(&self, start: &str, end: &str) -> std::io::Result<Vec<(String, String)>> {
        let mut merged: BTreeMap<String, String> = BTreeMap::new();

        // 从最深层到 L0（旧 → 新，新覆盖旧）
        for level in self.levels.iter().rev() {
            for sst in level.iter() {
                for (k, v) in sst.scan(start, end)? {
                    merged.insert(k, v);
                }
            }
        }
        // 最后是 MemTable
        for (k, v) in self.memtable.scan(start, end) {
            merged.insert(k, v);
        }

        merged.retain(|_, v| v != TOMBSTONE);
        Ok(merged.into_iter().collect())
    }

    /// MemTable 刷盘 → L0，然后检查是否要往下合并
    pub fn flush(&mut self) -> std::io::Result<()> {
        if self.memtable.is_empty() { return Ok(()); }

        let path = format!("{}/L0_{:06}.sst", self.data_dir, self.sstable_counter);
        let sst = SSTable::write(&path, self.memtable.data(), 4096)?;
        println!("[flush] L0 ← {} 条记录 ({})", sst.len(), path);

        self.levels[0].push(sst);
        self.sstable_counter += 1;
        self.memtable.clear();
        self.wal.clear()?;

        // 从 L0 开始检查是否触发合并
        self.maybe_compact(0)?;

        Ok(())
    }

    /// 递归：Ln 满了就合并到 Ln+1，然后检查 Ln+1
    fn maybe_compact(&mut self, level: usize) -> std::io::Result<()> {
        // 最后一层不合并
        if level + 1 >= self.levels.len() {
            return Ok(());
        }
        // 本层未满
        if self.levels[level].len() < self.level_thresholds[level] {
            return Ok(());
        }

        let to_merge = std::mem::take(&mut self.levels[level]);
        let count = to_merge.len();

        let new_path = format!("{}/L{}_{:06}.sst",
                               self.data_dir, level + 1, self.sstable_counter);
        self.sstable_counter += 1;

        let new_sst = crate::compaction::compact_all(to_merge, &new_path)?;
        println!("[compact] L{} ({}) → L{} ({} 条)",
                 level, count, level + 1, new_sst.len());

        self.levels[level + 1].push(new_sst);

        // 递归检查下一层
        self.maybe_compact(level + 1)?;

        Ok(())
    }

    pub fn memtable_len(&self) -> usize {
        self.memtable.len()
    }

    pub fn level_counts(&self) -> Vec<usize> {
        self.levels.iter().map(|l| l.len()).collect()
    }

    pub fn first_sstable_path(&self) -> Option<String> {
        // 优先返回最深层（最大的文件）
        for level in self.levels.iter().rev() {
            if let Some(sst) = level.first() {
                return Some(sst.path.clone());
            }
        }
        None
    }
}