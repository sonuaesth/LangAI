"use client";

import { useEffect, useState } from "react";
import { api } from "@/lib/tauri";
import type { ElevenLabsVoice } from "@/lib/types";

export function SettingsView() {
  const [key, setKey] = useState("");
  const [configured, setConfigured] = useState(false);
  const [model, setModel] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [message, setMessage] = useState("");
  const [elevenKey, setElevenKey] = useState("");
  const [elevenConfigured, setElevenConfigured] = useState(false);
  const [voices, setVoices] = useState<ElevenLabsVoice[]>([]);
  const [voiceId, setVoiceId] = useState("");
  const [voicesLoading, setVoicesLoading] = useState(false);

  useEffect(() => {
    api.settings().then(async settings => {
      setConfigured(settings.apiKeyConfigured);
      setModel(settings.model);
      setElevenConfigured(settings.elevenlabsKeyConfigured);
      setVoiceId(settings.elevenlabsVoiceId ?? "");
      if (settings.apiKeyConfigured) await refreshModels(settings.model);
      if (settings.elevenlabsKeyConfigured) await refreshVoices(settings.elevenlabsVoiceId ?? undefined);
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

  async function refreshVoices(savedVoice?: string) {
    setVoicesLoading(true);
    try {
      const available = await api.elevenLabsVoices();
      setVoices(available);
      const preferred = savedVoice ?? voiceId;
      setVoiceId(available.some(item => item.voiceId === preferred) ? preferred : (available[0]?.voiceId ?? ""));
    } catch (reason) { setMessage(String(reason)); }
    finally { setVoicesLoading(false); }
  }

  return <>
    <header><div><p className="eyebrow">Конфигурация</p><h1>Настройки</h1><p>Ключ остаётся в защищённом хранилище Windows и никогда не возвращается интерфейсу.</p></div></header>
    <div className="settingsGrid settingsSingle">
      <section className="card"><h2>OpenAI API</h2><div className="keyState">{configured ? "● Ключ настроен" : "○ Ключ не настроен"}</div>
        <label className="field"><span>Новый API-ключ</span><input type="password" autoComplete="off" value={key} onChange={event => setKey(event.target.value)} placeholder="sk-…"/></label>
        <label className="field"><span>Модель</span><select value={model} disabled={!configured || modelsLoading || !models.length} onChange={event => setModel(event.target.value)}>{modelsLoading && <option value="">Загрузка моделей…</option>}{!modelsLoading && !models.length && <option value="">Нет доступных моделей</option>}{models.map(item => <option value={item} key={item}>{item}</option>)}</select></label>
        <div className="actions"><button className="primary" disabled={!key} onClick={async () => { try { await api.verifyKey(key); await api.saveKey(key); setKey(""); setConfigured(true); await refreshModels(); setMessage("Ключ проверен, модели обновлены"); } catch (reason) { setMessage(String(reason)); } }}>Проверить и сохранить</button><button className="primary" disabled={!model} onClick={save}>Сохранить модель</button><button className="danger" disabled={!configured} onClick={async () => { await api.deleteKey(); setConfigured(false); setModels([]); setModel(""); }}>Удалить ключ</button></div>
      </section>
      <section className="card"><h2>ElevenLabs API</h2><div className="keyState">{elevenConfigured ? "● Ключ настроен" : "○ Ключ не настроен"}</div>
        <p>Используется только для создания озвучки. Ключ хранится в защищённом хранилище Windows.</p>
        <label className="field"><span>Новый API-ключ ElevenLabs</span><input type="password" autoComplete="off" value={elevenKey} onChange={event => setElevenKey(event.target.value)} placeholder="sk_…"/></label>
        <label className="field"><span>Голос</span><select value={voiceId} disabled={!elevenConfigured || voicesLoading || !voices.length} onChange={event => setVoiceId(event.target.value)}>{voicesLoading && <option value="">Загрузка голосов…</option>}{!voicesLoading && !voices.length && <option value="">Нет доступных голосов</option>}{voices.map(item => <option value={item.voiceId} key={item.voiceId}>{item.name}</option>)}</select></label>
        <div className="actions"><button className="primary" disabled={!elevenKey} onClick={async () => { try { const available = await api.verifyElevenLabsKey(elevenKey); await api.saveElevenLabsKey(elevenKey); setElevenKey(""); setElevenConfigured(true); setVoices(available); setVoiceId(available[0]?.voiceId ?? ""); setMessage("Ключ ElevenLabs проверен, голоса загружены"); } catch (reason) { setMessage(String(reason)); } }}>Проверить и сохранить</button><button className="primary" disabled={!voiceId} onClick={async () => { const voice = voices.find(item => item.voiceId === voiceId); if (!voice) return; await api.saveElevenLabsVoice(voice.voiceId, voice.name); setMessage("Голос ElevenLabs сохранён"); }}>Сохранить голос</button><button className="danger" disabled={!elevenConfigured} onClick={async () => { await api.deleteElevenLabsKey(); setElevenConfigured(false); setVoices([]); setVoiceId(""); }}>Удалить ключ</button></div>
      </section>
    </div>
    {message && <div className="notice">{message}</div>}
  </>;
}
