#include "index/daemon.hpp"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <mutex>
#include <thread>

#include "cmd/cmd.hpp"
#include "core/json.hpp"
#include "core/platform.hpp"
#include "core/snapshot.hpp"
#include "core/walk.hpp"
#include "index/index.hpp"

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <sddl.h>
#endif

namespace acu {

bool daemonCanServe(const std::string& c) {
  static const char* names[] = {"grep", "glob", "ls", "skim", "find-def", "refs", "outline"};
  for (const char* n : names)
    if (c == n) return true;
  return false;
}

#ifdef _WIN32

namespace {

std::wstring widenUtf8(const std::string& s) {
  if (s.empty()) return {};
  int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
  std::wstring w((size_t)n, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), w.data(), n);
  return w;
}

std::string narrowWide(const wchar_t* w, size_t len) {
  if (!len) return {};
  int n = WideCharToMultiByte(CP_UTF8, 0, w, (int)len, nullptr, 0, nullptr, nullptr);
  std::string s((size_t)n, '\0');
  WideCharToMultiByte(CP_UTF8, 0, w, (int)len, s.data(), n, nullptr, nullptr);
  return s;
}

std::wstring pipeNameFor(const std::string& root) {
  std::string key = lower(root);
  uint64_t h = 1469598103934665603ull;
  for (unsigned char c : key) h = (h ^ c) * 1099511628211ull;
  wchar_t buf[64];
  swprintf(buf, 64, L"\\\\.\\pipe\\acu-%016llx", (unsigned long long)h);
  return buf;
}

bool writeAllH(HANDLE h, const char* p, size_t n) {
  while (n) {
    DWORD w = 0;
    if (!WriteFile(h, p, (DWORD)std::min<size_t>(n, 1u << 20), &w, nullptr) || !w) return false;
    p += w;
    n -= w;
  }
  return true;
}

bool readAllH(HANDLE h, char* p, size_t n) {
  while (n) {
    DWORD r = 0;
    if (!ReadFile(h, p, (DWORD)std::min<size_t>(n, 1u << 20), &r, nullptr) || !r) return false;
    p += r;
    n -= r;
  }
  return true;
}

bool writeFrame(HANDLE h, const std::string& s) {
  uint32_t n = (uint32_t)s.size();
  return writeAllH(h, (const char*)&n, 4) && writeAllH(h, s.data(), s.size());
}

bool readFrame(HANDLE h, std::string& s) {
  uint32_t n = 0;
  if (!readAllH(h, (char*)&n, 4) || n > (512u << 20)) return false;
  s.resize(n);
  return readAllH(h, s.data(), n);
}

HANDLE connectPipe(const std::string& root, bool wait) {
  std::wstring name = pipeNameFor(root);
  for (int attempt = 0; attempt < 2; ++attempt) {
    HANDLE h = CreateFileW(name.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr, OPEN_EXISTING, 0, nullptr);
    if (h != INVALID_HANDLE_VALUE) return h;
    if (GetLastError() != ERROR_PIPE_BUSY || !wait) break;
    WaitNamedPipeW(name.c_str(), 5000);
  }
  return INVALID_HANDLE_VALUE;
}

// ---------------------------------------------------------------- the daemon --

struct Event {
  std::string rel;
  DWORD action;
};

class Daemon {
 public:
  Daemon(std::string rootAbs, int idleMinutes) : root_(std::move(rootAbs)), idleMinutes_(idleMinutes) {}

  bool start(std::string& err) {
    Index ix;
    auto base = std::make_shared<Index>();
    if (!Index::load(indexPathFor(root_), *base, err)) return false;
    base->root = root_;
    base_ = base;
    std::wstring wroot = widenUtf8(root_);
    dir_ = CreateFileW(wroot.c_str(), FILE_LIST_DIRECTORY, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                       OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED, nullptr);
    if (dir_ == INVALID_HANDLE_VALUE) {
      err = "cannot watch " + root_;
      return false;
    }
    stopEvent_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    watcher_ = std::thread([this] { watchLoop(); });
    rescan();
    lastRequest_ = std::chrono::steady_clock::now();
    started_ = lastRequest_;
    maint_ = std::thread([this] { maintenanceLoop(); });
    return true;
  }

