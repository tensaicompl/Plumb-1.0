import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError } from "../../api/useVersionedMutation";
import { DiffViewer, type ModelDiff } from "../../components/DiffViewer";
import { ProvenanceDrawer, type ProvenanceItem } from "../../components/ProvenanceDrawer";
import { t } from "../../i18n";

export type RequirementDetailView = components["schemas"]["RequirementDetail"];
export type SemanticChangePreview = components["schemas"]["SemanticChangePreviewResponse"];
export type SemanticElementDiff = components["schemas"]["SemanticElementDiff"];
type WriteResult = components["schemas"]["WriteResult"];

/** The query key of one requirement (GET /api/requirements/{id}). */
export function requirementQueryKey(id: string) {
  return ["requirements", "detail", id] as const;
}

async function fetchRequirement(id: string): Promise<RequirementDetailView> {
  const { data, response } = await apiClient.GET("/api/requirements/{id}", { params: { path: { id } } });
  if (data === undefined) {
    throw new Error("GET /api/requirements/{id} failed with HTTP " + String(response.status));
  }
  return data;
}

/**
 * Presentation-only reshaping of the server SemanticElementDiff for the shared DiffViewer: `from`
 * is the baseline semantic hash, `to` the candidate semantic hash, and the three ID lists are
 * passed through unchanged (never sorted, filtered or recomputed here).
 */
export function toViewerDiff(diff: SemanticElementDiff): ModelDiff {
  return {
    from: diff.base_semantic_hash,
    to: diff.candidate_semantic_hash,
    added: diff.added,
    removed: diff.removed,
    modified: diff.modified,
  };
}

/** The server error code of a failed write, when the body carries one. */
export function errorCode(error: unknown): string | null {
  if (error instanceof WriteFailedError && typeof error.body === "object" && error.body !== null) {
    const code = (error.body as Record<string, unknown>)["code"];
    return typeof code === "string" ? code : null;
  }
  return null;
}

/** A confirmation refused because the previewed proposal's baseline revision is no longer the head. */
export function isBaselineChanged(error: unknown): boolean {
  return error instanceof WriteFailedError && error.status === 409 && errorCode(error) === "E_PROPOSAL_BASELINE_CHANGED";
}

function WriteError({ error }: { error: Error | null }) {
  if (error === null || !(error instanceof WriteFailedError)) {
    return null;
  }
  return (
    <p role="alert" className="text-sm text-red-800">
      {t("write.failed")} <span className="font-mono">{errorCode(error) ?? String(error.status)}</span>
    </p>
  );
}

/**
 * A server-authoritative semantic change preview (Hotfix 073/074): the pending proposal, its server
 * intake and the exact server semantic element diff. Nothing is committed until the explicit
 * confirmation; the candidate hash describes the domain proposal only, never the committed model.
 */
