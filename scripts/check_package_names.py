#!/usr/bin/env python3
"""Non-destructive check that Rustify's preferred package names are free.

Stdlib only. Read-only requests. Exits 0 by default so it never blocks a
release; pass --strict to exit 1 when a preferred name is already taken.
"""

import argparse
import json
import sys
import urllib.error
import urllib.request

AUR_NAMES = ["rustify-bin", "rustify", "rustify-player-bin"]
PREFERRED_AUR = "rustify-bin"
HOMEBREW_CASK = "https://raw.githubusercontent.com/Homebrew/homebrew-cask/HEAD/Casks/r/rustify.rb"
HOMEBREW_CORE = "https://raw.githubusercontent.com/Homebrew/homebrew-core/HEAD/Formula/r/rustify.rb"
WINGET = "https://raw.githubusercontent.com/microsoft/winget-pkgs/master/manifests/1/1arley/Rustify"
USER_AGENT = "rustify-package-name-check"


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, b""
    except Exception:
        return None, b""


def aur_status():
    query = "&".join(f"arg[]={name}" for name in AUR_NAMES)
    status, body = fetch(f"https://aur.archlinux.org/rpc/v5/info?{query}")
    if status != 200:
        return {name: "unknown" for name in AUR_NAMES}
    try:
        taken = {item["Name"] for item in json.loads(body).get("results", [])}
    except ValueError:
        return {name: "unknown" for name in AUR_NAMES}
    return {name: "taken" if name in taken else "available" for name in AUR_NAMES}


def url_status(url):
    status, _ = fetch(url)
    if status == 404:
        return "available"
    if status == 200:
        return "taken"
    return "unknown"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    rows = []
    for name, state in aur_status().items():
        rows.append((f"AUR {name}", state, name == PREFERRED_AUR))
    rows.append(("Homebrew cask rustify (official)", url_status(HOMEBREW_CASK), False))
    rows.append(("Homebrew formula rustify (core)", url_status(HOMEBREW_CORE), False))
    rows.append(("Winget 1arley.Rustify", url_status(WINGET), True))

    width = max(len(label) for label, _, _ in rows)
    for label, state, _ in rows:
        print(f"{label.ljust(width)}  {state}")

    blocked = [label for label, state, preferred in rows if preferred and state == "taken"]
    if blocked:
        print(f"preferred name taken: {', '.join(blocked)}", file=sys.stderr)
        print("use the fallback names documented in packaging/README.md", file=sys.stderr)
        if args.strict:
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
