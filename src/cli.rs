use crate::storage::Storage;

pub fn run() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    let data_dir = "./test_data";
    let cmd = args[1].as_str();

    match cmd {
        "put" => {
            if args.len() < 4 {
                eprintln!("用法: put <key> <value>");
                return Ok(());
            }
            let mut s = Storage::open(data_dir, 100)?;
            s.put(&args[2], &args[3])?;
            println!("OK");
        }
        "get" => {
            if args.len() < 3 {
                eprintln!("用法: get <key>");
                return Ok(());
            }
            let s = Storage::open(data_dir, 100)?;
            match s.get(&args[2])? {
                Some(v) => println!("{}", v),
                None    => println!("(not found)"),
            }
        }
        "delete" => {
            if args.len() < 3 {
                eprintln!("用法: delete <key>");
                return Ok(());
            }
            let mut s = Storage::open(data_dir, 100)?;
            s.delete(&args[2])?;
            println!("OK");
        }
        "scan" => {
            if args.len() < 4 {
                eprintln!("用法: scan <start> <end>");
                return Ok(());
            }
            let s = Storage::open(data_dir, 100)?;
            let items = s.scan(&args[2], &args[3])?;
            for (k, v) in items {
                println!("{} => {}", k, v);
            }
        }
        "compact" => {
            let mut s = Storage::open(data_dir, 100)?;
            s.flush()?;
        }
        "stats" => {
            let s = Storage::open(data_dir, 100)?;
            println!("memtable: {} 条", s.memtable_len());
            let counts = s.level_counts();
            for (i, c) in counts.iter().enumerate() {
                println!("L{}:       {} 个", i, c);
            }
        }
        "demo" => {
            crate::demo::run()?;
        }
        _ => {
            print_help();
        }
    }

    Ok(())
}

fn print_help() {
    println!("存储引擎 CLI");
    println!();
    println!("用法:");
    println!("  put <key> <value>      写入");
    println!("  get <key>              读取");
    println!("  delete <key>           删除");
    println!("  scan <start> <end>     范围扫描");
    println!("  compact                触发一次 flush/compact");
    println!("  stats                  查看各层状态");
    println!("  demo                   跑内置演示");
}