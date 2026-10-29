"""The Groq key in the OS credential store through `keyring`, with the environment as an override.

Shared by the Windows (Credential Manager) and macOS (Keychain) apps. `keyring` is imported
lazily, so this module imports on any OS. Nothing here prints, logs or returns a key in an error
message.
"""

from __future__ import annotations

import getpass
from collections.abc import Callable, Mapping
from typing import Protocol

from ..client import WordInkError

SERVICE = "wordink-dictd"
KEY_NAME = "GROQ_API_KEY"


class Secret:
    """A string whose `repr` and `str` never show it. Use `.reveal()` at the one call that needs it."""

    __slots__ = ("_value",)

    def __init__(self, value: str) -> None:
        self._value = value

    def reveal(self) -> str:
        return self._value

    def __repr__(self) -> str:
        return "Secret(***)"

    __str__ = __repr__

    def __bool__(self) -> bool:
        return bool(self._value)


class Backend(Protocol):
    def get_password(self, service: str, username: str) -> str | None: ...
    def set_password(self, service: str, username: str, password: str) -> None: ...
    def delete_password(self, service: str, username: str) -> None: ...


def _keyring_backend(label: str, extra: str) -> Backend:
    try:
        import keyring
    except ImportError as exc:
        raise WordInkError(f"the {label} needs the 'keyring' package: install wordink with the '{extra}' extra") from exc
    return keyring  # type: ignore[return-value]


class KeyStore:
    #: Per-OS wording; the Windows and macOS subclasses set them.
    LABEL = "credential store"
    EXTRA = "keyring"

    def __init__(self, backend: Backend | None = None) -> None:
        self._backend = backend

    @property
    def backend(self) -> Backend:
        if self._backend is None:
            self._backend = _keyring_backend(self.LABEL, self.EXTRA)
        return self._backend

    def get(self, name: str = KEY_NAME) -> Secret | None:
        try:
            value = self.backend.get_password(SERVICE, name)
        except WordInkError:
            raise
        except Exception as exc:  # a locked or missing credential vault is not a crash
            raise WordInkError(f"cannot read the stored {name} from the {self.LABEL} ({type(exc).__name__})") from exc
        return Secret(value) if value else None

    def set(self, value: str, name: str = KEY_NAME) -> None:
        if not value.strip():
            raise WordInkError("refusing to store an empty key")
        try:
            self.backend.set_password(SERVICE, name, value.strip())
        except WordInkError:
            raise
        except Exception as exc:
            raise WordInkError(f"cannot write {name} to the {self.LABEL} ({type(exc).__name__})") from exc

    def delete(self, name: str = KEY_NAME) -> bool:
        try:
            self.backend.delete_password(SERVICE, name)
        except WordInkError:
            raise
        except Exception:
            return False  # nothing stored (keyring raises PasswordDeleteError)
        return True


def resolve_key(env: Mapping[str, str], store: KeyStore | None = None, name: str = KEY_NAME) -> Secret | None:
    """The environment wins; otherwise the credential store; otherwise None."""
    value = env.get(name)
    if value:
        return Secret(value)
    return (store or KeyStore()).get(name)


def env_with_stored_key(env: Mapping[str, str], store: KeyStore | None = None) -> dict[str, str]:
    """A copy of `env` with `GROQ_API_KEY` filled in from the store when absent, so the same
    pipeline that reads the environment on Linux runs unchanged. The copy is process-local."""
    out = dict(env)
    if not out.get(KEY_NAME):
        found = (store or KeyStore()).get(KEY_NAME)
        if found:
            out[KEY_NAME] = found.reveal()
    return out


def set_key_interactive(
    store: KeyStore | None = None,
    prompt: Callable[[str], str] = getpass.getpass,
) -> None:
    """Prompt without echo and store the key. The value is never printed back."""
    value = prompt("Groq API key (input hidden): ")
    (store or KeyStore()).set(value)

