// The docs site: a Vite multi-page build. Each page is Markdown in `pages/`; the `<slug>.html` files
// are empty entry stubs that the plugin below replaces with the rendered page inside the layout.
// Build for GitHub Pages with `vite build --base=/WordInk/` (all page links are relative).
import { readFileSync } from "node:fs";
import { basename, extname, join } from "node:path";
import { Marked, type Tokens } from "marked";
import { defineConfig, type Plugin } from "vite";
import { DOCS_DIR, PAGES, fillPlaceholders, release, snippet, type Release } from "./site.js";

const REPO_URL = "https://github.com/duketopceo/WordInk";
const LANGS: Record<string, string> = { ".html": "html", ".sh": "sh", ".ts": "ts", ".tsx": "tsx" };

function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/<[^>]+>/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

const marked = new Marked({
  gfm: true,
  renderer: {
    heading(this: { parser: { parseInline(t: Tokens.Heading["tokens"]): string } }, { tokens, depth, text }: Tokens.Heading) {
      const id = slugify(text);
      return `<h${depth} id="${id}"><a class="anchor" href="#${id}">${this.parser.parseInline(tokens)}</a></h${depth}>\n`;
    },
  },
});

/** `@include snippets/<file>` on its own line becomes a fenced code block of that file. */
function expandIncludes(md: string, r: Release): string {
  return md.replace(/^@include snippets\/(\S+)$/gm, (_, file: string) => {
    const lang = LANGS[extname(file)] ?? "";
    return `\`\`\`${lang}\n${snippet(file, r)}\n\`\`\``;
  });
}

function layout(slug: string, body: string, rel: Release): string {
  const page = PAGES.find((p) => p.slug === slug);
  if (!page) throw new Error(`No docs page "${slug}" in site.ts`);
  const nav = PAGES.map(
    (p) => `<a href="${p.slug === "index" ? "./" : `${p.slug}.html`}"${p.slug === slug ? ' aria-current="page"' : ""}>${p.nav}</a>`,
  ).join("\n        ");
  const banner =
    rel.webVersion === "0.0.0"
      ? `<p class="prerelease">Pre-release: the packages are not on npm yet, so versions below read 0.0.0.</p>\n`
      : "";
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>${page.title}</title>
    <meta name="description" content="WordInk: open-source, provider-agnostic push-to-talk dictation for any web app." />
    <link rel="stylesheet" href="/src/style.css" />
    <script type="module" src="/src/main.ts"></script>
  </head>
  <body>
    <header class="top">
      <a class="brand" href="./">WordInk</a>
      <a class="gh" href="${REPO_URL}">GitHub</a>
    </header>
    <div class="shell">
      <nav class="side" aria-label="Docs">
        ${nav}
      </nav>
      <main class="content">
${banner}${body}
      </main>
    </div>
    <footer class="foot">MIT licensed. No WordInk servers, no telemetry.</footer>
  </body>
</html>`;
}

function docsPages(): Plugin {
  let rel: Release | undefined;
  return {
    name: "wordink-docs-pages",
    async configResolved(config) {
      rel = await release({ checkCdn: config.command === "build" });
    },
    configureServer(server) {
      server.watcher.add([join(DOCS_DIR, "pages"), join(DOCS_DIR, "snippets")]);
      server.watcher.on("change", (file) => {
        if (file.endsWith(".md") || file.includes("snippets")) server.ws.send({ type: "full-reload" });
      });
    },
    transformIndexHtml: {
      order: "pre",
      handler(_html, ctx) {
        if (!rel) throw new Error("release info not loaded");
        const slug = basename(ctx.filename, ".html");
        const md = readFileSync(join(DOCS_DIR, "pages", `${slug}.md`), "utf8");
        const html = marked.parse(expandIncludes(fillPlaceholders(md, rel), rel), { async: false });
        return layout(slug, html, rel);
      },
    },
  };
}

export default defineConfig({
  plugins: [docsPages()],
  // The local engine's worker is an ES module that imports transformers.js.
  worker: { format: "es" },
  optimizeDeps: { exclude: ["@wordink/local"] },
  build: {
    target: "es2022",
    rollupOptions: { input: Object.fromEntries(PAGES.map((p) => [p.slug, join(DOCS_DIR, `${p.slug}.html`)])) },
  },
});
