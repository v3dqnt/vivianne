# Shell: bash and powershell

Both run one command and return its output. For something that must stay alive between calls (a dev server, an interactive prompt, an activated environment), use `terminal` instead.

## Which one

Use the shell the computer has. On Windows that is `powershell`; `bash` there is Git Bash, fine for Unix tools but with Windows paths that need care. On macOS and Linux it is `bash`.

## Chaining commands

In `bash`, `&&` runs the second command only if the first worked:
```json
{ "command": "cd packages/gui && npm test" }
```

Windows PowerShell 5.1 has no `&&`. It is a parse error, and the call fails before running anything. Use `;`, or `if ($?)`:
```json
{ "command": "Set-Location packages\\gui; npm test" }
```
```json
{ "command": "cargo build; if ($?) { cargo test }" }
```

Prefer not to `cd` at all: pass the folder to the tool (`npm --prefix packages/gui test`, `git -C packages/gui status`).

## Paths with spaces

Quote them. In `bash`: `cd "C:/Users/me/My Project"`. In PowerShell: `Set-Location "C:\Users\me\My Project"`. Without the quotes the command sees two arguments and fails with "too many arguments" or "cannot find path".

## Timeouts

Default 120 seconds, at most 600. A build or test run that takes longer needs `timeout`:
```json
{ "command": "cargo test --no-default-features --lib", "timeout": 600 }
```
If it still won't finish, start it in a `terminal` session and read the output as it comes.

## What not to use the shell for

Reading a file (`read`), searching (`grep`, `glob`), changing a file (`edit`, `write`): the dedicated tools are checked, cheaper in context and don't depend on which shell it is. Reserve the shell for running things: builds, tests, git, package managers.
