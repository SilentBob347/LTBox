import subprocess
import sys
from typing import TYPE_CHECKING, Any, List, Optional, Tuple

from . import i18n, update_service, utils
from .app_state import AppState
from .i18n import get_string
from .registry import CommandRegistry
from .scan_api import run_info_scan
from .settings_store import (
    SETTINGS_STORE,
    SettingsStore,
)
from .startup_checks import (
    acquire_single_instance_mutex,
    check_path_encoding,
    check_platform as _check_platform_impl,
    ensure_admin_or_exit as _ensure_admin_or_exit_impl,
    force_kill_processes as _force_kill_processes_impl,
    get_running_processes as _get_running_processes_impl,
    is_running_as_admin as _is_running_as_admin_impl,
    resolve_process_conflicts as _resolve_process_conflicts_impl,
    setup_console,
)
from .utils import ui

if TYPE_CHECKING:
    from .menu_router import DeviceControllerFactoryProtocol

try:
    from .errors import LTBoxError, ToolError
except ImportError:
    print(get_string("err_import_critical"), file=sys.stderr)
    print(get_string("err_ensure_errors"), file=sys.stderr)
    input(get_string("press_enter_to_exit"))
    sys.exit(1)


# --- Menus ---


def _resolve_language_code(
    is_info_mode: bool, settings_store: SettingsStore = SETTINGS_STORE
) -> str:
    from .menu_router import prompt_for_language

    return "en" if is_info_mode else prompt_for_language(settings_store=settings_store)


def _initialize_runtime(
    lang_code: str,
) -> Tuple["DeviceControllerFactoryProtocol", CommandRegistry, Any]:
    utils.check_dependencies()

    from . import constants, device
    from .menu_router import prompt_for_language
    from .registry import REGISTRY
    from .commands import register_all_commands

    @REGISTRY.register("change_language", get_string("lang_changed"), require_dev=False)
    def change_language_task(breadcrumbs: Optional[str] = None):
        new_lang = prompt_for_language(
            force_prompt=True, settings_store=SETTINGS_STORE, breadcrumbs=breadcrumbs
        )
        i18n.load_lang(new_lang)
        return get_string("lang_changed")

    register_all_commands()

    return device.DeviceController, REGISTRY, constants


def _run_entry_mode(
    is_info_mode: bool,
    device_controller_class: "DeviceControllerFactoryProtocol",
    registry: CommandRegistry,
    constants_module: Any,
    settings_store: Optional[SettingsStore] = None,
) -> None:
    check_path_encoding()

    if is_info_mode:
        if len(sys.argv) > 2:
            run_info_scan(sys.argv[2:], constants_module)
        else:
            ui.error(get_string("info_no_files_dragged"))
            ui.error(get_string("info_drag_files_prompt"))

        input(get_string("press_enter_to_exit"))
    else:
        if settings_store is None:
            settings_store = SETTINGS_STORE

        from .menu_router import main_loop

        settings = settings_store.load()
        final_state = main_loop(
            device_controller_class,
            registry,
            initial_state=AppState(
                target_region=settings.target_region,
                modify_region_code=settings.modify_region_code,
                preset_code=settings.preset_code,
                modify_rollback_index=settings.modify_rollback_index,
                language=settings.language,
            ),
        )
        settings_store.update(
            target_region=final_state.target_region,
            modify_region_code=final_state.modify_region_code,
            preset_code=final_state.preset_code,
            modify_rollback_index=final_state.modify_rollback_index,
            language=final_state.language,
        )


# --- Singleton Check ---


def _check_platform() -> None:
    _check_platform_impl()


def _acquire_single_instance_mutex() -> Optional[Any]:
    return acquire_single_instance_mutex()


# --- Entry Point ---


def _prepare_environment() -> Any:
    _check_platform()
    setup_console()
    return _acquire_single_instance_mutex()


def _setup_language(is_info_mode: bool) -> str:
    lang_code = _resolve_language_code(is_info_mode, settings_store=SETTINGS_STORE)
    i18n.load_lang(lang_code)
    return lang_code


def _check_updates() -> None:
    ui.clear()
    current_version, latest_version, _, _ = update_service.get_update_status()
    update_service.prompt_for_update(current_version, latest_version)


def _is_running_as_admin() -> bool:
    return _is_running_as_admin_impl()


def _ensure_admin_or_exit() -> None:
    _ensure_admin_or_exit_impl()


def _force_kill_processes(exe_names: List[str]) -> None:
    _force_kill_processes_impl(exe_names)


def _get_running_processes(exe_names: List[str]) -> List[str]:
    return _get_running_processes_impl(exe_names)


def _resolve_process_conflicts() -> None:
    _resolve_process_conflicts_impl(
        get_running_processes=_get_running_processes,
        force_kill_processes=_force_kill_processes,
    )


def _init_and_run(is_info_mode: bool, lang_code: str) -> None:
    try:
        (
            device_controller_class,
            registry,
            constants_module,
        ) = _initialize_runtime(lang_code)

        _run_entry_mode(
            is_info_mode,
            device_controller_class,
            registry,
            constants_module,
            settings_store=SETTINGS_STORE,
        )
    except (subprocess.CalledProcessError, FileNotFoundError, ToolError) as e:
        ui.error(get_string("critical_err_base_tools").format(e=e))
        ui.error(get_string("err_run_install_manually"))
        input(get_string("press_enter_to_exit"))
        sys.exit(1)
    except ImportError as e:
        ui.error(get_string("err_import_ltbox"))
        ui.error(get_string("err_details").format(e=e))
        ui.error(get_string("err_ensure_ltbox_present"))
        input(get_string("press_enter_to_exit"))
        sys.exit(1)


def entry_point() -> None:
    try:
        is_info_mode = len(sys.argv) > 1 and sys.argv[1].lower() == "info"
        singleton_mutex = _prepare_environment()
        lang_code = _setup_language(is_info_mode)

        if is_info_mode:
            _init_and_run(is_info_mode, lang_code)
            return

        if not singleton_mutex:
            ui.clear()
            ui.error(get_string("err_already_running"))
            input()
            sys.exit(0)

        _ensure_admin_or_exit()
        _resolve_process_conflicts()
        _check_updates()
        _init_and_run(is_info_mode, lang_code)

    except (LTBoxError, RuntimeError) as e:
        ui.error(get_string("err_fatal_abort"))
        ui.error(get_string("err_details").format(e=e))
        input(get_string("press_enter_to_exit"))
        sys.exit(1)
    except KeyboardInterrupt:
        ui.error(get_string("err_fatal_user_cancel"))
        sys.exit(0)


if __name__ == "__main__":
    entry_point()
