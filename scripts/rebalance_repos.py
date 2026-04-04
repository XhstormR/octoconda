#!/usr/bin/env python3
"""Rebalance the GitHub Actions workflow matrix so each runner handles
roughly the same number of packages from config.toml.

Starts with 1-char prefix buckets and merges them into runner groups.
If the result varies too much (>10% from target), retries with 2-char
prefixes, then 3, etc. This produces the simplest possible filters.
"""

import argparse
import random
import re
import string
import subprocess
import sys

import tomllib

WORKFLOW_PATH_DEFAULT = ".github/workflows/octoconda.yaml"
CONFIG_PATH_DEFAULT = "config.toml"
MAX_DEVIATION = 0.10  # 10%

MATRIX_START_RE = re.compile(r"^(\s*)include:\s*$", re.MULTILINE)


def load_repos(config_path: str) -> list[str]:
    """Load non-deprecated repository slugs, sorted case-insensitively."""
    with open(config_path, "rb") as f:
        config = tomllib.load(f)

    repos = []
    for pkg in config.get("packages", []):
        if pkg.get("deprecated"):
            continue
        repo = pkg.get("repository", "")
        if repo:
            repos.append(repo)
    repos.sort(key=str.lower)
    return repos


def build_prefix_counts(repos: list[str], n: int) -> list[tuple[str, int]]:
    """Count repos per n-char prefix, returned sorted."""
    counts: dict[str, int] = {}
    for repo in repos:
        p = repo[:n].lower()
        counts[p] = counts.get(p, 0) + 1
    return sorted(counts.items())


def merge_to_runners(prefix_counts: list[tuple[str, int]],
                     num_runners: int) -> list[list[tuple[str, int]]]:
    """Greedily merge prefix buckets into num_runners groups."""
    total = sum(c for _, c in prefix_counts)
    remaining = total
    runners_left = num_runners
    groups: list[list[tuple[str, int]]] = []
    current: list[tuple[str, int]] = []
    current_count = 0

    for prefix, count in prefix_counts:
        target = remaining / max(runners_left, 1)

        if not current:
            current.append((prefix, count))
            current_count = count
            continue

        new_count = current_count + count

        if (abs(current_count - target) <= abs(new_count - target)
                and runners_left > 1):
            groups.append(current)
            remaining -= current_count
            runners_left -= 1
            current = [(prefix, count)]
            current_count = count
        else:
            current.append((prefix, count))
            current_count = new_count

    if current:
        groups.append(current)

    return groups


def check_balance(groups: list[list[tuple[str, int]]],
                  target: float) -> bool:
    """Return True if all groups are within MAX_DEVIATION of target."""
    for group in groups:
        count = sum(c for _, c in group)
        if abs(count - target) / target > MAX_DEVIATION:
            return False
    return True


# --- Filter generation ---

def _safe_range(start: str, end: str) -> str:
    """Regex char-class range, splitting across the digit/letter boundary."""
    if start == end:
        return start
    if start.isdigit() and not end.isdigit():
        return f"0-9a-{end}"
    return f"{start}-{end}"


def make_filter(prefixes: list[str], is_first: bool, is_last: bool,
                next_first: str | None) -> str:
    """Build a regex filter from the list of prefixes in a group.

    Groups by first character, then uses character ranges for the second
    character when needed. Goes deeper only when prefixes share longer
    common prefixes.
    """
    # Group prefixes by first char
    by_c1: dict[str, list[str]] = {}
    for p in prefixes:
        by_c1.setdefault(p[0], []).append(p)

    # Determine which first chars we fully own vs partially own
    # A first char is "full" if ALL its prefixes are in our group
    # For the filter, we need to handle:
    # - Full letters: just the letter
    # - Partial start: letter + [start_c2-z]
    # - Partial end: letter + [0-end_c2]  (exclusive of next group)
    # - Partial both: letter + [start_c2-end_c2]

    sorted_c1s = sorted(by_c1.keys())

    # If this is the first group, fill in gaps so that ALL possible first
    # characters are covered (not just those present in the config).
    ALNUM = "0123456789abcdefghijklmnopqrstuvwxyz"
    if is_first and sorted_c1s:
        # Find the last single-char prefix in our group
        last_single = None
        for c in sorted_c1s:
            if len(c) == 1 or all(len(p) == 1 for p in by_c1.get(c, [])):
                last_single = c
        # Fill all alnum chars up to and including last_single
        if last_single:
            for ch in ALNUM:
                if ch not in by_c1:
                    by_c1[ch] = [ch]
                if ch == last_single:
                    break
            sorted_c1s = sorted(by_c1.keys())

    parts = []

    for idx, c1 in enumerate(sorted_c1s):
        group = by_c1[c1]

        # Is this a partial group? Check if all prefixes are 1-char (meaning
        # we cover the whole letter at this depth)
        if all(len(p) == 1 for p in group):
            parts.append(c1)
            continue

        # We have multi-char prefixes - need second-char ranges
        second_chars = sorted(set(p[1] for p in group if len(p) > 1))

        # Is this the first c1 and we have a partial start?
        is_first_c1 = (idx == 0 and not is_first)
        # Is this the last c1 and we have a partial end?
        is_last_c1 = (idx == len(sorted_c1s) - 1 and not is_last)

        if is_last_c1 and next_first and next_first[0] == c1:
            # We share this letter with the next group
            # Our range ends just before next group's second char
            end_c2 = next_first[1] if len(next_first) > 1 else "0"
            end_c2_excl = chr(ord(end_c2) - 1)

            start_c2 = second_chars[0]
            if is_first_c1:
                # Partial on both sides within same letter
                parts.append(f"{c1}[{_safe_range(start_c2, end_c2_excl)}]")
            else:
                # Full start, partial end - use negated class so that
                # non-alnum second chars (like '-', '/') are also covered
                if end_c2_excl >= "z":
                    parts.append(c1)
                else:
                    parts.append(f"{c1}[^{_safe_range(end_c2, 'z')}]")
        elif is_first_c1:
            # Partial start
            start_c2 = second_chars[0]
            if start_c2 in ("0", "a"):
                parts.append(c1)
            else:
                parts.append(f"{c1}[{_safe_range(start_c2, 'z')}]")
        else:
            # We fully own this letter
            parts.append(c1)

    # Merge consecutive single-letter parts into ranges
    merged = _merge_letter_ranges(parts)

    if len(merged) == 1:
        return f"(?i)^{merged[0]}"
    return f"(?i)^({'|'.join(merged)})"


