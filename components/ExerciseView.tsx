"use client";

import { useCallback, useEffect, useReducer, useState } from "react";
import { Lightbulb, RotateCcw } from "lucide-react";
import { api } from "@/lib/tauri";
import type { Exercise, Option } from "@/lib/types";
import { attemptHasErrors, availableBlockPositions, wrongAnswerPositions } from "@/lib/exercise";

type State = { answers: Record<number, string>; usedBlocks: number[]; wrongPositions: number[]; hintVisible: boolean };
type Action =
  | { type: "answer"; text: string; sourcePosition: number; total: number; expected: string[] }
  | { type: "clearWrong" }
  | { type: "hint" }
  | { type: "reset" };

const initial: State = { answers: {}, usedBlocks: [], wrongPositions: [], hintVisible: false };

function reducer(state: State, action: Action): State {
  if (action.type === "reset") return initial;
  if (action.type === "clearWrong") {
    return initial;
  }
  if (action.type === "hint") return { ...state, hintVisible: !state.hintVisible };
  const firstEmpty = Array.from({ length: action.total }, (_, position) => position)
    .find(position => state.answers[position] === undefined);
  if (firstEmpty === undefined) return state;
  const answers = { ...state.answers, [firstEmpty]: action.text };
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
  const [state, dispatch] = useReducer(reducer, initial);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    try {
      const next = await api.nextExercise(exercise?.sentenceId);
      setExercise(next);
      dispatch({ type: "reset" });
      if (next) {
        setOrder(Object.fromEntries(next.blocks.map(block => [block.position, shuffled(block.options)])));
      }
    } catch (reason) {
      setError(String(reason));
    }
  }, [exercise?.sentenceId]);

  useEffect(() => { void load(); }, []);

  if (error) return <div className="error">{error}</div>;
  if (!exercise) {
    return <div className="empty heroEmpty">Нет подготовленных предложений. Подготовьте их в разделе «Предложения».</div>;
  }

  const lesson = exercise;
  const solved = Object.keys(state.answers).length;
  const complete = solved === lesson.blocks.length && state.wrongPositions.length === 0;
  const availablePositions = availableBlockPositions(lesson.blocks.length, state.usedBlocks);
  const active = availablePositions.map(position => lesson.blocks[position]);
  const currentHint = active[0];

  function choose(option: Option, sourcePosition: number) {
    const isLastEmpty = solved === lesson.blocks.length - 1;
    const firstEmpty = Array.from({ length: lesson.blocks.length }, (_, position) => position)
      .find(position => state.answers[position] === undefined);
    const expected = lesson.blocks.map(block => block.correct);
    const completedAnswers = firstEmpty === undefined
      ? state.answers
      : { ...state.answers, [firstEmpty]: option.text };
    const shouldReset = isLastEmpty && attemptHasErrors(completedAnswers, expected);
    dispatch({
      type: "answer",
      text: option.text,
      sourcePosition,
      total: lesson.blocks.length,
      expected,
    });
    if (shouldReset) setTimeout(() => dispatch({ type: "clearWrong" }), 650);
  }

  return <div className="lesson">
    <div className="lessonTop">
      <div><span className="lessonKicker">Упражнение</span><strong>Соберите перевод</strong></div>
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
        {active.map(block => <div className="choiceColumn" key={block.id}>
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
      <button className="lessonNext" onClick={() => void load()}>Следующее предложение</button>
    </div>}

    <div className="lessonActions">
      <button onClick={() => dispatch({ type: "reset" })}><RotateCcw/><span>Начать заново</span></button>
      <button onClick={() => dispatch({ type: "hint" })} disabled={complete}><Lightbulb/><span>Подсказка</span></button>
    </div>
  </div>;
}
