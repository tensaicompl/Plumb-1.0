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

`scripts/run-pilot-e2e.sh` (E0.3) uses exactly this setup: it builds the HR pilot database, starts
`plumb serve`, starts the UI with `npm run dev` against it and runs the Playwright pilot workflow.
