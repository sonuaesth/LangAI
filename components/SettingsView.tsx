"use client";

import { useEffect, useState } from "react";
import { api } from "@/lib/tauri";

export function SettingsView() {
  const [key, setKey] = useState("");
  const [configured, setConfigured] = useState(false);
  const [model, setModel] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    api.settings().then(async settings => {
      setConfigured(settings.apiKeyConfigured);
      setModel(settings.model);
      if (settings.apiKeyConfigured) await refreshModels(settings.model);
    }).catch(reason => setMessage(String(reason)));
  }, []);

  async function refreshModels(savedModel?: string) {
    setModelsLoading(true);
    try {
      const available = await api.listModels();
      setModels(available);
      const preferred = savedModel ?? model;
      setModel(available.includes(preferred) ? preferred : (available[0] ?? ""));
      if (!available.length) setMessage("API не вернул совместимых текстовых моделей");
    } catch (reason) {
      setMessage(String(reason));
    } finally {
      setModelsLoading(false);
    }
  }

  async function save() {
    try {
      await api.saveSettings(model);
      setMessage("Настройки сохранены");
    } catch (reason) { setMessage(String(reason)); }
  }

  return <>
    <header><div><p className="eyebrow">Конфигурация</p><h1>Настройки</h1><p>Ключ остаётся в защищённом хранилище Windows и никогда не возвращается интерфейсу.</p></div></header>
    <div className="settingsGrid settingsSingle">
      <section className="card"><h2>OpenAI API</h2><div className="keyState">{configured ? "● Ключ настроен" : "○ Ключ не настроен"}</div>
        <label className="field"><span>Новый API-ключ</span><input type="password" autoComplete="off" value={key} onChange={event => setKey(event.target.value)} placeholder="sk-…"/></label>
        <label className="field"><span>Модель</span><select value={model} disabled={!configured || modelsLoading || !models.length} onChange={event => setModel(event.target.value)}>{modelsLoading && <option value="">Загрузка моделей…</option>}{!modelsLoading && !models.length && <option value="">Нет доступных моделей</option>}{models.map(item => <option value={item} key={item}>{item}</option>)}</select></label>
        <div className="actions"><button className="primary" disabled={!key} onClick={async () => { try { await api.verifyKey(key); await api.saveKey(key); setKey(""); setConfigured(true); await refreshModels(); setMessage("Ключ проверен, модели обновлены"); } catch (reason) { setMessage(String(reason)); } }}>Проверить и сохранить</button><button className="primary" disabled={!model} onClick={save}>Сохранить модель</button><button className="danger" disabled={!configured} onClick={async () => { await api.deleteKey(); setConfigured(false); setModels([]); setModel(""); }}>Удалить ключ</button></div>
      </section>
    </div>
    {message && <div className="notice">{message}</div>}
  </>;
}
