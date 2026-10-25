import { useQuery } from "@tanstack/react-query";

import { apiClient, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { useModelVersion } from "../../api/useVersionedMutation";
import { PILOT_GATES } from "../../app/nav";
import { ReadFailure } from "../../components/ReadFailure";
import { t, type MessageKey } from "../../i18n";

type GateStatus = components["schemas"]["GateStatus"];

/** The query key of the current gate statuses (GET /api/gates). */
export const gatesQueryKey = ["gates"] as const;

async function fetchGates(): Promise<GateStatus[]> {
  return (await readData(apiClient.GET("/api/gates")));
}

/** A returned gate passes or fails; a gate absent from the response is unevaluated. */
function gateState(status: GateStatus | undefined): MessageKey {
  if (status === undefined || status.state === "not_evaluated") {
    return "gate.unevaluated";
  }
  if (status.state === "stale") {
    return "gate.stale";
  }
  return status.passed ? "gate.pass" : "gate.fail";
}

/** The U1.1 home: the current model revision and the I0-F4 gate states. */
export function HomePage() {
  const version = useModelVersion();
  const gates = useQuery({ queryKey: gatesQueryKey, queryFn: fetchGates });

  const title = <h1 className="text-xl font-semibold">{t("home.title")}</h1>;
  if (version.isError || gates.isError) {
    return (
      <div className="space-y-6">
        {title}
        <ReadFailure error={version.error ?? gates.error} messageKey="home.loadFailed" />
      </div>
    );
  }
  if (version.data === undefined || gates.data === undefined) {
    return (
      <div className="space-y-6">
        {title}
        <p>{t("home.loading")}</p>
      </div>
    );
  }
  const byGate = new Map(gates.data.map((status) => [status.gate, status]));

  return (
    <div className="space-y-6">
      {title}
      <section aria-labelledby="home-model">
        <h2 id="home-model" className="font-semibold">
          {t("home.model")}
        </h2>
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
          <dt>{t("home.revision")}</dt>
          <dd className="font-mono" data-testid="home-revision">
            {version.data.revision}
          </dd>
          <dt>{t("home.semanticHash")}</dt>
          <dd className="break-all font-mono" data-testid="home-semantic-hash">
            {version.data.semantic_hash}
          </dd>
          <dt>{t("home.evidenceHash")}</dt>
          <dd className="break-all font-mono">{version.data.evidence_hash}</dd>
        </dl>
      </section>
      <section aria-labelledby="home-gates">
        <h2 id="home-gates" className="font-semibold">
          {t("home.gates")}
        </h2>
        <table className="text-sm">
            <caption className="sr-only">{t("home.gates.caption")}</caption>
          <tbody>
            {PILOT_GATES.map((gate) => {
              const state = gateState(byGate.get(gate));
              return (
                <tr key={gate} data-gate={gate} data-gate-state={state}>
                  <th scope="row" className="pr-4 text-left font-mono">
                    {gate}
                  </th>
                  <td>{t(state)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>
    </div>
  );
}
