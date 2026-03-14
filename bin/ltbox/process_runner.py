from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional, TypedDict, Union

from .logger import get_logger

logger = get_logger()


@dataclass(frozen=True)
class RunOptions:
    capture: bool = False
    stream: bool = False
    check: bool = True
    cwd: Optional[Union[str, Path]] = None
    env: Optional[dict[str, str]] = None


@dataclass(frozen=True)
class CommandResult:
    stdout: str
    stderr: str
    returncode: int
    combined_output: str


class SubprocessTextKwargs(TypedDict):
    encoding: str
    errors: str
    env: dict[str, str]
    cwd: Optional[Union[str, Path]]


def _get_subprocess_kwargs(
    env: dict[str, str], cwd: Optional[Union[str, Path]]
) -> SubprocessTextKwargs:
    run_env = env.copy()

    if cwd:
        resolved_cwd = str(Path(cwd).resolve())
        run_env["TMPDIR"] = resolved_cwd
        run_env["TEMP"] = resolved_cwd
        run_env["TMP"] = resolved_cwd

    return {
        "encoding": "utf-8",
        "errors": "ignore",
        "env": run_env,
        "cwd": cwd,
    }


class CommandRunner:
    def run(
        self,
        command: Union[list[str], str],
        *,
        shell: bool = False,
        options: Optional[RunOptions] = None,
        on_output: Optional[Callable[[str], None]] = None,
    ) -> CommandResult:
        opts = options or RunOptions()
        run_env = opts.env if opts.env is not None else os.environ.copy()
        run_kwargs = _get_subprocess_kwargs(run_env, opts.cwd)

        if opts.capture:
            proc = subprocess.run(
                command,
                shell=shell,
                check=False,
                capture_output=True,
                text=True,
                **run_kwargs,
            )
            stdout = proc.stdout or ""
            stderr = proc.stderr or ""
            result = CommandResult(
                stdout=stdout,
                stderr=stderr,
                returncode=proc.returncode,
                combined_output=(f"{stderr}{stdout}" if stderr else stdout),
            )
            if opts.check and proc.returncode != 0:
                raise subprocess.CalledProcessError(
                    proc.returncode,
                    command,
                    output=stdout,
                    stderr=stderr,
                )
            return result

        if opts.stream:
            process = subprocess.Popen(
                command,
                shell=shell,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
                **run_kwargs,
            )
            output_lines: list[str] = []
            if process.stdout:
                for line in process.stdout:
                    if on_output is not None:
                        on_output(line)
                    else:
                        logger.info(line.rstrip())
                    output_lines.append(line)

            process.wait()
            combined_output = "".join(output_lines)
            returncode = process.returncode
            if opts.check and returncode != 0:
                raise subprocess.CalledProcessError(
                    returncode,
                    command,
                    output=combined_output,
                )
            return CommandResult(
                stdout=combined_output,
                stderr="",
                returncode=returncode,
                combined_output=combined_output,
            )

        proc = subprocess.run(
            command, shell=shell, check=False, text=True, **run_kwargs
        )
        if opts.check and proc.returncode != 0:
            raise subprocess.CalledProcessError(proc.returncode, command)
        return CommandResult(
            stdout="",
            stderr="",
            returncode=proc.returncode,
            combined_output="",
        )
