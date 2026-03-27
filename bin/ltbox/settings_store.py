import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, ClassVar, Dict, Optional

from .i18n import get_string

APP_DIR = Path(__file__).parent.resolve()
SETTINGS_FILE = APP_DIR / "settings.json"


@dataclass(frozen=True)
class AppSettings:
    language: Optional[str] = None
    target_region: str = "PRC"
    modify_region_code: bool = True
    preset_code: str = "1"
    modify_rollback_index: str = "ON"

    _ALLOWED_TARGET_REGIONS: ClassVar[set[str]] = {"PRC", "ROW"}
    _ALLOWED_PRESET_CODES: ClassVar[set[str]] = {"1", "2", "3", "-"}
    _ALLOWED_MODIFY_RB: ClassVar[set[str]] = {"ON", "AUTO", "OFF"}

    @staticmethod
    def validate_language(value: Any) -> Optional[str]:
        return value if isinstance(value, str) else None

    @classmethod
    def validate_target_region(cls, value: Any) -> str:
        return value if value in cls._ALLOWED_TARGET_REGIONS else "PRC"

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "AppSettings":
        target_region = cls.validate_target_region(data.get("target_region", "PRC"))
        modify_region_code = bool(data.get("modify_region_code", True))
        preset_code = data.get("preset_code", "1")
        if preset_code not in cls._ALLOWED_PRESET_CODES:
            preset_code = "1"
        modify_rollback_index = data.get("modify_rollback_index", "ON")
        if modify_rollback_index not in cls._ALLOWED_MODIFY_RB:
            modify_rollback_index = "ON"
        return cls(
            language=cls.validate_language(data.get("language")),
            target_region=target_region,
            modify_region_code=modify_region_code,
            preset_code=preset_code,
            modify_rollback_index=modify_rollback_index,
        )


class SettingsStore:
    _UPDATE_VALIDATORS: ClassVar[Dict[str, Callable[[Any], bool]]] = {
        "language": lambda value: isinstance(value, str),
        "target_region": lambda value: value in AppSettings._ALLOWED_TARGET_REGIONS,
        "modify_region_code": lambda value: isinstance(value, bool),
        "preset_code": lambda value: value in AppSettings._ALLOWED_PRESET_CODES,
        "modify_rollback_index": lambda value: value in AppSettings._ALLOWED_MODIFY_RB,
    }

    def __init__(self, path: Path):
        self._path = path

    def load_raw(self) -> Dict[str, Any]:
        if self._path.exists():
            try:
                with open(self._path, "r", encoding="utf-8") as file:
                    data = json.load(file)
                    return data if isinstance(data, dict) else {}
            except (json.JSONDecodeError, OSError):
                return {}
        return {}

    def load(self) -> AppSettings:
        return AppSettings.from_dict(self.load_raw())

    def _filter_valid_updates(self, updates: Dict[str, Any]) -> Dict[str, Any]:
        validated: Dict[str, Any] = {}
        for key, value in updates.items():
            validator = self._UPDATE_VALIDATORS.get(key)
            if validator and validator(value):
                validated[key] = value
        return validated

    def update(self, **updates: Any) -> AppSettings:
        data = self.load_raw()
        validated = self._filter_valid_updates(updates)

        if not validated:
            return AppSettings.from_dict(data)

        data.update(validated)
        try:
            with open(self._path, "w", encoding="utf-8") as file:
                json.dump(data, file, indent=2)
        except (OSError, TypeError, ValueError) as error:
            print(
                get_string("warn_save_settings_failed").format(e=error),
                file=sys.stderr,
            )
        return AppSettings.from_dict(data)


SETTINGS_STORE = SettingsStore(SETTINGS_FILE)
