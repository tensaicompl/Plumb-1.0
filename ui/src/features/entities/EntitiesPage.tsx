import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient, readReviewed, ReviewedEtag, useReviewedEtag, type ReviewedItems } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { t, type MessageKey } from "../../i18n";
import { ReadFailure } from "../../components/ReadFailure";
import { RoleGate } from "../../components/RoleGate";

type Entity = components["schemas"]["Entity"];
type CrudMatrix = components["schemas"]["CrudMatrix"];
type CrudCell = components["schemas"]["CrudCell"];
type EntityAttribute = components["schemas"]["EntityAttribute"];
type WriteResult = components["schemas"]["WriteResult"];

/** The pilot primitive value types the server accepts besides value_type Concept names. */
const PRIMITIVE_TYPES = ["String", "Int", "Bool", "Date", "DateTime", "Decimal(2)"] as const;

/** The Entities list query root. */
export const entitiesQueryKey = ["entities"] as const;
/** The CRUD matrix query root. */
export const crudQueryKey = ["crud"] as const;

function fetchEntities(): Promise<ReviewedItems<Entity>> {
  return readReviewed(apiClient.GET("/api/model/entities"));
}

function fetchCrud(): Promise<CrudMatrix> {
  return readReviewed(apiClient.GET("/api/model/crud"));
}

function Optional({ value }: { value: string | null | undefined }) {
  return value === undefined || value === null ? <span className="text-slate-500">{t("entities.none")}</span> : <span className="font-mono">{value}</span>;
}

/** The attribute's value type with an editor for the pilot primitive types (set_attribute_type). */
function AttributeType({ attribute }: { attribute: EntityAttribute }) {
  const queryClient = useQueryClient();
  const [choice, setChoice] = useState("");
  const setType = useVersionedMutation<WriteResult, string>(
    (type, ifMatch) =>
      apiClient.POST("/api/model/attributes/{id}/type", {
        params: { path: { id: attribute.id }, header: { "If-Match": ifMatch } },
        body: { type },
      }),
    useReviewedEtag(),
  );
  const error = setType.error;
  return (
    <span className="space-x-1">
      <span className="font-mono">{attribute.type}</span>
      <RoleGate quiet>
      <select className="rounded border" data-testid="attribute-type-choice" aria-label={t("entities.attribute.setType")} value={choice} onChange={(event) => setChoice(event.target.value)}>
        <option value="">{t("entities.attribute.chooseType")}</option>
        {PRIMITIVE_TYPES.filter((type) => type !== attribute.type).map((type) => (
          <option key={type} value={type}>
            {type}
          </option>
        ))}
      </select>
      <button
        type="button"
        className="rounded border px-2 text-xs"
        data-testid="attribute-type-save"
        disabled={choice === "" || setType.isPending}
        onClick={() =>
          setType.mutate(choice, {
            onSuccess: () => {
              setChoice("");
              void queryClient.invalidateQueries({ queryKey: entitiesQueryKey });
            },
          })
        }
      >
        {t("entities.attribute.setType")}
      </button>
      </RoleGate>
      {error instanceof WriteFailedError && (
        <span role="alert" className="text-red-800">
          {t("write.failed")} {writeErrorMessage(error)}
        </span>
      )}
    </span>
  );
}

/** The attributes of one entity as the server projects them; the value type is editable. */
function Attributes({ entity }: { entity: Entity }) {
  if (entity.attributes.length === 0) {
    return <p className="text-sm">{t("entities.noAttributes")}</p>;
  }
  return (
    <table className="text-sm" data-testid="entity-attributes">
      <caption className="sr-only">{t("entities.attributes.caption")}</caption>
      <thead>
        <tr>
          <th scope="col" className="pr-2 text-left">{t("entities.attribute.name")}</th>
          <th scope="col" className="pr-2 text-left">{t("entities.attribute.type")}</th>
          <th scope="col" className="pr-2 text-left">{t("entities.attribute.nullable")}</th>
          <th scope="col" className="pr-2 text-left">{t("entities.attribute.class")}</th>
          <th scope="col" className="pr-2 text-left">{t("entities.attribute.unit")}</th>
        </tr>
      </thead>
      <tbody>
        {entity.attributes.map((attribute) => (
          <tr key={attribute.name} data-attribute={attribute.name}>
            <td className="pr-2 font-mono">{attribute.name}</td>
            <td className="pr-2">
              <AttributeType attribute={attribute} />
            </td>
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
      <caption className="sr-only">{t("entities.relationships.caption")}</caption>
      <thead>
        <tr>
          <th scope="col" className="pr-2 text-left">{t("entities.relationship.id")}</th>
          <th scope="col" className="pr-2 text-left">{t("entities.relationship.kind")}</th>
          <th scope="col" className="pr-2 text-left">{t("entities.relationship.target")}</th>
          <th scope="col" className="pr-2 text-left">{t("entities.relationship.cardinality")}</th>
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
      <caption className="sr-only">{t("entities.crud.caption")}</caption>
      <thead>
        <tr>
          <th scope="col" className="pr-2 text-left">{t("entities.crud.operation")}</th>
          {matrix.entities.map((ent) => (
            <th key={ent} scope="col" className="pr-2 text-left font-mono">
              {ent}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {matrix.operations.map((op) => (
          <tr key={op}>
            <th scope="row" className="pr-2 text-left font-mono">{op}</th>
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
        {entities.isError && <ReadFailure error={entities.error} messageKey="entities.loadFailed" />}
        {entities.data === undefined && !entities.isError && <p>{t("entities.loading")}</p>}
        {entities.data !== undefined && entities.data.items.length === 0 && <p>{t("entities.empty")}</p>}
        {entities.data !== undefined && entities.data.items.length > 0 && (
          <ReviewedEtag.Provider value={entities.data.etag}>
            <ul className="space-y-2">
              {entities.data.items.map((entity) => (
                <EntityItem key={entity.id} entity={entity} />
              ))}
            </ul>
          </ReviewedEtag.Provider>
        )}
      </section>
      <section aria-label={t("entities.crud.title")} className="space-y-2">
        <h2 className="font-semibold">{t("entities.crud.title")}</h2>
        {crud.isError && <ReadFailure error={crud.error} messageKey="entities.loadFailed" />}
        {crud.data === undefined && !crud.isError && <p>{t("entities.loading")}</p>}
        {crud.data !== undefined && <Crud matrix={crud.data} />}
      </section>
    </div>
  );
}