def _merge_letter_ranges(parts: list[str]) -> list[str]:
    """Merge consecutive single-letter parts into [a-z] ranges."""
    if not parts:
        return parts

    result = []
    run_start: str | None = None
    run_end: str | None = None

    for p in parts:
        if len(p) == 1 and p.isalnum():
            if run_start is None:
                run_start = p
                run_end = p
            elif ord(p) == ord(run_end) + 1:
                run_end = p
            else:
                result.append(_format_range(run_start, run_end))
                run_start = p
                run_end = p
        else:
            if run_start is not None:
                result.append(_format_range(run_start, run_end))
                run_start = None
                run_end = None
            result.append(p)

    if run_start is not None:
        result.append(_format_range(run_start, run_end))

    return result


def _format_range(start: str, end: str) -> str:
    if start == end:
        return start
    if ord(end) - ord(start) == 1:
        return f"[{start}{end}]"
    return f"[{_safe_range(start, end)}]"


def make_name(prefixes: list[str], is_first: bool, is_last: bool,
              next_first: str | None) -> str:
    """Generate a readable name from boundary prefixes."""
    first = prefixes[0]
    last = prefixes[-1]

    if is_first:
        start = "0"
    else:
        start = first[:2] if len(first) > 1 and first[1].isalnum() else first[0]

    if is_last:
        end = "z"
    elif next_first and next_first[0] == last[0]:
        nc2 = next_first[1] if len(next_first) > 1 else "0"
        end = f"{last[0]}{chr(ord(nc2) - 1)}"
    else:
        end = last[0]

    if start == end:
        return start
    return f"{start}-{end}"


# --- Workflow YAML ---

def generate_matrix_yaml(groups: list[dict], indent: str) -> str:
    lines = [f"{indent}include:"]
    for g in groups:
        lines.append(f'{indent}  - name: "{g["name"]}"')
        lines.append(f'{indent}    filter: "{g["filter"]}"')
    return "\n".join(lines)


