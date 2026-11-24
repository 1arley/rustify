#!/usr/bin/env python3
"""Render AUR / Homebrew / Winget packaging files for a Rustify release.

Stdlib only. Never publishes anything; it only writes files under --out-dir.

Example (dry run):
  scripts/render_packaging.py --version v9.9.9 --out-dir /tmp/pkg \
    --linux-sha256 <64 hex> --macos-sha256 <64 hex> --windows-sha256 <64 hex>
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACKAGING = ROOT / "packaging"

ASSETS = {
    "linux": "rustify-linux-x86_64.tar.gz",
    "macos": "rustify-macos-aarch64.tar.gz",
    "windows": "rustify-windows-x86_64.zip",
}

SHA_RE = re.compile(r"^[0-9a-fA-F]{64}$")
PLACEHOLDER_RE = re.compile(r"@[A-Z0-9_]+@")


def fail(message):
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def parse_version(raw):
    version = raw.strip()
    if version.startswith(("v", "V")):
        version = version[1:]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        fail(f"version must be X.Y.Z (got {raw!r})")
    return version


def parse_checksums(path):
    found = {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) != 2:
            continue
        digest, name = parts[0], parts[1].lstrip("*")
        name = Path(name).name
        for key, asset in ASSETS.items():
            if name == asset:
                if not SHA_RE.match(digest):
                    fail(f"invalid sha256 for {name}")
                found[key] = digest.lower()
    return found


def render(template, values):
    text = template.read_text(encoding="utf-8")
    for key, value in values.items():
        text = text.replace(f"@{key}@", value)
    leftover = PLACEHOLDER_RE.findall(text)
    if leftover:
        fail(f"unreplaced placeholders in {template.name}: {sorted(set(leftover))}")
    return text


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {path}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--version", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--checksums-file")
    parser.add_argument("--linux-sha256")
    parser.add_argument("--macos-sha256")
    parser.add_argument("--windows-sha256")
    parser.add_argument("--aur-pkgname", default="rustify")
    parser.add_argument("--homebrew-token", default="rustify")
    parser.add_argument("--winget-id", default="1arley.Rustify")
    parser.add_argument(
        "--generate-srcinfo",
        action="store_true",
        help="run `makepkg --printsrcinfo` on the rendered PKGBUILD",
    )
    args = parser.parse_args()

    version = parse_version(args.version)
    out = Path(args.out_dir)

    sums = {}
    if args.checksums_file:
        sums.update(parse_checksums(args.checksums_file))
    for key in ASSETS:
        value = getattr(args, f"{key}_sha256")
        if value:
            if not SHA_RE.match(value):
                fail(f"--{key}-sha256 must be 64 hex characters")
            sums[key] = value.lower()
    if not sums:
        fail("no checksums provided (use --checksums-file or --*-sha256)")

    if not re.fullmatch(r"[a-z0-9][a-z0-9@._+-]*", args.aur_pkgname):
        fail("invalid --aur-pkgname")
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", args.homebrew_token):
        fail("invalid --homebrew-token")
    if not re.fullmatch(r"[A-Za-z0-9]+(\.[A-Za-z0-9]+)+", args.winget_id):
        fail("invalid --winget-id (expected Publisher.Package)")

    base = {"VERSION": version}

    if "linux" in sums:
        provides = (
            ""
            if args.aur_pkgname == "rustify"
            else "provides=('rustify')\nconflicts=('rustify')"
        )
        values = {
            **base,
            "AUR_PKGNAME": args.aur_pkgname,
            "AUR_PROVIDES_CONFLICTS": provides,
            "LINUX_SHA256": sums["linux"],
        }
        pkgbuild = out / "aur" / "PKGBUILD"
        write(pkgbuild, render(PACKAGING / "aur" / "PKGBUILD.tmpl", values))
        if args.generate_srcinfo:
            result = subprocess.run(
                ["makepkg", "--printsrcinfo"],
                cwd=pkgbuild.parent,
                capture_output=True,
                text=True,
            )
            if result.returncode != 0 or not result.stdout.strip():
                fail(f"makepkg --printsrcinfo failed: {result.stderr.strip()}")
            write(pkgbuild.parent / ".SRCINFO", result.stdout)
    else:
        print("skip: AUR (no linux sha256)")

    if "macos" in sums:
        values = {
            **base,
            "HOMEBREW_TOKEN": args.homebrew_token,
            "MACOS_SHA256": sums["macos"],
        }
        write(
            out / "homebrew" / "Casks" / f"{args.homebrew_token}.rb",
            render(PACKAGING / "homebrew" / "Casks" / "rustify.rb.tmpl", values),
        )
    else:
        print("skip: Homebrew (no macos sha256)")

    if "windows" in sums:
        publisher, package = args.winget_id.split(".", 1)
        values = {
            **base,
            "WINGET_ID": args.winget_id,
            "WINDOWS_SHA256_UPPER": sums["windows"].upper(),
        }
        target = (
            out
            / "winget"
            / "manifests"
            / publisher[0].lower()
            / publisher
            / package.replace(".", "/")
            / version
        )
        for suffix, template in (
            ("yaml", "1arley.Rustify.yaml.tmpl"),
            ("installer.yaml", "1arley.Rustify.installer.yaml.tmpl"),
            ("locale.en-US.yaml", "1arley.Rustify.locale.en-US.yaml.tmpl"),
        ):
            write(
                target / f"{args.winget_id}.{suffix}",
                render(PACKAGING / "winget" / template, values),
            )
    else:
        print("skip: Winget (no windows sha256)")


if __name__ == "__main__":
    main()
