use std::collections::BTreeMap;
use std::fs;
use crate::memtable::{MemTable, TOMBSTONE};
use crate::sstable::SSTable;
use crate::wal::{Wal, OP_PUT, OP_DELETE};
use crate::manifest::Manifest;

const NUM_LEVELS: usize = 4;

pub struct Storage {
    memtable: MemTable,
    wal: Wal,
    levels: Vec<Vec<SSTable>>,
    level_thresholds: Vec<usize>,
    flush_threshold: usize,
    manifest: Manifest,
    data_dir: String,
}

impl Storage {
    pub fn open(data_dir: &str, flush_threshold: usize) -> std::io::Result<Self> {
        fs::create_dir_all(data_dir)?;

        // --- WAL 恢复 ---
        let wal_path = format!("{}/wal.log", data_dir);
        let mut wal = Wal::open(&wal_path)?;

        let mut memtable = MemTable::new();
        for (op, k, v) in wal.replay()? {
            if op == OP_PUT {
                memtable.put(k, v);
            } else if op == OP_DELETE {
                memtable.delete(k);
            }
        }

        // --- 从 Manifest 加载 ---
        let manifest = Manifest::load(data_dir)?;

        // 按层打开所有 SSTable
        let mut levels: Vec<Vec<SSTable>> = (0..NUM_LEVELS).map(|_| Vec::new()).collect();

        for entry in &manifest.entries {
            if entry.level >= NUM_LEVELS { continue; }
            // 跳过 __counter__ 这个特殊项
            if entry.file == "__counter__" { continue; }

            let path = format!("{}/{}", data_dir, entry.file);
            if let Ok(sst) = SSTable::open(&path, 4096) {
                levels[entry.level].push(sst);
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
            level_thresholds: vec![3, 2, 2, usize::MAX],
            flush_threshold,
            manifest,
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
        // 1. MemTable
        if let Some(v) = self.memtable.get(key) {
            if v == TOMBSTONE { return Ok(None); }
            return Ok(Some(v.clone()));
        }

        // 2. 从 L0 到 L3
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
        // MemTable
        for (k, v) in self.memtable.scan(start, end) {
            merged.insert(k, v);
        }

        merged.retain(|_, v| v != TOMBSTONE);
        Ok(merged.into_iter().collect())
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        if self.memtable.is_empty() { return Ok(()); }

        let id = self.manifest.next_counter();
        let filename = format!("L0_{:06}.sst", id);
        let path = format!("{}/{}", self.data_dir, filename);

        let sst = SSTable::write(&path, self.memtable.data(), 4096)?;
        println!("[flush] L0 ← {} 条记录 ({})", sst.len(), filename);

        self.levels[0].push(sst);
        self.manifest.add(0, filename);
        self.manifest.save(&self.data_dir)?;

        self.memtable.clear();
        self.wal.clear()?;

        self.maybe_compact(0)?;

        Ok(())
    }

    fn maybe_compact(&mut self, level: usize) -> std::io::Result<()> {
        if level + 1 >= self.levels.len() {
            return Ok(());
        }
        if self.levels[level].len() < self.level_thresholds[level] {
            return Ok(());
        }

        // 从内存和 manifest 里一起移除本层的文件
        let to_merge = std::mem::take(&mut self.levels[level]);
        let old_files = self.manifest.take_level(level);
        let count = to_merge.len();

        // 分配新编号
        let id = self.manifest.next_counter();
        let new_filename = format!("L{}_{:06}.sst", level + 1, id);
        let new_path = format!("{}/{}", self.data_dir, new_filename);

        // 物理合并
        let new_sst = crate::compaction::compact_all(to_merge, &new_path)?;

        // 删除旧文件的 manifest 记录（已经 take_level 清空），加新的
        self.levels[level + 1].push(new_sst);
        self.manifest.add(level + 1, new_filename);

        // 保存 manifest（原子）
        self.manifest.save(&self.data_dir)?;

        println!("[compact] L{} ({}) → L{} ({} 条, 旧文件 {} 个)",
                 level, count, level + 1,
                 self.levels[level + 1].last().unwrap().len(),
                 old_files.len());

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
        for level in self.levels.iter().rev() {
            if let Some(sst) = level.first() {
                return Some(sst.path.clone());
            }
        }
        None
    }
}