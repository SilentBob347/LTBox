from ltbox import main, menu_router


def test_main_loop_settings_flow(monkeypatch, tmp_path):
    settings_path = tmp_path / "settings.json"
    store = main.SettingsStore(settings_path)
    monkeypatch.setattr(main, "SETTINGS_STORE", store)

    actions = iter(
        [
            "menu_settings",
            "toggle_region",
            "toggle_adb",
            "toggle_rollback",
            "back",
            "exit",
        ]
    )

    def fake_select_menu_action(menu_items, title_key, **kwargs):
        return next(actions)

    monkeypatch.setattr(menu_router, "select_menu_action", fake_select_menu_action)

    class DummyController:
        last_instance = None

        def __init__(self, skip_adb=False):
            self.skip_adb = skip_adb
            DummyController.last_instance = self

    menu_router.main_loop(DummyController, main.CommandRegistry(), settings_store=store)

    assert DummyController.last_instance.skip_adb is True
    assert store.load().target_region == "ROW"
