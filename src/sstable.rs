use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::bloom::BloomFilter;
use crate::lru_cache::LruCache;

const MAGIC: u32 = 0x53535442;  // "SSTB"
const TAIL_SIZE: u64 = 20;      // index_offset(8) + bloom_offset(8) + magic(4)
const BLOCK_CACHE_CAPACITY: usize = 64;

/// SSTable 把数据分成固定大小的块。
/// 读取一个 key 时，把整块读入内存，之后同块的 key 直接走缓存。
pub struct SSTable {
    pub path: String,
    index: BTreeMap<String, (u64, u64)>,  // key → (block_offset, key_offset_in_block)
    bloom: BloomFilter,
    file: Mutex<File>,
    block_cache: Mutex<LruCache<u64, Vec<u8>>>,  // block_offset → 块字节
    block_size: usize,
    bloom_avoided: AtomicUsize,  // 因 bloom 避免的读盘次数
}

impl SSTable {
    /// 写入 SSTable，按块切分
    pub fn write(
        path: &str,
        data: &BTreeMap<String, String>,
        block_size: usize,
    ) -> std::io::Result<Self> {
        let mut file = OpenOptions::new()
            .write(true).create(true).truncate(true).read(true)
            .open(path)?;

        let mut index = BTreeMap::new();
        let mut bloom = BloomFilter::new(data.len().max(1));

        let mut block_offset: u64 = 0;
        let mut block_buf: Vec<u8> = Vec::with_capacity(block_size);

        // --- 数据区 ---
        for (k, v) in data.iter() {
            bloom.add(k);

            let mut rec: Vec<u8> = Vec::new();
            let klen = k.len() as u32;
            let vlen = v.len() as u32;
            rec.extend_from_slice(&klen.to_le_bytes());
            rec.extend_from_slice(k.as_bytes());
            rec.extend_from_slice(&vlen.to_le_bytes());
            rec.extend_from_slice(v.as_bytes());

            if !block_buf.is_empty() && block_buf.len() + rec.len() > block_size {
                file.write_all(&block_buf)?;
                block_offset += block_buf.len() as u64;
                block_buf.clear();
            }

            let key_offset_in_block = block_buf.len() as u64;
            index.insert(k.clone(), (block_offset, key_offset_in_block));
            block_buf.extend_from_slice(&rec);
        }

        if !block_buf.is_empty() {
            file.write_all(&block_buf)?;
            block_offset += block_buf.len() as u64;
        }

        // --- 索引区 ---
        let index_offset = block_offset;
        let num = index.len() as u32;
        file.write_all(&num.to_le_bytes())?;

        for (k, (bo, ko)) in index.iter() {
            let klen = k.len() as u32;
            file.write_all(&klen.to_le_bytes())?;
            file.write_all(k.as_bytes())?;
            file.write_all(&bo.to_le_bytes())?;
            file.write_all(&ko.to_le_bytes())?;
        }

        // --- Bloom 区 ---
        let bloom_offset = file.stream_position()?;
        let bloom_bytes = bloom.to_bytes();
        file.write_all(&bloom_bytes)?;

        // --- 尾部 ---
        file.write_all(&index_offset.to_le_bytes())?;
        file.write_all(&bloom_offset.to_le_bytes())?;
        file.write_all(&MAGIC.to_le_bytes())?;

        file.sync_all()?;

        Ok(SSTable {
            path: path.to_string(),
            index,
            bloom,
            file: Mutex::new(file),
            block_cache: Mutex::new(LruCache::new(BLOCK_CACHE_CAPACITY)),
            block_size,
            bloom_avoided: AtomicUsize::new(0),
        })
    }

