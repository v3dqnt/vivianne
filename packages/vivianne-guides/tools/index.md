# Tool use: worked examples

Calls that go wrong usually go wrong the same few ways: a path that isn't relative, an `old_string` that matches twice, `&&` in PowerShell, a loop done one call at a time that should be one script. These pages show the call that works, next to the one that doesn't, for the tools where mistakes are common.

Read the page for the tool you are about to use, before the first call, not after the first error.

- **[files](files.md)**: `read`, `grep`, `glob`, `edit`, `write`, `apply_patch`. Reading part of a file, making an `edit` unique, when to patch.
- **[shell](shell.md)**: `bash` and `powershell`. Which one, chaining commands, timeouts, quoting a path with spaces.
- **[scripts](scripts.md)**: `run_script` (JavaScript) and `run_native` (Rust or C++). When one program beats many calls, and a worked example of each.
- **[finding-tools](finding-tools.md)**: `search_tools`, `load_tools`, `list_tools`. What to do when the tool you need isn't in your function list.
- **[images](images.md)**: `generate_image`. Cloud providers, a model on this computer, and when the user's chosen model already is an image model.

## Rules that hold for every tool

1. **Arguments are exact.** Use the names in the tool's schema (`old_string`, not `old`; `patchText`, not `patch`). A required argument left out is an error, not a default.
2. **Paths are relative to the workspace** unless a tool says otherwise. Quote a path that has spaces.
3. **One call that can't be wrong beats two that might be.** `read` the lines you will change, then `edit` them; don't `edit` from memory.
4. **Independent calls go together.** Reading five files is one turn with five calls, or one `run_script`, not five turns.
5. **An error is information.** Read it before retrying; the same call again fails the same way.
