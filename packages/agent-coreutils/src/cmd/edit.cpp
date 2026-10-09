// edit, patch
#include <algorithm>
#include <cstring>
#include <unordered_set>

#include "cmd/cmd.hpp"
#include "core/json.hpp"
#include "core/platform.hpp"
#include "core/text.hpp"

namespace acu {

namespace {

struct EditOp {
  std::string oldS, newS;
  bool all = false;
};

std::string clip(std::string_view s, size_t n = 110) {
  std::string t = trim(s);
  if (t.size() > n) {
    size_t cut = n;
    while (cut > 0 && ((unsigned char)t[cut] & 0xC0) == 0x80) --cut;
    t.resize(cut);
    t += "\xE2\x80\xA6";
  }
  return t;
}

std::string collapseWs(std::string_view s) {
  std::string o;
  bool sp = false;
  for (char c : s) {
    if (c == ' ' || c == '\t' || c == '\r') {
      sp = true;
      continue;
    }
    if (sp && !o.empty()) o.push_back(' ');
    sp = false;
    o.push_back(c);
  }
  return o;
}

// Lines of `text` that look most like the first non-blank line of `old`.
std::string nearMisses(const std::string& text, const std::string& old) {
  std::string target;
  {
    size_t p = 0;
    while (p < old.size()) {
      size_t nl = old.find('\n', p);
      std::string_view l = std::string_view(old).substr(p, nl == std::string::npos ? std::string::npos : nl - p);
      if (!trim(l).empty()) {
        target = trim(l);
        break;
      }
      if (nl == std::string::npos) break;
      p = nl + 1;
    }
  }
  if (target.empty()) return "";
  std::string tc = collapseWs(target), tl = lower(target);
  std::vector<size_t> starts = lineStarts(text);
  size_t lines = starts.empty() ? 0 : starts.size() - 1;

  // 1. identical once whitespace is ignored
  for (size_t i = 0; i < lines; ++i) {
    std::string_view l = lineAt(text, starts, i);
    if (collapseWs(l) == tc) {
      return "  line " + std::to_string(i + 1) + " matches except for whitespace/indentation: " + clip(l) +
             "\n  (check tabs vs spaces and line endings in --old)\n";
    }
  }
  // 2. best trigram overlap
  struct Hit {
    double score;
    size_t line;
  };
  std::vector<Hit> hits;
  if (tl.size() >= 3) {
    std::unordered_set<uint32_t> tri;
    auto key = [](const char* p) { return (uint32_t)((unsigned char)p[0] << 16 | (unsigned char)p[1] << 8 | (unsigned char)p[2]); };
    for (size_t i = 0; i + 2 < tl.size(); ++i) tri.insert(key(&tl[i]));
    double windows = (double)(tl.size() - 2);
    std::string lw;
    for (size_t i = 0; i < lines; ++i) {
      std::string_view l = lineAt(text, starts, i);
      if (l.size() < 3) continue;
      lw.assign(l);
      for (auto& c : lw)
        if (c >= 'A' && c <= 'Z') c = (char)(c + 32);
      size_t cnt = 0;
      for (size_t k = 0; k + 2 < lw.size(); ++k) cnt += tri.count(key(&lw[k]));
      double score = (double)cnt / windows;
      if (score >= 0.5) hits.push_back({score, i});
    }
  } else {
    for (size_t i = 0; i < lines; ++i)
      if (lower(lineAt(text, starts, i)).find(tl) != std::string::npos) hits.push_back({1.0, i});
  }
  if (hits.empty()) return "";
  std::stable_sort(hits.begin(), hits.end(), [](const Hit& a, const Hit& b) { return a.score > b.score; });
  std::string out = "  closest lines:\n";
  for (size_t k = 0; k < hits.size() && k < 3; ++k)
    out += "    " + std::to_string(hits[k].line + 1) + ": " + clip(lineAt(text, starts, hits[k].line)) + "\n";
  return out;
}

// Applies one edit to `buf`. On failure returns false with a message in `err` and leaves buf alone.
bool applyEdit(std::string& buf, EditOp op, bool crlf, const std::string& path, std::string& err, size_t& replaced,
               std::vector<size_t>& lineNos, size_t baseLineAdjust = 0) {
  (void)baseLineAdjust;
  std::string oldS = crlf ? toCrlf(toLf(op.oldS)) : toLf(op.oldS);
  std::string newS = crlf ? toCrlf(toLf(op.newS)) : toLf(op.newS);
  if (oldS.empty()) {
    err = "--old is empty; give the exact text to replace";
    return false;
  }
  if (oldS == newS) {
    err = "--old and --new are identical; nothing to change";
    return false;
  }
  std::vector<size_t> offs;
  for (size_t p = 0; (p = buf.find(oldS, p)) != std::string::npos; p += oldS.size()) {
    offs.push_back(p);
    if (offs.size() > 100000) break;
  }
  if (offs.empty()) {
    err = "--old text not found in " + path + "\n";
    std::string text = crlf ? toLf(buf) : buf;
    err += nearMisses(text, toLf(op.oldS));
    while (!err.empty() && err.back() == '\n') err.pop_back();
    return false;
  }
  lineNos.clear();
  size_t line = 1, last = 0;
  for (size_t o : offs) {
    line += (size_t)std::count(buf.begin() + (long)last, buf.begin() + (long)o, '\n');
    last = o;
    lineNos.push_back(line);
  }
  if (offs.size() > 1 && !op.all) {
    err = "--old text found " + std::to_string(offs.size()) + " times in " + path + " (lines ";
    for (size_t i = 0; i < lineNos.size() && i < 10; ++i) err += (i ? ", " : "") + std::to_string(lineNos[i]);
    if (lineNos.size() > 10) err += ", ...";
    err += "); pass --all or include more surrounding text";
    return false;
  }
  std::string out;
  out.reserve(buf.size() + offs.size() * (newS.size() > oldS.size() ? newS.size() - oldS.size() : 0));
  size_t prev = 0;
  for (size_t o : offs) {
    out.append(buf, prev, o - prev);
    out += newS;
    prev = o + oldS.size();
  }
  out.append(buf, prev, std::string::npos);
  buf = std::move(out);
  replaced = offs.size();
  return true;
}

bool readTextArg(Args& a, const char* name, const char* fileOpt, std::string& out, bool& present, std::string& err) {
  present = false;
  if (a.has(name)) {
    out = a.str(name);
    present = true;
    return true;
  }
  if (a.has(fileOpt)) {
    present = true;
    std::string e;
    if (!readFile(a.str(fileOpt), out, e)) {
      err = std::string("--") + fileOpt + " " + a.str(fileOpt) + ": " + e;
      return false;
    }
  }
  return true;
}

int cmdEdit(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.size() != 1) throw UsageError{"expected exactly one file: edit <file> --old TEXT --new TEXT"};
  const std::string& path = a.pos[0];
  std::string err;
  if (!ctx.allow(path, err)) {
    io.err += "edit: " + path + ": " + err + "\n";
    return kUsage;
  }
  std::vector<EditOp> ops;
  std::string editsSrc;
  bool haveEdits = false;
  if (a.has("edits")) {
    if (!readFile(a.str("edits"), editsSrc, err)) {
      io.err += "edit: --edits " + a.str("edits") + ": " + err + "\n";
      return kNotFound;
    }
    haveEdits = true;
  } else if (a.has("edits-json")) {
    editsSrc = a.str("edits-json");
    haveEdits = true;
  }
  if (haveEdits) {
    Json j;
    if (!parseJson(editsSrc, j, err)) throw UsageError{"edits are not valid JSON: " + err};
    const Json* arr = j.t == Json::Arr ? &j : j.get("edits");
    if (!arr || arr->t != Json::Arr || arr->a.empty()) throw UsageError{"edits must be a non-empty JSON array of {old, new, all?}"};
    for (auto& e : arr->a) {
      const Json* o = e.get("old");
      const Json* n = e.get("new");
      if (!o || o->t != Json::Str || !n || n->t != Json::Str) throw UsageError{"each edit needs string \"old\" and \"new\""};
      EditOp op;
      op.oldS = o->s;
      op.newS = n->s;
      const Json* all = e.get("all");
      op.all = all && all->t == Json::Bool && all->b;
      ops.push_back(std::move(op));
    }
  } else {
    EditOp op;
    bool haveOld, haveNew;
    if (!readTextArg(a, "old", "old-file", op.oldS, haveOld, err) || !readTextArg(a, "new", "new-file", op.newS, haveNew, err)) {
      io.err += "edit: " + err + "\n";
      return kNotFound;
    }
    if (!haveOld) throw UsageError{"--old is required (or --edits FILE)"};
    if (!haveNew) throw UsageError{"--new is required (use --new '' to delete the text)"};
    op.all = a.has("all");
    ops.push_back(std::move(op));
  }

