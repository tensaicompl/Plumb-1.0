#!/usr/bin/env python3
from pathlib import Path
import json, re, hashlib, sys, collections

ROOT=Path(__file__).resolve().parents[1]

def fail(msg):
    print("ERROR:",msg,file=sys.stderr)
    return 1

errors=[]

def err(msg):
    errors.append(msg)

tasks_doc=json.loads((ROOT/"docs/plan/PLUMB-IMPLEMENTATION-TASKS-v3.json").read_text())
scope=json.loads((ROOT/"docs/plan/EXECUTION-SCOPE.json").read_text())
refs_doc=json.loads((ROOT/"docs/plan/SOURCE-REFERENCE-INDEX.json").read_text())
tasks=tasks_doc["tasks"]
refs={r["id"]:r for r in refs_doc["references"]}

# Scope is exact.
if scope.get("release")!="pilot-v3-foundation":
    err("execution release must equal pilot-v3-foundation")
if scope.get("allowed_scope")!=["NOW"]:
    err("allowed_scope must equal exactly [NOW]")

# Task schema, IDs, dependency order.
required={"id","title","phase","scope","depends_on","files","steps","tests","acceptance","source_refs","forbidden","commands","commit_message"}
ids=[t.get("id") for t in tasks]
if None in ids or len(ids)!=len(set(ids)):
    err("task IDs must be non-null and unique")
order={tid:i for i,tid in enumerate(ids)}
by_id={t["id"]:t for t in tasks}
for t in tasks:
    missing=required-set(t)
    if missing: err(f"{t['id']} missing fields {sorted(missing)}")
    if t["scope"] not in {"NOW","LATER"}: err(f"{t['id']} invalid scope {t['scope']}")
    for d in t["depends_on"]:
        if d not in by_id: err(f"{t['id']} depends on unknown {d}")
        elif order[d] >= order[t["id"]]: err(f"{t['id']} dependency {d} must appear earlier")
        elif t["scope"]=="NOW" and by_id[d]["scope"]=="LATER":
            err(f"NOW task {t['id']} depends on LATER task {d}")
    if not t["steps"]: err(f"{t['id']} has no steps")
    if not t["acceptance"]: err(f"{t['id']} has no acceptance criteria")
    if not t["source_refs"]: err(f"{t['id']} has no source_refs")
    for rid in t["source_refs"]:
        if rid not in refs: err(f"{t['id']} references unknown source ref {rid}")

# Dependency cycle check.
vis={}
def dfs(n,stack):
    vis[n]=1
    for d in by_id[n]["depends_on"]:
        if d not in by_id: continue
        if vis.get(d)==1: err("dependency cycle: "+" -> ".join(stack+[d]))
        elif vis.get(d,0)==0: dfs(d,stack+[d])
    vis[n]=2
for n in ids:
    if vis.get(n,0)==0: dfs(n,[n])

# Markdown locator support.
def headings(text):
    out=set()
    for line in text.splitlines():
        m=re.match(r"^#{1,6}\s+([0-9]+(?:\.[0-9]+)*|[A-Z][A-Z0-9]*(?:\.[0-9]+)?)\b",line)
        if m: out.add(m.group(1))
    return out

def has_token(text, token):
    # Task IDs appear either in headings or bold list task definitions.
    return bool(re.search(rf"(?m)(^#+\s+{re.escape(token)}\b|\*\*{re.escape(token)}(?:\s|\[|\*))",text))

for rid,r in refs.items():
    path=ROOT/r["file"]
    if not path.is_file():
        err(f"{rid} file missing: {r['file']}")
        continue
    loc=r["locator"]
    text=path.read_text(errors="ignore") if path.suffix.lower() in {".md",".yaml",".yml",".json",".toml",".txt"} else ""
    h=headings(text) if text else set()

    def validate_component(c):
        kind=c["kind"]
        if kind=="whole_file":
            return
        if kind=="yaml_filter":
            # The rules YAML is generated in a stable one-rule-per-entry form.
            target=str(c["equals"])
            if not re.search(rf"(?m)^\s*gate:\s*['\"]?{re.escape(target)}['\"]?\s*$",text):
                err(f"{rid} yaml filter gate={target} matches nothing in {r['file']}")
        elif kind=="contains":
            for p in c.get("phrases",[]):
                if p.lower() not in text.lower():
                    err(f"{rid} phrase not found in {r['file']}: {p}")
        elif kind=="sections":
            for s in c.get("sections",[]):
                if s not in h:
                    err(f"{rid} section {s} not found in {r['file']}")
            for p in c.get("contains",[]):
                if p.lower() not in text.lower():
                    err(f"{rid} descriptive phrase not found in {r['file']}: {p}")
        elif kind=="heading_token":
            tok=c["token"]
            if tok not in h and not has_token(text,tok):
                err(f"{rid} heading/token {tok} not found in {r['file']}")
            for p in c.get("contains",[]):
                if p.lower() not in text.lower():
                    err(f"{rid} descriptive phrase not found in {r['file']}: {p}")
        elif kind=="section_and_tokens":
            if c["section"] not in h:
                err(f"{rid} section {c['section']} not found in {r['file']}")
            for tok in c["tokens"]:
                if not has_token(text,tok):
                    err(f"{rid} task token {tok} not found in {r['file']}")
        elif kind=="composite":
            for x in c.get("components",[]): validate_component(x)
        else:
            err(f"{rid} unsupported locator kind {kind}")
    validate_component(loc)

