import { useTranslation } from "react-i18next";
import type { brains } from "../../../../../sdk/typescript/brains";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import {
  percentageValue,
  displayMetric,
  displayScale,
  effectiveBudget,
  periodLabel,
  type ResourceRule,
} from "../../lib/resource-policy";

export const resourceSelectClass =
  "h-8 rounded-[7px] border border-input bg-background px-2 text-body-sm focus-visible:outline-2 focus-visible:outline-ring/70";
type RuleUpdate = brains.ResourceRuleUpdate;

export function ResourceRuleRow({
  rule,
  draft,
  defaults,
  usage,
  expanded,
  onToggle,
  onChange,
}: {
  rule: ResourceRule;
  draft: RuleUpdate;
  defaults: brains.ResourcePolicyDefaults;
  usage: brains.ResourceUsage[];
  expanded: boolean;
  onToggle: () => void;
  onChange: (patch: Partial<RuleUpdate>) => void;
}) {
  const { t, i18n } = useTranslation("resources");
  const configured = { ...rule, limit: draft.limit, policy: draft.policy };
  const limit = effectiveBudget(configured, defaults);
  const active = usage.filter(
    (item) =>
      item.reset_at == null || Number(item.reset_at) > Date.now() / 1000,
  );
  const amount =
    active.reduce((sum, item) => sum + item.consumed, 0) /
    displayScale(rule.measure.unit);
  const held =
    usage.reduce((sum, item) => sum + item.reserved, 0) /
    displayScale(rule.measure.unit);
  const numeric = ["allowance", "ceiling", "reference"].includes(
    rule.limit.kind,
  );
  const hasBudget =
    rule.limit.kind === "allowance" || rule.limit.kind === "ceiling";
  const percent =
    limit != null && limit > 0
      ? Math.min(100, ((amount + held) / limit) * 100)
      : limit === 0 && amount + held > 0
        ? 100
        : 0;
  const unit = t(`units.${rule.measure.unit}`, {
    defaultValue: rule.measure.unit,
  });
  const number = (n: number) =>
    n.toLocaleString(i18n.language, { maximumFractionDigits: 3 });
  return (
    <div className="py-3">
      <div className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-3">
        <div className="min-w-0">
          <p className="text-body-sm">
            {displayMetric(rule.measure.metric, t)}
          </p>
          <p className="mt-0.5 break-words text-meta-lg text-muted-foreground">
            {unit} ·{" "}
            {t(`scopes.${rule.scope.kind}`, { defaultValue: rule.scope.kind })}{" "}
            · {periodLabel(rule.window, t)}
          </p>
        </div>
        <div className="flex items-start gap-2">
          <div className="text-right text-body-sm tabular-nums">
            <span>{usage.length ? number(amount) : t("noMeasurement")}</span>
            {limit != null && (
              <span className="block text-meta-lg text-muted-foreground">
                {t("ofLimit", { amount: number(limit), unit })}
              </span>
            )}
          </div>
          <Button
            type="button"
            size="sm"
            onClick={onToggle}
            aria-label={t("editRule", { rule: rule.id })}
            aria-expanded={expanded}
          >
            {t("edit")}
          </Button>
        </div>
      </div>
      {usage.length > 0 && (
        <>
          <meter
            aria-label={t("budgetUsed", { rule: rule.id })}
            min="0"
            max="100"
            value={percent}
            className="mt-2 h-1.5 w-full accent-primary"
          />
          <p className="mt-1 text-meta-lg text-muted-foreground">
            {[
              ...new Set(
                usage.map((item) =>
                  t(`evidence.${item.evidence}`, {
                    defaultValue: item.evidence,
                  }),
                ),
              ),
            ].join(", ")}
            {held > 0
              ? ` · ${t("reserved", { amount: number(held), unit })}`
              : ""}
            {active[0]?.reset_at
              ? ` · ${t("resets", { date: new Date(Number(active[0].reset_at) * 1000).toLocaleString(i18n.language) })}`
              : ""}
          </p>
        </>
      )}
      <p className="mt-1 text-meta-lg text-muted-foreground">
        {t(`enforcement.${rule.enforcement}`, {
          defaultValue: rule.enforcement.replaceAll("-", " "),
        })}
        {!usage.length ? ` · ${t("noUsage")}` : ""}
        {limit === 0 ? ` · ${t("zeroBudget")}` : ""}
        {limit != null && rule.limit.value != null && limit > rule.limit.value
          ? ` · ${t("aboveProvider")}`
          : ""}
      </p>
      {expanded && (
        <div className="mt-3 grid gap-3 rounded-md bg-muted/50 p-3 sm:grid-cols-2">
          {numeric && (
            <label className="space-y-1 text-body-sm">
              <span>
                {t("providerLimit", {
                  kind: t(`limitKinds.${rule.limit.kind}`),
                  unit,
                })}
              </span>
              <Input
                aria-label={`${t("providerLimit", { kind: t(`limitKinds.${rule.limit.kind}`), unit })} ${rule.id}`}
                type="number"
                min="0"
                step="any"
                required
                value={
                  Number.isNaN(draft.limit.value)
                    ? ""
                    : (draft.limit.value ?? "")
                }
                onChange={(event) =>
                  onChange({
                    limit: {
                      ...draft.limit,
                      value: event.target.valueAsNumber,
                    },
                  })
                }
              />
            </label>
          )}
          {hasBudget && (
            <BudgetEditor
              rule={configured}
              onChange={(policy) => onChange({ policy })}
            />
          )}
          {hasBudget &&
            (["when_exhausted", "when_charge_unknown"] as const).map((key) => (
              <label key={key} className="space-y-1 text-body-sm">
                <span>
                  {t(key === "when_exhausted" ? "exhausted" : "unknownCharge")}
                </span>
                <select
                  className={`${resourceSelectClass} w-full`}
                  aria-label={`${t(key === "when_exhausted" ? "exhausted" : "unknownCharge")} ${rule.id}`}
                  value={draft.policy[key] ?? ""}
                  onChange={(event) =>
                    onChange({
                      policy: {
                        ...draft.policy,
                        [key]: event.target.value || undefined,
                      },
                    })
                  }
                >
                  <option value="">
                    {t("inheritAction", {
                      action: t(`actions.${defaults[key] ?? "block"}`),
                    })}
                  </option>
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
            ))}
          {rule.source?.startsWith("https://") && (
            <a
              href={rule.source}
              target="_blank"
              rel="noreferrer"
              className="text-body-sm underline underline-offset-4"
            >
              {t("pricing")}
            </a>
          )}
        </div>
      )}
    </div>
  );
}