export function SemanticChangePreviewPanel({
  preview,
  reason,
  confirming,
  onConfirm,
  onDiscard,
}: {
  preview: SemanticChangePreview;
  reason: string;
  confirming: boolean;
  onConfirm: () => void;
  onDiscard: () => void;
}) {
  const intake = preview.intake;
  return (
    <section aria-label={t("preview.title")} className="space-y-3 rounded border border-amber-600 p-3 text-sm" data-testid="semantic-preview">
      <h3 className="font-semibold">{t("preview.title")}</h3>
      <p>{t("preview.notCommitted")}</p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
        <dt>{t("preview.proposal")}</dt>
        <dd className="font-mono" data-testid="preview-proposal">
          {preview.proposal_id}
        </dd>
        <dt>{t("preview.action")}</dt>
        <dd className="font-mono" data-testid="preview-action">
          {preview.action}
        </dd>
        <dt>{t("preview.baseRevision")}</dt>
        <dd className="font-mono">{preview.revision}</dd>
        <dt>{t("preview.baseHash")}</dt>
        <dd className="break-all font-mono" data-testid="preview-base-hash">
          {preview.diff.base_semantic_hash}
        </dd>
        <dt>{t("preview.candidateHash")}</dt>
        <dd className="break-all font-mono" data-testid="preview-candidate-hash">
          {preview.diff.candidate_semantic_hash}
        </dd>
        <dt>{t("preview.reason")}</dt>
        <dd data-testid="preview-reason">{reason}</dd>
      </dl>
      <p className="text-slate-600">{t("preview.candidateNote")}</p>
      <section aria-label={t("preview.intake")} className="space-y-1">
        <h4 className="font-semibold">{t("preview.intake")}</h4>
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
          <dt>{t("proposal.requiresReason")}</dt>
          <dd data-testid="preview-requires-reason">{t(intake.requires_reason ? "proposal.yes" : "proposal.no")}</dd>
          <dt>{t("proposal.reasonCodes")}</dt>
          <dd className="font-mono" data-testid="preview-reason-codes">
            {intake.reason_codes.length === 0 ? t("proposal.none") : intake.reason_codes.join(", ")}
          </dd>
        </dl>
        {intake.conflicts_with.length > 0 && (
          <section aria-label={t("proposal.conflictsWith")}>
            <h5 className="font-semibold">{t("proposal.conflictsWith")}</h5>
            <ul data-testid="preview-conflicts">
              {intake.conflicts_with.map((conflict) => (
                <li key={conflict.target_ref + conflict.reason}>
                  <span className="font-mono">{conflict.target_ref}</span> {conflict.reason}
                </li>
              ))}
            </ul>
          </section>
        )}
        {intake.duplicate_candidates.length > 0 && (
          <section aria-label={t("proposal.duplicateCandidates")}>
            <h5 className="font-semibold">{t("proposal.duplicateCandidates")}</h5>
            <ul>
              {intake.duplicate_candidates.map((d) => (
                <li key={d.subject_ref + d.candidate_ref}>
                  <span className="font-mono">{d.candidate_ref}</span> {d.score}
                </li>
              ))}
            </ul>
          </section>
        )}
        {intake.validation_findings.length > 0 && (
          <section aria-label={t("proposal.validationFindings")}>
            <h5 className="font-semibold">{t("proposal.validationFindings")}</h5>
            <ul>
              {intake.validation_findings.map((finding) => (
                <li key={finding.id}>
                  <span className="font-mono">{finding.code}</span> {finding.message}
                </li>
              ))}
            </ul>
          </section>
        )}
      </section>
      <DiffViewer diff={toViewerDiff(preview.diff)} />
      <div className="flex gap-2">
        <button type="button" disabled={confirming} onClick={onConfirm} className="rounded border px-3 py-1" data-testid="preview-confirm">
          {t("preview.confirm")}
        </button>
        <button type="button" disabled={confirming} onClick={onDiscard} className="rounded border px-3 py-1">
          {t("preview.discard")}
        </button>
      </div>
    </section>
  );
}

/** The proposal ID and reason of one server preview awaiting explicit confirmation. */
interface PendingPreview {
  readonly preview: SemanticChangePreview;
  readonly reason: string;
}

/**
 * The requirement split workflow: preview (server-built create-class proposal, Hotfix 074), then an
 * explicit confirmation through POST /api/proposals/{id}/create with the same reason. A moved
 * baseline discards the preview and requires a new one; nothing is retried.
 */
