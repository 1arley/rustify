import os
import re
import subprocess
import sys


def run(cmd):
    return subprocess.check_output(cmd, text=True).strip()


def main():
    github_output = os.environ.get("GITHUB_OUTPUT")

    cargo_path = "Cargo.toml"
    cargo = open(cargo_path).read()
    match = re.search(r'^version\s*=\s*"([^"]+)"', cargo, re.MULTILINE)
    if not match:
        raise SystemExit("Could not find version in Cargo.toml")
    current = match.group(1)

    # Last tag, if any.
    try:
        last_tag = run(["git", "describe", "--tags", "--abbrev=0"])
    except subprocess.CalledProcessError:
        last_tag = None

    if last_tag:
        log = run(["git", "log", f"{last_tag}..HEAD", "--format=%B%x1f", "--no-merges"])
    else:
        log = run(["git", "log", "--format=%B%x1f", "--no-merges"])

    messages = [m for m in log.split("\x1f") if m.strip()]

    has_major = False
    has_minor = False
    has_patch = False

    for message in messages:
        lines = message.strip().splitlines()
        subject = lines[0] if lines else ""
        body = "\n".join(lines[1:])

        breaking = "BREAKING CHANGE" in body or "BREAKING-CHANGE" in body
        if re.match(r"^\w+(\([^)]+\))?!:", subject):
            breaking = True

        if breaking:
            has_major = True
            continue

        if re.match(r"^feat(\([^)]+\))?:", subject):
            has_minor = True
        elif re.match(r"^(fix|perf)(\([^)]+\))?:", subject):
            has_patch = True

    if has_major:
        bump = "major"
    elif has_minor:
        bump = "minor"
    elif has_patch:
        bump = "patch"
    else:
        bump = "none"

    if bump == "none":
        write_output(github_output, {"bump": "none", "version": current})
        print("No releasable commits found; skipping release.")
        return

    major, minor, patch = (int(x) for x in current.split("."))
    if bump == "major":
        major += 1
        minor = 0
        patch = 0
    elif bump == "minor":
        minor += 1
        patch = 0
    else:
        patch += 1

    new_version = f"{major}.{minor}.{patch}"

    # Update Cargo.toml.
    new_cargo = re.sub(
        r'^version\s*=\s*"[^"]+"',
        f'version = "{new_version}"',
        cargo,
        count=1,
        flags=re.MULTILINE,
    )
    open(cargo_path, "w").write(new_cargo)

    # Refresh only this crate's entry in the lockfile.
    subprocess.run(["cargo", "update", "-p", "rustify-player"], check=True)

    write_output(github_output, {"bump": bump, "version": new_version})
    print(f"Bumped {current} -> {new_version} ({bump})")


def write_output(path, values):
    if not path:
        return
    with open(path, "a") as f:
        for key, value in values.items():
            f.write(f"{key}={value}\n")


if __name__ == "__main__":
    main()