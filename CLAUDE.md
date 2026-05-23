# wreq-php — notes for AI assistants

PHP HTTP client: a Rust `ext-php-rs` extension over the `wreq` crate, plus a
Composer PHP layer. Core value — deterministic connection-pool reuse: one
`Wreq\Client` owns one `wreq::Client` and one pool.

## Layout

- `src/` — the Rust extension (`Wreq\Ext\*`).
- `src-php/` — the Composer PHP layer (`Wreq\*`), the Laravel-style API.
- `scripts/ci/musl-build.sh` — Alpine/musl build (and optionally test) script,
  invoked via `docker run` from the GH workflow so JS actions stay on Ubuntu.
- `.github/workflows/` — CI and release automation.

## Building

Needs a PHP install with development files (`php-config`). On macOS use
Homebrew PHP, not Herd: `PHP_CONFIG=$(command -v php-config) cargo build`.

## Commits and releases

Every commit to `main` automatically publishes a release — see
`.github/workflows/autorelease.yml`. Before committing, judge the scale of the
change and write the commit message accordingly:

- An ordinary change (bug fix, small tweak, refactor) — write a normal commit
  message; the patch version is bumped automatically, nothing extra needed.
- A noteworthy new feature or capability — the change deserves a minor-level
  version bump.
- A change that breaks backwards compatibility — it deserves a major-level
  version bump.

For a minor or major bump the commit message must carry the corresponding
marker. The exact marker strings are documented in the header comment of
`.github/workflows/autorelease.yml` — read them there and add the right one
**only** when the change genuinely warrants it. Default to a plain patch
release; reserve the higher bumps for changes that truly merit them.
