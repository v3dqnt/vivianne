"use client";

import React, { useState } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import { AlertCircleIcon, Tick02Icon } from "@hugeicons/core-free-icons";
import { useAgent, ExtensionUIRequest, PermissionRequest, QuestionRequest } from "./AgentProvider";
import { getToolMeta } from "./ToolCards/toolMeta";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";

type RespondFn = (id: string, response: any) => void;
type DismissFn = (id: string) => void;

export const ExtensionUIHandler = () => {
  const {
    uiRequests,
    sendUiResponse,
    dismissUiRequest,
    permissionRequests,
    respondToPermission,
    questionRequests,
    answerQuestion,
  } = useAgent();

  const request = uiRequests[0];
  const permission = permissionRequests[0];
  const question = questionRequests[0];

  return (
    <>
      <Dialog open={!!request} onOpenChange={(open) => !open && request && dismissUiRequest(request.id)}>
        <DialogContent showCloseButton={false} className="max-w-[480px]">
          {request?.method === "confirm" && (
            <ConfirmDialog request={request} onRespond={sendUiResponse} onDismiss={dismissUiRequest} />
          )}
          {request?.method === "select" && (
            <SelectDialog request={request} onRespond={sendUiResponse} onDismiss={dismissUiRequest} />
          )}
          {request?.method === "input" && (
            <InputDialog request={request} onRespond={sendUiResponse} onDismiss={dismissUiRequest} />
          )}
          {request?.method === "editor" && (
            <EditorDialog request={request} onRespond={sendUiResponse} onDismiss={dismissUiRequest} />
          )}
        </DialogContent>
      </Dialog>

      {/* Tool-call approval (bypass mode off) */}
      <Dialog open={!!permission} onOpenChange={(open) => !open && permission && respondToPermission(permission.requestId, false)}>
        <DialogContent showCloseButton={false} className="max-w-[480px]">
          {permission && <PermissionDialog permission={permission} onRespond={respondToPermission} />}
        </DialogContent>
      </Dialog>

      {/* `question` tool — 1-4 multiple-choice questions */}
      <Dialog open={!!question} onOpenChange={(open) => !open && question && answerQuestion(question.requestId, question.questions.map(() => []))}>
        <DialogContent showCloseButton={false} className="max-w-[520px]">
          {question && <QuestionDialog question={question} onAnswer={answerQuestion} />}
        </DialogContent>
      </Dialog>
    </>
  );
};

const PermissionDialog = ({
  permission,
  onRespond,
}: {
  permission: PermissionRequest;
  onRespond: (requestId: string, approved: boolean) => void;
}) => {
  const meta = getToolMeta(permission.toolName);
  const argsPreview = permission.args ? JSON.stringify(permission.args, null, 2) : "";

  return (
    <>
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          <HugeiconsIcon icon={meta.icon} size={16} className={meta.accent} />
          Allow "{meta.label}"?
        </DialogTitle>
        <DialogDescription>
          {permission.reason
            ? `The agent ${permission.reason}. Bypass mode is off, so it needs your approval.`
            : "The agent wants to run this tool. Bypass mode is off, so it needs your approval."}
        </DialogDescription>
      </DialogHeader>
      {argsPreview && (
        <div className="px-6 pb-2">
          <pre className="bg-surface-subtle border border-hairline rounded-lg p-3 text-xs font-mono text-body max-h-40 overflow-y-auto whitespace-pre-wrap">
            {argsPreview}
          </pre>
        </div>
      )}
      <DialogFooter>
        <Button variant="outline" onClick={() => onRespond(permission.requestId, false)}>
          Deny
        </Button>
        <Button onClick={() => onRespond(permission.requestId, true)}>Allow</Button>
      </DialogFooter>
    </>
  );
};

