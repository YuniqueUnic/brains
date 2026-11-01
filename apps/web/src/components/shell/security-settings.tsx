import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { api, ApiError } from "../../api/client";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

export function SecuritySettings({ onSignedOut }: { onSignedOut: (reason: string) => void }) {
  const { t } = useTranslation("security");
  const [current, setCurrent] = useState("");
  const [replacement, setReplacement] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [saved, setSaved] = useState(false);
  const [confirmRevoke, setConfirmRevoke] = useState(false);
  const [pending, setPending] = useState<"idle" | "rotate" | "revoke">("idle");
  const [error, setError] = useState<string | null>(null);
  const busy = pending !== "idle";

  function explainFailure(cause: unknown) {
    setError(cause instanceof ApiError && cause.status === 403 ? t("currentRejected") : t("requestFailed"));
  }

  async function rotate(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    setError(null);
    if (!/^[!-~]{32,4096}$/.test(replacement) || replacement === current) {
      setError(t("invalidReplacement"));
      return;
    }
    if (replacement !== confirmation) {
      setError(t("mismatch"));
      return;
    }
    if (!saved) return;
    setPending("rotate");
    try {
      await api.rotateToken(current, replacement);
      setCurrent("");
      setReplacement("");
      setConfirmation("");
      onSignedOut(t("rotated"));
    } catch (cause) {
      explainFailure(cause);
    } finally {
      setPending("idle");
    }
  }

  async function revoke() {
    if (busy) return;
    setError(null);
    setPending("revoke");
    try {
      await api.revokeSessions();
      onSignedOut(t("revoked"));
    } catch (cause) {
      explainFailure(cause);
    } finally {
      setPending("idle");
    }
  }

  return (
    <div className="flex flex-col gap-6 pb-1" aria-busy={busy}>
      <form onSubmit={(event) => void rotate(event)} className="flex flex-col gap-3">
        <div>
          <h3 className="text-body font-medium">{t("rotateTitle")}</h3>
          <p className="mt-1 text-body-sm text-muted-foreground">{t("rotateHint")}</p>
        </div>
        <label className="flex flex-col gap-1.5 text-body-sm">
          {t("currentToken")}
          <Input type="password" autoComplete="current-password" required maxLength={4096} value={current} onChange={(event) => setCurrent(event.target.value)} disabled={busy} />
        </label>
        <label className="flex flex-col gap-1.5 text-body-sm">
          {t("replacementToken")}
          <Input type="password" aria-label={t("replacementToken")} autoComplete="new-password" required minLength={32} maxLength={4096} value={replacement} onChange={(event) => { setReplacement(event.target.value); setSaved(false); }} disabled={busy} aria-describedby="replacement-token-hint" />
          <span id="replacement-token-hint" className="text-muted-foreground">{t("tokenHint")}</span>
        </label>
        <label className="flex flex-col gap-1.5 text-body-sm">
          {t("confirmToken")}
          <Input type="password" autoComplete="new-password" required minLength={32} maxLength={4096} value={confirmation} onChange={(event) => setConfirmation(event.target.value)} disabled={busy} />
        </label>
        <label className="flex items-start gap-2 text-body-sm text-muted-foreground">
          <input type="checkbox" className="mt-0.5 size-4 shrink-0 accent-foreground" checked={saved} onChange={(event) => setSaved(event.target.checked)} required disabled={busy} />
          {t("savedConfirmation")}
        </label>
        <Button type="submit" variant="default" className="self-start" disabled={busy || !saved || !current || !replacement || !confirmation}>
          {pending === "rotate" ? t("rotating") : t("rotateAction")}
        </Button>
      </form>

      <section className="flex flex-col items-start gap-3 border-t border-border pt-5">
        <div>
          <h3 className="text-body font-medium">{t("revokeTitle")}</h3>
          <p className="mt-1 text-body-sm text-muted-foreground">{t("revokeHint")}</p>
        </div>
        {confirmRevoke ? (
          <div className="flex flex-col gap-2">
            <p className="text-body-sm">{t("revokeConfirmation")}</p>
            <div className="flex flex-wrap gap-2">
              <Button variant="destructive" disabled={busy} onClick={() => void revoke()}>{pending === "revoke" ? t("revoking") : t("revokeAction")}</Button>
              <Button variant="outline" disabled={busy} onClick={() => setConfirmRevoke(false)}>{t("cancel")}</Button>
            </div>
          </div>
        ) : (
          <Button variant="outline" disabled={busy} onClick={() => setConfirmRevoke(true)}>{t("revokeAction")}</Button>
        )}
      </section>
      {error && <p role="alert" className="text-body-sm text-destructive">{error}</p>}
    </div>
  );
}
