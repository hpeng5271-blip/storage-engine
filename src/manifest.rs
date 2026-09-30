use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ManifestEntry {
    pub level: usize,
    pub file: String,   // 相对路径（相对 data_dir）
}

pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
    pub counter: u32,   // 下一个要用的 SSTable 编号
}

impl Manifest {
    pub fn empty() -> Self {
        Manifest {
            entries: Vec::new(),
            counter: 0,
        }
    }

    /// 从 data_dir/MANIFEST 加载。文件不存在 → 返回空。
    pub fn load(data_dir: &str) -> std::io::Result<Self> {
        let path = format!("{}/MANIFEST", data_dir);
        if !Path::new(&path).exists() {
            return Ok(Manifest::empty());
        }

        let mut file = File::open(&path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let mut entries = Vec::new();
        let mut counter: u32 = 0;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }

            // 格式: counter level file
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 3 { continue; }

            if let Ok(c) = parts[0].parse::<u32>() {
                if c > counter { counter = c; }
            }
            if let Ok(l) = parts[1].parse::<usize>() {
                entries.push(ManifestEntry {
                    level: l,
                    file: parts[2].to_string(),
                });
            }
        }

        Ok(Manifest { entries, counter })
    }

    /// 原子写入 data_dir/MANIFEST：
    ///   1. 写 MANIFEST.tmp
    ///   2. fsync
    ///   3. rename 为 MANIFEST
    pub fn save(&self, data_dir: &str) -> std::io::Result<()> {
        let tmp_path = format!("{}/MANIFEST.tmp", data_dir);
        let final_path = format!("{}/MANIFEST", data_dir);

        let mut content = String::new();
        // 第一行：counter（用 level=0, file=__counter__ 表示）
        content.push_str(&format!("{} 0 __counter__\n", self.counter));
        for e in &self.entries {
            content.push_str(&format!("{} {} {}\n", self.counter, e.level, e.file));
        }

        let mut file = OpenOptions::new()
            .write(true).create(true).truncate(true)
            .open(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);

        // 原子重命名
        std::fs::rename(&tmp_path, &final_path)?;

        Ok(())
    }

    /// 加一条记录
    pub fn add(&mut self, level: usize, file: String) {
        self.entries.push(ManifestEntry { level, file });
    }

    #[allow(dead_code)]
    /// 移除一条（按文件名）
    pub fn remove(&mut self, file: &str) {
        self.entries.retain(|e| e.file != file);
    }

    /// 把某层的所有条目清空，返回被清空的文件名
    pub fn take_level(&mut self, level: usize) -> Vec<String> {
        let mut taken = Vec::new();
        let mut kept = Vec::new();
        for e in self.entries.drain(..) {
            if e.level == level {
                taken.push(e.file);
            } else {
                kept.push(e);
            }
        }
        self.entries = kept;
        taken
    }

    #[allow(dead_code)]
    /// 按层返回所有文件（有序）
    pub fn files_at_level(&self, level: usize) -> Vec<String> {
        let mut files: Vec<String> = self.entries.iter()
            .filter(|e| e.level == level)
            .map(|e| e.file.clone())
            .collect();
        files.sort();
        files
    }

    /// 分配一个新编号
    pub fn next_counter(&mut self) -> u32 {
        let c = self.counter;
        self.counter += 1;
        c
    }
}