import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { create } from "zustand";

import { apiClient, readData } from "../api/client";
import type { components } from "../api/schema";
import { useModelVersion, useVersionedMutation, writeErrorMessage } from "../api/useVersionedMutation";
import { toast } from "../components/ui";
import { t, tf } from "../i18n";
import type { StageName } from "./pipeline";
import { stageLabel } from "./pipeline";

type JobStatus = components["schemas"]["JobStatus"];

/** The stage job this browser started, as `get_job` last reported it. */
export interface RunJob {
  readonly id: string;
  readonly stage: StageName;
  readonly status: JobStatus;
  /** 0–1 (status-derived on the server). */
  readonly progress: number;
  readonly error: string | null;
}

interface RunState {
  readonly job: RunJob | null;
  start(id: string, stage: StageName): void;
  update(job: Partial<RunJob>): void;
  clear(): void;
}

/** View state of the tracked stage job (server data stays in TanStack Query). */
export const useRunStore = create<RunState>()((set, get) => ({
  job: null,
  start: (id, stage) => set({ job: { id, stage, status: "queued", progress: 0, error: null } }),
  update: (patch) => {
    const job = get().job;
    if (job === null) {
      return;
    }
    const next = { ...job, ...patch };
    // Unchanged reports keep the same object, so subscribers do not re-render.
    if (next.status !== job.status || next.progress !== job.progress || next.error !== job.error) {
      set({ job: next });
    }
  },
  clear: () => set({ job: null }),
}));

/** The poll interval of `get_job` while a job is in flight. */
export const JOB_POLL_MS = 1000;

const TERMINAL: readonly JobStatus[] = ["ready", "committed", "failed", "stale"];

/** Navigation the shell provides to the job toasts. */
export interface RunNavigation {
  readonly openInbox: () => void;
}

/**
 * Polls `get_job` for the tracked job until it is terminal, mirrors status and progress into the
 * run store, and announces the outcome with a toast ("<Step> step finished · Review"). Mount once.
 */
export function useJobPolling(navigation: RunNavigation): void {
  const job = useRunStore((state) => state.job);
  const update = useRunStore((state) => state.update);
  const queryClient = useQueryClient();
  const active = job !== null && !TERMINAL.includes(job.status);
  const polled = useQuery({
    queryKey: ["job", job?.id],
    queryFn: () => readData(apiClient.GET("/api/jobs/{id}", { params: { path: { id: job?.id ?? "" } } })),
    enabled: active,
    refetchInterval: active ? JOB_POLL_MS : false,
  });
  const data = polled.data;
  const { openInbox } = navigation;
  useEffect(() => {
    if (data === undefined || job === null || !active) {
      return;
    }
    update({ status: data.status, progress: data.progress, error: data.error ?? null });
    if (!TERMINAL.includes(data.status)) {
      return;
    }
    const step = stageLabel(job.stage);
    void queryClient.invalidateQueries();
    if (data.status === "failed") {
      toast({ tone: "error", message: tf("shell.run.failed", { step }) });
    } else if (data.status === "stale") {
      toast({ tone: "info", message: tf("shell.run.staleResult", { step }) });
    } else {
      toast({ message: tf("shell.run.finished", { step }), action: { label: t("shell.run.review"), onAction: openInbox } });
    }
  }, [data, job, active, update, queryClient, openInbox]);
}

/**
 * Retries a failed stage job (`POST /api/jobs/{id}/retry`) against the model version the user
 * sees, then tracks the new job. A 412 opens the conflict sheet; other failures are toasted.
 */
export function useRetryJob() {
  const version = useModelVersion();
  const start = useRunStore((state) => state.start);
  const mutation = useVersionedMutation(
    (target: { jobId: string; stage: StageName }, ifMatch: string) =>
      apiClient.POST("/api/jobs/{id}/retry", { params: { path: { id: target.jobId }, header: { "If-Match": ifMatch } }, body: {} }),
    version.data?.etag,
  );
  return {
    retry: (jobId: string, stage: StageName) =>
      mutation.mutate(
        { jobId, stage },
        {
          onSuccess: (data) => start(data.job_id, stage),
          onError: (error) => {
            if (error.name !== "PreconditionFailedError") {
              toast({ tone: "error", message: tf("shell.run.rejected", { message: writeErrorMessage(error) ?? error.message }) });
            }
          },
        },
      ),
    retrying: mutation.isPending,
  };
}

/**
 * Starts a stage (`run_stage`) against the model version the user sees (its ETag is If-Match), or
 * against `reviewed`, the entity-tag of a revision a write just returned (the import toast runs the
 * Requirements step on the revision its import produced). The started job is tracked even when the
 * component that started it has unmounted. A 412 opens the conflict sheet; other failures are
 * toasted.
 */
export function useRunStage() {
  const version = useModelVersion();
  const start = useRunStore((state) => state.start);
  const mutation = useVersionedMutation(
    ({ stage }: { stage: StageName; reviewed?: string }, ifMatch: string) =>
      apiClient.POST("/api/stages/{stage}/run", { params: { path: { stage }, header: { "If-Match": ifMatch } }, body: {} }),
    ({ reviewed }) => reviewed ?? version.data?.etag,
  );
  return {
    run: (stage: StageName, reviewed?: string) =>
      void mutation.mutateAsync(reviewed === undefined ? { stage } : { stage, reviewed }).then(
        (data) => start(data.job_id, stage),
        (error: Error) => {
          if (error.name !== "PreconditionFailedError") {
            toast({ tone: "error", message: tf("shell.run.rejected", { message: writeErrorMessage(error) ?? error.message }) });
          }
        },
      ),
    starting: mutation.isPending,
  };
}
