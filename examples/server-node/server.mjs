import { createServer } from "node:http";
import { createNodeHandler } from "@wordink/server/node";

// Keys come from GROQ_API_KEY / OPENAI_API_KEY / DEEPGRAM_API_KEY in the environment.
const SESSION_CHECK_URL = process.env.SESSION_CHECK_URL ?? "http://localhost:3000/api/session";
const port = Number(process.env.PORT ?? 8787);

const handler = createNodeHandler({
  basePath: "/wordink",
  allowedOrigins: (process.env.ALLOWED_ORIGINS ?? "http://localhost:5173").split(",").filter(Boolean),
  // Only your signed-in users may spend your provider quota. Replace with your own check
  // (verify a JWT, look up a session id, ...). Without `authorize` every request gets 403.
  authorize: async (request) => {
    const cookie = request.headers.get("cookie");
    if (!cookie) return false;
    const res = await fetch(SESSION_CHECK_URL, { headers: { cookie } });
    return res.ok;
  },
});

createServer(handler).listen(port, () => {
  console.log(`WordInk relay on http://localhost:${port}/wordink`);
});
