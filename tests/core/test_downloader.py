"""Tests for ltbox.downloader – archive extraction and helper functions."""

import io
import tarfile
import zipfile
from pathlib import Path
from unittest.mock import patch

import pytest

from ltbox.downloader import (
    _resolve_extract_target,
    extract_archive_files,
)
from ltbox.errors import ToolError


class TestResolveExtractTarget:
    def test_exact_match(self):
        extract_map = {"boot.img": Path("/out/boot.img")}
        assert _resolve_extract_target("boot.img", extract_map) == Path("/out/boot.img")

    def test_prefix_stripped(self):
        extract_map = {"boot.img": Path("/out/boot.img")}
        assert _resolve_extract_target("./boot.img", extract_map) == Path(
            "/out/boot.img"
        )

    def test_nested_suffix_match(self):
        extract_map = {"boot.img": Path("/out/boot.img")}
        result = _resolve_extract_target("firmware/images/boot.img", extract_map)
        assert result == Path("/out/boot.img")

    def test_no_match_returns_none(self):
        extract_map = {"boot.img": Path("/out/boot.img")}
        assert _resolve_extract_target("vbmeta.img", extract_map) is None

    def test_path_traversal_rejected(self):
        extract_map = {"boot.img": Path("/out/boot.img")}
        assert _resolve_extract_target("../etc/passwd", extract_map) is None

    def test_double_dot_in_middle_rejected(self):
        extract_map = {"boot.img": Path("/out/boot.img")}
        assert _resolve_extract_target("a/../boot.img", extract_map) is None


class TestExtractArchiveFiles:
    def test_extract_from_zip(self, tmp_path):
        zip_path = tmp_path / "test.zip"
        target = tmp_path / "boot.img"
        content = b"fake boot image content"

        with zipfile.ZipFile(zip_path, "w") as zf:
            zf.writestr("boot.img", content)

        extract_map = {"boot.img": target}

        with patch("ltbox.utils.ui"):
            result = extract_archive_files(zip_path, extract_map)

        assert target in result
        assert target.read_bytes() == content

    def test_extract_from_tar(self, tmp_path):
        tar_path = tmp_path / "test.tar"
        target = tmp_path / "vbmeta.img"
        content = b"fake vbmeta content"

        with tarfile.open(tar_path, "w") as tf:
            info = tarfile.TarInfo(name="vbmeta.img")
            info.size = len(content)
            tf.addfile(info, io.BytesIO(content))

        extract_map = {"vbmeta.img": target}

        with patch("ltbox.utils.ui"):
            result = extract_archive_files(tar_path, extract_map)

        assert target in result
        assert target.read_bytes() == content

    def test_extract_nested_file_from_zip(self, tmp_path):
        zip_path = tmp_path / "nested.zip"
        target = tmp_path / "init_boot.img"
        content = b"nested init boot"

        with zipfile.ZipFile(zip_path, "w") as zf:
            zf.writestr("images/init_boot.img", content)

        extract_map = {"init_boot.img": target}

        with patch("ltbox.utils.ui"):
            result = extract_archive_files(zip_path, extract_map)

        assert target in result
        assert target.read_bytes() == content

    def test_extract_skips_unmatched_members(self, tmp_path):
        zip_path = tmp_path / "multi.zip"
        target = tmp_path / "boot.img"

        with zipfile.ZipFile(zip_path, "w") as zf:
            zf.writestr("boot.img", b"boot")
            zf.writestr("readme.txt", b"ignored")

        extract_map = {"boot.img": target}

        with patch("ltbox.utils.ui"):
            result = extract_archive_files(zip_path, extract_map)

        assert len(result) == 1

    def test_bad_zip_raises_tool_error(self, tmp_path):
        bad_zip = tmp_path / "bad.zip"
        bad_zip.write_bytes(b"not a zip file at all")

        with patch("ltbox.utils.ui"):
            with pytest.raises(ToolError):
                extract_archive_files(bad_zip, {"boot.img": tmp_path / "boot.img"})

    def test_extract_multiple_files_from_zip(self, tmp_path):
        zip_path = tmp_path / "multi.zip"
        boot_target = tmp_path / "boot.img"
        vbmeta_target = tmp_path / "vbmeta.img"

        with zipfile.ZipFile(zip_path, "w") as zf:
            zf.writestr("boot.img", b"boot content")
            zf.writestr("vbmeta.img", b"vbmeta content")

        extract_map = {
            "boot.img": boot_target,
            "vbmeta.img": vbmeta_target,
        }

        with patch("ltbox.utils.ui"):
            result = extract_archive_files(zip_path, extract_map)

        assert boot_target in result
        assert vbmeta_target in result
        assert boot_target.read_bytes() == b"boot content"
        assert vbmeta_target.read_bytes() == b"vbmeta content"

    def test_extract_from_tar_gz(self, tmp_path):
        tar_path = tmp_path / "test.tar.gz"
        target = tmp_path / "boot.img"
        content = b"compressed boot image"

        with tarfile.open(tar_path, "w:gz") as tf:
            info = tarfile.TarInfo(name="boot.img")
            info.size = len(content)
            tf.addfile(info, io.BytesIO(content))

        extract_map = {"boot.img": target}

        with patch("ltbox.utils.ui"):
            result = extract_archive_files(tar_path, extract_map)

        assert target in result
        assert target.read_bytes() == content
