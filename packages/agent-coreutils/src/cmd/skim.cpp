// skim, outline, find-def, refs
#include <algorithm>
#include <atomic>
#include <cstring>
#include <map>
#include <set>

#include "cmd/cmd.hpp"
#include "cmd/search.hpp"
#include "core/json.hpp"
#include "core/lang.hpp"
#include "core/platform.hpp"
#include "core/text.hpp"
#include "core/walk.hpp"
#include "index/use.hpp"

namespace acu {

namespace {

constexpr uint64_t kMaxSymbolFile = 8ull << 20;  // skip generated bundles

bool loadText(Ctx& ctx, const std::string& path, std::string& text, TextInfo& ti, std::string& err) {
  if (!ctx.allow(path, err)) return false;
  std::string raw;
  if (!readFile(path, raw, err)) return false;
  toDisplayText(raw, text, ti);
  return true;
}

std::string_view srcLine(const std::string& text, const std::vector<size_t>& st, size_t line1) {
  return line1 >= 1 && line1 < st.size() ? lineAt(text, st, line1 - 1) : std::string_view();
}

// ---------------------------------------------------------------- outline ---

int cmdOutline(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.empty()) throw UsageError{"no file given: outline <file>..."};
  bool json = wantJson(a), multi = a.pos.size() > 1;
  int rc = kOk;
  Json files = Json::array();
  for (auto& path : a.pos) {
    std::string text, err;
    TextInfo ti;
    if (!loadText(ctx, path, text, ti, err)) {
      io.err += "outline: " + path + ": " + err + "\n";
      rc = kNotFound;
      continue;
    }
    if (ti.binary) {
      io.err += "outline: " + path + ": binary file\n";
      rc = kNotFound;
      continue;
    }
    LangInfo li = detectLanguage(path);
    std::vector<Symbol> syms;
    extractSymbols(li.family, text, syms);
    size_t lines = countLines(text);
    if (json) {
      Json f = Json::object(), arr = Json::array();
      f.set("path", Json::str(path));
      f.set("language", Json::str(li.name));
      f.set("lines", Json::num((double)lines));
      for (auto& s : syms) {
        Json j = Json::object();
        j.set("line", Json::num(s.line));
        j.set("depth", Json::num(s.depth));
        j.set("kind", Json::str(s.kind));
        j.set("name", Json::str(s.name));
        arr.push(std::move(j));
      }
      f.set("symbols", std::move(arr));
      files.push(std::move(f));
      continue;
    }
    if (multi) io.out += "== " + path + " ==\n";
    if (li.family == Family::None && !*li.name) io.out += "(unknown language; no outline)\n";
    else if (li.family == Family::None) io.out += std::string("(") + li.name + ": no outline for this language, " + fmtInt(lines) + " lines)\n";
    else if (syms.empty()) io.out += "(no symbols found, " + fmtInt(lines) + " lines)\n";
    for (auto& s : syms) {
      io.out += std::to_string(s.line) + ":";
      io.out.append(1 + (size_t)s.depth * 2, ' ');
      io.out += s.kind + " " + s.name + "\n";
    }
  }
  if (json) {
    Json r = Json::object();
    r.set("schema", Json::num(1));
    r.set("files", std::move(files));
    io.out += r.dump() + "\n";
  }
  return rc;
}

// -------------------------------------------------------------- find-def ---

bool nameMatches(const std::string& name, const std::string& sym, bool icase) {
  auto eq = [&](std::string_view x, std::string_view y) { return icase ? lower(x) == lower(y) : x == y; };
  if (eq(name, sym)) return true;
  size_t cut = name.find_last_of(".:");  // Type.Method, Type::method -> Method
  return cut != std::string::npos && eq(std::string_view(name).substr(cut + 1), sym);
}

struct DefHit {
  std::string path;
  uint32_t line;
  std::string kind, text;
};

// Collects symbol definitions named `sym` across `files` (those whose text could contain it).
std::vector<DefHit> findDefs(Ctx& ctx, const std::vector<WalkEntry>& files, const std::string& sym, bool icase,
                             const Index* ix) {
  std::vector<std::vector<DefHit>> per(files.size());
  std::string needle = icase ? lower(sym) : sym;
  parallelFor(files.size(), [&](size_t i, size_t) {
    LangInfo li = detectLanguage(files[i].path);
    if (li.family == Family::None || files[i].size > kMaxSymbolFile) return;
    thread_local auto* rawP = new std::string;
    thread_local auto* textP = new std::string;
    std::string &raw = *rawP, &text = *textP;
    std::string err;
    if (ix && files[i].idx >= 0) {  // symbols come from the index; the file is read only to show a hit's line
      const IndexedFile& f = ix->files[(size_t)files[i].idx];
      std::vector<const Symbol*> hits;
      for (uint32_t k = f.symBegin; k < f.symEnd; ++k)
        if (nameMatches(ix->symbols[k].name, sym, icase)) hits.push_back(&ix->symbols[k]);
      if (hits.empty() || !readFile(files[i].path, raw, err)) return;
      TextInfo ti;
      toDisplayText(raw, text, ti);
      std::vector<size_t> st = lineStarts(text);
      for (const Symbol* s : hits) {
        std::string t;
        appendTrunc(t, srcLine(text, st, s->line), 200);
        per[i].push_back({files[i].path, s->line, s->kind, trim(t)});
      }
      return;
    }
    if (!readFile(files[i].path, raw, err)) return;
    TextInfo ti;
    if (icase ? lower(raw).find(needle) == std::string::npos : raw.find(needle) == std::string::npos) return;
    toDisplayText(raw, text, ti);
    if (ti.binary) return;
    std::vector<Symbol> syms;
    extractSymbols(li.family, text, syms);
    std::vector<size_t> st;
    for (auto& s : syms) {
      if (!nameMatches(s.name, sym, icase)) continue;
      if (st.empty()) st = lineStarts(text);
      std::string t;
      appendTrunc(t, srcLine(text, st, s.line), 200);
      per[i].push_back({files[i].path, s.line, s.kind, trim(t)});
    }
  });
  std::vector<DefHit> all;
  for (auto& p : per)
    for (auto& h : p) all.push_back(std::move(h));
  return all;
}

bool collectFiles(Ctx& ctx, const std::vector<std::string>& paths, bool noIgnore, std::vector<WalkEntry>& files,
                  Io& io, const char* cmd, const Index* ix) {
  int failed = 0;
  for (auto& p : paths.empty() ? std::vector<std::string>{"."} : paths) {
    WalkOpts wo;
    wo.root = p;
    wo.noIgnore = noIgnore;
    wo.wantStat = ix != nullptr;
    std::string e;
    std::vector<WalkEntry> part;
    if (!walk(wo, ctx, part, e)) {
      io.err += std::string(cmd) + ": " + p + ": " + e + "\n";
      ++failed;
      continue;
    }
    if (ix) annotateEntries(*ix, p, part);
    for (auto& x : part) files.push_back(std::move(x));
  }
  return failed == 0 || !files.empty();
}

int cmdFindDef(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.empty()) throw UsageError{"no symbol given: find-def <symbol> [paths]"};
  std::string sym = a.pos[0];
  std::vector<std::string> paths(a.pos.begin() + 1, a.pos.end());
  size_t max = capOf(a, 50);
  bool json = wantJson(a);
  auto ix = indexFor(ctx, a);
  std::vector<WalkEntry> files;
  if (!collectFiles(ctx, paths, a.has("no-ignore"), files, io, "find-def", ix.get())) return kUsage;
  std::vector<DefHit> defs = findDefs(ctx, files, sym, a.has("ignore-case"), ix.get());
  if (defs.empty()) return kNotFound;
  size_t shown = std::min(max, defs.size());
  if (json) {
    Json arr = Json::array();
    for (size_t i = 0; i < shown; ++i) {
      Json j = Json::object();
      j.set("path", Json::str(defs[i].path));
      j.set("line", Json::num(defs[i].line));
      j.set("kind", Json::str(defs[i].kind));
      j.set("text", Json::str(defs[i].text));
      arr.push(std::move(j));
    }
    Json r = Json::object();
    r.set("schema", Json::num(1));
    r.set("definitions", std::move(arr));
    r.set("total", Json::num((double)defs.size()));
    io.out += r.dump() + "\n";
    return kOk;
  }
  for (size_t i = 0; i < shown; ++i)
    io.out += defs[i].path + ":" + std::to_string(defs[i].line) + ": [" + defs[i].kind + "] " + defs[i].text + "\n";
  if (shown < defs.size()) io.out += "\xE2\x80\xA6 " + fmtInt(defs.size() - shown) + " more; use --max " + std::to_string(defs.size()) + "\n";
  return kOk;
}

