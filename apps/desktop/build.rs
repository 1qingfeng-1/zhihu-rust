fn main() {
    println!("cargo:rerun-if-changed=assets/zhihu.rc");
    println!("cargo:rerun-if-changed=assets/zhihu.ico");
    #[cfg(windows)]
    {
        let _ = embed_resource::compile("assets/zhihu.rc", embed_resource::NONE);
    }
}
