fn main() {
    #[cfg(target_os = "windows")]
    {
        // 给 Windows exe 嵌入应用图标（否则资源管理器显示通用二进制图标）。
        winres::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to embed Windows app icon");
    }
}
