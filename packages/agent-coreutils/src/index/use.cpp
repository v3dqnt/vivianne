#include "index/use.hpp"

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

std::shared_ptr<const Index> indexFor(Ctx& ctx) {
  if (ctx.noIndex || !getEnv("ACU_NO_INDEX").empty()) return nullptr;
  if (ctx.pinnedIndex) return ctx.pinnedIndex;
  std::string root;
  if (!findIndexRoot(".", root)) return nullptr;
  std::string path = indexPathFor(root);
  FileInfo fi;
  if (!statPath(path, fi)) return nullptr;
  std::string key = path + "|" + std::to_string(fi.mtimeNs) + "|" + std::to_string(fi.size);
  if (ctx.index && ctx.indexKey == key) return ctx.index;
  auto ix = std::make_shared<Index>();
  std::string err;
  if (!Index::load(path, *ix, err)) {
    ctx.index.reset();
    ctx.indexKey.clear();
    return nullptr;
  }
  ix->root = root;
  ctx.index = ix;
  ctx.indexKey = key;
  return ctx.index;
}

void annotateEntries(const Index& ix, const std::string& userRootIn, std::vector<WalkEntry>& entries) {
  std::string userRoot = normalizeWalkRoot(userRootIn);
  std::string abs = canonicalPath(userRoot);
  const std::string& I = ix.root;
  bool inside = sameDir(abs, I) || (abs.size() > I.size() && sameDir(abs.substr(0, I.size()), I) &&
                                     (I.back() == '/' || abs[I.size()] == '/'));
  if (!inside) return;
  std::string relBase;
  if (!sameDir(abs, I)) relBase = abs.substr(I.size() + (I.back() == '/' ? 0 : 1)) + "/";
  std::string userPrefix = userRoot == "." ? "" : userRoot + "/";
  size_t j = 0;
  const size_t n = ix.files.size();
  for (auto& e : entries) {
    if (e.isDir) continue;
    std::string rel;
    if (e.path == userRoot && userRoot != ".") {  // the root was a single file
      rel = relBase.empty() ? e.path : relBase.substr(0, relBase.size() - 1);
    } else if (e.path.compare(0, userPrefix.size(), userPrefix) == 0) {
      rel = relBase + e.path.substr(userPrefix.size());
    } else {
      continue;
    }
    while (j < n && ix.files[j].path < rel) ++j;
    if (j < n && ix.files[j].path == rel && ix.files[j].size == e.size && ix.files[j].mtime == e.mtimeNs) e.idx = (int)j;
  }
}

bool narrowByContent(const Index& ix, const std::vector<std::string>& literals, std::vector<WalkEntry>& entries) {
  std::vector<uint32_t> ids;
  bool narrowed = ix.candidates(literals, ids);
  std::vector<WalkEntry> kept;
  kept.reserve(entries.size());
  for (auto& e : entries) {
    if (e.idx >= 0) {
      if (ix.files[(size_t)e.idx].flags & kIdxBinary) continue;
      if (narrowed && !std::binary_search(ids.begin(), ids.end(), (uint32_t)e.idx)) continue;
    }
    kept.push_back(std::move(e));
  }
  entries = std::move(kept);
  return narrowed;
}

}  // namespace acu
