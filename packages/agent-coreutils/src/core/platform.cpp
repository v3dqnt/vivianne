#include "core/platform.hpp"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <cstdio>
#include <cerrno>
#include <cstdlib>
#include <cstring>
#include <ctime>

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <fcntl.h>
#include <io.h>
#include <windows.h>
#include <shellapi.h>
#else
#include <dirent.h>
#include <fcntl.h>
#include <limits.h>
#include <pwd.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#endif

namespace acu {

static std::atomic<unsigned> g_tmpCounter{0};

#ifdef _WIN32
// ---------------------------------------------------------------- Windows ---

static std::wstring widen(const std::string& s) {
  if (s.empty()) return {};
  int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
  std::wstring w((size_t)n, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), w.data(), n);
  return w;
}

static std::string narrow(const std::wstring& w) {
  if (w.empty()) return {};
  int n = WideCharToMultiByte(CP_UTF8, 0, w.data(), (int)w.size(), nullptr, 0, nullptr, nullptr);
  std::string s((size_t)n, '\0');
  WideCharToMultiByte(CP_UTF8, 0, w.data(), (int)w.size(), s.data(), n, nullptr, nullptr);
  return s;
}

// Wide path with backslashes; long absolute paths get the \\?\ prefix.
static std::wstring winPath(const std::string& p) {
  std::wstring w = widen(p);
  for (auto& c : w)
    if (c == L'/') c = L'\\';
  if (w.empty()) w = L".";
  if (w.size() >= 240 && w.rfind(L"\\\\?\\", 0) != 0) {
    DWORD n = GetFullPathNameW(w.c_str(), 0, nullptr, nullptr);
    if (n) {
      std::wstring f(n, L'\0');
      DWORD m = GetFullPathNameW(w.c_str(), n, f.data(), nullptr);
      f.resize(m);
      if (f.rfind(L"\\\\", 0) == 0)
        w = L"\\\\?\\UNC\\" + f.substr(2);
      else
        w = L"\\\\?\\" + f;
    }
  }
  return w;
}

static std::string winErr(DWORD e) {
  switch (e) {
    case ERROR_FILE_NOT_FOUND:
    case ERROR_PATH_NOT_FOUND: return "not found";
    case ERROR_ACCESS_DENIED: return "permission denied";
    case ERROR_SHARING_VIOLATION:
    case ERROR_LOCK_VIOLATION: return "in use by another process";
    case ERROR_INVALID_NAME:
    case ERROR_BAD_PATHNAME: return "invalid path";
    default: return "error " + std::to_string(e);
  }
}

static int64_t ftToUnixNs(const FILETIME& ft) {
  uint64_t t = ((uint64_t)ft.dwHighDateTime << 32) | ft.dwLowDateTime;
  if (t < 116444736000000000ULL) return 0;
  return (int64_t)((t - 116444736000000000ULL) * 100ULL);
}

static int64_t ftToUnix(const FILETIME& ft) {
  uint64_t t = ((uint64_t)ft.dwHighDateTime << 32) | ft.dwLowDateTime;
  if (t < 116444736000000000ULL) return 0;
  return (int64_t)((t - 116444736000000000ULL) / 10000000ULL);
}

fs::path toPath(const std::string& s) { return fs::path(widen(s)); }
std::string fromPath(const fs::path& p) { return narrow(p.generic_wstring()); }

