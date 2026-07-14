import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Exercise, Progress, Sentence, Settings } from "./types";
const call = <T>(command:string,args:Record<string,unknown>={}) => invoke<T>(command,args);
export const api = {
  listSentences:()=>call<Sentence[]>("list_sentences"), addSentences:(texts:string[])=>call<Sentence[]>("add_sentences",{texts}),
  deleteSentences:(ids:number[])=>call<void>("delete_sentences",{ids}), prepare:(ids?:number[])=>call<void>("prepare_sentences",{ids:ids??null}),
  settings:()=>call<Settings>("get_settings"), saveSettings:(model:string,targetLanguage:string)=>call<Settings>("save_settings",{model,targetLanguage}),
  saveKey:(apiKey:string)=>call<Settings>("save_api_key",{apiKey}), deleteKey:()=>call<Settings>("delete_api_key"), verifyKey:(apiKey:string)=>call<string[]>("verify_api_key",{apiKey}),
  nextExercise:(lastId?:number)=>call<Exercise|null>("next_exercise",{lastId:lastId??null}),
  onProgress:(handler:(p:Progress)=>void):Promise<UnlistenFn>=>listen<Progress>("preparation-progress",e=>handler(e.payload))
};
