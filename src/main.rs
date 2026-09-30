mod memtable;
mod sstable;
mod wal;
mod storage;
mod compaction;
mod cli;
mod demo;
mod bloom;
mod lru_cache;
mod manifest;

fn main() -> std::io::Result<()> {
    cli::run()
}