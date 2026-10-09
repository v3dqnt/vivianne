#include "core/snapshot.hpp"

#include <algorithm>

#include "core/platform.hpp"

namespace acu {

static bool sameDir(const std::string& a, const std::string& b) {
#ifdef _WIN32
  return lower(a) == lower(b);
#else
  return a == b;
#endif
}

bool snapshotWalk(const Snapshot& s, const std::string& userRootIn, int maxDepth, std::vector<WalkEntry>& out) {
  std::string userRoot = normalizeWalkRoot(userRootIn);
  std::string abs = canonicalPath(userRoot);
  const std::string& R = s.root;
  bool same = sameDir(abs, R);
  bool inside = same || (abs.size() > R.size() && sameDir(abs.substr(0, R.size()), R) && (R.back() == '/' || abs[R.size()] == '/'));
  if (!inside) return false;
  std::string relBase = same ? "" : abs.substr(R.size() + (R.back() == '/' ? 0 : 1)) + "/";
  std::string userPrefix = userRoot == "." ? "" : userRoot + "/";
  auto it = std::lower_bound(s.files.begin(), s.files.end(), relBase,
                             [](const SnapEntry& e, const std::string& b) { return e.rel < b; });
  if (!same && (it == s.files.end() || it->rel.compare(0, relBase.size(), relBase) != 0)) {
    // either an empty directory or a plain file: let the real walk decide
    FileInfo fi;
    if (!statPath(userRoot, fi) || !fi.isDir) return false;
  }
  for (; it != s.files.end() && it->rel.compare(0, relBase.size(), relBase) == 0; ++it) {
    size_t restPos = relBase.size();
    if (maxDepth > 0) {
      int slashes = (int)std::count(it->rel.begin() + (long)restPos, it->rel.end(), '/');
      if (slashes >= maxDepth) continue;
    }
    WalkEntry e;
    e.path = userPrefix + it->rel.substr(restPos);
    e.size = it->size;
    e.mtimeNs = it->mtimeNs;
    e.mtime = it->mtimeNs / 1000000000LL;
    e.idx = it->idx;
    out.push_back(std::move(e));
  }
  return true;
}

}  // namespace acu
