import io
import time
from typing import Optional
from groq import Groq

from .config import config


class GroqService:
    """Handles all Groq API interactions"""

    def __init__(self):
        """Initialize Groq client"""
        api_key = config.groq_api_key
        if not api_key:
            raise ValueError(
                "GROQ_API_KEY not found. Please set it in your .env file or configuration."
            )

        self.client = Groq(api_key=api_key)
        self.whisper_model = config.whisper_model
        self.llm_model = config.llm_model

    def transcribe_audio(self, audio_data: bytes, retries: int = 3) -> Optional[str]:
        """
        Transcribe audio using Groq's Whisper API

        Args:
            audio_data: Audio data in WAV format
            retries: Number of retry attempts on failure

        Returns:
            Transcribed text or None on failure
        """
        for attempt in range(retries):
            try:
                # Create a file-like object from audio bytes
                audio_file = io.BytesIO(audio_data)
                audio_file.name = "audio.wav"

                # Call Groq Whisper API
                transcription = self.client.audio.transcriptions.create(
                    file=audio_file,
                    model=self.whisper_model,
                    response_format="text",
                    language="en",  # Change if you need other languages
                    temperature=0.0
                )

                # Extract text from response
                if isinstance(transcription, str):
                    text = transcription.strip()
                else:
                    text = transcription.text.strip() if hasattr(
                        transcription, 'text') else str(transcription).strip()

                if config.debug:
                    print(f"📝 Raw transcription: {text}")

                return text if text else None

            except Exception as e:
                print(
                    f"❌ Transcription error (attempt {attempt + 1}/{retries}): {e}")
                if attempt < retries - 1:
                    time.sleep(1)  # Wait before retry
                else:
                    return None

        return None

    def clean_text(self, text: str, retries: int = 2) -> Optional[str]:
        """
        Clean and enhance transcribed text using Groq's LLM
        - Removes filler words (um, uh, like, you know)
        - Fixes grammar and punctuation
        - Handles corrections ("actually", "I mean", etc.)
        - Makes text more polished

        Args:
            text: Raw transcribed text
            retries: Number of retry attempts on failure

        Returns:
            Cleaned text or original text on failure
        """
        if not text or not config.enable_ai_cleanup:
            return text

        system_prompt = """You are a text cleanup tool. Your job: remove filler words and fix grammar while preserving EXACT meaning.

STRICT RULES:
1. Remove ONLY: um, uh, like (filler), you know, basically, literally, I mean, sort of, kind of
2. Fix grammar/punctuation errors
3. If user corrects themselves ("no wait, actually X"), keep only the final correction
4. NEVER change meaning or rephrase
5. Return ONLY the cleaned text - NO quotes, NO explanations, NO extra words

CRITICAL: Your response must be ONLY the cleaned text. Nothing else.

Examples:
Input: um so I was like thinking we could uh meet at 6
Output: I was thinking we could meet at 6.

Input: can you make it so that the overlay thing works
Output: Can you make it so that the overlay thing works?

Input: This is great but um can you like make the overlay work
Output: This is great, but can you make the overlay work?

Now clean this text (return ONLY cleaned text):"""

        for attempt in range(retries):
            try:
                response = self.client.chat.completions.create(
                    model=self.llm_model,
                    messages=[
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": text}
                    ],
                    temperature=0.1,  # Lower temperature for more deterministic output
                    max_tokens=500,
                    top_p=0.9,
                    frequency_penalty=0.2  # Discourage repetitive explanations
                )

                cleaned_text = (
                    response.choices[0].message.content or "").strip()

                # Remove any surrounding quotes that LLM might add
                if cleaned_text.startswith('"') and cleaned_text.endswith('"'):
                    cleaned_text = cleaned_text[1:-1]
                if cleaned_text.startswith("'") and cleaned_text.endswith("'"):
                    cleaned_text = cleaned_text[1:-1]

                if config.debug:
                    print(f"✨ Cleaned text: {cleaned_text}")

                return cleaned_text if cleaned_text else text

            except Exception as e:
                print(
                    f"❌ Text cleaning error (attempt {attempt + 1}/{retries}): {e}")
                if attempt < retries - 1:
                    time.sleep(0.5)
                else:
                    # Return original text if cleaning fails
                    return text

        return text

    def process_audio(self, audio_data: bytes) -> Optional[str]:
        """
        Complete pipeline: transcribe and clean audio

        Args:
            audio_data: Audio data in WAV format

        Returns:
            Processed text ready for insertion
        """
        # Step 1: Transcribe
        print("🔄 Transcribing audio...")
        raw_text = self.transcribe_audio(audio_data)

        if not raw_text:
            print("❌ Transcription failed")
            return None

        print(f"✅ Transcription: {raw_text}")

        # Step 2: Clean (if enabled)
        if config.enable_ai_cleanup:
            print("✨ Cleaning text...")
            cleaned_text = self.clean_text(raw_text)
            print(f"✅ Final text: {cleaned_text}")
            return cleaned_text
        else:
            return raw_text