  ~Daemon() {
    stop_ = true;
    if (stopEvent_) SetEvent(stopEvent_);
    if (watcher_.joinable()) watcher_.join();
    if (maint_.joinable()) maint_.join();
    if (dir_ != INVALID_HANDLE_VALUE) CloseHandle(dir_);
    if (stopEvent_) CloseHandle(stopEvent_);
  }

  // Serves connections until told to stop or idle for too long.
  int serve(std::string& err) {
    SECURITY_ATTRIBUTES sa{};
    sa.nLength = sizeof sa;
    PSECURITY_DESCRIPTOR sd = nullptr;
    // owner only: other users on the machine cannot talk to this daemon
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:(A;;GA;;;OW)", SDDL_REVISION_1, &sd, nullptr)) {
      err = "cannot build the pipe's access rules";
      return 2;
    }
    sa.lpSecurityDescriptor = sd;
    std::wstring name = pipeNameFor(root_);
    bool first = true;
    while (!stop_) {
      HANDLE h = CreateNamedPipeW(name.c_str(),
                                  PIPE_ACCESS_DUPLEX | (first ? FILE_FLAG_FIRST_PIPE_INSTANCE : 0),
                                  PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                                  PIPE_UNLIMITED_INSTANCES, 1 << 20, 1 << 20, 0, &sa);
      if (h == INVALID_HANDLE_VALUE) {
        err = first ? "a daemon is already running for this project" : "cannot create the pipe";
        LocalFree(sd);
        return first ? 1 : 2;
      }
      first = false;
      if (ConnectNamedPipe(h, nullptr) || GetLastError() == ERROR_PIPE_CONNECTED) {
        std::string req;
        if (readFrame(h, req)) {
          std::string resp = handle(req);
          writeFrame(h, resp);
          FlushFileBuffers(h);
        }
        DisconnectNamedPipe(h);
      }
      CloseHandle(h);
    }
    LocalFree(sd);
    return 0;
  }

 private:
  // ---- request handling
  std::string handle(const std::string& reqText) {
    Json req;
    std::string perr;
    Json resp = Json::object();
    auto reply = [&](int rc, const std::string& out, const std::string& err) {
      resp.set("rc", Json::num(rc));
      resp.set("out", Json::str(out));
      resp.set("err", Json::str(err));
      return resp.dump();
    };
    if (!parseJson(reqText, req, perr)) return reply(2, "", "daemon: bad request: " + perr + "\n");
    const Json* cmd = req.get("cmd");
    if (!cmd || cmd->t != Json::Str) return reply(2, "", "daemon: no command\n");
    lastRequest_ = std::chrono::steady_clock::now();
    if (cmd->s == "__ping") return reply(0, "pong\n", "");
    if (cmd->s == "__stop") {
      stop_ = true;
      return reply(0, "stopping\n", "");
    }
    std::lock_guard<std::mutex> lk(mu_);
    refresh();
    if (cmd->s == "__status") return reply(0, statusText(), "");
    if (!daemonCanServe(cmd->s)) return reply(2, "", "daemon: '" + cmd->s + "' is not served by the daemon\n");
    std::vector<std::string> args;
    if (const Json* a = req.get("args"))
      for (auto& v : a->a) args.push_back(v.s);
    const Json* cwd = req.get("cwd");
    Ctx ctx;
    ctx.snap = snap_;
    ctx.pinnedIndex = base_;
    ctx.noDaemon = true;
    std::string prev = currentDir();
    if (cwd && cwd->t == Json::Str && !setCurrentDir(cwd->s)) return reply(2, "", "daemon: cannot enter " + cwd->s + "\n");
    Io io;
    int rc;
    try {
      rc = runCommandLocal(ctx, cmd->s, args, io);
    } catch (const std::exception& e) {
      io.err += std::string("daemon: ") + e.what() + "\n";
      rc = 2;
    }
    setCurrentDir(prev);
    return reply(rc, io.out, io.err);
  }

