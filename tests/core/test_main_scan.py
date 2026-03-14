from pathlib import Path
from types import SimpleNamespace

from ltbox import main


def test_collect_info_scan_files_filters_img_only(tmp_path):
    image_dir = tmp_path / "images"
    nested = image_dir / "nested"
    nested.mkdir(parents=True)
    (image_dir / "boot.img").write_bytes(b"x")
    (nested / "vendor.img").write_bytes(b"x")
    non_img = tmp_path / "note.txt"
    non_img.write_text("skip")

    result = main.collect_info_scan_files([str(image_dir), str(non_img)])

    names = sorted(path.name for path in result)
    assert names == ["boot.img", "vendor.img"]


def test_build_info_scan_command_uses_constants_paths():
    constants = SimpleNamespace(
        PYTHON_EXE=Path("python"), AVBTOOL_PY=Path("avbtool.py")
    )

    command = main.build_info_scan_command(Path("boot.img"), constants)

    assert command == ["python", "avbtool.py", "info_image", "--image", "boot.img"]


def test_run_info_scan_creates_log(tmp_path):
    image_dir = tmp_path / "images"
    image_dir.mkdir()
    (image_dir / "boot.img").write_bytes(b"fake")
    (image_dir / "vendor.img").write_bytes(b"fake")
    (image_dir / "ignore.txt").write_text("skip")

    extra_img = tmp_path / "extra.img"
    extra_img.write_bytes(b"fake")

    calls = []

    def fake_run_command(cmd, capture=True, check=False):
        calls.append(cmd)
        return SimpleNamespace(stdout="FAKE-INFO", stderr="")

    constants = SimpleNamespace(
        BASE_DIR=tmp_path / "bin",
        PYTHON_EXE=Path("python"),
        AVBTOOL_PY=Path("avbtool.py"),
    )
    avb_patch = SimpleNamespace(utils=SimpleNamespace(run_command=fake_run_command))

    main.run_info_scan([str(image_dir), str(extra_img)], constants, avb_patch)

    assert len(calls) == 3
    logs = list((tmp_path / "bin" / "log").glob("image_info_*.txt"))
    assert len(logs) == 1
    assert "FAKE-INFO" in logs[0].read_text(encoding="utf-8")
