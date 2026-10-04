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
    if not config.groq_api_key:
        print("❌ ERROR: GROQ_API_KEY not configured!")
        print()
        print("Please set your Groq API key:")
        print("1. Copy .env.example to .env")
        print("2. Edit .env and add your Groq API key")
        print("3. Get your API key from: https://console.groq.com/")
        print()
        return False

    return True


def main():
    """Main entry point"""
    print("="*60)
    print("🎤 Groq Flow - AI-Powered Speech-to-Text")
    print("   A Wispr Flow Alternative using Groq API")
    print("="*60)
    print()

    # Check requirements
    if not check_requirements():
        sys.exit(1)

    # Check if running with --tray flag (default mode)
    use_tray = True
    if "--no-tray" in sys.argv:
        use_tray = False

    try:
        if use_tray:
            # Run with system tray (recommended)
            from .tray import run_system_tray
            run_system_tray()
        else:
            # Run in console mode (for debugging)
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
