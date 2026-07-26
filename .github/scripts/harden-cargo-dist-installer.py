#!/usr/bin/env python3
"""Apply fail-closed verification to cargo-dist's generated shell installer."""

from pathlib import Path
import sys


def replace_once(contents: str, before: str, after: str, description: str) -> str:
    if contents.count(before) != 1:
        raise SystemExit(
            f"cargo-dist installer does not contain the expected {description} block"
        )
    return contents.replace(before, after)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: harden-cargo-dist-installer.py INSTALLER")

    installer = Path(sys.argv[1])
    contents = installer.read_text()
    checksum_before = """\
        sha256)
            if ! check_cmd sha256sum; then
                say "skipping sha256 checksum verification (it requires the 'sha256sum' command)"
                return 0
            fi
            _calculated_checksum="$(sha256sum -b "$_file" | awk '{printf $1}')"
            ;;
"""
    checksum_after = """\
        sha256)
            if check_cmd sha256sum; then
                _calculated_checksum="$(sha256sum -b "$_file" | awk '{printf $1}')"
            elif check_cmd shasum; then
                _calculated_checksum="$(shasum -a 256 "$_file" | awk '{printf $1}')"
            elif check_cmd openssl; then
                _calculated_checksum="$(openssl dgst -sha256 "$_file" | awk '{printf $NF}')"
            else
                err "cannot verify the sha256 checksum: sha256sum, shasum, or openssl is required"
            fi
            ;;
"""
    receipt_before = """\
            echo "$RECEIPT" > "$RECEIPT_HOME/$APP_NAME-receipt.json"
            # shellcheck disable=SC2320
            local _retval=$?
"""
    receipt_after = """\
            echo "$RECEIPT" > "$RECEIPT_HOME/$APP_NAME-receipt.json"
            # shellcheck disable=SC2320
            local _retval=$?
            if [ "$_retval" -eq 0 ]; then
                chmod 600 "$RECEIPT_HOME/$APP_NAME-receipt.json"
                _retval=$?
            fi
"""

    contents = replace_once(
        contents,
        checksum_before,
        checksum_after,
        "fail-closed SHA-256 verification",
    )
    contents = replace_once(
        contents,
        receipt_before,
        receipt_after,
        "owner-only receipt permissions",
    )
    installer.write_text(contents)


if __name__ == "__main__":
    main()
