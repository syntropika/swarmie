import { Disclosure } from "@/components/ui/disclosure";
import { useState } from "react";
import { ShieldCheck, MessageCircleQuestion } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Checkbox } from "@/components/ui/checkbox";
import { RadioGroup, RadioOption } from "@/components/ui/radio-group";
import { questionAnswers } from "./questions";
import type { Permission } from "./protocol";

export function RequestCard({
  permission,
  onAnswer,
}: {
  permission: Permission;
  onAnswer: (option: string, data: unknown) => Promise<unknown>;
}) {
  const questions = permission.metadata?.questions ?? [];
  const [answers, setAnswers] = useState<Record<string, string | string[]>>({});
  const [written, setWritten] = useState<Record<string, string>>({});
  const [json, setJson] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const answer = async (option: string) => {
    setBusy(true);
    setError("");
    try {
      await onAnswer(
        option,
        questions.length
          ? { answers: questionAnswers(questions, answers, written) }
          : json.trim()
            ? JSON.parse(json)
            : null,
      );
    } catch (e) {
      setError(
        e instanceof Error
          ? e.message
          : "Enter valid JSON or leave the answer empty.",
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <section
      className="permission-card"
      aria-label={questions.length ? "Agent question" : "Permission request"}
    >
      <div className="permission-title">
        {questions.length ? (
          <MessageCircleQuestion size={16} />
        ) : (
          <ShieldCheck size={16} />
        )}
        <strong>{permission.title}</strong>
      </div>
      {permission.description && <p>{permission.description}</p>}
      {questions.map((q) => (
        <fieldset className="question-field" key={q.id}>
          <legend>{q.prompt}</legend>
          {q.multiple ? q.options.map((option) => (
              <Checkbox
                key={option.id}
                label={option.label}
                className="question-option"
                disabled={busy}
                checked={Array.isArray(answers[q.id]) && answers[q.id].includes(option.id)}
                onChange={(checked) =>
                  setAnswers((previous) => ({
                    ...previous,
                    [q.id]: checked
                        ? [
                            ...(Array.isArray(previous[q.id])
                              ? (previous[q.id] as string[])
                              : []),
                            option.id,
                          ]
                        : (Array.isArray(previous[q.id])
                            ? (previous[q.id] as string[])
                            : []
                          ).filter((v) => v !== option.id),
                  }))
                }
              />
          )) : (
            <RadioGroup aria-label={q.prompt} name={`${permission.id}-${q.id}`} disabled={busy}
              value={typeof answers[q.id] === "string" ? String(answers[q.id]) : ""}
              onValueChange={(value) => setAnswers((previous) => ({ ...previous, [q.id]: value }))} className="question-radio-group">
              {q.options.map((option) => <RadioOption value={option.id} key={option.id} label={option.label} />)}
            </RadioGroup>
          )}
          {q.allowText && (
            <Input
              type={q.secret ? "password" : "text"}
              autoComplete={q.secret ? "off" : undefined}
              disabled={busy}
              aria-label={`${q.prompt} written answer`}
              placeholder="Your answer"
              value={
                q.multiple
                  ? (written[q.id] ?? "")
                  : typeof answers[q.id] === "string" &&
                      !q.options.some((o) => o.id === answers[q.id])
                    ? String(answers[q.id])
                    : ""
              }
              onChange={(e) => {
                setWritten({ ...written, [q.id]: e.target.value });
                if (!q.multiple)
                  setAnswers({ ...answers, [q.id]: e.target.value });
              }}
            />
          )}
        </fieldset>
      ))}
      {!questions.length && (
        <Disclosure>
          <Disclosure.Summary>Additional answer data</Disclosure.Summary>
          <Textarea
            aria-label="Structured permission answer"
            placeholder="Optional JSON answer"
            value={json}
            onChange={(e) => setJson(e.target.value)}
          />
        </Disclosure>
      )}
      {error && <p role="alert">{error}</p>}
      <div className="permission-actions">
        {permission.options.map((option) => (
          <Button
            variant="ghost"
            className="small-button"
            key={option.id}
            disabled={busy}
            onClick={() => void answer(option.id)}
          >
            {busy ? "Sending…" : option.label}
          </Button>
        ))}
      </div>
    </section>
  );
}
