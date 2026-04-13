import shutil
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from ltbox.actions import edl
from ltbox.part import partition
from ltbox.part.service import EdlPartitionService
from ltbox.part.xml_catalog import XmlCatalog, _parse_xml_records


def test_require_partition_params_raises_on_missing():
    with (
        patch(
            "ltbox.part.partition.scan_and_decrypt_xmls",
            return_value=[Path("dummy.xml")],
        ),
        patch("ltbox.part.partition.get_partition_params", return_value=None),
    ):
        with pytest.raises(ValueError):
            partition.require_partition_params("nonexistent_label")


def test_xml_catalog_groups_ab_and_non_ab_entries(tmp_path):
    xml_path = tmp_path / "rawprogram0.xml"
    xml_path.write_text(
        """<?xml version='1.0'?><data>
        <program label='boot_a' filename='boot_a.img' physical_partition_number='0' start_sector='100'/>
        <program label='boot_b' filename='' physical_partition_number='0' start_sector='200'/>
        <program label='super' filename='super.img' physical_partition_number='0' start_sector='300'/>
        <program label='persist' filename='' physical_partition_number='0' start_sector='400'/>
        </data>""",
        encoding="utf-8",
    )

    catalog = XmlCatalog.from_paths([xml_path])
    groups = catalog.group_by_base_label(with_files_only=True)

    assert sorted(groups.keys()) == ["boot", "super"]
    assert groups["boot"].is_ab is True
    assert groups["boot"].a[0].filename == "boot_a.img"
    assert groups["boot"].b[0].start_sector == "200"
    assert groups["super"].is_ab is False
    assert groups["super"].none[0].filename == "super.img"


def test_xml_catalog_reuses_cached_parse_for_unchanged_file(tmp_path):
    xml_path = tmp_path / "rawprogram0.xml"
    xml_path.write_text(
        """<?xml version='1.0'?><data>
        <program label='boot_a' filename='boot_a.img' physical_partition_number='0' start_sector='100'/>
        </data>""",
        encoding="utf-8",
    )

    _parse_xml_records.cache_clear()

    with patch("ltbox.part.xml_catalog.ET.parse", wraps=ET.parse) as mock_parse:
        XmlCatalog.from_paths([xml_path])
        XmlCatalog.from_paths([xml_path])

    assert mock_parse.call_count == 1


def test_xml_catalog_invalidates_cache_when_file_changes(tmp_path):
    xml_path = tmp_path / "rawprogram0.xml"
    xml_path.write_text(
        """<?xml version='1.0'?><data>
        <program label='boot_a' filename='boot_a.img' physical_partition_number='0' start_sector='100'/>
        </data>""",
        encoding="utf-8",
    )

    _parse_xml_records.cache_clear()

    with patch("ltbox.part.xml_catalog.ET.parse", wraps=ET.parse) as mock_parse:
        first_catalog = XmlCatalog.from_paths([xml_path])
        xml_path.write_text(
            """<?xml version='1.0'?><data>
            <program label='boot_a' filename='boot_new.img' physical_partition_number='0' start_sector='100'/>
            </data>""",
            encoding="utf-8",
        )
        second_catalog = XmlCatalog.from_paths([xml_path])

    assert mock_parse.call_count == 2
    assert first_catalog.find_partition("boot_a").filename == "boot_a.img"
    assert second_catalog.find_partition("boot_a").filename == "boot_new.img"


def _copy_firmware_xml(fw_pkg, image_dir):
    candidates = [
        "rawprogram_unsparse0.xml",
        "rawprogram_save_persist_unsparse0.xml",
    ]
    for name in candidates:
        src = fw_pkg.get(name)
        if src:
            dest = image_dir / name
            shutil.copy(src, dest)
            return dest
    return None


def _get_first_program(xml_path):
    root = ET.parse(xml_path).getroot()
    program = next((p for p in root.findall("program") if p.get("label")), None)
    if program is None:
        pytest.skip("No program entries found in firmware XML")
    return program


@pytest.mark.integration
def test_partition_params_from_firmware_xml(fw_pkg, mock_env):
    if not fw_pkg:
        pytest.skip("Firmware package not available")

    xml_path = _copy_firmware_xml(fw_pkg, mock_env["IMAGE_DIR"])
    if not xml_path:
        pytest.skip("Firmware XML not found")

    program = _get_first_program(xml_path)
    label = program.get("label")

    params = partition.require_partition_params(label)

    assert params["source_xml"] == xml_path.name
    assert params["lun"] == program.get("physical_partition_number")
    assert params["start_sector"] == program.get("start_sector")


@pytest.mark.integration
def test_flash_partition_target_uses_firmware_params(fw_pkg, mock_env):
    if not fw_pkg:
        pytest.skip("Firmware package not available")

    xml_path = _copy_firmware_xml(fw_pkg, mock_env["IMAGE_DIR"])
    if not xml_path:
        pytest.skip("Firmware XML not found")

    program = _get_first_program(xml_path)
    label = program.get("label")

    image_path = mock_env["OUTPUT_DP_DIR"] / "patched.img"
    image_path.write_bytes(b"test")

    mock_dev = MagicMock()

    with patch("ltbox.actions.edl.utils.ui"):
        edl.flash_partition_target(mock_dev, "COM3", label, image_path)

    mock_dev.edl.write_partition.assert_called_once_with(
        port="COM3",
        image_path=image_path,
        lun=program.get("physical_partition_number"),
        start_sector=program.get("start_sector"),
        partition_name=label,
    )


def test_edl_partition_service_logs_single_flash_line(tmp_path):
    service = EdlPartitionService(
        resolve_params=lambda _label: {
            "source_xml": "rawprogram0.xml",
            "lun": "4",
            "start_sector": "113318",
            "num_sectors": "1",
        }
    )
    image_path = tmp_path / "boot.img"
    image_path.write_bytes(b"boot")
    dev = MagicMock()

    messages = {
        "device_flashing_part": '[*] Flashing {filename} -> LUN="{lun}", start_sector="{start_sector}"...',
        "act_flash_img": "[+] Flashed '{filename}' to {part}.",
    }

    with (
        patch("ltbox.part.service.get_string", side_effect=messages.__getitem__),
        patch("ltbox.part.service.ui") as mock_ui,
    ):
        service.flash_partition(dev, "COM3", "boot_a", image_path)

    echoed_messages = [call.args[0] for call in mock_ui.echo.call_args_list]
    assert echoed_messages == [
        messages["device_flashing_part"].format(
            filename="boot.img",
            lun="4",
            start_sector="113318",
        ),
        messages["act_flash_img"].format(filename="boot.img", part="boot_a"),
    ]
