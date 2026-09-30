use std::collections::BTreeMap;
use std::fs;
use crate::memtable::TOMBSTONE;
use crate::sstable::SSTable;

/// 合并一组 SSTable 为 1 个新文件。
/// sstables 按旧 → 新排列，新数据覆盖旧数据。
/// 移除墓碑。
pub fn compact_all(sstables: Vec<SSTable>, new_path: &str) -> std::io::Result<SSTable> {
    let mut merged: BTreeMap<String, String> = BTreeMap::new();

    for sst in sstables.iter() {
        for (k, v) in sst.iter_all()? {
            merged.insert(k, v);
        }
    }

    merged.retain(|_, v| v != TOMBSTONE);

    let old_paths: Vec<String> = sstables.iter().map(|s| s.path.clone()).collect();
    drop(sstables);

    for p in &old_paths {
        let _ = fs::remove_file(p);
    }

    SSTable::write(new_path, &merged, 4096)
}