  std::string statusText() {
    auto up = std::chrono::duration_cast<std::chrono::seconds>(std::chrono::steady_clock::now() - started_).count();
    size_t overlay = 0;
    for (auto& f : files_) overlay += f.idx < 0;
    return "daemon for " + root_ + "\n  pid " + std::to_string(GetCurrentProcessId()) + ", up " + std::to_string(up) +
           " s, " + fmtInt(files_.size()) + " files (" + fmtInt(overlay) + " changed since the index was built), index " +
           fmtInt(base_->memoryBytes() / 1024) + " KB in memory, " + fmtInt(events_) + " watcher events, " + fmtInt(rescans_) +
           " rescans, " + fmtInt(rebuilds_) + " index rebuilds\n";
  }

  // ---- keeping the snapshot fresh (call with mu_ held)
  void refresh() {
    flushWatcher();
    applyDirty();
    size_t overlay = 0;
    for (auto& f : files_) overlay += f.idx < 0;
    if (overlay > std::max<size_t>(500, files_.size() / 10)) rebuildBase();
    if (snapDirty_) {
      auto s = std::make_shared<Snapshot>();
      s->root = root_;
      s->files = files_;
      snap_ = s;
      snapDirty_ = false;
    }
  }

  // Write a sentinel file and wait for the watcher to report it. Events arrive in order, so once it shows up,
  // everything that happened before this call has been queued.
  void flushWatcher() {
    uint64_t n = ++syncSent_;
    std::string name = ".sync-" + std::to_string(GetCurrentProcessId()) + "-" + std::to_string(n);
    std::string path = joinPath(joinPath(root_, ".acu"), name);
    std::wstring wp = widenUtf8(path);
    HANDLE f = CreateFileW(wp.c_str(), GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_DELETE, nullptr, CREATE_ALWAYS,
                           FILE_ATTRIBUTE_NORMAL | FILE_FLAG_DELETE_ON_CLOSE, nullptr);
    if (f == INVALID_HANDLE_VALUE) {
      needFull_ = true;  // cannot sync: be safe
      return;
    }
    {
      std::unique_lock<std::mutex> lk(dmu_);
      bool seen = dcv_.wait_for(lk, std::chrono::milliseconds(1500), [&] { return syncSeen_ >= n; });
      if (!seen) needFull_ = true;
    }
    CloseHandle(f);  // deletes the sentinel
  }

  void applyDirty() {
    std::vector<Event> ev;
    {
      std::lock_guard<std::mutex> lk(dmu_);
      ev.swap(dirty_);
      if (overflow_) {
        overflow_ = false;
        needFull_ = true;
      }
    }
    events_ += ev.size();
    if (needFull_ || ev.size() > 2000) {
      rescan();
      return;
    }
    for (auto& e : ev) {
      std::string name = e.rel.substr(e.rel.rfind('/') == std::string::npos ? 0 : e.rel.rfind('/') + 1);
      if (name == ".gitignore" || name == ".ignore" || name == ".acuignore") {
        rescan();  // ignore rules changed: what is visible may differ
        return;
      }
      bool gone = e.action == FILE_ACTION_REMOVED || e.action == FILE_ACTION_RENAMED_OLD_NAME;
      std::string abs = joinPath(root_, e.rel);
      FileInfo fi;
      bool exists = !gone && statPath(abs, fi);
      if (!exists) {
        removeRel(e.rel);
        continue;
      }
      if (fi.isDir) {
        bool created = e.action == FILE_ACTION_ADDED || e.action == FILE_ACTION_RENAMED_NEW_NAME;
        if (created && !pathIgnored(root_, e.rel, true)) {
          rescan();  // a new directory may hold many files
          return;
        }
        continue;
      }
      if (pathIgnored(root_, e.rel, false)) {
        removeRel(e.rel);
        continue;
      }
      auto it = std::lower_bound(files_.begin(), files_.end(), e.rel, [](const SnapEntry& a, const std::string& r) { return a.rel < r; });
      if (it != files_.end() && it->rel == e.rel) {
        it->size = fi.size;
        it->mtimeNs = fi.mtimeNs;
        it->idx = -1;
      } else {
        SnapEntry s;
        s.rel = e.rel;
        s.size = fi.size;
        s.mtimeNs = fi.mtimeNs;
        files_.insert(it, std::move(s));
      }
      snapDirty_ = true;
    }
  }

