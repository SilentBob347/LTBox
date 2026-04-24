import importlib.util
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BUNDLE_TOOLS_SCRIPT = REPO_ROOT / ".github" / "scripts" / "release" / "bundle-tools.py"


def _load_bundle_tools():
    spec = importlib.util.spec_from_file_location("bundle_tools", BUNDLE_TOOLS_SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load {BUNDLE_TOOLS_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build():
    print("[INFO] Downloading prebuilt CI tools into bin/tools...")
    _load_bundle_tools().main()


if __name__ == "__main__":
    build()
