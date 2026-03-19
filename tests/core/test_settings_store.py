from ltbox import main


def test_settings_store_ignores_unknown_and_invalid_updates(tmp_path):
    store = main.SettingsStore(tmp_path / "settings.json")

    updated = store.update(target_region="INVALID", language=123, unknown_key="x")

    assert updated.target_region == "PRC"
    assert updated.language is None
    assert updated.modify_region_code is True
    assert store.load_raw() == {}


def test_settings_store_applies_valid_updates_from_validator_map(tmp_path):
    store = main.SettingsStore(tmp_path / "settings.json")

    store.update(language="ko", target_region="ROW", modify_region_code=False)

    loaded = store.load()
    assert loaded.language == "ko"
    assert loaded.target_region == "ROW"
    assert loaded.modify_region_code is False
