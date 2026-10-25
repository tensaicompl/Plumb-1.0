import * as Dialog from "@radix-ui/react-dialog";
import { useId, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";

import { t } from "../../i18n";
import { Button } from "./Button";
import { cn, confirmShortcut } from "./cn";

export interface SheetProps {
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  /** Montserrat 600 18 px. */
  readonly title: string;
  /** 13.5 px secondary text under the title. */
  readonly description?: ReactNode;
  /** 480 (reason, confirm), 520 (settings), 760 (matrix). */
  readonly width?: 480 | 520 | 760;
  readonly children?: ReactNode;
  /** The confirm button label; with `onConfirm` it renders the standard footer. */
  readonly confirmLabel?: string;
  /** Runs on Confirm or ⌘↩ (unless disabled or pending). */
  readonly onConfirm?: () => void;
  readonly confirmDisabled?: boolean;
  /** Reject, discard: the confirm button is red. */
  readonly destructive?: boolean;
  readonly pending?: boolean;
  readonly cancelLabel?: string;
  /** Replaces the standard footer. */
  readonly footer?: ReactNode;
  /** Shown at the right of the title (e.g. a close button). */
  readonly headerAction?: ReactNode;
  /** False: Esc and the scrim do not close it (an explicit choice is required). */
  readonly dismissible?: boolean;
  /** Whether a click on the scrim closes it (default: `dismissible`). */
  readonly scrimDismiss?: boolean;
  /** The element focused when the sheet opens (default: the first focusable element). */
  readonly initialFocus?: RefObject<HTMLElement | null>;
  readonly className?: string;
  readonly "data-testid"?: string;
}

const WIDTH = { 480: "w-[480px]", 520: "w-[520px] p-7 gap-5", 760: "w-[760px]" } as const;

/**
 * The small modal sheet for reasons, confirmations and forms: centred on a midnight 60 % scrim,
 * radius 20, padding 24, --shadow-xl. Esc cancels; ⌘↩ (Ctrl↩) confirms; focus is trapped.
 */
export function Sheet({
  open,
  onOpenChange,
  title,
  description,
  width = 480,
  children,
  confirmLabel,
  onConfirm,
  confirmDisabled = false,
  destructive = false,
  pending = false,
  cancelLabel,
  footer,
  headerAction,
  dismissible = true,
  scrimDismiss,
  initialFocus,
  className,
  ...rest
}: SheetProps) {
  const canConfirm = onConfirm !== undefined && !confirmDisabled && !pending;
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
      event.preventDefault();
      if (canConfirm) {
        onConfirm();
      }
    }
  };
  const standardFooter = (
    <div className="mt-1 flex justify-end gap-2">
      <Button variant="outline" size={38} onClick={() => onOpenChange(false)}>
        {cancelLabel ?? t("ui.sheet.cancel")}
      </Button>
      {onConfirm !== undefined && confirmLabel !== undefined && (
        <Button
          variant={destructive ? "destructive" : "primary"}
          size={38}
          className="px-[18px]"
          disabled={confirmDisabled}
          pending={pending}
          onClick={onConfirm}
          shortcut={confirmShortcut()}
        >
          {confirmLabel}
        </Button>
      )}
    </div>
  );
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-[60] grid animate-fade-in place-items-center overflow-y-auto bg-scrim p-4">
          <Dialog.Content
            data-testid={rest["data-testid"]}
            onKeyDown={onKeyDown}
            onOpenAutoFocus={(event) => {
              if (initialFocus?.current) {
                event.preventDefault();
                initialFocus.current.focus();
              }
            }}
            onEscapeKeyDown={(event) => {
              if (!dismissible) {
                event.preventDefault();
              }
            }}
            onPointerDownOutside={(event) => {
              if (!(scrimDismiss ?? dismissible)) {
                event.preventDefault();
              }
            }}
            onInteractOutside={(event) => {
              if (!(scrimDismiss ?? dismissible)) {
                event.preventDefault();
              }
            }}
            className={cn(
              "relative flex max-w-full animate-overlay-in flex-col gap-3.5 rounded-sheet bg-card p-6 text-fg shadow-xl outline-none",
              WIDTH[width],
              className,
            )}
          >
            <div className="flex items-start justify-between gap-4">
              <Dialog.Title className={cn("m-0 font-display font-semibold text-fg", width === 520 ? "text-title leading-none" : "text-title-sm")}>{title}</Dialog.Title>
              {headerAction}
            </div>
            {description !== undefined ? (
              <Dialog.Description asChild>
                <div className="text-callout leading-normal text-fg-2">{description}</div>
              </Dialog.Description>
            ) : (
              <Dialog.Description className="sr-only">{title}</Dialog.Description>
            )}
            {children}
            {footer ?? standardFooter}
          </Dialog.Content>
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

// ------------------------------------------------------------------ ReasonSheet

export interface ReasonSheetProps {
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  readonly title: string;
  readonly description?: ReactNode;
  readonly confirmLabel: string;
  /** Receives the trimmed reason. */
  readonly onConfirm: (reason: string) => void;
  /** Reject, discard: red confirm. Waive is not destructive. */
  readonly destructive?: boolean;
  /** When the backend requires a reason (default true), Confirm stays disabled until one is typed. */
  readonly reasonRequired?: boolean;
  readonly reasonPlaceholder?: string;
  readonly pending?: boolean;
  /** A failed write's explanation, shown above the footer. */
  readonly error?: ReactNode;
  /** A governed choice shown above the reason (e.g. a rule outcome). */
  readonly children?: ReactNode;
  /** Keeps Confirm disabled while the choice in `children` is incomplete. */
  readonly confirmDisabled?: boolean;
}

/** The governed-action sheet: one multiline reason (min 96 px, radius 10) plus Cancel / Confirm. */
export function ReasonSheet({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  onConfirm,
  destructive = false,
  reasonRequired = true,
  reasonPlaceholder,
  pending = false,
  error,
  children,
  confirmDisabled = false,
}: ReasonSheetProps) {
  const [reason, setReason] = useState("");
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setReason("");
    }
  }
  const fieldId = useId();
  const textarea = useRef<HTMLTextAreaElement>(null);
  const trimmed = reason.trim();
  const label = t(reasonRequired ? "ui.sheet.reasonRequired" : "ui.sheet.reason");
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      description={description}
      confirmLabel={confirmLabel}
      onConfirm={() => onConfirm(trimmed)}
      confirmDisabled={confirmDisabled || (reasonRequired && trimmed === "")}
      destructive={destructive}
      pending={pending}
      initialFocus={children === undefined ? textarea : undefined}
    >
      {children}
      <label htmlFor={fieldId} className="sr-only">
        {label}
      </label>
      <textarea
        id={fieldId}
        ref={textarea}
        value={reason}
        required={reasonRequired}
        aria-required={reasonRequired}
        placeholder={reasonPlaceholder ?? label}
        onChange={(event) => setReason(event.target.value)}
        className={cn(
          "min-h-24 w-full resize-y rounded-field border border-line bg-card p-3 text-callout text-fg outline-none placeholder:text-fg-4",
          "focus:border-accent focus:shadow-focus focus-visible:outline-none",
        )}
      />
      {error !== undefined && (
        <p role="alert" className="m-0 text-footnote text-blocked-fg">
          {error}
        </p>
      )}
    </Sheet>
  );
}