# Supporting checksums: every listed file must exist and match.
checksum=ROOT/"docs/plan/SUPPORTING-DOCS.sha256"
protected=set()
for line in checksum.read_text().splitlines():
    if not line.strip(): continue
    expected,rel=line.split("  ",1)
    protected.add(rel)
    p=ROOT/rel
    if not p.is_file():
        err(f"checksummed file missing: {rel}")
    else:
        actual=hashlib.sha256(p.read_bytes()).hexdigest()
        if actual!=expected: err(f"checksum mismatch: {rel}")

# Implementation tasks may never write checksummed authorities.
for t in tasks:
    overlap=sorted(protected.intersection(t["files"]))
    if overlap: err(f"{t['id']} write allowlist contains protected authority: {overlap}")

# Markdown task headings must mirror manifest exactly once.
plan=(ROOT/"docs/plan/PLUMB-IMPLEMENTATION-PLAN-v3.md").read_text()
heading_ids=re.findall(r"(?m)^### `([^`]+)` —",plan)
counts=collections.Counter(heading_ids)
for tid in ids:
    if counts[tid]!=1: err(f"plan heading count for {tid} is {counts[tid]}, expected 1")
extra=sorted(set(heading_ids)-set(ids))
if extra: err(f"plan has extra task headings: {extra}")

# Rule pack gate counts using dependency-free text parsing.
rules_text=(ROOT/"config/profiles/plumb-software-2026.1-rules.yaml").read_text()
gates=re.findall(r"(?m)^\s*gate:\s*['\"]?([A-Z][A-Z0-9-]*)['\"]?\s*$",rules_text)
counts=collections.Counter(gates)
expected={"I0":6,"F1":11,"F2":24,"F3":6,"F4":7,"Q1":8,"A1":9,"A2":9,"A3":14,"A4":9,"D1":9,"D2":10,"C1":11}
for gate,n in expected.items():
    if counts[gate]!=n: err(f"rule count {gate}={counts[gate]}, expected {n}")
if sum(counts.values())!=133: err(f"total rule count {sum(counts.values())}, expected 133")

# Fixture fixed counts via dependency-free source patterns.
req_text=(ROOT/"fixtures/hr-leave/requirements.md").read_text()
req_ids=re.findall(r"\*\*(HR-\d{3})\*\*",req_text)
if len(req_ids)!=32 or len(set(req_ids))!=32:
    err(f"requirements.md must contain exactly 32 unique HR-xxx requirements; got {len(req_ids)}/{len(set(req_ids))}")

scenario_text=(ROOT/"fixtures/hr-leave/scenarios.yaml").read_text()
scenario_ids=re.findall(r"(?m)^\s*-\s+id:\s*([^\s#]+)",scenario_text)
if len(scenario_ids)!=44 or len(set(scenario_ids))!=44:
    err(f"scenarios.yaml must contain exactly 44 unique top-level scenario IDs; got {len(scenario_ids)}/{len(set(scenario_ids))}")

# No unresolved planning placeholders in task actions/acceptance.
# E0.4 is allowed to name the markers it explicitly scans for.
for t in tasks:
    if t["id"]=="E0.4":
        continue
    serialized=json.dumps({k:t[k] for k in ["steps","tests","acceptance","commands"]},ensure_ascii=False)
    for marker in ["TBD","FIXME","XXX"]:
        if re.search(rf"\b{marker}\b",serialized):
            err(f"{t['id']} contains unresolved marker {marker}")

if errors:
    for e in errors: print("ERROR:",e,file=sys.stderr)
    print(f"PLAN CONTRACT FAILED: {len(errors)} error(s)",file=sys.stderr)
    sys.exit(1)

print(f"PLAN CONTRACT OK: {len(tasks)} tasks, {len(refs)} source references, 133 validation rules")
