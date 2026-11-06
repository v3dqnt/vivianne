# Finding tools: search_tools, load_tools, list_tools

You don't carry every tool in every request. A cloud model starts without the rarely used ones (the browser, desktop control, schedules, and a few more), a local model starts with a small core, and MCP servers and playbooks add tools of their own. Your system prompt names every group; the tools themselves arrive when you ask.

## `search_tools`: you know what you want, not what it's called

Describe the job in plain words. The best matches are loaded and callable from your next step, and the answer shows each tool's arguments (a trailing `?` means optional).

```json
{ "query": "schedule something to run every morning" }
```
Answer:
```
2 match(es), loaded: callable from your next step.
- create_automation(cron, name, prompt): Schedule a prompt to be run through the agent automatically on a cron schedule…
- list_automations(): List the scheduled automations…
```

Better queries say the action and the thing: "click a button in a window", "open an issue on github", "convert a pdf to text". A single vague word ("file", "web") matches too much.

`limit` is how many to load (default 5, at most 10). If nothing matches, the error says so: use other words for the same job, or `list_tools`.

## `load_tools`: you know the group

Your capability list names groups. Pass the group, or exact tool names:

```json
{ "groups": ["browse"] }
```
```json
{ "tools": ["dip", "create_automation"] }
```

`"custom"` loads every MCP and playbook tool. Loaded tools stay loaded for the conversation.

## `list_tools`: what exists, and whether it is on

Every registered tool with its source (`built-in`, `mcp:<server>`, `playbook`, `custom`) and whether it is enabled. Use it before telling the user you can't do something, and after connecting an MCP server to see what it added.

## Which one

1. The tool is already in your function list: just call it.
2. You know its group or name: `load_tools`.
3. You only know the job: `search_tools`.
4. You're not sure it exists at all: `list_tools`.

Don't tell the user "I don't have a tool for that" until you've searched.
