// glob, ls
#include <algorithm>
#include <map>
#include <memory>
#include <set>

#include "cmd/cmd.hpp"
#include "core/glob.hpp"
#include "core/json.hpp"
#include "core/platform.hpp"
#include "core/walk.hpp"

namespace acu {

namespace {

std::string stripDotSlash(std::string s) {
  while (s.rfind("./", 0) == 0) s = s.substr(2);
  return s;
}

// Longest leading run of literal path segments: `src/**/*.rs` -> `src`.
std::string literalPrefix(const std::string& pat) {
  std::string out;
  size_t p = 0;
  while (p < pat.size()) {
    size_t slash = pat.find('/', p);
    if (slash == std::string::npos) break;
    std::string seg = pat.substr(p, slash - p);
    if (hasGlobChars(seg)) break;
    out += (out.empty() ? "" : "/") + seg;
    p = slash + 1;
  }
  return out;
}

int cmdGlob(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.empty()) throw UsageError{"no pattern given: glob <pattern>..."};
  size_t max = capOf(a, 500);
  std::string base = a.str("in", ".");
  bool json = wantJson(a), wantDirs = a.has("dirs");

  struct P {
    std::string pat;
    bool anyDepth;  // no '/' in the pattern: match the file name at any depth
    std::string prefix;
  };
  std::vector<P> pats;
  for (auto& raw : a.pos)
    for (auto& e : expandBraces(stripDotSlash(raw))) {
      if (e.empty()) continue;
      bool anyDepth = e.find('/') == std::string::npos;
      pats.push_back({e, anyDepth, anyDepth ? "" : literalPrefix(e)});
    }
  if (pats.empty()) throw UsageError{"empty pattern"};