  void removeRel(const std::string& rel) {
    auto it = std::lower_bound(files_.begin(), files_.end(), rel, [](const SnapEntry& a, const std::string& r) { return a.rel < r; });
    if (it != files_.end() && it->rel == rel) {
      files_.erase(it);
      snapDirty_ = true;
    }
    // it may have been a directory: drop everything below it
    std::string dirPrefix = rel + "/";
    auto lo = std::lower_bound(files_.begin(), files_.end(), dirPrefix, [](const SnapEntry& a, const std::string& r) { return a.rel < r; });
    auto hi = lo;
    while (hi != files_.end() && hi->rel.compare(0, dirPrefix.size(), dirPrefix) == 0) ++hi;
    if (hi != lo) {
      files_.erase(lo, hi);
      snapDirty_ = true;
    }
  }

  // Walk the tree for real and compare with the index.
  void rescan() {
    ++rescans_;
    needFull_ = false;
    Ctx plain;
    plain.noDaemon = true;
    WalkOpts wo;
    wo.root = root_;
    std::vector<WalkEntry> entries;
    std::string err;
    files_.clear();
    if (!walk(wo, plain, entries, err)) {
      snapDirty_ = true;
      return;
    }
    const std::string prefix = root_ + "/";
    files_.reserve(entries.size());
    for (auto& e : entries) {
      SnapEntry s;
      s.rel = e.path.compare(0, prefix.size(), prefix) == 0 ? e.path.substr(prefix.size()) : e.path;
      s.size = e.size;
      s.mtimeNs = e.mtimeNs;
      int bi = base_->find(s.rel);
      s.idx = bi >= 0 && base_->files[(size_t)bi].size == s.size && base_->files[(size_t)bi].mtime == s.mtimeNs ? bi : -1;
      files_.push_back(std::move(s));
    }
    snapDirty_ = true;
  }

  void rebuildBase() {
    Ctx plain;
    plain.noDaemon = true;
    auto nix = std::make_shared<Index>();
    std::string err;
    if (!buildIndex(root_, plain, base_.get(), *nix, err, nullptr)) return;
    if (!nix->save(indexPathFor(root_), err)) return;
    base_ = nix;
    ++rebuilds_;
    rescan();
  }

  void maintenanceLoop() {
    while (!stop_) {
      std::this_thread::sleep_for(std::chrono::milliseconds(500));
      auto idle = std::chrono::duration_cast<std::chrono::seconds>(std::chrono::steady_clock::now() - lastRequest_).count();
      if (idleMinutes_ > 0 && idle > idleMinutes_ * 60) {
        stop_ = true;
        HANDLE h = connectPipe(root_, false);  // wake the accept loop
        if (h != INVALID_HANDLE_VALUE) CloseHandle(h);
        return;
      }
      if (idle > 20 && idle < 25) {  // quiet for a moment: fold accumulated changes into the on-disk index
        std::unique_lock<std::mutex> lk(mu_, std::try_to_lock);
        if (!lk.owns_lock()) continue;
        size_t overlay = 0;
        for (auto& f : files_) overlay += f.idx < 0;
        if (overlay > 0) {
          refresh();
          rebuildBase();
          refresh();
        }
        std::this_thread::sleep_for(std::chrono::seconds(6));
      }
    }
  }

