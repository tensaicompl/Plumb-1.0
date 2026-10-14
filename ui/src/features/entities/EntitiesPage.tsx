import { useQuery } from "@tanstack/react-query";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { t, type MessageKey } from "../../i18n";

type Entity = components["schemas"]["Entity"];
type CrudMatrix = components["schemas"]["CrudMatrix"];
type CrudCell = components["schemas"]["CrudCell"];

/** The Entities list query root. */
export const entitiesQueryKey = ["entities"] as const;
/** The CRUD matrix query root. */
export const crudQueryKey = ["crud"] as const;

async function fetchEntities(): Promise<Entity[]> {
  const { data, response } = await apiClient.GET("/api/model/entities");
  if (data === undefined) {
    throw new Error("GET /api/model/entities failed with HTTP " + String(response.status));
  }
  return data.items;
}

async function fetchCrud(): Promise<CrudMatrix> {
  const { data, response } = await apiClient.GET("/api/model/crud");
  if (data === undefined) {
    throw new Error("GET /api/model/crud failed with HTTP " + String(response.status));
  }
  return data;
}

function Optional({ value }: { value: string | null | undefined }) {
  return value === undefined || value === null ? <span className="text-slate-500">{t("entities.none")}</span> : <span className="font-mono">{value}</span>;
}