// ------------------------------------------------------------------ refs ---

bool looksLikeImport(std::string_view l) {
  size_t i = 0;
  while (i < l.size() && (l[i] == ' ' || l[i] == '\t')) ++i;
  l.remove_prefix(i);
  // statements that bring a name in from elsewhere
  for (const char* p : {"import ", "import{", "from ", "use ", "#include", "using ", "extern crate", "package "})
    if (l.rfind(p, 0) == 0) return true;
  // re-exports and CommonJS / dynamic imports
  if (l.rfind("export", 0) == 0 && l.find(" from ") != std::string_view::npos) return true;
  return l.find("require(") != std::string_view::npos || l.find("import(") != std::string_view::npos;
}

int cmdRefs(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.empty()) throw UsageError{"no symbol given: refs <symbol> [paths]"};
  std::string sym = a.pos[0];
  std::vector<std::string> paths(a.pos.begin() + 1, a.pos.end());
  size_t max = capOf(a, 100);
  bool json = wantJson(a);
  auto ix = indexFor(ctx, a);
  std::vector<WalkEntry> files;
  if (!collectFiles(ctx, paths, a.has("no-ignore"), files, io, "refs", ix.get())) return kUsage;
  if (ix) narrowByContent(*ix, {sym}, files);  // only files whose trigrams include the symbol are read

  Regex re;
  std::string err;
  if (!buildMatcher({sym}, true, false, true, re, err)) {
    io.err += "refs: " + err + "\n";
    return kUsage;
  }
  struct Ref {
    std::string path;
    uint32_t line;
    std::string text;
    int cls;  // 0 definition, 1 import, 2 reference
  };
  std::vector<std::vector<Ref>> per(files.size());
  parallelFor(files.size(), [&](size_t i, size_t) {
    if (files[i].size > (64u << 20) || isBinaryExtension(files[i].path)) return;
    thread_local auto* rawP = new std::string;
    thread_local auto* textP = new std::string;
    std::string &raw = *rawP, &text = *textP;
    std::string e;
    if (!readFile(files[i].path, raw, e)) return;
    if (raw.find(sym) == std::string::npos) return;
    TextInfo ti;
    toDisplayText(raw, text, ti);
    if (ti.binary) return;
    LangInfo li = detectLanguage(files[i].path);
    std::set<uint32_t> defLines;
    if (li.family != Family::None && files[i].size <= kMaxSymbolFile) {
      std::vector<Symbol> syms;
      extractSymbols(li.family, text, syms);
      for (auto& s : syms)
        if (nameMatches(s.name, sym, false)) defLines.insert(s.line);
    }
    size_t lineNo = 1, counted = 0;
    searchBuffer(re, text, 0, [&](size_t ls, size_t le) {
      lineNo += (size_t)std::count(text.data() + counted, text.data() + ls, '\n');
      counted = ls;
      std::string_view l = std::string_view(text).substr(ls, le - ls);
      if (!l.empty() && l.back() == '\r') l.remove_suffix(1);
      int cls = defLines.count((uint32_t)lineNo) ? 0 : looksLikeImport(l) ? 1 : 2;
      std::string t;
      appendTrunc(t, trim(l), 200);
      per[i].push_back({files[i].path, (uint32_t)lineNo, std::move(t), cls});
      return true;
    });
  });
  std::vector<Ref> all[3];
  for (auto& p : per)
    for (auto& r : p) all[r.cls].push_back(std::move(r));
  size_t total = all[0].size() + all[1].size() + all[2].size();
  if (!total) return kNotFound;
  static const char* titles[3] = {"definitions", "imports", "references"};
  size_t budget = max, shown = 0;
  Json root = Json::object();
  root.set("schema", Json::num(1));
  for (int c = 0; c < 3; ++c) {
    if (all[c].empty()) continue;
    Json arr = Json::array();
    if (!json) io.out += std::string(titles[c]) + " (" + fmtInt(all[c].size()) + "):\n";
    for (auto& r : all[c]) {
      if (!budget) break;
      --budget;
      ++shown;
      if (json) {
        Json j = Json::object();
        j.set("path", Json::str(r.path));
        j.set("line", Json::num(r.line));
        j.set("text", Json::str(r.text));
        arr.push(std::move(j));
      } else {
        io.out += "  " + r.path + ":" + std::to_string(r.line) + ": " + r.text + "\n";
      }
    }
    if (json) root.set(titles[c], std::move(arr));
  }
  if (json) {
    root.set("total", Json::num((double)total));
    root.set("truncated", Json::boolean(shown < total));
    io.out += root.dump() + "\n";
  } else if (shown < total) {
    io.out += "\xE2\x80\xA6 " + fmtInt(total - shown) + " more; use --max " + std::to_string(total) + "\n";
  }
  return kOk;
}