    /// 打开：读尾部 + 索引区 + Bloom 区
    pub fn open(path: &str, block_size: usize) -> std::io::Result<Self> {
        let mut file = OpenOptions::new().read(true).open(path)?;
        let file_len = file.metadata()?.len();

        if file_len < TAIL_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "file too small",
            ));
        }

        // --- 尾部 20 字节 ---
        file.seek(SeekFrom::Start(file_len - TAIL_SIZE))?;
        let mut tail = [0u8; 20];
        file.read_exact(&mut tail)?;

        let index_offset = u64::from_le_bytes(tail[0..8].try_into().unwrap());
        let bloom_offset = u64::from_le_bytes(tail[8..16].try_into().unwrap());
        let magic = u32::from_le_bytes(tail[16..20].try_into().unwrap());

        if magic != MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad magic: 0x{:08x}", magic),
            ));
        }

        // --- 索引区 ---
        file.seek(SeekFrom::Start(index_offset))?;
        let mut num_buf = [0u8; 4];
        file.read_exact(&mut num_buf)?;
        let num = u32::from_le_bytes(num_buf);

        let mut index = BTreeMap::new();
        for _ in 0..num {
            let mut klen_buf = [0u8; 4];
            file.read_exact(&mut klen_buf)?;
            let klen = u32::from_le_bytes(klen_buf) as usize;

            let mut key_buf = vec![0u8; klen];
            file.read_exact(&mut key_buf)?;
            let key = String::from_utf8(key_buf).unwrap();

            let mut bo_buf = [0u8; 8];
            file.read_exact(&mut bo_buf)?;
            let bo = u64::from_le_bytes(bo_buf);

            let mut ko_buf = [0u8; 8];
            file.read_exact(&mut ko_buf)?;
            let ko = u64::from_le_bytes(ko_buf);

            index.insert(key, (bo, ko));
        }

        // --- Bloom 区 ---
        file.seek(SeekFrom::Start(bloom_offset))?;
        let bloom_len = (file_len - TAIL_SIZE - bloom_offset) as usize;
        let mut bloom_buf = vec![0u8; bloom_len];
        file.read_exact(&mut bloom_buf)?;
        let bloom = BloomFilter::from_bytes(&bloom_buf)?;

        Ok(SSTable {
            path: path.to_string(),
            index,
            bloom,
            file: Mutex::new(file),
            block_cache: Mutex::new(LruCache::new(BLOCK_CACHE_CAPACITY)),
            block_size,
            bloom_avoided: AtomicUsize::new(0),
        })
    }

    /// 读一个块（带 LRU 缓存）
    fn load_block(&self, block_offset: u64) -> std::io::Result<Vec<u8>> {
        // 1. 查缓存
        {
            let mut cache = self.block_cache.lock().unwrap();
            if let Some(b) = cache.get(&block_offset) {
                return Ok(b);
            }
        }

        // 2. 读盘
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(block_offset))?;

        let mut buf = vec![0u8; self.block_size];
        let n = file.read(&mut buf)?;
        buf.truncate(n);

        // 3. 入缓存
        {
            let mut cache = self.block_cache.lock().unwrap();
            cache.put(block_offset, buf.clone());
        }

        Ok(buf)
    }

    /// 按 key 查
    pub fn get(&self, key: &str) -> std::io::Result<Option<String>> {
        // 1. Bloom 过滤
        if !self.bloom.might_contain(key) {
            self.bloom_avoided.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        // 2. 查索引
        let (block_offset, key_offset) = match self.index.get(key) {
            Some(v) => *v,
            None => return Ok(None),
        };

        // 3. 读块
        let block = self.load_block(block_offset)?;

        let mut pos = key_offset as usize;

        if pos + 4 > block.len() { return Ok(None); }
        let klen = u32::from_le_bytes(block[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;

        if pos + klen > block.len() { return Ok(None); }
        let k = std::str::from_utf8(&block[pos..pos+klen]).unwrap();
        pos += klen;

        if pos + 4 > block.len() { return Ok(None); }
        let vlen = u32::from_le_bytes(block[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;

        if pos + vlen > block.len() { return Ok(None); }
        let v = std::str::from_utf8(&block[pos..pos+vlen]).unwrap().to_string();

        // 4. 验证 key 匹配（防止索引错位）
        if k != key {
            return Ok(None);
        }

        Ok(Some(v))
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn iter_all(&self) -> std::io::Result<Vec<(String, String)>> {
        let mut result = Vec::new();
        for k in self.index.keys() {
            if let Some(v) = self.get(k)? {
                result.push((k.clone(), v));
            }
        }
        Ok(result)
    }

    pub fn scan(&self, start: &str, end: &str) -> std::io::Result<Vec<(String, String)>> {
        let keys: Vec<String> = self.index
            .range(start.to_string()..=end.to_string())
            .map(|(k, _)| k.clone())
            .collect();

        let mut result = Vec::new();
        for k in keys {
            if let Some(v) = self.get(&k)? {
                result.push((k, v));
            }
        }
        Ok(result)
    }

    /// 当前缓存的块数
    pub fn cache_size(&self) -> usize {
        self.block_cache.lock().unwrap().len()
    }

    /// 缓存淘汰次数
    #[allow(dead_code)]
    pub fn evictions(&self) -> usize {
        self.block_cache.lock().unwrap().evictions
    }

    /// 因 bloom 避免的读盘次数
    pub fn bloom_avoided(&self) -> usize {
        self.bloom_avoided.load(Ordering::Relaxed)
    }

        /// 返回所有 key（供 demo 使用）
    pub fn index_keys(&self) -> Vec<String> {
        self.index.keys().cloned().collect()
    }
}