/** Read-only attributes of one entity, exactly as the server projects them. */
function Attributes({ entity }: { entity: Entity }) {
  if (entity.attributes.length === 0) {
    return <p className="text-sm">{t("entities.noAttributes")}</p>;
  }
  return (
    <table className="text-sm" data-testid="entity-attributes">
      <thead>
        <tr>
          <th className="pr-2 text-left">{t("entities.attribute.name")}</th>
          <th className="pr-2 text-left">{t("entities.attribute.type")}</th>
          <th className="pr-2 text-left">{t("entities.attribute.nullable")}</th>
          <th className="pr-2 text-left">{t("entities.attribute.class")}</th>
          <th className="pr-2 text-left">{t("entities.attribute.unit")}</th>
        </tr>
      </thead>
      <tbody>
        {entity.attributes.map((attribute) => (
          <tr key={attribute.name} data-attribute={attribute.name}>
            <td className="pr-2 font-mono">{attribute.name}</td>
            <td className="pr-2 font-mono">{attribute.type}</td>
            <td className="pr-2">{t(attribute.nullable ? "entities.yes" : "entities.no")}</td>
            <td className="pr-2">
              <Optional value={attribute.class} />
            </td>
            <td className="pr-2">
              <Optional value={attribute.unit} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** The ER relationships of one entity: only `Entity.relationships[]`, never inferred. */
function Relationships({ entity }: { entity: Entity }) {
  if (entity.relationships.length === 0) {
    return <p className="text-sm">{t("entities.noRelationships")}</p>;
  }
  return (
    <table className="text-sm" data-testid="entity-relationships">
      <thead>
        <tr>
          <th className="pr-2 text-left">{t("entities.relationship.id")}</th>
          <th className="pr-2 text-left">{t("entities.relationship.kind")}</th>
          <th className="pr-2 text-left">{t("entities.relationship.target")}</th>
          <th className="pr-2 text-left">{t("entities.relationship.cardinality")}</th>
        </tr>
      </thead>
      <tbody>
        {entity.relationships.map((relationship) => (
          <tr key={relationship.id} data-relationship={relationship.id}>
            <td className="pr-2 font-mono">{relationship.id}</td>
            <td className="pr-2 font-mono">{relationship.kind}</td>
            <td className="pr-2 font-mono">{relationship.target}</td>
            <td className="pr-2">
              <Optional value={relationship.cardinality} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function EntityItem({ entity }: { entity: Entity }) {
  return (
    <li className="space-y-2 rounded border p-2" data-entity-id={entity.id}>
      <h3 className="font-semibold">
        {entity.name} <span className="font-mono text-sm">{entity.id}</span>
      </h3>
      {entity.tags.length > 0 && (
        <p className="text-sm" data-testid="entity-tags">
          {t("entities.tags")} <span className="font-mono">{entity.tags.join(", ")}</span>
        </p>
      )}
      <h4 className="text-sm font-semibold">{t("entities.attributes")}</h4>
      <Attributes entity={entity} />
      <h4 className="text-sm font-semibold">{t("entities.relationships")}</h4>
      <Relationships entity={entity} />
    </li>
  );
}

/** A three-state CRUD flag: `null` is "not distinguishable", never "no" (Hotfix 065). */
function Flag({ letter, value }: { letter: "c" | "r" | "u" | "d"; value: boolean | null }) {
  const label: MessageKey = value === null ? "entities.crud.notDistinguishable" : value ? "entities.yes" : "entities.no";
  return (
    <span className="mr-2 whitespace-nowrap" data-crud={letter} data-value={String(value)}>
      <span className="font-mono uppercase">{letter}</span> {t(label)}
    </span>
  );
}

/** The read-only CRUD matrix exactly as GET /api/model/crud returns it. */
function Crud({ matrix }: { matrix: CrudMatrix }) {
  const cellOf = (op: string, ent: string): CrudCell | undefined => matrix.cells.find((cell) => cell.op === op && cell.ent === ent);
  if (matrix.operations.length === 0 || matrix.entities.length === 0) {
    return <p className="text-sm">{t("entities.crud.empty")}</p>;
  }
  return (
    <table className="text-sm" data-testid="crud-matrix">
      <thead>
        <tr>
          <th className="pr-2 text-left">{t("entities.crud.operation")}</th>
          {matrix.entities.map((ent) => (
            <th key={ent} className="pr-2 text-left font-mono">
              {ent}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {matrix.operations.map((op) => (
          <tr key={op}>
            <th className="pr-2 text-left font-mono">{op}</th>
            {matrix.entities.map((ent) => {
              const cell = cellOf(op, ent);
              return (
                <td key={ent} className="pr-2" data-crud-cell={op + "|" + ent}>
                  {cell === undefined ? (
                    <span className="text-slate-500">{t("entities.none")}</span>
                  ) : (
                    <>
                      <Flag letter="c" value={cell.c} />
                      <Flag letter="r" value={cell.r} />
                      <Flag letter="u" value={cell.u} />
                      <Flag letter="d" value={cell.d} />
                    </>
                  )}
                </td>
              );
            })}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** U2.6 Entities (Hotfix 080): read-only ER view plus the server CRUD matrix; no editors. */
export function EntitiesPage() {
  const entities = useQuery({ queryKey: entitiesQueryKey, queryFn: fetchEntities });
  const crud = useQuery({ queryKey: crudQueryKey, queryFn: fetchCrud });

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("entities.title")}</h1>
      <section aria-label={t("entities.list")} className="space-y-2">
        <h2 className="font-semibold">{t("entities.list")}</h2>
        {entities.isError && <p role="alert">{t("entities.loadFailed")}</p>}
        {entities.data === undefined && !entities.isError && <p>{t("entities.loading")}</p>}
        {entities.data !== undefined && entities.data.length === 0 && <p>{t("entities.empty")}</p>}
        {entities.data !== undefined && entities.data.length > 0 && (
          <ul className="space-y-2">
            {entities.data.map((entity) => (
              <EntityItem key={entity.id} entity={entity} />
            ))}
          </ul>
        )}
      </section>
      <section aria-label={t("entities.crud.title")} className="space-y-2">
        <h2 className="font-semibold">{t("entities.crud.title")}</h2>
        {crud.isError && <p role="alert">{t("entities.loadFailed")}</p>}
        {crud.data === undefined && !crud.isError && <p>{t("entities.loading")}</p>}
        {crud.data !== undefined && <Crud matrix={crud.data} />}
      </section>
    </div>
  );
}
