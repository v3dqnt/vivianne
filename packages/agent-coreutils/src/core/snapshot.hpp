#pragma once
// An in-memory file list kept fresh by the daemon's file watcher. When a Ctx carries one, walk() answers from it
// instead of listing directories.
#include <cstdint>
#include <string>
#include <vector>

#include "core/walk.hpp"

namespace acu {

struct SnapEntry {
  std::string rel;  // relative to Snapshot::root, '/' separators
  uint64_t size = 0;
  int64_t mtimeNs = 0;
  int idx = -1;  // entry in the base index when unchanged since it was built, else -1
};

struct Snapshot {
  std::string root;  // absolute, canonical
  std::vector<SnapEntry> files;  // sorted by rel
};

// Fills `out` with the files under userRoot (as walk() would spell them). Returns false if the snapshot cannot
// answer (userRoot outside the snapshot, or a file).
bool snapshotWalk(const Snapshot& s, const std::string& userRoot, int maxDepth, std::vector<WalkEntry>& out);

}  // namespace acu
