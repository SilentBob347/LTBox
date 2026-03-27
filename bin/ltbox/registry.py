from dataclasses import dataclass, field
from typing import Any, Callable, Dict, Optional


@dataclass(frozen=True)
class CommandSpec:
    func: Callable[..., Any]
    title: str
    require_dev: bool = True
    default_kwargs: Dict[str, Any] = field(default_factory=dict)
    result_handler: Optional[Callable[[Any], Any]] = None
    log_filename_prefix: Optional[str] = None

    def __getitem__(self, key: str) -> Any:
        try:
            return getattr(self, key)
        except AttributeError as exc:
            raise KeyError(key) from exc


class CommandRegistry:
    def __init__(self):
        self._commands: Dict[str, CommandSpec] = {}

    def register(
        self,
        name: str,
        title: str,
        require_dev: bool = True,
        result_handler: Optional[Callable[[Any], Any]] = None,
        log_filename_prefix: Optional[str] = None,
        **default_kwargs,
    ):
        def decorator(func: Callable[..., Any]) -> Callable[..., Any]:
            self._commands[name] = CommandSpec(
                func=func,
                title=title,
                require_dev=require_dev,
                default_kwargs=default_kwargs,
                result_handler=result_handler,
                log_filename_prefix=log_filename_prefix,
            )
            return func

        return decorator

    def add(
        self,
        name: str,
        func: Callable[..., Any],
        title: str,
        require_dev: bool = True,
        result_handler: Optional[Callable[[Any], Any]] = None,
        log_filename_prefix: Optional[str] = None,
        **default_kwargs: Any,
    ):
        self.register(
            name,
            title,
            require_dev=require_dev,
            result_handler=result_handler,
            log_filename_prefix=log_filename_prefix,
            **default_kwargs,
        )(func)

    def get(self, name: str) -> Optional[CommandSpec]:
        return self._commands.get(name)


REGISTRY = CommandRegistry()
