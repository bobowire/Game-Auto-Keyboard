fn main() {
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
