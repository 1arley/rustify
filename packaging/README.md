# Packaging

Templates and automation for distributing Rustify through package managers.
Release binaries on GitHub are always published first; package jobs run
afterwards and never block them.

| Channel | Package | Template | Published by |
| --- | --- | --- | --- |
| AUR (Arch/Omarchy) | `rustify-bin` | `aur/PKGBUILD.tmpl` | `publish-aur` job (SSH push) |
| Homebrew cask | `1arley/tap/rustify` | `homebrew/Casks/rustify.rb.tmpl` | `publish-homebrew` job (push to tap) |
| Winget | `1arley.Rustify` | `winget/*.tmpl` | `publish-winget` job (PR to `microsoft/winget-pkgs`) |

All jobs live in `.github/workflows/release.yml` and run on `v*` tags.
`scripts/render_packaging.py` fills the templates with the release version and
the SHA-256 values from `SHA256SUMS.txt` (also attached to each release).

## Name decisions

Checked on 2026-11-23 with `scripts/check_package_names.py`:

- AUR: `rustify`, `rustify-bin` and `rustify-player-bin` were free. The `-bin`
  suffix follows the AUR convention for prebuilt binaries; the package
  `provides`/`conflicts` with `rustify`.
- Homebrew: `rustify` is free in `homebrew-cask` and `homebrew-core`. It is
  published from our own tap `1arley/homebrew-tap`.
- Winget: `1arley.Rustify` is free in `microsoft/winget-pkgs`.

Fallbacks, only if the preferred name is taken when publishing: AUR
`rustify-player-bin`, Homebrew cask `rustify-player`, Winget
`1arley.RustifyPlayer`. Pass them to `render_packaging.py` via
`--aur-pkgname`, `--homebrew-token` and `--winget-id`, and record the
decision here.

## Secrets and variables

Each publish job skips with a notice when its secret is missing.

| Name | Used by | Minimum permission |
| --- | --- | --- |
| `AUR_SSH_KEY` | `publish-aur` | SSH private key registered on the AUR account that maintains the package |
| `HOMEBREW_TAP_TOKEN` | `publish-homebrew` | Fine-grained token, `contents: write` on `1arley/homebrew-tap` only |
| `WINGET_GITHUB_TOKEN` | `publish-winget` | Classic token with `public_repo` (or fine-grained on the fork) to push a branch and open a PR |
| `AUR_PKGNAME` (variable, optional) | `publish-aur` | Overrides `rustify-bin` |
| `HOMEBREW_TAP_REPO` (variable, optional) | `publish-homebrew` | Overrides `1arley/homebrew-tap` |

The AUR package must be created once by pushing the first `PKGBUILD` and
`.SRCINFO` manually or with the job; the Winget package needs a fork of
`microsoft/winget-pkgs` owned by the token's user.

## Dry run (publishes nothing)

```sh
python3 scripts/check_package_names.py
python3 scripts/render_packaging.py --version v9.9.9 --out-dir /tmp/rustify-pkg \
  --linux-sha256   1111111111111111111111111111111111111111111111111111111111111111 \
  --macos-sha256   2222222222222222222222222222222222222222222222222222222222222222 \
  --windows-sha256 3333333333333333333333333333333333333333333333333333333333333333 \
  --generate-srcinfo   # needs makepkg (Arch)
```

The `package-manifests` CI job uploads the same output as a workflow artifact
so it can be inspected before the first real publish.

## Release checklist

1. `Version` workflow tags `vX.Y.Z`; `Release` builds the three archives.
2. `SHA256SUMS.txt` is attached to the GitHub release.
3. `package-manifests` renders PKGBUILD, cask and Winget manifests; check the
   name-collision output in its log.
4. `publish-aur`, `publish-homebrew` and `publish-winget` push or open PRs.
   A failed job can be re-run on its own.
5. Confirm `yay -S rustify-bin`, `brew install --cask 1arley/tap/rustify`
   and `winget install 1arley.Rustify` pick up the new version.
