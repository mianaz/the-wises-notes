fn main() {
    println!("cargo:rerun-if-changed=app/persona.txt");
    println!("cargo:rerun-if-env-changed=SAGE_LINK_DIR");
    if std::env::var("CARGO_FEATURE_TAKEOVER").is_ok() {
        // libquill.so (the quill e-ink C ABI) and libqsgepaper.so (reMarkable's
        // engine) are linked from SAGE_LINK_DIR, default ~/sage-link. Copy them
        // from your own tablet; see scripts/build.sh. Neither is redistributed.
        let dir = std::env::var("SAGE_LINK_DIR").unwrap_or_else(|_| {
            format!("{}/sage-link", std::env::var("HOME").unwrap_or_default())
        });
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rustc-link-lib=dylib=quill");
        println!("cargo:rustc-link-lib=dylib=qsgepaper");
        println!("cargo:rustc-link-arg=-Wl,-rpath,/home/root/quill:/usr/lib/plugins/scenegraph");
        // libquill's Qt dependencies resolve on the device at run time.
        println!("cargo:rustc-link-arg=-Wl,--allow-shlib-undefined");
    }
}
