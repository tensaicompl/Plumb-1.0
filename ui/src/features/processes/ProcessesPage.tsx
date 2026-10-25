import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { apiClient, readReviewed, type Reviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { t } from "../../i18n";
import { ProcessDiagram } from "./ProcessDiagram";
import { ReadFailure } from "../../components/ReadFailure";
import { RoleGate } from "../../components/RoleGate";

type Process = components["schemas"]["Process"];
type ProcessStepRequest = components["schemas"]["ProcessStepRequest"];
type WriteResult = components["schemas"]["WriteResult"];

/** Every process query (list and detail) shares this prefix and is refetched after a write. */
export const processesQueryKey = ["processes"] as const;

async function fetchProcesses(): Promise<Process[]> {
  return (await readReviewed(apiClient.GET("/api/processes"))).items;
}

function fetchProcess(id: string): Promise<Reviewed<Process>> {
  return readReviewed(apiClient.GET("/api/processes/{id}", { params: { path: { id } } }));
}

/** A zero-based position, or null when the input is not a non-negative integer. */
function parsePosition(input: string): number | null {
  return /^[0-9]+$/.test(input) ? Number(input) : null;
}

function errorCode(error: unknown): string | null {
  if (error instanceof WriteFailedError && typeof error.body === "object" && error.body !== null) {
    const code = (error.body as Record<string, unknown>)["code"];
    return typeof code === "string" ? code : null;
  }
  return null;
}

/**
 * The three U2.4 semantic edits (Hotfix 079): add an existing operation, reorder a step and move a
 * task to another performer lane. Each explicit submission is one POST /api/processes/{id}/steps
 * through the versioned mutation; the server builds and commits the semantic change.
 */
function SemanticEdits({ process }: { process: Reviewed<Process> }) {
  const queryClient = useQueryClient();
  const steps = useVersionedMutation<WriteResult, ProcessStepRequest>(
    (body, ifMatch) =>
      apiClient.POST("/api/processes/{id}/steps", {
        params: { path: { id: process.id }, header: { "If-Match": ifMatch } },
        body,
      }),
    process.etag,
  );
  const [addOperation, setAddOperation] = useState("");
  const [addPosition, setAddPosition] = useState("");
  const [addReason, setAddReason] = useState("");
  const [moveNode, setMoveNode] = useState("");
  const [movePosition, setMovePosition] = useState("");
  const [moveReason, setMoveReason] = useState("");
  const [laneNode, setLaneNode] = useState("");
  const [lanePerformer, setLanePerformer] = useState("");
  const [laneReason, setLaneReason] = useState("");

  const reorderable = process.nodes.filter((node) => node.kind !== "start" && node.kind !== "end");
  const tasks = process.nodes.filter((node) => node.kind === "human_task" || node.kind === "service_task");
  const reasonOf = (text: string) => (text === "" ? null : text);
  const submit = (body: ProcessStepRequest) => (event: FormEvent) => {
    event.preventDefault();
    steps.mutate(body, { onSuccess: () => void queryClient.invalidateQueries({ queryKey: processesQueryKey }) });
  };
  const addAt = parsePosition(addPosition);
  const moveTo = parsePosition(movePosition);

  return (
    <section aria-label={t("processes.edits")} className="space-y-3">
      <h2 className="font-semibold">{t("processes.edits")}</h2>
      {steps.error instanceof WriteFailedError && (
        <p role="alert" className="text-sm text-red-800" data-testid="process-write-error">
          {t("processes.writeFailed")} <span className="font-mono">{errorCode(steps.error) ?? String(steps.error.status)}</span> <span>{writeErrorMessage(steps.error)}</span>
        </p>
      )}

      <form
        aria-label={t("processes.add.title")}
        className="space-y-1 rounded border p-2"
        onSubmit={submit({ op: "add", operation_ref: addOperation, position: addAt ?? 0, reason: reasonOf(addReason) })}
      >
        <h3 className="font-semibold">{t("processes.add.title")}</h3>
        <label className="block text-sm">
          {t("processes.add.operation")}
          <input className="ml-2 rounded border p-1 font-mono" value={addOperation} data-testid="add-operation" onChange={(e) => setAddOperation(e.target.value)} />
        </label>
        <label className="block text-sm">
          {t("processes.position")}
          <input type="number" min={0} step={1} className="ml-2 rounded border p-1" value={addPosition} data-testid="add-position" onChange={(e) => setAddPosition(e.target.value)} />
        </label>
        <label className="block text-sm">
          {t("processes.reason")}
          <input className="ml-2 rounded border p-1" value={addReason} data-testid="add-reason" onChange={(e) => setAddReason(e.target.value)} />
        </label>
        <button type="submit" className="rounded border px-3 py-1" data-testid="add-submit" disabled={steps.isPending || addOperation === "" || addAt === null}>
          {t("processes.submit")}
        </button>
      </form>

      <form
        aria-label={t("processes.reorder.title")}
        className="space-y-1 rounded border p-2"
        onSubmit={submit({ op: "move", node_ref: moveNode, position: moveTo ?? 0, reason: reasonOf(moveReason) })}
      >
        <h3 className="font-semibold">{t("processes.reorder.title")}</h3>
        <label className="block text-sm">
          {t("processes.node")}
          <select className="ml-2 rounded border font-mono" value={moveNode} data-testid="move-node" onChange={(e) => setMoveNode(e.target.value)}>
            <option value="">{t("processes.chooseNode")}</option>
            {reorderable.map((node) => (
              <option key={node.id} value={node.id}>
                {node.id}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          {t("processes.position")}
          <input type="number" min={0} step={1} className="ml-2 rounded border p-1" value={movePosition} data-testid="move-position" onChange={(e) => setMovePosition(e.target.value)} />
        </label>
        <label className="block text-sm">
          {t("processes.reason")}
          <input className="ml-2 rounded border p-1" value={moveReason} data-testid="move-reason" onChange={(e) => setMoveReason(e.target.value)} />
        </label>
        <button type="submit" className="rounded border px-3 py-1" data-testid="move-submit" disabled={steps.isPending || moveNode === "" || moveTo === null}>
          {t("processes.submit")}
        </button>
      </form>

      <form
        aria-label={t("processes.moveLane.title")}
        className="space-y-1 rounded border p-2"
        onSubmit={submit({ op: "move_lane", node_ref: laneNode, performer_ref: lanePerformer, reason: reasonOf(laneReason) })}
      >
        <h3 className="font-semibold">{t("processes.moveLane.title")}</h3>
        <label className="block text-sm">
          {t("processes.node")}
          <select className="ml-2 rounded border font-mono" value={laneNode} data-testid="lane-node" onChange={(e) => setLaneNode(e.target.value)}>
            <option value="">{t("processes.chooseNode")}</option>
            {tasks.map((node) => (
              <option key={node.id} value={node.id}>
                {node.id}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          {t("processes.moveLane.performer")}
          <input className="ml-2 rounded border p-1 font-mono" value={lanePerformer} data-testid="lane-performer" onChange={(e) => setLanePerformer(e.target.value)} />
        </label>
        <label className="block text-sm">
          {t("processes.reason")}
          <input className="ml-2 rounded border p-1" value={laneReason} data-testid="lane-reason" onChange={(e) => setLaneReason(e.target.value)} />
        </label>
        <button type="submit" className="rounded border px-3 py-1" data-testid="lane-submit" disabled={steps.isPending || laneNode === "" || lanePerformer === ""}>
          {t("processes.submit")}
        </button>
      </form>
    </section>
  );
}

/** The selected process: its summary, validation, branches, swimlane diagram and semantic edits. */
function ProcessView({ id }: { id: string }) {
  const process = useQuery({ queryKey: [...processesQueryKey, "detail", id], queryFn: () => fetchProcess(id) });
  if (process.isError) {
    return <ReadFailure error={process.error} messageKey="processes.loadFailed" />;
  }
  if (process.data === undefined) {
    return <p>{t("processes.loading")}</p>;
  }
  const data = process.data;
  return (
    <div className="space-y-4" data-process={data.id}>
      <h2 className="text-lg font-semibold">
        {data.name} <span className="font-mono text-sm">{data.id}</span>
      </h2>
      <section aria-label={t("processes.validation")} className="text-sm">
        <h3 className="font-semibold">{t("processes.validation")}</h3>
        {data.validation.length === 0 ? (
          <p data-testid="validation-empty">{t("processes.noValidation")}</p>
        ) : (
          <ul className="font-mono" data-testid="validation-list">
            {data.validation.map((value, index) => (
              <li key={index}>{value}</li>
            ))}
          </ul>
        )}
      </section>
      <section aria-label={t("processes.branches")} className="text-sm">
        <h3 className="font-semibold">{t("processes.branches")}</h3>
        {data.branches.length === 0 ? (
          <p>{t("processes.none")}</p>
        ) : (
          <ul className="font-mono" data-testid="branch-list">
            {data.branches.map((value, index) => (
              <li key={index}>{value}</li>
            ))}
          </ul>
        )}
      </section>
      <ProcessDiagram input={data} />
      <RoleGate>
        <SemanticEdits key={data.id} process={data} />
      </RoleGate>
    </div>
  );
}

/** U2.4 process view: explicit selection among the server processes on /model/processes. */
export function ProcessesPage() {
  const processes = useQuery({ queryKey: [...processesQueryKey, "list"], queryFn: fetchProcesses });
  const [selected, setSelected] = useState("");

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("processes.title")}</h1>
      {processes.isError && <ReadFailure error={processes.error} messageKey="processes.loadFailed" />}
      {processes.data === undefined && !processes.isError && <p>{t("processes.loading")}</p>}
      {processes.data !== undefined && processes.data.length === 0 && <p>{t("processes.empty")}</p>}
      {processes.data !== undefined && processes.data.length > 0 && (
        <label className="block text-sm">
          {t("processes.selectProcess")}
          <select className="ml-2 rounded border" value={selected} data-testid="process-select" onChange={(e) => setSelected(e.target.value)}>
            <option value="">{t("processes.chooseProcess")}</option>
            {processes.data.map((process) => (
              <option key={process.id} value={process.id}>
                {process.name + " (" + process.id + ")"}
              </option>
            ))}
          </select>
        </label>
      )}
      {selected !== "" && <ProcessView key={selected} id={selected} />}
    </div>
  );
}
