import { useEffect, useState } from "react";
import { api, ApiError, type SessionInfo } from "./api/client";
import Login from "./views/Login";
import Workspace from "./views/Workspace";

type SessionState =
  | { kind: "checking" }
  | { kind: "anonymous"; error?: string }
  | { kind: "ready"; session: SessionInfo };

export default function App() {
  const [state, setState] = useState<SessionState>({ kind: "checking" });

  useEffect(() => {
    api
      .getSession()
      .then((session) => setState({ kind: "ready", session }))
      .catch(() => setState({ kind: "anonymous" }));
  }, []);

  if (state.kind === "checking") {
    return <div className="center">连接中…</div>;
  }
  if (state.kind === "anonymous") {
    return (
      <Login
        error={state.error}
        onLogin={(session) => setState({ kind: "ready", session })}
      />
    );
  }

  const logout = async () => {
    try {
      await api.deleteSession();
    } catch (error) {
      // Cookie already gone or auth expired: treat both as logged out.
      if (!(error instanceof ApiError && (error.status === 401 || error.status === 403))) {
        // fall through to local logout either way
      }
    }
    setState({ kind: "anonymous" });
  };

  return <Workspace session={state.session} onLogout={logout} />;
}
