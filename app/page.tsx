"use client";
import { useState } from "react";
import { BookOpenCheck, ListPlus, Settings as SettingsIcon } from "lucide-react";
import { ExerciseView } from "@/components/ExerciseView";
import { SentencesView } from "@/components/SentencesView";
import { SettingsView } from "@/components/SettingsView";
type Tab="exercise"|"sentences"|"settings";
export default function Home(){ const [tab,setTab]=useState<Tab>("exercise"); const items=[['exercise','Упражнения',BookOpenCheck],['sentences','Предложения',ListPlus],['settings','Настройки',SettingsIcon]] as const; return <main className="shell"><aside><div className="brand"><span>LA</span><div>LangAI<small>переводы в ритме</small></div></div><nav>{items.map(([id,label,Icon])=><button key={id} className={tab===id?'active':''} onClick={()=>setTab(id)}><Icon size={19}/>{label}</button>)}</nav><div className="offline">● Локальное приложение</div></aside><section className="content">{tab==='exercise'?<ExerciseView/>:tab==='sentences'?<SentencesView/>:<SettingsView/>}</section></main> }
