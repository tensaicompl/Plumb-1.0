import { X } from "lucide-react";
import type { ReactNode } from "react";

import { useModelVersion } from "../../api/useVersionedMutation";
import { cn, IconButton, SegmentedControl, Sheet } from "../../components/ui";
import { t, tf, useLocaleStore, type Locale, type MessageKey } from "../../i18n";
import { useMe } from "../auth";
import { APPEARANCES, useAppearanceStore, useShellStore, type Appearance } from "../preferences";
import { rolesLabel } from "./Sidebar";
import { useProject, versionLabel } from "./Toolbar";

const APPEARANCE_LABEL: Readonly<Record<Appearance, MessageKey>> = {
  system: "settings.appearance.system",
  light: "settings.appearance.light",
  dark: "settings.appearance.dark",
};

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-fg-3">{label}</dt>
      <dd className="m-0 min-w-0">{children}</dd>
    </>
  );
}

/**
 * Settings (520 px sheet): language and appearance (per browser), project, database and AI
 * status from the server. The AI key is configured on the server, never entered here.
 */
export function SettingsSheet() {
  const open = useShellStore((state) => state.overlay === "settings");
  const closeOverlay = useShellStore((state) => state.closeOverlay);
  const locale = useLocaleStore((state) => state.locale);
  const setLocale = useLocaleStore((state) => state.setLocale);
  const appearance = useAppearanceStore((state) => state.appearance);
  const setAppearance = useAppearanceStore((state) => state.setAppearance);
  const me = useMe();
  const version = useModelVersion();
  const project = useProject();
  const ai = me.data?.inference_available;
  const model = me.data?.inference_model ?? null;
  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          closeOverlay();
        }
      }}
      title={t("settings.title")}
      width={520}
      footer={false}
      headerAction={
        <IconButton size={30} label={t("settings.close")} icon={<X size={16} strokeWidth={1.6} aria-hidden="true" />} onClick={closeOverlay} />
      }
    >
      <dl className="m-0 grid grid-cols-[120px_1fr] items-center gap-x-3 gap-y-4 text-footnote">
        <Row label={t("settings.language")}>
          <SegmentedControl<Locale>
            label={t("settings.language")}
            value={locale}
            onChange={setLocale}
            options={[
              { value: "en", label: t("shell.language.en") },
              { value: "pl", label: t("shell.language.pl") },
            ]}
          />
        </Row>
        <Row label={t("settings.appearance")}>
          <SegmentedControl<Appearance>
            label={t("settings.appearance")}
            value={appearance}
            onChange={setAppearance}
            options={APPEARANCES.map((value) => ({ value, label: t(APPEARANCE_LABEL[value]) }))}
          />
        </Row>
        <Row label={t("settings.project")}>
          <span data-testid="settings-project" className="text-fg">
            {project.name}
            {project.id !== null && (
              <>
                {SEPARATOR}
                <span className="font-mono">{project.id}</span>
              </>
            )}
            {version.data !== undefined && " · " + versionLabel(version.data.revision)}
          </span>
        </Row>
        <Row label={t("settings.database")}>
          <span className="break-all font-mono text-caption text-fg-2">{project.database ?? t("settings.notReported")}</span>
        </Row>
        {me.data !== undefined && (
          <Row label={t("settings.user")}>
            <span className="text-fg">
              {me.data.user}
              {me.data.roles.length > 0 && " · " + rolesLabel(me.data.roles)}
            </span>
          </Row>
        )}
        <Row label={t("settings.ai")}>
          {ai !== undefined && (
            <span className="flex items-center gap-2 text-fg">
              <span aria-hidden="true" className={cn("size-2 rounded-pill", ai ? "bg-pass" : "bg-neutral")} />
              {ai
                ? model === null
                  ? t("settings.ai.available")
                  : tf("settings.ai.availableModel", { model })
                : t("settings.ai.off")}
            </span>
          )}
        </Row>
      </dl>
      <p className="m-0 rounded-field bg-surface px-3.5 py-3 text-caption leading-normal text-fg-3-surface">
        {t("settings.keyInfo.before")} <span className="font-mono text-fg">{API_KEY_VARIABLE}</span> {t("settings.keyInfo.after")}
      </p>
    </Sheet>
  );
}

/** The separator between the facts of one row (punctuation, not copy). */
const SEPARATOR = " · ";

/** The server environment variable that holds the AI key (a technical name, not copy). */
const API_KEY_VARIABLE = "PLUMB_API_KEY";
