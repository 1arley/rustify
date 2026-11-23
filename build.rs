#[cfg(target_os = "windows")]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("packaging/rustify.ico");
    res.compile().expect("failed to compile Windows resources");
}

#[cfg(not(target_os = "windows"))]
fn main() {}