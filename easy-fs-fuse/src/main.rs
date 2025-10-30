use clap::{App, Arg};
use easy_fs::{BlockDevice, EasyFileSystem};
use std::fs::{read_dir, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

mod profiler;
use profiler::PackingProfiler;

// 简单的计时器工具
struct Timer {
    name: Option<String>,
    start: Instant,
}

impl Timer {
    // 创建静默计时器，不会自动打印
    fn silent() -> Self {
        Self {
            name: None,
            start: Instant::now(),
        }
    }

    fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(name) = &self.name {
            println!("[{}] {:.3}ms", name, self.elapsed_ms());
        }
    }
}

// 文件操作统计
struct FileStats {
    read_time: f64,
    create_time: f64,
    write_time: f64,
    size: usize,
}

impl FileStats {
    fn new() -> Self {
        Self {
            read_time: 0.0,
            create_time: 0.0,
            write_time: 0.0,
            size: 0,
        }
    }

    fn add(&mut self, other: &FileStats) {
        self.read_time += other.read_time;
        self.create_time += other.create_time;
        self.write_time += other.write_time;
        self.size += other.size;
    }

    fn print_line(&self, name: &str) {
        println!(
            "{:<30}\tR:{:>6.2}ms\tC:{:>6.2}ms\tW:{:>6.2}ms\t{:>10}",
            name,
            self.read_time,
            self.create_time,
            self.write_time,
            format_size(self.size)
        );
    }

    fn print_summary(&self) {
        println!("\n=== Summary ===");
        println!("Total size: {}", format_size(self.size));
        println!("Read:   {:.3}ms", self.read_time);
        println!("Create: {:.3}ms", self.create_time);
        println!("Write:  {:.3}ms", self.write_time);
    }
}

// 格式化文件大小为人类可读格式
fn format_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let size = bytes as f64;
    if size >= GB {
        format!("{:.2} GB", size / GB)
    } else if size >= MB {
        format!("{:.2} MB", size / MB)
    } else if size >= KB {
        format!("{:.2} KB", size / KB)
    } else {
        format!("{} B", bytes)
    }
}

const BLOCK_SZ: usize = 4096;

struct BlockFile(Mutex<File>);

impl BlockDevice for BlockFile {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        assert_eq!(file.read(buf).unwrap(), BLOCK_SZ, "Not a complete block!");
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        assert_eq!(file.write(buf).unwrap(), BLOCK_SZ, "Not a complete block!");
    }
}

fn main() {
    let start = Instant::now();
    let result = easy_fs_pack();
    let elapsed = start.elapsed();
    println!("easy_fs_pack took {:.3}s", elapsed.as_secs_f64());
    result.expect("Error when packing easy-fs!");
}