// ------------------------------------------------------------------ skim ---

struct FileNode {
  std::string name, lang;
  size_t lines = 0;
  std::vector<Symbol> syms;  // depth-0 symbols only
};

struct DirNode {
  std::string name;
  std::map<std::string, DirNode> dirs;
  std::vector<FileNode> files;
  size_t totalFiles = 0, totalLines = 0;
  std::map<std::string, size_t> langLines;
};

void aggregate(DirNode& d) {
  d.totalFiles = d.files.size();
  d.totalLines = 0;
  d.langLines.clear();
  for (auto& f : d.files) {
    d.totalLines += f.lines;
    if (!f.lang.empty()) d.langLines[f.lang] += f.lines;
  }
  for (auto& [k, c] : d.dirs) {
    aggregate(c);
    d.totalFiles += c.totalFiles;
    d.totalLines += c.totalLines;
    for (auto& [l, n] : c.langLines) d.langLines[l] += n;
  }
}

std::string langMix(const DirNode& d) {
  std::vector<std::pair<std::string, size_t>> v(d.langLines.begin(), d.langLines.end());
  std::sort(v.begin(), v.end(), [](auto& x, auto& y) { return x.second != y.second ? x.second > y.second : x.first < y.first; });
  size_t sum = 0;
  for (auto& p : v) sum += p.second;
  std::string o;
  int shown = 0;
  for (auto& p : v) {
    if (!sum) break;
    int pct = (int)((p.second * 100 + sum / 2) / sum);
    if (pct < 5 && shown > 0) break;
    if (shown == 3) break;
    o += (shown ? ", " : "") + p.first + " " + std::to_string(std::max(pct, 1)) + "%";
    ++shown;
  }
  return o;
}

