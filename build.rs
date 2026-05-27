fn main() {
    // ext-php-rs 0.15+ resolves all PHP include/link configuration in its own
    // build script. We only re-run when the PHP toolchain selection changes.
    println!("cargo:rerun-if-env-changed=PHP_CONFIG");
    println!("cargo:rerun-if-env-changed=PHP");

    // The release version (the git tag) is injected by CI via WREQ_PHP_VERSION
    // and exposed to PHP as `Wreq\Ext\version()`, so the PHP layer can detect a
    // mismatch between the native binary and the Composer package. Local builds
    // fall back to a dev marker.
    let version = std::env::var("WREQ_PHP_VERSION")
        .ok()
        .map(|v| v.trim().trim_start_matches('v').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "0.0.0-dev".to_string());
    println!("cargo:rustc-env=WREQ_PHP_BUILD_VERSION={version}");
    println!("cargo:rerun-if-env-changed=WREQ_PHP_VERSION");

    // Surface the resolved `wreq` version in `phpinfo()` — `CARGO_PKG_VERSION`
    // only exposes our own crate, so we read it out of `Cargo.lock`.
    let wreq_version = read_wreq_version().unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=WREQ_CRATE_VERSION={wreq_version}");
    println!("cargo:rerun-if-changed=Cargo.lock");

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

/// Scans `Cargo.lock` for the `wreq` package and returns its resolved version.
/// Hand-rolled rather than pulling in `toml` just for the build script.
fn read_wreq_version() -> Option<String> {
    let lock = std::fs::read_to_string("Cargo.lock").ok()?;
    let mut in_wreq = false;
    for line in lock.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            in_wreq = false;
        } else if trimmed == "name = \"wreq\"" {
            in_wreq = true;
        } else if in_wreq {
            if let Some(rest) = trimmed.strip_prefix("version = \"") {
                return rest.strip_suffix('"').map(str::to_string);
            }
        }
    }
    None
}
