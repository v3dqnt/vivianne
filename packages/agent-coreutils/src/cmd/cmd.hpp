#pragma once
#include <string>
#include <vector>

#include "core/common.hpp"

namespace acu {

extern const Command cmd_read, cmd_head, cmd_tail, cmd_stat, cmd_write, cmd_edit, cmd_patch, cmd_mv, cmd_cp, cmd_rm,
    cmd_grep, cmd_glob, cmd_ls, cmd_find_def, cmd_refs, cmd_skim, cmd_outline, cmd_guide, cmd_index, cmd_daemon;

const std::vector<const Command*>& allCommands();
const Command* findCommand(const std::string& name);

// Parses argv for the command, runs it, converts UsageError into exit code 2. Output goes to io.
int runCommand(Ctx& ctx, const std::string& name, const std::vector<std::string>& argv, Io& io);
// Like runCommand but never hands the command to a daemon.
int runCommandLocal(Ctx& ctx, const std::string& name, const std::vector<std::string>& argv, Io& io);
int runCommandArgs(Ctx& ctx, const Command& cmd, Args& args, Io& io);

std::string helpText();
std::string commandHelp(const Command& c);

// Shared by commands that take a --json flag.
inline bool wantJson(const Args& a) { return a.has("json"); }

// Resolves the effective --max for list-style commands (0 = unlimited).
size_t capOf(const Args& a, size_t def);

}  // namespace acu
