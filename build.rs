fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/soziopolis-hires.ico");
        res.set("FileDescription", "Soziopolis Reader");
        res.set("ProductName", "Soziopolis Reader");
        res.set("OriginalFilename", "Soziopolis Reader.exe");
        res.set("InternalName", "soziopolis_lingq_tool");
        res.set(
            "LegalCopyright",
            "Copyright 2026 Soziopolis Reader contributors",
        );

        if let Err(err) = res.compile() {
            panic!("failed to compile Windows resources: {err}");
        }
    }
}
