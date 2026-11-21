"""Check Pages publication provenance in the called deployment workflow."""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import quote, urlencode, urlsplit
from urllib.request import Request, urlopen

PROOF_NAME = "bevy-cicd-build.json"


def read_proof(path: Path, commit: str) -> tuple[bytes, str]:
    raw = path.read_bytes()
    proof = json.loads(raw)
    if not isinstance(proof, dict) or proof.get("commit") != commit:
        raise ValueError("The publication proof is for a different commit")
    index_hash = proof.get("index_sha256")
    if not isinstance(index_hash, str) or not re.fullmatch(r"[0-9a-f]{64}", index_hash):
        raise ValueError("The publication proof must include a lowercase index_sha256")
    return raw, index_hash


def verify_publication(url: str, proof: bytes, index_hash: str, commit: str) -> None:
    base = urlsplit(url)
    if (
        base.scheme != "https"
        or not base.hostname
        or base.username is not None
        or base.password is not None
        or base.query
        or base.fragment
        or any(character.isspace() for character in url)
    ):
        raise ValueError("The successful Pages deployment did not return a valid HTTPS URL")

    def fetch(path: str, attempt: int) -> bytes:
        query = urlencode({"bevy-cicd": f"{commit}-{attempt}"})
        request = Request(
            f"{url.rstrip('/')}/{path}?{query}",
            headers={"Accept-Encoding": "identity", "Cache-Control": "no-cache"},
        )
        with urlopen(request, timeout=30) as response:
            return response.read()

    for attempt in range(1, 7):
        try:
            if fetch(PROOF_NAME, attempt) != proof:
                raise ValueError("The published marker differs from the expected publication proof")
            if hashlib.sha256(fetch("", attempt)).hexdigest() != index_hash:
                raise ValueError("The published index SHA-256 differs from the publication proof")
            return
        except (OSError, ValueError) as error:
            print(f"HTTP verification attempt {attempt}/6: {error}", file=sys.stderr)
            if attempt == 6:
                raise ValueError("Published Pages content did not match the checked artifact") from error
            time.sleep(10)


def github_api(endpoint: str, field: str) -> str:
    return subprocess.run(
        ["gh", "api", endpoint, "--jq", field],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def is_current_commit(repository: str, branch: str, commit: str) -> bool:
    endpoint = f"repos/{repository}"
    default_branch = github_api(endpoint, ".default_branch")
    if branch != default_branch:
        return False
    tip = github_api(f"{endpoint}/git/ref/heads/{quote(branch)}", ".object.sha")
    return tip == commit


def append_line(path: str, line: str) -> None:
    with open(path, "a", encoding="utf-8") as output:
        print(line, file=output)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    current = commands.add_parser("current")
    current.add_argument("--wait", type=int, default=0)
    validate = commands.add_parser("validate")
    validate.add_argument("proof", type=Path)
    verify = commands.add_parser("verify")
    verify.add_argument("proof", type=Path)
    verify.add_argument("url")
    args = parser.parse_args()
    commit = os.environ["GITHUB_SHA"]

    try:
        if args.command == "current":
            if args.wait:
                time.sleep(args.wait)
            is_current = is_current_commit(os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_REF_NAME"], commit)
            append_line(os.environ["GITHUB_OUTPUT"], "current=true" if is_current else "current=false")
            if not is_current:
                append_line(os.environ["GITHUB_STEP_SUMMARY"], "This commit is no longer at the tip of the default branch. Pages deployment was skipped.")
            return

        proof, index_hash = read_proof(args.proof, commit)
        if args.command == "verify":
            verify_publication(args.url, proof, index_hash, commit)
            append_line(os.environ["GITHUB_OUTPUT"], f"page_url={args.url}")
            append_line(os.environ["GITHUB_STEP_SUMMARY"], f"Verified the deployed Pages marker and index for `{commit}`.")
    except subprocess.CalledProcessError as error:
        parser.exit(1, f"Could not check the current default-branch commit: {error.stderr}\n")
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
