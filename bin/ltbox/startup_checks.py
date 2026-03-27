import os
import platform
import subprocess
import sys
from pathlib import Path
from typing import Any, Callable, List, Optional

from .i18n import get_string
from .utils import ui


def _abort_platform_check(messages: List[str]) -> None:
    for message in messages:
        print(message, file=sys.stderr)
    print(get_string("err_aborting"), file=sys.stderr)
    input(get_string("press_enter_to_exit"))
    sys.exit(1)


def check_platform() -> None:
    if platform.system() != "Windows":
        _abort_platform_check(
            [
                get_string("err_fatal_windows"),
                get_string("err_current_platform").format(platform=platform.system()),
            ]
        )

    if platform.machine() != "AMD64":
        _abort_platform_check(
            [
                get_string("err_fatal_amd64"),
                get_string("err_current_arch").format(arch=platform.machine()),
                get_string("err_arch_unsupported"),
            ]
        )


def setup_console() -> None:
    try:
        import ctypes

        if sys.platform == "win32":
            kernel32 = ctypes.windll.kernel32
            kernel32.SetConsoleTitleW("LTBox")

            std_input_handle = -10
            enable_quick_edit_mode = 0x0040
            enable_extended_flags = 0x0080

            stdin_handle = kernel32.GetStdHandle(std_input_handle)
            mode = ctypes.c_uint32()
            if kernel32.GetConsoleMode(stdin_handle, ctypes.byref(mode)):
                mode.value &= ~enable_quick_edit_mode
                mode.value |= enable_extended_flags
                kernel32.SetConsoleMode(stdin_handle, mode)

        sys.stdout.write("\x1b[8;40;80t")
        sys.stdout.flush()

        os.system("mode con: cols=80 lines=40")

    except (ImportError, OSError, AttributeError) as error:
        print(get_string("warn_set_console_title").format(e=error), file=sys.stderr)


def check_path_encoding() -> None:
    current_path = str(Path(__file__).parent.parent.resolve())
    if not current_path.isascii():
        ui.clear()
        width = ui.get_term_width()
        ui.box_output(
            [
                get_string("critical_error_path_encoding"),
                "-" * width,
                get_string("current_path").format(current_path=current_path),
                "-" * width,
                get_string("path_encoding_details_1"),
                get_string("path_encoding_details_2"),
                "",
                get_string("action_required"),
                get_string("action_required_details"),
                get_string("example_path"),
            ],
            err=True,
        )

        input(get_string("press_enter_to_continue"))
        raise RuntimeError(get_string("critical_error_path_encoding"))


def acquire_single_instance_mutex() -> Optional[Any]:
    import ctypes

    if sys.platform != "win32":
        return "Non-Windows-Mutex"

    windll = getattr(ctypes, "windll", None)
    if windll is None:
        return None

    kernel32 = windll.kernel32
    mutex_name = "Global\\LTBox_Singleton_Mutex"

    mutex = kernel32.CreateMutexW(None, False, mutex_name)

    if kernel32.GetLastError() == 183:
        return None

    return mutex


def is_running_as_admin() -> bool:
    if os.name != "nt":
        return True

    try:
        import ctypes

        windll = getattr(ctypes, "windll", None)
        if windll is None:
            return False

        return bool(windll.shell32.IsUserAnAdmin())
    except (ImportError, OSError, AttributeError):
        return False


def ensure_admin_or_exit() -> None:
    if is_running_as_admin():
        return

    ui.clear()
    ui.error(get_string("startup_admin_required"))
    input(get_string("press_enter_to_exit"))
    sys.exit(0)


def force_kill_processes(exe_names: List[str]) -> None:
    for exe_name in exe_names:
        try:
            subprocess.run(
                ["taskkill", "/F", "/IM", exe_name, "/T"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=(
                    getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0
                ),
            )
        except (subprocess.CalledProcessError, OSError):
            pass


def get_running_processes(exe_names: List[str]) -> List[str]:
    if os.name != "nt":
        return []
    try:
        result = subprocess.run(
            ["tasklist"],
            capture_output=True,
            text=True,
            check=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        tasklist_output = result.stdout.lower()
        return [name for name in exe_names if name.lower() in tasklist_output]
    except (subprocess.CalledProcessError, OSError):
        return []


def resolve_process_conflicts(
    *,
    get_running_processes: Callable[[List[str]], List[str]],
    force_kill_processes: Callable[[List[str]], None],
) -> None:
    process_names = [
        "adb.exe",
        "fastboot.exe",
        "Software Fix.exe",
        "fh_loader.exe",
        "QSaharaServer.exe",
    ]
    running = get_running_processes(process_names)
    if not running:
        return

    ui.clear()
    running_list = ", ".join(running)
    ui.warn(
        get_string("startup_conflict_processes_prompt").format(processes=running_list)
    )
    choice = ui.prompt(get_string("startup_conflict_confirm")).strip().lower()
    if choice == "y":
        force_kill_processes(running)
        return

    ui.warn(get_string("startup_conflict_exit_message"))
    input(get_string("press_enter_to_exit"))
    sys.exit(0)