bool listDir(const std::string& dir, std::vector<DirEntry>& out) {
  std::wstring pat = winPath(dir);
  if (!pat.empty() && pat.back() != L'\\') pat.push_back(L'\\');
  pat.push_back(L'*');
  WIN32_FIND_DATAW fd;
  HANDLE h = FindFirstFileExW(pat.c_str(), FindExInfoBasic, &fd, FindExSearchNameMatch, nullptr,
                              FIND_FIRST_EX_LARGE_FETCH);
  if (h == INVALID_HANDLE_VALUE) return false;
  do {
    const wchar_t* n = fd.cFileName;
    if (n[0] == L'.' && (n[1] == 0 || (n[1] == L'.' && n[2] == 0))) continue;
    DirEntry e;
    e.name = narrow(n);
    e.isLink = (fd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
    e.isDir = (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
    e.size = ((uint64_t)fd.nFileSizeHigh << 32) | fd.nFileSizeLow;
    e.mtime = ftToUnix(fd.ftLastWriteTime);
    e.mtimeNs = ftToUnixNs(fd.ftLastWriteTime);
    out.push_back(std::move(e));
  } while (FindNextFileW(h, &fd));
  FindClose(h);
  return true;
}

bool statPath(const std::string& p, FileInfo& fi) {
  std::wstring w = winPath(p);
  WIN32_FILE_ATTRIBUTE_DATA d;
  if (!GetFileAttributesExW(w.c_str(), GetFileExInfoStandard, &d)) {
    fi = FileInfo{};
    return false;
  }
  fi.exists = true;
  fi.isLink = (d.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
  fi.isDir = (d.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
  fi.size = ((uint64_t)d.nFileSizeHigh << 32) | d.nFileSizeLow;
  fi.mtime = ftToUnix(d.ftLastWriteTime);
  fi.mtimeNs = ftToUnixNs(d.ftLastWriteTime);
  if (fi.isLink) {  // report the target's details
    HANDLE h = CreateFileW(w.c_str(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                           OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr);
    if (h != INVALID_HANDLE_VALUE) {
      BY_HANDLE_FILE_INFORMATION bi;
      if (GetFileInformationByHandle(h, &bi)) {
        fi.isDir = (bi.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
        fi.size = ((uint64_t)bi.nFileSizeHigh << 32) | bi.nFileSizeLow;
        fi.mtime = ftToUnix(bi.ftLastWriteTime);
        fi.mtimeNs = ftToUnixNs(bi.ftLastWriteTime);
      }
      CloseHandle(h);
    }
  }
  return true;
}

bool readFile(const std::string& p, std::string& out, std::string& err) {
  std::wstring w = winPath(p);
  HANDLE h = CreateFileW(w.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                         OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
  if (h == INVALID_HANDLE_VALUE) {
    DWORD e = GetLastError();
    DWORD a = GetFileAttributesW(w.c_str());
    if (a != INVALID_FILE_ATTRIBUTES && (a & FILE_ATTRIBUTE_DIRECTORY))
      err = "is a directory";
    else
      err = winErr(e);
    return false;
  }
  LARGE_INTEGER sz;
  if (!GetFileSizeEx(h, &sz)) sz.QuadPart = 0;
  out.resize((size_t)sz.QuadPart);
  size_t got = 0;
  while (got < out.size()) {
    DWORD chunk = (DWORD)std::min<size_t>(out.size() - got, 1u << 30), n = 0;
    if (!ReadFile(h, out.data() + got, chunk, &n, nullptr) || n == 0) break;
    got += n;
  }
  out.resize(got);
  CloseHandle(h);
  return true;
}

bool atomicWrite(const std::string& p, std::string_view data, std::string& err) {
  std::wstring target = winPath(p);
  DWORD ta = GetFileAttributesW(target.c_str());
  bool exists = ta != INVALID_FILE_ATTRIBUTES;
  if (exists && (ta & FILE_ATTRIBUTE_DIRECTORY)) {
    err = "is a directory";
    return false;
  }
  std::wstring tmp = target + L".acu-tmp-" + std::to_wstring(GetCurrentProcessId()) + L"-" +
                     std::to_wstring(g_tmpCounter.fetch_add(1));
  HANDLE h = CreateFileW(tmp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (h == INVALID_HANDLE_VALUE) {
    err = winErr(GetLastError());
    return false;
  }
  size_t done = 0;
  bool ok = true;
  while (done < data.size()) {
    DWORD chunk = (DWORD)std::min<size_t>(data.size() - done, 1u << 30), n = 0;
    if (!WriteFile(h, data.data() + done, chunk, &n, nullptr)) {
      ok = false;
      break;
    }
    done += n;
  }
  if (ok && !FlushFileBuffers(h)) ok = false;
  DWORD werr = ok ? 0 : GetLastError();
  CloseHandle(h);
  if (!ok) {
    DeleteFileW(tmp.c_str());
    err = winErr(werr);
    return false;
  }
  // One rename that replaces the target. ReplaceFileW would keep the old ACLs, but it moves the target aside
  // first, so a crash in the middle can leave no file at all (found by tests/crash_safety.py). Keep the old
  // file's hidden/system/archive flags instead.
  if (exists) SetFileAttributesW(tmp.c_str(), ta & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_ARCHIVE | FILE_ATTRIBUTE_NORMAL));
  bool moved = MoveFileExW(tmp.c_str(), target.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) != 0;
  if (!moved) {
    DWORD e = GetLastError();
    DeleteFileW(tmp.c_str());
    err = winErr(e);
    return false;
  }
  return true;
}

bool moveToTrash(const std::string& p, std::string& err) {
  std::error_code ec;
  fs::path abs = fs::absolute(toPath(p), ec);
  if (ec) {
    err = "cannot resolve path";
    return false;
  }
  abs.make_preferred();
  std::wstring from = abs.wstring();
  from.push_back(L'\0');  // SHFileOperation wants a double-null-terminated list
  SHFILEOPSTRUCTW op{};
  op.wFunc = FO_DELETE;
  op.pFrom = from.c_str();
  op.fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT;
  int r = SHFileOperationW(&op);
  if (r != 0 || op.fAnyOperationsAborted) {
    err = "could not move to the Recycle Bin (code " + std::to_string(r) + ")";
    return false;
  }
  return true;
}

void initProcess() {
  _setmode(_fileno(stdin), _O_BINARY);
  _setmode(_fileno(stdout), _O_BINARY);
  _setmode(_fileno(stderr), _O_BINARY);
}

std::vector<std::string> processArgs(int, char**) {
  std::vector<std::string> v;
  int n = 0;
  LPWSTR* a = CommandLineToArgvW(GetCommandLineW(), &n);
  if (!a) return v;
  for (int i = 1; i < n; ++i) v.push_back(narrow(a[i]));
  LocalFree(a);
  return v;
}

std::string getEnv(const char* name) {
  std::wstring w = widen(name);
  DWORD n = GetEnvironmentVariableW(w.c_str(), nullptr, 0);
  if (!n) return {};
  std::wstring v(n, L'\0');
  DWORD m = GetEnvironmentVariableW(w.c_str(), v.data(), n);
  v.resize(m);
  return narrow(v);
}

#else
// ------------------------------------------------------------------ POSIX ---

fs::path toPath(const std::string& s) { return fs::path(s); }
std::string fromPath(const fs::path& p) { return p.generic_string(); }

bool listDir(const std::string& dir, std::vector<DirEntry>& out) {
  DIR* d = opendir(dir.empty() ? "." : dir.c_str());
  if (!d) return false;
  std::string base = dir.empty() ? "." : dir;
  while (dirent* e = readdir(d)) {
    const char* n = e->d_name;
    if (n[0] == '.' && (n[1] == 0 || (n[1] == '.' && n[2] == 0))) continue;
    DirEntry de;
    de.name = n;
#ifdef _DIRENT_HAVE_D_TYPE
    unsigned char t = e->d_type;
#else
    unsigned char t = DT_UNKNOWN;
#endif
    if (t == DT_DIR) {
      de.isDir = true;
    } else if (t == DT_REG) {
    } else {  // symlink or unknown: ask the filesystem
      struct stat st;
      std::string full = base + "/" + de.name;
      if (lstat(full.c_str(), &st) == 0) {
        de.isLink = S_ISLNK(st.st_mode);
        if (de.isLink) {
          struct stat t2;
          de.isDir = stat(full.c_str(), &t2) == 0 && S_ISDIR(t2.st_mode);
        } else {
          de.isDir = S_ISDIR(st.st_mode);
        }
      }
    }
    out.push_back(std::move(de));  // size and mtime are left 0 here; statPath has them
  }
  closedir(d);
  return true;
}

bool statPath(const std::string& p, FileInfo& fi) {
  struct stat ls;
  if (lstat(p.c_str(), &ls) != 0) {
    fi = FileInfo{};
    return false;
  }
  fi.exists = true;
  fi.isLink = S_ISLNK(ls.st_mode);
  struct stat st = ls;
  if (fi.isLink && stat(p.c_str(), &st) != 0) st = ls;
  fi.isDir = S_ISDIR(st.st_mode);
  fi.size = (uint64_t)st.st_size;
  fi.mtime = (int64_t)st.st_mtime;
#if defined(__APPLE__)
  fi.mtimeNs = (int64_t)st.st_mtimespec.tv_sec * 1000000000LL + st.st_mtimespec.tv_nsec;
#else
  fi.mtimeNs = (int64_t)st.st_mtim.tv_sec * 1000000000LL + st.st_mtim.tv_nsec;
#endif
  return true;
}

static std::string errnoText(int e) {
  if (e == ENOENT || e == ENOTDIR) return "not found";
  if (e == EACCES || e == EPERM) return "permission denied";
  if (e == EISDIR) return "is a directory";
  return std::strerror(e);
}

bool readFile(const std::string& p, std::string& out, std::string& err) {
  int fd = open(p.c_str(), O_RDONLY);
  if (fd < 0) {
    err = errnoText(errno);
    return false;
  }
  struct stat st;
  if (fstat(fd, &st) == 0 && S_ISDIR(st.st_mode)) {
    close(fd);
    err = "is a directory";
    return false;
  }
  size_t want = fstat(fd, &st) == 0 ? (size_t)st.st_size : 0;
  out.resize(want);
  size_t got = 0;
  for (;;) {
    if (got == out.size()) out.resize(out.size() ? out.size() * 2 : 65536);  // /proc-style files report size 0
    ssize_t n = read(fd, out.data() + got, out.size() - got);
    if (n < 0) {
      if (errno == EINTR) continue;
      err = errnoText(errno);
      close(fd);
      return false;
    }
    if (n == 0) break;
    got += (size_t)n;
  }
  out.resize(got);
  close(fd);
  return true;
}

bool atomicWrite(const std::string& p, std::string_view data, std::string& err) {
  std::string target = p;
  struct stat st;
  if (lstat(p.c_str(), &st) == 0 && S_ISLNK(st.st_mode)) {
    char buf[PATH_MAX];
    if (realpath(p.c_str(), buf)) target = buf;
  }
  mode_t mode = 0666 & ~([] { mode_t m = umask(0); umask(m); return m; }());
  if (stat(target.c_str(), &st) == 0) {
    if (S_ISDIR(st.st_mode)) {
      err = "is a directory";
      return false;
    }
    mode = st.st_mode & 07777;
  }
  size_t slash = target.rfind('/');
  std::string dir = slash == std::string::npos ? "." : target.substr(0, slash ? slash : 1);
  std::string name = slash == std::string::npos ? target : target.substr(slash + 1);
  std::string tmp = dir + "/.acu-tmp-" + std::to_string(getpid()) + "-" + std::to_string(g_tmpCounter.fetch_add(1)) +
                    "-" + name;
  int fd = open(tmp.c_str(), O_WRONLY | O_CREAT | O_EXCL, mode);
  if (fd < 0) {
    err = errnoText(errno);
    return false;
  }
  fchmod(fd, mode);
  size_t done = 0;
  while (done < data.size()) {
    ssize_t n = write(fd, data.data() + done, data.size() - done);
    if (n < 0) {
      if (errno == EINTR) continue;
      err = errnoText(errno);
      close(fd);
      unlink(tmp.c_str());
      return false;
    }
    done += (size_t)n;
  }
#ifdef __APPLE__
  if (fcntl(fd, F_FULLFSYNC) != 0) fsync(fd);
#else
  fsync(fd);
#endif
  close(fd);
  if (rename(tmp.c_str(), target.c_str()) != 0) {
    err = errnoText(errno);
    unlink(tmp.c_str());
    return false;
  }
  return true;
}

bool moveToTrash(const std::string& p, std::string& err) {
  std::string home = getEnv("HOME");
  if (home.empty()) {
    if (passwd* pw = getpwuid(getuid())) home = pw->pw_dir;
  }
  if (home.empty()) {
    err = "no home directory for the trash";
    return false;
  }
  std::string abs = absolutePath(p);
  std::string name = abs.substr(abs.rfind('/') + 1);
  std::string filesDir, infoDir;
#ifdef __APPLE__
  filesDir = home + "/.Trash";
#else
  std::string data = getEnv("XDG_DATA_HOME");
  if (data.empty()) data = home + "/.local/share";
  filesDir = data + "/Trash/files";
  infoDir = data + "/Trash/info";
#endif
  std::error_code ec;
  fs::create_directories(filesDir, ec);
  if (!infoDir.empty()) fs::create_directories(infoDir, ec);
  std::string dest = filesDir + "/" + name;
  for (int i = 1; fs::exists(dest, ec) || (!infoDir.empty() && fs::exists(infoDir + "/" + fs::path(dest).filename().string() + ".trashinfo", ec)); ++i)
    dest = filesDir + "/" + name + "." + std::to_string(i);
  fs::rename(abs, dest, ec);
  if (ec) {  // other filesystem: copy, then remove
    fs::copy(abs, dest, fs::copy_options::recursive | fs::copy_options::copy_symlinks, ec);
    if (ec) {
      err = "could not move to the trash: " + ec.message();
      return false;
    }
    fs::remove_all(abs, ec);
  }
  if (!infoDir.empty()) {
    std::string enc;
    for (unsigned char c : abs) {
      if (std::isalnum(c) || c == '/' || c == '-' || c == '_' || c == '.' || c == '~') {
        enc.push_back((char)c);
      } else {
        char b[4];
        std::snprintf(b, sizeof b, "%%%02X", c);
        enc += b;
      }
    }
    std::time_t t = std::time(nullptr);
    char ts[32];
    std::strftime(ts, sizeof ts, "%Y-%m-%dT%H:%M:%S", std::localtime(&t));
    std::string info = "[Trash Info]\nPath=" + enc + "\nDeletionDate=" + ts + "\n";
    std::string e2;
    atomicWrite(infoDir + "/" + fs::path(dest).filename().string() + ".trashinfo", info, e2);
  }
  return true;
}

void initProcess() {}

std::vector<std::string> processArgs(int argc, char** argv) {
  std::vector<std::string> v;
  for (int i = 1; i < argc; ++i) v.push_back(argv[i]);
  return v;
}

std::string getEnv(const char* name) {
  const char* v = std::getenv(name);
  return v ? v : "";
}
#endif

// --------------------------------------------------------------- portable ---

static std::string stripSlash(std::string s) {
  while (s.size() > 1 && s.back() == '/' && !(s.size() == 3 && s[1] == ':')) s.pop_back();
  return s;
}

std::string currentDir() {
  std::error_code ec;
  return stripSlash(fromPath(fs::current_path(ec)));
}

bool setCurrentDir(const std::string& p) {
  std::error_code ec;
  fs::current_path(toPath(p), ec);
  return !ec;
}

std::string absolutePath(const std::string& p) {
  std::error_code ec;
  fs::path a = fs::absolute(toPath(p), ec);
  if (ec) return p;
  return stripSlash(fromPath(a.lexically_normal()));
}

#ifdef _WIN32
// std::filesystem::weakly_canonical does not follow junctions or symlinks on Windows, so ask the OS for the
// final path of the longest prefix that exists and append the rest.
static bool finalPathOf(const std::string& utf8, std::wstring& out) {
  HANDLE h = CreateFileW(winPath(utf8).c_str(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                         OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr);
  if (h == INVALID_HANDLE_VALUE) return false;
  DWORD n = GetFinalPathNameByHandleW(h, nullptr, 0, VOLUME_NAME_DOS);
  std::wstring buf(n + 1, L'\0');
  if (n) n = GetFinalPathNameByHandleW(h, buf.data(), n + 1, VOLUME_NAME_DOS);
  CloseHandle(h);
  if (!n) return false;
  buf.resize(n);
  if (buf.rfind(L"\\\\?\\UNC\\", 0) == 0) buf = L"\\\\" + buf.substr(8);
  else if (buf.rfind(L"\\\\?\\", 0) == 0) buf = buf.substr(4);
  out = buf;
  return true;
}

std::string canonicalPath(const std::string& p) {
  std::error_code ec;
  fs::path abs = fs::absolute(toPath(p), ec).lexically_normal();
  if (ec) return p;
  fs::path cur = abs;
  std::vector<fs::path> rest;
  for (;;) {
    std::wstring resolved;
    if (finalPathOf(fromPath(cur), resolved)) {
      fs::path res(resolved);
      for (size_t i = rest.size(); i-- > 0;) res /= rest[i];
      return stripSlash(fromPath(res.lexically_normal()));
    }
    if (!cur.has_relative_path() || cur == cur.root_path()) break;
    rest.push_back(cur.filename());
    cur = cur.parent_path();
  }
  return stripSlash(fromPath(abs));
}
#else
std::string canonicalPath(const std::string& p) {
  std::error_code ec;
  fs::path c = fs::weakly_canonical(toPath(p), ec);
  if (ec) return absolutePath(p);
  return stripSlash(fromPath(c));
}
#endif

bool readAllStdin(std::string& out) {
  char buf[65536];
  size_t n;
  while ((n = std::fread(buf, 1, sizeof buf, stdin)) > 0) out.append(buf, n);
  return true;
}

// Large single writes to a pipe can fail on Windows, so write in modest chunks and retry short writes.
static void writeAll(FILE* f, const std::string& s) {
  size_t done = 0;
  while (done < s.size()) {
    size_t n = std::fwrite(s.data() + done, 1, std::min<size_t>(s.size() - done, 32768), f);
    if (n == 0) break;
    done += n;
    std::fflush(f);
  }
}

void writeOut(const std::string& s) { writeAll(stdout, s); }

void writeErr(const std::string& s) { writeAll(stderr, s); }

}  // namespace acu
