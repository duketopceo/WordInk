"""The Groq key on Windows: Credential Manager through `keyring`, with the environment as an override.

The store itself is shared with macOS (`wordink.desktop.keystore`); this module pins the Windows
wording and defaults. `keyring` is imported lazily, so it imports on any OS.
"""

from __future__ import annotations

import getpass
from collections.abc import Callable, Mapping

from .. import keystore
from ..keystore import KEY_NAME, SERVICE, Backend, Secret

__all__ = [
    "KEY_NAME", "MISSING_KEY_HELP", "SERVICE", "Backend", "KeyStore", "Secret",
    "env_with_stored_key", "resolve_key", "set_key_interactive",
]  # fmt: skip


class KeyStore(keystore.KeyStore):
    LABEL = "Windows credential store"
    EXTRA = "windows"


def resolve_key(env: Mapping[str, str], store: keystore.KeyStore | None = None, name: str = KEY_NAME) -> Secret | None:
    """The environment wins; otherwise the credential store; otherwise None."""
    return keystore.resolve_key(env, store or KeyStore(), name)


def env_with_stored_key(env: Mapping[str, str], store: keystore.KeyStore | None = None) -> dict[str, str]:
    """A copy of `env` with `GROQ_API_KEY` filled in from the store when absent (process-local)."""
    return keystore.env_with_stored_key(env, store or KeyStore())


def set_key_interactive(
    store: keystore.KeyStore | None = None,
    prompt: Callable[[str], str] | None = None,
) -> None:
    """Prompt without echo and store the key. The value is never printed back."""
    keystore.set_key_interactive(store or KeyStore(), prompt or getpass.getpass)


MISSING_KEY_HELP = (
    "no Groq key found: run `wordink-dictd set-key` to store one in Windows Credential Manager, "
    "or set GROQ_API_KEY in the environment (or use WORDINK_GATEWAY_URL and WORDINK_DEVICE_TOKEN)"
)
