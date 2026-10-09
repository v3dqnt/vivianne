// read, head, tail, stat
#include <algorithm>
#include <cstring>

#include "cmd/cmd.hpp"
#include "core/json.hpp"
#include "core/lang.hpp"
#include "core/platform.hpp"
#include "core/text.hpp"

namespace acu {

namespace {

struct Range {
  long from = 1;  // negative: that many lines from the end
  long to = 0;    // 0 = to the end
};

long parseLineNo(const std::string& s, const char* what) {
  char* end = nullptr;
  long v = std::strtol(s.c_str(), &end, 10);
  if (s.empty() || *end || v < 0) throw UsageError{std::string("--lines: bad ") + what + " '" + s + "' (use A:B, A: or :B)"};
  return v;
}

Range parseRange(const std::string& spec) {
  Range r;
  if (spec.empty()) return r;
  size_t c = spec.find_first_of(":-");
  if (c == std::string::npos) {
    r.from = r.to = parseLineNo(spec, "line number");
    return r;
  }
  std::string a = spec.substr(0, c), b = spec.substr(c + 1);
  if (!a.empty()) r.from = parseLineNo(a, "start");
  if (!b.empty()) r.to = parseLineNo(b, "end");
  if (r.from < 1) r.from = 1;
  if (r.to && r.to < r.from) throw UsageError{"--lines: end is before start in '" + spec + "'"};
  return r;
}

struct FileOut {
  size_t totalLines = 0;
  size_t first = 0, last = 0;  // lines shown, 1-based; first > last if none
  size_t remaining = 0;        // lines in the requested range that were not shown
  std::string text;            // numbered lines
  std::string content;         // plain lines (for --json)
  std::string note;            // the "… N more lines" line
  size_t bytes = 0;
};

// Returns false with `err` set if the file can't be shown.
bool readOne(Ctx& ctx, const std::string& path, Range rg, size_t max, bool notice, FileOut& fo, std::string& err) {
  if (!ctx.allow(path, err)) return false;
  std::string raw;
  if (!readFile(path, raw, err)) return false;
  fo.bytes = raw.size();
  std::string text;
  TextInfo ti;
  toDisplayText(raw, text, ti);
  if (ti.binary) {
    err = "binary file (" + fmtInt(raw.size()) + " bytes); not shown";
    return false;
  }
  size_t total = countLines(text);
  fo.totalLines = total;
  if (total == 0) return true;
  long from = rg.from;
  if (from < 0) from = std::max<long>(1, (long)total + from + 1);
  size_t to = rg.to == 0 ? total : std::min<size_t>((size_t)rg.to, total);
  if ((size_t)from > total) {
    err = "file has only " + fmtInt(total) + " lines";
    return false;
  }
  size_t limit = max ? std::min(to, (size_t)from + max - 1) : to;
  size_t lineNo = 1, pos = 0;
  while (lineNo < (size_t)from) {
    const void* nl = std::memchr(text.data() + pos, '\n', text.size() - pos);
    if (!nl) break;
    pos = (size_t)((const char*)nl - text.data()) + 1;
    ++lineNo;
  }
  fo.first = lineNo;
  while (lineNo <= limit && pos <= text.size()) {
    const void* nl = pos < text.size() ? std::memchr(text.data() + pos, '\n', text.size() - pos) : nullptr;
    size_t end = nl ? (size_t)((const char*)nl - text.data()) : text.size();
    size_t le = end;
    if (le > pos && text[le - 1] == '\r') --le;
    std::string_view line(text.data() + pos, le - pos);
    fo.text += std::to_string(lineNo);
    fo.text += '\t';
    appendTrunc(fo.text, line);
    fo.text += '\n';
    fo.content.append(line);
    fo.content += '\n';
    fo.last = lineNo;
    ++lineNo;
    pos = nl ? end + 1 : text.size() + 1;
  }
  fo.remaining = to > fo.last ? to - fo.last : 0;
  if (fo.remaining && notice) {
    size_t x = fo.last + 1, y = std::min(to, fo.last + (max ? max : fo.remaining));
    fo.note = "\xE2\x80\xA6 " + fmtInt(fo.remaining) + " more line" + (fo.remaining == 1 ? "" : "s") + "; use --lines " +
              std::to_string(x) + ":" + std::to_string(y) + "\n";
  }
  return true;
}

int readCommon(Ctx& ctx, Args& a, Io& io, const char* cmd, Range rg, size_t max, bool notice) {
  if (a.pos.empty()) throw UsageError{"no file given"};
  bool multi = a.pos.size() > 1, json = wantJson(a);
  int rc = kOk;
  Json files = Json::array();
  for (auto& path : a.pos) {
    FileOut fo;
    std::string err;
    if (!readOne(ctx, path, rg, max, notice, fo, err)) {
      io.err += std::string(cmd) + ": " + path + ": " + err + "\n";
      rc = kNotFound;
      continue;
    }
    if (json) {
      Json f = Json::object();
      f.set("path", Json::str(path));
      f.set("total_lines", Json::num((double)fo.totalLines));
      f.set("from", Json::num((double)fo.first));
      f.set("to", Json::num((double)fo.last));
      f.set("remaining", Json::num((double)fo.remaining));
      f.set("content", Json::str(fo.content));
      files.push(std::move(f));
      continue;
    }
    if (multi) io.out += "== " + path + " ==\n";
    if (fo.totalLines == 0) io.out += "(empty file)\n";
    io.out += fo.text;
    io.out += fo.note;
  }
  if (json) {
    Json root = Json::object();
    root.set("schema", Json::num(1));
    root.set("files", std::move(files));
    io.out += root.dump() + "\n";
  }
  return rc;
}

int cmdRead(Ctx& ctx, Args& a, Io& io) {
  Range rg = parseRange(a.str("lines"));
  long max = a.num("max", 2000);
  if (max < 0) throw UsageError{"--max must not be negative"};
  return readCommon(ctx, a, io, "read", rg, (size_t)max, true);
}

int cmdHead(Ctx& ctx, Args& a, Io& io) {
  long n = a.num("n", 10);
  if (n < 1) throw UsageError{"-n must be at least 1"};
  Range rg;
  rg.to = n;
  return readCommon(ctx, a, io, "head", rg, 0, false);
}

int cmdTail(Ctx& ctx, Args& a, Io& io) {
  long n = a.num("n", 10);
  if (n < 1) throw UsageError{"-n must be at least 1"};
  Range rg;
  rg.from = -n;
  return readCommon(ctx, a, io, "tail", rg, 0, false);
}

int cmdStat(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.empty()) throw UsageError{"no path given"};
  int rc = kOk;
  bool json = wantJson(a);
  Json arr = Json::array();
  for (auto& path : a.pos) {
    std::string why;
    FileInfo fi;
    if (!ctx.allow(path, why)) {
      io.err += "stat: " + path + ": " + why + "\n";
      rc = kNotFound;
      continue;
    }
    if (!statPath(path, fi)) {
      io.err += "stat: " + path + ": not found\n";
      rc = kNotFound;
      continue;
    }
    Json j = Json::object();
    j.set("path", Json::str(path));
    std::string line = path + ": ";
    if (fi.isDir) {
      j.set("type", Json::str("directory"));
      line = path + (path.back() == '/' ? "" : "/") + ": directory";
    } else {
      j.set("type", Json::str("file"));
      j.set("bytes", Json::num((double)fi.size));
      std::string raw, err;
      bool binary = false, crlf = false, bom = false;
      size_t lines = 0;
      if (fi.size <= (256u << 20) && readFile(path, raw, err)) {
        std::string text;
        TextInfo ti;
        toDisplayText(raw, text, ti);
        binary = ti.binary;
        crlf = ti.crlf;
        bom = ti.bom || ti.utf16;
        if (!binary) lines = countLines(text);
      }
      line += fmtInt(fi.size) + (fi.size == 1 ? " byte" : " bytes");
      if (binary) line += ", binary";
      else {
        line += ", " + fmtInt(lines) + (lines == 1 ? " line" : " lines");
        j.set("lines", Json::num((double)lines));
      }
      LangInfo li = detectLanguage(path);
      if (*li.name) {
        line += std::string(", ") + li.name;
        j.set("language", Json::str(li.name));
      }
      if (crlf) line += ", CRLF";
      if (bom) line += ", BOM";
      j.set("binary", Json::boolean(binary));
      j.set("crlf", Json::boolean(crlf));
    }
    j.set("modified", Json::str(fmtTime(fi.mtime)));
    line += ", modified " + fmtTime(fi.mtime);
    if (json) arr.push(std::move(j));
    else io.out += line + "\n";
  }
  if (json) {
    Json root = Json::object();
    root.set("schema", Json::num(1));
    root.set("files", std::move(arr));
    io.out += root.dump() + "\n";
  }
  return rc;
}

}  // namespace

const Command cmd_read = {"read",
                          "read <file>... [--lines A:B] [--max N]   print files with line numbers (default max 2000 lines)",
                          {{"lines", 'l', true}, {"max", 'm', true}, {"json", 0, false}},
                          cmdRead};
const Command cmd_head = {"head", "head [-n N] <file>...                    first N lines (default 10), numbered like read",
                          {{"n", 'n', true}, {"json", 0, false}}, cmdHead};
const Command cmd_tail = {"tail", "tail [-n N] <file>...                    last N lines (default 10), numbered like read",
                          {{"n", 'n', true}, {"json", 0, false}}, cmdTail};
const Command cmd_stat = {"stat", "stat <path>...                           size, lines, language, line endings, modified time",
                          {{"json", 0, false}}, cmdStat};

}  // namespace acu