struct Cfg {
  int depth;       // directory levels to expand below the root
  size_t symLimit;
  size_t fileLimit;
};

int symPriority(const Symbol& s) {
  static const char* hi[] = {"class", "struct", "interface", "trait", "enum", "impl", "mod", "namespace", "function",
                             "fn", "func", "type", "object", "record", "module", "protocol", "method", "union"};
  for (const char* k : hi)
    if (s.kind == k) return 0;
  return 1;
}

std::string fileLine(const FileNode& f, size_t symLimit) {
  std::string o = f.name + " (" + fmtInt(f.lines) + ")";
  if (symLimit == 0 || f.syms.empty()) return o;
  std::vector<Symbol> pick;
  for (auto& s : f.syms)
    if (f.lang != "Markdown" || s.depth <= 1) pick.push_back(s);
  std::stable_sort(pick.begin(), pick.end(), [](const Symbol& x, const Symbol& y) { return symPriority(x) < symPriority(y); });
  size_t extra = pick.size() > symLimit ? pick.size() - symLimit : 0;
  if (pick.size() > symLimit) pick.resize(symLimit);
  std::sort(pick.begin(), pick.end(), [](const Symbol& x, const Symbol& y) { return x.line < y.line; });
  o += "  ";
  for (size_t i = 0; i < pick.size(); ++i) {
    if (i) o += " \xC2\xB7 ";
    o += pick[i].name.size() > 40 ? pick[i].name.substr(0, 40) + "\xE2\x80\xA6" : pick[i].name;
  }
  if (extra) o += " \xC2\xB7 +" + std::to_string(extra);
  return o;
}

