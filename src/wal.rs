use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};

pub const OP_PUT: u8 = 0;
pub const OP_DELETE: u8 = 1;

pub struct Wal {
    file: File,
    pub path: String,
}

impl Wal {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true).write(true).create(true)
            .open(path)?;
        file.seek(SeekFrom::End(0))?;
        Ok(Wal { file, path: path.to_string() })
    }

    pub fn append_put(&mut self, key: &str, value: &str) -> std::io::Result<()> {
        self.append(OP_PUT, key, value)
    }

    pub fn append_delete(&mut self, key: &str) -> std::io::Result<()> {
        self.append(OP_DELETE, key, "")
    }

    fn append(&mut self, op: u8, key: &str, value: &str) -> std::io::Result<()> {
        let klen = key.len() as u32;
        let vlen = value.len() as u32;

        self.file.write_all(&[op])?;
        self.file.write_all(&klen.to_le_bytes())?;
        self.file.write_all(key.as_bytes())?;
        self.file.write_all(&vlen.to_le_bytes())?;
        self.file.write_all(value.as_bytes())?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn replay(&mut self) -> std::io::Result<Vec<(u8, String, String)>> {
        let mut file = File::open(&self.path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let mut entries = Vec::new();
        let mut pos = 0;

        while pos < buf.len() {
            if pos + 1 > buf.len() { break; }
            let op = buf[pos];
            pos += 1;

            if pos + 4 > buf.len() { break; }
            let klen = u32::from_le_bytes(buf[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;

            if pos + klen > buf.len() { break; }
            let key = String::from_utf8(buf[pos..pos+klen].to_vec()).unwrap();
            pos += klen;

            if pos + 4 > buf.len() { break; }
            let vlen = u32::from_le_bytes(buf[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;

            if pos + vlen > buf.len() { break; }
            let value = String::from_utf8(buf[pos..pos+vlen].to_vec()).unwrap();
            pos += vlen;

            entries.push((op, key, value));
        }

        Ok(entries)
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}