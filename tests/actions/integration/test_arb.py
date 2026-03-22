import shutil
from unittest.mock import patch

import pytest
from ltbox import constants as const
from ltbox.actions.arb import ArbStatus, read_anti_rollback


pytestmark = pytest.mark.integration


def test_read_anti_rollback_with_real_firmware(firmware_file_getter, tmp_path):
    boot_img, vbmeta_system_img = firmware_file_getter("boot.img", "vbmeta_system.img")

    dumped_boot = tmp_path / "boot.img"
    dumped_vbmeta = tmp_path / "vbmeta_system.img"
    shutil.copy(boot_img, dumped_boot)
    shutil.copy(vbmeta_system_img, dumped_vbmeta)

    new_boot = tmp_path / "new_boot.img"
    new_vbmeta_sys = tmp_path / "new_vbmeta_system.img"
    shutil.copy(boot_img, new_boot)
    shutil.copy(vbmeta_system_img, new_vbmeta_sys)

    with (
        patch("ltbox.actions.arb.utils.ui"),
        patch.object(const, "IMAGE_DIR", tmp_path),
        patch.object(const, "FN_BOOT", "new_boot.img"),
        patch.object(const, "FN_VBMETA_SYSTEM", "new_vbmeta_system.img"),
    ):
        status, boot_rb, vbmeta_rb = read_anti_rollback(
            dumped_boot_path=dumped_boot,
            dumped_vbmeta_path=dumped_vbmeta,
        )

    assert status == ArbStatus.MATCH
    assert isinstance(boot_rb, int)
    assert isinstance(vbmeta_rb, int)
