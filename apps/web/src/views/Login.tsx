import { Brain, KeyRound, LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useState } from "react";
import { api, ApiError, type SessionInfo } from "../api/client";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";

export default function Login({
  onLogin,
  error,
}: {
  onLogin: (session: SessionInfo) => void;
  error?: string;
}) {
  const { t } = useTranslation("login");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(error ?? null);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!token.trim() || busy) return;
    setBusy(true);
    setMessage(null);
    try {
      const session = await api.createSession(token.trim());
      onLogin(session);
    } catch (err) {
      setMessage(
        err instanceof ApiError
          ? t("failed", { message: err.message })
          : t("networkError"),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center bg-background p-6">
      <form
        className="w-[22rem] rounded-[12px] border bg-card p-6 shadow-overlay"
        onSubmit={submit}
      >
        <div className="mb-5 flex flex-col items-center gap-2 text-center">
          <span className="flex size-10 items-center justify-center rounded-[10px] bg-primary/10 text-primary">
            <Brain className="size-5 stroke-[1.5]" />
          </span>
          <h1 className="text-[15px] font-semibold tracking-tight text-foreground">Brains</h1>
          <p className="text-xs text-muted-foreground">{t("subtitle")}</p>
        </div>
        <div className="relative">
          <KeyRound className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 stroke-[1.5] -translate-y-1/2 text-muted-foreground" />
          <Input
            data-testid="token-input"
            type="password"
            placeholder={t("placeholder")}
            className="h-9 pl-8"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            autoFocus
          />
        </div>
        <Button
          data-testid="login-button"
          type="submit"
          variant="default"
          className="mt-3 h-9 w-full text-sm"
          disabled={busy || !token.trim()}
        >
          {busy && <LoaderCircle className="size-3.5 animate-spin" />}
          {busy ? t("submitting") : t("submit")}
        </Button>
        {message && (
          <p
            className="mt-3 rounded-md px-2.5 py-1.5 text-center text-xs"
            data-testid="login-error"
            role="alert"
            style={{
              backgroundColor: "color-mix(in srgb, var(--destructive) 8%, transparent)",
              color: "color-mix(in srgb, var(--destructive) 80%, var(--foreground))",
            }}
          >
            {message}
          </p>
        )}
      </form>
    </div>
  );
}
