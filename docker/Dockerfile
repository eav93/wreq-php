# Builds the `wreq_php` extension inside an Alpine (musl) PHP image and bakes
# it into a ready-to-use runtime image.
#
# `PHP_VERSION` selects the PHP series (8.1 … 8.5). Stages:
#
#   runtime  (default) — php:<ver>-cli-alpine with wreq_php compiled in and
#                         enabled; use it as a base image for your project.
#   artifact           — `FROM scratch` holding just libwreq_php.so, for
#                         extracting the bare musl binary.
#   test               — builds Composer deps and runs the unit suite on musl.
#
# Ready-to-use image:
#   docker build -f docker/Dockerfile --build-arg PHP_VERSION=8.3 -t wreq-php:8.3 .
#
# Extract just the binary (to ./dist/libwreq_php.so):
#   docker build -f docker/Dockerfile --build-arg PHP_VERSION=8.3 \
#       --target artifact --output type=local,dest=dist .

ARG PHP_VERSION=8.3

# ---- build stage -----------------------------------------------------------
FROM php:${PHP_VERSION}-cli-alpine AS build

# Toolchain: build-base/cmake/perl/go build BoringSSL (via wreq's boring-sys2);
# clang + llvm provide libclang for bindgen. The PHP image already ships
# `php-config` and the Zend headers.
RUN apk add --no-cache \
        build-base cmake perl go git curl unzip \
        clang clang-dev llvm-dev linux-headers

# Install Rust via rustup — Alpine's packaged toolchain can lag behind the MSRV.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    LIBCLANG_PATH=/usr/lib
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /src
COPY . .
RUN cargo build --release \
    && mkdir -p /out \
    && cp target/release/libwreq_php.so /out/libwreq_php.so

# ---- test stage ------------------------------------------------------------
# Builds Composer dependencies and runs the unit suite against the musl build.
FROM build AS test
RUN curl -fsSL https://getcomposer.org/installer | php -- \
        --install-dir=/usr/local/bin --filename=composer \
    && composer install --no-interaction --no-progress --no-scripts \
    && php -d extension=/out/libwreq_php.so vendor/bin/phpunit \
        --no-coverage --exclude-group integration

# ---- artifact stage --------------------------------------------------------
# Contents are exactly the built extension, so `--output type=local` yields
# just the .so file.
FROM scratch AS artifact
COPY --from=build /out/libwreq_php.so /

# ---- runtime stage (default) ----------------------------------------------
# A clean PHP image with the extension installed and enabled — no build
# toolchain. Use it directly as the base image for a PHP project.
FROM php:${PHP_VERSION}-cli-alpine AS runtime
COPY --from=build /out/libwreq_php.so /tmp/wreq_php.so
RUN cp /tmp/wreq_php.so "$(php-config --extension-dir)/wreq_php.so" \
    && rm /tmp/wreq_php.so \
    && docker-php-ext-enable wreq_php \
    && php -m | grep -qx wreq_php \
    && echo "wreq_php extension enabled"
