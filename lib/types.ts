export type Status = "unprepared"|"queued"|"generating"|"ready"|"failed";
export type EntityId = number | string;
export interface SentenceLanguage { targetLanguage:string; status:Status; error?:string|null }
export interface Sentence { id:EntityId; sourceText:string; languages:SentenceLanguage[]; topics:string[]; status:Status; error?:string|null; createdAt:string }
export interface Option { id:EntityId; text:string; isCorrect:boolean }
export interface Block { id:EntityId; position:number; correct:string; prefix:string; suffix:string; hint?:string|null; options:Option[] }
export interface Exercise { sentenceId:EntityId; sourceText:string; targetLanguage:string; translation:string; audioAvailable:boolean; blocks:Block[] }
export interface Settings { apiKeyConfigured:boolean; elevenlabsKeyConfigured:boolean; model:string; targetLanguage:string; elevenlabsVoiceId?:string|null; elevenlabsVoiceName?:string|null }
export interface ElevenLabsVoice { voiceId:string; name:string }
export interface Progress { sentenceId:EntityId; status:Status; completed:number; total:number; error?:string }
export interface ManualBlock { correct:string; distractors:string[]; hint?:string|null }
export interface ManualTranslation { targetLanguage:string; translation:string; blocks:ManualBlock[]; audioName?:string|null; audioMime?:string|null }
export interface SentenceDetails { id:EntityId; sourceText:string; topics:string[]; translations:ManualTranslation[] }
export interface SyncStatus {
  connected:boolean;
  serverUrl?:string|null;
  userId?:string|null;
  deviceId?:string|null;
  status:string;
  pendingOperations:number;
  lastError?:string|null;
  lastSyncAt?:string|null;
  initialUploadCompleted:boolean;
}
