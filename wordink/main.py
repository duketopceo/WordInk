import sys
import os

if sys.platform == "win32":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
        sys.stderr.reconfigure(encoding="utf-8")
    except Exception:
        pass


def check_requirements():
    """Check if all requirements are met"""
    from .config import config

    # Check API key
    if not config.groq_api_key or config.groq_api_key in ("your_groq_key_here", "your_groq_api_key_here", ""):
        print("❌ ERROR: Valid GROQ_API_KEY not configured!")
        print()
        print("Please set your Groq API key:")
        print("1. Launch interactive onboarding: python -m wordink.onboarding.server")
        print("2. Or edit .env and paste your Groq API key")
        print("3. Get your free API key from: https://console.groq.com/keys")
        print()
        return False
    return True


def ensure_single_instance():
    """Ensure only one instance of WordInk runs at a time"""
    if sys.platform == "win32":
        try:
            import ctypes
            kernel32 = ctypes.windll.kernel32
            mutex_name = "Local\\WordInk_SingleInstance_Mutex"
            # Keep mutex handle alive for lifetime of process
            global _wordink_mutex
            _wordink_mutex = kernel32.CreateMutexW(None, False, mutex_name)
            if kernel32.GetLastError() == 183:  # ERROR_ALREADY_EXISTS
                print("ℹ️ WordInk is already running in background.")
                sys.exit(0)
        except Exception:
            pass


def main():
    """Main entry point for WordInk"""
    # Check for onboarding / setup flag
    if "--onboard" in sys.argv or "--setup" in sys.argv:
        from .onboarding.server import start_onboarding_server
        start_onboarding_server()
        return

    ensure_single_instance()
    print("=" * 60)
    print("🖋️  WordInk - Sub-Second Voice Dictation & CUA Grounded Driver")
    print("   Powered by Groq Whisper Turbo (~300ms latency)")
    print("=" * 60)
    print()

    # Check requirements
    if not check_requirements():
        # Offer to launch onboarding
        print("Launching Setup Wizard...")
        from .onboarding.server import start_onboarding_server
        start_onboarding_server()
        return

    # Check if running with --no-tray flag (default is tray)
    use_tray = "--no-tray" not in sys.argv

    try:
        if use_tray:
            from .tray import run_system_tray
            run_system_tray()
        else:
            from .app import GroqFlowApp
            app = GroqFlowApp()
            app.start()
    except KeyboardInterrupt:
        print("\n👋 Goodbye!")
        sys.exit(0)
    except Exception as e:
        print(f"❌ Fatal error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