  // ---- the watcher thread
  void watchLoop() {
    std::vector<char> buf(1 << 18), copy;
    OVERLAPPED ov{};
    ov.hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    const DWORD filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_SIZE |
                         FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_CREATION;
    bool issued = false;
    while (!stop_) {
      if (!issued) {
        ResetEvent(ov.hEvent);
        if (!ReadDirectoryChangesW(dir_, buf.data(), (DWORD)buf.size(), TRUE, filter, nullptr, &ov, nullptr)) {
          std::lock_guard<std::mutex> lk(dmu_);
          overflow_ = true;
          std::this_thread::sleep_for(std::chrono::milliseconds(100));
          continue;
        }
        issued = true;
      }
      HANDLE hs[2] = {ov.hEvent, stopEvent_};
      DWORD w = WaitForMultipleObjects(2, hs, FALSE, INFINITE);
      if (w != WAIT_OBJECT_0) break;
      DWORD n = 0;
      bool ok = GetOverlappedResult(dir_, &ov, &n, FALSE) != 0;
      issued = false;
      if (!ok || n == 0) {
        std::lock_guard<std::mutex> lk(dmu_);
        overflow_ = true;  // the buffer overflowed: some changes were lost
        dcv_.notify_all();
        continue;
      }
      copy.assign(buf.begin(), buf.begin() + n);
      ResetEvent(ov.hEvent);
      if (ReadDirectoryChangesW(dir_, buf.data(), (DWORD)buf.size(), TRUE, filter, nullptr, &ov, nullptr)) issued = true;  // re-arm before parsing
      std::lock_guard<std::mutex> lk(dmu_);
      size_t off = 0;
      for (;;) {
        auto* fn = (FILE_NOTIFY_INFORMATION*)(copy.data() + off);
        std::string rel = narrowWide(fn->FileName, fn->FileNameLength / 2);
        std::replace(rel.begin(), rel.end(), '\\', '/');
        if (rel.compare(0, 5, ".acu/") == 0) {
          size_t p = rel.rfind(".sync-");
          if (p != std::string::npos && fn->Action == FILE_ACTION_ADDED) {
            size_t dash = rel.rfind('-');
            uint64_t num = std::strtoull(rel.c_str() + dash + 1, nullptr, 10);
            if (num > syncSeen_) syncSeen_ = num;
          }
        } else if (rel.compare(0, 5, ".git/") != 0 && rel != ".git" && rel != ".acu") {
          dirty_.push_back({rel, fn->Action});
        }
        if (!fn->NextEntryOffset) break;
        off += fn->NextEntryOffset;
      }
      dcv_.notify_all();
    }
    if (issued) CancelIo(dir_);
    CloseHandle(ov.hEvent);
  }

  std::string root_;
  int idleMinutes_;
  std::mutex mu_;  // one request at a time (commands use the process's working directory)
  std::shared_ptr<Index> base_;
  std::vector<SnapEntry> files_;
  std::shared_ptr<Snapshot> snap_;
  bool snapDirty_ = true;
  bool needFull_ = false;
  HANDLE dir_ = INVALID_HANDLE_VALUE, stopEvent_ = nullptr;
  std::thread watcher_, maint_;
  std::atomic<bool> stop_{false};
  std::mutex dmu_;
  std::condition_variable dcv_;
  std::vector<Event> dirty_;
  bool overflow_ = false;
  uint64_t syncSent_ = 0, syncSeen_ = 0;
  std::chrono::steady_clock::time_point lastRequest_, started_;
  size_t events_ = 0, rescans_ = 0, rebuilds_ = 0;
};

std::string exePath() {
  wchar_t buf[MAX_PATH * 4];
  DWORD n = GetModuleFileNameW(nullptr, buf, (DWORD)(sizeof buf / sizeof buf[0]));
  return narrowWide(buf, n);
}

bool sendRequest(HANDLE h, const std::string& cwd, const std::string& cmd, const std::vector<std::string>& args, Json& resp) {
  Json req = Json::object();
  req.set("cwd", Json::str(cwd));
  req.set("cmd", Json::str(cmd));
  Json a = Json::array();
  for (auto& s : args) a.push(Json::str(s));
  req.set("args", std::move(a));
  if (!writeFrame(h, req.dump())) return false;
  std::string text, err;
  if (!readFrame(h, text)) return false;
  return parseJson(text, resp, err);
}

}  // namespace

bool daemonForward(const Ctx& ctx, const std::string& command, const std::vector<std::string>& argv, Io& io, int& rc) {
  if (!daemonCanServe(command) || !ctx.root.empty() || ctx.noDaemon || ctx.noIndex) return false;
  if (!getEnv("ACU_NO_DAEMON").empty() || !getEnv("ACU_NO_INDEX").empty()) return false;
  for (auto& a : argv)
    if (a == "--no-index" || a == "--no-daemon") return false;
  std::string root;
  if (!findIndexRoot(".", root)) return false;
  HANDLE h = connectPipe(root, false);
  if (h == INVALID_HANDLE_VALUE) return false;
  Json resp;
  bool ok = sendRequest(h, currentDir(), command, argv, resp);
  CloseHandle(h);
  if (!ok) return false;
  const Json* r = resp.get("rc");
  const Json* o = resp.get("out");
  const Json* e = resp.get("err");
  if (!r || !o || !e) return false;
  io.out += o->s;
  io.err += e->s;
  rc = (int)r->n;
  return true;
}

