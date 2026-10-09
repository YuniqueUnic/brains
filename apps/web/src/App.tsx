import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Toaster } from "sonner";
import { Tooltip as TooltipPrimitive } from "@base-ui-components/react/tooltip";
import { api, ApiError, type SessionInfo } from "./api/client";
import { UiSettingsProvider } from "./components/shell/settings-dialog";
import Login from "./views/Login";
import Workspace from "./views/Workspace";

type SessionState =
  | { kind: "checking" }
  | { kind: "anonymous"; error?: string }
  | { kind: "ready"; session: SessionInfo };

export default function App() {
  const { t } = useTranslation("login");
  const [state, setState] = useState<SessionState>({ kind: "checking" });

  useEffect(() => {
    api
      .getSession()
      .then((session) => setState({ kind: "ready", session }))
      .catch(() => setState({ kind: "anonymous" }));
  }, []);

  if (state.kind === "checking") {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        {t("connecting")}
      </div>
    );
  }
  if (state.kind === "anonymous") {
    return (
      <UiSettingsProvider>
        <TooltipPrimitive.Provider delay={400}>
          <Login
            error={state.error}
            onLogin={(session) => setState({ kind: "ready", session })}
          />
        </TooltipPrimitive.Provider>
      </UiSettingsProvider>
    );
  }

  const logout = async (reason?: string) => {
    try {
      await api.deleteSession();
    } catch (error) {
      // Cookie already gone or auth expired: treat both as logged out.
      if (!(error instanceof ApiError && (error.status === 401 || error.status === 403))) {
        // fall through to local logout either way
      }
    }
    // A mid-work expiry must EXPLAIN itself: the banner set in Workspace dies
    // with it on unmount, so the reason rides into the Login screen.
    setState({ kind: "anonymous", error: reason });
  };

  return (
    <UiSettingsProvider>
      <TooltipPrimitive.Provider delay={400}>
        <Workspace session={state.session} onLogout={logout} />
        <Toaster
          position="bottom-center"
          toastOptions={{
            style: {
              background: "var(--card)",
              color: "var(--foreground)",
              border: "1px solid var(--border)",
              borderRadius: "8px",
              fontSize: "12.5px",
            },
          }}
        />
      </TooltipPrimitive.Provider>
    </UiSettingsProvider>
  );
}
