from unittest.mock import patch

from ltbox.actions.root.prompts import StrategySourceSelection
from ltbox.actions.root.strategies import APatchStrategy, LkmRootStrategy


def test_apatch_strategy_configure_source_applies_prompt_selection():
    selection = StrategySourceSelection(
        repo_config={"repo": "owner/apatch"},
        source_label="Nightly",
        is_nightly=True,
        workflow_id="12345",
    )
    strategy = APatchStrategy("apatch")

    with patch(
        "ltbox.actions.root.strategies.select_apatch_source",
        return_value=selection,
    ) as select_source:
        strategy.configure_source("main > root")

    select_source.assert_called_once_with("apatch", breadcrumbs="main > root")
    assert strategy.repo_config == {"repo": "owner/apatch"}
    assert strategy.source_label == "Nightly"
    assert strategy.is_nightly is True
    assert strategy.workflow_id == "12345"


def test_apatch_strategy_download_resources_uses_download_helper():
    strategy = APatchStrategy("folkpatch")
    strategy.repo_config = {"repo": "owner/folkpatch"}
    strategy.is_nightly = True
    strategy.workflow_id = "run-123"

    with patch(
        "ltbox.actions.root.strategies.download_apatch_resources",
        return_value=True,
    ) as download_resources:
        assert strategy.download_resources() is True

    download_resources.assert_called_once_with(
        profile=strategy.provider,
        staging_dir=strategy._staging_dir,
        repo_config={"repo": "owner/folkpatch"},
        is_nightly=True,
        workflow_id="run-123",
    )


def test_lkm_strategy_configure_source_applies_prompt_selection():
    selection = StrategySourceSelection(
        repo_config={"repo": "owner/ksu"},
        source_label="Release",
        is_nightly=False,
        workflow_id="",
        is_tagged_build=True,
    )
    strategy = LkmRootStrategy("kernelsu-next")

    with patch(
        "ltbox.actions.root.strategies.select_lkm_source",
        return_value=selection,
    ) as select_source:
        strategy.configure_source("main > root")

    select_source.assert_called_once_with("kernelsu-next", breadcrumbs="main > root")
    assert strategy.repo_config == {"repo": "owner/ksu"}
    assert strategy.source_label == "Release"
    assert strategy.is_nightly is False
    assert strategy.workflow_id == ""
    assert strategy.is_tagged_build is True


def test_lkm_strategy_download_resources_uses_download_helper():
    strategy = LkmRootStrategy("kernelsu-next")
    strategy.repo_config = {"repo": "owner/ksu-next"}
    strategy.is_nightly = False
    strategy.workflow_id = ""
    strategy.is_tagged_build = True

    with patch(
        "ltbox.actions.root.strategies.download_lkm_resources",
        return_value=True,
    ) as download_resources:
        assert strategy.download_resources("6.6.0") is True

    download_resources.assert_called_once_with(
        profile=strategy.provider,
        staging_dir=strategy.staging_dir,
        repo_config={"repo": "owner/ksu-next"},
        kernel_version="6.6.0",
        is_nightly=False,
        workflow_id="",
        is_tagged_build=True,
    )
