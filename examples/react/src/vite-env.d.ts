/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Groq key for local experiments (sent from the page; localhost only). */
  readonly VITE_GROQ_KEY?: string;
}
