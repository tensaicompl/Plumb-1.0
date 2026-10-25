#!/usr/bin/env bash
# E0.4 performance release gate (plan v3.2 hotfix, architecture §33.10).
#
# Reproduces the audit's measured shapes through the product CLI and times every plumb-cli
# invocation:
#   * a 50,000-paragraph Markdown import (one file over the frozen 20,000-fragment source limit
#     must be rejected promptly; the same paragraphs then import as three sources),
#   * a 10,000-node graph (10,000 extracted Requirements, accepted as one set and confirmed,
#     then lint/ready/findings/export/report over that head),
#   * 8 concurrent GET /api/model/version requests against `plumb serve` on that head,
#   * the R0.7 customer-zero flow.
# Limits: every CLI command <= 120 s, peak RSS of every process <= 4 GiB, the 8 concurrent
# version reads <= 4 s in total, no OOM and no timeout. Any failure exits 1; per the plan the
# matching POST.0 optimisation then becomes a pre-release blocker (never accepted debt).
#
# BENCH_PROFILE=release (default, the shipped binary) or BENCH_PROFILE=debug (what `cargo test`
# builds). BENCH_PARAGRAPHS (default 50000) scales the workload down for scaling probes only;
# the gate is the default shape. BENCH_KEEP=1 keeps the work directory. Results: $WORK/commands.tsv.
# Linux and macOS (peak RSS from getrusage of each child process).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${BENCH_PROFILE:-release}"
CMD_LIMIT_S=120
RSS_LIMIT_BYTES=$((4 * 1024 * 1024 * 1024))
READS=8
READS_LIMIT_S=4
# A command still running after BENCH_HARD_KILL_S (default 900 s) is killed and reported as a
# timeout; slower-than-limit commands below it still fail the gate but let the workload finish,
# so every limit is measured in one run.
HARD_KILL_S="${BENCH_HARD_KILL_S:-900}"
# A whole workload driver (it also runs the in-process stage recording that feeds the CLI
# replay) is stopped after BENCH_DRIVER_BUDGET_S (default 3600 s) and reported as a timeout.
DRIVER_BUDGET_S="${BENCH_DRIVER_BUDGET_S:-3600}"
API_PORT="${BENCH_API_PORT:-8791}"
WORK="$(mktemp -d)"
SERVER_PID=""
cleanup() {
  if [[ -n "$SERVER_PID" ]]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  if [[ "${BENCH_KEEP:-0}" == "1" ]]; then echo "work directory kept: $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT

case "$PROFILE" in
  release) CARGO_PROFILE=(--release) ;;
  debug) CARGO_PROFILE=() ;;
  *) echo "BENCH_PROFILE must be release or debug" >&2; exit 2 ;;
esac

cd "$ROOT"
echo "== building plumb-cli and the benchmark drivers ($PROFILE)"
cargo build -q "${CARGO_PROFILE[@]}" -p plumb-cli
REAL_CLI="$ROOT/target/$PROFILE/plumb-cli"
test_binary() {
  cargo test -q "${CARGO_PROFILE[@]}" -p plumb-cli --test "$1" --no-run --message-format=json \
    | python3 -c '
import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("target", {}).get("name") == sys.argv[1] and m.get("executable"):
        print(m["executable"])
' "$1" | tail -1
}
SCALE_TEST="$(test_binary representative_scale)"
CZ_TEST="$(test_binary customer_zero)"

# The timing wrapper: runs one process, records wall time, peak RSS and exit status as a row
# of commands.tsv, and kills it at HARD_KILL_S. Test drivers find it as CARGO_BIN_EXE_plumb-cli.
TIMED="$WORK/plumb-cli"
cat >"$TIMED" <<'PY'
#!/usr/bin/env python3
import os, resource, subprocess, sys, time
real = os.environ["BENCH_REAL_CLI"]
label = os.environ.get("BENCH_LABEL", "cli")
start = time.monotonic()
child = subprocess.Popen([real] + sys.argv[1:])
try:
    code = child.wait(timeout=float(os.environ["BENCH_HARD_KILL_S"]))
except subprocess.TimeoutExpired:
    child.kill()
    child.wait()
    code = "timeout"
