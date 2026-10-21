#!/bin/sh
# Run in php:8.5-zts-bookworm; builds the FrankenPHP-compatible ZTS extension.
set -eu
WS_OWNER="$(stat -c '%u:%g' .)"
trap 'chown -R "$WS_OWNER" .' EXIT
apt-get update
apt-get install -y --no-install-recommends build-essential cmake perl golang-go git curl ca-certificates clang libclang-dev python3
curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.98.0
. "$HOME/.cargo/env"
cargo build --release --locked
php -d extension="$PWD/target/release/libwreq_php.so" -r 'exit(PHP_ZTS && class_exists("Wreq\\Ext\\Client") ? 0 : 1);'
python3 tests/http2_settings_wire.py target/release/libwreq_php.so
