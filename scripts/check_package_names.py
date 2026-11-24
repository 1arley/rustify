#!/usr/bin/env python3
"""Non-destructive check of Rustify's package names on AUR, Homebrew and Winget.

Stdlib only, read-only requests. States per name:
  available  nobody uses it
  ours       already published from this project
  taken      used by something else
  unknown    could not be determined (network, rate limit)

Exits 0 by default so it never blocks a release; pass --strict to exit 1 when
a preferred name is taken or its availability is unknown. Set GITHUB_TOKEN or
GH_TOKEN to avoid GitHub API rate limits.
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

PROJECT_URL = "https://github.com/1arley/rustify"
FALLBACK_AUR = ["rustify", "rustify-player-bin"]
USER_AGENT = "rustify-package-name-check"


def fetch(url, github_api=False):
    headers = {"User-Agent": USER_AGENT}
    if github_api:
        headers["Accept"] = "application/vnd.github+json"
        token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
        if token:
            headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, b""
    except Exception:
        return None, b""


def aur_states(names):
    query = "&".join(f"arg[]={name}" for name in names)
    status, body = fetch(f"https://aur.archlinux.org/rpc/v5/info?{query}")
    if status != 200:
        return {name: "unknown" for name in names}
    try:
        results = {
            item["Name"]: (item.get("URL") or "").rstrip("/")
            for item in json.loads(body).get("results", [])
        }
    except ValueError:
        return {name: "unknown" for name in names}
    states = {}
    for name in names:
        if name not in results:
            states[name] = "available"
        elif results[name] == PROJECT_URL:
            states[name] = "ours"
        else:
            states[name] = "taken"
    return states


def raw_file_state(url):
    status, _ = fetch(url)
    return {404: "available", 200: "taken"}.get(status, "unknown")


def winget_state(winget_id):
    publisher, package = winget_id.split(".", 1)
    path = f"manifests/{publisher[0].lower()}/{publisher}/{package.replace('.', '/')}"
    status, _ = fetch(
        f"https://api.github.com/repos/microsoft/winget-pkgs/contents/{path}",
        github_api=True,
    )
    # The publisher folder is ours, so an existing package folder is our own package.
    return {404: "available", 200: "ours"}.get(status, "unknown")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--aur-pkgname", default="rustify")
    parser.add_argument("--homebrew-token", default="rustify")
    parser.add_argument("--winget-id", default="1arley.Rustify")
    args = parser.parse_args()

    token = args.homebrew_token
    first = token[0]
    rows = []
    aur_names = [args.aur_pkgname] + [n for n in FALLBACK_AUR if n != args.aur_pkgname]
    for name, state in aur_states(aur_names).items():
        rows.append((f"AUR {name}", state, name == args.aur_pkgname))
    rows.append(
        (
            f"Homebrew cask {token} (official)",
            raw_file_state(
                f"https://raw.githubusercontent.com/Homebrew/homebrew-cask/HEAD/Casks/{first}/{token}.rb"
            ),
            True,
        )
    )
    rows.append(
        (
            f"Homebrew formula {token} (core)",
            raw_file_state(
                f"https://raw.githubusercontent.com/Homebrew/homebrew-core/HEAD/Formula/{first}/{token}.rb"
            ),
            False,
        )
    )
    rows.append((f"Winget {args.winget_id}", winget_state(args.winget_id), True))

    width = max(len(label) for label, _, _ in rows)
    for label, state, _ in rows:
        print(f"{label.ljust(width)}  {state}")

    blocked = [
        label
        for label, state, preferred in rows
        if preferred and state in {"taken", "unknown"}
    ]
    if blocked:
        print(f"preferred name unavailable: {', '.join(blocked)}", file=sys.stderr)
        print("use the fallback names documented in packaging/README.md", file=sys.stderr)
        if args.strict:
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
