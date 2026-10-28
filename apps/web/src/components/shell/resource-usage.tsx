import { useEffect, useState } from "react";
import { api } from "../../api/client";
import { Button } from "../ui/button";

type Report = Awaited<ReturnType<typeof api.resourceStatus>>;
type State = { kind: "loading" } | { kind: "failed"; message: string } | { kind: "ready"; report: Report };

export function ResourceUsage() {
  const [state, setState] = useState<State>({ kind: "loading" });
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let active = true;
    void api.resourceStatus().then((report) => {
      if (active) setState({ kind: "ready", report });
    }).catch((error: unknown) => {
      if (active) setState({ kind: "failed", message: error instanceof Error ? error.message : String(error) });
    });
    return () => { active = false; };
  }, [reload]);
  return <section className="border-t border-border pt-4" aria-labelledby="resource-usage-title">
    <div className="flex items-center justify-between gap-3">
      <h3 id="resource-usage-title" className="text-sm font-medium">Resource usage</h3>
      <Button type="button" variant="ghost" size="sm" disabled={state.kind === "loading"} onClick={() => { setState({ kind: "loading" }); setReload((value) => value + 1); }}>Refresh</Button>
    </div>
    {state.kind === "loading" && <p role="status" className="text-sm text-muted-foreground">Loading usage…</p>}
    {state.kind === "failed" && <p role="alert" className="text-sm text-destructive">{state.message}</p>}
    {state.kind === "ready" && <>
      <p className="mt-1 text-xs text-muted-foreground">Application counters. Review current provider pricing and edit the deployment profile to change limits.</p>
      {state.report.usage.length === 0 ? <p className="mt-3 text-sm">No measured usage in the current windows.</p> : <dl className="mt-2 divide-y divide-border">
        {state.report.usage.map((usage) => <div key={usage.key} className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 py-2 text-xs">
          <dt className="min-w-0 break-words">{usage.rule}<span className="mt-0.5 block text-muted-foreground">{usage.evidence}{usage.reset_at ? ` · resets ${new Date(Number(usage.reset_at) * 1000).toLocaleDateString()}` : ""}</span></dt>
          <dd className="text-right tabular-nums">{usage.consumed.toLocaleString()} / {usage.limit.toLocaleString()} {usage.unit}{usage.reserved > 0 && <span className="block text-muted-foreground">{usage.reserved.toLocaleString()} reserved</span>}</dd>
        </div>)}
      </dl>}
      <p className="mt-2 text-xs text-muted-foreground">Pricing snapshot reviewed {state.report.profile.reviewed_at}. Other applications and retained storage can consume additional account usage.</p>
    </>}
  </section>;
}