function BudgetEditor({
  rule,
  onChange,
}: {
  rule: ResourceRule;
  onChange: (policy: RuleUpdate["policy"]) => void;
}) {
  const { t } = useTranslation("resources");
  const mode =
    rule.policy?.maximum != null
      ? "maximum"
      : rule.policy?.fraction != null
        ? "fraction"
        : "inherit";
  return (
    <div className="space-y-1 text-body-sm">
      <label className="block" htmlFor={`budget-${rule.id}`}>
        {t("applicationBudget")}
      </label>
      <select
        id={`budget-${rule.id}`}
        className={`${resourceSelectClass} w-full`}
        value={mode}
        onChange={(event) =>
          onChange({
            ...rule.policy,
            maximum:
              event.target.value === "maximum"
                ? (rule.limit.value ?? 0)
                : undefined,
            fraction: event.target.value === "fraction" ? 0.8 : undefined,
          })
        }
      >
        <option value="inherit">{t("inherit")}</option>
        <option value="fraction">{t("percentage")}</option>
        <option value="maximum">{t("fixed")}</option>
      </select>
      {mode !== "inherit" && (
        <Input
          aria-label={`${t("applicationBudget")} ${rule.id}`}
          type="number"
          min="0"
          max={
            rule.limit.kind === "ceiling"
              ? mode === "maximum"
                ? rule.limit.value
                : 100
              : undefined
          }
          step="any"
          required
          value={
            mode === "maximum"
              ? Number.isNaN(rule.policy?.maximum)
                ? ""
                : rule.policy?.maximum
              : percentageValue(rule.policy?.fraction ?? 0)
          }
          onChange={(event) =>
            onChange(
              mode === "maximum"
                ? { ...rule.policy, maximum: event.target.valueAsNumber }
                : {
                    ...rule.policy,
                    fraction: event.target.valueAsNumber / 100,
                  },
            )
          }
        />
      )}
    </div>
  );
}
