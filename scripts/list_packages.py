#!/usr/bin/env python3
"""List all packages in a prefix.dev channel, sorted oldest to youngest."""

import argparse
import json
import os
import sys
import urllib.request
from datetime import datetime, timezone

try:
    import tomllib
except ImportError:
    import tomli as tomllib

PLATFORMS = [
    "noarch",
    "linux-32",
    "linux-64",
    "linux-aarch64",
    "osx-64",
    "osx-arm64",
    "win-32",
    "win-64",
    "win-arm64",
]


def load_channel(config_path):
    with open(config_path, "rb") as f:
        config = tomllib.load(f)
    return config["conda"]["channel"]


def fetch_repodata(channel_url, platform):
    url = f"{channel_url}/{platform}/repodata.json"
    req = urllib.request.Request(url, headers={"User-Agent": "octoconda/1.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError:
        return None


def collect_packages(repodata, platform):
    packages = []
    for section in ("packages", "packages.conda"):
        for filename, info in repodata.get(section, {}).items():
            packages.append({
                "name": info.get("name", ""),
                "version": info.get("version", ""),
                "build_number": info.get("build_number", 0),
                "platform": platform,
                "timestamp": info.get("timestamp", 0),
                "filename": filename,
            })
    return packages


def format_timestamp(ts):
    if ts:
        dt = datetime.fromtimestamp(ts / 1000, tz=timezone.utc)
        return dt.strftime("%Y-%m-%d %H:%M:%S")
    return "unknown            "


def delete_package(channel, platform, filename, api_key):
    url = f"https://prefix.dev/api/v1/delete/{channel}/{platform}/{filename}"
    req = urllib.request.Request(url, method="DELETE", headers={
        "User-Agent": "octoconda/1.0",
        "Authorization": f"Bearer {api_key}",
    })
    with urllib.request.urlopen(req) as resp:
        return resp.status


def main():
    parser = argparse.ArgumentParser(description="List or delete packages in a prefix.dev channel")
    parser.add_argument("config", nargs="?", default="config.toml", help="Path to config.toml (default: ./config.toml)")
    parser.add_argument("--delete-oldest", type=int, metavar="N",
                        help="Delete the N oldest packages")
    parser.add_argument("--delete-name", metavar="NAME",
                        help="Delete all packages with this name")
    parser.add_argument("--delete-file", nargs="+", metavar="PLATFORM/FILENAME",
                        help="Delete specific files (e.g. linux-64/package-1.0-h1234_0.conda)")
    args = parser.parse_args()

    delete_opts = [args.delete_oldest is not None, args.delete_name is not None, args.delete_file is not None]
    if sum(delete_opts) > 1:
        parser.error("--delete-oldest, --delete-name, and --delete-file are mutually exclusive")

    channel = load_channel(args.config)
    channel_url = f"https://prefix.dev/{channel}"
    print(f"Channel: {channel_url}", file=sys.stderr)

    if args.delete_file is not None:
        api_key = os.environ.get("PREFIX_DEV_API_KEY")
        if not api_key:
            print("Error: PREFIX_DEV_API_KEY environment variable is required for deletion", file=sys.stderr)
            sys.exit(1)
        to_delete = []
        for path in args.delete_file:
            platform, filename = path.split("/", 1)
            to_delete.append({"platform": platform, "filename": filename})
        print(f"\nDeleting {len(to_delete)} packages:", file=sys.stderr)
        total = len(to_delete)
        failed = 0
        for i, pkg in enumerate(to_delete, 1):
            print(f"  [{i}/{total}] {pkg['platform']}/{pkg['filename']} ... ", end="", file=sys.stderr)
            try:
                delete_package(channel, pkg["platform"], pkg["filename"], api_key)
                print("deleted", file=sys.stderr)
            except urllib.error.HTTPError as e:
                print(f"failed: {e.code} {e.reason}", file=sys.stderr)
                failed += 1
        succeeded = total - failed
        print(f"\n{succeeded}/{total} packages deleted", file=sys.stderr)
        if failed:
            print(f"{failed}/{total} packages failed to delete", file=sys.stderr)
        return

    packages = []
    for platform in PLATFORMS:
        print(f"  Fetching {platform}...", file=sys.stderr)
        repodata = fetch_repodata(channel_url, platform)
        if repodata is None:
            continue
        packages.extend(collect_packages(repodata, platform))

    packages.sort(key=lambda p: p["timestamp"])

    if args.delete_oldest is None and args.delete_name is None:
        for pkg in packages:
            date_str = format_timestamp(pkg["timestamp"])
            print(f"{date_str}  {pkg['platform']:20s}  {pkg['platform']}/{pkg['filename']}")
        unique_names = {pkg["name"] for pkg in packages}
        print(f"\n{len(packages)} packages, {len(unique_names)} unique package names", file=sys.stderr)
        return

    api_key = os.environ.get("PREFIX_DEV_API_KEY")
    if not api_key:
        print("Error: PREFIX_DEV_API_KEY environment variable is required for deletion", file=sys.stderr)
        sys.exit(1)

    if args.delete_name is not None:
        to_delete = [p for p in packages if p["name"] == args.delete_name]
        if not to_delete:
            print(f"No packages found with name '{args.delete_name}'", file=sys.stderr)
            sys.exit(1)
    else:
        to_delete = packages[:args.delete_oldest]
    print(f"\nDeleting {len(to_delete)} packages:", file=sys.stderr)

    total = len(to_delete)
    failed = 0
    for i, pkg in enumerate(to_delete, 1):
        date_str = format_timestamp(pkg["timestamp"])
        print(f"  [{i}/{total}] {date_str}  {pkg['platform']:20s}  {pkg['platform']}/{pkg['filename']} ... ", end="", file=sys.stderr)
        try:
            delete_package(channel, pkg["platform"], pkg["filename"], api_key)
            print("deleted", file=sys.stderr)
        except urllib.error.HTTPError as e:
            print(f"failed: {e.code} {e.reason}", file=sys.stderr)
            failed += 1

    succeeded = total - failed
    print(f"\n{succeeded}/{total} packages deleted", file=sys.stderr)
    if failed:
        print(f"{failed}/{total} packages failed to delete", file=sys.stderr)


if __name__ == "__main__":
    main()
