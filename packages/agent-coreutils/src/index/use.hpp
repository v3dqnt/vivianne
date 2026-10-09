#pragma once
// How commands consult the index: find it, check which walked files it knows, and narrow content searches.
#include <memory>
#include <string>
#include <vector>

#include "core/common.hpp"
#include "core/walk.hpp"
#include "index/index.hpp"

namespace acu {

// The project index covering the current directory, or null (none built, disabled by --no-index / ACU_NO_INDEX,
// unreadable). Cached in ctx and reloaded when the file on disk changes.
std::shared_ptr<const Index> indexFor(Ctx& ctx);

// indexFor, unless the command was given --no-index.
inline std::shared_ptr<const Index> indexFor(Ctx& ctx, const Args& a) {
  return a.has("no-index") ? nullptr : indexFor(ctx);
}

// Sets WalkEntry::idx for entries (from walk(root=userRoot)) whose size and mtime match the index.
void annotateEntries(const Index& ix, const std::string& userRoot, std::vector<WalkEntry>& entries);

// Drops entries the index knows cannot contain any of `literals` (and binary files). Entries without an index match
// are kept. Returns true if it narrowed the list using trigrams.
bool narrowByContent(const Index& ix, const std::vector<std::string>& literals, std::vector<WalkEntry>& entries);

}  // namespace acu