  // Walk only the directories the patterns can reach.
  std::set<std::string> roots;
  for (auto& p : pats) roots.insert(p.prefix);
  if (roots.count("")) roots = {""};
  std::vector<WalkEntry> found;
  std::string err;
  for (auto& r : roots) {
    WalkOpts wo;
    wo.root = r.empty() ? base : joinPath(base, r);
    wo.noIgnore = a.has("no-ignore");
    wo.wantDirs = wantDirs;
    std::vector<WalkEntry> part;
    if (!walk(wo, ctx, part, err)) {
      if (r.empty()) {
        io.err += "glob: " + base + ": " + err + "\n";
        return kUsage;
      }
      continue;  // a pattern's directory does not exist: it simply matches nothing
    }
    for (auto& e : part) found.push_back(std::move(e));
  }
  std::vector<std::string> hits;
  std::string prefixCut = base == "." ? "" : stripDotSlash(base) + "/";
  for (auto& e : found) {
    std::string rel = e.path;
    if (!prefixCut.empty() && rel.compare(0, prefixCut.size(), prefixCut) == 0) rel = rel.substr(prefixCut.size());
    std::string_view name = rel;
    size_t sl = rel.rfind('/');
    if (sl != std::string::npos) name = std::string_view(rel).substr(sl + 1);
    for (auto& p : pats) {
      if (p.anyDepth ? globMatch(p.pat, name) : globMatch(p.pat, rel)) {
        hits.push_back(e.isDir ? e.path + "/" : e.path);
        break;
      }
    }
  }
  std::sort(hits.begin(), hits.end());
  hits.erase(std::unique(hits.begin(), hits.end()), hits.end());
  if (hits.empty()) return kNotFound;
  size_t shown = std::min(max, hits.size());
  if (json) {
    Json arr = Json::array();
    for (size_t i = 0; i < shown; ++i) arr.push(Json::str(hits[i]));
    Json root = Json::object();
    root.set("schema", Json::num(1));
    root.set("paths", std::move(arr));
    root.set("total", Json::num((double)hits.size()));
    root.set("truncated", Json::boolean(shown < hits.size()));
    io.out += root.dump() + "\n";
    return kOk;
  }
  for (size_t i = 0; i < shown; ++i) io.out += hits[i] + "\n";
  if (shown < hits.size())
    io.out += "\xE2\x80\xA6 " + fmtInt(hits.size() - shown) + " more path" + (hits.size() - shown == 1 ? "" : "s") +
              "; use --max " + std::to_string(hits.size()) + " to list all\n";
  return kOk;
}

// ---------------------------------------------------------------------- ls ---

struct Node {
  bool isDir = false;
  std::map<std::string, std::unique_ptr<Node>> kids;
};

void printTree(const Node& n, int indent, size_t& budget, size_t& skipped, std::string& out) {
  std::vector<const std::pair<const std::string, std::unique_ptr<Node>>*> dirs, files;
  for (auto& kv : n.kids) (kv.second->isDir ? dirs : files).push_back(&kv);
  for (auto* group : {&dirs, &files})
    for (auto* kv : *group) {
      if (budget == 0) {
        ++skipped;
        continue;
      }
      --budget;
      out.append((size_t)indent * 2, ' ');
      out += kv->first;
      if (kv->second->isDir) out += "/";
      out += "\n";
      if (kv->second->isDir) printTree(*kv->second, indent + 1, budget, skipped, out);
    }
}

size_t countAll(const Node& n) {
  size_t c = 0;
  for (auto& kv : n.kids) c += 1 + countAll(*kv.second);
  return c;
}

int cmdLs(Ctx& ctx, Args& a, Io& io) {
  std::string dir = a.pos.empty() ? "." : a.pos[0];
  if (a.pos.size() > 1) throw UsageError{"expected at most one directory"};
  size_t max = capOf(a, 500);
  bool recursive = a.has("recursive") || a.has("depth");
  long depth = a.has("depth") ? a.num("depth", 1) : (recursive ? -1 : 1);
  if (a.has("depth") && depth < 1) throw UsageError{"--depth must be at least 1"};
  bool json = wantJson(a);

  WalkOpts wo;
  wo.root = dir;
  wo.noIgnore = a.has("no-ignore");
  wo.wantDirs = true;
  wo.maxDepth = (int)depth;
  std::vector<WalkEntry> entries;
  std::string err;
  FileInfo fi;
  if (!ctx.allow(dir, err) || !walk(wo, ctx, entries, err)) {
    io.err += "ls: " + dir + ": " + err + "\n";
    return kNotFound;
  }
  if (statPath(dir, fi) && !fi.isDir) {  // ls of a file prints the file
    io.out += dir + "\n";
    return kOk;
  }
  std::string prefix = dir == "." ? "" : stripDotSlash(dir);
  while (prefix.size() > 1 && prefix.back() == '/') prefix.pop_back();
  if (!prefix.empty()) prefix += "/";
  Node root;
  root.isDir = true;
  for (auto& e : entries) {
    std::string rel = e.path;
    if (!prefix.empty() && rel.compare(0, prefix.size(), prefix) == 0) rel = rel.substr(prefix.size());
    Node* cur = &root;
    size_t p = 0;
    while (p < rel.size()) {
      size_t sl = rel.find('/', p);
      std::string seg = rel.substr(p, sl == std::string::npos ? std::string::npos : sl - p);
      bool last = sl == std::string::npos;
      auto& slot = cur->kids[seg];
      if (!slot) slot = std::make_unique<Node>();
      if (!last || e.isDir) slot->isDir = true;
      cur = slot.get();
      if (last) break;
      p = sl + 1;
    }
  }
  if (root.kids.empty()) {
    if (!json) io.out += "(empty)\n";
    else io.out += "{\"schema\":1,\"entries\":[],\"total\":0,\"truncated\":false}\n";
    return kOk;
  }
  size_t total = countAll(root);
  if (json) {
    Json arr = Json::array();
    for (size_t i = 0; i < std::min(total, max) && i < entries.size(); ++i)
      arr.push(Json::str(entries[i].isDir ? entries[i].path + "/" : entries[i].path));
    Json r = Json::object();
    r.set("schema", Json::num(1));
    r.set("entries", std::move(arr));
    r.set("total", Json::num((double)entries.size()));
    r.set("truncated", Json::boolean(entries.size() > max));
    io.out += r.dump() + "\n";
    return kOk;
  }
  size_t budget = max, skipped = 0;
  printTree(root, 0, budget, skipped, io.out);
  if (skipped)
    io.out += "\xE2\x80\xA6 " + fmtInt(skipped) + " more entr" + (skipped == 1 ? "y" : "ies") + "; use --max " +
              std::to_string(total) + " to list all" + (recursive ? "" : "") + "\n";
  return kOk;
}

}  // namespace

const Command cmd_glob = {"glob",
                          "glob <pattern>... [--in DIR] [--dirs]    find files by name: **/*.test.ts, src/**/{a,b}.rs (no '/' = any depth)",
                          {{"max", 'm', true}, {"in", 0, true}, {"dirs", 0, false}, {"no-ignore", 0, false}, {"json", 0, false}},
                          cmdGlob};
const Command cmd_ls = {"ls", "ls [dir] [-R] [--depth N]                list a directory; -R prints a compact tree",
                        {{"recursive", 'R', false}, {"depth", 'd', true}, {"max", 'm', true}, {"no-ignore", 0, false}, {"json", 0, false}},
                        cmdLs};

}  // namespace acu
