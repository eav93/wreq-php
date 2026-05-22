# Publishing

**Audience: maintainers / release managers.** End users do not need this file —
they only follow the Installation section of `README.md`. This document records
how to cut a release of `wreq-php`: the Composer package, the prebuilt
extension binaries, and the Docker images.

## Versioning

The Composer package takes its version from the **git tag** — `composer.json`
deliberately has no `version` field. Use semantic versioning with a `v`
prefix: `v0.1.0`, `v0.2.0`, …

The `Installer` downloads prebuilt binaries from the GitHub Release whose tag
matches the installed package version, so the tag, the Release and the package
version are always the same thing.

## Cutting a release

1. Make sure `main` is green (CI passed) and the working tree is clean.

2. Tag and push:

   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

3. Pushing a `v*` tag triggers two workflows automatically:

   - **`release.yml`** — builds the extension for every PHP version × OS/libc ×
     arch, and attaches each binary (plus its `.sha256`) to the GitHub Release.
   - **`docker.yml`** — builds and pushes the ready-to-use images to
     `ghcr.io/eav93/wreq-php` for the whole PHP variant matrix.

4. Edit the GitHub Release notes if desired. The binaries are already attached.

## Publishing to Packagist (one-time setup)

So that `composer require eav93/wreq-php` works for everyone:

1. Sign in at <https://packagist.org> with the GitHub account.
2. **Submit** → paste `https://github.com/eav93/wreq-php` → Check → Submit.
3. Enable auto-updates so new tags publish themselves: on GitHub, the Packagist
   integration is added under **Settings → Integrations → Packagist**, or
   Packagist shows a one-click "GitHub Hook" / "Enable auto-update" button on
   the package page. After that, every pushed tag updates the package within
   seconds.

Until Packagist is set up, the package can still be used straight from the
repository — see "Installing without Packagist" below.

## Making the Docker images public

`docker.yml` pushes to the GitHub Container Registry. New GHCR packages are
**private** by default. Once: open
<https://github.com/users/eav93/packages/container/wreq-php/settings> →
**Change visibility** → **Public**. Then anyone can `docker pull` /
`FROM ghcr.io/eav93/wreq-php:...` without authenticating.

## How users install

With Packagist:

```bash
composer require eav93/wreq-php
```

Installing without Packagist — add the repository to the consumer's
`composer.json`:

```json
{
    "repositories": [
        { "type": "vcs", "url": "https://github.com/eav93/wreq-php" }
    ],
    "require": { "eav93/wreq-php": "^0.1" }
}
```

Either way, Composer's `post-install` hook fetches the matching prebuilt
extension binary; the user then enables it in `php.ini` (the installer prints
the exact line). Docker users skip all of this and just base their image on
`ghcr.io/eav93/wreq-php:<php>-<variant>`.

## Release checklist

- [ ] CI green on `main`.
- [ ] `CHANGELOG` / release notes updated.
- [ ] Tag pushed (`vX.Y.Z`); `release.yml` and `docker.yml` succeeded.
- [ ] Release assets present (one binary + `.sha256` per platform).
- [ ] `composer require eav93/wreq-php` resolves the new version.
