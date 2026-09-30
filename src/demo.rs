use crate::storage::Storage;

pub fn run() -> std::io::Result<()> {
    let dir = "./demo_data";
    let _ = std::fs::remove_dir_all(dir);

    // ============================================================
    //  多级 LSM 演示
    // ============================================================
    println!("=== 多级 LSM 演示 ===");
    println!("flush_threshold = 2");
    println!("层级阈值: L0=3, L1=2, L2=2, L3=∞");
    println!();
    println!("规则：Ln 累积到阈值 → 全部合并为 1 个 SSTable → 推入 Ln+1");
    println!();

    let mut s = Storage::open(dir, 2)?;

    for i in 0..30 {
        let key = format!("key_{:02}", i);
        let val = format!("value_{}", i);
        s.put(&key, &val)?;

        let lc = s.level_counts();
        println!("  put {}   [mem={}, L0={}, L1={}, L2={}, L3={}]",
                 key, s.memtable_len(), lc[0], lc[1], lc[2], lc[3]);
    }

    println!();
    println!("=== 最终各层状态 ===");
    let lc = s.level_counts();
    println!("memtable: {} 条", s.memtable_len());
    for (i, c) in lc.iter().enumerate() {
        println!("L{}:       {} 个", i, c);
    }

    println!();
    println!("=== 验证所有数据（走完整读路径）===");
    for i in 0..30 {
        let key = format!("key_{:02}", i);
        match s.get(&key)? {
            Some(v) => println!("  {} => {}", key, v),
            None    => println!("  {} => (not found)", key),
        }
    }

    // 关闭再打开，验证持久化
    drop(s);
    println!();
    println!("=== 重新打开（验证层级持久化）===");
    let s = Storage::open(dir, 2)?;
    let lc = s.level_counts();
    println!("memtable: {} 条", s.memtable_len());
    for (i, c) in lc.iter().enumerate() {
        println!("L{}:       {} 个", i, c);
    }
    // 抽查几个 key
    for i in [0, 5, 15, 29] {
        let key = format!("key_{:02}", i);
        match s.get(&key)? {
            Some(v) => println!("  {} => {}", key, v),
            None    => println!("  {} => (not found)", key),
        }
    }

    // ============================================================
    //  块缓存演示
    // ============================================================
    let sst_path = s.first_sstable_path();

    println!();
    println!("=== 块缓存演示 ===");

    match sst_path.clone() {
        Some(path) => {
            let sst = crate::sstable::SSTable::open(&path, 256)?;
            println!("打开 SSTable: {}", path);
            println!("块大小: 256 字节");
            println!("记录数: {}", sst.len());
            println!();

            for k in &["key_00", "key_01", "key_02"] {
                let v = sst.get(k)?;
                println!("读 {}  →  {:?}  缓存块数: {}", k, v, sst.cache_size());
            }
            let missing = sst.get("nonexistent")?;
            println!("读 nonexistent  →  {:?}", missing);
            println!("缓存块数: {}", sst.cache_size());
        }
        None => println!("未找到 SSTable，跳过"),
    }

    // ============================================================
    //  Bloom Filter 演示
    // ============================================================
    println!();
    println!("=== Bloom Filter 演示 ===");

    match sst_path {
        Some(path) => {
            let sst = crate::sstable::SSTable::open(&path, 4096)?;
            println!("打开 SSTable: {}", path);
            println!("记录数: {}", sst.len());
            println!();

            // 只查这个文件里有的 key
            for k in sst.index_keys().iter().take(3) {
                let v = sst.get(k)?;
                println!("读 {}  →  {:?}", k, v);
            }
            println!("bloom 避免读盘次数: {}", sst.bloom_avoided());

            println!();
            println!("查询 100 个不存在的 key...");
            for i in 1000..1100 {
                let k = format!("missing_{}", i);
                let _ = sst.get(&k)?;
            }
            println!("bloom 避免读盘次数: {}", sst.bloom_avoided());
        }
        None => println!("未找到 SSTable，跳过"),
    }

    Ok(())
}