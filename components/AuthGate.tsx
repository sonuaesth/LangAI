"use client";

import { useEffect, useState } from "react";
import { webAuth } from "@/lib/web";

export function AuthGate({ children }: { children: React.ReactNode }) {
  const [desktop, setDesktop] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const [loading, setLoading] = useState(true);
  const [registering, setRegistering] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [invite, setInvite] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    const isDesktop = "__TAURI_INTERNALS__" in window;
    setDesktop(isDesktop);
    if (isDesktop) {
      setAuthenticated(true);
      setLoading(false);
      return;
    }
    webAuth.session().then(session => setAuthenticated(session.authenticated))
      .catch(reason => setError(String(reason))).finally(() => setLoading(false));
  }, []);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setError("");
    try {
      if (registering) await webAuth.register(email, password, invite);
      else await webAuth.login(email, password);
      setAuthenticated(true);
    } catch (reason) { setError(String(reason)); }
  }

  if (loading) return <main className="authShell"><div className="card authCard">Загрузка…</div></main>;
  if (desktop || authenticated) return <>{children}</>;
  return <main className="authShell"><form className="card authCard" onSubmit={submit}>
    <div className="brand"><span>LA</span><div>LangAI<small>переводы в ритме</small></div></div>
    <div><p className="eyebrow">Web-приложение</p><h1>{registering ? "Регистрация" : "Вход"}</h1></div>
    <label className="field"><span>Email</span><input type="email" required autoComplete="email" value={email} onChange={event => setEmail(event.target.value)}/></label>
    <label className="field"><span>Пароль</span><input type="password" required minLength={12} autoComplete={registering ? "new-password" : "current-password"} value={password} onChange={event => setPassword(event.target.value)}/></label>
    {registering && <label className="field"><span>Код приглашения</span><input value={invite} onChange={event => setInvite(event.target.value)}/></label>}
    {error && <div className="error">{error}</div>}
    <button className="primary" type="submit">{registering ? "Создать аккаунт" : "Войти"}</button>
    <button type="button" onClick={() => { setRegistering(value => !value); setError(""); }}>{registering ? "У меня уже есть аккаунт" : "Создать аккаунт"}</button>
  </form></main>;
}
