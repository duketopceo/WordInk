"""Windows key storage (plan W2): env wins, store fills in, nothing leaks."""

from __future__ import annotations

import pytest

from wordink.client import WordInkError
from wordink.desktop.win.secrets import (
    KEY_NAME, MISSING_KEY_HELP, SERVICE, KeyStore, Secret, env_with_stored_key, resolve_key, set_key_interactive,
)

KEY = "gsk_super_secret_value_123"


class FakeBackend:
    def __init__(self, data=None, fail=False) -> None:
        self.data = dict(data or {})
        self.fail = fail

    def get_password(self, service, username):
        if self.fail:
            raise RuntimeError(f"vault locked while reading {KEY}")
        return self.data.get((service, username))

    def set_password(self, service, username, password):
        if self.fail:
            raise RuntimeError(f"vault locked while writing {password}")
        self.data[(service, username)] = password

    def delete_password(self, service, username):
        del self.data[(service, username)]


def test_env_var_wins_over_the_stored_key():
    store = KeyStore(FakeBackend({(SERVICE, KEY_NAME): "stored"}))
    assert resolve_key({KEY_NAME: "from-env"}, store).reveal() == "from-env"
    assert resolve_key({}, store).reveal() == "stored"


def test_missing_key_is_none_and_the_help_text_names_no_value():
    assert resolve_key({}, KeyStore(FakeBackend())) is None
    assert "set-key" in MISSING_KEY_HELP and "GROQ_API_KEY" in MISSING_KEY_HELP and "gsk_" not in MISSING_KEY_HELP


def test_env_with_stored_key_fills_only_when_absent_and_does_not_mutate():
    store = KeyStore(FakeBackend({(SERVICE, KEY_NAME): "stored"}))
    base = {"PATH": "x"}
    out = env_with_stored_key(base, store)
    assert out[KEY_NAME] == "stored" and KEY_NAME not in base
    assert env_with_stored_key({KEY_NAME: "env"}, store)[KEY_NAME] == "env"
    assert KEY_NAME not in env_with_stored_key({}, KeyStore(FakeBackend()))


def test_secret_never_shows_in_repr_or_str_or_format():
    s = Secret(KEY)
    for shown in (repr(s), str(s), f"{s}", f"{s!r}", repr([s]), repr({"k": s})):
        assert KEY not in shown
    assert s.reveal() == KEY and s and not Secret("")


def test_set_key_prompts_hidden_and_never_echoes(capsys):
    backend = FakeBackend()
    prompts: list[str] = []

    def fake_getpass(prompt: str) -> str:
        prompts.append(prompt)
        return f"  {KEY}\n"

    set_key_interactive(KeyStore(backend), prompt=fake_getpass)
    assert backend.data[(SERVICE, KEY_NAME)] == KEY  # stripped
    out = capsys.readouterr()
    assert KEY not in out.out + out.err and KEY not in prompts[0]


def test_empty_key_is_refused():
    with pytest.raises(WordInkError, match="empty"):
        set_key_interactive(KeyStore(FakeBackend()), prompt=lambda _p: "   ")


def test_backend_errors_become_wordink_errors_without_the_secret():
    store = KeyStore(FakeBackend(fail=True))
    with pytest.raises(WordInkError) as read:
        store.get()
    with pytest.raises(WordInkError) as write:
        store.set(KEY)
    assert KEY not in str(read.value) and KEY not in str(write.value)
    assert "RuntimeError" in str(read.value)


def test_delete_reports_whether_something_was_stored():
    store = KeyStore(FakeBackend({(SERVICE, KEY_NAME): "x"}))
    assert store.delete() is True
    assert store.delete() is False


def test_without_keyring_installed_the_error_says_how_to_fix_it(monkeypatch):
    import builtins

    real = builtins.__import__

    def no_keyring(name, *a, **k):
        if name == "keyring":
            raise ImportError("no keyring")
        return real(name, *a, **k)

    monkeypatch.setattr(builtins, "__import__", no_keyring)
    with pytest.raises(WordInkError, match="windows' extra"):
        KeyStore().get()
