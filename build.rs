fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("manifest.xml");
        if let Err(e) = res.compile() {
            eprintln!("嵌入清单失败: {}", e);
        }
    }
}
