// write, mv, cp, rm
#include <algorithm>

#include "cmd/cmd.hpp"
#include "core/platform.hpp"
#include "core/text.hpp"

namespace acu {

namespace {

bool makeParents(const std::string& path, std::string& err) {
  fs::path p = toPath(path).parent_path();
  if (p.empty()) return true;
  std::error_code ec;
  fs::create_directories(p, ec);
  if (ec) {
    err = "cannot create directory " + fromPath(p) + ": " + ec.message();
    return false;
  }
  return true;
}

int cmdWrite(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.size() != 1) throw UsageError{"expected exactly one file: write <file> (content on stdin)"};
  const std::string& path = a.pos[0];
  std::string err;
  if (!ctx.allow(path, err)) {
    io.err += "write: " + path + ": " + err + "\n";
    return kUsage;
  }
  std::string data = io.input();
  FileInfo fi;
  bool existed = statPath(path, fi);
  if (existed && fi.isDir) {
    io.err += "write: " + path + ": is a directory\n";
    return kUsage;
  }
  bool raw = a.has("raw"), append = a.has("append");
  std::string existing;
  bool hadBom = false;
  if (existed && (!raw || append)) {
    if (!readFile(path, existing, err)) {
      io.err += "write: " + path + ": " + err + "\n";
      return kNotFound;
    }
    TextInfo probe;
    std::string tmp;
    if (!raw) {
      if (existing.size() >= 2 && (((unsigned char)existing[0] == 0xFF && (unsigned char)existing[1] == 0xFE) ||
                                   ((unsigned char)existing[0] == 0xFE && (unsigned char)existing[1] == 0xFF))) {
        io.err += "write: " + path + ": existing file is UTF-16; pass --raw to replace it as-is\n";
        return kUsage;
      }
      hadBom = existing.size() >= 3 && (unsigned char)existing[0] == 0xEF && (unsigned char)existing[1] == 0xBB &&
               (unsigned char)existing[2] == 0xBF;
      std::string_view body = std::string_view(existing).substr(hadBom ? 3 : 0);
      bool hasNl = body.find('\n') != std::string_view::npos;
      if (hasNl) data = dominantCrlf(body) ? toCrlf(toLf(data)) : toLf(data);  // keep the file's line endings
    }
  }
  std::string result;
  if (append && existed) {
    result = existing;
    if (!raw && !result.empty() && result.back() != '\n' && !data.empty()) {
      // appended text starts a new line only if the caller included the newline; do not guess
    }
    result += data;
  } else {
    if (hadBom && !(data.size() >= 3 && (unsigned char)data[0] == 0xEF && (unsigned char)data[1] == 0xBB &&
                    (unsigned char)data[2] == 0xBF))
      result = "\xEF\xBB\xBF";
    result += data;
  }
  if (!a.has("no-parents") && !makeParents(path, err)) {
    io.err += "write: " + err + "\n";
    return kNotFound;
  }
  if (!atomicWrite(path, result, err)) {
    io.err += "write: " + path + ": " + err + "\n";
    return kNotFound;
  }
  io.out += std::string(existed ? (append ? "appended to " : "wrote ") : "created ") + path + " (" +
            fmtInt(result.size()) + " bytes, " + fmtInt(countLines(result)) + " lines)\n";
  return kOk;
}

std::string baseName(const std::string& p) {
  std::string s = p;
  while (s.size() > 1 && s.back() == '/') s.pop_back();
  size_t k = s.find_last_of('/');
  return k == std::string::npos ? s : s.substr(k + 1);
}

// Shared by mv and cp: resolves each source's destination.
bool destinations(Ctx& ctx, Args& a, const char* cmd, Io& io, std::vector<std::pair<std::string, std::string>>& pairs) {
  if (a.pos.size() < 2) throw UsageError{"usage: " + std::string(cmd) + " <source>... <destination>"};
  std::string dest = a.pos.back(), why;
  FileInfo di;
  bool destIsDir = statPath(dest, di) && di.isDir;
  if (a.pos.size() > 2 && !destIsDir) {
    io.err += std::string(cmd) + ": " + dest + ": with several sources the destination must be an existing directory\n";
    return false;
  }
  for (size_t i = 0; i + 1 < a.pos.size(); ++i) {
    const std::string& src = a.pos[i];
    std::string to = destIsDir ? joinPath(dest, baseName(src)) : dest;
    if (!ctx.allow(src, why) || !ctx.allow(to, why)) {
      io.err += std::string(cmd) + ": " + src + ": " + why + "\n";
      return false;
    }
    pairs.emplace_back(src, to);
  }
  return true;
}

int cmdMv(Ctx& ctx, Args& a, Io& io) {
  std::vector<std::pair<std::string, std::string>> pairs;
  if (!destinations(ctx, a, "mv", io, pairs)) return kUsage;
  int rc = kOk;
  for (auto& [src, to] : pairs) {
    FileInfo si, ti;
    if (!statPath(src, si)) {
      io.err += "mv: " + src + ": not found\n";
      rc = kNotFound;
      continue;
    }
    if (statPath(to, ti) && !a.has("force")) {
      io.err += "mv: " + to + " already exists; pass --force to replace it\n";
      rc = kNotFound;
      continue;
    }
    std::string err;
    if (!a.has("no-parents") && !makeParents(to, err)) {
      io.err += "mv: " + err + "\n";
      rc = kNotFound;
      continue;
    }
    std::error_code ec;
    if (ti.exists && a.has("force") && ti.isDir && !si.isDir) {
      io.err += "mv: " + to + " is a directory\n";
      rc = kNotFound;
      continue;
    }
    fs::rename(toPath(src), toPath(to), ec);
    if (ec) {  // other volume: copy then remove
      fs::copy(toPath(src), toPath(to), fs::copy_options::recursive | fs::copy_options::overwrite_existing, ec);
      if (!ec) fs::remove_all(toPath(src), ec);
    }
    if (ec) {
      io.err += "mv: " + src + " -> " + to + ": " + ec.message() + "\n";
      rc = kNotFound;
      continue;
    }
    io.out += "moved " + src + " -> " + to + "\n";
  }
  return rc;
}

int cmdCp(Ctx& ctx, Args& a, Io& io) {
  std::vector<std::pair<std::string, std::string>> pairs;
  if (!destinations(ctx, a, "cp", io, pairs)) return kUsage;
  int rc = kOk;
  for (auto& [src, to] : pairs) {
    FileInfo si, ti;
    if (!statPath(src, si)) {
      io.err += "cp: " + src + ": not found\n";
      rc = kNotFound;
      continue;
    }
    if (si.isDir && !a.has("recursive")) {
      io.err += "cp: " + src + " is a directory; pass -r to copy it\n";
      rc = kNotFound;
      continue;
    }
    if (statPath(to, ti) && !a.has("force")) {
      io.err += "cp: " + to + " already exists; pass --force to replace it\n";
      rc = kNotFound;
      continue;
    }
    std::string err;
    if (!a.has("no-parents") && !makeParents(to, err)) {
      io.err += "cp: " + err + "\n";
      rc = kNotFound;
      continue;
    }
    std::error_code ec;
    fs::copy(toPath(src), toPath(to),
             (si.isDir ? fs::copy_options::recursive : fs::copy_options::none) | fs::copy_options::overwrite_existing, ec);
    if (ec) {
      io.err += "cp: " + src + " -> " + to + ": " + ec.message() + "\n";
      rc = kNotFound;
      continue;
    }
    io.out += "copied " + src + " -> " + to + "\n";
  }
  return rc;
}

int cmdRm(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.empty()) throw UsageError{"no path given"};
  int rc = kOk;
  std::string cwd = absolutePath(currentDir());
  for (auto& path : a.pos) {
    std::string why;
    if (!ctx.allow(path, why)) {
      io.err += "rm: " + path + ": " + why + "\n";
      rc = kUsage;
      continue;
    }
    FileInfo fi;
    if (!statPath(path, fi)) {
      if (!a.has("force")) {
        io.err += "rm: " + path + ": not found\n";
        rc = kNotFound;
      }
      continue;
    }
    std::string abs = absolutePath(path);
    bool isRootish = abs.size() <= 3 && (abs == "/" || (abs.size() >= 2 && abs[1] == ':'));
    bool ancestorOfCwd = cwd == abs || (cwd.size() > abs.size() && cwd.compare(0, abs.size(), abs) == 0 &&
                                        (abs.back() == '/' || cwd[abs.size()] == '/'));
    if (isRootish || ancestorOfCwd) {
      io.err += "rm: refusing to remove " + path + " (it is, or contains, the current directory)\n";
      rc = kUsage;
      continue;
    }
    if (fi.isDir && !fi.isLink && !a.has("recursive")) {
      io.err += "rm: " + path + " is a directory; pass -r to remove it\n";
      rc = kNotFound;
      continue;
    }
    std::string err;
    bool permanent = a.has("permanent");
    if (permanent) {
      std::error_code ec;
      fs::remove_all(toPath(path), ec);
      if (ec) err = ec.message();
    } else if (!moveToTrash(path, err)) {
      io.err += "rm: " + path + ": " + err + " (pass --permanent to delete outright)\n";
      rc = kNotFound;
      continue;
    }
    if (!err.empty()) {
      io.err += "rm: " + path + ": " + err + "\n";
      rc = kNotFound;
      continue;
    }
    io.out += std::string("removed ") + path + (permanent ? " (permanently)" : " (moved to trash)") + "\n";
  }
  return rc;
}

}  // namespace

const Command cmd_write = {"write",
                           "write <file> [--append] [--raw]          write stdin to a file atomically; keeps its line endings and BOM",
                           {{"append", 'a', false}, {"raw", 0, false}, {"no-parents", 0, false}},
                           cmdWrite};
const Command cmd_mv = {"mv", "mv <source>... <dest> [--force]            move or rename; refuses to overwrite unless --force",
                        {{"force", 'f', false}, {"no-parents", 0, false}}, cmdMv};
const Command cmd_cp = {"cp", "cp <source>... <dest> [-r] [--force]       copy files or (with -r) directories",
                        {{"recursive", 'r', false}, {"force", 'f', false}, {"no-parents", 0, false}}, cmdCp};
const Command cmd_rm = {"rm",
                        "rm <path>... [-r] [--permanent] [-f]     move to the OS trash (--permanent deletes outright)",
                        {{"recursive", 'r', false}, {"permanent", 0, false}, {"force", 'f', false}}, cmdRm};

}  // namespace acu
