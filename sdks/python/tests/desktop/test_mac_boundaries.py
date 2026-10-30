"""Keep the macOS code honest without a Mac on the desk (plan M8).

* Native frameworks are imported in one place each, so everything else is testable against fakes.
* Every `mac/` module imports on any OS: nothing touches a framework at import time.
* On a Mac runner, the real native packages are installed and import for real.
* `mac_hardware` smoke tests are read-only and only run when asked for, on a Mac.
"""

from __future__ import annotations

import ast
import importlib
import pkgutil
import subprocess
import sys
from pathlib import Path

import pytest

import wordink.desktop.mac as mac_pkg

MAC_DIR = Path(mac_pkg.__file__).parent
MAC_MODULES = [m.name for m in pkgutil.iter_modules(mac_pkg.__path__, mac_pkg.__name__ + ".")]

#: Frameworks that only exist on a Mac, and the one module allowed to import each.
#: The plan names quartz.py and appkit.py; `rumps` (a thin wrapper over AppKit) belongs to the menu bar module.
OWNERS = {
    "Quartz": {"quartz.py"},
    "CoreFoundation": {"quartz.py"},
    "AppKit": {"appkit.py"},
    "Foundation": {"appkit.py"},
    "objc": {"quartz.py", "appkit.py"},
    "PyObjCTools": {"quartz.py", "appkit.py"},
    "rumps": {"menubar.py"},
}


def _imported_roots(path: Path) -> set[str]:
    roots: set[str] = set()
    for node in ast.walk(ast.parse(path.read_text())):
        if isinstance(node, ast.Import):
            roots.update(a.name.split(".")[0] for a in node.names)
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            roots.add(node.module.split(".")[0])
    return roots


def _toplevel_imported_roots(path: Path) -> set[str]:
    roots: set[str] = set()
    for node in ast.parse(path.read_text()).body:
        if isinstance(node, ast.Import):
            roots.update(a.name.split(".")[0] for a in node.names)
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            roots.add(node.module.split(".")[0])
    return roots


def test_mac_modules_were_found():
    assert {"wordink.desktop.mac.quartz", "wordink.desktop.mac.appkit", "wordink.desktop.mac.app"} <= set(MAC_MODULES)


def test_each_native_framework_is_imported_only_by_its_owner():
    offenders = []
    for path in sorted(MAC_DIR.glob("*.py")):
        for root in _imported_roots(path):
            if root in OWNERS and path.name not in OWNERS[root]:
                offenders.append(f"{path.name} imports {root}")
    assert offenders == []
    # the rule applies to the shared desktop modules too, not only mac/
    shared = sorted(p for p in MAC_DIR.parent.glob("*.py"))
    assert [p.name for p in shared if _imported_roots(p) & set(OWNERS)] == []


def test_no_mac_module_imports_a_native_framework_at_module_level():
    # Imports inside functions are fine: they run only on a Mac, when called.
    eager = [f"{p.name}: {sorted(_toplevel_imported_roots(p) & set(OWNERS))}" for p in sorted(MAC_DIR.glob("*.py")) if _toplevel_imported_roots(p) & set(OWNERS)]
    assert eager == []


@pytest.mark.parametrize("name", MAC_MODULES)
def test_every_mac_module_imports_in_a_fresh_interpreter(name):
    done = subprocess.run([sys.executable, "-c", f"import {name}"], capture_output=True, text=True, timeout=60)
    assert done.returncode == 0, done.stderr


@pytest.mark.skipif(sys.platform != "darwin", reason="the real native packages only install on macOS")
def test_real_native_packages_import_and_every_mac_module_loads_with_them():
    for pkg in ("Quartz", "AppKit", "Foundation", "rumps", "keyring", "sounddevice"):
        importlib.import_module(pkg)
    for name in MAC_MODULES:
        importlib.import_module(name)


def test_mac_hardware_tests_are_skipped_unless_asked_for():
    done = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-rs", str(Path(__file__)), "-k", "smoke", "-p", "no:cacheprovider"],
        capture_output=True,
        text=True,
        timeout=120,
        env={"PATH": "/usr/bin:/bin", "HOME": str(Path.home()), "PYTHONPATH": ":".join(sys.path)},
    )
    assert done.returncode == 0, done.stdout + done.stderr
    assert "skipped" in done.stdout and "passed" not in done.stdout


@pytest.mark.mac_hardware
def test_smoke_secure_input_state_is_readable():
    from wordink.desktop.mac import quartz

    assert isinstance(quartz.secure_input_enabled(), bool)


@pytest.mark.mac_hardware
def test_smoke_frontmost_app_has_a_bundle_id():
    from wordink.desktop.mac import appkit

    bundle = appkit.frontmost_bundle_id()
    assert bundle is None or "." in bundle


@pytest.mark.mac_hardware
def test_smoke_there_is_a_default_input_device():
    import sounddevice as sd

    assert sd.query_devices(kind="input")["max_input_channels"] >= 1
