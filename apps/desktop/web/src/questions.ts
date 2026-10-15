import type { Question } from "./protocol";

export function questionAnswers(
  questions: Question[],
  selected: Record<string, string | string[]>,
  written: Record<string, string>,
): Record<string, string | string[]> {
  return Object.fromEntries(
    questions.map((question) => {
      const choice = selected[question.id];
      const text = written[question.id] ?? "";
      return [
        question.id,
        (() => {
          if (question.multiple === true) {
            return [...(Array.isArray(choice) ? choice : []), ...(text.trim() ? [text] : [])];
          }
          if (typeof choice === "string") {
            return choice;
          }
          return text;
        })(),
      ];
    }),
  );
}
