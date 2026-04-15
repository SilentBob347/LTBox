from pathlib import Path
from types import SimpleNamespace

from ltbox import scan_api


def test_collect_info_scan_files_filters_img_only(tmp_path):
    image_dir = tmp_path / "images"
    nested = image_dir / "nested"
    nested.mkdir(parents=True)
    (image_dir / "boot.img").write_bytes(b"x")
    (nested / "vendor.img").write_bytes(b"x")
    non_img = tmp_path / "note.txt"
    non_img.write_text("skip")

    result = scan_api.collect_info_scan_files([str(image_dir), str(non_img)])

    names = sorted(path.name for path in result)
    assert names == ["boot.img", "vendor.img"]


def test_build_info_scan_command_uses_constants_paths():
    constants = SimpleNamespace(AVBTOOL_RS=Path("avbtool-rs.exe"))

    command = scan_api.build_info_scan_command(Path("boot.img"), constants)

    assert command == ["avbtool-rs.exe", "info_image", "--image", "boot.img"]


def test_run_info_scan_creates_log(tmp_path):
    image_dir = tmp_path / "images"
    image_dir.mkdir()
    (image_dir / "boot.img").write_bytes(b"fake")
    (image_dir / "vendor.img").write_bytes(b"fake")
    (image_dir / "ignore.txt").write_text("skip")

    extra_img = tmp_path / "extra.img"
    extra_img.write_bytes(b"fake")

    calls = []

    class FakeRunner:
        def run(self, cmd, options):
            calls.append((cmd, options.capture, options.check))
            return SimpleNamespace(stdout="FAKE-INFO", stderr="")

    constants = SimpleNamespace(
        BASE_DIR=tmp_path / "bin",
        AVBTOOL_RS=Path("avbtool-rs.exe"),
    )
    scan_api.run_info_scan(
        [str(image_dir), str(extra_img)],
        constants,
        runner=FakeRunner(),
    )

    assert len(calls) == 3
    assert all(capture is True and check is False for _, capture, check in calls)
    logs = list((tmp_path / "bin" / "log").glob("image_info_*.txt"))
    assert len(logs) == 1
    assert "FAKE-INFO" in logs[0].read_text(encoding="utf-8")
