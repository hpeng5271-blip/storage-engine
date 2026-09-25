use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};

struct Storage {
    data_file: File,
    idx_file: File,
    offsets: Vec<u64>,
}

impl Storage {
    fn open(data_path: &str, idx_path: &str) -> std::io::Result<Self> {
        let mut data_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(data_path)?;

        let mut idx_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(idx_path)?;

        let offsets = match Self::load_offsets_from_idx(&mut idx_file) {
            Ok(offs) if !offs.is_empty() => {
                println!("[启动] 从索引文件加载了 {} 条偏移量", offs.len());
                offs
            }
            _ => {
                println!("[启动] 索引文件为空或无效，从数据文件重建");
                let offs = Self::rebuild_offsets(&mut data_file)?;
                Self::save_offsets_to_idx(&mut idx_file, &offs)?;
                println!("[启动] 重建完成，写入了 {} 条偏移量到索引文件", offs.len());
                offs
            }
        };

        Ok(Storage { data_file, idx_file, offsets })
    }

    fn load_offsets_from_idx(idx_file: &mut File) -> std::io::Result<Vec<u64>> {
        let mut offsets = Vec::new();
        idx_file.seek(SeekFrom::Start(0))?;
        let mut buf = [0u8; 8];
        loop {
            match idx_file.read_exact(&mut buf) {
                Ok(()) => offsets.push(u64::from_le_bytes(buf)),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        }
        Ok(offsets)
    }

    fn save_offsets_to_idx(idx_file: &mut File, offsets: &[u64]) -> std::io::Result<()> {
        idx_file.seek(SeekFrom::Start(0))?;
        idx_file.set_len(0)?;
        for &off in offsets {
            idx_file.write_all(&off.to_le_bytes())?;
        }
        idx_file.sync_all()?;
        Ok(())
    }

    fn rebuild_offsets(data_file: &mut File) -> std::io::Result<Vec<u64>> {
        let mut offsets = Vec::new();
        let file_len = data_file.metadata()?.len();
        let mut pos = 0u64;

        while pos < file_len {
            offsets.push(pos);
            data_file.seek(SeekFrom::Start(pos))?;
            let mut len_buf = [0u8; 4];
            data_file.read_exact(&mut len_buf)?;
            let len = u32::from_le_bytes(len_buf) as u64;
            pos += 4 + len;
        }
        Ok(offsets)
    }

    fn append(&mut self, content: &str) -> std::io::Result<()> {
        let pos = self.data_file.seek(SeekFrom::End(0))?;
        println!("[写入] 记录 '{}' 将写入偏移量 {}", content, pos);

        let bytes = content.as_bytes();
        let len = bytes.len() as u32;
        self.data_file.write_all(&len.to_le_bytes())?;
        self.data_file.write_all(bytes)?;
        self.data_file.sync_all()?;

        self.offsets.push(pos);

        self.idx_file.seek(SeekFrom::End(0))?;
        self.idx_file.write_all(&pos.to_le_bytes())?;
        self.idx_file.sync_all()?;
        println!("[写入] 索引文件已更新，当前共 {} 条偏移量", self.offsets.len());

        Ok(())
    }

    fn read(&mut self, index: usize) -> std::io::Result<String> {
        let offset = self.offsets[index];
        println!("[读取] 记录 {} 的偏移量是 {}", index, offset);

        self.data_file.seek(SeekFrom::Start(offset))?;

        let mut len_buf = [0u8; 4];
        self.data_file.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf) as usize;

        let mut buf = vec![0u8; len];
        self.data_file.read_exact(&mut buf)?;

        Ok(String::from_utf8(buf).unwrap())
    }

    fn len(&self) -> usize {
        self.offsets.len()
    }
}

fn main() -> std::io::Result<()> {
    println!("=== 第一次打开：写入三条记录 ===");
    {
        let mut storage = Storage::open("data.bin", "data.idx")?;
        storage.append("第一条记录")?;
        storage.append("这是第二条，比第一条长一些")?;
        storage.append("short")?;
        println!("[完成] 写入完成，共 {} 条记录\n", storage.len());
    }

    println!("=== 第二次打开：验证索引从文件加载 ===");
    {
        let mut storage = Storage::open("data.bin", "data.idx")?;
        println!("[完成] 重新打开，共 {} 条记录", storage.len());
        for i in 0..storage.len() {
            let s = storage.read(i)?;
            println!("记录 {}: {}", i, s);
        }
    }

    Ok(())
}