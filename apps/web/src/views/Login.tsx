import { useState } from "react";
import { api, ApiError, type SessionInfo } from "../api/client";

export default function Login({
  onLogin,
  error,
}: {
  onLogin: (session: SessionInfo) => void;
  error?: string;
}) {
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
        err instanceof ApiError ? `登录失败：${err.message}` : "登录失败：网络错误",
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="center">
      <form className="login-card" onSubmit={submit}>
        <h1>BrainStore</h1>
        <p className="muted">使用访问令牌登录本机知识库</p>
        <input
          data-testid="token-input"
          type="password"
          placeholder="访问令牌"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          autoFocus
        />
        <button data-testid="login-button" type="submit" disabled={busy || !token.trim()}>
          {busy ? "登录中…" : "登录"}
        </button>
        {message && (
          <p className="error" data-testid="login-error">
            {message}
          </p>
        )}
      </form>
    </div>
  );
}
