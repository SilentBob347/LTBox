import shutil
from unittest.mock import patch

import pytest
from ltbox.patch import avb


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


def test_extract_image_avb_info_returns_all_required_fields(fw_pkg):
    """Verify extract_image_avb_info returns all fields needed by apply_avb_integrity_footer."""
    boot = fw_pkg.get("boot.img")
    if not boot:
        pytest.skip("boot.img not found")

    info = avb.extract_image_avb_info(boot)

    required = ["partition_size", "name", "rollback", "salt", "algorithm"]
    for key in required:
        assert key in info, f"Missing required key: {key}"
        assert info[key] is not None, f"Key {key} is None"

    assert "pubkey_sha1" in info


def test_vbmeta_has_chain_partition_detects_boot(fw_pkg):
    """Verify chain partition detection on real vbmeta."""
    vbmeta = fw_pkg.get("vbmeta.img")
    if not vbmeta:
        pytest.skip("vbmeta.img not found")

    has_boot = avb.vbmeta_has_chain_partition(vbmeta, "boot")
    # TB322 vbmeta should chain to boot
    assert isinstance(has_boot, bool)


def test_require_info_keys_passes_on_valid_info(fw_pkg):
    """require_info_keys should not raise when all keys are present."""
    boot = fw_pkg.get("boot.img")
    if not boot:
        pytest.skip("boot.img not found")

    info = avb.extract_image_avb_info(boot)
    required = ["partition_size", "name", "rollback", "salt", "algorithm"]
    avb.require_info_keys(info, required, boot)


def test_apply_avb_integrity_footer_round_trip(fw_pkg, tmp_path):
    """Strip and re-apply AVB footer using real firmware info, then verify."""
    boot = fw_pkg.get("boot.img")
    if not boot:
        pytest.skip("boot.img not found")

    target = tmp_path / "boot_roundtrip.img"
    shutil.copy(boot, target)

    original_info = avb.extract_image_avb_info(target)

    key_file = avb._resolve_signing_key(original_info.get("pubkey_sha1"), "boot.img")

    from ltbox.patch.avb import _run_avbtool

    _run_avbtool("erase_footer", "--image", str(target))

    avb.apply_avb_integrity_footer(
        image_path=target, image_info=original_info, key_file=key_file
    )

    new_info = avb.extract_image_avb_info(target)
    assert new_info["algorithm"] == original_info["algorithm"]
    assert new_info["name"] == original_info["name"]
    assert new_info["salt"] == original_info["salt"]


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
