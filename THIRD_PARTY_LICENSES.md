# Third-party licenses

`wreq-php` is distributed under **LGPL-3.0-or-later**. The native extension
links several Rust crates; their licenses are summarised below.

| Crate | License | Notes |
|-------|---------|-------|
| [`wreq`](https://crates.io/crates/wreq) | Apache-2.0 | HTTP client core. |
| [`wreq-util`](https://crates.io/crates/wreq-util) | **LGPL-3.0** | Browser emulation profiles. This is why the project as a whole is LGPL-3.0. |
| [`ext-php-rs`](https://crates.io/crates/ext-php-rs) | MIT OR Apache-2.0 | PHP ↔ Rust bindings. |
| [`ext-php-rs-bindgen`](https://crates.io/crates/ext-php-rs-bindgen) | BSD-3-Clause | Build-time only. |
| [`tokio`](https://crates.io/crates/tokio), `serde_json`, `thiserror`, `strum` | MIT (or MIT/Apache) | Permissive. |

## Why the project is LGPL-3.0

`wreq-util` is licensed **LGPL-3.0**. Because the prebuilt extension binaries
statically link it, the combined work — and therefore this project — is
distributed under **LGPL-3.0-or-later**. Apache-2.0 (`wreq`) and MIT crates are
one-way compatible with LGPL-3.0, so the combination is consistent.

LGPL-3.0 incorporates the GNU GPL v3 by reference; its full text is at
<https://www.gnu.org/licenses/gpl-3.0.txt>. The LGPL-3.0 text is in `LICENSE`.

Under LGPL-3.0 §4, recipients of a prebuilt binary may modify `wreq-util` (or
any other component) and relink: the complete, buildable source of `wreq-php`
is published in this repository, and `cargo build --release` reproduces the
extension.

If you need a permissively licensed build, replace `wreq-util` with a profile
source under a compatible license and rebuild.
