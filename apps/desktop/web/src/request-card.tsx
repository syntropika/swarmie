import { ShieldCheck, MessageCircleQuestion } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Disclosure } from "@/components/ui/disclosure";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioOption } from "@/components/ui/radio-group";
import { Textarea } from "@/components/ui/textarea";

import { startAction, errorReporter } from "./action-boundary";
import type { Permission } from "./protocol";
import { questionAnswers } from "./questions";
import { hasText } from "./value-presence";

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
  const [issue, setIssue] = useState("");
  const [busy, setBusy] = useState(false);
  const answer = async (option: string) => {
    setBusy(true);
    setIssue("");
    try {
      await onAnswer(
        option,
        (() => {
          if (questions.length) {
            return { answers: questionAnswers(questions, answers, written) };
          }
          if (json.trim()) {
            const value: unknown = JSON.parse(json);
            return value;
          }
          return null;
        })(),
      );
    } catch (error) {
      setIssue(
        error instanceof Error ? error.message : "Enter valid JSON or leave the answer empty.",
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
        {questions.length ? <MessageCircleQuestion size={16} /> : <ShieldCheck size={16} />}
        <strong>{permission.title}</strong>
      </div>
      {hasText(permission.description) && <p>{permission.description}</p>}
      {questions.map((q) => (
        <fieldset className="question-field" key={q.id}>
          <legend>{q.prompt}</legend>
          {q.multiple === true ? (
            q.options.map((option) => (
              <Checkbox
                key={option.id}
                label={option.label}
                disabled={busy}
                checked={Array.isArray(answers[q.id]) && answers[q.id].includes(option.id)}
                onChange={(checked) => {
                  setAnswers((previous) => ({
                    ...previous,
                    [q.id]: checked
                      ? [...multipleAnswer(previous[q.id]), option.id]
                      : multipleAnswer(previous[q.id]).filter((v) => v !== option.id),
                  }));
                }}
              />
            ))
          ) : (
            <RadioGroup
              aria-label={q.prompt}
              name={`${permission.id}-${q.id}`}
              disabled={busy}
              value={singleAnswer(answers[q.id])}
              onValueChange={(value) => {
                setAnswers((previous) => ({ ...previous, [q.id]: value }));
              }}
            >
              {q.options.map((option) => (
                <RadioOption value={option.id} key={option.id} label={option.label} />
              ))}
            </RadioGroup>
          )}
          {q.allowText === true && (
            <Input
              type={q.secret === true ? "password" : "text"}
              autoComplete={q.secret === true ? "off" : undefined}
              disabled={busy}
              aria-label={`${q.prompt} written answer`}
              placeholder="Your answer"
              value={(() => {
                if (q.multiple === true) {
                  return written[q.id] ?? "";
                }
                if (
                  typeof answers[q.id] === "string" &&
                  !q.options.some((o) => o.id === answers[q.id])
                ) {
                  return singleAnswer(answers[q.id]);
                }
                return "";
              })()}
              onChange={(e) => {
                setWritten({ ...written, [q.id]: e.target.value });
                if (!(q.multiple === true)) {
                  setAnswers({ ...answers, [q.id]: e.target.value });
                }
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
            onChange={(e) => {
              setJson(e.target.value);
            }}
          />
        </Disclosure>
      )}
      {hasText(issue) && <p role="alert">{issue}</p>}
      <div className="permission-actions">
        {permission.options.map((option) => (
          <Button
            variant="ghost"
            size="sm"
            key={option.id}
            disabled={busy}
            onClick={() => {
              startAction(answer(option.id), errorReporter(setIssue));
            }}
          >
            {busy ? "Sending…" : option.label}
          </Button>
        ))}
      </div>
    </section>
  );
}

function multipleAnswer(value: string | string[] | undefined): string[] {
  return Array.isArray(value) ? value : [];
}
function singleAnswer(value: string | string[] | undefined): string {
  return typeof value === "string" ? value : "";
}
