import type { TFunction } from "i18next";
import type { brains } from "../../../../sdk/typescript/brains";

export type ResourceReport = brains.ResourceReport;
export type ResourceInput = brains.ResourceSettingsInput;
export type ResourceRule = brains.ResourceRule;

/** Keep binary floating-point noise out of percentage inputs. */
export function percentageValue(fraction: number): number | "" {
  return Number.isFinite(fraction)
    ? Number((fraction * 100).toPrecision(15))
    : "";
}

export function resourceDraft(report: ResourceReport): ResourceInput {
  return {
    expected_revision: report.settings.revision,
    enforced: report.settings.enforced,
    policy: { ...report.settings.policy },
    billing_cycles: report.settings.billing_cycles.map((cycle) => ({
      ...cycle,
    })),
    rules: report.rules.map((rule) => ({
      id: rule.id,
      limit: { ...rule.limit },
      policy: { ...rule.policy },
    })),
  };
}

/** Validate the whole draft, including collapsed or filtered-out rule inputs. */
export function validResourceNumbers(input: ResourceInput): boolean {
  const numbers = [
    input.policy.fraction,
    ...input.rules.flatMap((rule) => [
      rule.limit.value,
      rule.policy.fraction,
      rule.policy.maximum,
    ]),
  ].filter((value) => value != null);
  if (numbers.some((value) => !Number.isFinite(value) || value < 0))
    return false;
  return input.rules.every((rule) => {
    if (
      ["allowance", "ceiling", "reference"].includes(rule.limit.kind) &&
      rule.limit.value == null
    )
      return false;
    if (rule.limit.kind !== "ceiling") return true;
    const ceiling = rule.limit.value!;
    const budget = rule.policy.maximum ?? ceiling * (rule.policy.fraction ?? 1);
    return budget <= ceiling;
  });
}

export function effectiveBudget(
  rule: Pick<ResourceRule, "limit" | "policy" | "measure">,
  defaults: brains.ResourcePolicyDefaults,
): number | undefined {
  if (rule.limit.kind !== "allowance" && rule.limit.kind !== "ceiling")
    return undefined;
  if (rule.limit.value == null) return undefined;
  const value =
    rule.policy?.maximum ??
    rule.limit.value *
      (rule.policy?.fraction ??
        (rule.limit.kind === "allowance" ? (defaults.fraction ?? 0.8) : 1));
  if (!Number.isFinite(value)) return undefined;
  const continuous = [
    "GB",
    "MB",
    "MiB",
    "GB-month",
    "byte-month",
    "day",
    "hour",
    "second",
    "ms",
    "microsecond",
  ];
  return continuous.includes(rule.measure.unit) ? value : Math.floor(value);
}

export function serviceLabel(id: string, t: TFunction<"resources">): string {
  return t(`services.${id}`, { defaultValue: id });
}

export function periodLabel(
  window: brains.ResourceWindow,
  t: TFunction<"resources">,
): string {
  if (window.kind === "billing_cycle")
    return t("periodBilling", { id: serviceLabel(window.ref ?? "", t) });
  if (window.kind === "calendar" || window.kind === "rolling")
    return (
      t("periodInterval", {
        amount: Number(window.size),
        unit: t(`units.${window.unit}`, { defaultValue: window.unit }),
      }) + (window.timezone ? ` · ${window.timezone}` : "")
    );
  return t("periodNone");
}

export function displayMetric(
  metric: string,
  t: TFunction<"resources">,
): string {
  return t(`metrics.${metric}`, { defaultValue: metric.replaceAll("_", " ") });
}

export function displayScale(unit: string): number {
  return (
    (
      {
        GB: 1e9,
        MB: 1e6,
        MiB: 1048576,
        "GB-month": 1e9,
        day: 86400e6,
        hour: 3600e6,
        second: 1e6,
        ms: 1e3,
      } as Record<string, number>
    )[unit] ?? 1
  );
}

export function utcInput(value: number | undefined): string {
  return value == null
    ? ""
    : new Date(Number(value) * 1000).toISOString().slice(0, 16);
}

export function utcSeconds(value: string): number | undefined {
  return value ? Math.floor(new Date(value + "Z").getTime() / 1000) : undefined;
}
