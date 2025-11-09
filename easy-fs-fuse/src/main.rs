use clap::{App, Arg};
use easy_fs::{BlockDevice, EasyFileSystem, BLOCK_SZ};
use std::fs::{read_dir, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::Mutex;

const LABEL_WIDTH: usize = 28;
const DISK_SIZE_BYTES: usize = 320 * 1024 * 1024; // 320 MiB

fn report_duration(label: &str, duration_us: u128) {
    let duration_ms = duration_us / 1_000;
    if duration_ms > 0 {
        println!(
            "[time]\t{:width$}\t{} us ({} ms)",
            label,
            duration_us,
            duration_ms,
            width = LABEL_WIDTH
        );
    } else {
        println!(
            "[time]\t{:width$}\t{} us",
            label,
            duration_us,
            width = LABEL_WIDTH
        );
    }
}

macro_rules! time_call {
    ($label:expr, $expr:expr) => {{
        let __label: std::borrow::Cow<'static, str> = $label.into();
        let __start = std::time::Instant::now();
        let __result = { $expr };
        let __elapsed = __start.elapsed();
        crate::report_duration(__label.as_ref(), __elapsed.as_micros());
        __result
    }};
}

struct BlockFile(Mutex<File>);

impl BlockDevice for BlockFile {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        let mut filled = 0usize;
        while filled < BLOCK_SZ {
            let chunk = file.read(&mut buf[filled..]).unwrap();
            if chunk == 0 {
                // host file may be sparse; pad logical block with zeros instead of failing
                buf[filled..].fill(0);
                filled = BLOCK_SZ;
                break;
            }
            filled += chunk;
        }
        assert_eq!(filled, BLOCK_SZ, "Not a complete block!");
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        let mut written = 0usize;
        while written < BLOCK_SZ {
            let chunk = file.write(&buf[written..]).unwrap();
            if chunk == 0 {
                break;
            }
            written += chunk;
        }
        assert_eq!(written, BLOCK_SZ, "Not a complete block!");
    }
}

fn main() {
    time_call!("easy_fs_pack", easy_fs_pack()).expect("Error when packing easy-fs!");
}

fn easy_fs_pack() -> std::io::Result<()> {
    let matches = time_call!("parse_args", {
        App::new("EasyFileSystem packer")
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
            .get_matches()
    });
    let src_path = matches.value_of("source").unwrap();
    let target_path = matches.value_of("target").unwrap();
    println!("src_path = {}\ntarget_path = {}", src_path, target_path);
    let block_file = time_call!("prepare_block_file", {
        let file_path = format!("{}{}", target_path, "fs.img");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(file_path)?;
        file.set_len(DISK_SIZE_BYTES as u64).unwrap();
        Ok::<Arc<dyn BlockDevice>, std::io::Error>(Arc::new(BlockFile(Mutex::new(file))))
    })?;
    // 320MiB, at most 4095 files
    let total_blocks = DISK_SIZE_BYTES / BLOCK_SZ;
    let efs = time_call!(
        "EasyFileSystem::create",
        EasyFileSystem::create(Arc::clone(&block_file), total_blocks as u32, 1)
    );
    let root_inode = time_call!("root_inode", Arc::new(EasyFileSystem::root_inode(&efs)));
    let mut apps: Vec<_> = time_call!("scan_apps", {
        read_dir(src_path)
            .unwrap()
            .map(|dir_entry| {
                let mut name_with_ext = dir_entry.unwrap().file_name().into_string().unwrap();
                name_with_ext.drain(name_with_ext.find('.').unwrap()..name_with_ext.len());
                name_with_ext
            })
            .collect()
    });
    apps.sort();
    for app in apps {
        time_call!(format!("pack {}", app), {
            // load app data from host file system
            let mut host_file = File::open(format!("{}{}", target_path, app)).unwrap();
            let mut all_data: Vec<u8> = Vec::new();
            host_file.read_to_end(&mut all_data).unwrap();
            // create a file in easy-fs
            let inode = root_inode.create(app.as_str()).unwrap();
            // write data to easy-fs
            inode.write_at(0, all_data.as_slice());
        });
    }
    // list apps
    // for app in root_inode.ls() {
    //     println!("{}", app);
    // }
    Ok(())
}

#[test]
fn efs_test() -> std::io::Result<()> {
    const TEST_DISK_SIZE_BYTES: usize = 4 * 1024 * 1024; // 4 MiB
    let block_file = Arc::new(BlockFile(Mutex::new({
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open("target/fs.img")?;
        f.set_len(TEST_DISK_SIZE_BYTES as u64).unwrap();
        f
    })));
    let test_blocks = TEST_DISK_SIZE_BYTES / BLOCK_SZ;
    EasyFileSystem::create(block_file.clone(), test_blocks as u32, 1);
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
    //let mut buffer = [0u8; 512];
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
