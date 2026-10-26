import { useQueryClient } from "@tanstack/react-query";
import { Upload } from "lucide-react";
import { useState, type DragEvent } from "react";

import { revisionEtag, type ApiResponse } from "../../api/client";
import type { components } from "../../api/schema";
import { useModelVersion, useVersionedMutation, WriteFailedError } from "../../api/useVersionedMutation";
import { useRunStage } from "../../app/run";
import { cn, toast } from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";

type SourceUploadResponse = components["schemas"]["SourceUploadResponse"];

/** The media type the server imports for each accepted extension (`create_source`). */
const MEDIA_TYPES: Readonly<Record<string, string>> = {
  md: "text/markdown",
  markdown: "text/markdown",
  txt: "text/plain",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
};

/** The file picker filter. */
export const ACCEPT = ".md,.markdown,.txt,.docx";

/** Above this size the import notes that analysis may take several minutes. */
export const LARGE_DOCUMENT_BYTES = 1_000_000;

/** The media type of an accepted file name, or null for an unsupported type. */
export function mediaTypeOf(name: string): string | null {
  const dot = name.lastIndexOf(".");
  return dot === -1 ? null : (MEDIA_TYPES[name.slice(dot + 1).toLowerCase()] ?? null);
}

/** A refused import in plain language, with the server's code for the details. */
export interface ImportFailure {
  readonly message: MessageKey;
  readonly code: string | null;
}

const FAILURE: Readonly<Record<string, MessageKey>> = {
  E_SOURCE_EXISTS: "sources.import.exists",
  E_SOURCE_EMPTY: "sources.import.empty",
  E_UNSUPPORTED_MEDIA_TYPE: "sources.import.unsupported",
  E_PAYLOAD_TOO_LARGE: "sources.import.tooLarge",
};

/** The plain explanation of a failed upload. */
export function importFailureOf(error: unknown): ImportFailure {
  const body = error instanceof WriteFailedError && typeof error.body === "object" && error.body !== null ? (error.body as Record<string, unknown>) : null;
  const code = typeof body?.["code"] === "string" ? body["code"] : null;
  return { message: (code === null ? undefined : FAILURE[code]) ?? "sources.import.failed", code };
}

/** A `multipart/form-data` body with the one `file` part `create_source` accepts. */
export async function multipartBody(file: File, mediaType: string): Promise<{ body: Uint8Array<ArrayBuffer>; contentType: string }> {
  const boundary = "plumb-" + Math.random().toString(36).slice(2) + Date.now().toString(36);
  const encoder = new TextEncoder();
  const name = file.name.replace(/["\r\n]/g, "_");
  const head = encoder.encode(`--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${name}"\r\nContent-Type: ${mediaType}\r\n\r\n`);
  const content = new Uint8Array(await file.arrayBuffer());
  const tail = encoder.encode(`\r\n--${boundary}--\r\n`);
  const body = new Uint8Array(head.length + content.length + tail.length);
  body.set(head, 0);
  body.set(content, head.length);
  body.set(tail, head.length + content.length);
  return { body, contentType: "multipart/form-data; boundary=" + boundary };
}

/**
 * One multipart upload with progress (fetch reports no upload progress), as a client response.
 * The body is assembled as bytes so every environment sends exactly the same request.
 */
async function upload(file: File, mediaType: string, ifMatch: string, onProgress: (fraction: number) => void): Promise<ApiResponse<SourceUploadResponse>> {
  const { body, contentType } = await multipartBody(file, mediaType);
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open("POST", "/api/sources");
    xhr.setRequestHeader("If-Match", ifMatch);
    xhr.setRequestHeader("Content-Type", contentType);
    xhr.upload.addEventListener("progress", (event) => {
      if (event.lengthComputable && event.total > 0) {
        onProgress(event.loaded / event.total);
      }
    });
    xhr.addEventListener("load", () => {
      let body: unknown = undefined;
      try {
        body = JSON.parse(xhr.responseText);
      } catch {
        // A non-JSON body stays undefined; the status decides.
      }
      const response = new Response(null, { status: xhr.status });
      resolve(response.ok ? { data: body as SourceUploadResponse, response } : { error: body, response });
    });
    xhr.addEventListener("error", () => reject(new TypeError("upload failed")));
    xhr.send(body);
  });
}

/** The file being uploaded. */
export interface Uploading {
  readonly name: string;
  /** 0–100. */
  readonly pct: number;
  readonly large: boolean;
}

/**
 * Imports a document (`create_source`) against the model version the user sees, with upload
 * progress. On success it toasts "38 passages imported from requirements.md" and offers to run the
 * Requirements step; a refusal is explained in plain language.
 */
