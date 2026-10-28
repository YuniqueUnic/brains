import catalog from "../../../../crates/domain/resources/cloudflare-free-2026-10-27.json";
import type { brains } from "../../../../sdk/typescript/brains";
import { ApiError } from "./client";
import {
  effectiveBudget,
  validResourceNumbers,
  type ResourceInput,
  type ResourceReport,
} from "../lib/resource-policy";

const KEY = "brains-demo-resource-settings-v1";
const simulatedUse: Record<string, number> = {
  "worker-requests": 1240,
  "r2-class-a": 56,
  "r2-class-b": 438,
  "d1-reads": 12860,
  "d1-writes": 320,
  "search-semantic": 12,
};
const now = new Date();
const start = Math.floor(
  Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1) / 1000,
);
const end = Math.floor(
  Date.UTC(now.getUTCFullYear(), now.getUTCMonth() + 1, 1) / 1000,
);
const baseRules: brains.ResourceRule[] = catalog.rules.map((rule) => ({
  ...rule,
  policy: rule.policy ?? {},
  window: {
    ...rule.window,
    size: "size" in rule.window ? rule.window.size : undefined,
  },
  source: rule.source
    ? catalog.sources[rule.source as keyof typeof catalog.sources]
    : undefined,
  enforcement: [
    "worker-requests",
    "r2-class-a",
    "r2-class-b",
    "search-semantic",
  ].includes(rule.id)
    ? "simulated-admission"
    : rule.id.startsWith("d1-")
      ? "simulated-observation"
      : "platform-catalog",
}));

function initial(): ResourceInput {
  return {
    expected_revision: "demo-default",
    enforced: true,
    policy: { ...catalog.policy },
    rules: baseRules.map((rule) => ({
      id: rule.id,
      limit: { ...rule.limit },
      policy: { ...rule.policy },
    })),
    billing_cycles: Object.keys(catalog.context.billing_cycles).map((id) => ({
      id,
      start,
      end,
    })),
  };
}
function read(): ResourceInput {
  const raw = localStorage.getItem(KEY);
  if (raw) {
    try {
      return JSON.parse(raw) as ResourceInput;
    } catch {
      localStorage.removeItem(KEY);
    }
  }
  return initial();
}
function report(state: ResourceInput): ResourceReport {
  const rules = baseRules.map((base) => {
    const rule = {
      ...base,
      ...state.rules.find((item) => item.id === base.id),
    };
    return { ...rule, effective_limit: effectiveBudget(rule, state.policy) };
  });
  return {
    settings: {
      revision: state.expected_revision,
      enforced: state.enforced,
      source: "demo",
      policy: state.policy,
      billing_cycles: state.billing_cycles,
    },
    profile: {
      profile_id: catalog.profile.id,
      reviewed_at: catalog.profile.reviewed_at,
      coverage:
        "Simulated browser demo. No Cloudflare account usage is queried.",
    },
    rules,
    usage: rules
      .filter((rule) => rule.id in simulatedUse)
      .map((rule) => ({
        key: `demo:${rule.id}`,
        meter: `demo:${rule.id}`,
        rule: rule.id,
        unit: rule.measure.unit,
        evidence: "simulated",
        consumed: simulatedUse[rule.id],
        reserved: 0,
        limit: rule.effective_limit ?? 0,
        reset_at:
          rule.window.kind === "calendar"
            ? Math.floor(
                Date.UTC(
                  now.getUTCFullYear(),
                  now.getUTCMonth(),
                  now.getUTCDate() + 1,
                ) / 1000,
              )
            : end,
      })),
    limitations: [
      "Illustrative usage only; this demo does not consume or administer a Cloudflare account.",
    ],
  };
}
export async function demoResourceStatus(): Promise<ResourceReport> {
  return report(read());
}
export async function updateDemoResources(
  input: ResourceInput,
): Promise<ResourceReport> {
  const current = read();
  if (input.expected_revision !== current.expected_revision)
    throw new ApiError(
      412,
      "REVISION_CONFLICT",
      "Settings changed in another tab. Refresh usage and apply your changes again.",
    );
  if (!validResourceNumbers(input))
    throw new ApiError(
      400,
      "INVALID_ARGUMENT",
      "Limits and budgets must be finite non-negative numbers, and budgets cannot exceed provider ceilings.",
    );
  const ids = new Set<string>();
  for (const update of input.rules) {
    const base = baseRules.find((rule) => rule.id === update.id);
    if (!base || ids.has(update.id) || base.limit.kind !== update.limit.kind)
      throw new ApiError(
        400,
        "INVALID_ARGUMENT",
        "Unknown, duplicate or incompatible resource rule.",
      );
    ids.add(update.id);
  }
  for (const cycle of input.billing_cycles) {
    if (
      (cycle.start == null) !== (cycle.end == null) ||
      (cycle.start != null && cycle.end != null && cycle.start >= cycle.end)
    )
      throw new ApiError(
        400,
        "INVALID_ARGUMENT",
        "Billing periods need a start before their end.",
      );
    const old = current.billing_cycles.find((item) => item.id === cycle.id);
    const measured = baseRules.some(
      (rule) => rule.window.ref === cycle.id && simulatedUse[rule.id] > 0,
    );
    if (
      old?.end != null &&
      measured &&
      (old.start !== cycle.start || old.end !== cycle.end) &&
      (cycle.start == null || cycle.start < old.end)
    )
      throw new ApiError(
        400,
        "INVALID_ARGUMENT",
        "This billing period already has recorded usage. Keep its dates or enter the next non-overlapping period.",
      );
  }
  if (input.enforced) {
    const r2 = input.billing_cycles.find((cycle) => cycle.id === "r2");
    const seconds = Math.floor(Date.now() / 1000);
    if (
      r2?.start == null ||
      r2.end == null ||
      r2.start > seconds ||
      r2.end <= seconds
    )
      throw new ApiError(
        400,
        "INVALID_ARGUMENT",
        "Set the current R2 billing period before enabling budgets.",
      );
  }
  const next = { ...input, expected_revision: crypto.randomUUID() };
  localStorage.setItem(KEY, JSON.stringify(next));
  return report(next);
}
