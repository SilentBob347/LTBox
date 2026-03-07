import shutil
import sys
from pathlib import Path
from unittest.mock import patch

import pytest
from ltbox.patch import avb

sys.path.append(str(Path(__file__).resolve().parents[2] / "bin"))

pytestmark = pytest.mark.integration


def test_vbmeta_parse(fw_pkg):
    path = fw_pkg.get("vbmeta.img")
    assert path and path.exists()

    info = avb.extract_image_avb_info(path)
    assert info["algorithm"] == "SHA256_RSA4096"


def test_boot_parse(fw_pkg):
    path = fw_pkg.get("boot.img")
    assert path and path.exists()

    info = avb.extract_image_avb_info(path)
    assert int(info["partition_size"]) > int(info["data_size"])


def test_process_boot_image_avb_reapplies_footer(fw_pkg, tmp_path):
    boot_img = fw_pkg.get("boot.img")
    if not boot_img:
        pytest.skip("boot.img not found in firmware package")

    boot_bak = tmp_path / "boot.bak.img"
    target_boot = tmp_path / "boot_target.img"
    shutil.copy(boot_img, boot_bak)
    shutil.copy(boot_img, target_boot)

    boot_info = avb.extract_image_avb_info(boot_bak)

    with patch("ltbox.constants.BASE_DIR", tmp_path):
        avb.process_boot_image_avb(target_boot, gki=True)

    patched_info = avb.extract_image_avb_info(target_boot)

    for key in ["algorithm", "name", "rollback", "salt"]:
        assert patched_info.get(key) == boot_info.get(key)

    assert int(patched_info["partition_size"]) >= int(
        patched_info.get("data_size", patched_info["partition_size"])
    )


def test_patch_chained_image_rollback_noop(fw_pkg, tmp_path):
    init_boot = fw_pkg.get("init_boot.img")
    if not init_boot:
        pytest.skip("init_boot.img not found in firmware package")

    source = tmp_path / "init_boot.img"
    patched = tmp_path / "init_boot_patched.img"
    shutil.copy(init_boot, source)

    info = avb.extract_image_avb_info(source)
    current_rb = int(info.get("rollback", "0"))

    avb.patch_chained_image_rollback("init_boot", current_rb, source, patched)

    assert patched.exists()
    assert patched.read_bytes() == source.read_bytes()


def test_patch_boot_with_magisk_uses_stub_xz_name(tmp_path):
    from ltbox.patch import root as root_patch

    work_dir = tmp_path / "work"
    work_dir.mkdir()
    (work_dir / "init_boot.img").write_bytes(b"img")
    (work_dir / "magiskinit").write_bytes(b"magiskinit")
    (work_dir / "magisk").write_bytes(b"magisk")
    (work_dir / "init-ld").write_bytes(b"init-ld")
    (work_dir / "stub.apk").write_bytes(b"stub")

    calls = []

    class FakeMB:
        def __init__(self, exe):
            self.exe = exe

        def run(self, *args, cwd=None, check=True, capture=False):
            calls.append(args)
            if args[:2] == ("unpack", "init_boot.img"):
                (cwd / "ramdisk.cpio").write_bytes(b"cpio")
                return None
            if args[:3] == ("cpio", "ramdisk.cpio", "exists init"):

                class Proc:
                    returncode = 0
                    stdout = ""

                return Proc()
            if args[:2] == ("sha1", "init_boot.img"):

                class Proc:
                    returncode = 0
                    stdout = "deadbeef\n"

                return Proc()
            if args[:2] == ("cpio", "ramdisk.cpio") and any(
                item == "add 0644 overlay.d/sbin/stub.xz stub.xz" for item in args
            ):
                assert (cwd / "stub.xz").exists()
                assert not (cwd / "stub.apk.xz").exists()
                return None
            if args[:2] == ("repack", "init_boot.img"):
                (cwd / "new-boot.img").write_bytes(b"patched")
                return None
            return None

    with (
        patch("ltbox.patch.root.utils.MagiskBootWrapper", FakeMB),
        patch("ltbox.patch.root.const.BASE_DIR", tmp_path),
    ):
        patched_path = root_patch.patch_boot_with_root_algo(
            work_dir=work_dir,
            magiskboot_exe=tmp_path / "magiskboot.exe",
            root_type="magisk",
            skip_lkm_download=True,
        )

    assert patched_path == tmp_path / "init_boot.root.img"
    assert patched_path.exists()
    assert not (work_dir / "stub.xz").exists()
    assert not (work_dir / "magisk.xz").exists()
    assert not (work_dir / "init-ld.xz").exists()
    assert any(
        call[:2] == ("cpio", "ramdisk.cpio")
        and "add 0644 overlay.d/sbin/stub.xz stub.xz" in call
        for call in calls
    )
