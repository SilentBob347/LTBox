import sys
from datetime import datetime
from pathlib import Path
from typing import Any, List

from .i18n import get_string
from .logger import logging_context
from .utils import ui


def collect_info_scan_files(paths: List[str]) -> List[Path]:
    files_to_scan: List[Path] = []

    for path_str in paths:
        candidate = Path(path_str)
        if candidate.is_dir():
            files_to_scan.extend(candidate.rglob("*.img"))
        elif candidate.is_file() and candidate.suffix.lower() == ".img":
            files_to_scan.append(candidate)

    return files_to_scan


def build_info_scan_command(image_path: Path, constants: Any) -> List[str]:
    return [
        str(constants.PYTHON_EXE),
        str(constants.AVBTOOL_PY),
        "info_image",
        "--image",
        str(image_path),
    ]


def run_info_scan(paths: List[str], constants: Any, avb_patch: Any) -> None:
    print(get_string("scan_start"))

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_dir = constants.BASE_DIR / "log"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_filename = log_dir / f"image_info_{timestamp}.txt"

    files_to_scan = collect_info_scan_files(paths)

    if not files_to_scan:
        print(get_string("scan_no_files"), file=sys.stderr)
        return

    print(get_string("scan_found_files").format(count=len(files_to_scan)))

    with logging_context(log_filename) as logger:
        for image_path in files_to_scan:
            header = get_string("scan_log_header").format(path=image_path.resolve())
            logger.info(header)
            print(get_string("scan_scanning_file").format(filename=image_path.name))

            try:
                command = build_info_scan_command(image_path, constants)
                result = avb_patch.utils.run_command(command, capture=True, check=False)

                logger.info(result.stdout.strip())

                if result.stderr:
                    logger.info(
                        get_string("scan_log_errors").format(
                            errors=result.stderr.strip()
                        )
                    )

                logger.info("\n" + "=" * ui.get_term_width() + "\n")
            except (OSError, RuntimeError, ValueError, AttributeError) as error:
                error_msg = get_string("scan_failed").format(
                    filename=image_path.name,
                    e=error,
                )
                print(error_msg, file=sys.stderr)
                logger.info(error_msg)

    print(get_string("scan_complete"))
    print(get_string("scan_saved_to").format(filename=log_filename.name))
