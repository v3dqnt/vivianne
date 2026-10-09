#pragma once
// Shared types and small helpers used by every command.
#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace acu {

struct UsageError {
  std::string msg;
};

// Per-process settings shared by all commands.
class Index;
struct Snapshot;

struct Ctx {
  std::string root;  // canonical confinement root; empty = unconfined
  bool allow(const std::string& path, std::string& err) const;

  // Project index (see index/index.hpp), loaded on demand and cached; stale copies are reloaded.
  bool noIndex = false;
  std::shared_ptr<const Index> index;
  std::string indexKey;  // which index file `index` was loaded from, and its stamp

  // Set by the daemon while it serves a request: walk() answers from `snap`, indexFor() returns `pinnedIndex`.
  std::shared_ptr<const Snapshot> snap;
  std::shared_ptr<const Index> pinnedIndex;
  bool noDaemon = false;  // never forward commands to a daemon (set inside the daemon itself, and by --no-daemon)
};

// A command's streams. Output is buffered so the same code serves the CLI and the MCP server.
struct Io {
  std::string out, err, in;
  bool haveIn = false;
  const std::string& input();  // reads real stdin on first use unless `in` was supplied
};

struct OptDef {
  const char* name;  // long name, without dashes
  char sh;           // short flag or 0
  bool val;          // takes a value
};

struct Args {
  std::vector<std::string> pos;
  std::map<std::string, std::vector<std::string>> opt;
  bool has(const std::string& n) const { return opt.count(n) != 0; }
  std::string str(const std::string& n, const std::string& def = "") const;
  long num(const std::string& n, long def) const;  // throws UsageError if not a number
  std::vector<std::string> all(const std::string& n) const;
};

bool parseArgs(const std::vector<std::string>& argv, const std::vector<OptDef>& defs, Args& a, std::string& err);

struct Command {
  const char* name;
  const char* usage;
  std::vector<OptDef> opts;
  int (*fn)(Ctx&, Args&, Io&);
};

// Exit codes
constexpr int kOk = 0, kNotFound = 1, kUsage = 2;

std::string fmtInt(uint64_t n);                                  // 1,234,567
void appendTrunc(std::string& out, std::string_view s, size_t max = 500);  // cut long lines, note the rest
std::string joinPath(const std::string& a, const std::string& b);
bool startsWith(std::string_view s, std::string_view p);
bool endsWith(std::string_view s, std::string_view p);
std::string trim(std::string_view s);
std::string lower(std::string_view s);
std::string fmtTime(int64_t unixSeconds);  // 2026-10-07T12:00:00Z

// Worker threads to use: min(cores, cap), or ACU_THREADS if set.
size_t threadCount(size_t cap);

// Run `fn` for i in [0, n) on a pool of threads. Indices are handed out in order.
void parallelFor(size_t n, const std::function<void(size_t, size_t)>& fn);

}  // namespace acu