fn easy_fs_pack() -> std::io::Result<()> {
    let mut profiler = PackingProfiler::new();

    // 开始总体计时
    profiler.start_timing("total_packing");

    let matches = App::new("EasyFileSystem packer")
        .arg(
            Arg::with_name("source")
                .short("s")
                .long("source")
                .takes_value(true)
                .help("Executable source dir(with backslash)"),
        )
        .arg(
            Arg::with_name("target")
                .short("t")
                .long("target")
                .takes_value(true)
                .help("Executable target dir(with backslash)"),
        )
        .get_matches();
    let src_path = matches.value_of("source").unwrap();
    let target_path = matches.value_of("target").unwrap();
    println!("src_path = {}\ntarget_path = {}", src_path, target_path);

    let block_file = pack_time_it!(profiler, "create_block_file", {
        Arc::new(BlockFile(Mutex::new({
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(format!("{}{}", target_path, "fs.img"))?;
            f.set_len(160u64 * 2048 * BLOCK_SZ as u64).unwrap();
            f
        })))
    });

    let efs = pack_time_it!(profiler, "create_filesystem", {
        EasyFileSystem::create(block_file, 160 * 2048, 1)
    });

    let root_inode = Arc::new(EasyFileSystem::root_inode(&efs));

    let apps: Vec<_> = pack_time_it!(profiler, "file_discovery", {
        read_dir(src_path)
            .unwrap()
            .into_iter()
            .map(|dir_entry| {
                let mut name_with_ext = dir_entry.unwrap().file_name().into_string().unwrap();
                name_with_ext.drain(name_with_ext.find('.').unwrap()..name_with_ext.len());
                name_with_ext
            })
            .collect()
    });

    println!("Found {} files", apps.len());

    let mut total_stats = FileStats::new();

    // 文件处理循环
    pack_time_it!(profiler, "file_processing", {
        for app in apps {
            let mut stats = FileStats::new();

            // Read
            let timer = Timer::silent();
            let mut host_file = File::open(format!("{}{}", target_path, app)).unwrap();
            let mut all_data: Vec<u8> = Vec::new();
            host_file.read_to_end(&mut all_data).unwrap();
            stats.read_time = timer.elapsed_ms();
            stats.size = all_data.len();
            drop(timer);

            // Create
            let timer = Timer::silent();
            let inode = root_inode.create(app.as_str()).unwrap();
            stats.create_time = timer.elapsed_ms();
            drop(timer);

            // Write
            let timer = Timer::silent();
            inode.write_at(0, all_data.as_slice());
            stats.write_time = timer.elapsed_ms();
            drop(timer);

            stats.print_line(&app);
            total_stats.add(&stats);
        }
    });

    // 结束总体计时
    profiler.end_timing("total_packing");

    // 输出性能报告
    profiler.print_timing_results();
    total_stats.print_summary();

    Ok(())
}

#[test]
fn efs_test() -> std::io::Result<()> {
    let block_file = Arc::new(BlockFile(Mutex::new({
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open("target/fs.img")?;
        f.set_len(8192u64 * BLOCK_SZ as u64).unwrap();
        f
    })));
    EasyFileSystem::create(block_file.clone(), 4096, 1);
    let efs = EasyFileSystem::open(block_file.clone());
    let root_inode = EasyFileSystem::root_inode(&efs);
    root_inode.create("filea");
    root_inode.create("fileb");
    for name in root_inode.ls() {
        println!("{}", name);
    }
    let filea = root_inode.find("filea").unwrap();
    let greet_str = "Hello, world!";
    filea.write_at(0, greet_str.as_bytes());
    //let mut buffer = [0u8; BLOCK_SZ];
    let mut buffer = [0u8; 233];
    let len = filea.read_at(0, &mut buffer);
    assert_eq!(greet_str, core::str::from_utf8(&buffer[..len]).unwrap(),);

    let mut random_str_test = |len: usize| {
        filea.clear();
        assert_eq!(filea.read_at(0, &mut buffer), 0,);
        let mut str = String::new();
        use rand;
        // random digit
        for _ in 0..len {
            str.push(char::from('0' as u8 + rand::random::<u8>() % 10));
        }
        filea.write_at(0, str.as_bytes());
        let mut read_buffer = [0u8; 127];
        let mut offset = 0usize;
        let mut read_str = String::new();
        loop {
            let len = filea.read_at(offset, &mut read_buffer);
            if len == 0 {
                break;
            }
            offset += len;
            read_str.push_str(core::str::from_utf8(&read_buffer[..len]).unwrap());
        }
        assert_eq!(str, read_str);
    };

    random_str_test(4 * BLOCK_SZ);
    random_str_test(8 * BLOCK_SZ + BLOCK_SZ / 2);
    random_str_test(100 * BLOCK_SZ);
    random_str_test(70 * BLOCK_SZ + BLOCK_SZ / 7);
    random_str_test((12 + 128) * BLOCK_SZ);
    random_str_test(400 * BLOCK_SZ);
    random_str_test(1000 * BLOCK_SZ);
    random_str_test(2000 * BLOCK_SZ);

    Ok(())
}
