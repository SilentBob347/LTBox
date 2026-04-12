from unittest.mock import MagicMock, patch

from ltbox.github_client import GitHubClient


def test_github_client_fetch_release_data_uses_latest_release_endpoint():
    latest_response = MagicMock()
    latest_response.raise_for_status.return_value = None
    latest_response.json.return_value = {"tag_name": "v1.2.3"}

    session = MagicMock()
    session.get.return_value = latest_response

    with patch("ltbox.github_client.net.get_client", return_value=session):
        release_data = GitHubClient("owner/repo").fetch_release_data("latest", ".*")

    assert release_data["tag_name"] == "v1.2.3"
    session.get.assert_called_once()


def test_github_client_workflow_run_id_for_tag_falls_back_to_unfiltered_runs():
    filtered_runs_response = MagicMock()
    filtered_runs_response.raise_for_status.return_value = None
    filtered_runs_response.json.return_value = {"workflow_runs": []}

    all_runs_response = MagicMock()
    all_runs_response.raise_for_status.return_value = None
    all_runs_response.json.return_value = {
        "workflow_runs": [
            {"id": 42, "head_branch": "refs/tags/v1.2.3"},
        ]
    }

    session = MagicMock()
    session.get.side_effect = [filtered_runs_response, all_runs_response]

    with patch(
        "ltbox.github_client.net.get_client",
        return_value=session,
    ):
        run_id = GitHubClient("owner/repo").workflow_run_id_for_tag("v1.2.3")

    assert run_id == "42"


def test_github_client_latest_successful_workflow_run_filters_by_file_and_branch():
    workflow_runs_response = MagicMock()
    workflow_runs_response.raise_for_status.return_value = None
    workflow_runs_response.json.return_value = {
        "workflow_runs": [
            {"id": 1, "path": ".github/workflows/build.yml", "head_branch": "dev"},
            {"id": 2, "path": ".github/workflows/other.yml", "head_branch": "main"},
        ]
    }

    all_runs_response = MagicMock()
    all_runs_response.raise_for_status.return_value = None
    all_runs_response.json.return_value = {
        "workflow_runs": [
            {
                "id": 99,
                "path": ".github/workflows/build.yml@refs/heads/main",
                "head_branch": "main",
            }
        ]
    }

    session = MagicMock()
    session.get.side_effect = [workflow_runs_response, all_runs_response]

    with patch("ltbox.github_client.net.get_client", return_value=session):
        run_id = GitHubClient("owner/repo").latest_successful_workflow_run(
            "build.yml", branch="main"
        )

    assert run_id == "99"


def test_github_client_workflow_run_matches_checks_workflow_file_and_branch():
    run_response = MagicMock()
    run_response.raise_for_status.return_value = None
    run_response.json.return_value = {
        "id": 77,
        "path": ".github/workflows/build.yml@refs/heads/main",
        "head_branch": "main",
    }

    session = MagicMock()
    session.get.return_value = run_response

    with patch("ltbox.github_client.net.get_client", return_value=session):
        client = GitHubClient("owner/repo")

        assert client.workflow_run_matches("77", "build.yml", branch="main") is True
        assert client.workflow_run_matches("77", "build.yml", branch="dev") is False
        assert client.workflow_run_matches("77", "other.yml", branch="main") is False
