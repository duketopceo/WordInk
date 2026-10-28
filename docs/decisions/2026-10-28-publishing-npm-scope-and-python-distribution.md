# Decision: npm scope and Python distribution

Date: 2026-10-28. Status: decided (owner approved npm scope and trusted publishing).

## npm: keep the `@wordink` scope, publish with trusted publishing (OIDC)

- The five packages stay `@wordink/core`, `@wordink/web`, `@wordink/react`, `@wordink/local` and `@wordink/server`. They already carry `publishConfig.provenance: true`, and `.github/workflows/release.yml` already publishes through npm trusted publishing (GitHub OIDC, no stored token) with provenance.
- As of this date `@wordink/core` and the unscoped fallbacks (`wordink-core`, `wordink`) are all unclaimed on the registry (404), so the scope can be created. The unscoped names stay the fallback if the `wordink` org name is taken when it is created.
- The publish job stays dormant until the `NPM_PUBLISH_ENABLED` repository variable is `true`.

### One-time steps on npmjs.com (owner account, not automatable)

1. Sign in to npmjs.com and create the organization `wordink` (free, public packages).
2. Trusted publishing only works for packages that already exist, so publish a first `0.0.1`-style placeholder (or the current version) of each of `@wordink/core`, `web`, `react`, `local` and `server` by hand: `npm publish --access public` from a local build, with 2FA.
3. For each package, open Settings, Trusted publishing, add a GitHub Actions publisher: owner `duketopceo`, repository `WordInk`, workflow `release.yml`, no environment.
4. For each package, set Publishing access to "Require two-factor authentication and disallow tokens".
5. Run `gh variable set NPM_PUBLISH_ENABLED --body true -R duketopceo/WordInk`.
6. Merge the "chore: version packages" PR (#32). The `publish` job then releases.

## Python: `uvx` from git now, PyPI later

- `sdks/python` (`wordink`, which provides `wordink` and `wordink-mcp`, and the planned `wordink-dictd`) is distributed as `uvx --from "git+https://github.com/duketopceo/WordInk#subdirectory=sdks/python" wordink`, and installed on Linux desktops by the dictation installer with `uv tool install` from the same source.
- Why: no registry account, no token and no spend; the only users today are Luke's own machines and the MCP clients he configures. `uv` is already installed on omarchy-max. Installing from git pins to a ref, so a release is a tag.
- PyPI is deferred until there is outside demand. The name `wordink` is unclaimed on PyPI (404 as of this date). When wanted, publish with a PyPI trusted publisher for `release.yml`-style OIDC, not an API token.
