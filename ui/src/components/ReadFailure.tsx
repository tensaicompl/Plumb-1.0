import { ReadFailedError } from "../api/client";
import { t, type MessageKey } from "../i18n";

/**
 * A failed read (architecture §33.12): the page's catalogue message followed by the backend typed
 * code and message, or the typed transport failure when the server was not reached. A typed error
 * is never hidden behind a generic message.
 */
export function ReadFailure({ error, messageKey }: { error: unknown; messageKey: MessageKey }) {
  return (
    <p role="alert" data-testid="read-failure">
      {t(messageKey)}{" "}
      {error instanceof ReadFailedError ? (
        <>
          <span className="font-mono" data-testid="read-failure-code">
            {error.code ?? String(error.status)}
          </span>{" "}
          {error.serverMessage !== null && <span data-testid="read-failure-message">{error.serverMessage}</span>}
        </>
      ) : (
        <span data-testid="read-failure-code">{t("read.transportFailed")}</span>
      )}
    </p>
  );
}