def update_workflow(workflow_path: str, groups: list[dict], dry_run: bool) -> None:
    with open(workflow_path, "r") as f:
        content = f.read()

    m = MATRIX_START_RE.search(content)
    if not m:
        print("Error: Could not find 'include:' in workflow file.", file=sys.stderr)
        sys.exit(1)

    indent = m.group(1)
    include_start = m.start()

    pos = m.end()
    while pos < len(content):
        if content[pos] == "\n":
            pos += 1
            continue
        line_end = content.find("\n", pos)
        if line_end == -1:
            line_end = len(content)
        stripped = content[pos:line_end].strip()
        if (stripped.startswith("- name:") or
                stripped.startswith("filter:") or
                stripped.startswith("#") or
                stripped == ""):
            pos = line_end + 1
        else:
            break

    include_end = pos
    new_matrix = generate_matrix_yaml(groups, indent)
    new_content = content[:include_start] + new_matrix + "\n" + content[include_end:]

    if dry_run:
        print(new_matrix)
    else:
        with open(workflow_path, "w") as f:
            f.write(new_content)
        print(f"Updated {workflow_path}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(
        description="Rebalance GitHub Actions runner matrix for even package distribution."
    )
    parser.add_argument(
        "-c", "--config", default=CONFIG_PATH_DEFAULT,
        help=f"Path to config.toml (default: {CONFIG_PATH_DEFAULT})",
    )
    parser.add_argument(
        "-w", "--workflow", default=WORKFLOW_PATH_DEFAULT,
        help=f"Path to workflow YAML (default: {WORKFLOW_PATH_DEFAULT})",
    )
    parser.add_argument(
        "-r", "--runners", type=int, default=20,
        help="Number of runners to distribute across (default: 20)",
    )
    parser.add_argument(
        "-n", "--dry-run", action="store_true",
        help="Print the new matrix without modifying the workflow file",
    )
    parser.add_argument(
        "--random-repos", type=int, default=10, metavar="N",
        help="Number of random repo names to generate for verification (default: 10)",
    )
    args = parser.parse_args()

    repos = load_repos(args.config)
    if not repos:
        print("No packages found in config.", file=sys.stderr)
        sys.exit(1)

    total = len(repos)
    target = total / args.runners

    # Try increasing prefix depths until balance is acceptable
    for n in range(1, 7):
        prefix_counts = build_prefix_counts(repos, n)
        runner_groups = merge_to_runners(prefix_counts, args.runners)
        if check_balance(runner_groups, target):
            print(f"Balanced with prefix depth n={n}", file=sys.stderr)
            break
        print(f"Prefix depth n={n}: too unbalanced, trying n={n + 1}...",
              file=sys.stderr)
    else:
        print("Warning: could not achieve +-10% balance even at max depth",
              file=sys.stderr)

    print(f"Total packages: {total}", file=sys.stderr)
    print(f"Runners: {len(runner_groups)}, target per runner: ~{target:.0f}", file=sys.stderr)
    print(file=sys.stderr)

    groups = []
    for i, group in enumerate(runner_groups):
        prefixes = [p for p, _ in group]
        count = sum(c for _, c in group)
        is_first = (i == 0)
        is_last = (i == len(runner_groups) - 1)
        next_first = runner_groups[i + 1][0][0] if not is_last else None
        filt = make_filter(prefixes, is_first, is_last, next_first)
        name = make_name(prefixes, is_first, is_last, next_first)
        pct_off = ((count - target) / target) * 100
        print(f"  {name:8s}  {count:4d} pkgs ({pct_off:+.1f}%)  filter: {filt}",
              file=sys.stderr)
        groups.append({"name": name, "filter": filt, "count": count})

    # --- Verification ---
    verify_filters(groups, repos, args.random_repos)

    update_workflow(args.workflow, groups, args.dry_run)

    if not args.dry_run:
        print("\nRunning actionlint...", file=sys.stderr)
        result = subprocess.run(["actionlint", args.workflow])
        if result.returncode != 0:
            sys.exit(result.returncode)
        print("actionlint passed", file=sys.stderr)


def _random_github_name(min_len: int = 1, max_len: int = 20) -> str:
    """Generate a random valid GitHub username or repo name.

    GitHub rules:
    - Owner names: alphanumeric and hyphens, cannot start/end with hyphen,
      no consecutive hyphens, max 39 chars.
    - Repo names: alphanumeric, hyphens, periods, underscores, max 100 chars.
      Cannot start with a period.

    We combine both character sets since the filter matches "owner/repo"
    and must handle all valid characters in either position.
    """
    # Valid characters: a-z, 0-9, -, ., _
    inner_chars = string.ascii_lowercase + string.digits + "-._"
    # First char: alphanumeric only (no leading -, ., _)
    first_chars = string.ascii_lowercase + string.digits
    length = random.randint(min_len, max_len)
    if length == 1:
        return random.choice(first_chars)
    name = random.choice(first_chars)
    name += "".join(random.choice(inner_chars) for _ in range(length - 2))
    # Last char: no trailing hyphen
    name += random.choice(first_chars)
    return name


def generate_random_repos(count: int) -> list[str]:
    """Generate random owner/repo strings matching GitHub naming rules."""
    repos = []
    for _ in range(count):
        owner = _random_github_name(1, 6)
        repo = _random_github_name(1, 6)
        repos.append(f"{owner}/{repo}")
    return repos


def verify_filters(groups: list[dict], repos: list[str],
                   num_random: int) -> None:
    """Verify every repo matches exactly one filter. Exit on failure."""
    compiled = [(g["name"], re.compile(g["filter"])) for g in groups]

    errors = []

    # Check all real repos
    for repo in repos:
        matches = [name for name, pat in compiled if pat.search(repo)]
        if len(matches) == 0:
            errors.append(f"  unmatched: {repo}")
        elif len(matches) > 1:
            errors.append(f"  multi-matched: {repo} -> {matches}")

    # Check random repos
    random_repos = generate_random_repos(num_random)
    for repo in random_repos:
        matches = [name for name, pat in compiled if pat.search(repo)]
        if len(matches) == 0:
            errors.append(f"  unmatched (random): {repo}")
        elif len(matches) > 1:
            errors.append(f"  multi-matched (random): {repo} -> {matches}")

    if errors:
        print(f"\nVerification FAILED ({len(errors)} errors):", file=sys.stderr)
        for e in errors:
            print(e, file=sys.stderr)
        sys.exit(1)

    print(f"\nVerification passed: {len(repos)} real + {num_random} random repos OK",
          file=sys.stderr)


if __name__ == "__main__":
    main()
