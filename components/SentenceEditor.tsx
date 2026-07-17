"use client";

import { useEffect, useState } from "react";
import { LANGUAGES } from "@/lib/languages";
import { api } from "@/lib/tauri";
import type { ManualBlock, ManualTranslation, SentenceDetails } from "@/lib/types";

const emptyBlock = (): ManualBlock => ({ correct: "", distractors: ["", "", ""], hint: "" });

export function SentenceEditor({ sentenceId, onClose, onSaved }: { sentenceId:number; onClose:()=>void; onSaved:()=>void }) {
  const [details, setDetails] = useState<SentenceDetails | null>(null);
  const [draft, setDraft] = useState<ManualTranslation>({ targetLanguage: LANGUAGES[0], translation: "", blocks: [emptyBlock()] });
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [audioBusy, setAudioBusy] = useState(false);
  const [audioFiles, setAudioFiles] = useState<Record<string, { name:string; mime:string }>>({});
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);

  useEffect(() => { api.sentenceDetails(sentenceId).then(value => {
    setDetails(value);
    setAudioFiles(Object.fromEntries(value.translations.filter(item => item.audioName).map(item => [item.targetLanguage, { name:item.audioName!, mime:item.audioMime ?? "audio/mpeg" }])));
    if (value.translations[0]) setDraft(value.translations[0]);
  }).catch(reason => setError(String(reason))); }, [sentenceId]);

  useEffect(() => {
    if (draggedIndex === null) return;
    const finish = () => setDraggedIndex(null);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => { window.removeEventListener("pointerup", finish); window.removeEventListener("pointercancel", finish); };
  }, [draggedIndex]);

  function chooseLanguage(targetLanguage: string) {
    const existing = details?.translations.find(item => item.targetLanguage === targetLanguage);
    setDraft(existing ? structuredClone(existing) : { targetLanguage, translation: "", blocks: [emptyBlock()] });
    setError("");
  }

  function updateBlock(index: number, update: (block: ManualBlock) => ManualBlock) {
    setDraft(current => ({ ...current, blocks: current.blocks.map((block, position) => position === index ? update(block) : block) }));
  }

  function moveBlock(from: number, to: number) {
    if (from === to) return;
    setDraft(current => {
      const blocks = [...current.blocks];
      const [moved] = blocks.splice(from, 1);
      blocks.splice(to, 0, moved);
      return { ...current, blocks };
    });
  }

  async function save() {
    setSaving(true); setError("");
    try { await api.saveManualTranslation(sentenceId, draft); await onSaved(); onClose(); }
    catch (reason) { setError(String(reason)); }
    finally { setSaving(false); }
  }

  async function uploadAudio(file?: File) {
    if (!file) return;
    setAudioBusy(true); setError("");
    try {
      const extension = file.name.split(".").pop()?.toLowerCase();
      const fallback: Record<string,string> = { mp3:"audio/mpeg", wav:"audio/wav", ogg:"audio/ogg", m4a:"audio/mp4", mp4:"audio/mp4", webm:"audio/webm" };
      const mimeType = file.type || fallback[extension ?? ""] || "application/octet-stream";
      await api.saveAudio(sentenceId, draft.targetLanguage, file.name, mimeType, Array.from(new Uint8Array(await file.arrayBuffer())));
      setAudioFiles(current => ({ ...current, [draft.targetLanguage]: { name:file.name, mime:mimeType } }));
    } catch (reason) { setError(String(reason)); }
    finally { setAudioBusy(false); }
  }

  async function playAudio() {
    setAudioBusy(true); setError("");
    try {
      const audio = await api.sentenceAudio(sentenceId, draft.targetLanguage);
      const url = URL.createObjectURL(new Blob([new Uint8Array(audio.bytes)], { type: audio.mimeType }));
      const player = new Audio(url);
      player.addEventListener("ended", () => URL.revokeObjectURL(url), { once: true });
      player.addEventListener("error", () => URL.revokeObjectURL(url), { once: true });
      await player.play();
    } catch (reason) { setError(String(reason)); }
    finally { setAudioBusy(false); }
  }

  async function deleteAudio() {
    setAudioBusy(true); setError("");
    try { await api.deleteAudio(sentenceId, draft.targetLanguage); setAudioFiles(current => { const next = { ...current }; delete next[draft.targetLanguage]; return next; }); }
    catch (reason) { setError(String(reason)); }
    finally { setAudioBusy(false); }
  }

  return <div className="editorBackdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="sentenceEditor" role="dialog" aria-modal="true" aria-label="Редактор предложения">
      <div className="editorHeader"><div><span>Карточка предложения</span><h2>{details?.sourceText ?? "Загрузка…"}</h2>{details?.topics.length ? <div className="editorTopics">{details.topics.map(item => <i key={item}>#{item}</i>)}</div> : null}</div><button className="editorClose" onClick={onClose} aria-label="Закрыть">×</button></div>
      <div className="editorControls">
        <label><span>Язык перевода</span><select value={draft.targetLanguage} onChange={event => chooseLanguage(event.target.value)}>{LANGUAGES.map(item => <option key={item}>{item}</option>)}</select></label>
        <div className="availableTranslations"><span>Готовые переводы:</span>{details?.translations.length ? details.translations.map(item => <button className={item.targetLanguage === draft.targetLanguage ? "active" : ""} key={item.targetLanguage} onClick={() => chooseLanguage(item.targetLanguage)}>{item.targetLanguage}</button>) : <small>пока нет</small>}</div>
      </div>
      <label className="editorField"><span>Готовый перевод</span><input value={draft.translation} onChange={event => setDraft(current => ({ ...current, translation: event.target.value }))} placeholder="Ich bin früh in Italien eingeschlafen."/></label>
      <div className="audioAttachment"><div><strong>Аудио для языка {draft.targetLanguage}</strong><small>{audioFiles[draft.targetLanguage]?.name ?? "Файл пока не добавлен. Поддерживаются MP3, WAV, OGG, M4A и WebM до 25 МБ."}</small></div><label className="audioUpload"><input type="file" accept="audio/mpeg,audio/wav,audio/ogg,audio/mp4,audio/webm,.mp3,.wav,.ogg,.m4a,.webm" disabled={audioBusy} onChange={event => { void uploadAudio(event.target.files?.[0]); event.currentTarget.value = ""; }}/>{audioBusy ? "Обработка…" : audioFiles[draft.targetLanguage] ? "Заменить аудио" : "Добавить аудио"}</label>{audioFiles[draft.targetLanguage] && <><button disabled={audioBusy} onClick={() => void playAudio()}>▶ Прослушать</button><button className="danger" disabled={audioBusy} onClick={() => void deleteAudio()}>Удалить аудио</button></>}</div>
      <div className="positionsHeader"><div><h3>Позиции предложения</h3><small>В каждой позиции: правильный вариант и три неверных.</small></div><button onClick={() => setDraft(current => ({ ...current, translation: current.blocks.map(block => block.correct.trim()).filter(Boolean).join(" ") }))}>Собрать перевод из позиций</button></div>
      <div className="manualBlocks">{draft.blocks.map((block, index) => <article
        className={`manualBlock ${draggedIndex === index ? "dragging" : ""}`}
        key={index}
        onPointerEnter={() => { if (draggedIndex !== null && draggedIndex !== index) { moveBlock(draggedIndex, index); setDraggedIndex(index); } }}
      >
        <div className="manualBlockHead"><strong><button type="button" className="dragHandle" title="Перетащите, чтобы изменить порядок" aria-label={`Перетащить позицию ${index + 1}`} onPointerDown={event => { event.preventDefault(); setDraggedIndex(index); }}>⠿</button> Позиция {index + 1}</strong><button disabled={draft.blocks.length === 1} onClick={() => setDraft(current => ({ ...current, blocks: current.blocks.filter((_, position) => position !== index) }))}>Удалить</button></div>
        <label><span>Правильный вариант</span><input value={block.correct} onChange={event => updateBlock(index, current => ({ ...current, correct: event.target.value }))}/></label>
        <span className="variantsLabel">Неверные варианты</span>
        {block.distractors.map((option, optionIndex) => <input key={optionIndex} value={option} onChange={event => updateBlock(index, current => ({ ...current, distractors: current.distractors.map((value, position) => position === optionIndex ? event.target.value : value) }))} placeholder={`Вариант ${optionIndex + 1}`}/>)}
        <label><span>Подсказка <small>необязательно</small></span><input value={block.hint ?? ""} onChange={event => updateBlock(index, current => ({ ...current, hint: event.target.value }))}/></label>
      </article>)}</div>
      <button className="addPosition" onClick={() => setDraft(current => ({ ...current, blocks: [...current.blocks, emptyBlock()] }))}>+ Добавить позицию</button>
      {error && <div className="error">{error}</div>}
      <div className="editorActions"><button onClick={onClose}>Отмена</button><button className="primary" disabled={saving || !details} onClick={save}>{saving ? "Сохранение…" : "Сохранить перевод"}</button></div>
    </section>
  </div>;
}
