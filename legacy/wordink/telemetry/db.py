import sqlite3
import time
from pathlib import Path
from typing import Dict, Any, List, Optional


class TelemetryDB:
    """Local SQLite telemetry tracker for voice dictation sessions"""

    def __init__(self, db_path: Optional[Path] = None):
        if db_path is None:
            data_dir = Path.home() / ".groq_flow"
            data_dir.mkdir(parents=True, exist_ok=True)
            self.db_path = data_dir / "telemetry.db"
        else:
            self.db_path = db_path

        self._init_db()

    def _init_db(self):
        with sqlite3.connect(self.db_path) as conn:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp REAL NOT NULL,
                    duration_sec REAL NOT NULL,
                    word_count INTEGER NOT NULL,
                    char_count INTEGER NOT NULL,
                    latency_ms REAL NOT NULL,
                    model_used TEXT NOT NULL,
                    preview TEXT
                )
            """)
            conn.commit()

    def record_session(
        self,
        duration_sec: float,
        text: str,
        latency_ms: float,
        model_used: str = "whisper-large-v3-turbo"
    ):
        """Record a completed dictation event"""
        words = len(text.strip().split()) if text else 0
        chars = len(text.strip()) if text else 0
        preview = text.strip()[:60] if text else ""

        with sqlite3.connect(self.db_path) as conn:
            conn.execute(
                """
                INSERT INTO sessions (timestamp, duration_sec, word_count, char_count, latency_ms, model_used, preview)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (time.time(), duration_sec, words, chars, latency_ms, model_used, preview)
            )
            conn.commit()

    def get_stats(self) -> Dict[str, Any]:
        """Calculate aggregated usage metrics and savings"""
        with sqlite3.connect(self.db_path) as conn:
            conn.row_factory = sqlite3.Row
            row = conn.execute("""
                SELECT 
                    COUNT(*) as total_sessions,
                    COALESCE(SUM(duration_sec), 0) as total_duration_sec,
                    COALESCE(SUM(word_count), 0) as total_words,
                    COALESCE(AVG(latency_ms), 0) as avg_latency_ms
                FROM sessions
            """).fetchone()

        total_sessions = row["total_sessions"]
        total_duration_sec = row["total_duration_sec"]
        total_words = row["total_words"]
        avg_latency_ms = row["avg_latency_ms"]

        total_hours = total_duration_sec / 3600.0
        # Groq Whisper Turbo is $0.04 / audio hour
        groq_cost = total_hours * 0.04
        # OpenAI Whisper is $0.36 / audio hour
        openai_cost = total_hours * 0.36
        # Wispr Flow is $15/month ($180/year)
        wispr_monthly_cost = 15.0

        return {
            "total_sessions": total_sessions,
            "total_words": total_words,
            "total_duration_sec": round(total_duration_sec, 1),
            "total_duration_min": round(total_duration_sec / 60.0, 1),
            "total_duration_hr": round(total_hours, 2),
            "avg_latency_ms": round(avg_latency_ms, 1),
            "groq_cost_est": round(groq_cost, 4),
            "openai_equivalent_cost": round(openai_cost, 4),
            "wispr_flow_monthly_savings": wispr_monthly_cost,
        }

    def get_recent_history(self, limit: int = 10) -> List[Dict[str, Any]]:
        """Retrieve recent transcription history"""
        with sqlite3.connect(self.db_path) as conn:
            conn.row_factory = sqlite3.Row
            rows = conn.execute(
                """
                SELECT id, timestamp, duration_sec, word_count, latency_ms, model_used, preview
                FROM sessions
                ORDER BY timestamp DESC
                LIMIT ?
                """,
                (limit,)
            ).fetchall()

        return [dict(r) for r in rows]


# Global singleton instance
telemetry = TelemetryDB()