int daemonCommand(Ctx& ctx, Args& a, Io& io) {
  std::string sub = a.pos.empty() ? "status" : a.pos[0];
  std::string dir = a.pos.size() > 1 ? a.pos[1] : ".";
  std::string root;
  bool haveIndex = findIndexRoot(dir, root);

  if (sub == "run") {  // foreground server (what `start` launches)
    if (!haveIndex) {
      io.err += "daemon: no index here; run `acu index` first\n";
      return kNotFound;
    }
    int idle = (int)a.num("idle", 120);
    Daemon d(root, idle);
    std::string err;
    if (!d.start(err)) {
      io.err += "daemon: " + err + "\n";
      return kNotFound;
    }
    int rc = d.serve(err);
    if (rc) io.err += "daemon: " + err + "\n";
    return rc;
  }

  if (sub == "start") {
    if (!haveIndex) {  // build the index first
      std::string target = canonicalPath(dir);
      Io bio;
      Args ia;
      if (a.pos.size() > 1) ia.pos.push_back(dir);
      int brc = runCommandArgs(ctx, cmd_index, ia, bio);
      io.out += bio.out;
      io.err += bio.err;
      if (brc != 0 || !findIndexRoot(dir, root)) return kNotFound;
    }
    HANDLE existing = connectPipe(root, false);
    if (existing != INVALID_HANDLE_VALUE) {
      CloseHandle(existing);
      io.out += "daemon already running for " + root + "\n";
      return kOk;
    }
    std::wstring cmdline = L"\"" + widenUtf8(exePath()) + L"\" daemon run \"" + widenUtf8(root) + L"\" --idle " +
                           widenUtf8(std::to_string(a.num("idle", 120)));
    STARTUPINFOW si{};
    si.cb = sizeof si;
    PROCESS_INFORMATION pi{};
    if (!CreateProcessW(nullptr, cmdline.data(), nullptr, nullptr, FALSE,
                        DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW, nullptr, nullptr, &si, &pi)) {
      io.err += "daemon: could not start the process (error " + std::to_string(GetLastError()) + ")\n";
      return kNotFound;
    }
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    for (int i = 0; i < 400; ++i) {  // up to 20 s: the first rescan of a big tree takes a moment
      HANDLE h = connectPipe(root, false);
      if (h != INVALID_HANDLE_VALUE) {
        CloseHandle(h);
        io.out += "daemon started for " + root + "\n";
        return kOk;
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
    io.err += "daemon: started but not answering yet\n";
    return kNotFound;
  }

  if (!haveIndex) {
    io.out += "no index here, so no daemon\n";
    return sub == "stop" ? kOk : kNotFound;
  }
  HANDLE h = connectPipe(root, false);
  if (h == INVALID_HANDLE_VALUE) {
    io.out += "no daemon running for " + root + "\n";
    return sub == "stop" ? kOk : kNotFound;
  }
  Json resp;
  bool ok = sendRequest(h, currentDir(), sub == "stop" ? "__stop" : "__status", {}, resp);
  CloseHandle(h);
  if (!ok || !resp.get("out")) {
    io.err += "daemon: no answer\n";
    return kNotFound;
  }
  io.out += resp.get("out")->s;
  return kOk;
}

#else  // ------------------------------------------------------------ not Windows --

bool daemonForward(const Ctx&, const std::string&, const std::vector<std::string>&, Io&, int&) { return false; }

int daemonCommand(Ctx&, Args&, Io& io) {
  io.err += "daemon: not implemented on this platform yet; `acu index` still speeds up grep, skim, find-def and refs\n";
  return kUsage;
}

#endif

}  // namespace acu
