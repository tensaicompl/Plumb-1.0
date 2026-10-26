# Running the UI against `plumb serve`

The supported way to use the pilot UI is the Vite dev server in `ui/`, which serves the React app
and forwards every relative `/api` call (including the `/api/events` WebSocket) to a running
`plumb serve`:

```bash
# 1. The backend, over an existing project database and configuration.
cargo run -p plumb-cli -- --db plumb.sqlite --config plumb.json serve --listen 127.0.0.1:8080

# 2. The UI (in another terminal); PLUMB_API_TARGET defaults to http://127.0.0.1:8080.
cd ui
npm ci
PLUMB_API_TARGET=http://127.0.0.1:8080 npm run dev -- --host 127.0.0.1 --port 5173
```

Open <http://127.0.0.1:5173>. `npm run build` type-checks and produces a static bundle in
`ui/dist/` (for inspection; the pilot does not serve it from `plumb serve`).

`plumb serve` writes one tracing record per request (method, matched route, status, latency) and
the cause of every internal failure to stderr; `RUST_LOG` overrides the default `info` filter.

`scripts/run-pilot-e2e.sh` (E0.3) uses exactly this setup: it builds the HR pilot databases, starts
`plumb serve`, starts the UI with `npm run dev` against it and runs every Playwright spec in
`ui/e2e/` with one worker. Five databases, each served in its own phase and selected by tag: the
finished pilot (untagged), `@inbox-db`, `@triage-db`, `@questions-db` and `@empty-db`; the last
three are served with `--provider replay` so the UI can run steps without an API key.
`PLUMB_E2E_API_PORT` and `PLUMB_E2E_UI_PORT` choose the ports (default 8790 and 5173).

## Local install (built UI served by `plumb serve`)

`plumb serve --ui-dir ui/dist` serves the built UI (`npm run build`) on the API's own address:
- a non-API path that names no file returns `index.html`, so in-app links load;
- `/api` paths are unchanged;
- a directory without `index.html` is refused with `E_UI_DIR_INVALID`.

`make package` (`scripts/package-local.sh`) builds `dist/plumb-local/`, plus a `.tar.gz` of it:
- the release CLI with the Anthropic provider (`bin/plumb`);
- the built UI;
- `start.sh`, which creates a project on first start and opens the browser;
- `plumb.sh`, which runs one command for a project;
- an example configuration and the HR requirements;
- a README for pilot users (`packaging/README.md`).
