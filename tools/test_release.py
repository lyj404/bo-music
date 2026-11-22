import argparse
import hashlib
from pathlib import Path
import tarfile
import tempfile
import unittest

import release


class ReleaseTests(unittest.TestCase):
    def test_tag_must_match_the_compiled_package_version(self):
        with self.assertRaisesRegex(ValueError, "must match"):
            release.metadata(argparse.Namespace(tag="v999.0.0", repository="owner/repo"))

    def test_unsafe_repository_and_version_are_rejected(self):
        for version, repository in (("1.0.0/../../file", "owner/repo"), ("1.0.0", "owner/repo';bad")):
            with self.assertRaises(ValueError):
                release.validate(version, repository)

    def test_prerelease_aur_uses_original_tag_and_archive_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "bo_music_1.2.3-beta.1_linux_x64.tar.gz"
            archive.write_bytes(b"release archive content")
            release.aur(argparse.Namespace(version="1.2.3-beta.1", repository="owner/renamed", directory=root))
            package = (root / "aur/bo_music-bin/PKGBUILD").read_text()
            srcinfo = (root / "aur/bo_music-bin/.SRCINFO").read_text()
            expected = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertIn("pkgver=1.2.3beta.1", package)
            self.assertIn("/owner/renamed/releases/download/v1.2.3-beta.1/", package)
            self.assertIn(expected, package)
            self.assertIn(expected, srcinfo)
            self.assertNotIn("SKIP", package)
            self.assertNotIn("aarch64", srcinfo)
            with tarfile.open(root / "bo_music_1.2.3-beta.1_aur.tar.gz") as source:
                self.assertIn("bo_music-bin/.SRCINFO", source.getnames())

    def test_release_requires_every_asset_and_hashes_the_final_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = argparse.Namespace(version="1.2.3", repository="owner/repo", directory=root)
            with self.assertRaisesRegex(ValueError, "Missing release asset"):
                release.notes(args)
            for suffix in ("windows_x64.zip", "windows_x64_setup.exe", "linux_x64.tar.gz", "amd64.deb", "aur.tar.gz"):
                (root / f"bo_music_1.2.3_{suffix}").write_bytes(suffix.encode())
            release.notes(args)
            for line in (root / "SHA256SUMS").read_text().splitlines():
                digest, name = line.split("  ", 1)
                self.assertEqual(digest, hashlib.sha256((root / name).read_bytes()).hexdigest())
            self.assertIn("简体中文", (root / "release-notes.md").read_text())


if __name__ == "__main__":
    unittest.main()
