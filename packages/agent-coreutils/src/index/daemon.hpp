#pragma once
// The optional daemon: keeps a project's index in memory, keeps its file list fresh with a file watcher, and answers
// read-only commands over a local pipe so each call skips loading the index and walking the tree.
//
// Correctness does not depend on the watcher being quick. Before answering, the daemon writes a sentinel file inside
// the project and waits until the watcher reports it; because the OS delivers events in order, every earlier change
// has been seen by then. If that wait times out, the daemon rescans the tree instead.
#include <string>
#include <vector>

#include "core/common.hpp"

namespace acu {

// Read-only commands that may be answered by a daemon.
bool daemonCanServe(const std::string& command);

// If a daemon is running for the project containing the current directory, runs the command there. Returns false
// (and leaves io untouched) when there is none, so the caller runs the command itself.
bool daemonForward(const Ctx& ctx, const std::string& command, const std::vector<std::string>& argv, Io& io, int& rc);

// Implements `acu daemon start|stop|status|run`.
int daemonCommand(Ctx& ctx, Args& a, Io& io);

}  // namespace acu