export function useImportSource() {
  const version = useModelVersion();
  const queryClient = useQueryClient();
  const { run } = useRunStage();
  const [uploading, setUploading] = useState<Uploading | null>(null);
  const [failure, setFailure] = useState<ImportFailure | null>(null);
  const mutation = useVersionedMutation(
    (input: { file: File; mediaType: string }, ifMatch: string) =>
      upload(input.file, input.mediaType, ifMatch, (fraction) => setUploading((current) => (current === null ? null : { ...current, pct: Math.round(fraction * 100) }))),
    version.data?.etag,
  );
  const importFile = (file: File) => {
    setFailure(null);
    const mediaType = mediaTypeOf(file.name);
    if (mediaType === null) {
      setFailure({ message: "sources.import.unsupported", code: null });
      return;
    }
    setUploading({ name: file.name, pct: 0, large: file.size > LARGE_DOCUMENT_BYTES });
    mutation.mutate(
      { file, mediaType },
      {
        onSuccess: (data) => {
          setUploading(null);
          void queryClient.invalidateQueries();
          toast({
            message: tf("sources.import.done", { count: data.spans_count, name: file.name }),
            action: { label: t("sources.import.runRequirements"), onAction: () => run("extract", revisionEtag(data.revision)) },
          });
        },
        onError: (error) => {
          setUploading(null);
          if (error.name !== "PreconditionFailedError") {
            setFailure(importFailureOf(error));
          }
        },
      },
    );
  };
  // The upload is a write against the version the user sees: wait until it has loaded.
  return { importFile, uploading, failure, busy: mutation.isPending || version.data === undefined };
}

export interface ImportDropZoneProps {
  /** `hero`: the Overview's first-run zone; `compact`: the Sources column zone. */
  readonly variant: "hero" | "compact";
  readonly className?: string;
}

/** The dashed import zone: drop a file or choose one; shows upload progress and refusals. */
export function ImportDropZone({ variant, className }: ImportDropZoneProps) {
  const { importFile, uploading, failure, busy } = useImportSource();
  const [over, setOver] = useState(false);
  const hero = variant === "hero";
  const title = t(hero ? "sources.import.dropTitle" : "sources.import.title");
  const onDrop = (event: DragEvent<HTMLLabelElement>) => {
    event.preventDefault();
    setOver(false);
    const file = event.dataTransfer.files[0];
    if (file !== undefined && !busy) {
      importFile(file);
    }
  };
  return (
    <div className={cn("flex w-full flex-col gap-3", className)}>
      <label
        onDragOver={(event) => {
          event.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={onDrop}
        data-drop-zone={variant}
        className={cn(
          "flex w-full cursor-pointer flex-col items-center border border-dashed border-line-strong bg-card text-fg-2 transition-colors duration-[120ms] ease-standard hover:border-primary focus-within:border-accent focus-within:ring-[3px] focus-within:ring-accent-tint",
          hero ? "gap-2.5 rounded-card p-10 text-body font-medium hover:bg-surface" : "gap-1.5 rounded-tile p-[18px] text-footnote font-medium",
          over && "border-primary bg-surface",
        )}
      >
        <Upload size={hero ? 24 : 16} strokeWidth={1.6} aria-hidden="true" />
        <span className="font-semibold text-fg">{title}</span>
        <span className={cn("text-fg-3", hero ? "text-caption" : "text-eyebrow")}>{t(hero ? "sources.import.typesLong" : "sources.import.types")}</span>
        <input
          type="file"
          accept={ACCEPT}
          aria-label={title}
          disabled={busy}
          className="sr-only"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (file !== undefined) {
              importFile(file);
            }
          }}
        />
      </label>
      {uploading !== null && (
        <div className="rounded-field bg-surface px-3 py-2.5 text-caption text-fg">
          <div className="flex justify-between gap-3">
            <span className="truncate font-semibold">{uploading.name}</span>
            <span>{tf("sources.import.percent", { pct: uploading.pct })}</span>
          </div>
          <div
            role="progressbar"
            aria-label={tf("sources.import.uploading", { name: uploading.name })}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={uploading.pct}
            className="mt-2 h-[3px] overflow-hidden rounded-pill bg-line"
          >
            <div className="h-full bg-primary transition-[width] duration-200 ease-standard" style={{ width: String(uploading.pct) + "%" }} />
          </div>
          {uploading.large && <p className="m-0 mt-2 text-fg-3-surface">{t("sources.import.large")}</p>}
        </div>
      )}
      {failure !== null && (
        <div role="alert" className="flex flex-wrap items-baseline gap-x-2 rounded-field bg-blocked-tint px-3 py-2.5 text-caption font-medium text-blocked-fg">
          <span>{t(failure.message)}</span>
          {failure.code !== null && <span className="font-mono text-eyebrow">{failure.code}</span>}
        </div>
      )}
    </div>
  );
}
