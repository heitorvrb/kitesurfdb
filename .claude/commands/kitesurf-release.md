Do a new release following these steps exactly. Do not skip steps. Do not add Co-Authored-By to commits.

## 1. Determine the new version

Read `crates/app/Cargo.toml` to find the current version. Bump the patch number by 1 (e.g. 0.1.7 → 0.1.8). Call it NEW_VERSION.

## 2. Collect changelog entries

Run `git log vPREV_VERSION..HEAD --oneline` (where PREV_VERSION is the current version before bumping). Each commit becomes one bullet point.

## 3. Update all three Cargo.toml files

Bump `version` to NEW_VERSION in:
- `crates/app/Cargo.toml`
- `crates/core/Cargo.toml`
- `crates/db/Cargo.toml`

## 4. Update debian/changelog

Add a new entry at the very top of `debian/changelog`. Get the current date with `date -R`. Format:

```
kitesurfdb (NEW_VERSION) trixie; urgency=low

  * <bullet per commit from step 2>

 -- Heitor Rodrigues <heitor.vrb@gmail.com>  <RFC 2822 date>
```

## 5. Update debian/com.heitorvrb.kitesurfdb.metainfo.xml

Add a new `<release>` entry at the top of the `<releases>` block. Use date in YYYY-MM-DD format. Follow the same structure as existing entries.

## 6. Build the package

```bash
dpkg-buildpackage -b -us -uc --buildinfo-file=kitesurfdb.buildinfo --changes-file=kitesurfdb.changes --buildinfo-option=-u. --changes-option=-u.
```

## 7. Verify the package

```bash
dpkg-deb -I kitesurfdb_NEW_VERSION_amd64.deb
dpkg-deb -c kitesurfdb_NEW_VERSION_amd64.deb
```

Confirm the version number and file list look correct before continuing.

## 8. Commit

Stage exactly these files — no others:
- `crates/app/Cargo.toml`
- `crates/core/Cargo.toml`
- `crates/db/Cargo.toml`
- `Cargo.lock`
- `debian/changelog`
- `debian/com.heitorvrb.kitesurfdb.metainfo.xml`

Commit message: `vNEW_VERSION` (nothing else, no Co-Authored-By).

## 9. Create GitHub release

```bash
gh release create vNEW_VERSION ./kitesurfdb_NEW_VERSION_amd64.deb --title "vNEW_VERSION" --notes "Release vNEW_VERSION"
```
