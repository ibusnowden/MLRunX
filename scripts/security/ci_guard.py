#!/usr/bin/env python3
"""CI guardrails for secret hygiene and public env safety.

Fails CI when:
1) Sensitive variables are exposed as NEXT_PUBLIC_*.
2) High-confidence credentials/private keys are committed.
"""

from __future__ import annotations

import pathlib
import re
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]

SKIP_DIRS = {
    ".git",
    ".venv",
    "node_modules",
    "target",
    ".next",
    "dist",
}

ALLOWED_NEXT_PUBLIC = {
    "NEXT_PUBLIC_API_URL",
    "NEXT_PUBLIC_SUPABASE_URL",
    "NEXT_PUBLIC_SUPABASE_ANON_KEY",
}

FORBIDDEN_NEXT_PUBLIC_KEYWORDS = (
    "SECRET",
    "PASSWORD",
    "PRIVATE",
    "SERVICE_ROLE",
    "JWT",
    "TOKEN",
    "KEY",
)

NEXT_PUBLIC_PATTERN = re.compile(r"\b(NEXT_PUBLIC_[A-Z0-9_]+)\b")

HIGH_CONFIDENCE_SECRET_PATTERNS = [
    re.compile(r"mlrunx_[0-9a-fA-F]{64}\b"),
    re.compile(r"-----BEGIN (?:RSA|EC|OPENSSH|PRIVATE) PRIVATE KEY-----"),
]


def iter_text_files(root: pathlib.Path) -> list[pathlib.Path]:
    files: list[pathlib.Path] = []
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        files.append(path)
    return files


def is_text_file(path: pathlib.Path) -> bool:
    try:
        data = path.read_bytes()
    except OSError:
        return False
    return b"\x00" not in data


def main() -> int:
    findings: list[str] = []

    for path in iter_text_files(REPO_ROOT):
        if not is_text_file(path):
            continue
        try:
            content = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue

        rel_path = path.relative_to(REPO_ROOT)
        for lineno, line in enumerate(content.splitlines(), start=1):
            for match in NEXT_PUBLIC_PATTERN.finditer(line):
                var = match.group(1)
                if var in ALLOWED_NEXT_PUBLIC:
                    continue
                if any(keyword in var for keyword in FORBIDDEN_NEXT_PUBLIC_KEYWORDS):
                    findings.append(
                        f"{rel_path}:{lineno}: disallowed public env var: {var}"
                    )

            for pattern in HIGH_CONFIDENCE_SECRET_PATTERNS:
                if pattern.search(line):
                    findings.append(
                        f"{rel_path}:{lineno}: potential committed secret matched '{pattern.pattern}'"
                    )

    if findings:
        print("Security CI guard failed:", file=sys.stderr)
        for finding in findings:
            print(f" - {finding}", file=sys.stderr)
        return 1

    print("Security CI guard passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