elapsed = time.monotonic() - start
rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
if sys.platform != "darwin":
    rss *= 1024  # Linux reports kilobytes
args = " ".join(a for a in sys.argv[1:] if not a.startswith("/"))
with open(os.environ["BENCH_LOG"], "a") as log:
    log.write(f"{label}\t{elapsed:.2f}\t{rss}\t{code}\t{args}\n")
sys.exit(code if isinstance(code, int) and code >= 0 else 1)
PY
chmod +x "$TIMED"
export BENCH_REAL_CLI="$REAL_CLI" BENCH_LOG="$WORK/commands.tsv" BENCH_HARD_KILL_S="$HARD_KILL_S"
printf 'label\tseconds\tpeak_rss_bytes\texit\tcommand\n' >"$BENCH_LOG"

# Runs a test driver with the wrapper in place of the CLI; the driver itself is recorded too.
# Usage: drive LABEL TEST_BINARY TEST_NAME OUTPUT_FILE [NAME=VALUE...]
drive() {
  local label="$1" binary="$2" test_name="$3" out="$4"
  shift 4
  local status=0
  rm -f "$WORK/driver.stats"
  env "CARGO_BIN_EXE_plumb-cli=$TIMED" BENCH_LABEL="$label" BENCH_DRIVER_OUT="$out" "$@" \
    /usr/bin/env python3 - "$binary" "$test_name" <<'PY' || status=$?
import os, resource, signal, subprocess, sys, time
start = time.time()
child = subprocess.Popen([sys.argv[1], "--exact", sys.argv[2], "--include-ignored", "--test-threads=1"],
                         stdout=open(os.environ["BENCH_DRIVER_OUT"], "w"), stderr=subprocess.STDOUT,
                         start_new_session=True)
try:
    code = child.wait(timeout=float(os.environ["BENCH_DRIVER_BUDGET_S"]))
except subprocess.TimeoutExpired:
    os.killpg(child.pid, signal.SIGKILL)
    child.wait()
    code = 124
rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
if sys.platform != "darwin":
    rss *= 1024
with open(os.environ["BENCH_DRIVER_STATS"], "w") as f:
    f.write(f"{time.time() - start:.2f} {rss}")
sys.exit(code if code >= 0 else 1)
PY
  local stats="0 0" shown="$status"
  [[ -f "$WORK/driver.stats" ]] && stats="$(cat "$WORK/driver.stats")"
  if [[ "$status" == 124 ]]; then shown=timeout; fi
  printf '%s-driver\t%s\t%s\t%s\t%s\n' "$label" "${stats% *}" "${stats#* }" "$shown" \
    "$test_name (test process incl. in-process stage recording)" >>"$BENCH_LOG"
  return "$status"
}
export BENCH_DRIVER_STATS="$WORK/driver.stats" BENCH_DRIVER_BUDGET_S="$DRIVER_BUDGET_S"

FAILED=0
echo "== representative scale: ${BENCH_PARAGRAPHS:-50000} paragraphs, 1 Requirement in 5"
drive scale "$SCALE_TEST" representative_scale_workload "$WORK/scale.log" SIM_DIR="$WORK/scale" || { echo "FAIL: the scale workload failed (see $WORK/scale.log)"; FAILED=1; }
grep -a '^BENCH' "$WORK/scale.log" || true

echo "== customer-zero flow"
drive customer-zero "$CZ_TEST" customer_zero_horizontal_flow "$WORK/customer-zero.log" || { echo "FAIL: customer-zero failed (see $WORK/customer-zero.log)"; FAILED=1; }

