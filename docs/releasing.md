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

The release name always follows `VERSION`: 1.1.1 is V1R1M1. Where the README,
these documents, a script's usage line or a doc comment must write a release
out (an installer's file name, a tag), it is written with the current
version, and `ci/version.sh prepare` rewrites those lines with the new one.
`ci/version.sh names`, run in CI, fails on a `VxRyMz` or `x.y.z` release name
that is not `VERSION`'s, in any file but the few the script lists (a system's
own release in test data, and QS/400's protocol document) and the past
sections of `CHANGELOG.md`. So the commands below always show the version in
`VERSION`; put the version you are releasing in its place.

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
   `Cargo.lock`, turns `## Unreleased` into `## X.Y.Z (date)` with a new
   empty Unreleased section above it, and rewrites the release names written
   out in the README, the documents and the scripts.

   ```sh
   ci/version.sh prepare 1.1.1      # Git Bash
   ci\version.ps1 prepare 1.1.1     # PowerShell
   ```

2. Bless the screenshot goldens again: the main window and the installer's
   pages show the release, so the old goldens no longer match. Commit the
   prepared version on a branch and bless it as TESTING.md's "Screenshot
   goldens" says, then commit the goldens with the release.

3. Read over the section (`ci/version.sh notes` prints it), then commit and
   tag, one command at a time (Windows PowerShell 5.1 has no `&&`):

   ```sh
   git commit -am "chore: release 1.1.1 (V1R1M1)"
   git tag v1.1.1
   git push origin main v1.1.1
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
git tag -f v1.1.1
git push -f origin v1.1.1
```
