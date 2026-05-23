#!/bin/sh
# Compile (and optionally test) the wreq_php extension inside Alpine/musl.
#
# Invoked from the host GH runner via:
#
#   docker run --rm \
#       -v "$GITHUB_WORKSPACE:/workspace" \
#       -w /workspace \
#       php:${PHP_VERSION}-cli-alpine \
#       sh scripts/ci/musl-build.sh [build|test]
#
# Keeping the GH runner on Ubuntu means JS-based actions (checkout, cache,
# upload-artifact) never enter the Alpine container — only this script does.
# The build artifact lands in target/release/libwreq_php.so inside the bind-
# mounted workspace; subsequent host steps stage and upload it normally.
#
# Modes:
#   build (default) — apk deps, rustup, cargo build, smoke load. Used for the
#                     release path where downstream host steps publish the .so.
#   test            — `build` plus composer install + phpunit unit suite. Used
#                     by CI to catch musl-specific regressions in the PHP layer.

set -eu

MODE="${1:-build}"
case "$MODE" in
    build|test) ;;
    *) echo "usage: $0 [build|test]" >&2; exit 2 ;;
esac

# build-base/cmake/perl/go build BoringSSL via wreq's boring-sys2; clang+llvm
# provide libclang for bindgen. The PHP image already ships php-config and the
# Zend headers.
apk add --no-cache \
    build-base cmake perl go git curl unzip \
    clang clang-dev llvm-dev linux-headers

# Alpine's packaged Rust can lag behind the MSRV — install via rustup.
curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
. "$HOME/.cargo/env"

export LIBCLANG_PATH=/usr/lib

# .cargo/config.toml disables crt-static for musl so a cdylib can be produced.
cargo build --release

EXT="$PWD/target/release/libwreq_php.so"

# Smoke load: catches musl/dlopen regressions before phpunit or upload.
php -d extension="$EXT" \
    -r 'exit(class_exists("Wreq\\Ext\\Client") ? 0 : 1);'

if [ "$MODE" = test ]; then
    curl -fsSL https://getcomposer.org/installer | php -- \
        --install-dir=/usr/local/bin --filename=composer
    composer install --no-interaction --no-progress --no-scripts
    php -d extension="$EXT" \
        vendor/bin/phpunit --no-coverage --exclude-group integration
fi

# Restore workspace ownership to the host runner — the container ran as root,
# but subsequent host steps (cache save, staging, upload-artifact) act as the
# unprivileged runner user.
WS_OWNER="$(stat -c '%u:%g' .)"
chown -R "$WS_OWNER" .
