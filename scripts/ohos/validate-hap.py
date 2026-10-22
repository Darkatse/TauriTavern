#!/usr/bin/env python3
"""Reject missing or wrong-architecture native libraries before publishing a HAP."""
import argparse
import hashlib
from pathlib import Path
import struct
import zipfile


def validate_hap(path, target):
    abi, machine = {
        "aarch64-unknown-linux-ohos": ("arm64-v8a", 183),
        "x86_64-unknown-linux-ohos": ("x86_64", 62),
    }[target]
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        library = f"libs/{abi}/libtauritavern_lib.so"
        if library not in names:
            raise ValueError(f"Missing application library: {library}")
        for name in names:
            if not name.startswith("libs/") or not name.endswith(".so"):
                continue
            if name.split("/")[1] != abi:
                raise ValueError(f"Unexpected ABI in HAP: {name}; expected {abi}")
            with archive.open(name) as stream:
                header = stream.read(20)
            if (len(header) != 20 or header[:6] != b"\x7fELF\x02\x01"
                    or struct.unpack_from("<H", header, 18)[0] != machine):
                raise ValueError(f"Wrong ELF architecture: {name}; expected {target}")
        if "ets/modules.abc" not in names or "module.json" not in names:
            raise ValueError("HAP is missing its ArkTS bytecode or module manifest")
        bad = archive.testzip()
        if bad:
            raise ValueError(f"Corrupt HAP entry: {bad}")
    with Path(path).open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return digest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("hap", type=Path)
    args = parser.parse_args()
    digest = validate_hap(args.hap, args.target)
    print(f"Verified {args.target}: {args.hap}\nSHA256 {digest}")
