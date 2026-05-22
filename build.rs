fn main() {
    // ext-php-rs 0.15+ resolves all PHP include/link configuration in its own
    // build script. We only re-run when the PHP toolchain selection changes.
    println!("cargo:rerun-if-env-changed=PHP_CONFIG");
    println!("cargo:rerun-if-env-changed=PHP");

    // On macOS a PHP extension is a `cdylib` that references PHP's symbols,
    // which are only available once the host PHP process loads it. Tell the
    // linker to leave those symbols unresolved and bind them dynamically at
    // load time. ELF (Linux) already allows this; Windows links the PHP import
    // library through ext-php-rs's own build script.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-undefined");
        println!("cargo:rustc-link-arg=dynamic_lookup");
    }
}
