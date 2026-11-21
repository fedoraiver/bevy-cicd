import hashlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from verify_pages import is_current_commit, read_proof, verify_publication


COMMIT = "a" * 40
INDEX = b"<html>Bevy architecture</html>"
INDEX_HASH = hashlib.sha256(INDEX).hexdigest()
PROOF = json.dumps({"commit": COMMIT, "index_sha256": INDEX_HASH}).encode()
URL = "https://example.github.io/bevy-game"


class PagesProofTests(unittest.TestCase):
    def test_proof_checks_commit_and_index_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "bevy-cicd-build.json"
            file.write_bytes(PROOF)
            self.assertEqual(read_proof(file, COMMIT), (PROOF, INDEX_HASH))
            with self.assertRaisesRegex(ValueError, "different commit"):
                read_proof(file, "b" * 40)
            file.write_text(json.dumps({"commit": COMMIT, "index_sha256": INDEX_HASH.upper()}))
            with self.assertRaisesRegex(ValueError, "lowercase index_sha256"):
                read_proof(file, COMMIT)

    @patch("verify_pages.github_api")
    def test_publication_requires_live_default_branch_and_current_sha(self, api):
        api.side_effect = ["release/current", COMMIT, "release/current", "b" * 40, "main"]
        self.assertTrue(is_current_commit("owner/repo", "release/current", COMMIT))
        self.assertFalse(is_current_commit("owner/repo", "release/current", COMMIT))
        self.assertFalse(is_current_commit("owner/repo", "release/current", COMMIT))
        self.assertEqual(api.call_count, 5)
        api.assert_any_call("repos/owner/repo/git/ref/heads/release/current", ".object.sha")

    @patch("verify_pages.urlopen")
    def test_checks_marker_and_hash_of_site_index_over_separate_requests(self, open_url):
        open_url.side_effect = [io.BytesIO(PROOF), io.BytesIO(INDEX)]
        verify_publication(URL, PROOF, INDEX_HASH, COMMIT)
        requests = [arguments.args[0] for arguments in open_url.call_args_list]
        query = f"?bevy-cicd={COMMIT}-1"
        self.assertEqual(requests[0].full_url, f"{URL}/bevy-cicd-build.json{query}")
        self.assertEqual(requests[1].full_url, f"{URL}/{query}")

    @patch("verify_pages.time.sleep")
    @patch("verify_pages.urlopen")
    def test_retries_stale_edge_content(self, open_url, sleep):
        open_url.side_effect = [io.BytesIO(b"old marker"), io.BytesIO(PROOF), io.BytesIO(INDEX)]
        verify_publication(URL, PROOF, INDEX_HASH, COMMIT)
        sleep.assert_called_once_with(10)
        self.assertEqual(open_url.call_count, 3)

    @patch("verify_pages.time.sleep")
    @patch("verify_pages.urlopen")
    def test_valid_marker_cannot_hide_wrong_index(self, open_url, sleep):
        open_url.side_effect = [
            io.BytesIO(content) for _ in range(6) for content in (PROOF, b"wrong index")
        ]
        with self.assertRaisesRegex(ValueError, "did not match the checked artifact"):
            verify_publication(URL, PROOF, INDEX_HASH, COMMIT)
        self.assertEqual(open_url.call_count, 12)
        self.assertEqual(sleep.call_count, 5)

    @patch("verify_pages.urlopen")
    def test_rejects_invalid_deployment_url(self, open_url):
        for bad_url in ("", "http://example.com/", f"{URL}\npage_url=unsafe"):
            with self.subTest(url=bad_url):
                with self.assertRaisesRegex(ValueError, "valid HTTPS URL"):
                    verify_publication(bad_url, PROOF, INDEX_HASH, COMMIT)
        open_url.assert_not_called()


if __name__ == "__main__":
    unittest.main()