  std::string raw;
  if (!readFile(path, raw, err)) {
    io.err += "edit: " + path + ": " + err + "\n";
    return kNotFound;
  }
  TextInfo ti;
  {
    std::string tmp;
    toDisplayText(raw, tmp, ti);
  }
  if (ti.binary) {
    io.err += "edit: " + path + ": binary file\n";
    return kNotFound;
  }
  if (ti.utf16) {
    io.err += "edit: " + path + ": UTF-16 files are not supported; convert to UTF-8 first\n";
    return kNotFound;
  }
  std::string buf = raw.substr(ti.bom ? 3 : 0);
  bool crlf = dominantCrlf(buf);
  size_t total = 0;
  std::vector<size_t> firstLines;
  for (size_t i = 0; i < ops.size(); ++i) {
    size_t n = 0;
    std::vector<size_t> lines;
    std::string e;
    if (!applyEdit(buf, ops[i], crlf, path, e, n, lines)) {
      io.err += "edit: " + (ops.size() > 1 ? "edit #" + std::to_string(i + 1) + ": " : std::string()) + e + "\n";
      if (ops.size() > 1) io.err += "edit: nothing was written\n";
      return kNotFound;
    }
    total += n;
    if (ops.size() == 1) firstLines = lines;
  }
  if (!a.has("dry-run")) {
    std::string result = (ti.bom ? std::string("\xEF\xBB\xBF") : std::string()) + buf;
    if (!atomicWrite(path, result, err)) {
      io.err += "edit: " + path + ": " + err + "\n";
      return kNotFound;
    }
  }
  std::string verb = a.has("dry-run") ? "would edit " : "edited ";
  if (ops.size() == 1) {
    io.out += verb + path + ": " + std::to_string(total) + (total == 1 ? " replacement" : " replacements") + " (line" +
              (firstLines.size() == 1 ? " " : "s ");
    for (size_t i = 0; i < firstLines.size() && i < 10; ++i) io.out += (i ? ", " : "") + std::to_string(firstLines[i]);
    if (firstLines.size() > 10) io.out += ", ...";
    io.out += ")\n";
  } else {
    io.out += verb + path + ": " + std::to_string(ops.size()) + " edits, " + std::to_string(total) + " replacements\n";
  }
  return kOk;
}

// ------------------------------------------------------------------- patch ---

struct Hunk {
  long oldStart = 0, oldCount = 0, newStart = 0, newCount = 0;
  std::vector<std::string> oldLines, newLines;  // content without prefix or EOL
  bool newNoEol = false;
  std::string header;
};

struct FilePatch {
  std::string oldPath, newPath;
  std::vector<Hunk> hunks;
};

std::string cutPath(std::string s) {
  size_t t = s.find('\t');
  if (t != std::string::npos) s.resize(t);
  while (!s.empty() && (s.back() == ' ' || s.back() == '\r')) s.pop_back();
  if (s.size() >= 2 && s.front() == '"' && s.back() == '"') s = s.substr(1, s.size() - 2);
  return s;
}

std::string stripComponents(const std::string& p, int n) {
  std::string s = p;
  for (int i = 0; i < n; ++i) {
    size_t k = s.find('/');
    if (k == std::string::npos) return s;
    s = s.substr(k + 1);
  }
  return s;
}

bool parsePatch(const std::string& diff, std::vector<FilePatch>& out, std::string& err) {
  std::vector<std::string> lines;
  {
    size_t p = 0;
    while (p < diff.size()) {
      size_t nl = diff.find('\n', p);
      std::string l = diff.substr(p, nl == std::string::npos ? std::string::npos : nl - p);
      if (!l.empty() && l.back() == '\r') l.pop_back();
      lines.push_back(std::move(l));
      if (nl == std::string::npos) break;
      p = nl + 1;
    }
  }
  size_t i = 0;
  while (i < lines.size()) {
    if (lines[i].rfind("--- ", 0) == 0 && i + 1 < lines.size() && lines[i + 1].rfind("+++ ", 0) == 0) {
      FilePatch fp;
      fp.oldPath = cutPath(lines[i].substr(4));
      fp.newPath = cutPath(lines[i + 1].substr(4));
      i += 2;
      while (i < lines.size() && lines[i].rfind("@@", 0) == 0) {
        Hunk h;
        h.header = lines[i];
        long a = 0, b = 1, c = 0, d = 1;
        const char* s = lines[i].c_str() + 2;
        while (*s == ' ') ++s;
        if (*s != '-') {
          err = "bad hunk header: " + lines[i];
          return false;
        }
        char* e;
        a = std::strtol(s + 1, &e, 10);
        if (*e == ',') b = std::strtol(e + 1, &e, 10);
        while (*e == ' ') ++e;
        if (*e != '+') {
          err = "bad hunk header: " + lines[i];
          return false;
        }
        c = std::strtol(e + 1, &e, 10);
        if (*e == ',') d = std::strtol(e + 1, &e, 10);
        h.oldStart = a;
        h.oldCount = b;
        h.newStart = c;
        h.newCount = d;
        ++i;
        long oc = 0, nc = 0;
        char lastKind = ' ';
        while (i < lines.size() && (oc < b || nc < d || (i < lines.size() && lines[i].rfind("\\", 0) == 0))) {
          const std::string& l = lines[i];
          if (l.rfind("\\", 0) == 0) {
            if (lastKind == '+' || lastKind == ' ') h.newNoEol = true;
            ++i;
            continue;
          }
          char k = l.empty() ? ' ' : l[0];
          std::string body = l.empty() ? "" : l.substr(1);
          if (k == ' ') {
            h.oldLines.push_back(body);
            h.newLines.push_back(body);
            ++oc;
            ++nc;
          } else if (k == '-') {
            h.oldLines.push_back(body);
            ++oc;
          } else if (k == '+') {
            h.newLines.push_back(body);
            ++nc;
          } else {
            err = "unexpected line in hunk " + h.header + ": " + l;
            return false;
          }
          lastKind = k;
          ++i;
        }
        if (oc != b || nc != d) {
          err = "hunk " + h.header + " is shorter than its header says";
          return false;
        }
        fp.hunks.push_back(std::move(h));
      }
      out.push_back(std::move(fp));
    } else {
      ++i;
    }
  }
  if (out.empty()) {
    err = "no file changes found in the diff (expected unified diff format)";
    return false;
  }
  return true;
}

int cmdPatch(Ctx& ctx, Args& a, Io& io) {
  std::string diff, err;
  if (a.has("file")) {
    if (!readFile(a.str("file"), diff, err)) {
      io.err += "patch: " + a.str("file") + ": " + err + "\n";
      return kNotFound;
    }
  } else {
    diff = io.input();
  }
  std::vector<FilePatch> patches;
  if (!parsePatch(diff, patches, err)) {
    io.err += "patch: " + err + "\n";
    return kUsage;
  }
  long fuzz = a.num("fuzz", 0);
  if (fuzz < 0) throw UsageError{"--fuzz must not be negative"};
  int rc = kOk;
  for (auto& fp : patches) {
    bool isNew = fp.oldPath == "/dev/null", isDel = fp.newPath == "/dev/null";
    int strip;
    if (a.has("p")) {
      strip = (int)a.num("p", 1);
    } else {
      const std::string& ref = isNew ? fp.newPath : fp.oldPath;
      bool gitStyle = (fp.oldPath.rfind("a/", 0) == 0 || isNew) && (fp.newPath.rfind("b/", 0) == 0 || isDel) &&
                      ref != "/dev/null";
      strip = gitStyle ? 1 : 0;
    }
    std::string path = stripComponents(isDel ? fp.oldPath : fp.newPath, strip);
    if (!ctx.allow(path, err)) {
      io.err += "patch: " + path + ": " + err + "\n";
      rc = kUsage;
      continue;
    }
    std::string raw;
    TextInfo ti;
    std::string text;
    bool exists = false;
    if (!isNew) {
      if (!readFile(path, raw, err)) {
        io.err += "patch: " + path + ": " + err + "\n";
        rc = kNotFound;
        continue;
      }
      exists = true;
      toDisplayText(raw, text, ti);
      if (ti.binary || ti.utf16) {
        io.err += "patch: " + path + ": binary or UTF-16 file\n";
        rc = kNotFound;
        continue;
      }
    }
    bool crlf = exists && ti.crlf;
    // file lines
    std::vector<std::string> fl;
    bool noEol = false;
    {
      size_t p = 0;
      while (p < text.size()) {
        size_t nl = text.find('\n', p);
        std::string l = text.substr(p, nl == std::string::npos ? std::string::npos : nl - p);
        if (nl == std::string::npos) noEol = true;
        if (!l.empty() && l.back() == '\r') l.pop_back();
        fl.push_back(std::move(l));
        if (nl == std::string::npos) break;
        p = nl + 1;
      }
    }
    long delta = 0;
    bool failed = false;
    int applied = 0;
    for (size_t hi = 0; hi < fp.hunks.size(); ++hi) {
      Hunk& h = fp.hunks[hi];
      long want = (h.oldCount == 0 ? h.oldStart : h.oldStart - 1) + delta;  // 0-based index of first old line
      long found = -1;
      for (long d = 0; d <= fuzz && found < 0; ++d) {
        for (int sgn : {1, -1}) {
          if (d == 0 && sgn == -1) continue;
          long at = want + sgn * d;
          if (at < 0 || at + (long)h.oldLines.size() > (long)fl.size()) continue;
          bool ok = true;
          for (size_t k = 0; k < h.oldLines.size() && ok; ++k) ok = fl[(size_t)at + k] == h.oldLines[k];
          if (ok) {
            found = at;
            break;
          }
        }
      }
      if (found < 0) {
        std::string why = "context does not match at line " + std::to_string(h.oldStart + delta);
        if (!h.oldLines.empty()) why += " (expected: " + clip(h.oldLines[0], 60) + ")";
        io.err += "patch: " + path + ": hunk " + std::to_string(hi + 1) + " " + h.header + " rejected: " + why + "\n";
        failed = true;
        continue;
      }
      fl.erase(fl.begin() + found, fl.begin() + found + (long)h.oldLines.size());
      fl.insert(fl.begin() + found, h.newLines.begin(), h.newLines.end());
      delta += (long)h.newLines.size() - (long)h.oldLines.size() + (found - want);
      if ((size_t)found + h.newLines.size() >= fl.size()) noEol = h.newNoEol;
      ++applied;
    }
    if (failed) {
      io.err += "patch: " + path + ": not modified (" + std::to_string(fp.hunks.size() - (size_t)applied) + " of " +
                std::to_string(fp.hunks.size()) + " hunks rejected)\n";
      rc = kNotFound;
      continue;
    }
    if (isDel) {
      if (!fl.empty()) {
        io.err += "patch: " + path + ": file is not empty after removing the diff's lines; not deleted\n";
        rc = kNotFound;
        continue;
      }
      if (!a.has("dry-run")) {
        std::error_code ec;
        fs::remove(toPath(path), ec);
        if (ec) {
          io.err += "patch: " + path + ": " + ec.message() + "\n";
          rc = kNotFound;
          continue;
        }
      }
      io.out += std::string(a.has("dry-run") ? "would delete " : "deleted ") + path + "\n";
      continue;
    }
    std::string result;
    const char* eol = crlf ? "\r\n" : "\n";
    for (size_t k = 0; k < fl.size(); ++k) {
      result += fl[k];
      if (k + 1 < fl.size() || !noEol) result += eol;
    }
    if (ti.bom) result = "\xEF\xBB\xBF" + result;
    if (!a.has("dry-run")) {
      fs::path parent = toPath(path).parent_path();
      std::error_code ec;
      if (!parent.empty()) fs::create_directories(parent, ec);
      if (!atomicWrite(path, result, err)) {
        io.err += "patch: " + path + ": " + err + "\n";
        rc = kNotFound;
        continue;
      }
    }
    io.out += std::string(a.has("dry-run") ? "would patch " : isNew ? "created " : "patched ") + path + " (" +
              std::to_string(applied) + (applied == 1 ? " hunk)" : " hunks)") + "\n";
  }
  return rc;
}

}  // namespace

const Command cmd_edit = {
    "edit",
    "edit <file> --old TEXT --new TEXT [--all]  exact-string replace; fails without writing if --old is missing or ambiguous\n"
    "edit <file> --edits edits.json              several replacements, all or nothing ([{\"old\":..,\"new\":..}])",
    {{"old", 0, true}, {"new", 0, true}, {"old-file", 0, true}, {"new-file", 0, true}, {"all", 0, false},
     {"edits", 0, true}, {"edits-json", 0, true}, {"dry-run", 0, false}},
    cmdEdit};

const Command cmd_patch = {"patch",
                           "patch [-p N] [--fuzz N] [--file F]       apply a unified diff from stdin; reports rejected hunks (default -p 1 for git diffs)",
                           {{"p", 'p', true}, {"fuzz", 0, true}, {"file", 0, true}, {"dry-run", 0, false}},
                           cmdPatch};

}  // namespace acu
