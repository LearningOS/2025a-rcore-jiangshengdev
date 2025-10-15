//! 将用户应用程序加载到内存中

/// 获取应用程序总数。
use alloc::vec::Vec;
use lazy_static::*;

/// 获取应用程序数量
pub fn get_num_app() -> usize {
    extern "C" {
        fn _num_app();
    }
    // 从链接器脚本定义的符号中读取应用程序数量
    unsafe { (_num_app as usize as *const usize).read_volatile() }
}
/// 获取应用程序数据
pub fn get_app_data(app_id: usize) -> &'static [u8] {
    extern "C" {
        fn _num_app();
    }
    let num_app_ptr = _num_app as usize as *const usize;
    let num_app = get_num_app();
    // 获取应用程序起始地址数组
    let app_start = unsafe { core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1) };
    assert!(app_id < num_app);
    // 根据起始地址和结束地址计算应用程序数据切片
    unsafe {
        core::slice::from_raw_parts(
            app_start[app_id] as *const u8,
            app_start[app_id + 1] - app_start[app_id],
        )
    }
}

lazy_static! {
    /// 所有应用程序的名称
    static ref APP_NAMES: Vec<&'static str> = {
        let num_app = get_num_app();
        extern "C" {
            fn _app_names();
        }
        let mut start = _app_names as usize as *const u8;
        let mut v = Vec::new();
        unsafe {
            // 解析以空字符分隔的应用程序名称字符串
            for _ in 0..num_app {
                let mut end = start;
                // 找到字符串结束位置
                while end.read_volatile() != b'\0' {
                    end = end.add(1);
                }
                // 创建字符串切片并转换为&str
                let slice = core::slice::from_raw_parts(start, end as usize - start as usize);
                let str = core::str::from_utf8(slice).unwrap();
                v.push(str);
                // 移动到下一个字符串的开始位置
                start = end.add(1);
            }
        }
        v
    };
}

#[allow(unused)]
/// 根据名称获取应用程序数据
pub fn get_app_data_by_name(name: &str) -> Option<&'static [u8]> {
    let num_app = get_num_app();
    // 在应用程序名称列表中查找匹配的名称，然后获取对应的数据
    (0..num_app)
        .find(|&i| APP_NAMES[i] == name)
        .map(get_app_data)
}
/// 列出所有应用程序
pub fn list_apps() {
    println!("/**** APPS ****");
    // 遍历并打印所有应用程序名称
    for app in APP_NAMES.iter() {
        println!("{}", app);
    }
    println!("**************/");
}
