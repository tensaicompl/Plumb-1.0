// The shared semantic change preview of the old requirement detail, kept only because the old
// Glossary page (features/glossary, replaced by the Model lane) still imports it from here.
// Delete this file (and components/DiffViewer.tsx if nothing else uses it) with that page.
import type { components } from "../../api/schema";
import { WriteFailedError } from "../../api/useVersionedMutation";
import { DiffViewer, type ModelDiff } from "../../components/DiffViewer";
import { t } from "../../i18n";

export type SemanticChangePreview = components["schemas"]["SemanticChangePreviewResponse"];
export type SemanticElementDiff = components["schemas"]["SemanticElementDiff"];

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

/** A submission refused because the change it rebuilds is no longer the reviewed preview (H0.11). */
export function isPreviewMismatch(error: unknown): boolean {
  return error instanceof WriteFailedError && error.status === 409 && errorCode(error) === "E_PREVIEW_MISMATCH";
}

/** The identity of a reviewed preview that a direct write submits (H0.11, architecture §33.12). */
export function previewConfirmation(preview: SemanticChangePreview) {
  return { proposal_id: preview.proposal_id, built_at: preview.built_at };
}

/**
 * A server-authoritative semantic change preview (Hotfix 073/074): the exact proposal, its server
 * intake and the exact server semantic element diff. Nothing is persisted (H0.11) or committed until
 * the explicit confirmation; the candidate hash describes the domain proposal only, never the
 * committed model. Discarding it is local: there is nothing on the server to discard.
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
