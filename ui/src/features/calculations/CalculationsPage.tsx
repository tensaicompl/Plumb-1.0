import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { t } from "../../i18n";
import { ReadFailure } from "../../components/ReadFailure";

type Calculation = components["schemas"]["Calculation"];
type CalculationExample = components["schemas"]["CalculationExample"];
type CalculationValue = components["schemas"]["CalculationValue"];

/** Every calculation query shares this root. */
export const calculationsQueryKey = ["calculations"] as const;

/** A worked example the server could not produce, with its error code and message when given. */
class ExampleUnavailable extends Error {
  constructor(
    readonly status: number,
    readonly code: string | null,
    readonly detail: string | null,
  ) {
    super("GET /api/model/calculations/{id}/example failed with HTTP " + String(status));
  }
}

async function fetchCalculations(): Promise<Calculation[]> {
  return (await readData(apiClient.GET("/api/model/calculations"))).items;
}

function field(body: unknown, name: string): string | null {
  if (typeof body === "object" && body !== null) {
    const value = (body as Record<string, unknown>)[name];
    return typeof value === "string" ? value : null;
  }
  return null;
}

/** The read-only worked example: the analyst's inputs (JSON text, when given) are sent as-is and
 * nothing is evaluated in the browser. */
async function fetchExample(id: string, inputs: string | null): Promise<CalculationExample> {
  const query = inputs === null ? {} : { inputs };
  const { data, error, response } = await apiClient.GET("/api/model/calculations/{id}/example", {
    params: { path: { id }, query },
  });
  if (data === undefined) {
    const body: unknown = error;
    throw new ExampleUnavailable(response.status, field(body, "code"), field(body, "message"));
  }
  return data;
}

/** An exact server value: decimal, duration and quantity stay the server's decimal strings. */
function Value({ value }: { value: CalculationValue }) {
  return (
    <span data-value-type={value.type}>
      <span className="mr-1 text-xs uppercase text-slate-500">{value.type}</span>
      {value.type === "enum" ? (
        <span className="font-mono" data-testid="example-value">
          {value.enum_ref + "." + value.member}
        </span>
      ) : (
        <span className="font-mono" data-testid="example-value">
          {String(value.value)}
        </span>
      )}
      {(value.type === "duration" || value.type === "quantity") && <span className="ml-1 font-mono">{value.unit}</span>}
    </span>
  );
}

function WorkedExample({ id, inputs }: { id: string; inputs: string | null }) {
  const example = useQuery({
    queryKey: [...calculationsQueryKey, "example", id, inputs],
    queryFn: () => fetchExample(id, inputs),
    retry: false,
  });
  if (example.isError) {
    const error = example.error;
    return (
      <div role="alert" className="text-sm" data-testid="example-unavailable">
        <p className="font-semibold">{t("calculations.unavailable")}</p>
        {error instanceof ExampleUnavailable && (
          <p>
            <span className="font-mono">{error.code ?? String(error.status)}</span>
            {error.detail !== null && <span className="ml-2">{error.detail}</span>}
          </p>
        )}
      </div>
    );
  }
  if (example.data === undefined) {
    return <p className="text-sm">{t("calculations.loading")}</p>;
  }
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-2 text-sm" data-testid="worked-example">
      <dt>{t("calculations.expr")}</dt>
      <dd className="font-mono">{example.data.expr}</dd>
      <dt>{t("calculations.rendered")}</dt>
      <dd className="font-mono" data-testid="rendered-example">
        {example.data.rendered_example}
      </dd>
      <dt>{t("calculations.value")}</dt>
      <dd>
        <Value value={example.data.value} />
      </dd>
    </dl>
  );
}

/** U2.6 Calculations (Hotfix 080): read-only expressions and the server worked example. */
export function CalculationsPage() {
  const calculations = useQuery({ queryKey: [...calculationsQueryKey, "list"], queryFn: fetchCalculations });
  const [selected, setSelected] = useState("");
  const [draft, setDraft] = useState("");
  const [inputs, setInputs] = useState<string | null>(null);

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("calculations.title")}</h1>
      {calculations.isError && <ReadFailure error={calculations.error} messageKey="calculations.loadFailed" />}
      {calculations.data === undefined && !calculations.isError && <p>{t("calculations.loading")}</p>}
      {calculations.data !== undefined && calculations.data.length === 0 && <p>{t("calculations.empty")}</p>}
      {calculations.data !== undefined && calculations.data.length > 0 && (
        <ul className="space-y-2">
          {calculations.data.map((calculation) => (
            <li key={calculation.id} className="space-y-1 rounded border p-2" data-calculation-id={calculation.id}>
              <h2 className="font-semibold">
                {calculation.name} <span className="font-mono text-sm">{calculation.id}</span>
              </h2>
              <p className="text-sm">
                {t("calculations.expr")} <code data-testid="calculation-expr">{calculation.expr}</code>
              </p>
              <button
                type="button"
                className="rounded border px-3 py-1 text-sm"
                data-testid="show-example"
                onClick={() => {
                  setSelected(calculation.id);
                  setInputs(null);
                }}
              >
                {t("calculations.showExample")}
              </button>
              {selected === calculation.id && (
                <div className="space-y-1">
                  <label className="block text-sm">
                    {t("calculations.inputs")}
                    <textarea
                      className="ml-2 w-full rounded border font-mono text-xs"
                      data-testid="example-inputs"
                      placeholder={t("calculations.inputsExample")}
                      value={draft}
                      onChange={(event) => setDraft(event.target.value)}
                    />
                  </label>
                  <button
                    type="button"
                    className="rounded border px-3 py-1 text-sm"
                    data-testid="compute-example"
                    disabled={draft.trim() === ""}
                    onClick={() => setInputs(draft.trim())}
                  >
                    {t("calculations.compute")}
                  </button>
                  <WorkedExample id={calculation.id} inputs={inputs} />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