void renderDir(const DirNode& d, const std::string& label, int level, const Cfg& cfg, std::string& out, bool isRoot) {
  std::string indent((size_t)level * 2, ' ');
  out += indent + label + "/ (" + fmtInt(d.totalFiles) + (d.totalFiles == 1 ? " file, " : " files, ") + fmtInt(d.totalLines) + " lines)";
  if (isRoot || d.totalFiles >= 5) {
    std::string mix = langMix(d);
    if (!mix.empty()) out += "  " + mix;
  }
  out += "\n";
  if (level >= cfg.depth) return;

  // files, most substantial first when there are too many
  std::vector<const FileNode*> files;
  for (auto& f : d.files) files.push_back(&f);
  std::vector<const FileNode*> shown = files;
  if (shown.size() > cfg.fileLimit) {
    std::stable_sort(shown.begin(), shown.end(), [](auto* x, auto* y) { return x->lines > y->lines; });
    shown.resize(cfg.fileLimit);
  }
  std::vector<const DirNode*> dirs;
  for (auto& [k, c] : d.dirs) dirs.push_back(&c);
  std::vector<const DirNode*> shownDirs = dirs;
  size_t dirLimit = std::max<size_t>(cfg.fileLimit, 12);
  if (shownDirs.size() > dirLimit) {
    std::stable_sort(shownDirs.begin(), shownDirs.end(), [](auto* x, auto* y) { return x->totalLines > y->totalLines; });
    shownDirs.resize(dirLimit);
  }
  std::sort(shownDirs.begin(), shownDirs.end(), [](auto* x, auto* y) { return x->name < y->name; });
  std::sort(shown.begin(), shown.end(), [](auto* x, auto* y) { return x->name < y->name; });

  for (auto* c : shownDirs) {
    const DirNode* cur = c;
    std::string lab = c->name;
    while (cur->files.empty() && cur->dirs.size() == 1) {  // a/b/c/ as one line
      cur = &cur->dirs.begin()->second;
      lab += "/" + cur->name;
    }
    renderDir(*cur, lab, level + 1, cfg, out, false);
  }
  for (auto* f : shown) out += indent + "  " + fileLine(*f, cfg.symLimit) + "\n";
  if (shown.size() < files.size()) {
    std::map<std::string, size_t> ext;
    std::set<const FileNode*> sh(shown.begin(), shown.end());
    for (auto* f : files) {
      if (sh.count(f)) continue;
      size_t dot = f->name.rfind('.');
      ext[dot == std::string::npos ? f->name : "*" + f->name.substr(dot)]++;
    }
    std::vector<std::pair<std::string, size_t>> ev(ext.begin(), ext.end());
    std::sort(ev.begin(), ev.end(), [](auto& x, auto& y) { return x.second > y.second; });
    out += indent + "  \xE2\x80\xA6 " + fmtInt(files.size() - shown.size()) + " more file" + (files.size() - shown.size() == 1 ? "" : "s") + ": ";
    for (size_t i = 0; i < ev.size() && i < 3; ++i) out += (i ? ", " : "") + ev[i].first + " (" + std::to_string(ev[i].second) + ")";
    if (ev.size() > 3) out += ", \xE2\x80\xA6";
    out += "\n";
  }
  if (shownDirs.size() < dirs.size())
    out += indent + "  \xE2\x80\xA6 " + fmtInt(dirs.size() - shownDirs.size()) + " more director" + (dirs.size() - shownDirs.size() == 1 ? "y" : "ies") + "\n";
}

