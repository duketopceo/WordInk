import { createWorker } from "@wordink/server/cloudflare";

interface Env {
  GROQ_API_KEY?: string;
  OPENAI_API_KEY?: string;
  DEEPGRAM_API_KEY?: string;
  /** Your app's endpoint that answers 2xx for a signed-in user (it receives the browser's cookies). */
  SESSION_CHECK_URL: string;
  /** Comma-separated origins allowed to call the relay from a browser. */
  ALLOWED_ORIGINS?: string;
}

export default createWorker<Env>((env) => ({
  basePath: "/wordink",
  allowedOrigins: (env.ALLOWED_ORIGINS ?? "").split(",").filter(Boolean),
  // Only your signed-in users may spend your provider quota. Replace with your own check
  // (verify a JWT, look up a session id, ...). Without `authorize` every request gets 403.
  authorize: async (request) => {
    const cookie = request.headers.get("cookie");
    if (!cookie) return false;
    const res = await fetch(env.SESSION_CHECK_URL, { headers: { cookie } });
    return res.ok;
  },
}));
