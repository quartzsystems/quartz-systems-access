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

The release name always follows `VERSION`: 1.2.0 is V1R2M0. Where the README,
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
   ci/version.sh prepare 1.2.0      # Git Bash
   ci\version.ps1 prepare 1.2.0     # PowerShell
   ```

2. Bless the screenshot goldens again: the main window and the installer's
   pages show the release, so the old goldens no longer match. Commit the
   prepared version on a branch and bless it as TESTING.md's "Screenshot
   goldens" says, then commit the goldens with the release.

3. Read over the section (`ci/version.sh notes` prints it), then commit and
   tag, one command at a time (Windows PowerShell 5.1 has no `&&`):

   ```sh
   git commit -am "chore: release 1.2.0 (V1R2M0)"
   git tag v1.2.0
   git push origin main v1.2.0
   ```

Pushing the tag runs `.github/workflows/release.yml`:

1. **version**: the tag must be `v` followed by `VERSION`, and `CHANGELOG.md`
   must have a non-empty section for it.
2. **ci**: every job in `ci.yml` (checks, tests and screenshots on Windows and
   Linux, and the installers built and tested end to end).
3. **publish**: the two installers get a GitHub artifact attestation (build
   provenance), then a GitHub release named `QS/400 Access for Desktop
   VxRyMz (x.y.z)` is made with the installers, `SHA256SUMS` and the
   changelog section as its notes. If the attestation fails, nothing is
   published.

A failed run publishes nothing. Fix the problem on main, then move the tag,
or, if the release was published, prepare the next patch version:

```sh
git tag -f v1.2.0
git push -f origin v1.2.0
```

## Checking a release

The installers are not code-signed, and nothing here changes that: Windows
still shows its SmartScreen warning for a downloaded installer (the README's
Installing section). What a release does carry is two ways to check a
download.

**The checksum** says the file is the one the release published, whole and
unchanged. `SHA256SUMS` is a release asset beside the installers:

```sh
sha256sum --check --ignore-missing SHA256SUMS      # Linux, Git Bash
```

```powershell
(Get-FileHash QS400-Access-V1R2M0-windows-x86_64.exe -Algorithm SHA256).Hash   # PowerShell
Get-Content SHA256SUMS
```

On Linux the command prints `OK` for each installer it finds. In PowerShell,
compare the hash printed with the installer's line in `SHA256SUMS`; capitals
and small letters do not matter. A checksum comes from the same place as the
file, so it shows damage or a swapped file on a mirror, not who built it.

**The attestation** says who built the file. The release workflow asks
GitHub to attest both installers: a statement, signed through Sigstore with a
certificate issued to that workflow run, that the file with this SHA-256 was
built by `.github/workflows/release.yml` of this repository from the tagged
commit. With the GitHub CLI:

```sh
gh attestation verify QS400-Access-V1R2M0-windows-x86_64.exe --repo quartzsystems/quartz-systems-access
gh attestation verify QS400-Access-V1R2M0-linux-x86_64 --repo quartzsystems/quartz-systems-access
```

It prints `Verification succeeded!` and the workflow and commit the file was
built from, and fails for a file that was changed after the build or built
anywhere else. It needs a network connection and `gh` 2.49 or later, signed
in to any GitHub account. The attestations are also listed on the
repository's **Attestations** page, under **Actions**.

Attestations are free for public repositories. They are made only by the
release workflow: an installer built by hand with `cargo xtask dist`, or by a
branch's CI run, has none.
