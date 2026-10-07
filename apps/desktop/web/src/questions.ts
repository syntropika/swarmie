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
        question.multiple
          ? [
              ...(Array.isArray(choice) ? choice : []),
              ...(text.trim() ? [text] : []),
            ]
          : typeof choice === "string"
            ? choice
            : text,
      ];
    }),
  );
}
