fn main() {
    println!("cargo:rerun-if-changed=assets/zhihu.rc");
    println!("cargo:rerun-if-changed=assets/zhihu.ico");
    #[cfg(windows)]
    {
        let _ = embed_resource::compile("assets/zhihu.rc", embed_resource::NONE);
    }
    // GPUI's debug-mode layout tree for the settings screen needs more than the
    // 1 MiB Windows default main-thread stack. Release builds fit comfortably,
    // but raise the reservation so `cargo run` behaves like the shipped build.
    #[cfg(all(target_os = "windows", target_env = "msvc"))]
    println!("cargo:rustc-link-arg-bin=zhihu-desktop=/STACK:8388608");
}
