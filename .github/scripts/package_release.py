import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[2]
TARGETS = (
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
)


def validate_version(version, tag=None):
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-beta\.(?:0|[1-9][0-9]*)", version):
        raise ValueError("release version must be an exact beta version")
    if tag is not None and tag != f"v{version}":
        raise ValueError("release tag must match compiler/Cargo.toml version")
    return version


def repository_version(tag=None):
    manifest = tomllib.loads((ROOT / "compiler/Cargo.toml").read_text(encoding="utf-8"))
    return validate_version(manifest["package"]["version"], tag)


def command_output(arguments):
    return subprocess.run(arguments, check=True, capture_output=True, text=True, timeout=60).stdout.strip()


def validate_host(target, rustc):
    if target not in TARGETS:
        raise ValueError("unsupported release target")
    hosts = [line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: ")]
    if hosts != [target]:
        raise ValueError(f"rustc host {hosts!r} does not match {target}")


def archive_name(version, target):
    validate_version(version)
    if target not in TARGETS:
        raise ValueError("unsupported release target")
    suffix = "zip" if target.endswith("windows-msvc") else "tar.gz"
    return f"zore-{version}-{target}.{suffix}"


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def create_archive(binary, license_path, output, version, target, source_commit, rustc, toolchain):
    validate_host(target, rustc)
    if not re.fullmatch("[0-9a-f]{40}", source_commit):
        raise ValueError("source commit must be a full lowercase Git commit ID")
    name = archive_name(version, target)
    executable = "zore.exe" if target.endswith("windows-msvc") else "zore"
    if not binary.is_file() or binary.is_symlink() or binary.stat().st_size == 0:
        raise ValueError("compiler binary must be a nonempty regular file")
    manifest = {
        "version": version,
        "target": target,
        "source_commit": source_commit,
        "toolchain": toolchain,
        "rustc": rustc,
        "binary_sha256": digest(binary),
    }
    contents = {
        executable: binary.read_bytes(),
        "LICENSE": license_path.read_bytes(),
        "artifact.json": (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode(),
    }
    output.mkdir(parents=True, exist_ok=True)
    archive = output / name
    if name.endswith(".zip"):
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as destination:
            for member, data in contents.items():
                destination.writestr(member, data)
    else:
        with tarfile.open(archive, "w:gz") as destination:
            for member, data in contents.items():
                entry = tarfile.TarInfo(member)
                entry.size = len(data)
                entry.mode = 0o755 if member == executable else 0o644
                destination.addfile(entry, io.BytesIO(data))
    archive.with_name(name + ".sha256").write_text(f"{digest(archive)}  {name}\n", encoding="utf-8")
    return archive


def verify_and_extract(archive, destination, version, target):
    expected_name = archive_name(version, target)
    if archive.name != expected_name:
        raise ValueError("archive filename does not match version and target")
    expected_checksum = f"{digest(archive)}  {archive.name}\n"
    if archive.with_name(archive.name + ".sha256").read_text(encoding="utf-8") != expected_checksum:
        raise ValueError("archive checksum mismatch")
    executable = "zore.exe" if target.endswith("windows-msvc") else "zore"
    expected_members = {executable, "LICENSE", "artifact.json"}
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as source:
            members = source.infolist()
            if len(members) != 3 or {member.filename for member in members} != expected_members:
                raise ValueError("unexpected archive members")
            if any(member.is_dir() or (member.external_attr >> 16) & 0o170000 == 0o120000 for member in members):
                raise ValueError("archive members must be regular files")
            contents = {member.filename: source.read(member) for member in members}
    else:
        with tarfile.open(archive, "r:gz") as source:
            members = source.getmembers()
            if len(members) != 3 or {member.name for member in members} != expected_members:
                raise ValueError("unexpected archive members")
            if any(not member.isfile() for member in members):
                raise ValueError("archive members must be regular files")
            contents = {member.name: source.extractfile(member).read() for member in members}
    manifest = json.loads(contents["artifact.json"])
    if manifest.get("version") != version or manifest.get("target") != target:
        raise ValueError("archive manifest does not match version and target")
    if not re.fullmatch("[0-9a-f]{40}", manifest.get("source_commit", "")) or not manifest.get("toolchain"):
        raise ValueError("archive manifest lacks release provenance")
    validate_host(target, manifest.get("rustc", ""))
    if hashlib.sha256(contents[executable]).hexdigest() != manifest.get("binary_sha256"):
        raise ValueError("compiler checksum does not match archive manifest")
    destination.mkdir(parents=True, exist_ok=True)
    for name, data in contents.items():
        (destination / name).write_bytes(data)
    (destination / executable).chmod(0o755)
    return destination / executable


def combine_checksums(directory):
    archives = sorted(path for path in directory.iterdir() if path.name.endswith((".tar.gz", ".zip")))
    version = repository_version()
    expected = {archive_name(version, target) for target in TARGETS}
    expected.add(f"zore-builder-{version}-linux-amd64.tar.gz")
    if {path.name for path in archives} != expected:
        raise ValueError("release requires exactly one archive for each supported target and one builder image")
    lines = []
    for archive in archives:
        line = f"{digest(archive)}  {archive.name}\n"
        if archive.with_name(archive.name + ".sha256").read_text(encoding="utf-8") != line:
            raise ValueError(f"archive checksum mismatch: {archive.name}")
        lines.append(line)
    (directory / "SHA256SUMS").write_text("".join(lines), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    metadata = commands.add_parser("metadata")
    metadata.add_argument("--tag")
    metadata.add_argument("--github-output", type=Path)
    host = commands.add_parser("host")
    host.add_argument("--target", required=True, choices=TARGETS)
    create = commands.add_parser("create")
    create.add_argument("--binary", required=True, type=Path)
    create.add_argument("--target", required=True, choices=TARGETS)
    create.add_argument("--output", required=True, type=Path)
    create.add_argument("--tag")
    checksums = commands.add_parser("checksums")
    checksums.add_argument("--directory", required=True, type=Path)
    extract = commands.add_parser("extract")
    extract.add_argument("--archive", required=True, type=Path)
    extract.add_argument("--destination", required=True, type=Path)
    extract.add_argument("--version", required=True)
    extract.add_argument("--target", required=True, choices=TARGETS)
    arguments = parser.parse_args()
    if arguments.command == "metadata":
        version = repository_version(arguments.tag)
        print(version)
        if arguments.github_output:
            with arguments.github_output.open("a", encoding="utf-8") as destination:
                destination.write(f"version={version}\n")
    elif arguments.command == "host":
        validate_host(arguments.target, command_output(["rustc", "-vV"]))
    elif arguments.command == "create":
        version = repository_version(arguments.tag)
        binary_version = command_output([str(arguments.binary.resolve()), "--version"])
        if binary_version != f"zore {version}":
            raise ValueError("compiler binary version does not match Cargo version")
        toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
        print(create_archive(arguments.binary, ROOT / "LICENSE", arguments.output, version, arguments.target,
                             command_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"]),
                             command_output(["rustc", "-vV"]), toolchain))
    elif arguments.command == "checksums":
        combine_checksums(arguments.directory)
    else:
        print(verify_and_extract(arguments.archive, arguments.destination, arguments.version, arguments.target))


if __name__ == "__main__":
    main()
