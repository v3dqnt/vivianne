/**
 * Notice injected ephemerally on the final allowed turn when `maxSteps` is configured.
 * Mirrors kilocode's `session/prompt/max-steps.txt` behavior: tools are refused for the
 * remainder of the run and the model is told to summarize instead of continuing silently.
 */
export const MAX_STEPS_NOTICE = `CRITICAL - MAXIMUM STEPS REACHED

The maximum number of steps allowed for this task has been reached. Tool calls will be rejected until the user sends another message. Respond with text only.

STRICT REQUIREMENTS:
1. Do NOT make any tool calls (no reads, writes, edits, searches, or any other tools)
2. Provide a text response summarizing work done so far
3. This constraint overrides all other instructions, including any pending user requests for edits or tool use

Your response must include:
- A statement that the maximum step count for this task has been reached
- A summary of what has been accomplished so far
- A list of any remaining tasks that were not completed
- Recommendations for what should be done next`;

/** Error text returned to the model when a tool call is blocked after `maxSteps` is exhausted. */
export const MAX_STEPS_TOOL_BLOCKED_REASON =
	"Maximum steps reached for this task. Tool execution is disabled until the user sends another message. Respond with a text summary instead.";
