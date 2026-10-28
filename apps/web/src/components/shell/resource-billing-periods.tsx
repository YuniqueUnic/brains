import { useTranslation } from "react-i18next";
import type { ResourceInput } from "../../lib/resource-policy";
import { serviceLabel, utcInput, utcSeconds } from "../../lib/resource-policy";
import { Input } from "../ui/input";

export function ResourceBillingPeriods({
  cycles,
  onChange,
}: {
  cycles: ResourceInput["billing_cycles"];
  onChange: (cycles: ResourceInput["billing_cycles"]) => void;
}) {
  const { t } = useTranslation("resources");
  return (
    <details className="border-t border-border pt-3">
      <summary className="cursor-pointer text-body-sm font-medium">
        {t("billingPeriods")}
      </summary>
      <p className="my-2 text-body-sm text-muted-foreground">
        {t("billingHelp")}
      </p>
      <div className="space-y-3">
        {cycles.map((cycle) => (
          <fieldset key={cycle.id} className="grid gap-2 sm:grid-cols-2">
            <legend className="mb-1 text-body-sm">
              {serviceLabel(cycle.id, t)}
            </legend>
            {(["start", "end"] as const).map((key) => (
              <label key={key} className="space-y-1 text-meta-lg">
                <span>{t(key === "start" ? "starts" : "ends")}</span>
                <Input
                  aria-label={t(key === "start" ? "periodStart" : "periodEnd", {
                    service: serviceLabel(cycle.id, t),
                  })}
                  type="datetime-local"
                  value={utcInput(cycle[key])}
                  onChange={(event) =>
                    onChange(
                      cycles.map((item) =>
                        item.id === cycle.id
                          ? { ...item, [key]: utcSeconds(event.target.value) }
                          : item,
                      ),
                    )
                  }
                />
              </label>
            ))}
          </fieldset>
        ))}
      </div>
    </details>
  );
}
