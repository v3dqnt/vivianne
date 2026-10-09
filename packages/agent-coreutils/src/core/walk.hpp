#pragma once
#include <cstdint>
#include <string>
#include <vector>

#include "core/common.hpp"

namespace acu {

struct WalkEntry {
  std::string path;  // as the user would type it: prefix of the root they gave, '/' separators
  bool isDir = false;
  uint64_t size = 0;
  int64_t mtime = 0;    // unix seconds
  int64_t mtimeNs = 0;  // unix nanoseconds
  int idx = -1;         // entry in the project index when its size and mtime match, else -1
};

struct WalkOpts {
  std::string root = ".";
  bool noIgnore = false;
  bool wantDirs = false;
  int maxDepth = -1;  // -1 = unlimited; 1 = direct children only
  bool wantStat = false;  // fill size and mtime for files (free on Windows; one stat each elsewhere)
};

// The root as walk() will spell it in results: "./" prefixes and trailing slashes removed.
std::string normalizeWalkRoot(const std::string& root);

// Would a walk of `rootAbs` skip `rel` (a path below it, '/' separated)? Applies .gitignore, .ignore and .acuignore
// rules from the repository root down, and always skips .git and .acu.
bool pathIgnored(const std::string& rootAbs, const std::string& rel, bool isDir);

// Parallel directory walk with ignore rules applied on the way down. Results are sorted by path.
// If `root` is a file it is returned as the only entry. Returns false (with err) if root is missing.
bool walk(const WalkOpts& o, const Ctx& ctx, std::vector<WalkEntry>& out, std::string& err);

}  // namespace acu