function SplitAction({ requirementId }: { requirementId: string }) {
  const queryClient = useQueryClient();
  const [parts, setParts] = useState<string[]>(["", ""]);
  const [reason, setReason] = useState("");
  const [pending, setPending] = useState<PendingPreview | null>(null);
  const [stale, setStale] = useState(false);

  const preview = useVersionedMutation<SemanticChangePreview, { parts: string[]; reason: string }>((body, ifMatch) =>
    apiClient.POST("/api/requirements/{id}/split/preview", {
      params: { path: { id: requirementId }, header: { "If-Match": ifMatch } },
      body,
    }),
  );
  const confirm = useVersionedMutation<WriteResult, PendingPreview>((target, ifMatch) =>
    apiClient.POST("/api/proposals/{id}/create", {
      params: { path: { id: target.preview.proposal_id }, header: { "If-Match": ifMatch } },
      body: { reason: target.reason },
    }),
  );

  const requestPreview = () => {
    setStale(false);
    confirm.reset();
    const body = { parts, reason };
    preview.mutate(body, { onSuccess: (data) => setPending({ preview: data, reason: body.reason }) });
  };

  const confirmPreview = () => {
    if (pending === null) {
      return;
    }
    confirm.mutate(pending, {
      onSuccess: () => {
        setPending(null);
        setParts(["", ""]);
        setReason("");
        void queryClient.invalidateQueries();
      },
      onError: (error) => {
        if (isBaselineChanged(error)) {
          setPending(null);
          setStale(true);
          void queryClient.invalidateQueries();
        }
      },
    });
  };

  return (
    <section aria-label={t("requirement.split.title")} className="space-y-2 rounded border p-3" data-action="split">
      <h3 className="font-semibold">{t("requirement.split.title")}</h3>
      {parts.map((part, index) => (
        <label key={index} className="block text-sm">
          {t("requirement.split.part")} {index + 1}
          <input
            className="ml-2 w-full rounded border p-1"
            value={part}
            data-testid="split-part"
            disabled={pending !== null}
            onChange={(event) => setParts((current) => current.map((p, i) => (i === index ? event.target.value : p)))}
          />
        </label>
      ))}
      <button
        type="button"
        className="rounded border px-2 text-sm"
        disabled={pending !== null}
        onClick={() => setParts((current) => [...current, ""])}
      >
        {t("requirement.split.addPart")}
      </button>
      <label className="block text-sm">
        {t("requirement.reason")}
        <input
          className="ml-2 w-full rounded border p-1"
          value={reason}
          data-testid="split-reason"
          disabled={pending !== null}
          onChange={(event) => setReason(event.target.value)}
        />
      </label>
      {pending === null && (
        <button
          type="button"
          className="rounded border px-3 py-1"
          disabled={preview.isPending || parts.length < 2 || reason === ""}
          onClick={requestPreview}
          data-testid="split-preview"
        >
          {t("preview.request")}
        </button>
      )}
      {stale && (
        <p role="alert" className="text-sm text-red-800" data-testid="preview-stale">
          {t("preview.stale")}
        </p>
      )}
      <WriteError error={preview.error} />
      {!stale && <WriteError error={confirm.error} />}
      {pending !== null && (
        <SemanticChangePreviewPanel
          preview={pending.preview}
          reason={pending.reason}
          confirming={confirm.isPending}
          onConfirm={confirmPreview}
          onDiscard={() => setPending(null)}
        />
      )}
    </section>
  );
}