Json dirJson(const DirNode& d, int level, int maxDepth) {
  Json j = Json::object();
  j.set("name", Json::str(d.name));
  j.set("files", Json::num((double)d.totalFiles));
  j.set("lines", Json::num((double)d.totalLines));
  Json langs = Json::object();
  for (auto& [l, n] : d.langLines) langs.set(l, Json::num((double)n));
  j.set("languages", std::move(langs));
  if (level < maxDepth) {
    Json fs_ = Json::array(), ds = Json::array();
    for (auto& f : d.files) {
      Json fj = Json::object(), sy = Json::array();
      fj.set("name", Json::str(f.name));
      fj.set("lines", Json::num((double)f.lines));
      fj.set("language", Json::str(f.lang));
      for (size_t i = 0; i < f.syms.size() && i < 30; ++i) {
        Json s = Json::object();
        s.set("line", Json::num(f.syms[i].line));
        s.set("kind", Json::str(f.syms[i].kind));
        s.set("name", Json::str(f.syms[i].name));
        sy.push(std::move(s));
      }
      fj.set("symbols", std::move(sy));
      fs_.push(std::move(fj));
    }
    for (auto& [k, c] : d.dirs) ds.push(dirJson(c, level + 1, maxDepth));
    j.set("file_list", std::move(fs_));
    j.set("dirs", std::move(ds));
  }
  return j;
}

