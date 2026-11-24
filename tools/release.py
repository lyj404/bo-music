#!/usr/bin/env python3
"""Shared release metadata, AUR packaging and release asset verification."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import shlex
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
VERSION_PATTERN = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?"


def validate(version, repository):
    if not re.fullmatch(VERSION_PATTERN, version):
        raise ValueError("Expected MAJOR.MINOR.PATCH with an optional prerelease suffix")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Expected a GitHub owner/repository")


def checksum(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def metadata(args):
    version = tomllib.loads((ROOT / "crates/bo_music/Cargo.toml").read_text())["package"]["version"]
    tag = args.tag or f"v{version}"
    if tag != f"v{version}":
        raise ValueError(f"Tag {tag} must match the desktop package version v{version}")
    validate(version, args.repository)
    values = {"version": version, "tag": tag, "prerelease": str("-" in version).lower()}
    output = "".join(f"{key}={value}\n" for key, value in values.items())
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as target:
            target.write(output)
    print(output, end="")


def aur(args):
    validate(args.version, args.repository)
    version = args.version
    archive = args.directory / f"bo_music_{version}_linux_x64.tar.gz"
    url = f"https://github.com/{args.repository}"
    substitutions = {
        "PKGVER": version.replace("-", ""), "URL": url, "VERSION": version,
        "SOURCE_URL": f"{url}/releases/download/v{version}/{archive.name}",
        "SHA256": checksum(archive),
    }
    pkgbuild = (ROOT / "installer/aur/PKGBUILD.in").read_text()
    for key, value in substitutions.items():
        pkgbuild = pkgbuild.replace(f"@{key}@", value)
    destination = args.directory / "aur" / "bo_music"
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "PKGBUILD").write_text(pkgbuild)
    # Only literal assignments in the template header are used. No shell code is executed.
    fields = {}
    for line in pkgbuild.split("package()", 1)[0].splitlines():
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            fields[key] = shlex.split(value.strip("()"))
    srcinfo = ["pkgbase = bo_music"]
    for field in ("pkgdesc", "pkgver", "pkgrel", "url", "arch", "license", "depends", "optdepends", "options", "source_x86_64", "sha256sums_x86_64"):
        srcinfo.extend(f"\t{field} = {value}" for value in fields[field])
    srcinfo.extend(["", "pkgname = bo_music", ""])
    (destination / ".SRCINFO").write_text("\n".join(srcinfo))
    with tarfile.open(args.directory / f"bo_music_{version}_aur.tar.gz", "w:gz") as target:
        target.add(destination, arcname="bo_music")


def notes(args):
    validate(args.version, args.repository)
    version = args.version
    assets = [
        f"bo_music_{version}_windows_x64.zip",
        f"bo_music_{version}_windows_x64_setup.exe",
        f"bo_music_{version}_linux_x64.tar.gz",
        f"bo_music_{version}_amd64.deb",
        f"bo_music_{version}_aur.tar.gz",
    ]
    for name in assets:
        if not (args.directory / name).is_file():
            raise ValueError(f"Missing release asset: {name}")
    (args.directory / "SHA256SUMS").write_text("".join(
        f"{checksum(args.directory / name)}  {name}\n" for name in assets
    ))
    base = f"https://github.com/{args.repository}/releases/download/v{version}"
    rows = zip(("Windows x64 压缩包", "Windows x64 简体中文安装包", "Linux x64 压缩包", "Debian / Ubuntu 24.04+ amd64", "AUR 构建文件（bo_music）"), assets)
    text = "| 平台 | 下载 |\n| --- | --- |\n"
    text += "".join(f"| {label} | [{name}]({base}/{name}) |\n" for label, name in rows)
    text += "\nLinux 压缩包需要系统提供 libmpv.so.2、字体、图形库和 Secret Service。DEB 会自动安装声明的依赖。\n"
    text += "\nAUR 文件包含 PKGBUILD 和 .SRCINFO，可解压后运行 `makepkg -si`。首次提交到 AUR 后，可使用 `yay -S bo_music`。\n"
    (args.directory / "release-notes.md").write_text(text)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name, function in (("metadata", metadata), ("aur", aur), ("notes", notes)):
        command = commands.add_parser(name)
        command.add_argument("--repository", required=True)
        command.set_defaults(function=function)
        if name == "metadata":
            command.add_argument("--tag", default="")
        else:
            command.add_argument("--version", required=True)
            command.add_argument("--directory", type=Path, default=Path("target/dist"))
    arguments = parser.parse_args()
    arguments.function(arguments)
