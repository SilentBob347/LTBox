from typing import Any, Callable, Dict, List, Tuple

from . import actions, workflow
from .i18n import get_string
from .registry import REGISTRY
from .utils import ui


def _handle_read_anti_rollback_result(result: Any) -> None:
    if not isinstance(result, tuple):
        if result:
            ui.echo(get_string("act_unhandled_success_result").format(res=result))
        return

    ui.echo(get_string("act_arb_complete").format(status=result[0]))
    ui.echo(get_string("act_curr_boot_idx").format(idx=result[1]))
    ui.echo(get_string("act_curr_vbmeta_idx").format(idx=result[2]))


def register_all_commands() -> None:
    command_specs: List[Tuple[str, Callable[..., Any], str, bool, Dict[str, Any]]] = [
        (
            "convert",
            actions.convert_region_images,
            get_string("task_title_convert_rom"),
            True,
            {},
        ),
        (
            "root_device_gki",
            actions.root_device,
            get_string("task_title_root_gki"),
            True,
            {"gki": True},
        ),
        (
            "patch_root_image_file_gki",
            actions.patch_root_image_file,
            get_string("task_title_root_file_gki"),
            False,
            {"gki": True},
        ),
        (
            "patch_root_image_file_flash_gki",
            actions.patch_root_image_file_and_flash,
            get_string("task_title_root_file_gki"),
            True,
            {"gki": True},
        ),
        (
            "root_device_fp",
            actions.root_device,
            get_string("task_title_root_gki") + " (FolkPatch)",
            True,
            {"gki": True, "root_type": "folkpatch"},
        ),
        (
            "patch_root_image_file_fp",
            actions.patch_root_image_file,
            get_string("task_title_root_file_gki") + " (FolkPatch)",
            False,
            {"gki": True, "root_type": "folkpatch"},
        ),
        (
            "patch_root_image_file_flash_fp",
            actions.patch_root_image_file_and_flash,
            get_string("task_title_root_file_gki") + " (FolkPatch)",
            True,
            {"gki": True, "root_type": "folkpatch"},
        ),
        (
            "root_device_lkm",
            actions.root_device,
            get_string("task_title_root_lkm"),
            True,
            {"gki": False},
        ),
        (
            "patch_root_image_file_lkm",
            actions.patch_root_image_file,
            get_string("task_title_root_file_lkm"),
            False,
            {"gki": False},
        ),
        (
            "patch_root_image_file_flash_lkm",
            actions.patch_root_image_file_and_flash,
            get_string("task_title_root_file_lkm"),
            True,
            {"gki": False},
        ),
        (
            "unroot_device",
            actions.unroot_device,
            get_string("task_title_unroot"),
            True,
            {},
        ),
        (
            "sign_and_flash_twrp",
            actions.sign_and_flash_twrp,
            get_string("task_title_rec_flash"),
            True,
            {},
        ),
        (
            "disable_ota",
            actions.disable_ota,
            get_string("task_title_disable_ota"),
            True,
            {},
        ),
        (
            "rescue_ota",
            actions.rescue_after_ota,
            get_string("task_title_rescue"),
            True,
            {},
        ),
        (
            "edit_dp",
            actions.edit_devinfo_persist,
            get_string("task_title_patch_devinfo"),
            False,
            {},
        ),
        (
            "dump_partitions",
            actions.dump_partitions,
            get_string("task_title_dump_devinfo"),
            True,
            {},
        ),
        (
            "flash_partitions",
            actions.flash_partitions,
            get_string("task_title_write_devinfo"),
            True,
            {},
        ),
        (
            "read_anti_rollback",
            actions.read_anti_rollback_from_device,
            get_string("task_title_read_arb"),
            True,
            {},
        ),
        (
            "patch_anti_rollback",
            actions.patch_anti_rollback_in_rom,
            get_string("task_title_patch_arb"),
            False,
            {},
        ),
        (
            "write_anti_rollback",
            actions.write_anti_rollback,
            get_string("task_title_write_arb"),
            True,
            {},
        ),
        (
            "decrypt_xml",
            actions.decrypt_x_files,
            get_string("task_title_decrypt_xml"),
            False,
            {},
        ),
        (
            "modify_xml",
            actions.modify_xml,
            get_string("task_title_modify_xml_nowipe"),
            False,
            {"wipe": 0},
        ),
        (
            "modify_xml_wipe",
            actions.modify_xml,
            get_string("task_title_modify_xml_wipe"),
            False,
            {"wipe": 1},
        ),
        (
            "flash_full_firmware",
            actions.flash_full_firmware,
            get_string("task_title_flash_full_firmware"),
            True,
            {},
        ),
        (
            "flash_partition_labels",
            actions.flash_partition_labels,
            get_string("task_title_flash_partitions_label"),
            True,
            {},
        ),
        (
            "patch_all",
            workflow.patch_all,
            get_string("task_title_install_nowipe"),
            True,
            {"wipe": 0},
        ),
        (
            "patch_all_wipe",
            workflow.patch_all,
            get_string("task_title_install_wipe"),
            True,
            {"wipe": 1},
        ),
    ]

    result_handlers = {
        "read_anti_rollback": _handle_read_anti_rollback_result,
    }

    for name, func, title, require_dev, extra_kwargs in command_specs:
        REGISTRY.add(
            name,
            func,
            title,
            require_dev=require_dev,
            result_handler=result_handlers.get(name),
            **extra_kwargs,
        )
