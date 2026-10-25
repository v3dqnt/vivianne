# Nebius Serverless AI: how it fits Vivianne

Source: nebius.com/serverless and docs.nebius.com/serverless, read 2026-10-14. Product facts below come from those pages; anything marked **verify** is an API or CLI detail to confirm in the console before building on it.

## 1. What Nebius Serverless gives us

Three containerised primitives, billed per second, no cluster to run:

| Primitive | What it is | Fit for Vivianne |
|---|---|---|
| **Endpoints** | A container (or model server) exposed as an authenticated HTTP service, scaled to demand | Host models we cannot run locally: a fine-tuned router, a private Nemotron, a vision model |
| **Jobs** | Finite containerised runs on GPU, then they stop | Batch work the agent hands off: fine-tuning, evals, embeddings of a large corpus, long renders |
| **Devlabs** | A remote dev environment (GPU, shell, notebook-like) | A "24/7 computer" for agents: the thing the user asked about earlier |

Token Factory (already integrated) is hosted *inference on Nebius's models*. Serverless is *our own containers on Nebius's GPUs*. They are complementary, not alternatives.

## 2. Where it plugs in (four integrations, in value order)

### 2.1 Serverless Jobs as an agent tool: "run this on a GPU" (build first)
- New tool `nebius_job` with actions `submit`, `status`, `logs`, `cancel`.
- Input: image (or a preset: `python-cuda`, `llama-cpp`, `unsloth`), command, GPU class, a workspace folder to upload, max cost.
- Output streams into the existing task/agent card; the finished artifact lands back in the workspace as a file link.
- Guardrails (reuse what exists): per-job approval with the estimated cost shown; hard `max_cost` and `max_duration`; credentials only from the vault, never in the command line; job logs pass through the same secret masking as the browser extension.
- Why first: it is the smallest unit that is genuinely new capability (agents can fine-tune, benchmark, process big data) and it demos in one scene.

### 2.2 Serverless Endpoints as a provider: "bring your own model"
- A provider kind `nebius-endpoint`: base URL plus bearer token from the vault, OpenAI-compatible when the container runs vLLM, SGLang or NIM (**verify** auth header and health route).
- Appears in the model picker like any provider; the difficulty router can use it as a tier (for example a fine-tuned Laya/Jev classifier or a cheap small model on an endpoint that scales to zero).
- Cold start is the cost: surface "warming up" in the picker and fall back to the next tier if the first token takes too long (reuse the router's fallback).
- Deploy flow: a guided "Deploy a model" action that picks a recipe from the serving catalog planned in the NemoClaw doc (phase E), so one catalog serves both local llama.cpp and a Nebius endpoint.

### 2.3 Devlab as a remote workspace: the 24/7 computer
- A workspace backend `nebius-devlab`: the agent's shell and file tools run in the Devlab over SSH instead of locally (**verify** SSH/port access and whether a persistent disk is offered).
- This is the honest answer to "where does the agent run while my laptop is closed". It pairs with `vivd` (NemoClaw phase C): `vivd` can live in the Devlab, the desktop becomes a client.
- Cost control matters most here: idle auto-stop, a visible running-cost badge in the top bar, a daily cap.

### 2.4 Jobs for the fine-tuning loop (post-hackathon)
- Local-model customisation already exists. Add "train on my corrections": export approved/edited turns as a dataset, launch a LoRA job, register the result as an endpoint or a GGUF.
- Needs a dataset consent screen and a clear "this leaves your machine" notice.

## 3. Shared pieces to build once

1. **`nebius` connection**: project ID, region, service-account key or IAM token in the vault. A single "Test connection" check in Settings that lists quotas.
2. **A thin client** (`nebius/client.rs`): auth, request signing, retry, typed errors. Prefer the REST/gRPC API; shelling out to the `nebius` CLI is the fallback (**verify** which is stable and documented).
3. **Cost ledger**: every job, endpoint and devlab minute is recorded with a price estimate next to `usage_by_model`, so the user sees one spend number.
4. **Lifecycle safety**: anything that can bill is listed in one "Running on Nebius" panel with a Stop button, and everything is stopped on explicit user request only (never silently killed, never silently left running past its cap).

## 4. Order of work

| Step | Deliver | Effort |
|---|---|---|
| 1 | Connection + client + "Running on Nebius" panel | about 1.5 days |
| 2 | `nebius_job` tool with cost approval and logs | about 2 days |
| 3 | Endpoint provider + router tier | about 2 days |
| 4 | Devlab workspace backend | about 4 days |
| 5 | Fine-tune loop | post-hackathon |

For the hackathon (deadline Oct 30): steps 1 and 2 are realistic and tell a strong Personal AI story ("my assistant can rent a GPU for ten minutes, ask me first, and show me the bill"). Step 3 if time allows.

## 5. Risks and open questions
- **API surface**: confirm whether Serverless has a public API and SDK or only the console and CLI, and its auth model. This decides whether step 1 is a client or a CLI wrapper.
- **Quotas and GPU availability** for a new account during the hackathon window; request quota early.
- **Billing surprises**: per-second billing makes runaway loops expensive. Hard caps are not optional.
- **Data egress**: uploading a workspace sends files off-device. Reuse the egress policy engine (NemoClaw phase B) and show exactly what is uploaded.
- **Duplication with Token Factory**: keep the picker honest, label "hosted by Nebius" versus "your endpoint".
