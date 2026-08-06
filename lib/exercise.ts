export function matchesExpected(selected: string, expected: string): boolean {
  return selected.trim() === expected.trim();
}

export function wrongAnswerPositions(answers: Record<number, string>, expected: string[]): number[] {
  return expected.flatMap((correct, position) =>
    matchesExpected(answers[position] ?? "", correct) ? [] : [position]
  );
}

export function availableBlockPositions(total: number, used: number[], limit = 2): number[] {
  const positions = Array.from({ length: total }, (_, position) => position);
  return [0, 1]
    .map(column => positions.find(position => position % 2 === column && !used.includes(position)))
    .filter((position): position is number => position !== undefined)
    .slice(0, limit);
}

export function attemptHasErrors(answers: Record<number, string>, expected: string[]): boolean {
  return wrongAnswerPositions(answers, expected).length > 0;
}
