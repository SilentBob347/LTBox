import xml.etree.ElementTree as ET
from unittest.mock import patch

import pytest

from ltbox.actions import edl
from ltbox.actions import xml as xml_action


def test_patch_xml_for_keep_data_clears_userdata_and_metadata(tmp_path):
    xml_path = tmp_path / "rawprogram_save_persist_unsparse0.xml"
    xml_path.write_text(
        """
<data>
  <program label="metadata" filename="metadata.img"/>
  <program label="userdata" filename="userdata.img"/>
  <program label="system" filename="system.img"/>
</data>
""".strip(),
        encoding="utf-8",
    )

    with patch("ltbox.actions.xml.utils.ui"):
        xml_action._patch_xml_for_wipe(xml_path, wipe=0)

    root = ET.parse(xml_path).getroot()
    entries = {p.get("label"): p.get("filename") for p in root.findall("program")}
    assert entries["metadata"] == ""
    assert entries["userdata"] == ""
    assert entries["system"] == "system.img"


def test_patch_xml_for_wipe_data_keeps_userdata_and_metadata(tmp_path):
    xml_path = tmp_path / "rawprogram_save_persist_unsparse0.xml"
    xml_path.write_text(
        """
<data>
  <program label="metadata" filename="metadata.img"/>
  <program label="userdata" filename="userdata.img"/>
  <program label="system" filename="system.img"/>
</data>
""".strip(),
        encoding="utf-8",
    )

    with patch("ltbox.actions.xml.utils.ui"):
        xml_action._patch_xml_for_wipe(xml_path, wipe=1)

    root = ET.parse(xml_path).getroot()
    entries = {p.get("label"): p.get("filename") for p in root.findall("program")}
    assert entries["metadata"] == "metadata.img"
    assert entries["userdata"] == "userdata.img"
    assert entries["system"] == "system.img"


def test_select_flash_xmls_uses_patched_dp_xmls_once(mock_env):
    img_dir = mock_env["IMAGE_DIR"]
    out_dp = mock_env["OUTPUT_DP_DIR"]

    (img_dir / "rawprogram1.xml").write_text("<data/>", encoding="utf-8")
    (img_dir / "rawprogram_save_persist_unsparse0.xml").write_text(
        '<data><program label="persist" filename=""/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram_write_persist_unsparse0.xml").write_text(
        '<data><program label="persist" filename="persist.img"/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram4.xml").write_text(
        '<data><program label="devinfo" filename=""/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram4_write_devinfo.xml").write_text(
        '<data><program label="devinfo" filename="devinfo.img"/></data>',
        encoding="utf-8",
    )
    (img_dir / "patch0.xml").write_text("<patches/>", encoding="utf-8")
    (out_dp / "persist.img").write_bytes(b"persist")
    (out_dp / "devinfo.img").write_bytes(b"devinfo")

    with patch("ltbox.actions.edl.utils.ui"):
        raw, _patch_files = edl._select_flash_xmls(skip_dp=False)

    r_names = [p.name for p in raw]
    assert "rawprogram_write_persist_unsparse0.xml" in r_names
    assert "rawprogram4_write_devinfo.xml" in r_names
    assert "rawprogram_save_persist_unsparse0.xml" not in r_names
    assert "rawprogram4.xml" not in r_names
    assert r_names.count("rawprogram_write_persist_unsparse0.xml") == 1
    assert r_names.count("rawprogram4_write_devinfo.xml") == 1
    assert len(r_names) == len(set(r_names))


def test_select_flash_xmls_uses_safe_dp_xmls_when_dp_is_skipped(mock_env):
    img_dir = mock_env["IMAGE_DIR"]

    (img_dir / "rawprogram1.xml").write_text("<data/>", encoding="utf-8")
    (img_dir / "rawprogram_save_persist_unsparse0.xml").write_text(
        '<data><program label="persist" filename=""/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram_write_persist_unsparse0.xml").write_text(
        '<data><program label="persist" filename="persist.img"/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram4.xml").write_text(
        '<data><program label="devinfo" filename=""/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram4_write_devinfo.xml").write_text(
        '<data><program label="devinfo" filename="devinfo.img"/></data>',
        encoding="utf-8",
    )
    (img_dir / "patch0.xml").write_text("<patches/>", encoding="utf-8")

    with patch("ltbox.actions.edl.utils.ui"):
        raw, _patch_files = edl._select_flash_xmls(skip_dp=True)

    r_names = [p.name for p in raw]
    assert "rawprogram_save_persist_unsparse0.xml" in r_names
    assert "rawprogram4.xml" in r_names
    assert "rawprogram_write_persist_unsparse0.xml" not in r_names
    assert "rawprogram4_write_devinfo.xml" not in r_names
    assert len(r_names) == len(set(r_names))


def test_select_flash_xmls_allows_independent_dp_patch_files(mock_env):
    img_dir = mock_env["IMAGE_DIR"]
    out_dp = mock_env["OUTPUT_DP_DIR"]

    (img_dir / "rawprogram1.xml").write_text("<data/>", encoding="utf-8")
    (img_dir / "rawprogram_save_persist_unsparse0.xml").write_text(
        '<data><program label="persist" filename=""/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram_write_persist_unsparse0.xml").write_text(
        '<data><program label="persist" filename="persist.img"/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram4.xml").write_text(
        '<data><program label="devinfo" filename=""/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram4_write_devinfo.xml").write_text(
        '<data><program label="devinfo" filename="devinfo.img"/></data>',
        encoding="utf-8",
    )
    (img_dir / "patch0.xml").write_text("<patches/>", encoding="utf-8")
    (out_dp / "devinfo.img").write_bytes(b"devinfo")

    with patch("ltbox.actions.edl.utils.ui"):
        raw, _patch_files = edl._select_flash_xmls(skip_dp=False)

    r_names = [p.name for p in raw]
    assert "rawprogram_save_persist_unsparse0.xml" in r_names
    assert "rawprogram_write_persist_unsparse0.xml" not in r_names
    assert "rawprogram4_write_devinfo.xml" in r_names
    assert "rawprogram4.xml" not in r_names
    assert len(r_names) == len(set(r_names))


def test_select_flash_xmls_rejects_unpatched_dp_references(mock_env):
    img_dir = mock_env["IMAGE_DIR"]

    (img_dir / "rawprogram1.xml").write_text("<data/>", encoding="utf-8")
    (img_dir / "rawprogram_save_persist_unsparse0.xml").write_text(
        '<data><program label="persist" filename="persist.img"/></data>',
        encoding="utf-8",
    )
    (img_dir / "rawprogram4.xml").write_text(
        '<data><program label="devinfo" filename=""/></data>',
        encoding="utf-8",
    )
    (img_dir / "patch0.xml").write_text("<patches/>", encoding="utf-8")

    with (
        patch("ltbox.actions.edl.utils.ui"),
        pytest.raises(RuntimeError, match="persist.img"),
    ):
        edl._select_flash_xmls(skip_dp=True)
