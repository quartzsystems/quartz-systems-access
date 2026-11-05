# Releasing

`VERSION`, at the root, holds the version as MAJOR.MINOR.PATCH, and
everything else follows it:

- the workspace version in `Cargo.toml`, and so every crate's
  `CARGO_PKG_VERSION` (QAP's Hello sends it as the client version);
- the release, VxRyMz (major, minor, patch): `qsaccess::VERSION`, shown in
  the About box and the status bar, and `qsa_install::RELEASE`, in the
  installer's pages, its manifest and its file name,
  `QS400-Access-VxRyMz-<os>-x86_64[.exe]`;
- the Windows version resources, `x.y.z (VxRyMz)` and `x.y.z.0`, and
  `DisplayVersion` in Apps & features;
- the GitHub release and its notes.

Cargo cannot read a version from a file, so `Cargo.toml` keeps its own copy.
`ci/version.sh check`, run in CI, and the `version_follows_the_version_file`
test fail when the two differ. Change them together with `ci/version.sh
prepare`.

## Changelog

`CHANGELOG.md` has one section per release, newest first. Add each change to
QS/400 Access for Desktop, or to the public API of a crate other projects pin,
under `## Unreleased` as you make it. The release's section becomes its
GitHub release's notes.

## Cutting a release

On Windows, run `ci\version.ps1` in place of `ci/version.sh`, with the same
arguments. It runs the script with Git for Windows' bash, which it finds next
to `git.exe`; plain `bash` in PowerShell is usually WSL's. From a Git Bash
window, `ci/version.sh` works as it is.

1. Prepare it: writes the version to `VERSION`, `Cargo.toml` and
   `Cargo.lock`, and turns `## Unreleased` into `## X.Y.Z (date)` with a new
   empty Unreleased section above it.

   ```sh
   ci/version.sh prepare 1.1.0      # Git Bash
   ci\version.ps1 prepare 1.1.0     # PowerShell
   ```

2. Bless the screenshot goldens again: the installer's pages show the
   release, so the old goldens no longer match. Commit the
   prepared version on a branch and bless it as TESTING.md's "Screenshot
   goldens" says, then commit the goldens with the release.

3. Read over the section (`ci/version.sh notes` prints it), then commit and
   tag, one command at a time (Windows PowerShell 5.1 has no `&&`):

   ```sh
   git commit -am "chore: release 1.1.0 (V1R1M0)"
   git tag v1.1.0
   git push origin main v1.1.0
   ```

Pushing the tag runs `.github/workflows/release.yml`:

1. **version**: the tag must be `v` followed by `VERSION`, and `CHANGELOG.md`
   must have a non-empty section for it.
2. **ci**: every job in `ci.yml` (checks, tests and screenshots on Windows and
   Linux, and the installers built and tested end to end).
3. **publish**: a GitHub release named `QS/400 Access for Desktop VxRyMz
   (x.y.z)`, with the two installers, `SHA256SUMS` and the changelog section
   as its notes.

A failed run publishes nothing. Fix the problem on main, then move the tag,
or, if the release was published, prepare the next patch version:

```sh
git tag -f v1.1.0
git push -f origin v1.1.0
```

The first release, 1.0.0, is already in `VERSION`: run `ci/version.sh prepare
1.0.0` only to date its changelog section.
