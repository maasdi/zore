import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from package_release import (
    TARGETS, archive_name, combine_checksums, create_archive, digest,
    validate_host, validate_version, verify_and_extract,
)


VERSION = "0.1.0-alpha.1"
COMMIT = "a" * 40


class ReleasePackagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.binary = self.directory / "compiler"
        self.binary.write_bytes(b"compiler bytes\x00\xff")
        self.license = self.directory / "license"
        self.license.write_text("release license", encoding="utf-8")

    def create(self, target=TARGETS[0]):
        return create_archive(self.binary, self.license, self.directory / "dist", VERSION,
                              target, COMMIT, f"rustc 1.98.1\nhost: {target}", "1.98.1")

    def refresh_checksum(self, archive):
        archive.with_name(archive.name + ".sha256").write_text(
            f"{digest(archive)}  {archive.name}\n", encoding="utf-8")

    def rewrite_tar(self, archive, contents):
        with tarfile.open(archive, "w:gz") as output:
            for name, data in contents.items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                output.addfile(member, io.BytesIO(data))
        self.refresh_checksum(archive)

    def test_all_platform_archives_include_license_and_provenance(self):
        for target in TARGETS:
            with self.subTest(target=target):
                archive = self.create(target)
                installed = verify_and_extract(archive, self.directory / target, VERSION, target)
                self.assertEqual(installed.read_bytes(), self.binary.read_bytes())
                self.assertEqual((installed.parent / "LICENSE").read_bytes(), self.license.read_bytes())
                manifest = json.loads((installed.parent / "artifact.json").read_text(encoding="utf-8"))
                self.assertEqual(manifest["version"], VERSION)
                self.assertEqual(manifest["target"], target)
                self.assertEqual(manifest["source_commit"], COMMIT)
                self.assertEqual(manifest["toolchain"], "1.98.1")
                self.assertEqual(manifest["binary_sha256"], hashlib.sha256(self.binary.read_bytes()).hexdigest())
                if archive.name.endswith(".tar.gz"):
                    with tarfile.open(archive) as source:
                        self.assertEqual(source.getmember("zore").mode, 0o755)

    def test_version_tag_host_and_commit_rejections(self):
        for version in ("0.1.0", "0.1.0-beta.1", "0.1.0-rc.1", "0.1.0-alpha.01", "../0.1.0-alpha.1", "0.1.0-alpha.1+build"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                validate_version(version)
        for tag in ("v0.1.0-alpha.2", "v0.1.0-beta.1"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                validate_version(VERSION, tag)
        self.assertEqual(validate_version(VERSION, f"v{VERSION}"), VERSION)
        for rustc in ("rustc 1.98.1", "host: x86_64-apple-darwin", f"host: {TARGETS[0]}\nhost: {TARGETS[0]}"):
            with self.subTest(rustc=rustc), self.assertRaises(ValueError):
                validate_host(TARGETS[0], rustc)
        with self.assertRaises(ValueError):
            create_archive(self.binary, self.license, self.directory, VERSION, TARGETS[0],
                           "abc", f"host: {TARGETS[0]}", "1.98.1")
        self.binary.write_bytes(b"")
        with self.assertRaises(ValueError):
            self.create()

    def test_checksum_tampering_is_rejected(self):
        archive = self.create()
        archive.write_bytes(archive.read_bytes() + b"tampered")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            verify_and_extract(archive, self.directory / "installed", VERSION, TARGETS[0])

    def test_unexpected_and_traversal_members_are_rejected_before_extraction(self):
        archive = self.create()
        self.rewrite_tar(archive, {"../escape": b"bad", "LICENSE": b"license", "artifact.json": b"{}"})
        with self.assertRaisesRegex(ValueError, "unexpected archive members"):
            verify_and_extract(archive, self.directory / "installed", VERSION, TARGETS[0])
        self.assertFalse((self.directory / "escape").exists())
        archive = self.create(TARGETS[-1])
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("zore.exe", b"bad")
            output.writestr("LICENSE", b"license")
            output.writestr("artifact.json", b"{}")
            output.writestr("../escape", b"bad")
        self.refresh_checksum(archive)
        with self.assertRaisesRegex(ValueError, "unexpected archive members"):
            verify_and_extract(archive, self.directory / "installed", VERSION, TARGETS[-1])

    def test_manifest_and_binary_tampering_are_rejected(self):
        archive = self.create()
        with tarfile.open(archive) as source:
            contents = {member.name: source.extractfile(member).read() for member in source.getmembers()}
        manifest = json.loads(contents["artifact.json"])
        manifest["target"] = TARGETS[1]
        contents["artifact.json"] = json.dumps(manifest).encode()
        self.rewrite_tar(archive, contents)
        with self.assertRaisesRegex(ValueError, "manifest does not match"):
            verify_and_extract(archive, self.directory / "installed", VERSION, TARGETS[0])
        manifest["target"] = TARGETS[0]
        contents["artifact.json"] = json.dumps(manifest).encode()
        contents["zore"] = b"different compiler"
        self.rewrite_tar(archive, contents)
        with self.assertRaisesRegex(ValueError, "compiler checksum"):
            verify_and_extract(archive, self.directory / "installed", VERSION, TARGETS[0])

    def test_checksum_aggregate_requires_all_targets_and_valid_sidecars(self):
        archive = self.create()
        with patch("package_release.repository_version", return_value=VERSION):
            with self.assertRaisesRegex(ValueError, "exactly one archive"):
                combine_checksums(archive.parent)
            for target in TARGETS[1:]:
                self.create(target)
            image = archive.parent / f"zore-builder-{VERSION}-linux-amd64.tar.gz"
            image.write_bytes(b"validated container image")
            self.refresh_checksum(image)
            combine_checksums(archive.parent)
            lines = (archive.parent / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(lines), 5)
            self.assertEqual({line.split("  ")[1] for line in lines},
                             {archive_name(VERSION, target) for target in TARGETS} | {image.name})
            archive.with_name(archive.name + ".sha256").write_text("bad", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                combine_checksums(archive.parent)


if __name__ == "__main__":
    unittest.main()
