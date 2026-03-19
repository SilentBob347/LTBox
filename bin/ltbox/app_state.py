from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class AppState:
    skip_adb: bool = False
    skip_rollback: bool = False
    modify_region_code: bool = True
    target_region: str = "PRC"
    language: Optional[str] = None
