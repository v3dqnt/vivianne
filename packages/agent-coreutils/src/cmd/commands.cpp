#include "cmd/cmd.hpp"
#include "index/daemon.hpp"

namespace acu {

const std::vector<const Command*>& allCommands() {
  static const std::vector<const Command*> v = {
      &cmd_skim, &cmd_outline, &cmd_read, &cmd_head, &cmd_tail, &cmd_stat,   &cmd_ls,   &cmd_glob, &cmd_grep,
      &cmd_find_def, &cmd_refs, &cmd_write, &cmd_edit, &cmd_patch, &cmd_mv, &cmd_cp,   &cmd_rm,   &cmd_guide, &cmd_index, &cmd_daemon};
  return v;
}

const Command* findCommand(const std::string& name) {
  for (auto* c : allCommands())
    if (name == c->name) return c;
  return nullptr;
}

size_t capOf(const Args& a, size_t def) {
  long v = a.num("max", (long)def);
  if (v < 0) throw UsageError{"--max must not be negative"};
  return v == 0 ? (size_t)-1 : (size_t)v;
}

std::string commandHelp(const Command& c) { return std::string("usage: acu ") + c.usage + "\n"; }

std::string helpText() {
  std::string h =
      "acu " ACU_VERSION " - fast file tools for AI agents\n\n"
      "usage: acu [--root DIR] <command> [options]\n\ncommands:\n";
  for (auto* c : allCommands()) {
    std::string u = c->usage;
    for (size_t p = 0; p < u.size();) {
      size_t nl = u.find('\n', p);
      h += "  " + u.substr(p, nl == std::string::npos ? std::string::npos : nl - p) + "\n";
      if (nl == std::string::npos) break;
      p = nl + 1;
    }
  }
  h +=
      "  mcp [--root DIR]                    serve every command as an MCP tool over stdio\n"
      "\nglobal options:\n"
      "  --root DIR   refuse any path that resolves outside DIR (or set ACU_ROOT)\n"
      "  --json       machine-readable output (read, ls, glob, grep, stat, skim, outline, find-def, refs)\n"
      "  --version    print the version\n"
      "\nexit codes: 0 ok, 1 not found / no match, 2 usage error\n"
      "run `acu agent-guide` for a short guide to paste into an agent's instructions.\n";
  return h;
}

int runCommandArgs(Ctx& ctx, const Command& cmd, Args& args, Io& io) {
  try {
    return cmd.fn(ctx, args, io);
  } catch (const UsageError& e) {
    io.err += std::string(cmd.name) + ": " + e.msg + "\n";
    return kUsage;
  }
}

int runCommand(Ctx& ctx, const std::string& name, const std::vector<std::string>& argv, Io& io) {
  int rc;
  if (daemonForward(ctx, name, argv, io, rc)) return rc;
  return runCommandLocal(ctx, name, argv, io);
}

int runCommandLocal(Ctx& ctx, const std::string& name, const std::vector<std::string>& argv, Io& io) {
  const Command* c = findCommand(name);
  if (!c) {
    io.err += "acu: unknown command '" + name + "' (run `acu --help`)\n";
    return kUsage;
  }
  for (auto& a : argv) {
    if (a == "--") break;
    if (a == "--help") {
      io.out += commandHelp(*c);
      return kOk;
    }
  }
  Args args;
  std::string err;
  if (!parseArgs(argv, c->opts, args, err)) {
    io.err += std::string(c->name) + ": " + err + "\n" + commandHelp(*c);
    return kUsage;
  }
  return runCommandArgs(ctx, *c, args, io);
}

}  // namespace acu
