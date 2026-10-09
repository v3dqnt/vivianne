#include "mcp/mcp.hpp"

#include <iostream>

#include "cmd/cmd.hpp"
#include "core/platform.hpp"

namespace acu {

namespace {

struct Prop {
  const char* name;
  const char* type;    // string | boolean | integer | string[]
  const char* target;  // "@pos", "@stdin", or the command's option name
  const char* desc;
  bool required;
};

struct Tool {
  const char* name;
  const char* cmd;
  const char* desc;
  std::vector<Prop> props;
};

const std::vector<Tool>& tools() {
  static const std::vector<Tool> t = {
      {"acu_skim", "skim",
       "The shape of a codebase in one call: directories with file/line counts and language mix, files with top-level "
       "symbols, within a token budget. Start here when exploring an unfamiliar repository.",
       {{"path", "string", "@pos", "Directory to skim (default: current directory)", false},
        {"depth", "integer", "depth", "Maximum directory depth to expand", false},
        {"budget", "integer", "budget", "Output budget in tokens (default 4000; 0 = unlimited)", false}}},
      {"acu_outline", "outline",
       "Symbols (classes, functions, methods, types) of one or more files, nested, with 1-based line numbers.",
       {{"paths", "string[]", "@pos", "Files to outline", true}}},
      {"acu_read", "read",
       "Read files with line numbers (N<TAB>text). Several files are returned under '== path ==' headers. Long files "
       "are cut after `max` lines with a note giving the next --lines range.",
       {{"paths", "string[]", "@pos", "Files to read", true},
        {"lines", "string", "lines", "Line range such as '40:80', '40:' or ':80'", false},
        {"max", "integer", "max", "Maximum lines per file (default 2000; 0 = no limit)", false}}},
      {"acu_grep", "grep",
       "Search file contents with a regular expression (or literal). Respects .gitignore, skips binary files. Output is "
       "path:line:text, capped with a note saying how many matches were left out.",
       {{"pattern", "string", "@pos", "Regular expression (or literal text with fixed=true)", false},
        {"patterns", "string[]", "e", "Several patterns searched in one pass (instead of `pattern`)", false},
        {"paths", "string[]", "@pos", "Files or directories to search (default: current directory)", false},
        {"fixed", "boolean", "fixed-strings", "Treat patterns as literal text", false},
        {"ignore_case", "boolean", "ignore-case", "Case-insensitive (ASCII)", false},
        {"word", "boolean", "word-regexp", "Match whole words only", false},
        {"context", "integer", "context", "Lines of context around each match", false},
        {"type", "string", "type", "File types, comma separated: ts, js, py, rs, go, java, c, cpp, ...", false},
        {"glob", "string[]", "glob", "Only files matching these globs; prefix '!' to exclude", false},
        {"max", "integer", "max", "Maximum matches to print (default 200; 0 = all)", false},
        {"files_only", "boolean", "files-with-matches", "Only list the files that match", false},
        {"count", "boolean", "count", "Only print match counts per file", false},
        {"no_ignore", "boolean", "no-ignore", "Do not apply ignore files", false}}},
      {"acu_glob", "glob",
       "Find files by name. Patterns such as '**/*.test.ts' or 'src/**/{a,b}.rs'; a pattern without '/' matches file "
       "names at any depth.",
       {{"patterns", "string[]", "@pos", "Glob patterns", true},
        {"in", "string", "in", "Directory to search in (default: current directory)", false},
        {"dirs", "boolean", "dirs", "Include directories in the results", false},
        {"max", "integer", "max", "Maximum paths (default 500; 0 = all)", false},
        {"no_ignore", "boolean", "no-ignore", "Do not apply ignore files", false}}},
      {"acu_ls", "ls",
       "List a directory (directories first, trailing '/'); recursive gives a compact tree.",
       {{"path", "string", "@pos", "Directory (default: current directory)", false},
        {"recursive", "boolean", "recursive", "Show a tree", false},
        {"depth", "integer", "depth", "Tree depth (implies recursive)", false},
        {"max", "integer", "max", "Maximum entries (default 500; 0 = all)", false},
        {"no_ignore", "boolean", "no-ignore", "Do not apply ignore files", false}}},
      {"acu_stat", "stat",
       "Size, line count, language, line endings and modified time for each path.",
       {{"paths", "string[]", "@pos", "Paths to inspect", true}}},
      {"acu_find_def", "find-def",
       "Where a function, class, type or constant is defined.",
       {{"symbol", "string", "@pos", "Symbol name", true},
        {"paths", "string[]", "@pos", "Where to look (default: current directory)", false},
        {"ignore_case", "boolean", "ignore-case", "Case-insensitive name match", false},
        {"max", "integer", "max", "Maximum results (default 50)", false}}},
      {"acu_refs", "refs",
       "Whole-word search for a symbol, ranked: definitions, then import lines, then other references.",
       {{"symbol", "string", "@pos", "Symbol name", true},
        {"paths", "string[]", "@pos", "Where to look (default: current directory)", false},
        {"max", "integer", "max", "Maximum results (default 100)", false}}},
      {"acu_write", "write",
       "Write a file atomically (creates parent directories). Keeps an existing file's line endings and BOM.",
       {{"path", "string", "@pos", "File to write", true},
        {"content", "string", "@stdin", "Full file content", true},
        {"append", "boolean", "append", "Append instead of replacing", false},
        {"raw", "boolean", "raw", "Write the bytes exactly as given (no line-ending or BOM preservation)", false}}},
      {"acu_edit", "edit",
       "Exact-string replace in a file. Fails without writing if `old` is not found or matches more than once (unless "
       "`all`); the error lists line numbers or the closest lines so you can correct it. Use `edits` for several "
       "replacements applied all-or-nothing.",
       {{"path", "string", "@pos", "File to edit", true},
        {"old", "string", "old", "Exact text to replace", false},
        {"new", "string", "new", "Replacement text (may be empty)", false},
        {"all", "boolean", "all", "Replace every occurrence", false},
        {"edits", "string", "edits-json", "JSON array of {old, new, all?} objects, instead of old/new", false}}},
      {"acu_patch", "patch",
       "Apply a unified diff. Reports each rejected hunk; a file with a rejected hunk is left untouched.",
       {{"patch", "string", "@stdin", "The diff", true},
        {"strip", "integer", "p", "Leading path components to strip (default 1 for git diffs)", false},
        {"fuzz", "integer", "fuzz", "Allow hunks to drift this many lines (default 0)", false}}},
      {"acu_mv", "mv",
       "Move or rename. Refuses to overwrite unless `force`.",
       {{"sources", "string[]", "@pos", "Paths to move", true},
        {"destination", "string", "@pos", "Destination path or directory", true},
        {"force", "boolean", "force", "Replace an existing destination", false}}},
      {"acu_cp", "cp",
       "Copy files or (recursive) directories. Refuses to overwrite unless `force`.",
       {{"sources", "string[]", "@pos", "Paths to copy", true},
        {"destination", "string", "@pos", "Destination path or directory", true},
        {"recursive", "boolean", "recursive", "Copy directories", false},
        {"force", "boolean", "force", "Replace an existing destination", false}}},
      {"acu_rm", "rm",
       "Remove files. Goes to the OS trash unless `permanent`.",
       {{"paths", "string[]", "@pos", "Paths to remove", true},
        {"recursive", "boolean", "recursive", "Remove directories", false},
        {"permanent", "boolean", "permanent", "Delete outright instead of using the trash", false}}},
  };
  return t;
}

Json schemaFor(const Tool& t) {
  Json props = Json::object(), req = Json::array();
  for (auto& p : t.props) {
    Json j = Json::object();
    std::string ty = p.type;
    if (ty == "string[]") {
      j.set("type", Json::str("array"));
      Json items = Json::object();
      items.set("type", Json::str("string"));
      j.set("items", std::move(items));
    } else {
      j.set("type", Json::str(ty));
    }
    j.set("description", Json::str(p.desc));
    props.set(p.name, std::move(j));
    if (p.required) req.push(Json::str(p.name));
  }
  Json s = Json::object();
  s.set("type", Json::str("object"));
  s.set("properties", std::move(props));
  if (!req.a.empty()) s.set("required", std::move(req));
  s.set("additionalProperties", Json::boolean(false));
  return s;
}

std::string scalarText(const Json& v) {
  if (v.t == Json::Str) return v.s;
  if (v.t == Json::Num) return v.isInt ? std::to_string((long long)v.n) : std::to_string(v.n);
  if (v.t == Json::Bool) return v.b ? "1" : "0";
  return v.dump();
}

// Runs a tool; returns the text and whether it is an error.
std::string callTool(Ctx& ctx, const Tool& t, const Json* argsJson, bool& isError) {
  isError = false;
  Args args;
  Io io;
  if (argsJson && argsJson->t == Json::Obj) {
    for (auto& kv : argsJson->o) {
      const Prop* p = nullptr;
      for (auto& q : t.props)
        if (kv.first == q.name) p = &q;
      if (!p) {
        isError = true;
        return "unknown argument '" + kv.first + "' for " + t.name;
      }
      std::string ty = p->type, target = p->target;
      const Json& v = kv.second;
      if (v.t == Json::Null) continue;
      if (ty == "string[]") {
        std::vector<std::string> items;
        if (v.t == Json::Arr) {
          for (auto& e : v.a) items.push_back(scalarText(e));
        } else {
          items.push_back(scalarText(v));  // tolerate a bare string
        }
        for (auto& s : items) {
          if (target == "@pos") args.pos.push_back(s);
          else args.opt[target].push_back(s);
        }
      } else if (ty == "boolean") {
        if (v.t == Json::Bool && v.b) args.opt[target].push_back("1");
      } else if (target == "@pos") {
        args.pos.push_back(scalarText(v));
      } else if (target == "@stdin") {
        io.in = scalarText(v);
        io.haveIn = true;
      } else {
        args.opt[target].push_back(scalarText(v));
      }
    }
  }
  for (auto& p : t.props) {
    if (!p.required) continue;
    bool present = argsJson && argsJson->get(p.name) && argsJson->get(p.name)->t != Json::Null;
    if (!present) {
      isError = true;
      return std::string("missing required argument '") + p.name + "' for " + t.name;
    }
  }
  io.haveIn = true;  // a tool call never reads the server's own stdin
  const Command* c = findCommand(t.cmd);
  int rc;
  try {
    rc = runCommandArgs(ctx, *c, args, io);
  } catch (const std::exception& e) {
    isError = true;
    return std::string(t.name) + ": " + e.what();
  }
  std::string text = io.out;
  if (!io.err.empty()) text += (text.empty() || text.back() == '\n' ? "" : "\n") + io.err;
  if (rc >= kUsage || (rc == kNotFound && io.out.empty() && !io.err.empty())) isError = true;
  if (text.empty()) text = rc == kNotFound ? "no results" : "ok";
  return text;
}

Json errorResponse(const Json& id, int code, const std::string& msg) {
  Json r = Json::object(), e = Json::object();
  r.set("jsonrpc", Json::str("2.0"));
  r.set("id", id);
  e.set("code", Json::num(code));
  e.set("message", Json::str(msg));
  r.set("error", std::move(e));
  return r;
}

Json handleOne(Ctx& ctx, const Json& req, bool& respond) {
  respond = true;
  const Json* idp = req.get("id");
  Json id = idp ? *idp : Json();
  const Json* m = req.get("method");
  if (req.t != Json::Obj || !m || m->t != Json::Str) return errorResponse(id, -32600, "invalid request");
  const std::string& method = m->s;
  if (!idp) {  // notification
    respond = false;
    return Json();
  }
  Json res = Json::object();
  if (method == "initialize") {
    std::string ver = "2025-06-18";
    if (const Json* p = req.get("params"))
      if (const Json* pv = p->get("protocolVersion"))
        if (pv->t == Json::Str && (pv->s == "2024-11-05" || pv->s == "2025-03-26" || pv->s == "2025-06-18")) ver = pv->s;
    res.set("protocolVersion", Json::str(ver));
    Json caps = Json::object(), tl = Json::object();
    tl.set("listChanged", Json::boolean(false));
    caps.set("tools", std::move(tl));
    res.set("capabilities", std::move(caps));
    Json info = Json::object();
    info.set("name", Json::str("agent-coreutils"));
    info.set("version", Json::str(ACU_VERSION));
    res.set("serverInfo", std::move(info));
    res.set("instructions", Json::str("Fast file tools. Start with acu_skim to see a codebase, acu_grep/acu_glob to find things, "
                                      "acu_read to read, acu_edit for exact-string edits, acu_write to create files."));
  } else if (method == "ping") {
    // empty result
  } else if (method == "tools/list") {
    Json arr = Json::array();
    for (auto& t : tools()) {
      Json j = Json::object();
      j.set("name", Json::str(t.name));
      j.set("description", Json::str(t.desc));
      j.set("inputSchema", schemaFor(t));
      arr.push(std::move(j));
    }
    res.set("tools", std::move(arr));
  } else if (method == "tools/call") {
    const Json* p = req.get("params");
    const Json* n = p ? p->get("name") : nullptr;
    if (!n || n->t != Json::Str) return errorResponse(id, -32602, "tools/call needs params.name");
    const Tool* tool = nullptr;
    for (auto& t : tools())
      if (n->s == t.name) tool = &t;
    if (!tool) return errorResponse(id, -32602, "unknown tool '" + n->s + "'");
    bool isErr = false;
    std::string text = callTool(ctx, *tool, p->get("arguments"), isErr);
    Json content = Json::array(), item = Json::object();
    item.set("type", Json::str("text"));
    item.set("text", Json::str(text));
    content.push(std::move(item));
    res.set("content", std::move(content));
    res.set("isError", Json::boolean(isErr));
  } else {
    return errorResponse(id, -32601, "method not found: " + method);
  }
  Json r = Json::object();
  r.set("jsonrpc", Json::str("2.0"));
  r.set("id", id);
  r.set("result", std::move(res));
  return r;
}

}  // namespace

std::string handleMcpMessage(Ctx& ctx, const std::string& line) {
  Json req;
  std::string err;
  if (!parseJson(line, req, err)) return errorResponse(Json(), -32700, "parse error: " + err).dump();
  if (req.t == Json::Arr) {
    Json out = Json::array();
    for (auto& r : req.a) {
      bool respond;
      Json resp = handleOne(ctx, r, respond);
      if (respond) out.push(std::move(resp));
    }
    return out.a.empty() ? "" : out.dump();
  }
  bool respond;
  Json resp = handleOne(ctx, req, respond);
  return respond ? resp.dump() : "";
}

int runMcpServer(Ctx& ctx) {
  std::string line;
  while (std::getline(std::cin, line)) {
    if (!line.empty() && line.back() == '\r') line.pop_back();
    if (line.empty()) continue;
    std::string out = handleMcpMessage(ctx, line);
    if (!out.empty()) writeOut(out + "\n");
  }
  return 0;
}

}  // namespace acu
