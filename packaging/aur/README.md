# AUR Packaging

This directory is the source of truth for the
[`panache-bin`](https://aur.archlinux.org/packages/panache-bin) AUR package. It
replaces the old `jolars/panache-aur` mirror repo: the AUR git remote
(`ssh://aur@aur.archlinux.org/panache-bin.git`) is now a pure deploy target.

## How It Publishes

The `publish-aur.yml` workflow runs automatically after each release's binary
assets are uploaded (chained in `build-and-test.yml`). It rewrites `pkgver`,
`pkgrel`, and the checksums in this `PKGBUILD`, then pushes `PKGBUILD` +
`.SRCINFO` to the AUR via
[KSXGitHub/github-actions-deploy-aur](https://github.com/KSXGitHub/github-actions-deploy-aur),
which also test-builds the package first.

Only the primary CLI release stream (`v*` tags) publishes; the other tag streams
the monorepo produces (`panache-parser-v*`, `panache-formatter-v*`,
`panache-code-v*`, `panache-zed-v*`) carry no binaries and are skipped.

The `pkgver` and checksums committed here are a snapshot of the last release at
the time this file was touched; the workflow always overwrites them at publish
time. They are kept real (not placeholders) so `makepkg -si` from a checkout
works.

Requirements (one-time):

- An AUR account with an SSH public key registered.
- The matching private key stored as the `AUR_SSH_PRIVATE_KEY` repo secret.

## Manual Fallback

If CI is unavailable, `task aur:push` (see `scripts/aur_push.sh`) does the same
update locally. It needs `makepkg` on the `PATH` (the `pacman` package on
non-Arch distros) for `.SRCINFO` generation, plus AUR SSH access.

Re-releasing the same version (e.g. a repackaging fix) requires bumping
`pkgrel`: run the workflow manually via `workflow_dispatch` with a `pkgrel`
input, or `task aur:push -- <version> <pkgrel>`.

## Related Packages

The source-built [`panache`](https://aur.archlinux.org/packages/panache) AUR
package is not maintained from here.