echo "== $READS concurrent GET /api/model/version on the scale head"
READS_TOTAL=""
if [[ -f "$WORK/scale/plumb.sqlite" ]]; then
  BENCH_LABEL=serve "$TIMED" --db "$WORK/scale/plumb.sqlite" --config "$WORK/scale/plumb.json" \
    serve --listen "127.0.0.1:$API_PORT" >"$WORK/serve.log" 2>&1 &
  SERVER_PID=$!
  READS_TOTAL=$(python3 - "$API_PORT" "$READS" <<'PY'
import sys, threading, time, urllib.request
port, n = sys.argv[1], int(sys.argv[2])
base = f"http://127.0.0.1:{port}"
for _ in range(240):
    try:
        urllib.request.urlopen(f"{base}/api/me", timeout=5).read()
        break
    except Exception:
        time.sleep(0.5)
else:
    print("unreachable"); sys.exit(0)
barrier = threading.Barrier(n)
results = [None] * n
def read(i):
    barrier.wait()
    t = time.monotonic()
    try:
        with urllib.request.urlopen(f"{base}/api/model/version", timeout=60) as r:
            r.read()
            results[i] = (r.status, time.monotonic() - t)
    except Exception as e:
        results[i] = (str(e), time.monotonic() - t)
threads = [threading.Thread(target=read, args=(i,)) for i in range(n)]
start = time.monotonic()
for t in threads: t.start()
for t in threads: t.join()
total = time.monotonic() - start
ok = all(r[0] == 200 for r in results)
print(f"{total:.3f} {'ok' if ok else 'error'} " + ",".join(f"{r[1]:.3f}" for r in results))
PY
)
  echo "version reads (total, status, each): $READS_TOTAL"
  # Stop the server (the wrapper records its peak RSS) and wait for the row.
  pkill -TERM -P "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
  SERVER_PID=""
else
  echo "FAIL: no scale database to serve"
  FAILED=1
fi

echo
echo "== results ($PROFILE)"
python3 - "$BENCH_LOG" <<'PY'
import sys
rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1])]
for label, seconds, rss, code, command in rows:
    rss = rss if not rss.isdigit() else f"{int(rss) / 2**20:.0f} MiB"
    command = command if len(command) <= 90 else command[:87] + "..."
    print(f"{label:<22}{seconds:>9}  {rss:>9}  {code:>7}  {command}")
PY
echo
python3 - "$BENCH_LOG" "$CMD_LIMIT_S" "$RSS_LIMIT_BYTES" "$READS_TOTAL" "$READS_LIMIT_S" <<'PY' || FAILED=1
import sys
log, cmd_limit, rss_limit, reads, reads_limit = sys.argv[1], float(sys.argv[2]), int(sys.argv[3]), sys.argv[4], float(sys.argv[5])
failures = []
rows = [l.rstrip("\n").split("\t") for l in open(log)][1:]
for label, seconds, rss, code, command in rows:
    seconds, rss = float(seconds), int(rss or 0)
    command = command if len(command) <= 90 else command[:87] + "..."
    if code == "timeout":
        failures.append(f"timeout: {label} {command}")
    if not label.endswith("-driver") and label != "serve" and seconds > cmd_limit:
        failures.append(f"{seconds:.1f} s > {cmd_limit:.0f} s: {label} {command}")
    if rss > rss_limit:
        failures.append(f"peak RSS {rss / 2**30:.2f} GiB > 4 GiB: {label} {command}")
cli = [r for r in rows if not r[0].endswith("-driver") and r[0] != "serve"]
slowest = max(cli, key=lambda r: float(r[1])) if cli else None
peak = max(rows, key=lambda r: int(r[2] or 0)) if rows else None
short = lambda c: c if len(c) <= 90 else c[:87] + "..."
if slowest:
    print(f"slowest CLI command: {float(slowest[1]):.2f} s ({slowest[0]}: {short(slowest[4])})")
if peak:
    print(f"peak RSS: {int(peak[2] or 0) / 2**20:.0f} MiB ({peak[0]}: {short(peak[4])})")
parts = reads.split()
if len(parts) < 2 or parts[1] != "ok":
    failures.append(f"concurrent version reads failed: {reads or 'not run'}")
else:
    print(f"{len(parts[2].split(','))} concurrent version reads: {float(parts[0]):.3f} s total (each: {parts[2]})")
    if float(parts[0]) > reads_limit:
        failures.append(f"concurrent version reads {float(parts[0]):.2f} s > {reads_limit:.0f} s")
for f in failures:
    print("FAIL:", f)
sys.exit(1 if failures else 0)
PY
if [[ "$FAILED" != "0" ]]; then
  echo "performance release gate: FAILED (the matching POST.0 items are pre-release blockers)"
  exit 1
fi
echo "performance release gate: passed"
