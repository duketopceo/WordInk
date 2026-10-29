"""macOS key storage on the shared store (plan M1): env wins, store fills in, nothing leaks."""

from __future__ import annotations

import builtins

import pytest

from wordink.client import WordInkError
from wordink.desktop.mac.secrets import (
    KEY_NAME, MISSING_KEY_HELP, SERVICE, KeyStore, env_with_stored_key, resolve_key, set_key_interactive,
)
from wordink.desktop.win import secrets as win_secrets

KEY = "gsk_super_secret_value_123"


class FakeBackend:
    def __init__(self, data=None, fail=False) -> None:
        self.data = dict(data or {})
        self.fail = fail

    def get_password(self, service, username):
        if self.fail:
            raise RuntimeError(f"keychain locked while reading {KEY}")
        return self.data.get((service, username))

    def set_password(self, service, username, password):
        if self.fail:
            raise RuntimeError(f"keychain locked while writing {password}")
        self.data[(service, username)] = password

    def delete_password(self, service, username):
        del self.data[(service, username)]


def test_env_wins_and_the_keychain_fills_in():
    store = KeyStore(FakeBackend({(SERVICE, KEY_NAME): "stored"}))
    assert resolve_key({KEY_NAME: "from-env"}, store).reveal() == "from-env"
    assert resolve_key({}, store).reveal() == "stored"
    assert env_with_stored_key({}, store)[KEY_NAME] == "stored"
    assert env_with_stored_key({KEY_NAME: "e"}, store)[KEY_NAME] == "e"


def test_missing_key_is_none_and_the_help_names_the_keychain():
    assert resolve_key({}, KeyStore(FakeBackend())) is None
    assert "Keychain" in MISSING_KEY_HELP and "set-key" in MISSING_KEY_HELP


def test_set_key_is_hidden_stripped_and_never_echoed(capsys):
    backend = FakeBackend()
    set_key_interactive(KeyStore(backend), prompt=lambda _p: f"  {KEY}\n")
    assert backend.data[(SERVICE, KEY_NAME)] == KEY
    out = capsys.readouterr()
    assert KEY not in out.out + out.err
    with pytest.raises(WordInkError, match="empty"):
        set_key_interactive(KeyStore(FakeBackend()), prompt=lambda _p: " ")


def test_backend_errors_name_the_keychain_and_never_the_secret():
    store = KeyStore(FakeBackend(fail=True))
    with pytest.raises(WordInkError) as read:
        store.get()
    with pytest.raises(WordInkError) as write:
        store.set(KEY)
    assert "macOS Keychain" in str(read.value) and "macOS Keychain" in str(write.value)
    assert KEY not in str(read.value) + str(write.value)


def test_without_keyring_the_error_names_the_macos_extra(monkeypatch):
    real = builtins.__import__

    def no_keyring(name, *a, **k):
        if name == "keyring":
            raise ImportError("no keyring")
        return real(name, *a, **k)

    monkeypatch.setattr(builtins, "__import__", no_keyring)
    with pytest.raises(WordInkError, match="macos' extra"):
        KeyStore().get()


def test_windows_and_macos_share_one_implementation_with_their_own_wording():
    from wordink.desktop import keystore

    assert issubclass(KeyStore, keystore.KeyStore) and issubclass(win_secrets.KeyStore, keystore.KeyStore)
    assert win_secrets.KeyStore.LABEL != KeyStore.LABEL
    assert win_secrets.SERVICE == SERVICE and win_secrets.KEY_NAME == KEY_NAME
