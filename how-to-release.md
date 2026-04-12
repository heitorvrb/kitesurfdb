# How to release a .deb

## Prerequisites (one-time)

```bash
sudo apt-get install debhelper
```

`cargo`, `rustc`, and `dx` (dioxus-cli) must already be installed.

## Bump the version

Update the version in both places:

1. `crates/app/Cargo.toml` — change the `version` field
2. `debian/changelog` — add a new entry at the top:

```
kitesurfdb (X.Y.Z) trixie; urgency=low

  * What changed.

 -- Heitor Rodrigues <heitor.vrb@gmail.com>  Thu, 01 Jan 2026 00:00:00 +0000
```

The date must be RFC 2822 format. Generate it with `date -R`.

## Build

```bash
dpkg-buildpackage -b -us -uc --buildinfo-file=kitesurfdb.buildinfo --changes-file=kitesurfdb.changes --buildinfo-option=-u. --changes-option=-u.
```

This runs `dx bundle --release` under the hood, strips the binary, and
auto-detects runtime library dependencies.

## Verify

```bash
dpkg-deb -I kitesurfdb_*.deb   # check metadata and dependencies
dpkg-deb -c kitesurfdb_*.deb   # list installed files
```

## Install locally

```bash
sudo dpkg -i kitesurfdb_*.deb
```
