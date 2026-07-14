import { describe, expect, it } from "vitest";
import { attemptHasErrors, availableBlockPositions, matchesExpected, wrongAnswerPositions } from "./exercise";

describe("first empty translation slot", () => {
  it("accepts the expected option regardless of which visible group supplied it", () => {
    expect(matchesExpected("früh", "früh")).toBe(true);
  });

  it("rejects an option for a later slot until the first slot is filled", () => {
    expect(matchesExpected("eingeschlafen", "früh")).toBe(false);
  });

  it("ignores surrounding whitespace returned by a model", () => {
    expect(matchesExpected("  My name ", "My name")).toBe(true);
  });
});

describe("deferred attempt validation", () => {
  it("identifies an error that causes the entire attempt to reset", () => {
    const answers = { 0: "Ich", 1: "habe", 2: "ein", 3: "Hund" };
    expect(wrongAnswerPositions(answers, ["Ich", "habe", "einen", "Hund"])).toEqual([2]);
  });
});

describe("single-use option groups", () => {
  it("does not display a group again after an option was selected from it", () => {
    expect(availableBlockPositions(4, [1])).toEqual([0, 2]);
  });
});

describe("completed attempt", () => {
  it("does not schedule a reset for a completely correct translation", () => {
    expect(attemptHasErrors({ 0: "Ich", 1: "bin", 2: "hier" }, ["Ich", "bin", "hier"])).toBe(false);
  });

  it("does schedule a reset when any position is incorrect", () => {
    expect(attemptHasErrors({ 0: "Ich", 1: "war", 2: "hier" }, ["Ich", "bin", "hier"])).toBe(true);
  });
});
