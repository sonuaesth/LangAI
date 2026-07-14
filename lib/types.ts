export type Status = "unprepared"|"queued"|"generating"|"ready"|"failed";
export interface SentenceLanguage { targetLanguage:string; status:Status; error?:string|null }
export interface Sentence { id:number; sourceText:string; languages:SentenceLanguage[]; status:Status; error?:string|null; createdAt:string }
export interface Option { id:number; text:string; isCorrect:boolean }
export interface Block { id:number; position:number; correct:string; prefix:string; suffix:string; hint?:string|null; options:Option[] }
export interface Exercise { sentenceId:number; sourceText:string; translation:string; blocks:Block[] }
export interface Settings { apiKeyConfigured:boolean; model:string; targetLanguage:string }
export interface Progress { sentenceId:number; status:Status; completed:number; total:number; error?:string }
