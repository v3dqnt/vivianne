# agent-coreutils

Fast file tools for AI agents, in one small native binary: `acu`.

```
npm i -g agent-coreutils      # or: npx agent-coreutils skim
acu skim                      # the shape of a codebase in one call
acu grep 'TODO|FIXME' src -C 1
acu edit src/a.ts --old 'retries = 3' --new 'retries = 5'
acu mcp                       # serve every command to an MCP client over stdio
acu agent-guide               # a short guide to paste into an agent's instructions
```

This package installs a prebuilt binary for your platform (Windows, macOS, Linux; x64 and arm64) through
`optionalDependencies`, the same way esbuild does. See the project README for the full command list.