/** The requirement detail: separate text, EARS, evidence, lint, links and governed actions. */
export function RequirementDetail({ requirementId }: { requirementId: string }) {
  const queryClient = useQueryClient();
  const requirement = useQuery({
    queryKey: requirementQueryKey(requirementId),
    queryFn: () => fetchRequirement(requirementId),
  });
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  const [classValue, setClassValue] = useState("");
  const [waiveFinding, setWaiveFinding] = useState("");
  const [waiveReason, setWaiveReason] = useState("");
  const [duplicateOf, setDuplicateOf] = useState("");
  const [duplicateReason, setDuplicateReason] = useState("");

  const onWritten = { onSuccess: () => void queryClient.invalidateQueries() };
  const path = { id: requirementId };
  const classify = useVersionedMutation<WriteResult, string>((value, ifMatch) =>
    apiClient.POST("/api/requirements/{id}/classify", { params: { path, header: { "If-Match": ifMatch } }, body: { class: value } }),
  );
  const acceptEars = useVersionedMutation<WriteResult, string>((text, ifMatch) =>
    apiClient.POST("/api/requirements/{id}/accept-ears", { params: { path, header: { "If-Match": ifMatch } }, body: { text } }),
  );
  const waive = useVersionedMutation<WriteResult, { finding_id: string; reason: string }>((body, ifMatch) =>
    apiClient.POST("/api/requirements/{id}/waive-finding", { params: { path, header: { "If-Match": ifMatch } }, body }),
  );
  const duplicate = useVersionedMutation<WriteResult, { of: string; reason: string }>((body, ifMatch) =>
    apiClient.POST("/api/requirements/{id}/mark-duplicate", { params: { path, header: { "If-Match": ifMatch } }, body }),
  );

  if (requirement.isError) {
    return <p role="alert">{t("requirement.loadFailed")}</p>;
  }
  if (requirement.data === undefined) {
    return <p>{t("requirement.loading")}</p>;
  }
  const detail = requirement.data;
  // Absent and null both mean not calculated by the server; nothing is derived locally.
  const ears = detail.ears_suggestion ?? null;
  const readiness = detail.readiness ?? null;
  const evidence: ProvenanceItem[] = detail.spans.map((ref) => ({ category: "evidence", ref }));
  const linked: readonly { key: "operations" | "processes" | "scenarios"; label: "requirement.linked.operations" | "requirement.linked.processes" | "requirement.linked.scenarios" }[] = [
    { key: "operations", label: "requirement.linked.operations" },
    { key: "processes", label: "requirement.linked.processes" },
    { key: "scenarios", label: "requirement.linked.scenarios" },
  ];

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">
        {t("requirement.title")} <span className="font-mono" data-testid="requirement-id">{detail.id}</span>
      </h1>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 text-sm">
        <dt>{t("requirements.column.class")}</dt>
        <dd className="font-mono" data-testid="requirement-class">
          {detail.class}
        </dd>
        <dt>{t("requirements.column.status")}</dt>
        <dd className="font-mono">{detail.status}</dd>
        <dt>{t("requirements.column.readiness")}</dt>
        <dd data-testid="requirement-readiness">{readiness === null ? t("readiness.notCalculated") : String(readiness)}</dd>
      </dl>

      <section aria-label={t("requirement.section.original")} data-section="original" className="rounded border p-3">
        <h2 className="font-semibold">{t("requirement.section.original")}</h2>
        <p data-testid="requirement-text">{detail.text}</p>
      </section>

      <section aria-label={t("requirement.section.ears")} data-section="ears" className="rounded border p-3">
        <h2 className="font-semibold">{t("requirement.section.ears")}</h2>
        {ears === null ? (
          <p data-testid="ears-unavailable">{t("requirement.ears.notCalculated")}</p>
        ) : (
          <>
            <p data-testid="ears-suggestion">{ears}</p>
            <button
              type="button"
              className="mt-2 rounded border px-3 py-1"
              data-testid="ears-accept"
              disabled={acceptEars.isPending}
              onClick={() => {
                acceptEars.mutate(ears, onWritten);
              }}
            >
              {t("requirement.ears.accept")}
            </button>
            <WriteError error={acceptEars.error} />
          </>
        )}
      </section>

      <section aria-label={t("requirement.section.evidence")} data-section="evidence" className="rounded border p-3">
        <h2 className="font-semibold">{t("requirement.section.evidence")}</h2>
        {detail.spans.length === 0 ? (
          <p>{t("requirement.evidence.none")}</p>
        ) : (
          <>
            <ul className="font-mono text-sm" data-testid="evidence-refs">
              {detail.spans.map((ref) => (
                <li key={ref}>{ref}</li>
              ))}
            </ul>
            <button type="button" className="mt-2 rounded border px-3 py-1" onClick={() => setEvidenceOpen(true)}>
              {t("requirement.evidence.open")}
            </button>
          </>
        )}
        <ProvenanceDrawer open={evidenceOpen} onOpenChange={setEvidenceOpen} subjectRef={detail.id} items={evidence} />
      </section>

      <section
        aria-label={t("requirement.section.lint")}
        data-section="lint"
        data-lint={detail.lint_current ? "current" : "unavailable"}
        className="rounded border p-3"
      >
        <h2 className="font-semibold">{t("requirement.section.lint")}</h2>
        {/* R0.4: without a current F1 report lint is unavailable, never "no findings". */}
        {!detail.lint_current ? (
          <p className="text-sm" data-testid="lint-unavailable">
            {t("readiness.notCalculated")}
          </p>
        ) : (
          <>
            <h3 className="text-sm font-semibold">{t("requirement.lint.findings")}</h3>
            {detail.lint_findings.length === 0 ? (
              <p className="text-sm">{t("requirement.lint.none")}</p>
            ) : (
              <ul className="font-mono text-sm" data-testid="lint-findings">
                {detail.lint_findings.map((finding) => (
                  <li key={finding}>{finding}</li>
                ))}
              </ul>
            )}
            <h3 className="text-sm font-semibold">{t("requirement.lint.smells")}</h3>
            {detail.smells.length === 0 ? (
              <p className="text-sm">{t("requirement.lint.none")}</p>
            ) : (
              <ul className="font-mono text-sm" data-testid="lint-smells">
                {detail.smells.map((smell) => (
                  <li key={smell}>{smell}</li>
                ))}
              </ul>
            )}
          </>
        )}
      </section>

      <section aria-label={t("requirement.section.linked")} data-section="linked" className="rounded border p-3">
        <h2 className="font-semibold">{t("requirement.section.linked")}</h2>
        {linked.map(({ key, label }) => (
          <div key={key} className="text-sm">
            <h3 className="font-semibold">{t(label)}</h3>
            {detail.linked[key].length === 0 ? (
              <p>{t("requirement.linked.none")}</p>
            ) : (
              <ul className="font-mono" data-linked={key}>
                {detail.linked[key].map((ref) => (
                  <li key={ref}>{ref}</li>
                ))}
              </ul>
            )}
          </div>
        ))}
      </section>

      <section aria-label={t("requirement.section.actions")} data-section="actions" className="space-y-3">
        <h2 className="font-semibold">{t("requirement.section.actions")}</h2>

        <form
          aria-label={t("requirement.classify.title")}
          className="space-y-2 rounded border p-3"
          onSubmit={(event) => {
            event.preventDefault();
            classify.mutate(classValue, onWritten);
          }}
        >
          <label className="block text-sm">
            {t("requirement.classify.class")}
            <input className="ml-2 rounded border p-1" value={classValue} data-testid="classify-class" onChange={(event) => setClassValue(event.target.value)} />
          </label>
          <button type="submit" className="rounded border px-3 py-1" disabled={classify.isPending || classValue === ""}>
            {t("requirement.classify.submit")}
          </button>
          <WriteError error={classify.error} />
        </form>

        <form
          aria-label={t("requirement.waive.title")}
          className="space-y-2 rounded border p-3"
          onSubmit={(event) => {
            event.preventDefault();
            waive.mutate({ finding_id: waiveFinding, reason: waiveReason }, onWritten);
          }}
        >
          <label className="block text-sm">
            {t("requirement.waive.finding")}
            <select className="ml-2 rounded border" value={waiveFinding} data-testid="waive-finding" onChange={(event) => setWaiveFinding(event.target.value)}>
              <option value="">{t("requirement.waive.choose")}</option>
              {detail.lint_findings.map((finding) => (
                <option key={finding} value={finding}>
                  {finding}
                </option>
              ))}
            </select>
          </label>
          <label className="block text-sm">
            {t("requirement.reason")}
            <input className="ml-2 rounded border p-1" value={waiveReason} data-testid="waive-reason" onChange={(event) => setWaiveReason(event.target.value)} />
          </label>
          <button type="submit" className="rounded border px-3 py-1" disabled={waive.isPending || waiveFinding === "" || waiveReason === ""}>
            {t("requirement.waive.submit")}
          </button>
          <WriteError error={waive.error} />
        </form>

        <form
          aria-label={t("requirement.duplicate.title")}
          className="space-y-2 rounded border p-3"
          onSubmit={(event) => {
            event.preventDefault();
            duplicate.mutate({ of: duplicateOf, reason: duplicateReason }, onWritten);
          }}
        >
          <label className="block text-sm">
            {t("requirement.duplicate.of")}
            <input className="ml-2 rounded border p-1" value={duplicateOf} data-testid="duplicate-of" onChange={(event) => setDuplicateOf(event.target.value)} />
          </label>
          <label className="block text-sm">
            {t("requirement.reason")}
            <input className="ml-2 rounded border p-1" value={duplicateReason} data-testid="duplicate-reason" onChange={(event) => setDuplicateReason(event.target.value)} />
          </label>
          <button type="submit" className="rounded border px-3 py-1" disabled={duplicate.isPending || duplicateOf === "" || duplicateReason === ""}>
            {t("requirement.duplicate.submit")}
          </button>
          <WriteError error={duplicate.error} />
        </form>

        <SplitAction requirementId={detail.id} />
      </section>
    </div>
  );
}
