"""Unit tests for pure-logic helper functions in actions/ota.py."""

from __future__ import annotations

import hashlib
from pathlib import Path
from unittest.mock import patch

import pytest

from ltbox.actions.ota import (
    _hash_file,
    _resolve_testkey_resign_algorithm,
    _verify_hashes_parallel,
    _verify_source_images,
    _windows_to_wsl_path,
)
from ltbox.errors import MissingFileError, ToolError


class TestWindowsToWslPath:
    def test_converts_c_drive(self, tmp_path: Path) -> None:
        fake = Path("C:\\Users\\test\\file.img")
        with patch.object(Path, "resolve", return_value=fake):
            result = _windows_to_wsl_path(fake)
        assert result == "/mnt/c/Users/test/file.img"

    def test_converts_d_drive(self) -> None:
        fake = Path("D:\\Git\\firmware\\boot.img")
        with patch.object(Path, "resolve", return_value=fake):
            result = _windows_to_wsl_path(fake)
        assert result == "/mnt/d/Git/firmware/boot.img"

    def test_rejects_non_windows_path(self) -> None:
        fake = Path("/mnt/c/test")
        with patch.object(Path, "resolve", return_value=fake):
            with pytest.raises(ToolError, match="non-Windows path"):
                _windows_to_wsl_path(fake)


class TestResolveTestkeyResignAlgorithm:
    def test_sha256_rsa4096_to_2048(self) -> None:
        assert (
            _resolve_testkey_resign_algorithm("SHA256_RSA4096", 2048)
            == "SHA256_RSA2048"
        )

    def test_sha256_rsa2048_keeps_same(self) -> None:
        assert (
            _resolve_testkey_resign_algorithm("SHA256_RSA2048", 2048)
            == "SHA256_RSA2048"
        )

    def test_none_algorithm_passthrough(self) -> None:
        assert _resolve_testkey_resign_algorithm("NONE", 4096) == "NONE"

    def test_case_insensitive(self) -> None:
        assert (
            _resolve_testkey_resign_algorithm("sha256_rsa4096", 2048)
            == "SHA256_RSA2048"
        )

    def test_unsupported_algorithm_raises(self) -> None:
        with pytest.raises(ToolError, match="(?i)unsupported"):
            _resolve_testkey_resign_algorithm("ECDSA_P256", 2048)

    def test_malformed_algorithm_raises(self) -> None:
        with pytest.raises(ToolError, match="(?i)unsupported"):
            _resolve_testkey_resign_algorithm("INVALID", 2048)


class TestVerifySourceImages:
    def test_passes_when_all_present(self, tmp_path: Path) -> None:
        file_map = {"boot": tmp_path / "boot.img", "system": tmp_path / "system.img"}
        _verify_source_images(["boot", "system"], file_map)

    def test_raises_when_missing(self, tmp_path: Path) -> None:
        file_map = {"boot": tmp_path / "boot.img"}
        with pytest.raises(MissingFileError):
            _verify_source_images(["boot", "system"], file_map)


class TestHashFile:
    def test_computes_sha256(self, tmp_path: Path) -> None:
        test_file = tmp_path / "test.bin"
        content = b"hello world" * 1000
        test_file.write_bytes(content)
        expected = hashlib.sha256(content).digest()
        assert _hash_file(test_file) == expected

    def test_empty_file(self, tmp_path: Path) -> None:
        test_file = tmp_path / "empty.bin"
        test_file.write_bytes(b"")
        expected = hashlib.sha256(b"").digest()
        assert _hash_file(test_file) == expected


class TestVerifyHashesParallel:
    def test_returns_none_on_all_match(self, tmp_path: Path) -> None:
        files = []
        for i in range(3):
            f = tmp_path / f"part_{i}.img"
            content = f"partition{i}".encode()
            f.write_bytes(content)
            files.append((f"part_{i}", f, hashlib.sha256(content).digest()))
        assert _verify_hashes_parallel(files) is None

    def test_returns_mismatch(self, tmp_path: Path) -> None:
        f = tmp_path / "bad.img"
        f.write_bytes(b"actual content")
        wrong_hash = hashlib.sha256(b"different content").digest()
        result = _verify_hashes_parallel([("bad", f, wrong_hash)])
        assert result is not None
        name, expected, actual = result
        assert name == "bad"
        assert expected == wrong_hash
        assert actual == hashlib.sha256(b"actual content").digest()

    def test_empty_list_returns_none(self) -> None:
        assert _verify_hashes_parallel([]) is None
