#include "core/common.hpp"

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cstdlib>
#include <thread>

#include "core/platform.hpp"

namespace acu {

const std::string& Io::input() {
  if (!haveIn) {
    readAllStdin(in);
    haveIn = true;
  }
  return in;
}

std::string Args::str(const std::string& n, const std::string& def) const {
  auto it = opt.find(n);
  return it == opt.end() || it->second.empty() ? def : it->second.back();
}

long Args::num(const std::string& n, long def) const {
  auto it = opt.find(n);
  if (it == opt.end() || it->second.empty()) return def;
  const std::string& s = it->second.back();
  char* end = nullptr;
  errno = 0;
  long v = std::strtol(s.c_str(), &end, 10);
  if (s.empty() || *end != 0 || errno) throw UsageError{"--" + n + " expects a whole number, got '" + s + "'"};
  return v;
}

std::vector<std::string> Args::all(const std::string& n) const {
  auto it = opt.find(n);
  return it == opt.end() ? std::vector<std::string>{} : it->second;
}

bool parseArgs(const std::vector<std::string>& v, const std::vector<OptDef>& defs, Args& a, std::string& err) {
  auto findLong = [&](const std::string& n) -> const OptDef* {
    for (auto& d : defs)
      if (n == d.name) return &d;
    return nullptr;
  };
  auto findShort = [&](char c) -> const OptDef* {
    for (auto& d : defs)
      if (d.sh && d.sh == c) return &d;
    return nullptr;
  };
  bool endOpts = false;
  for (size_t i = 0; i < v.size(); ++i) {
    const std::string& t = v[i];
    if (endOpts || t.size() < 2 || t[0] != '-') {
      a.pos.push_back(t);
      continue;
    }
    if (t == "--") {
      endOpts = true;
      continue;
    }
    if (t[1] == '-') {
      std::string name = t.substr(2), val;
      bool hasVal = false;
      size_t eq = name.find('=');
      if (eq != std::string::npos) {
        val = name.substr(eq + 1);
        name.resize(eq);
        hasVal = true;
      }
      const OptDef* d = findLong(name);
      if (!d) {
        err = "unknown option --" + name;
        return false;
      }
      if (d->val) {
        if (!hasVal) {
          if (i + 1 >= v.size()) {
            err = "--" + name + " needs a value";
            return false;
          }
          val = v[++i];
        }
        a.opt[name].push_back(val);
      } else {
        if (hasVal) {
          err = "--" + name + " does not take a value";
          return false;
        }
        a.opt[name].push_back("1");
      }
    } else {
      if (std::all_of(t.begin() + 1, t.end(), [](unsigned char c) { return std::isdigit(c); })) {
        a.pos.push_back(t);  // "-5" is a number, not a flag
        continue;
      }
      for (size_t k = 1; k < t.size(); ++k) {
        const OptDef* d = findShort(t[k]);
        if (!d) {
          err = std::string("unknown option -") + t[k];
          return false;
        }
        if (d->val) {
          std::string val = t.substr(k + 1);
          if (val.empty()) {
            if (i + 1 >= v.size()) {
              err = std::string("-") + t[k] + " needs a value";
              return false;
            }
            val = v[++i];
          }
          a.opt[d->name].push_back(val);
          break;
        }
        a.opt[d->name].push_back("1");
      }
    }
  }
  return true;
}

bool Ctx::allow(const std::string& path, std::string& err) const {
  if (root.empty()) return true;
  std::string c = canonicalPath(path);
#ifdef _WIN32
  std::string cl = lower(c), rl = lower(root);
#else
  const std::string &cl = c, &rl = root;
#endif
  if (cl == rl) return true;
  if (cl.size() > rl.size() && cl.compare(0, rl.size(), rl) == 0 && (rl.back() == '/' || cl[rl.size()] == '/'))
    return true;
  err = "path is outside the allowed root (" + root + ")";
  return false;
}

std::string fmtInt(uint64_t n) {
  std::string s = std::to_string(n), r;
  int c = 0;
  for (size_t i = s.size(); i-- > 0;) {
    r.push_back(s[i]);
    if (++c % 3 == 0 && i) r.push_back(',');
  }
  std::reverse(r.begin(), r.end());
  return r;
}

void appendTrunc(std::string& out, std::string_view s, size_t max) {
  if (s.size() <= max) {
    out.append(s);
    return;
  }
  size_t cut = max;
  while (cut > 0 && ((unsigned char)s[cut] & 0xC0) == 0x80) --cut;
  size_t rest = 0;
  for (size_t i = cut; i < s.size(); ++i)
    if (((unsigned char)s[i] & 0xC0) != 0x80) ++rest;
  out.append(s.substr(0, cut));
  out += "\xE2\x80\xA6[+" + fmtInt(rest) + " chars]";
}

std::string joinPath(const std::string& a, const std::string& b) {
  if (a.empty() || a == ".") return b;
  if (a.back() == '/') return a + b;
  return a + "/" + b;
}

bool startsWith(std::string_view s, std::string_view p) { return s.size() >= p.size() && s.compare(0, p.size(), p) == 0; }
bool endsWith(std::string_view s, std::string_view p) {
  return s.size() >= p.size() && s.compare(s.size() - p.size(), p.size(), p) == 0;
}

std::string trim(std::string_view s) {
  size_t a = 0, b = s.size();
  while (a < b && std::isspace((unsigned char)s[a])) ++a;
  while (b > a && std::isspace((unsigned char)s[b - 1])) --b;
  return std::string(s.substr(a, b - a));
}

std::string lower(std::string_view s) {
  std::string r(s);
  for (auto& c : r)
    if (c >= 'A' && c <= 'Z') c = (char)(c + 32);
  return r;
}

std::string fmtTime(int64_t t) {
  // civil-from-days (Howard Hinnant)
  int64_t days = t / 86400, rem = t % 86400;
  if (rem < 0) {
    rem += 86400;
    --days;
  }
  days += 719468;
  int64_t era = (days >= 0 ? days : days - 146096) / 146097;
  int64_t doe = days - era * 146097;
  int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  int64_t y = yoe + era * 400;
  int64_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  int64_t mp = (5 * doy + 2) / 153;
  int64_t d = doy - (153 * mp + 2) / 5 + 1;
  int64_t m = mp < 10 ? mp + 3 : mp - 9;
  if (m <= 2) ++y;
  char b[32];
  std::snprintf(b, sizeof b, "%04lld-%02lld-%02lldT%02lld:%02lld:%02lldZ", (long long)y, (long long)m, (long long)d,
                (long long)(rem / 3600), (long long)(rem % 3600 / 60), (long long)(rem % 60));
  return b;
}

size_t threadCount(size_t cap) {
  std::string env = getEnv("ACU_THREADS");  // override for debugging and benchmarks
  if (!env.empty()) {
    long v = std::strtol(env.c_str(), nullptr, 10);
    if (v >= 1) return (size_t)v;
  }
  size_t hw = std::thread::hardware_concurrency();
  return std::min<size_t>(hw ? hw : 4, cap);
}

void parallelFor(size_t n, const std::function<void(size_t, size_t)>& fn) {
  size_t threads = std::min<size_t>(threadCount(16), n);
  if (threads <= 1) {
    for (size_t i = 0; i < n; ++i) fn(i, 0);
    return;
  }
  std::atomic<size_t> next{0};
  auto work = [&](size_t tid) {
    for (;;) {
      size_t i = next.fetch_add(1);
      if (i >= n) break;
      fn(i, tid);
    }
  };
  std::vector<std::thread> pool;
  for (size_t t = 1; t < threads; ++t) pool.emplace_back(work, t);
  work(0);
  for (auto& t : pool) t.join();
}

}  // namespace acu
