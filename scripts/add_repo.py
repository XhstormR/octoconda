#!/usr/bin/env python3
"""Download a web page and extract all GitHub repository URLs from it."""

import argparse
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed

import requests
import tomllib
from bs4 import BeautifulSoup

# Matches github.com/<owner>/<repo> but not deeper paths like
# /owner/repo/issues or /owner/repo/blob/...
GITHUB_REPO_RE = re.compile(
    r"https?://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+?)(?:\.git)?/?$"
)

GITHUB_SPECIAL_PATHS = {
    "about", "enterprise", "features", "login", "join", "pricing",
    "security", "customer-stories", "readme", "explore", "topics",
    "trending", "collections", "events", "sponsors", "settings",
    "marketplace", "notifications", "issues", "pulls", "codespaces",
    "discussions", "organizations", "orgs", "new", "apps", "site",
}


def is_repo_url(url: str) -> bool:
    m = GITHUB_REPO_RE.match(url)
    if not m:
        return False
    owner = m.group(1)
    if owner.lower() in GITHUB_SPECIAL_PATHS:
        return False
    return True


def normalize(url: str) -> str:
    """Normalize a GitHub repo URL to a canonical form."""
    url = url.rstrip("/")
    if url.endswith(".git"):
        url = url[:-4]
    if url.startswith("http://"):
        url = "https://" + url[7:]
    return url


def repo_slug(url: str) -> str:
    """Extract 'owner/repo' from a normalized GitHub URL."""
    m = GITHUB_REPO_RE.match(url)
    return f"{m.group(1)}/{m.group(2)}" if m else ""


def load_known_repos(config_path: str) -> tuple[set[str], set[str]]:
    """Load repository slugs and package names from config.toml.

    Returns (known_slugs, known_names) where names are the effective
    package names (explicit name or the repo part of the slug).
    """
    try:
        with open(config_path, "rb") as f:
            config = tomllib.load(f)
    except FileNotFoundError:
        print(f"Warning: config file '{config_path}' not found, not filtering.", file=sys.stderr)
        return set(), set()

    slugs = set()
    names = set()
    for pkg in config.get("packages", []):
        repo = pkg.get("repository", "")
        if repo:
            slugs.add(repo.lower())
            # Effective name: explicit name, or the repo part of the slug
            name = pkg.get("name") or repo.split("/", 1)[-1]
            names.add(name.lower())
    return slugs, names


# Patterns that indicate a source-only archive (not a binary release)
SOURCE_ARCHIVE_RE = re.compile(
    r"^("
    # Common source archive naming: <name>-<version>-source.tar.gz, etc.
    r".*[-_.]source[-_.].*"
    r"|.*[-_.]src[-_.].*"
    r"|.*[-_.]sources[-_.].*"
    # Bare version-only archives: v1.2.3.tar.gz, project-1.2.3.zip
    r"|[a-zA-Z0-9_.-]*\d+\.\d+[a-zA-Z0-9_.+-]*\.(tar\.gz|tar\.bz2|tar\.xz|zip|tgz)"
    r")$",
    re.IGNORECASE,
)

# Keywords in asset names that suggest a binary/platform-specific build
BINARY_HINTS_RE = re.compile(
    r"(linux|darwin|macos|mac|windows|win|amd64|x86_64|x86-64|arm64|aarch64"
    r"|i686|i386|armv[67]|mips|ppc|s390|riscv|musl|gnu"
    r"|\.deb|\.rpm|\.apk|\.msi|\.exe|\.dmg|\.pkg|\.AppImage|\.snap|\.flatpak"
    r"|\.wasm|\.so|\.dylib|\.dll)",
    re.IGNORECASE,
)


def has_binary_release(slug: str, session: requests.Session) -> tuple[bool, str]:
    """Check if a repo's latest release contains binary assets.

    Returns (passed, reason) where reason explains why it was skipped.
    """
    url = f"https://api.github.com/repos/{slug}/releases/latest"
    try:
        resp = session.get(url, timeout=15)
    except requests.RequestException as e:
        return False, f"request failed: {e}"
    if resp.status_code == 404:
        return False, "no releases or inaccessible"
    if resp.status_code == 403:
        remaining = resp.headers.get("x-ratelimit-remaining", "?")
        if remaining == "0":
            return False, "GitHub API rate limit exceeded - set GITHUB_TOKEN or GH_TOKEN"
        return False, "access forbidden"
    if resp.status_code != 200:
        return False, f"API error {resp.status_code}"

    release = resp.json()
    assets = release.get("assets", [])
    if not assets:
        return False, "latest release has no assets"

    for asset in assets:
        name = asset.get("name", "")
        if BINARY_HINTS_RE.search(name) and not SOURCE_ARCHIVE_RE.match(name):
            return True, ""

    asset_names = ", ".join(a.get("name", "?") for a in assets)
    return False, f"no binary assets in latest release ({asset_names})"


