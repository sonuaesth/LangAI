import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ElevenLabsVoice,
  EntityId,
  Exercise,
  ManualTranslation,
  Progress,
  Sentence,
  SentenceDetails,
  Settings,
  SyncStatus,
} from "./types";
import { webApi } from "./web";

const isDesktop = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const isDesktopApp = isDesktop;
const call = <T>(command: string, args: Record<string, unknown> = {}) =>
  invoke<T>(command, args);
const select = <T>(desktop: () => Promise<T>, web: () => Promise<T>) =>
  isDesktop() ? desktop() : web();

export const api = {
  listSentences: (filterLanguage?: string, targetLanguage?: string, filterTopic?: string) =>
    select(
      () => call<Sentence[]>("list_sentences", { filterLanguage: filterLanguage ?? null, targetLanguage: targetLanguage ?? null, filterTopic: filterTopic ?? null }),
      () => webApi.listSentences(filterLanguage, targetLanguage, filterTopic),
    ),
  addSentences: (texts: string[], targetLanguage: string, translationComment?: string, topic?: string) =>
    select(
      () => call<Sentence[]>("add_sentences", { texts, targetLanguage, translationComment: translationComment?.trim() || null, topic: topic?.trim() || null }),
      () => webApi.addSentences(texts, targetLanguage, translationComment, topic),
    ),
  sentenceDetails: (id: EntityId) =>
    select(() => call<SentenceDetails>("sentence_details", { id }), () => webApi.sentenceDetails(id)),
  saveManualTranslation: (sentenceId: EntityId, translation: ManualTranslation) =>
    select(() => call<void>("save_manual_translation", { sentenceId, translation }), () => webApi.saveManualTranslation(sentenceId, translation)),
  saveAudio: (sentenceId: EntityId, targetLanguage: string, fileName: string, mimeType: string, bytes: number[]) =>
    select(() => call<void>("save_sentence_audio", { sentenceId, targetLanguage, fileName, mimeType, bytes }), () => webApi.saveAudio(sentenceId, targetLanguage, fileName, mimeType, bytes)),
  deleteAudio: (sentenceId: EntityId, targetLanguage: string) =>
    select(() => call<void>("delete_sentence_audio", { sentenceId, targetLanguage }), () => webApi.deleteAudio(sentenceId, targetLanguage)),
  sentenceAudio: (sentenceId: EntityId, targetLanguage: string) =>
    select(() => call<{ mimeType: string; bytes: number[] }>("sentence_audio", { sentenceId, targetLanguage }), () => webApi.sentenceAudio(sentenceId, targetLanguage)),
  generateAudio: (sentenceId: EntityId, targetLanguage: string) =>
    select(() => call<void>("generate_sentence_audio", { sentenceId, targetLanguage }), () => webApi.generateAudio(sentenceId, targetLanguage) as Promise<void>),
  verifyElevenLabsKey: (apiKey: string) =>
    select(() => call<ElevenLabsVoice[]>("verify_elevenlabs_key", { apiKey }), () => webApi.verifyElevenLabsKey(apiKey)),
  saveElevenLabsKey: (apiKey: string) =>
    select(() => call<Settings>("save_elevenlabs_key", { apiKey }), () => webApi.saveElevenLabsKey(apiKey)),
  deleteElevenLabsKey: () =>
    select(() => call<Settings>("delete_elevenlabs_key"), () => webApi.deleteElevenLabsKey()),
  elevenLabsVoices: () =>
    select(() => call<ElevenLabsVoice[]>("list_elevenlabs_voices"), () => webApi.elevenLabsVoices()),
  saveElevenLabsVoice: (voiceId: string, voiceName: string) =>
    select(() => call<Settings>("save_elevenlabs_voice", { voiceId, voiceName }), () => webApi.saveElevenLabsVoice(voiceId, voiceName)),
  deleteSentences: (ids: EntityId[]) =>
    select(() => call<void>("delete_sentences", { ids }), () => webApi.deleteSentences(ids)),
  prepare: (ids?: EntityId[], targetLanguage?: string, translationComment?: string, topic?: string) =>
    select(
      () => call<void>("prepare_sentences", { ids: ids ?? null, targetLanguage: targetLanguage ?? null, translationComment: translationComment?.trim() || null, topic: topic?.trim() || null }),
      () => webApi.prepare(ids, targetLanguage, translationComment, topic),
    ),
  settings: () => select(() => call<Settings>("get_settings"), () => webApi.settings()),
  saveSettings: (model: string) =>
    select(() => call<Settings>("save_settings", { model }), () => webApi.saveSettings(model)),
  listModels: () => select(() => call<string[]>("list_available_models"), () => webApi.listModels()),
  saveKey: (apiKey: string) =>
    select(() => call<Settings>("save_api_key", { apiKey }), () => webApi.saveKey(apiKey)),
  deleteKey: () => select(() => call<Settings>("delete_api_key"), () => webApi.deleteKey()),
  verifyKey: (apiKey: string) =>
    select(() => call<string[]>("verify_api_key", { apiKey }), () => webApi.verifyKey(apiKey)),
  topics: () => select(() => call<string[]>("list_topics"), () => webApi.topics()),
  exerciseLanguages: () =>
    select(() => call<string[]>("exercise_languages"), () => webApi.exerciseLanguages()),
  exerciseTopics: (targetLanguage: string) =>
    select(() => call<string[]>("exercise_topics", { targetLanguage }), () => webApi.exerciseTopics(targetLanguage)),
  nextExercise: (lastId?: EntityId, targetLanguage?: string, topic?: string, shuffle = false) =>
    select(
      () => call<Exercise | null>("next_exercise", { lastId: lastId ?? null, targetLanguage: targetLanguage ?? null, topic: topic ?? null, shuffle }),
      () => webApi.nextExercise(lastId, targetLanguage, topic, shuffle),
    ),
  onProgress: (handler: (progress: Progress) => void): Promise<UnlistenFn> =>
    isDesktop()
      ? listen<Progress>("preparation-progress", event => handler(event.payload))
      : webApi.onProgress(handler),
  syncStatus: () => call<SyncStatus>("sync_status"),
  connectSync: (serverUrl: string, email: string, password: string, deviceName: string) =>
    call<SyncStatus>("connect_sync_account", { serverUrl, email, password, deviceName }),
  syncNow: () => call<SyncStatus>("sync_now"),
  disconnectSync: () => call<SyncStatus>("disconnect_sync_account"),
};
