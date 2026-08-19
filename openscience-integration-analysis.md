# OpenScience → Vivianne Playbook Integration Analysis

> How to integrate [OpenScience](https://github.com/synthetic-sciences/openscience)
> into Vivianne as a **playbook** — a plugin-like system that adds scientific
> research capabilities to the base agent.

---

## 1. What Is OpenScience?

OpenScience is an **AI workbench for scientific research** — think "an autonomous
research assistant that reads papers, writes and runs code, queries scientific
databases, runs experiments, and writes up findings."

The key difference from Vivianne: **Vivianne is a general-purpose coding agent**,
while **OpenScience is domain-specific for scientific research**. OpenScience has:

- **290+ domain skills** — training (DeepSpeed, PEFT, TRL), evaluation, datasets,
  molecular/clinical biology, cheminformatics, papers/LaTeX, cloud compute
- **30+ scientific database connectors** — UniProt, PDB, Ensembl, ChEMBL,
  PubChem, arXiv, OpenAlex, Semantic Scholar, and more
- **Specialist agent harnesses** — `research` (default), `biology`, `physics`,
  `ml` specialists, plus critique/literature-review/write sub-agents
- **Read-only plan mode** with structured research workflow
- **Plugin system** (`@synsci/plugin`) — a formal hooks-based extension API

---

## 2. What a "Playbook" Means in Vivianne

A **playbook** is a plugin-like extension that adds **multiple coordinated
capabilities** to the base agent. Based on your existing `agent-playbook.md`
and the `directive.rs` system, a playbook would:

| Aspect | What It Adds |
|---|---|
| **Tools** | New agent-callable functions (e.g., `science_search`, `protein_viewer`) |
| **Agent prompts** | Domain-specific system prompts (e.g., "You are a biology research assistant") |
| **Sub-agents** | Specialist agents (e.g., `literature-review`, `critique`, `write`) |
| **Skills** | Instruction bundles for domain tasks (e.g., `train-model`, `analyze-genome`) |
| **MCP servers** | Domain-specific MCP tool providers |
| **UI panels** | Specialized frontend views (e.g., molecule viewer, structure renderer) |
| **Config schema** | Playbook-specific settings merged into the main config |

---

## 3. OpenScience's Architecture — The Key Integration Points

### 3.1 Scientific Database Connectors

The heart of OpenScience's value — **30+ REST connectors to major scientific databases**:

```
backend/cli/src/science/connectors/
├── index.ts                 ← Connector registry (shared singleton)
├── types.ts                 ← ConnectorHit, Connector interface
├── http.ts                  ← Shared HTTP client with rate limiting & retry
├── proteins/
│   ├── uniprot.ts           ← UniProt protein database
│   ├── alphafold.ts         ← AlphaFold structure predictions
│   ├── pdbe.ts              ← PDBe (Protein Data Bank Europe)
│   ├── rcsb-pdb.ts          ← RCSB Protein Data Bank
│   ├── interpro.ts          ← InterPro protein families
│   ├── sifts.ts             ← SIFTS residue mapping
│   └── util.ts
├── genomics/
│   ├── ensembl.ts           ← Ensembl genome browser
│   ├── ncbi-gene.ts         ← NCBI Gene
│   ├── ncbi-nucleotide.ts   ← NCBI Nucleotide
│   └── refseq.ts            ← RefSeq
├── chemistry/
│   ├── chembl.ts            ← ChEMBL bioactive molecules
│   ├── pubchem.ts           ← PubChem compounds
│   └── ...
├── literature/
│   ├── arxiv.ts             ← arXiv preprints
│   ├── pubmed.ts            ← PubMed abstracts
│   ├── semantic-scholar.ts  ← Semantic Scholar
│   ├── openalex.ts          ← OpenAlex
│   ├── crossref.ts          ← CrossRef
│   ├── europepmc.ts         ← Europe PMC
│   └── biorxiv.ts           ← bioRxiv
├── pathways/
│   ├── reactome.ts          ← Reactome pathways
│   ├── kegg.ts              ← KEGG pathways
│   └── wikipathways.ts      ← WikiPathways
└── omics/
    ├── geo.ts               ← GEO expression data
    ├── arrayexpress.ts      ← ArrayExpress
    └── ...
```

**Connector interface** (from `types.ts`):
```typescript
export interface Connector {
  id: string                  // e.g. "uniprot", "arxiv"
  name: string               // e.g. "UniProt"
  domain: string             // e.g. "proteins", "literature"
  description: string        // Human-readable summary
  search(query: string, opts: SearchOptions): Promise<ConnectorHit[]>
}
```

**Only 2 agent tools** (`science_list_dbs` + `science_search`) expose all 30+
databases — the model discovers databases via `science_list_dbs` and queries
them via `science_search`. This keeps the tool count flat as connectors grow.

### 3.2 The Plugin System (`@synsci/plugin`)

OpenScience has a formal **hooks-based plugin API**:

```typescript
export type Plugin = (input: PluginInput) => Promise<Hooks>

export interface Hooks {
  event?: (input: { event: Event }) => Promise<void>
  config?: (input: Config) => Promise<void>
  tool?: { [key: string]: ToolDefinition }          // ← Add tools
  auth?: AuthHook                                    // ← Add auth providers
  "chat.message"?: (input, output) => Promise<void>   // ← Intercept messages
  "chat.params"?: (input, output) => Promise<void>    // ← Modify LLM params
  "chat.headers"?: (input, output) => Promise<void>   // ← Modify LLM headers
  "permission.ask"?: (input, output) => Promise<void> // ← Permission hooks
  "tool.execute.before"?: (input, output) => Promise<void>
  "tool.execute.after"?: (input, output) => Promise<void>
  "experimental.chat.messages.transform"?: ...
  "experimental.chat.system.transform"?: ...
  "experimental.session.compacting"?: ...
  "experimental.text.complete"?: ...
}
```

This is **directly analogous to what a Vivianne playbook would look like**.

### 3.3 Agent System

OpenScience has specialist agents with their own prompts:

| Agent | Prompt File | Purpose |
|---|---|---|
| `research` | `research.txt` | Default research harness — full research loop |
| `biology` | `biology.txt` | Molecular/clinical biology specialist |
| `physics` | `physics.txt` | Physics specialist |
| `ml` | `ml.txt` | Machine learning specialist |
| `critique` | `critique.txt` | Sub-agent: critiques plans/results |
| `literature-review` | `literature-review.txt` | Sub-agent: reviews literature |
| `write` | `write.txt` | Sub-agent: writes up findings |
| `plan` | `plan.txt` | Read-only plan mode |

### 3.4 Skills System

OpenScience has **290+ domain skills** in `backend/cli/skills/`. Skills are
instruction bundles the agent loads on demand, covering:

- **Training**: DeepSpeed, PEFT, TRL, distributed training
- **Evaluation**: Model evaluation, benchmarks
- **Datasets**: Dataset processing, loading, analysis
- **Biology**: Molecular dynamics, sequence analysis
- **Chemistry**: Cheminformatics, molecular modeling
- **Publishing**: LaTeX, papers, figures
- **Cloud**: Modal, Tinker compute

---

## 4. Integration Architecture — Vivianne Playbook Design

### 4.1 High-Level Design

The playbook would be a **self-contained module** that Vivianne discovers and
loads at startup. It would hook into existing extension points:

```
vivianne core                          openscience playbook
─────────────────                      ─────────────────────
Agent loop ──────tool registry────────► ScienceTools
                 │                        ├── science_list_dbs
                 │                        └── science_search
                 │
                 agent registry ─────────► ResearchAgent
                 │                        ├── biology agent
                 │                        ├── physics agent
                 │                        └── ml agent
                 │
                 skill loader ───────────► Skills bundle (290+)
                 │
                 MCP server ────────────► Connector registry (30+ DBs)
                 │
                 config file ────────────► .openscience/config.json
                 │
                 UI panels ─────────────► Molecule viewer
                 │                        Genome browser
                 │                        Structure renderer
                 │
                 plugin system ─────────► Hooks: tool, auth, chat.*
```

### 4.2 Connector Integration — The Two-Tool Pattern

The most elegant aspect of OpenScience is that **30+ databases are exposed
through only 2 agent tools**. Vivianne should adopt this exact pattern:

```rust
// In Vivianne's tool system (Rust backend)
Tool::register(ScienceListDbsTool {   // science_list_dbs
    description: "List available scientific databases",
    execute: || -> Vec<DatabaseInfo>,
});

Tool::register(ScienceSearchTool {    // science_search
    description: "Search a scientific database",
    parameters: { db: String, query: String, limit: u32 },
    execute: |params| -> Vec<Result>,
});
```

The connector registry backend (Rust port of the TypeScript connectors) would
live in a new crate or module:

```
packages/gui/src-tauri/src/science/
├── mod.rs                    ← Module declarations
├── connector.rs              ← Connector trait + registry
├── http.rs                   ← Shared HTTP client with rate limiting
├── proteins/
│   ├── mod.rs
│   ├── uniprot.rs
│   ├── alphafold.rs
│   └── pdbe.rs
├── literature/
│   ├── mod.rs
│   ├── arxiv.rs
│   ├── pubmed.rs
│   └── semantic_scholar.rs
├── chemistry/
│   ├── mod.rs
│   ├── chembl.rs
│   └── pubchem.rs
├── genomics/
│   ├── mod.rs
│   ├── ensembl.rs
│   └── ncbi.rs
└── pathways/
    ├── mod.rs
    ├── reactome.rs
    └── kegg.rs
```

### 4.3 Agent Prompt Integration

OpenScience's dual-layer prompt system translates directly to Vivianne's:

| OpenScience | Vivianne Equivalent |
|---|---|
| `src/session/system.ts` (provider-level) | `agent/harness/prompt_templates.rs` |
| `src/session/prompt.ts` (agent-level) | `coding_agent/system_prompt.rs` |
| `src/agent/agent.ts` (agent registry) | `coding_agent/session.rs` (agent config) |
| `src/agent/prompt/*.txt` | Playbook's prompt files |

A playbook would register its prompts via the **directive system**:

```rust
// In the playbook's registration code
Directive::register("research-agent", AgentDirective {
    name: "research",
    system_prompt: include_str!("./prompts/research.txt"),
    tools: vec!["science_list_dbs", "science_search", "bash", "read", "edit", ...],
    sub_agents: vec!["literature-review", "critique", "write"],
    default_model: "claude-sonnet-4-5",  // or other capable model
});
```

### 4.4 Skills Integration

OpenScience's 290+ skills would be ported as Vivianne **playbook skills**:

```
.openscience/
└── skills/
    ├── train-model/
    │   └── skill.json
    ├── analyze-genome/
    │   └── skill.json
    ├── write-paper/
    │   └── skill.json
    ├── run-molecular-dynamics/
    │   └── skill.json
    └── ...
```

Vivianne's existing skill system (`src/coding_agent/skills.rs`) would load these
from the playbook's skill directory.

### 4.5 Frontend Additions

The playbook could add specialized UI components to Vivianne's workspace:

| Component | Technology | Purpose |
|---|---|---|
| **MoleculeViewer** | 3Dmol.js / NGL Viewer | Render molecular structures from PDB |
| **SequenceViewer** | BioJS / custom | Display DNA/protein sequences |
| **StructureViewer** | Mol* / custom | 3D protein structure rendering |
| **PlotViewer** | Observable Plot / Chart.js | Render experimental plots |
| **GenomeBrowser** | igv.js / custom | Visualize genomic data |
| **LiteraturePanel** | Custom | Browse/search papers inline |

These would be registered via Vivianne's extension UI system (`ExtensionUIHandler`).

### 4.6 MCP Server for Scientific Databases

The connectors could alternatively (or additionally) be exposed as an **MCP
server** that Vivianne connects to:

```
openscience-mcp-server/
├── index.ts              ← MCP server entry
├── connectors/           ← Re-used connector implementations
│   ├── proteins/
│   ├── literature/
│   └── ...
└── tools/
    ├── science_list_dbs   ← MCP tool
    └── science_search     ← MCP tool
```

This approach means **zero Rust changes** — the connectors run as a sidecar
MCP process that Vivianne already supports (see `mcp/mod.rs` and the MCP
server management UI in `SettingsPage.tsx`).

---

## 5. Recommended Integration Approach

### Phase 1: MCP Sidecar (Fastest Path, ~1 week)

1. **Package the TypeScript connectors** as a standalone MCP server
2. Vivianne already supports MCP servers — the user adds the MCP server URL
   via Settings → MCP Servers
3. The `science_list_dbs` and `science_search` tools appear automatically
4. **No code changes to Vivianne core** — pure configuration

**Pros**: Zero Vivianne changes, works immediately, connectors stay in TypeScript
**Cons**: MCP latency, dependency on Node/Bun runtime

### Phase 2: Native Playbook (Deeper Integration, ~2-4 weeks)

1. **Create a `playbooks/openscience/` directory** in the project root
2. **Port the connector registry** to Rust (or keep in TS via `deno_core`/`boa_engine`)
3. **Register agent directives** for `research`, `biology`, `physics`, `ml`
4. **Add the two science tools** (`science_list_dbs`, `science_search`) to the tool registry
5. **Create skill directory** with domain-specific skill bundles
6. **Add UI panels** as React components (molecule viewer, etc.)

### Phase 3: Full Plugin System (Long-term, ~1 month)

1. **Design a Vivianne plugin API** inspired by OpenScience's `@synsci/plugin`:
   ```rust
   pub trait Playbook: Send + Sync {
       fn name(&self) -> &str;
       fn tools(&self) -> Vec<ToolInfo>;
       fn agents(&self) -> Vec<AgentDirective>;
       fn skills(&self) -> Vec<SkillBundle>;
       fn hooks(&self) -> PlaybookHooks;
       fn ui_panels(&self) -> Vec<PanelDefinition>;
   }
   ```
2. **Dynamic loading** — playbooks are discovered from `~/.vivianne/playbooks/`
3. **Hot-reload** — playbooks can be added/removed without restart
4. **Plugin marketplace** — shareable playbooks via npm/GitHub

---

## 6. Connector Porting Map

| Domain | Connector | API | Auth | Priority |
|---|---|---|---|---|
| **Proteins** | UniProt | REST | None | P0 |
| | AlphaFold | REST | None | P0 |
| | RCSB PDB | REST (GraphQL) | None | P0 |
| | PDBe | REST | None | P1 |
| | InterPro | REST | None | P1 |
| | SIFTS | REST | None | P2 |
| **Literature** | arXiv | REST | None | P0 |
| | PubMed | E-utilities | None (rate-limited) | P0 |
| | Semantic Scholar | REST | API key (optional) | P0 |
| | OpenAlex | REST | None | P1 |
| | CrossRef | REST | None | P1 |
| | Europe PMC | REST | None | P1 |
| | bioRxiv | REST | None | P2 |
| **Chemistry** | PubChem | REST (PUG) | None | P0 |
| | ChEMBL | REST | None | P0 |
| **Genomics** | Ensembl | REST | None | P1 |
| | NCBI Gene | E-utilities | None (rate-limited) | P1 |
| | RefSeq | FTP/API | None | P2 |
| **Pathways** | Reactome | REST (GraphQL) | None | P1 |
| | KEGG | REST | None (key req.) | P2 |
| | WikiPathways | REST (SPARQL) | None | P2 |
| **Omics** | GEO | E-utilities | None | P2 |
| | ArrayExpress | REST | None | P2 |

Most connectors are **open-access REST APIs requiring no authentication** —
only rate limits. This makes them trivial to port.

---

## 7. Key Differences to Bridge

| Aspect | OpenScience | Vivianne |
|---|---|---|
| **Language** | TypeScript (Bun) | Rust (Tauri) |
| **Tool system** | Zod-validated, `Tool.define()` | Rust-based tool exec |
| **Plugin system** | Formal `Plugin` type + `Hooks` | MCP servers, directive.rs |
| **Frontend** | SolidJS workspace | React/Next.js (shadcn/ui) |
| **Agent model** | Dual-layer prompts | Single system prompt |
| **Streaming** | SSE to browser | Tauri IPC events |
| **Skills** | 290+ bundled skill bundles | Skills loaded from `.viv/skills/` |
| **Connectors** | TypeScript classes with single interface | No equivalent |

---

## 8. Conclusion — What to Build

The OpenScience project is an ideal fit for a Vivianne **playbook** because:

1. **Clean separation** — All 30+ database connectors follow the same `Connector`
   interface with 2 flat tools exposing them
2. **No auth overhead** — Almost all scientific databases are open-access
3. **Plugin API exists** — OpenScience's `@synsci/plugin` hooks model can be
   adapted as Vivianne's playbook API
4. **Process-isolated MCP option** — Can ship as MCP server without any Rust changes
5. **High-value domain** — Scientific research is a use case Vivianne currently
   cannot address at all

**Recommended first step**: Package the connectors as an MCP server (phase 1).
This gives instant value with zero core changes and lets you validate the
workflow before committing to deeper Rust integration.

---

*Analysis performed July 2026. OpenScience repository: [github.com/synthetic-sciences/openscience](https://github.com/synthetic-sciences/openscience)*
