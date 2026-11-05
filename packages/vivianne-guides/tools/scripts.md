# Scripts: run_script and run_native

A script is for when the work is a loop, not a decision. Instead of one tool call and one model turn per item, you write a short program that does all of it and returns only the answer. The results it reads along the way never enter your context.

## Pick one

- **Don't script one call**, or steps where each depends on your judgment of the last. Call the tool.
- **`run_script`**: JavaScript, no compile, every tool of yours callable as an async function. For fan-out and light orchestration: search ten pages and keep the three that matter; read forty files and report which mention X.
- **`run_native`**: Rust or C++, compiled once and cached. For heavy lifting: rewriting thousands of files, parsing a large log, number crunching, anything where a compiled language or its standard library is the point. Needs `rustc` or `g++` on PATH; if it isn't there the error says so.

Inside a script, tool calls are approved the same as if you made them directly. A script cannot start another script.

## `run_script`: a search across many pages

```json
{ "code": "const urls = ['https://a.example/pricing', 'https://b.example/pricing', 'https://c.example/pricing'];\nconst pages = await parallel(urls, (u) => web_fetch({ url: u, prompt: 'the monthly price of the cheapest paid plan' }));\nreturn urls.map((u, i) => `${u}: ${pages[i]}`).join('\\n');" }
```

Only the returned lines reach you, not three pages of text. `parallel(items, fn)` runs them at once.

## `run_script`: which files mention something

```json
{ "code": "const files = (await glob('**/*.ts', 'src')).split('\\n');\nconst hits = [];\nfor (const f of files) { const t = await read(f); if (t.includes('deprecatedApi(')) hits.push(f); }\nreturn hits.join('\\n') || 'none';" }
```

## `run_native`: rename a function across a repository (Rust)

Write `main` and nothing else; the bridge is already in scope. The program runs in the workspace folder, so it can read and write files with the standard library directly. That is fast, but those writes skip the approval and undo history that a `write` or `edit` call has: use `viv::write` for files the user will want to review, and std for generated or scratch files.

```json
{
  "language": "rust",
  "code": "use std::fs;\nfn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {\n    for e in fs::read_dir(dir).into_iter().flatten().flatten() {\n        let p = e.path();\n        if p.is_dir() {\n            if !matches!(p.file_name().and_then(|n| n.to_str()), Some(\"node_modules\" | \"target\" | \".git\")) { walk(&p, out); }\n        } else if p.extension().and_then(|x| x.to_str()) == Some(\"ts\") { out.push(p); }\n    }\n}\nfn main() {\n    let mut files = Vec::new();\n    walk(std::path::Path::new(\"src\"), &mut files);\n    let mut changed = 0;\n    for f in files {\n        let text = fs::read_to_string(&f).unwrap_or_default();\n        if text.contains(\"loadUser(\") {\n            // through the bridge: shown to the user, approved, undoable\n            viv::write(&f.to_string_lossy(), &text.replace(\"loadUser(\", \"fetchUser(\")).unwrap();\n            changed += 1;\n        }\n    }\n    println!(\"rewrote {changed} files\");\n}"
}
```

What the program prints (`println!`) is what comes back. A compile error comes back as the error, with the compiler's message; fix it and call again. The same source is compiled once, so re-running costs nothing.

## `run_native`: the same idea in C++

```json
{
  "language": "cpp",
  "code": "#include <iostream>\nint main() {\n    try {\n        std::string out = viv::grep(\"TODO\", \"src\");\n        std::cout << out << std::endl;\n    } catch (const std::exception& e) {\n        std::cout << \"grep failed: \" << e.what() << std::endl;\n    }\n}"
}
```

In C++ the calls are in namespace `viv`, return `std::string`, and throw `std::runtime_error` carrying the tool's error.

## The bridge, for anything the shorthands don't cover

`viv::call(tool, args_json)` calls any tool by name with its JSON arguments, exactly as you would call it directly, and gives back its text output (Rust: `Result<String, String>`; C++: a `std::string` or an exception). `viv::json(s)` turns a string into a JSON string literal.

```rust
let reply = viv::call("web_search", &format!("{{\"query\":{}}}", viv::json("rust 2024 edition changes"))).unwrap();
println!("{reply}");
```

Standard library only: there is no Cargo, so no crates.

## When it fails

- **"didn't compile"**: the message has the line and the reason. Line numbers count the bridge's lines too; look at the quoted code, not the number.
- **"ran over N seconds"**: raise `timeout_seconds` (at most 600) or make the program do less per run.
- **A tool call inside returned an error**: it comes back as `Err` / an exception with the tool's message. Handle it, as above, or the program stops with it.
