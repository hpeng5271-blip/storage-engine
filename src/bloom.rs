/// FNV-1a 哈希
fn fnv1a(data: &[u8], seed: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64 ^ seed;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// 布隆过滤器
pub struct BloomFilter {
    bits: Vec<u8>,
    num_bits: u32,
    num_hashes: u32,
}

impl BloomFilter {
    /// 按预期元素数创建，每个元素 10 bits，4 个哈希
    pub fn new(expected_items: usize) -> Self {
        let num_bits = ((expected_items * 10).max(64)) as u32;
        let num_hashes = 4;
        let bytes = ((num_bits + 7) / 8) as usize;
        BloomFilter {
            bits: vec![0u8; bytes],
            num_bits,
            num_hashes,
        }
    }

    pub fn add(&mut self, key: &str) {
        for i in 0..self.num_hashes {
            let h = fnv1a(key.as_bytes(), i as u64);
            let pos = (h % self.num_bits as u64) as usize;
            self.bits[pos / 8] |= 1 << (pos % 8);
        }
    }

    /// false = 肯定不存在；true = 可能存在
    pub fn might_contain(&self, key: &str) -> bool {
        for i in 0..self.num_hashes {
            let h = fnv1a(key.as_bytes(), i as u64);
            let pos = (h % self.num_bits as u64) as usize;
            if self.bits[pos / 8] & (1 << (pos % 8)) == 0 {
                return false;
            }
        }
        true
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.num_bits.to_le_bytes());
        out.extend_from_slice(&self.num_hashes.to_le_bytes());
        out.extend_from_slice(&self.bits);
        out
    }

    pub fn from_bytes(data: &[u8]) -> std::io::Result<Self> {
        if data.len() < 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bloom too small",
            ));
        }
        let num_bits = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let num_hashes = u32::from_le_bytes(data[4..8].try_into().unwrap());
        let bytes = ((num_bits + 7) / 8) as usize;

        if data.len() < 8 + bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bloom truncated",
            ));
        }

        Ok(BloomFilter {
            bits: data[8..8 + bytes].to_vec(),
            num_bits,
            num_hashes,
        })
    }
}