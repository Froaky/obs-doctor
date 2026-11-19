//! Puts the product name and version inside the Windows executable, so
//! Explorer shows them under Properties > Details and code signing can check
//! them.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "OBS Doctor")
        .set("FileDescription", "OBS Doctor")
        .set("OriginalFilename", "obs-doctor.exe")
        .set("LegalCopyright", "Licencia MIT");
    if let Err(err) = res.compile() {
        panic!("could not embed the Windows version information: {err}");
    }
}
