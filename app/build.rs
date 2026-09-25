fn main() {
    slint_build::compile("ui/app.slint").expect("slint UI");
    // Executable icon for Explorer and the taskbar.
    embed_resource::compile("app.rc", embed_resource::NONE).manifest_optional().unwrap();
    println!("cargo:rerun-if-changed=icons/icon.ico");
}
