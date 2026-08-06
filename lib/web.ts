import type {
  ElevenLabsVoice,
  EntityId,
  Exercise,
  ManualTranslation,
  Sentence,
  SentenceDetails,
  Settings,
  Status,
} from "./types";

type ServerPreparation = {
  id: string;
  model: string;
  translation: string;
  blocks: Array<{
    id: string;
    position: number;
    correct: string;
    hint?: string | null;
    options: Array<{ id: string; text: string; isCorrect: boolean }>;
  }>;
};

type ServerSentence = {
  id: string;
  sourceText: string;
  createdAt: string;
  topics: string[];
  languages: Array<{
    targetLanguage: string;
    status: Status;
    error?: string | null;
    audioAvailable: boolean;
    activePreparation?: ServerPreparation | null;
  }>;
};

function cookie(name: string) {
  if (typeof document === "undefined") return undefined;
  return document.cookie
    .split(";")
    .map(part => part.trim().split("="))
    .find(([key]) => key === name)?.[1];
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body && !(init.body instanceof Uint8Array) && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  if (init.method && !["GET", "HEAD"].includes(init.method.toUpperCase())) {
    const csrf = cookie("langai_csrf");
    if (csrf) headers.set("x-csrf-token", csrf);
  }
  const response = await fetch(path, { ...init, headers, credentials: "include" });
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.error?.message ?? `HTTP ${response.status}`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

function summary(sentence: ServerSentence, targetLanguage?: string): Sentence {
  const selected = targetLanguage
    ? sentence.languages.find(language => language.targetLanguage === targetLanguage)
    : sentence.languages[0];
  return {
    id: sentence.id,
    sourceText: sentence.sourceText,
    createdAt: sentence.createdAt,
    topics: sentence.topics,
    languages: sentence.languages.map(language => ({
      targetLanguage: language.targetLanguage,
      status: language.status,
      error: language.error,
    })),
    status: selected?.status ?? "unprepared",
    error: selected?.error,
  };
}

function details(sentence: ServerSentence): SentenceDetails {
  return {
    id: sentence.id,
    sourceText: sentence.sourceText,
    topics: sentence.topics,
    translations: sentence.languages.flatMap(language => {
      const preparation = language.activePreparation;
      if (!preparation) return [];
      return [{
        targetLanguage: language.targetLanguage,
        translation: preparation.translation,
        blocks: preparation.blocks.map(block => ({
          correct: block.correct,
          hint: block.hint,
          distractors: block.options.filter(option => !option.isCorrect).map(option => option.text),
        })),
        audioName: language.audioAvailable ? `${language.targetLanguage}.audio` : null,
        audioMime: language.audioAvailable ? "audio/mpeg" : null,
      }];
    }),
  };
}

function exercise(sentence: ServerSentence, targetLanguage: string): Exercise | null {
  const language = sentence.languages.find(item => item.targetLanguage === targetLanguage);
  const preparation = language?.activePreparation;
  if (!language || !preparation || language.status !== "ready") return null;
  return {
    sentenceId: sentence.id,
    sourceText: sentence.sourceText,
    targetLanguage,
    translation: preparation.translation,
    audioAvailable: language.audioAvailable,
    blocks: preparation.blocks.map(block => ({
      ...block,
      prefix: "",
      suffix: "",
    })),
  };
}

async function sentences(language?: string, topic?: string) {
  const query = new URLSearchParams();
  if (language) query.set("targetLanguage", language);
  if (topic) query.set("topic", topic);
  return request<ServerSentence[]>(`/api/v1/sentences?${query}`);
}

async function settings(): Promise<Settings> {
  const [value, keys] = await Promise.all([
    request<{
      model: string;
      targetLanguage: string;
      elevenlabsVoiceId?: string | null;
      elevenlabsVoiceName?: string | null;
    }>("/api/v1/settings"),
    request<Array<{ provider: string; configured: boolean }>>("/api/v1/provider-keys"),
  ]);
  return {
    model: value.model,
    targetLanguage: value.targetLanguage,
    elevenlabsVoiceId: value.elevenlabsVoiceId,
    elevenlabsVoiceName: value.elevenlabsVoiceName,
    apiKeyConfigured: keys.some(key => key.provider === "openai" && key.configured),
    elevenlabsKeyConfigured: keys.some(key => key.provider === "elevenlabs" && key.configured),
  };
}

export const webApi = {
  async listSentences(filterLanguage?: string, targetLanguage?: string, filterTopic?: string) {
    return (await sentences(filterLanguage, filterTopic)).map(sentence => summary(sentence, targetLanguage));
  },
  async addSentences(texts: string[], targetLanguage: string, _comment?: string, topic?: string) {
    for (const sourceText of texts) {
      await request("/api/v1/sentences", {
        method: "POST",
        body: JSON.stringify({
          sourceText,
          targetLanguages: [targetLanguage],
          topics: topic ? [topic] : [],
        }),
      });
    }
    return (await sentences()).map(sentence => summary(sentence, targetLanguage));
  },
  async sentenceDetails(id: EntityId) {
    return details(await request<ServerSentence>(`/api/v1/sentences/${id}`));
  },
  saveManualTranslation: (id: EntityId, translation: ManualTranslation) =>
    request<void>(`/api/v1/sentences/${id}/translations`, {
      method: "POST",
      body: JSON.stringify(translation),
    }),
  async saveAudio(id: EntityId, targetLanguage: string, fileName: string, mimeType: string, bytes: number[]) {
    const data = new Uint8Array(bytes);
    const digest = await crypto.subtle.digest("SHA-256", data);
    const hash = [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, "0")).join("");
    await request(`/api/v1/sentences/${id}/audio?targetLanguage=${encodeURIComponent(targetLanguage)}`, {
      method: "PUT",
      headers: { "content-type": mimeType, "x-content-sha256": hash, "x-file-name": fileName },
      body: data,
    });
  },
  deleteAudio: (id: EntityId, targetLanguage: string) =>
    request<void>(`/api/v1/sentences/${id}/audio?targetLanguage=${encodeURIComponent(targetLanguage)}`, { method: "DELETE" }),
  async sentenceAudio(id: EntityId, targetLanguage: string) {
    const response = await fetch(`/api/v1/sentences/${id}/audio?targetLanguage=${encodeURIComponent(targetLanguage)}`, { credentials: "include" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return { mimeType: response.headers.get("content-type") ?? "audio/mpeg", bytes: [...new Uint8Array(await response.arrayBuffer())] };
  },
  generateAudio: (id: EntityId, targetLanguage: string) =>
    request(`/api/v1/sentences/${id}/audio/generate`, { method: "POST", body: JSON.stringify({ targetLanguage }) }),
  verifyElevenLabsKey: (apiKey: string): Promise<ElevenLabsVoice[]> =>
    request("/api/v1/providers/elevenlabs/verify", { method: "POST", body: JSON.stringify({ apiKey }) }),
  async saveElevenLabsKey(apiKey: string) {
    await request("/api/v1/provider-keys/elevenlabs", { method: "PUT", body: JSON.stringify({ apiKey }) });
    return settings();
  },
  async deleteElevenLabsKey() {
    await request("/api/v1/provider-keys/elevenlabs", { method: "DELETE" });
    return settings();
  },
  elevenLabsVoices: (): Promise<ElevenLabsVoice[]> => request("/api/v1/providers/elevenlabs/voices"),
  async saveElevenLabsVoice(voiceId: string, voiceName: string) {
    const current = await settings();
    await request("/api/v1/settings", {
      method: "PUT",
      body: JSON.stringify({ model: current.model, targetLanguage: current.targetLanguage, elevenlabsVoiceId: voiceId, elevenlabsVoiceName: voiceName }),
    });
    return settings();
  },
  async deleteSentences(ids: EntityId[]) {
    await Promise.all(ids.map(id => request(`/api/v1/sentences/${id}`, { method: "DELETE" })));
  },
  async prepare(ids: EntityId[] | undefined, targetLanguage?: string, comment?: string, _topic?: string) {
    const selected = ids?.length ? ids : (await sentences(targetLanguage)).filter(sentence =>
      sentence.languages.some(language => language.targetLanguage === targetLanguage && ["unprepared", "failed"].includes(language.status)),
    ).map(sentence => sentence.id);
    const current = await settings();
    const autoGenerateAudio = current.elevenlabsKeyConfigured && Boolean(current.elevenlabsVoiceId);
    await Promise.all(selected.map(async id => {
      await request(`/api/v1/sentences/${id}/prepare`, {
        method: "POST",
        body: JSON.stringify({ targetLanguage, translationComment: comment || null }),
      });
      if (autoGenerateAudio) {
        await request(`/api/v1/sentences/${id}/audio/generate`, {
          method: "POST",
          body: JSON.stringify({ targetLanguage }),
        });
      }
    }));
  },
  settings,
  async saveSettings(model: string) {
    const current = await settings();
    await request("/api/v1/settings", {
      method: "PUT",
      body: JSON.stringify({ model, targetLanguage: current.targetLanguage, elevenlabsVoiceId: current.elevenlabsVoiceId, elevenlabsVoiceName: current.elevenlabsVoiceName }),
    });
    return settings();
  },
  listModels: (): Promise<string[]> => request("/api/v1/providers/openai/models"),
  async saveKey(apiKey: string) {
    await request("/api/v1/provider-keys/openai", { method: "PUT", body: JSON.stringify({ apiKey }) });
    return settings();
  },
  async deleteKey() {
    await request("/api/v1/provider-keys/openai", { method: "DELETE" });
    return settings();
  },
  verifyKey: (apiKey: string): Promise<string[]> =>
    request("/api/v1/providers/openai/verify", { method: "POST", body: JSON.stringify({ apiKey }) }),
  async topics() {
    return [...new Set((await sentences()).flatMap(sentence => sentence.topics))].sort();
  },
  async exerciseLanguages() {
    return [...new Set((await sentences()).flatMap(sentence =>
      sentence.languages.filter(language => language.status === "ready").map(language => language.targetLanguage),
    ))].sort();
  },
  async exerciseTopics(targetLanguage: string) {
    return [...new Set((await sentences(targetLanguage)).filter(sentence =>
      sentence.languages.some(language => language.targetLanguage === targetLanguage && language.status === "ready"),
    ).flatMap(sentence => sentence.topics))].sort();
  },
  async nextExercise(lastId?: EntityId, targetLanguage?: string, topic?: string, shuffle = false) {
    if (!targetLanguage) return null;
    const available = (await sentences(targetLanguage, topic)).filter(sentence => exercise(sentence, targetLanguage));
    let next: ServerSentence | undefined;
    if (shuffle) {
      const candidates = available.filter(sentence => sentence.id !== lastId);
      next = candidates[Math.floor(Math.random() * candidates.length)] ?? available[0];
    } else {
      const current = available.findIndex(sentence => sentence.id === lastId);
      next = available[current >= 0 ? (current + 1) % available.length : 0];
    }
    return next ? exercise(next, targetLanguage) : null;
  },
  onProgress: async (_handler: unknown) => () => undefined,
};

export const webAuth = {
  session: () => request<{ authenticated: boolean; user?: { id: string; email: string } }>("/api/v1/auth/session"),
  login: (email: string, password: string) =>
    request("/api/v1/auth/login", { method: "POST", body: JSON.stringify({ email, password }) }),
  register: (email: string, password: string, invite?: string) =>
    request("/api/v1/auth/register", { method: "POST", body: JSON.stringify({ email, password, invite: invite || null }) }),
  logout: () => request("/api/v1/auth/session", { method: "DELETE" }),
};