int cmdSkim(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.size() > 1) throw UsageError{"expected at most one directory"};
  std::string root = a.pos.empty() ? "." : a.pos[0];
  long userDepth = a.num("depth", 0);
  long budgetTokens = a.num("budget", 4000);
  if (userDepth < 0 || budgetTokens < 0) throw UsageError{"--depth and --budget must not be negative"};
  bool json = wantJson(a);

  WalkOpts wo;
  wo.root = root;
  wo.noIgnore = a.has("no-ignore");
  auto ix = indexFor(ctx, a);
  wo.wantStat = ix != nullptr;
  std::vector<WalkEntry> files;
  std::string err;
  if (!walk(wo, ctx, files, err)) {
    io.err += "skim: " + root + ": " + err + "\n";
    return kNotFound;
  }
  if (ix) annotateEntries(*ix, root, files);
  if (files.size() == 1 && files[0].path == root) {  // a single file: outline it
    Args oa;
    oa.pos.push_back(root);
    if (json) oa.opt["json"].push_back("1");
    return cmdOutline(ctx, oa, io);
  }

  std::vector<FileNode> nodes(files.size());
  std::vector<char> keep(files.size(), 0);
  parallelFor(files.size(), [&](size_t i, size_t) {
    thread_local auto* rawP = new std::string;
    thread_local auto* textP = new std::string;
    std::string &raw = *rawP, &text = *textP;
    const WalkEntry& e = files[i];
    if (isBinaryExtension(e.path)) return;
    LangInfo li = detectLanguage(e.path);
    FileNode& n = nodes[i];
    size_t sl = e.path.rfind('/');
    n.name = sl == std::string::npos ? e.path : e.path.substr(sl + 1);
    n.lang = li.name;
    if (e.idx >= 0) {  // the index already knows this file's line count and symbols
      const IndexedFile& f = ix->files[(size_t)e.idx];
      if (f.flags & kIdxBinary) return;
      keep[i] = 1;
      n.lines = f.lines;
      for (uint32_t k = f.symBegin; k < f.symEnd; ++k) {
        const Symbol& s = ix->symbols[k];
        if (s.depth == 0 || (li.family == Family::Markdown && s.depth <= 1)) n.syms.push_back(s);
      }
      return;
    }
    if (e.size > (32u << 20)) {  // huge: count it, don't read it
      keep[i] = 1;
      return;
    }
    std::string er;
    if (!readFile(e.path, raw, er)) return;
    TextInfo ti;
    toDisplayText(raw, text, ti);
    if (ti.binary) return;
    keep[i] = 1;
    n.lines = countLines(text);
    if (li.family != Family::None && e.size <= kMaxSymbolFile) {
      std::vector<Symbol> syms;
      extractSymbols(li.family, text, syms);
      for (auto& s : syms)
        if (s.depth == 0 || (li.family == Family::Markdown && s.depth <= 1)) n.syms.push_back(std::move(s));
    }
  });

  DirNode tree;
  std::string rootLabel = root == "." ? "." : root;
  while (rootLabel.size() > 1 && rootLabel.back() == '/') rootLabel.pop_back();
  tree.name = rootLabel;
  std::string prefix = root == "." ? "" : root + "/";
  while (prefix.rfind("./", 0) == 0) prefix = prefix.substr(2);
  size_t kept = 0;
  for (size_t i = 0; i < files.size(); ++i) {
    if (!keep[i]) continue;
    ++kept;
    std::string rel = files[i].path;
    if (!prefix.empty() && rel.compare(0, prefix.size(), prefix) == 0) rel = rel.substr(prefix.size());
    DirNode* cur = &tree;
    size_t p = 0;
    for (;;) {
      size_t sl = rel.find('/', p);
      if (sl == std::string::npos) break;
      std::string seg = rel.substr(p, sl - p);
      DirNode& c = cur->dirs[seg];
      c.name = seg;
      cur = &c;
      p = sl + 1;
    }
    cur->files.push_back(std::move(nodes[i]));
  }
  aggregate(tree);
  if (!kept) {
    io.out += "(no text files found)\n";
    return kOk;
  }

  if (json) {
    Json r = Json::object();
    r.set("schema", Json::num(1));
    r.set("tree", dirJson(tree, 0, userDepth ? (int)userDepth : 64));
    io.out += r.dump() + "\n";
    return kOk;
  }

  // Compact single-child chains at the root too.
  const DirNode* top = &tree;
  std::string label = rootLabel;
  while (top->files.empty() && top->dirs.size() == 1) {
    top = &top->dirs.begin()->second;
    label += (label == "." ? "" : "/") + top->name;
  }
  if (label.rfind("./", 0) == 0) label = label.substr(2);

  int maxDepth = 0;
  {
    std::vector<std::pair<const DirNode*, int>> st{{top, 0}};
    while (!st.empty()) {
      auto [d, lv] = st.back();
      st.pop_back();
      maxDepth = std::max(maxDepth, lv);
      for (auto& [k, c] : d->dirs) st.push_back({&c, lv + 1});
    }
  }
  int startDepth = userDepth > 0 ? (int)std::min<long>(userDepth, maxDepth + 1) : maxDepth + 1;
  const size_t budgetChars = budgetTokens == 0 ? (size_t)-1 : (size_t)budgetTokens * 4;
  static const Cfg tiers[] = {{0, 8, 40}, {0, 4, 15}, {0, 0, 6}};
  std::string best;
  for (int d = startDepth; d >= 1; --d) {
    for (auto& t : tiers) {
      Cfg cfg = t;
      cfg.depth = d;
      std::string out;
      renderDir(*top, label, 0, cfg, out, true);
      if (out.size() <= budgetChars) {
        io.out += out;
        return kOk;
      }
      best = std::move(out);
    }
    if (budgetChars == (size_t)-1) break;
  }
  // Even the leanest view is over budget: cut it on a line boundary.
  size_t cut = std::min(best.size(), budgetChars);
  size_t nl = best.rfind('\n', cut);
  io.out += best.substr(0, nl == std::string::npos ? cut : nl + 1);
  io.out += "\xE2\x80\xA6 truncated to fit --budget " + std::to_string(budgetTokens) + "; use a subdirectory or a larger --budget\n";
  return kOk;
}

}  // namespace

const Command cmd_skim = {"skim",
                          "skim [dir] [--depth N] [--budget TOKENS]   the shape of a codebase in one call: directories, sizes, languages, top-level symbols",
                          {{"depth", 'd', true}, {"budget", 'b', true}, {"no-ignore", 0, false}, {"no-index", 0, false}, {"json", 0, false}},
                          cmdSkim};
const Command cmd_outline = {"outline", "outline <file>...                         symbols of a file, nested, with line numbers",
                             {{"json", 0, false}}, cmdOutline};
const Command cmd_find_def = {"find-def", "find-def <symbol> [paths] [-i]            where a function, class, type or constant is defined",
                              {{"ignore-case", 'i', false}, {"max", 'm', true}, {"no-ignore", 0, false}, {"no-index", 0, false}, {"json", 0, false}},
                              cmdFindDef};
const Command cmd_refs = {"refs", "refs <symbol> [paths]                       word-boundary search, ranked: definitions, imports, then other references",
                          {{"max", 'm', true}, {"no-ignore", 0, false}, {"no-index", 0, false}, {"json", 0, false}}, cmdRefs};

}  // namespace acu
