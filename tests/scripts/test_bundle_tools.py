import importlib.util
import zipfile
from pathlib import Path


def _load_bundle_tools_module():
    script_path = (
        Path(__file__).resolve().parents[2]
        / ".github"
        / "scripts"
        / "release"
        / "bundle-tools.py"
    )
    spec = importlib.util.spec_from_file_location("bundle_tools", script_path)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_bundle_magiskboot_extracts_prebuilt_tools(tmp_path, monkeypatch):
    bundle_tools = _load_bundle_tools_module()
    tools_dir = tmp_path / "bin" / "tools"
    tools_dir.mkdir(parents=True)
    archive = tmp_path / "magiskboot.zip"

    files = [
        "magiskboot.exe",
        "magiskboot_xz_helper.exe",
        "openssl.exe",
        "libwinpthread-1.dll",
        "msys-2.0.dll",
        "msys-crypto-3.dll",
        "msys-ssl-3.dll",
        "msys-z.dll",
    ]
    with zipfile.ZipFile(archive, "w") as zf:
        for name in files:
            zf.writestr(f"LTBox-win_amd64-v2.6.7/bin/tools/{name}", name)

    monkeypatch.setattr(bundle_tools, "TOOLS_DIR", tools_dir)
    monkeypatch.setattr(
        bundle_tools,
        "_download",
        lambda _url, dest, _description: dest.write_bytes(archive.read_bytes()),
    )

    bundle_tools.bundle_magiskboot(
        {"archive_url": "https://example.test/LTBox.zip", "files": files}
    )

    for name in files:
        assert (tools_dir / name).read_text(encoding="utf-8") == name
