import shutil
from unittest.mock import MagicMock

import pytest
from ltbox.actions import region


pytestmark = pytest.mark.integration


def test_convert_region_round_trip(firmware_file_getter, mock_env):
    real_vb, real_vbmeta = firmware_file_getter("vendor_boot.img", "vbmeta.img")

    img_dir = mock_env["IMAGE_DIR"]
    output_dir = mock_env["OUTPUT_DIR"]

    shutil.copy(real_vb, img_dir / "vendor_boot.img")
    shutil.copy(real_vbmeta, img_dir / "vbmeta.img")

    mock_dev = MagicMock()
    mock_dev.skip_adb = True

    region.convert_region_images(dev=mock_dev, target_region="ROW", on_log=print)

    out_vb = output_dir / "vendor_boot.img"
    out_vbmeta = output_dir / "vbmeta.img"

    assert out_vb.exists()
    assert out_vbmeta.exists()

    # Convert back to PRC
    shutil.copy(out_vb, img_dir / "vendor_boot.img")
    shutil.copy(out_vbmeta, img_dir / "vbmeta.img")

    region.convert_region_images(dev=mock_dev, target_region="PRC", on_log=print)

    assert out_vb.exists()
    assert out_vbmeta.exists()
    assert out_vb.stat().st_size > 0
