"use client";

import { useEffect, useState } from "react";
import { api } from "@/lib/tauri";
import { LANGUAGES } from "@/lib/languages";
import type { Sentence } from "@/lib/types";

const labels = { unprepared: "Не готово", queued: "В очереди", generating: "Генерация", ready: "Готово", failed: "Ошибка" };

export function SentencesView() {
  const [rows, setRows] = useState<Sentence[]>([]);
  const [text, setText] = useState("");
  const [translationComment, setTranslationComment] = useState("");
  const [targetLanguage, setTargetLanguage] = useState<string>(LANGUAGES[0]);
  const [filterLanguage, setFilterLanguage] = useState<string>("");
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState("");

  const load = () => api.listSentences(filterLanguage || undefined, targetLanguage).then(setRows).catch(reason => setError(String(reason)));

  useEffect(() => { setSelected(new Set()); void load(); }, [filterLanguage, targetLanguage]);
  useEffect(() => {
    let off: undefined | (() => void);
    api.onProgress(progress => setRows(current => current.map(row => row.id === progress.sentenceId ? { ...row, status: progress.status, error: progress.error } : row))).then(value => off = value);
    return () => off?.();
  }, []);

  async function add() {
    const texts = text.split(/\r?\n/).map(value => value.trim()).filter(Boolean);
    if (!texts.length) return;
    try { await api.addSentences(texts, targetLanguage, translationComment); setText(""); await load(); }
    catch (reason) { setError(String(reason)); }
  }

  async function prepare(ids?: number[]) {
    try { await api.prepare(ids, targetLanguage, translationComment); await load(); }
    catch (reason) { setError(String(reason)); }
  }

  return <>
    <header><div><p className="eyebrow">Библиотека</p><h1>Предложения</h1><p>Предложения каждого языка хранятся и практикуются отдельно.</p></div></header>
    <div className="sentenceLanguage"><label><span>Перевести на</span><select value={targetLanguage} onChange={event => setTargetLanguage(event.target.value)}>{LANGUAGES.map(item => <option value={item} key={item}>{item}</option>)}</select></label><label><span>Фильтр</span><select value={filterLanguage} onChange={event => setFilterLanguage(event.target.value)}><option value="">Все языки</option>{LANGUAGES.map(item => <option value={item} key={item}>{item}</option>)}</select></label></div>
    <div className="card composer translationComposer"><div className="composerFields"><label><span>Предложения</span><textarea value={text} onChange={event => setText(event.target.value)} placeholder={"Я рано заснул в Италии.\nЗавтра мы идём в музей."}/></label><label><span>Комментарий к переводу <small>необязательно</small></span><textarea className="commentInput" maxLength={1000} value={translationComment} onChange={event => setTranslationComment(event.target.value)} placeholder="Например: переведи разговорно; предлоги выделяй отдельными блоками; используй британский английский…"/></label></div><button className="primary" onClick={add}>Добавить</button></div>
    {error && <div className="error">{error}</div>}
    <div className="toolbar"><label><input type="checkbox" checked={rows.length > 0 && selected.size === rows.length} onChange={event => setSelected(event.target.checked ? new Set(rows.map(row => row.id)) : new Set())}/> Выбрать все</label><span/><button onClick={() => prepare([...selected])} disabled={!selected.size}>Подготовить выбранные</button><button onClick={() => prepare()}>Подготовить все новые</button><button className="danger" disabled={!selected.size} onClick={async () => { await api.deleteSentences([...selected]); setSelected(new Set()); await load(); }}>Удалить</button></div>
    <div className="list">{rows.length ? rows.map(sentence => <article className="sentence" key={sentence.id}><input type="checkbox" checked={selected.has(sentence.id)} onChange={() => setSelected(current => { const next = new Set(current); next.has(sentence.id) ? next.delete(sentence.id) : next.add(sentence.id); return next; })}/><div><strong>{sentence.sourceText}</strong><div className="languageBadges">{sentence.languages.map(item => <span className={`languageBadge ${item.status}`} key={item.targetLanguage}>{item.targetLanguage}</span>)}</div>{sentence.error && <small className="errorText">{sentence.error}</small>}</div><span className={`badge ${sentence.status}`}>{labels[sentence.status]}</span><button onClick={() => prepare([sentence.id])}>{sentence.languages.some(item => item.targetLanguage === targetLanguage && item.status === "ready") ? "Создать заново" : `Подготовить: ${targetLanguage}`}</button></article>) : <div className="empty">По выбранному фильтру предложений пока нет.</div>}</div>
  </>;
}
