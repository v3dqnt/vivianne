#include "core/walk.hpp"

#include <algorithm>
#include <condition_variable>
#include <deque>
#include <mutex>
#include <thread>

#include "core/ignore.hpp"
#include "core/platform.hpp"
#include "core/snapshot.hpp"

namespace acu {

namespace {

struct Task {
  std::string dirPath;    // what to list
  std::string outPrefix;  // "" or "dir/"
  std::string igRel;      // path relative to the top of the ignore chain
  IgnorePtr ig;
  int depth = 0;
};

const char* kIgnoreNames[] = {".gitignore", ".ignore", ".acuignore"};

IgnorePtr loadIgnoreFiles(const std::string& dirPath, const std::vector<std::string>& names, IgnorePtr parent,
                          size_t baseLen) {
  auto set = std::make_shared<IgnoreSet>();
  set->parent = parent;
  set->baseLen = baseLen;
  for (auto& n : names) {
    std::string content, err;
    if (readFile(joinPath(dirPath, n), content, err)) parseIgnoreFile(content, set->rules);
  }
  if (set->rules.empty()) return parent;
  return set;
}

struct Walker {
  const WalkOpts& o;
  const Ctx& ctx;
  Walker(const WalkOpts& opts, const Ctx& c) : o(opts), ctx(c) {}
  std::mutex mu;
  std::condition_variable cv;
  std::deque<Task> queue;
  size_t pending = 0;
  std::vector<std::vector<WalkEntry>> results;

  void push(Task t) {
    std::lock_guard<std::mutex> lk(mu);
    queue.push_back(std::move(t));
    ++pending;
    cv.notify_one();
  }

  void process(const Task& t, std::vector<WalkEntry>& res) {
    std::vector<DirEntry> entries;
    if (!listDir(t.dirPath, entries)) return;
    IgnorePtr ig = t.ig;
    if (!o.noIgnore) {
      std::vector<std::string> found;
      for (auto& e : entries) {
        if (e.isDir) continue;
        for (const char* n : kIgnoreNames)
          if (e.name == n) found.push_back(e.name);
      }
      if (!found.empty()) {
        std::sort(found.begin(), found.end(), [](const std::string& a, const std::string& b) {
          // .gitignore first so .ignore/.acuignore can override it
          auto rank = [](const std::string& s) { return s == ".gitignore" ? 0 : s == ".ignore" ? 1 : 2; };
          return rank(a) < rank(b);
        });
        ig = loadIgnoreFiles(t.dirPath, found, ig, t.igRel.empty() ? 0 : t.igRel.size() + 1);
      }
    }
    for (auto& e : entries) {
      if (e.isDir && (e.name == ".git" || e.name == ".acu")) continue;
      std::string igPath = t.igRel.empty() ? e.name : t.igRel + "/" + e.name;
      if (!o.noIgnore && ignoreDecision(ig.get(), igPath, e.isDir) == 1) continue;
      std::string outPath = t.outPrefix + e.name;
      if (e.isDir) {
        if (e.isLink) continue;  // never follow directory links
        if (o.wantDirs) res.push_back({outPath, true, 0, e.mtime, e.mtimeNs, -1});
        if (o.maxDepth < 0 || t.depth + 1 < o.maxDepth)
          push({joinPath(t.dirPath, e.name), outPath + "/", igPath, ig, t.depth + 1});
      } else {
        if (e.isLink && !ctx.root.empty()) {
          std::string why;
          if (!ctx.allow(outPath, why)) continue;
        }
#ifndef _WIN32
        if (o.wantStat) {
          FileInfo fi;
          if (statPath(joinPath(t.dirPath, e.name), fi)) e.size = fi.size, e.mtime = fi.mtime, e.mtimeNs = fi.mtimeNs;
        }
#endif
        res.push_back({outPath, false, e.size, e.mtime, e.mtimeNs, -1});
      }
    }
  }