const QuestionDialog = ({
  question,
  onAnswer,
}: {
  question: QuestionRequest;
  onAnswer: (requestId: string, answers: string[][]) => void;
}) => {
  const [selections, setSelections] = useState<string[][]>(() => question.questions.map(() => []));

  const toggleOption = (qIdx: number, label: string, multiSelect?: boolean) => {
    setSelections((prev) => {
      const next = [...prev];
      const current = next[qIdx] ?? [];
      if (multiSelect) {
        next[qIdx] = current.includes(label) ? current.filter((l) => l !== label) : [...current, label];
      } else {
        next[qIdx] = [label];
      }
      return next;
    });
  };

  const allAnswered = selections.every((s) => s.length > 0);

  return (
    <>
      <DialogHeader>
        <DialogTitle>Question{question.questions.length > 1 ? "s" : ""} from the agent</DialogTitle>
      </DialogHeader>
      <div className="p-4 space-y-5 max-h-[55vh] overflow-y-auto">
        {question.questions.map((q, qIdx) => (
          <div key={qIdx} className="space-y-2">
            <div className="flex items-center gap-2">
              <span className="text-[10px] font-bold uppercase tracking-wider text-mute bg-surface-soft border border-hairline rounded-full px-2 py-0.5">
                {q.header}
              </span>
            </div>
            <p className="text-sm font-sans text-ink font-medium">{q.question}</p>
            <div className="space-y-1">
              {q.options.map((opt, oIdx) => {
                const checked = (selections[qIdx] ?? []).includes(opt.label);
                return (
                  <label
                    key={oIdx}
                    className="flex items-start gap-3 p-2.5 rounded-lg hover:bg-surface-soft cursor-pointer border border-transparent hover:border-hairline transition-colors"
                  >
                    <input
                      type={q.multiSelect ? "checkbox" : "radio"}
                      name={`question-${qIdx}`}
                      checked={checked}
                      onChange={() => toggleOption(qIdx, opt.label, q.multiSelect)}
                      className="w-4 h-4 accent-ink mt-0.5"
                    />
                    <div className="min-w-0">
                      <div className="text-sm font-sans text-ink font-medium">{opt.label}</div>
                      {opt.description && <div className="text-xs text-body font-sans">{opt.description}</div>}
                    </div>
                  </label>
                );
              })}
            </div>
          </div>
        ))}
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={() => onAnswer(question.requestId, question.questions.map(() => []))}>
          Skip
        </Button>
        <Button disabled={!allAnswered} onClick={() => onAnswer(question.requestId, selections)}>
          Submit
        </Button>
      </DialogFooter>
    </>
  );
};

const ConfirmDialog = ({
  request,
  onRespond,
  onDismiss,
}: {
  request: ExtensionUIRequest;
  onRespond: RespondFn;
  onDismiss: DismissFn;
}) => {
  return (
    <>
      <DialogHeader>
        <DialogTitle>{request.title}</DialogTitle>
        {request.message && <DialogDescription>{request.message}</DialogDescription>}
      </DialogHeader>
      <DialogFooter>
        <Button variant="outline" onClick={() => onDismiss(request.id)}>
          Cancel
        </Button>
        <Button onClick={() => onRespond(request.id, { confirmed: true })}>Confirm</Button>
      </DialogFooter>
    </>
  );
};

const SelectDialog = ({
  request,
  onRespond,
  onDismiss,
}: {
  request: ExtensionUIRequest;
  onRespond: RespondFn;
  onDismiss: DismissFn;
}) => {
  const [selected, setSelected] = useState<string>("");

  return (
    <>
      <DialogHeader>
        <DialogTitle>{request.title}</DialogTitle>
      </DialogHeader>
      <div className="p-4 max-h-[50vh] overflow-y-auto space-y-1">
        {(request.options ?? []).map((opt, i) => (
          <label
            key={i}
            className="flex items-center gap-3 p-3 rounded-lg hover:bg-surface-soft cursor-pointer border border-transparent hover:border-hairline transition-colors"
          >
            <input
              type="radio"
              name="selectOption"
              value={opt}
              checked={selected === opt}
              onChange={(e) => setSelected(e.target.value)}
              className="w-4 h-4 accent-ink"
            />
            <span className="text-sm font-sans text-ink">{opt}</span>
          </label>
        ))}
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={() => onDismiss(request.id)}>
          Cancel
        </Button>
        <Button disabled={!selected} onClick={() => onRespond(request.id, { value: selected })}>
          Select
        </Button>
      </DialogFooter>
    </>
  );
};

const InputDialog = ({
  request,
  onRespond,
  onDismiss,
}: {
  request: ExtensionUIRequest;
  onRespond: RespondFn;
  onDismiss: DismissFn;
}) => {
  const [value, setValue] = useState("");

  return (
    <>
      <DialogHeader>
        <DialogTitle>{request.title}</DialogTitle>
      </DialogHeader>
      <div className="p-6">
        <Input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={request.placeholder || "Enter text..."}
          autoFocus
        />
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={() => onDismiss(request.id)}>
          Cancel
        </Button>
        <Button disabled={!value.trim()} onClick={() => onRespond(request.id, { value })}>
          Submit
        </Button>
      </DialogFooter>
    </>
  );
};

const EditorDialog = ({
  request,
  onRespond,
  onDismiss,
}: {
  request: ExtensionUIRequest;
  onRespond: RespondFn;
  onDismiss: DismissFn;
}) => {
  const [value, setValue] = useState(request.prefill || "");

  return (
    <>
      <DialogHeader>
        <DialogTitle>{request.title}</DialogTitle>
      </DialogHeader>
      <div className="p-4">
        <Textarea
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className="h-[40vh] font-mono text-sm rounded-lg"
          autoFocus
        />
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={() => onDismiss(request.id)}>
          Cancel
        </Button>
        <Button onClick={() => onRespond(request.id, { value })}>Save</Button>
      </DialogFooter>
    </>
  );
};