def add_repos_to_config(config_path: str, new_slugs: list[str],
                        known_names: set[str]) -> None:
    """Add new repos into config.toml, keeping [[packages]] sorted case-insensitively.

    If a new repo's default package name (the part after '/') collides with
    an existing name, an explicit name = "owner__repo" is added.
    """
    with open(config_path, "r") as f:
        content = f.read()

    # Split the file into a header (everything before the first [[packages]])
    # and individual package blocks.
    first_pkg = content.find("[[packages]]")
    if first_pkg == -1:
        header = content.rstrip("\n") + "\n"
        blocks = []
    else:
        header = content[:first_pkg]
        rest = content[first_pkg:]
        # Split on [[packages]] boundaries, keeping each block together
        raw_blocks = re.split(r"(?=^\[\[packages\]\])", rest, flags=re.MULTILINE)
        blocks = [b for b in raw_blocks if b.strip()]

    # Parse each block to extract its repository slug (for sorting)
    def block_repo(block: str) -> str:
        m = re.search(r'^repository\s*=\s*"([^"]+)"', block, re.MULTILINE)
        return m.group(1) if m else ""

    existing_slugs = {block_repo(b).lower() for b in blocks}

    # Track names as we add, to catch collisions between new repos too
    all_names = set(known_names)

    # Create new blocks for repos not already present
    for slug in new_slugs:
        if slug.lower() in existing_slugs:
            continue
        owner, repo = slug.split("/", 1)
        default_name = repo.lower()
        if default_name in all_names:
            # Name collision: use "owner__repo" as explicit name
            explicit_name = f"{owner}__{repo}"
            print(f"  name collision for '{repo}', using name = \"{explicit_name}\"",
                  file=sys.stderr)
            block = f'[[packages]]\nrepository = "{slug}"\nname = "{explicit_name}"\n'
            all_names.add(explicit_name.lower())
        else:
            block = f'[[packages]]\nrepository = "{slug}"\n'
            all_names.add(default_name)
        blocks.append(block)

    # Sort all blocks case-insensitively by repository
    blocks.sort(key=lambda b: block_repo(b).lower())

    with open(config_path, "w") as f:
        f.write(header)
        f.write("\n".join(blocks))

    print(f"Added {len(new_slugs)} repo(s) to {config_path}", file=sys.stderr)


def make_github_session() -> requests.Session:
    session = requests.Session()
    gh_token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if not gh_token:
        try:
            result = subprocess.run(
                ["gh", "auth", "token"], capture_output=True, text=True, timeout=5,
            )
            if result.returncode == 0 and result.stdout.strip():
                gh_token = result.stdout.strip()
        except (FileNotFoundError, subprocess.TimeoutExpired):
            pass
    if gh_token:
        session.headers["Authorization"] = f"token {gh_token}"
    else:
        print(
            "Warning: No GitHub token found. API rate limit is 60 requests/hour.\n"
            "  Set GITHUB_TOKEN/GH_TOKEN or log in with: gh auth login",
            file=sys.stderr,
        )
    session.headers["Accept"] = "application/vnd.github+json"
    return session


def main():
    parser = argparse.ArgumentParser(
        description="Download a web page and extract GitHub repository URLs."
    )
    parser.add_argument("url", help="URL of the page to scan")
    parser.add_argument(
        "-c", "--config",
        default="./config.toml",
        help="Path to config.toml for filtering known repos (default: ./config.toml)",
    )
    parser.add_argument(
        "-n", "--dry-run",
        action="store_true",
        help="Only print discovered repos, don't modify config.toml",
    )
    args = parser.parse_args()

    known, known_names = load_known_repos(args.config)

    input_url = normalize(args.url)

    if is_repo_url(input_url):
        # Input is a GitHub repo URL itself, use it directly
        repos = [input_url]
    else:
        # Download and parse the page for GitHub repo URLs
        resp = requests.get(
            args.url,
            headers={
                "User-Agent": "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            },
            timeout=30,
        )
        resp.raise_for_status()

        soup = BeautifulSoup(resp.text, "html.parser")

        # Extract links from <a href="...">
        href_urls = {a["href"] for a in soup.find_all("a", href=True)}

        # Also scan raw HTML for GitHub URLs not in href attributes
        raw_urls = set(re.findall(r"https?://github\.com/[A-Za-z0-9_./-]+", resp.text))

        candidates = href_urls | raw_urls
        repos = sorted({normalize(u) for u in candidates if is_repo_url(u)})

    # Filter out repos already in config.toml
    repos = [r for r in repos if repo_slug(r).lower() not in known]

    if not repos:
        print("No new GitHub repository URLs found.", file=sys.stderr)
        return

    # Check GitHub API for releases, using a shared session for connection reuse
    session = make_github_session()

    print(f"Checking {len(repos)} repos for releases...", file=sys.stderr)

    with_releases = []
    with ThreadPoolExecutor(max_workers=10) as pool:
        futures = {pool.submit(has_binary_release, repo_slug(r), session): r for r in repos}
        for future in as_completed(futures):
            url = futures[future]
            passed, reason = future.result()
            if passed:
                with_releases.append(url)
            else:
                print(f"  skipped ({reason}): {url}", file=sys.stderr)

    with_releases.sort()

    if not with_releases:
        print("No new repos with releases found.", file=sys.stderr)
        return

    seen = set()
    new_slugs = []
    for r in with_releases:
        slug = repo_slug(r)
        if slug.lower() not in seen:
            seen.add(slug.lower())
            new_slugs.append(slug)

    for slug in new_slugs:
        print(slug)

    if not args.dry_run:
        add_repos_to_config(args.config, new_slugs, known_names)
    else:
        print("(dry run, config.toml not modified)", file=sys.stderr)


if __name__ == "__main__":
    main()
