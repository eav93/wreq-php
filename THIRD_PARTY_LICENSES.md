# Third-party licenses

`wreq-php` is distributed under **LGPL-3.0-or-later**. The native extension
links several Rust crates; their licenses are summarised below.

| Crate | License | Notes |
|-------|---------|-------|
| [`wreq`](https://crates.io/crates/wreq) | Apache-2.0 | HTTP client core. |
| [`wreq-util`](https://crates.io/crates/wreq-util) | Apache-2.0 | Browser emulation profiles. |
| [`ext-php-rs`](https://crates.io/crates/ext-php-rs) | MIT OR Apache-2.0 | PHP ↔ Rust bindings. |
| [`ext-php-rs-bindgen`](https://crates.io/crates/ext-php-rs-bindgen) | BSD-3-Clause | Build-time only. |
| [`btls`](https://crates.io/crates/btls), [`btls-sys`](https://crates.io/crates/btls-sys) | Apache-2.0 / MIT | BoringSSL bindings used by `wreq`. |
| [`tokio`](https://crates.io/crates/tokio), `serde_json`, `thiserror`, `indexmap` | MIT (or MIT/Apache) | Permissive. |

## Project and dependency licensing

`wreq-php` remains distributed under **LGPL-3.0-or-later**. The current
`wreq` and `wreq-util` releases are Apache-2.0, while the remaining core
dependencies use permissive MIT, Apache-2.0, BSD, or compatible dual licenses.
Those licenses are compatible with distribution of the combined extension
under the project's LGPL license.

LGPL-3.0 incorporates the GNU GPL v3 by reference; its full text is at
<https://www.gnu.org/licenses/gpl-3.0.txt>. The LGPL-3.0 text is in `LICENSE`.

The complete, buildable source of `wreq-php` is published in this repository,
and `cargo build --release` reproduces the extension.
