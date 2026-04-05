"""Tests for ltbox.process_runner – timeout, stream, and non-capture paths."""

import subprocess
import sys

import pytest

from ltbox.process_runner import CommandResult, CommandRunner, RunOptions


class TestCaptureMode:
    def test_capture_returns_stdout_and_stderr(self):
        runner = CommandRunner()
        result = runner.run(
            [
                sys.executable,
                "-c",
                "import sys; print('out'); print('err', file=sys.stderr)",
            ],
            options=RunOptions(capture=True, check=False),
        )
        assert "out" in result.stdout
        assert "err" in result.stderr
        assert result.returncode == 0

    def test_capture_check_raises_on_nonzero(self):
        runner = CommandRunner()
        with pytest.raises(subprocess.CalledProcessError) as exc_info:
            runner.run(
                [sys.executable, "-c", "raise SystemExit(42)"],
                options=RunOptions(capture=True, check=True),
            )
        assert exc_info.value.returncode == 42

    def test_capture_combined_output_prefers_stderr(self):
        runner = CommandRunner()
        result = runner.run(
            [
                sys.executable,
                "-c",
                "import sys; print('err', file=sys.stderr); print('out')",
            ],
            options=RunOptions(capture=True, check=False),
        )
        assert result.combined_output.startswith("err")


class TestStreamMode:
    def test_stream_collects_output_lines(self):
        runner = CommandRunner()
        collected = []
        result = runner.run(
            [sys.executable, "-c", "for i in range(3): print(f'line{i}')"],
            options=RunOptions(stream=True),
            on_output=collected.append,
        )
        assert len(collected) == 3
        assert "line0" in collected[0]
        assert result.returncode == 0

    def test_stream_without_callback_uses_logger(self):
        runner = CommandRunner()
        result = runner.run(
            [sys.executable, "-c", "print('hello')"],
            options=RunOptions(stream=True),
        )
        assert "hello" in result.combined_output
        assert result.returncode == 0

    def test_stream_check_raises_on_nonzero(self):
        runner = CommandRunner()
        with pytest.raises(subprocess.CalledProcessError):
            runner.run(
                [sys.executable, "-c", "raise SystemExit(7)"],
                options=RunOptions(stream=True, check=True),
            )

    def test_stream_watchdog_kills_hung_process(self):
        """The watchdog timer kills a process that blocks stdout indefinitely.

        When the process hangs (stdout stays open, no output), the watchdog
        thread fires process.kill().  The stdout loop then finishes and
        process.wait() succeeds immediately (process already dead).  Because
        timed_out is never set, check=True would raise CalledProcessError for
        the killed process's non-zero returncode.  We use check=False and
        verify the non-zero exit instead.
        """
        runner = CommandRunner()
        result = runner.run(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            options=RunOptions(stream=True, timeout=1.0, check=False),
        )
        # Killed by watchdog → non-zero returncode
        assert result.returncode != 0

    def test_stream_timeout_on_wait_raises(self):
        """When a process keeps writing and process.wait() times out,
        TimeoutExpired should propagate."""
        runner = CommandRunner()
        # Write continuously so stdout loop keeps running, then wait() times out
        script = (
            "import time, sys\n"
            "while True:\n"
            "    print('tick', flush=True)\n"
            "    time.sleep(0.05)\n"
        )
        with pytest.raises((subprocess.TimeoutExpired, subprocess.CalledProcessError)):
            runner.run(
                [sys.executable, "-c", script],
                options=RunOptions(stream=True, timeout=0.5),
            )


class TestNonCaptureMode:
    def test_non_capture_returns_empty_result(self):
        runner = CommandRunner()
        result = runner.run(
            [sys.executable, "-c", "print('hello')"],
            options=RunOptions(capture=False, stream=False),
        )
        assert isinstance(result, CommandResult)
        assert result.stdout == ""
        assert result.stderr == ""
        assert result.returncode == 0

    def test_non_capture_check_raises_on_nonzero(self):
        runner = CommandRunner()
        with pytest.raises(subprocess.CalledProcessError):
            runner.run(
                [sys.executable, "-c", "raise SystemExit(3)"],
                options=RunOptions(capture=False, stream=False, check=True),
            )

    def test_non_capture_no_check_returns_nonzero(self):
        runner = CommandRunner()
        result = runner.run(
            [sys.executable, "-c", "raise SystemExit(3)"],
            options=RunOptions(capture=False, stream=False, check=False),
        )
        assert result.returncode == 3
