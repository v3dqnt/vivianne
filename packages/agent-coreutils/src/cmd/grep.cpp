// grep
#include <algorithm>
#include <atomic>
#include <cstring>
#include <set>

#include "cmd/cmd.hpp"
#include "cmd/search.hpp"
#include "core/glob.hpp"
#include "core/json.hpp"
#include "core/lang.hpp"
#include "core/platform.hpp"
#include "core/regex.hpp"
#include "core/simd.hpp"
#include "core/text.hpp"
#include "core/walk.hpp"
#include "index/use.hpp"

namespace acu {

// ------------------------------------------------------------------ engine ---

namespace {

bool hasBom16(std::string_view r) {
  return r.size() >= 2 && (((unsigned char)r[0] == 0xFF && (unsigned char)r[1] == 0xFE) ||
                           ((unsigned char)r[0] == 0xFE && (unsigned char)r[1] == 0xFF));
}

std::string_view stripCr(std::string_view l) {
  if (!l.empty() && l.back() == '\r') l.remove_suffix(1);
  return l;
}

}  // namespace

bool buildMatcher(const std::vector<std::string>& patterns, bool fixed, bool icase, bool word, Regex& re,
                  std::string& err) {
  std::string pat;
  if (patterns.size() == 1) {
    pat = fixed ? Regex::escape(patterns[0]) : patterns[0];
  } else {
    for (size_t i = 0; i < patterns.size(); ++i) {
      if (i) pat += "|";
      pat += "(?:" + (fixed ? Regex::escape(patterns[i]) : patterns[i]) + ")";
    }
  }
  if (!Regex::compile(pat, icase, word, re, err)) return false;
  for (auto& l : re.literals)
    if (l.find('\n') != std::string::npos) {
      re.literals.clear();
      re.pureLiterals = false;
      break;
    }
  return true;
}

void searchBuffer(const Regex& re, std::string_view buf, size_t start, const std::function<bool(size_t, size_t)>& hit) {
  const size_t n = buf.size();
  size_t pos = start;
  const auto& lits = re.literals;
  if (!lits.empty()) {
    const std::string_view hay = buf;
    std::vector<Needle> needles;
    needles.reserve(lits.size());
    for (auto& l : lits) needles.emplace_back(l, re.litIcase);
    constexpr size_t kUnset = (size_t)-1, kDone = (size_t)-2;
    std::vector<size_t> next(lits.size(), kUnset);
    while (pos < n) {
      size_t best = kDone;
      for (size_t i = 0; i < lits.size(); ++i) {
        if (next[i] == kDone) continue;
        if (next[i] == kUnset || next[i] < pos) {
          size_t f = findNeedle(needles[i], hay.data(), n, pos);
          next[i] = f == kNoPos ? kDone : f;
        }
        if (next[i] != kDone && next[i] < best) best = next[i];
      }
      if (best == kDone) break;
      size_t ls = best == 0 ? std::string_view::npos : hay.rfind('\n', best - 1);
      ls = ls == std::string_view::npos ? 0 : ls + 1;
      if (ls < start) ls = start;
      size_t le = hay.find('\n', best);
      if (le == std::string_view::npos) le = n;
      if (re.pureLiterals || re.search(stripCr(buf.substr(ls, le - ls)))) {
        if (!hit(ls, le)) return;
      }
      pos = le + 1;
    }
    return;
  }
  while (pos < n) {
    const void* nl = std::memchr(buf.data() + pos, '\n', n - pos);
    size_t le = nl ? (size_t)((const char*)nl - buf.data()) : n;
    if (re.search(stripCr(buf.substr(pos, le - pos)))) {
      if (!hit(pos, le)) return;
    }
    pos = le + 1;
  }
}

namespace {

struct Opts {
  size_t before = 0, after = 0, max = 200;
  bool filesOnly = false, count = false;
};

struct Line {
  size_t no;
  bool match;
  std::string text;
};
struct Group {
  std::vector<Line> lines;
};
struct FileRes {
  size_t count = 0;
  bool collected = false, processed = false, unreadable = false;
  std::vector<Group> groups;
};

void searchFile(const std::string& path, const Regex& re, const Opts& o, bool collect, FileRes& res) {
  thread_local auto* rawP = new std::string;
  thread_local auto* convP = new std::string;
  std::string &raw = *rawP, &conv = *convP;
  std::string err;
  res = FileRes{};
  res.processed = true;
  res.collected = collect;
  if (!readFile(path, raw, err)) {
    res.unreadable = true;
    return;
  }
  std::string_view buf = raw;
  size_t start = 0;
  if (hasBom16(raw)) {
    TextInfo ti;
    toDisplayText(raw, conv, ti);
    buf = conv;
  } else {
    if (looksBinary(raw)) return;
    if (raw.size() >= 3 && (unsigned char)raw[0] == 0xEF && (unsigned char)raw[1] == 0xBB && (unsigned char)raw[2] == 0xBF)
      start = 3;
  }
  struct Hit {
    size_t ls, le, no;
  };
  std::vector<Hit> hits;
  size_t lineNo = 1, counted = start;
  searchBuffer(re, buf, start, [&](size_t ls, size_t le) {
    ++res.count;
    if (o.filesOnly) return false;
    if (collect && hits.size() < o.max) {
      lineNo += (size_t)std::count(buf.data() + counted, buf.data() + ls, '\n');
      counted = ls;
      hits.push_back({ls, le, lineNo});
    }
    return true;
  });
  if (!collect || o.count || o.filesOnly) return;

  const size_t n = buf.size();
  size_t prevEnd = start;  // offset where the next group may begin
  for (size_t k = 0; k < hits.size(); ++k) {
    Group g;
    std::vector<std::pair<size_t, size_t>> before;
    size_t off = hits[k].ls;
    for (size_t b = 0; b < o.before; ++b) {
      if (off <= prevEnd || off == 0) break;
      size_t e = off - 1;  // the '\n' ending the previous line
      size_t s = e == 0 ? std::string_view::npos : buf.rfind('\n', e - 1);
      s = s == std::string_view::npos ? 0 : s + 1;
      if (s < prevEnd) s = prevEnd;
      before.emplace_back(s, e);
      off = s;
    }
    std::reverse(before.begin(), before.end());
    size_t no = hits[k].no - before.size();
    for (auto& [s, e] : before) {
      Line l{no++, false, {}};
      appendTrunc(l.text, stripCr(buf.substr(s, e - s)));
      g.lines.push_back(std::move(l));
    }
    Line m{hits[k].no, true, {}};
    appendTrunc(m.text, stripCr(buf.substr(hits[k].ls, hits[k].le - hits[k].ls)));
    g.lines.push_back(std::move(m));
    size_t after = hits[k].le + 1;  // start of the line after the match
    size_t limit = k + 1 < hits.size() ? hits[k + 1].ls : n + 1;
    size_t ln = hits[k].no + 1;
    for (size_t a = 0; a < o.after; ++a) {
      if (after >= n || after >= limit) break;
      const void* nl = std::memchr(buf.data() + after, '\n', n - after);
      size_t le = nl ? (size_t)((const char*)nl - buf.data()) : n;
      Line l{ln++, false, {}};
      appendTrunc(l.text, stripCr(buf.substr(after, le - after)));
      g.lines.push_back(std::move(l));
      after = le + 1;
    }
    prevEnd = std::min(after, n);
    res.groups.push_back(std::move(g));
  }
}

bool matchesGlobs(const std::string& path, const std::vector<std::string>& inc, const std::vector<std::string>& exc) {
  auto hit = [&](const std::string& pat) {
    std::string_view target = path;
    if (pat.find('/') == std::string::npos) {
      size_t s = path.rfind('/');
      if (s != std::string::npos) target = std::string_view(path).substr(s + 1);
    }
    return globMatch(pat, target);
  };
  for (auto& e : exc)
    if (hit(e)) return false;
  if (inc.empty()) return true;
  for (auto& i : inc)
    if (hit(i)) return true;
  return false;
}

int cmdGrep(Ctx& ctx, Args& a, Io& io) {
  std::vector<std::string> patterns = a.all("e");
  std::vector<std::string> paths = a.pos;
  if (patterns.empty()) {
    if (paths.empty()) throw UsageError{"no pattern given: grep <pattern> [paths]"};
    patterns.push_back(paths.front());
    paths.erase(paths.begin());
  }
  if (paths.empty()) paths.push_back(".");

  Opts o;
  long c = a.num("context", 0);
  o.before = o.after = (size_t)std::max(0L, c);
  if (a.has("A")) o.after = (size_t)std::max(0L, a.num("A", 0));
  if (a.has("B")) o.before = (size_t)std::max(0L, a.num("B", 0));
  o.max = capOf(a, 200);
  o.filesOnly = a.has("files-with-matches");
  o.count = a.has("count");
  bool json = wantJson(a);

  Regex re;
  std::string err;
  if (!buildMatcher(patterns, a.has("fixed-strings"), a.has("ignore-case"), a.has("word-regexp"), re, err)) {
    io.err += "grep: invalid pattern: " + err + "\n";
    return kUsage;
  }

  // file list
  auto ix = indexFor(ctx, a);
  std::vector<WalkEntry> files;
  int failedRoots = 0;
  for (auto& p : paths) {
    std::string e;
    WalkOpts wo;
    wo.root = p;
    wo.noIgnore = a.has("no-ignore");
    wo.wantStat = ix != nullptr;
    std::vector<WalkEntry> part;
    if (!walk(wo, ctx, part, e)) {
      io.err += "grep: " + p + ": " + e + "\n";
      ++failedRoots;
      continue;
    }
    if (ix) annotateEntries(*ix, p, part);
    for (auto& x : part) files.push_back(std::move(x));
  }
  if (failedRoots == (int)paths.size()) return kUsage;
  if (paths.size() > 1) {
    std::sort(files.begin(), files.end(), [](const WalkEntry& x, const WalkEntry& y) { return x.path < y.path; });
    files.erase(std::unique(files.begin(), files.end(),
                            [](const WalkEntry& x, const WalkEntry& y) { return x.path == y.path; }),
                files.end());
  }
  {
    std::vector<WalkEntry> kept;
    for (auto& f : files)
      if (!isBinaryExtension(f.path)) kept.push_back(std::move(f));
    files = std::move(kept);
  }
  if (ix) narrowByContent(*ix, re.literals, files);  // opens only files that can contain the pattern
  std::set<std::string> exts;
  for (auto& t : a.all("type"))
    for (size_t p = 0; p <= t.size();) {
      size_t comma = t.find(',', p);
      std::string tok = t.substr(p, comma == std::string::npos ? std::string::npos : comma - p);
      for (auto& e : typeExtensions(trim(tok))) exts.insert(e);
      if (comma == std::string::npos) break;
      p = comma + 1;
    }
  std::vector<std::string> inc, exc;
  for (auto& g : a.all("glob"))
    for (auto& x : expandBraces(g)) (!x.empty() && x[0] == '!' ? exc : inc).push_back(!x.empty() && x[0] == '!' ? x.substr(1) : x);
  if (!exts.empty() || !inc.empty() || !exc.empty()) {
    std::vector<WalkEntry> kept;
    for (auto& f : files) {
      if (!exts.empty()) {
        size_t dot = f.path.rfind('.'), sl = f.path.rfind('/');
        if (dot == std::string::npos || (sl != std::string::npos && dot < sl) || !exts.count(lower(f.path.substr(dot + 1))))
          continue;
      }
      if (!matchesGlobs(f.path, inc, exc)) continue;
      kept.push_back(std::move(f));
    }
    files = std::move(kept);
  }

  // search
  std::vector<FileRes> results(files.size());
  std::atomic<size_t> collectedGroups{0};
  const bool collectAll = !o.count && !o.filesOnly;
  parallelFor(files.size(), [&](size_t i, size_t) {
    bool collect = collectAll && collectedGroups.load(std::memory_order_relaxed) < o.max;
    searchFile(files[i].path, re, o, collect, results[i]);
    collectedGroups.fetch_add(results[i].groups.size(), std::memory_order_relaxed);
  });

  size_t totalMatches = 0, filesWithMatches = 0, unreadable = 0;
  for (auto& r : results) {
    totalMatches += r.count;
    filesWithMatches += r.count > 0;
    unreadable += r.unreadable;
  }

  // report
  size_t emitted = 0, shownFiles = 0;
  Json jm = Json::array();
  bool printedAny = false;
  const bool ctxMode = o.before > 0 || o.after > 0;
  if (o.filesOnly || o.count) {
    for (size_t i = 0; i < files.size(); ++i) {
      if (!results[i].count) continue;
      if (emitted >= o.max) break;
      ++emitted;
      if (json) {
        Json j = Json::object();
        j.set("path", Json::str(files[i].path));
        if (o.count) j.set("count", Json::num((double)results[i].count));
        jm.push(std::move(j));
      } else if (o.count) {
        io.out += files[i].path + ":" + std::to_string(results[i].count) + "\n";
      } else {
        io.out += files[i].path + "\n";
      }
    }
    if (!json && filesWithMatches > emitted)
      io.out += "\xE2\x80\xA6 " + fmtInt(filesWithMatches - emitted) + " more file" +
                (filesWithMatches - emitted == 1 ? "" : "s") + "; use --max " + std::to_string(filesWithMatches) + " to list all\n";
    shownFiles = emitted;
  } else {
    for (size_t i = 0; i < files.size() && emitted < o.max; ++i) {
      FileRes& r = results[i];
      if (!r.count) continue;
      if (!r.collected) searchFile(files[i].path, re, o, true, r);  // skipped earlier because the cap looked full
      size_t prevLast = 0;
      bool firstGroup = true;
      for (auto& g : r.groups) {
        if (emitted >= o.max) break;
        if (!json && ctxMode && (printedAny || !firstGroup)) {
          bool adjacent = !firstGroup && g.lines.front().no == prevLast + 1;
          if (!adjacent) io.out += "--\n";
        }
        for (auto& l : g.lines) {
          if (json) {
            if (!l.match) continue;
            Json j = Json::object();
            j.set("path", Json::str(files[i].path));
            j.set("line", Json::num((double)l.no));
            j.set("text", Json::str(l.text));
            jm.push(std::move(j));
          } else {
            io.out += files[i].path + (l.match ? ":" : "-") + std::to_string(l.no) + (l.match ? ":" : "-") + l.text + "\n";
          }
          if (l.match) ++emitted;
        }
        prevLast = g.lines.back().no;
        firstGroup = false;
        printedAny = true;
      }
      ++shownFiles;
    }
    if (!json && totalMatches > emitted) {
      size_t leftFiles = 0;
      for (size_t i = shownFiles; i < files.size(); ++i) leftFiles += results[i].count > 0;
      io.out += "\xE2\x80\xA6 " + fmtInt(totalMatches - emitted) + " more match" + (totalMatches - emitted == 1 ? "" : "es") +
                (leftFiles ? " (" + fmtInt(leftFiles) + " more file" + (leftFiles == 1 ? "" : "s") + ")" : "") +
                " not shown; use --max " + std::to_string(totalMatches) + " to see all\n";
    }
  }
  if (json) {
    Json root = Json::object();
    root.set("schema", Json::num(1));
    root.set((o.filesOnly || o.count) ? "files" : "matches", std::move(jm));
    root.set("total_matches", Json::num((double)totalMatches));
    root.set("files_with_matches", Json::num((double)filesWithMatches));
    root.set("truncated", Json::boolean(o.filesOnly || o.count ? filesWithMatches > emitted : totalMatches > emitted));
    io.out += root.dump() + "\n";
  }
  if (unreadable) io.err += "grep: skipped " + fmtInt(unreadable) + " unreadable file" + (unreadable == 1 ? "" : "s") + "\n";
  if (Regex::skippedLines())
    io.err += "grep: skipped " + fmtInt(Regex::skippedLines()) + " line(s) too long for the regex engine\n";
  return totalMatches ? kOk : kNotFound;
}

}  // namespace

const Command cmd_grep = {
    "grep",
    "grep <pattern> [paths] [-F] [-i] [-w] [-C N] [--type ts,rs] [--glob G] [--max N] [-l] [-c]\n"
    "                                            search file contents; output is path:line:text (context lines use '-')\n"
    "grep -e A -e B [paths]                      several patterns in one pass",
    {{"e", 'e', true},
     {"fixed-strings", 'F', false},
     {"ignore-case", 'i', false},
     {"word-regexp", 'w', false},
     {"context", 'C', true},
     {"A", 'A', true},
     {"B", 'B', true},
     {"type", 't', true},
     {"glob", 'g', true},
     {"max", 'm', true},
     {"files-with-matches", 'l', false},
     {"count", 'c', false},
     {"no-ignore", 0, false},
     {"no-index", 0, false},
     {"recursive", 'r', false},     // accepted and ignored: directories are always searched
     {"line-number", 'n', false},   // accepted and ignored: line numbers are always printed
     {"json", 0, false}},
    cmdGrep};

}  // namespace acu