  void worker(size_t id) {
    for (;;) {
      Task t;
      {
        std::unique_lock<std::mutex> lk(mu);
        cv.wait(lk, [&] { return !queue.empty() || pending == 0; });
        if (queue.empty()) return;
        t = std::move(queue.front());
        queue.pop_front();
      }
      process(t, results[id]);
      {
        std::lock_guard<std::mutex> lk(mu);
        if (--pending == 0) cv.notify_all();
      }
    }
  }
};

// Ignore sets for the directories above `root`, up to the repository root (the directory holding .git).
IgnorePtr ancestorIgnores(const std::string& rootAbs, std::string& igRelOut) {
  std::vector<std::string> chain;  // from root upwards
  std::string cur = rootAbs;
  bool found = false;
  for (int i = 0; i < 64; ++i) {
    chain.push_back(cur);
    FileInfo fi;
    if (statPath(joinPath(cur, ".git"), fi)) {
      found = true;
      break;
    }
    size_t slash = cur.rfind('/');
    if (slash == std::string::npos || slash == 0 || (slash == 2 && cur[1] == ':')) break;
    cur = cur.substr(0, slash);
  }
  if (!found) {
    igRelOut.clear();
    return nullptr;
  }
  // chain.back() is the repo root. Walk down from it to (but excluding) root.
  IgnorePtr ig;
  const std::string& top = chain.back();
  for (size_t k = chain.size(); k-- > 1;) {
    const std::string& dir = chain[k];
    std::vector<std::string> names(std::begin(kIgnoreNames), std::end(kIgnoreNames));
    size_t baseLen = dir.size() == top.size() ? 0 : dir.size() - top.size();  // length of "sub/" below the top
    ig = loadIgnoreFiles(dir, names, ig, baseLen);
  }
  igRelOut = rootAbs.size() == top.size() ? "" : rootAbs.substr(top.size() + 1);
  return ig;
}

}  // namespace

bool pathIgnored(const std::string& rootAbs, const std::string& rel, bool isDir) {
  std::string igRel;
  IgnorePtr ig = ancestorIgnores(rootAbs, igRel);
  std::vector<std::string> parts;
  for (size_t p = 0; p <= rel.size();) {
    size_t sl = rel.find('/', p);
    parts.push_back(rel.substr(p, sl == std::string::npos ? std::string::npos : sl - p));
    if (sl == std::string::npos) break;
    p = sl + 1;
  }
  std::string dir = rootAbs, cur = igRel;
  std::vector<std::string> names(std::begin(kIgnoreNames), std::end(kIgnoreNames));
  for (size_t i = 0; i < parts.size(); ++i) {
    if (parts[i] == ".git" || parts[i] == ".acu") return true;
    ig = loadIgnoreFiles(dir, names, ig, cur.empty() ? 0 : cur.size() + 1);
    std::string next = cur.empty() ? parts[i] : cur + "/" + parts[i];
    bool last = i + 1 == parts.size();
    if (ignoreDecision(ig.get(), next, last ? isDir : true) == 1) return true;
    dir = joinPath(dir, parts[i]);
    cur = next;
  }
  return false;
}

std::string normalizeWalkRoot(const std::string& in) {
  std::string root = in.empty() ? "." : in;
  while (root.size() > 2 && root.back() == '/') root.pop_back();
  if (root.rfind("./", 0) == 0) root = root.substr(2);
  if (root.empty()) root = ".";
  return root;
}

bool walk(const WalkOpts& o, const Ctx& ctx, std::vector<WalkEntry>& out, std::string& err) {
  std::string root = normalizeWalkRoot(o.root);
  if (!ctx.allow(root, err)) return false;
  FileInfo fi;
  if (!statPath(root, fi)) {
    err = "not found";
    return false;
  }
  if (!fi.isDir) {
    out.push_back({root, false, fi.size, fi.mtime, fi.mtimeNs, -1});
    return true;
  }

  if (ctx.snap && !o.noIgnore && !o.wantDirs && snapshotWalk(*ctx.snap, root, o.maxDepth, out)) return true;

  Walker w(o, ctx);
  std::string igRel;
  IgnorePtr ig;
  if (!o.noIgnore) {
    std::string abs = absolutePath(root);
    // find the repo root by looking for .git above and including root
    std::string rootCopy = abs;
    ig = ancestorIgnores(rootCopy, igRel);
  }
  size_t nthreads = threadCount(8);
  w.results.resize(nthreads);
  w.push({root, root == "." ? "" : root + "/", igRel, ig, 0});
  std::vector<std::thread> pool;
  for (size_t i = 1; i < nthreads; ++i) pool.emplace_back([&w, i] { w.worker(i); });
  w.worker(0);
  for (auto& t : pool) t.join();
  for (auto& r : w.results) {
    out.insert(out.end(), std::make_move_iterator(r.begin()), std::make_move_iterator(r.end()));
  }
  std::sort(out.begin(), out.end(), [](const WalkEntry& a, const WalkEntry& b) { return a.path < b.path; });
  return true;
}

}  // namespace acu
