"use client";

import { useCallback, useEffect, useReducer, useState } from "react";
import { Lightbulb, RotateCcw, Shuffle, Volume2 } from "lucide-react";
import { api } from "@/lib/tauri";
import type { Exercise, Option } from "@/lib/types";
import { attemptHasErrors, availableBlockPositions, wrongAnswerPositions } from "@/lib/exercise";

type State = { answers: Record<number, string>; usedBlocks: number[]; wrongPositions: number[]; hintVisible: boolean };
type Action =
  | { type: "answer"; text: string; sourcePosition: number; total: number; expected: string[] }
  | { type: "clearWrong" }
  | { type: "hint" }
  | { type: "restore"; state: State }
  | { type: "reset" };

const initial: State = { answers: {}, usedBlocks: [], wrongPositions: [], hintVisible: false };

function reducer(state: State, action: Action): State {
  if (action.type === "reset") return initial;
  if (action.type === "restore") return action.state;
  if (action.type === "clearWrong") {
    return initial;
  }
  if (action.type === "hint") return { ...state, hintVisible: !state.hintVisible };
  if (state.answers[action.sourcePosition] !== undefined) return state;
  const answers = { ...state.answers, [action.sourcePosition]: action.text };
  const usedBlocks = [...state.usedBlocks, action.sourcePosition];
  const filled = Object.keys(answers).length === action.total;
  return {
    answers,
    usedBlocks,
    wrongPositions: filled ? wrongAnswerPositions(answers, action.expected) : [],
    hintVisible: false,
  };
}

function shuffled(options: Option[]) {
  const result = [...options];
  for (let index = result.length - 1; index > 0; index--) {
    const target = Math.floor(Math.random() * (index + 1));
    [result[index], result[target]] = [result[target], result[index]];
  }
  return result;
}

