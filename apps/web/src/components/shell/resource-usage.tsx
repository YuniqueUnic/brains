import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, IS_DEMO, ApiError } from "../../api/client";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import {
  percentageValue,
  resourceDraft,
  validResourceNumbers,
  serviceLabel,
  displayMetric,
  type ResourceReport,
  type ResourceInput,
} from "../../lib/resource-policy";
import { ResourceRuleRow, resourceSelectClass } from "./resource-rule-row";
import { ResourceBillingPeriods } from "./resource-billing-periods";

type State =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "ready"; report: ResourceReport };

export function ResourceUsage() {
  const { t } = useTranslation("resources");
  const [state, setState] = useState<State>({ kind: "loading" });
  const [draft, setDraft] = useState<ResourceInput>();
  const [dirty, setDirty] = useState(false);
  const [pending, setPending] = useState<"idle" | "saving" | "refreshing">(
    "idle",
  );
  const [conflict, setConflict] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [validation, setValidation] = useState<"billing" | "numbers">();
  const [filter, setFilter] = useState("");
  const [service, setService] = useState("");
  const [showAll, setShowAll] = useState(false);
  const [editing, setEditing] = useState<string>();
  const busy = pending !== "idle";

  const accept = (report: ResourceReport) => {
    setState({ kind: "ready", report });
    setDraft(resourceDraft(report));
    setDirty(false);
    setConflict(false);
  };
  useEffect(() => {
    let active = true;
    void api
      .resourceStatus()
      .then((report) => {
        if (active) accept(report);
      })
      .catch((reason: unknown) => {
        if (active)
          setState({
            kind: "failed",
            message: reason instanceof Error ? reason.message : String(reason),
          });
      });
    return () => {
      active = false;
    };
  }, []);
  const refresh = async () => {
    setError("");
    setSaved(false);
    setPending("refreshing");
    setValidation(undefined);
    try {
      accept(await api.resourceStatus());
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setPending("idle");
    }
  };
  const change = (next: ResourceInput) => {
    setDraft(next);
    setDirty(true);
    setSaved(false);
    setError("");
    setValidation(undefined);
  };
  const updateRule = (
    id: string,
    patch: Partial<ResourceInput["rules"][number]>,
  ) => {
    if (draft)
      change({
        ...draft,
        rules: draft.rules.map((rule) =>
          rule.id === id ? { ...rule, ...patch } : rule,
        ),
      });
  };
  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!draft) return;
    if (!validResourceNumbers(draft)) {
      setValidation("numbers");
      return;
    }
    const cycle = draft.billing_cycles.find((item) => item.id === "r2");
    const now = Date.now() / 1000;
    if (
      draft.enforced &&
      (cycle?.start == null ||
        cycle.end == null ||
        cycle.start > now ||
        cycle.end <= now)
    ) {
      setValidation("billing");
      return;
    }
    setPending("saving");
    setError("");
    setSaved(false);
    try {
      accept(await api.updateResourceSettings(draft));
      setSaved(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setConflict(reason instanceof ApiError && reason.status === 412);
    } finally {
      setPending("idle");
    }
  };
  if (state.kind === "loading")
    return (
      <p role="status" className="py-6 text-body-sm text-muted-foreground">
        {t("loading")}
      </p>
    );
  if (state.kind === "failed")
    return (
      <div role="alert" className="space-y-3 text-body-sm">
        <p>{t("loadFailed")}</p>
        <p className="break-words text-meta-lg text-muted-foreground">
          {error || state.message}
        </p>
        <Button
          type="button"
          onClick={() => void refresh()}
          disabled={busy}
          variant="outline"
        >
          {t("retry")}
        </Button>
      </div>
    );
  if (!draft) return null;
  const report = state.report;
  const services = [
    ...new Set(report.rules.map((rule) => rule.resource.service)),
  ];
  const rows = report.rules.filter(
    (rule) =>
      (!service || service === rule.resource.service) &&
      (showAll ||
        rule.limit.kind === "allowance" ||
        report.usage.some((usage) => usage.rule === rule.id)) &&
      `${rule.id} ${displayMetric(rule.measure.metric, t)} ${serviceLabel(rule.resource.service, t)}`
        .toLowerCase()
        .includes(filter.toLowerCase()),
  );
  return (
    <form
      onSubmit={(event) => void save(event)}
      className="flex min-h-0 flex-1 flex-col"
      aria-label={t("formLabel")}
    >
      <div
        data-testid="resource-settings-body"
        className="min-h-0 flex-1 space-y-5 overflow-y-auto pb-3 pr-1"
      >
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h3 className="text-body font-medium">{t("title")}</h3>
            <p className="mt-1 max-w-prose text-body-sm text-muted-foreground">
              {t("intro")}
            </p>
          </div>
          <Button
            type="button"
            variant="outline"
            disabled={dirty || busy}
            onClick={() => void refresh()}
          >
            {t(pending === "refreshing" ? "refreshing" : "refresh")}
          </Button>
        </div>
        {IS_DEMO && (
          <p className="rounded-md bg-muted px-3 py-2 text-body-sm">
            {t("demoNotice")}
          </p>
        )}
        <fieldset disabled={busy} className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3 border-y border-border py-3">
            <label className="flex items-center gap-2 text-body-sm">
              <input
                type="checkbox"
                checked={draft.enforced}
                onChange={(event) =>
                  change({ ...draft, enforced: event.target.checked })
                }
                className="size-4 accent-primary"
              />
              {t("enforce")}
            </label>
            <label className="flex items-center gap-2 text-body-sm">
              {t("defaultBudget")}
              <Input
                aria-label={t("percentLabel")}
                type="number"
                required
                min="0"
                step="any"
                className="w-20 text-right tabular-nums"
                value={percentageValue(draft.policy.fraction ?? 0.8)}
                onChange={(event) =>
                  change({
                    ...draft,
                    policy: {
                      ...draft.policy,
                      fraction: event.target.valueAsNumber / 100,
                    },
                  })
                }
              />
              %
            </label>
          </div>
          {!draft.enforced && (
            <p className="text-body-sm text-muted-foreground">
              {t("disabled")}
            </p>
          )}
          <details>
            <summary className="cursor-pointer text-body-sm">
              {t("defaults")}
            </summary>
            <div className="mt-2 grid gap-3 sm:grid-cols-2">
              {(["when_exhausted", "when_charge_unknown"] as const).map(
                (key) => (
                  <label key={key} className="space-y-1 text-body-sm">
                    <span>
                      {t(
                        key === "when_exhausted"
                          ? "exhausted"
                          : "unknownCharge",
                      )}
                    </span>
                    <select
                      className={`${resourceSelectClass} w-full`}
                      value={draft.policy[key] ?? "block"}
                      onChange={(event) =>
                        change({
                          ...draft,
                          policy: {
                            ...draft.policy,
                            [key]: event.target.value,
                          },
                        })
                      }
                    >
                      <option value="block">
                        {t(
                          key === "when_exhausted"
                            ? "actions.blockNew"
                            : "actions.blockUnknown",
                        )}
                      </option>
                      <option value="observe">
                        {t(
                          key === "when_exhausted"
                            ? "actions.observeOnly"
                            : "actions.allowUnknown",
                        )}
                      </option>
                    </select>
                  </label>
                ),
              )}
            </div>
          </details>
          <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto]">
            <Input
              aria-label={t("find")}
              placeholder={t("findPlaceholder")}
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
            />
            <select
              aria-label={t("filterService")}
              className={resourceSelectClass}
              value={service}
              onChange={(event) => setService(event.target.value)}
            >
              <option value="">{t("allServices")}</option>
              {services.map((id) => (
                <option key={id} value={id}>
                  {serviceLabel(id, t)}
                </option>
              ))}
            </select>
          </div>
          <label className="flex items-center gap-2 text-body-sm text-muted-foreground">
            <input
              type="checkbox"
              checked={showAll}
              onChange={(event) => setShowAll(event.target.checked)}
              className="accent-primary"
            />
            {t("includeReference")}
          </label>
          <div className="divide-y divide-border">
            {services.map((id) => {
              const group = rows.filter((rule) => rule.resource.service === id);
              if (!group.length) return null;
              return (
                <section
                  key={id}
                  className="py-4 first:pt-0"
                  aria-label={serviceLabel(id, t)}
                >
                  <h4 className="mb-2 text-body-sm font-medium">
                    {serviceLabel(id, t)}
                  </h4>
                  <div className="divide-y divide-border/60">
                    {group.map((rule) => (
                      <ResourceRuleRow
                        key={rule.id}
                        rule={rule}
                        draft={draft.rules.find((item) => item.id === rule.id)!}
                        defaults={draft.policy}
                        usage={report.usage.filter(
                          (item) => item.rule === rule.id,
                        )}
                        expanded={editing === rule.id}
                        onToggle={() =>
                          setEditing(editing === rule.id ? undefined : rule.id)
                        }
                        onChange={(patch) => updateRule(rule.id, patch)}
                      />
                    ))}
                  </div>
                </section>
              );
            })}
            {rows.length === 0 && (
              <p className="py-6 text-body-sm text-muted-foreground">
                {t("noMatches")}
              </p>
            )}
          </div>
          <ResourceBillingPeriods
            cycles={draft.billing_cycles}
            onChange={(billing_cycles) => change({ ...draft, billing_cycles })}
          />
        </fieldset>
        <p className="text-meta-lg text-muted-foreground">
          {t("snapshot", { date: report.profile.reviewed_at })}
        </p>
        {validation && (
          <p role="alert" className="text-body-sm text-destructive">
            {t(
              validation === "billing"
                ? "currentPeriodRequired"
                : "invalidNumbers",
            )}
          </p>
        )}
        {error && (
          <div role="alert" className="text-body-sm text-destructive">
            <p>{t(conflict ? "conflict" : "saveFailed")}</p>
            {!conflict && (
              <details className="mt-1 text-meta-lg">
                <summary className="cursor-pointer">{t("details")}</summary>
                <p className="break-words">{error}</p>
              </details>
            )}
          </div>
        )}
        {conflict && (
          <Button
            type="button"
            variant="outline"
            disabled={busy}
            onClick={() => void refresh()}
          >
            {t("loadLatest")}
          </Button>
        )}
        {saved && (
          <p role="status" className="text-body-sm">
            {t(IS_DEMO ? "savedDemo" : "savedLive")}
          </p>
        )}
      </div>
      <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border bg-background pt-3">
        <Button
          type="button"
          disabled={!dirty || busy}
          onClick={() => {
            setDraft(resourceDraft(report));
            setDirty(false);
            setError("");
            setValidation(undefined);
          }}
        >
          {t("discard")}
        </Button>
        <Button variant="default" type="submit" disabled={!dirty || busy}>
          {t(pending === "saving" ? "saving" : "save")}
        </Button>
      </div>
    </form>
  );
}
