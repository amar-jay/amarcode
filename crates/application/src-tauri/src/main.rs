fn main() {
    // WebKitGTK's DMA-BUF renderer can silently skip backdrop-filter effects on
    // some Linux/Wayland GPU stacks. Select its fallback renderer before the
    // webview process starts so translucent surfaces are composited correctly.
    // #[cfg(target_os = "linux")]
    // std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");

    acp_workbench_lib::run();
}
