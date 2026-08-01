fn main() {
    // 生成构建日期版本号 (YYYYMMDD)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 计算日期（简化算法，基于 Unix 纪元）
    let days = now / 86400;
    let (year, month, day) = unix_days_to_date(days);

    let build_date = format!("{:04}{:02}{:02}", year, month, day);
    // 不输出 cargo:rerun-if-changed=build.rs：那条指令会把 build script 限制为仅在
    // build.rs 自身变化时重跑，导致即使源码改了 BUILD_DATE 也不刷新。保留 Cargo 默认
    // 行为（包内任意文件变化即重跑 build script），保证每次重新编译都更新为当天日期。
    println!("cargo:rustc-env=BUILD_DATE={}", build_date);

    #[cfg(target_os = "windows")]
    {
        // manifest.xml 里声明了 requireAdministrator，嵌进 exe 后 cargo test 生成的
        // 测试二进制也会带上，非管理员环境下直接起不来（os error 740）。
        // 设 GAK_SKIP_MANIFEST=1 可跳过嵌入，供跑测试用。
        println!("cargo:rerun-if-env-changed=GAK_SKIP_MANIFEST");
        if std::env::var("GAK_SKIP_MANIFEST").is_ok() {
            return;
        }

        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("manifest.xml");
        if let Err(e) = res.compile() {
            eprintln!("嵌入清单失败: {}", e);
        }
    }
}

/// Unix 天数转公历日期（简化算法，适用于 1970-2100 年）
fn unix_days_to_date(days: u64) -> (i32, u32, u32) {
    // 1970-01-01 = day 0
    let mut year = 1970_i32;
    let mut remaining = days;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let days_in_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1_u32;
    let mut day = remaining;

    for (m, &dim) in days_in_month.iter().enumerate() {
        let dim = if m == 1 && is_leap_year(year) { 29 } else { dim };
        if day < dim {
            month = m as u32 + 1;
            break;
        }
        day -= dim;
    }

    (year, month, day as u32 + 1)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
