"use client";

import { useEffect, useState } from "react";
import { BookOpenCheck, ListPlus, Settings as SettingsIcon } from "lucide-react";
import { AuthGate } from "@/components/AuthGate";
import { ExerciseView } from "@/components/ExerciseView";
import { SentencesView } from "@/components/SentencesView";
import { SettingsView } from "@/components/SettingsView";
import { api, isDesktopApp } from "@/lib/tauri";

type Tab = "exercise" | "sentences" | "settings";

export default function Home() {
  const [tab, setTab] = useState<Tab>("exercise");
  useEffect(() => {
    if (!isDesktopApp()) return;
    const synchronize = () => api.syncStatus()
      .then(status => status.connected ? api.syncNow() : undefined)
      .catch(() => undefined);
    void synchronize();
    const timer = window.setInterval(synchronize, 30_000);
    return () => window.clearInterval(timer);
  }, []);
  const items = [
    ["exercise", "Упражнения", BookOpenCheck],
    ["sentences", "Предложения", ListPlus],
    ["settings", "Настройки", SettingsIcon],
  ] as const;
  return <AuthGate><main className="shell">
    <aside>
      <div className="brand"><span>LA</span><div>LangAI<small>переводы в ритме</small></div></div>
      <nav>{items.map(([id, label, Icon]) => <button key={id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}><Icon size={19}/>{label}</button>)}</nav>
      <div className="offline">● LangAI</div>
    </aside>
    <section className="content">{tab === "exercise" ? <ExerciseView/> : tab === "sentences" ? <SentencesView/> : <SettingsView/>}</section>
  </main></AuthGate>;
}