export function ExerciseView() {
  const [exercise, setExercise] = useState<Exercise | null>(null);
  const [order, setOrder] = useState<Record<number, Option[]>>({});
  const [languages, setLanguages] = useState<string[]>([]);
  const [language, setLanguage] = useState("");
  const [topics, setTopics] = useState<string[]>([]);
  const [topic, setTopic] = useState("");
  const [shuffle, setShuffle] = useState(false);
  const [beforeShuffle, setBeforeShuffle] = useState<{ exercise: Exercise; state: State; order: Record<number, Option[]> } | null>(null);
  const [state, dispatch] = useReducer(reducer, initial);
  const [error, setError] = useState("");
  const [audioBusy, setAudioBusy] = useState(false);

  const load = useCallback(async (selectedLanguage = language, lastId = exercise?.sentenceId, selectedTopic = topic, randomOrder = shuffle) => {
    try {
      if (!selectedLanguage) return;
      const next = await api.nextExercise(lastId, selectedLanguage, selectedTopic || undefined, randomOrder);
      setExercise(next);
      dispatch({ type: "reset" });
      if (next) {
        setOrder(Object.fromEntries(next.blocks.map(block => [block.position, shuffled(block.options)])));
      }
    } catch (reason) {
      setError(String(reason));
    }
  }, [exercise?.sentenceId, language, topic, shuffle]);

  async function changeLanguage(value: string) {
    setLanguage(value);
    setTopic("");
    setShuffle(false);
    setBeforeShuffle(null);
    try { setTopics(await api.exerciseTopics(value)); } catch (reason) { setError(String(reason)); }
    await load(value, undefined, "");
  }

  function changeTopic(value: string) {
    setTopic(value);
    setShuffle(false);
    setBeforeShuffle(null);
    void load(language, undefined, value, false);
  }

  function toggleShuffle() {
    if (!shuffle) {
      if (exercise) setBeforeShuffle({ exercise, state, order });
      setShuffle(true);
      void load(language, undefined, topic, true);
      return;
    }
    setShuffle(false);
    if (beforeShuffle) {
      setExercise(beforeShuffle.exercise);
      setOrder(beforeShuffle.order);
      dispatch({ type: "restore", state: beforeShuffle.state });
      setBeforeShuffle(null);
    } else {
      void load(language, undefined, topic, false);
    }
  }

  useEffect(() => {
    api.exerciseLanguages().then(available => {
      setLanguages(available);
      const first = available[0] ?? "";
      setLanguage(first);
      if (first) {
        api.exerciseTopics(first).then(setTopics).catch(reason => setError(String(reason)));
        void load(first, undefined, "");
      }
    }).catch(reason => setError(String(reason)));
  }, []);

  if (error) return <div className="error">{error}</div>;
  if (!exercise) {
    return <div className="practiceEmpty"><label><span>Язык практики</span><select value={language} disabled={!languages.length} onChange={event => void changeLanguage(event.target.value)}>{languages.map(item => <option value={item} key={item}>{item}</option>)}</select></label><label><span>Тема</span><select value={topic} onChange={event => changeTopic(event.target.value)}><option value="">Все темы</option>{topics.map(item => <option value={item} key={item}>{item}</option>)}</select></label><div className="empty heroEmpty">{languages.length ? `Для выбранных языка и темы нет доступных предложений.` : "Нет подготовленных предложений. Подготовьте их в разделе «Предложения»."}</div></div>;
  }

  const lesson = exercise;
  const solved = Object.keys(state.answers).length;
  const complete = solved === lesson.blocks.length && state.wrongPositions.length === 0;
  const availablePositions = availableBlockPositions(lesson.blocks.length, state.usedBlocks);
  const active = availablePositions.map(position => lesson.blocks[position]);
  const currentHint = active[0];

  function choose(option: Option, sourcePosition: number) {
    const isLastEmpty = solved === lesson.blocks.length - 1;
    const expected = lesson.blocks.map(block => block.correct);
    const completedAnswers = { ...state.answers, [sourcePosition]: option.text };
    const shouldReset = isLastEmpty && attemptHasErrors(completedAnswers, expected);
    dispatch({
      type: "answer",
      text: option.text,
      sourcePosition,
      total: lesson.blocks.length,
      expected,
    });
    if (shouldReset) setTimeout(() => dispatch({ type: "clearWrong" }), 650);
    if (isLastEmpty && !shouldReset && lesson.audioAvailable) {
      void playAnswerAudio();
    }
  }

  async function playAnswerAudio() {
    if (!exercise) return;
    setAudioBusy(true);
    try {
      const audio = await api.sentenceAudio(exercise.sentenceId, exercise.targetLanguage);
      const url = URL.createObjectURL(new Blob([new Uint8Array(audio.bytes)], { type: audio.mimeType }));
      const player = new Audio(url);
      player.addEventListener("ended", () => URL.revokeObjectURL(url), { once:true });
      player.addEventListener("error", () => URL.revokeObjectURL(url), { once:true });
      await player.play();
    } catch (reason) { setError(String(reason)); }
    finally { setAudioBusy(false); }
  }

  return <div className="lesson">
    <div className="lessonTop">
      <div><span className="lessonKicker">Упражнение</span><strong>Соберите перевод</strong><select className="practiceLanguage" value={language} onChange={event => void changeLanguage(event.target.value)}>{languages.map(item => <option value={item} key={item}>{item}</option>)}</select><select className="practiceLanguage" value={topic} onChange={event => changeTopic(event.target.value)}><option value="">Все темы</option>{topics.map(item => <option value={item} key={item}>{item}</option>)}</select><button className={`practiceShuffle ${shuffle ? "active" : ""}`} onClick={toggleShuffle}><Shuffle size={17}/>Перемешать</button></div>
      <div className="lessonProgress">{solved} / {exercise.blocks.length}</div>
    </div>

    <div className="lessonSource">{exercise.sourceText}</div>
    <div className={`answerLine ${state.wrongPositions.length ? "answerWrong" : ""}`}>
      {exercise.blocks.map((block, index) =>
        <span key={block.id} className={`answerToken ${state.wrongPositions.includes(index) ? "slotWrong" : ""}`}>
          {block.prefix && <i className="fixedPunctuation">{block.prefix}</i>}
          <i className={state.answers[index] !== undefined ? "filled" : "blank"}>{state.answers[index] ?? ""}</i>
          {block.suffix && <i className="fixedPunctuation">{block.suffix}</i>}
        </span>
      )}
    </div>

    {!complete && <div className="choiceArea">
      {state.hintVisible && currentHint && <div className="lessonHint">
        {currentHint.hint || `Начинается с «${currentHint.correct.slice(0, 1)}», ${currentHint.correct.length} символов.`}
      </div>}
      <div className="choiceColumns">
        {active.map(block => <div className={`choiceColumn ${block.position % 2 === 0 ? "choiceColumnLeft" : "choiceColumnRight"}`} key={block.id}>
          {order[block.position]?.map(option => <button
            key={option.id}
            className="wordChoice"
            onClick={() => choose(option, block.position)}
          >{option.text}</button>)}
        </div>)}
      </div>
    </div>}

    {complete && <div className="lessonComplete">
      <span>Перевод собран</span>
      <h2>{exercise.translation}</h2>
      {exercise.audioAvailable && <button className="listenAnswer" disabled={audioBusy} onClick={() => void playAnswerAudio()}><Volume2/>{audioBusy ? "Загрузка…" : "Прослушать"}</button>}
      <button className="lessonNext" onClick={() => void load(language, exercise.sentenceId, topic)}>Следующее предложение</button>
    </div>}

    <div className="lessonActions">
      <button onClick={() => dispatch({ type: "reset" })}><RotateCcw/><span>Начать заново</span></button>
      <button onClick={() => dispatch({ type: "hint" })} disabled={complete}><Lightbulb/><span>Подсказка</span></button>
    </div>
  </div>;
}
