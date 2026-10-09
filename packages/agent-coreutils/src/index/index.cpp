#include "index/index.hpp"

#include <algorithm>
#include <chrono>
#include <cstring>

#include "core/platform.hpp"
#include "core/text.hpp"
#include "core/walk.hpp"

namespace acu {

namespace {

constexpr char kMagic[8] = {'A', 'C', 'U', 'I', 'D', 'X', '0', '1'};
constexpr uint64_t kMaxTrigramFile = 4ull << 20;   // larger files are not trigram-indexed
constexpr uint64_t kMaxReadFile = 64ull << 20;     // larger files are not even read
constexpr uint64_t kMaxSymbolFile = 8ull << 20;

// ---- little-endian writer / reader

struct Writer {
  std::string buf;
  void u8(uint8_t v) { buf.push_back((char)v); }
  void u16(uint16_t v) { buf.append((const char*)&v, 2); }
  void u32(uint32_t v) { buf.append((const char*)&v, 4); }
  void u64(uint64_t v) { buf.append((const char*)&v, 8); }
  void str(const std::string& s) {
    u32((uint32_t)s.size());
    buf += s;
  }
  void raw(const void* p, size_t n) { buf.append((const char*)p, n); }
};

struct Reader {
  const char* p;
  const char* end;
  bool ok = true;
  bool need(size_t n) {
    if ((size_t)(end - p) < n) ok = false;
    return ok;
  }
  uint8_t u8() {
    if (!need(1)) return 0;
    return (uint8_t)*p++;
  }
  uint16_t u16() {
    uint16_t v = 0;
    if (need(2)) std::memcpy(&v, p, 2), p += 2;
    return v;
  }
  uint32_t u32() {
    uint32_t v = 0;
    if (need(4)) std::memcpy(&v, p, 4), p += 4;
    return v;
  }
  uint64_t u64() {
    uint64_t v = 0;
    if (need(8)) std::memcpy(&v, p, 8), p += 8;
    return v;
  }
  std::string str() {
    uint32_t n = u32();
    if (!need(n)) return {};
    std::string s(p, n);
    p += n;
    return s;
  }
};

uint64_t fnv(const char* p, size_t n) {
  uint64_t h = 1469598103934665603ull;
  for (size_t i = 0; i < n; ++i) h = (h ^ (unsigned char)p[i]) * 1099511628211ull;
  return h;
}

void putVarint(std::vector<uint8_t>& out, uint32_t v) {
  while (v >= 0x80) {
    out.push_back((uint8_t)(v | 0x80));
    v >>= 7;
  }
  out.push_back((uint8_t)v);
}

}  // namespace

// --------------------------------------------------------------------- Index --

int Index::find(const std::string& rel) const {
  auto it = std::lower_bound(files.begin(), files.end(), rel,
                             [](const IndexedFile& f, const std::string& r) { return f.path < r; });
  return it != files.end() && it->path == rel ? (int)(it - files.begin()) : -1;
}

void Index::decodePostings(size_t k, std::vector<uint32_t>& out) const {
  out.clear();
  const uint8_t* p = triData.data() + triOffs[k];
  const uint8_t* e = triData.data() + triOffs[k + 1];
  uint32_t prev = 0;
  while (p < e) {
    uint32_t v = 0;
    int shift = 0;
    while (p < e) {
      uint8_t b = *p++;
      v |= (uint32_t)(b & 0x7F) << shift;
      if (!(b & 0x80)) break;
      shift += 7;
    }
    prev += v;
    out.push_back(prev);
  }
}

bool Index::candidates(const std::vector<std::string>& literals, std::vector<uint32_t>& ids) const {
  ids.clear();
  if (literals.empty()) return false;
  for (auto& l : literals)
    if (l.size() < 3) return false;
  std::vector<uint32_t> result, list, acc, tmp;
  for (auto& lit : literals) {
    std::string low = lower(lit);
    struct T {
      size_t key;
      uint32_t bytes;
    };
    std::vector<T> ts;
    bool missing = false;
    for (size_t i = 0; i + 3 <= low.size() && !missing; ++i) {
      if (low[i] == '\n' || low[i + 1] == '\n' || low[i + 2] == '\n') continue;
      uint32_t key = (uint32_t)((unsigned char)low[i] << 16 | (unsigned char)low[i + 1] << 8 | (unsigned char)low[i + 2]);
      auto it = std::lower_bound(triKeys.begin(), triKeys.end(), key);
      if (it == triKeys.end() || *it != key) {
        missing = true;  // no indexed file has this trigram, so none contains the literal
        break;
      }
      size_t k = (size_t)(it - triKeys.begin());
      bool dup = false;
      for (auto& t : ts) dup |= t.key == k;
      if (!dup) ts.push_back({k, triOffs[k + 1] - triOffs[k]});
    }
    if (missing) continue;
    if (ts.empty()) return false;  // only newline trigrams: cannot narrow
    std::sort(ts.begin(), ts.end(), [](const T& a, const T& b) { return a.bytes < b.bytes; });
    if (ts.size() > 8) ts.resize(8);
    decodePostings(ts[0].key, acc);
    for (size_t i = 1; i < ts.size() && !acc.empty(); ++i) {
      decodePostings(ts[i].key, list);
      tmp.clear();
      std::set_intersection(acc.begin(), acc.end(), list.begin(), list.end(), std::back_inserter(tmp));
      acc.swap(tmp);
    }
    result.insert(result.end(), acc.begin(), acc.end());
  }
  for (size_t i = 0; i < files.size(); ++i)
    if (files[i].flags & kIdxNoTrigrams) result.push_back((uint32_t)i);
  std::sort(result.begin(), result.end());
  result.erase(std::unique(result.begin(), result.end()), result.end());
  ids = std::move(result);
  return true;
}

size_t Index::memoryBytes() const {
  size_t n = triData.size() + triKeys.size() * 4 + triOffs.size() * 4;
  for (auto& f : files) n += sizeof(f) + f.path.size();
  n += symbols.size() * (sizeof(Symbol) + 16);
  return n;
}

bool Index::save(const std::string& path, std::string& err) const {
  Writer w;
  w.raw(kMagic, 8);
  w.str(root);
  w.u32((uint32_t)files.size());
  for (auto& f : files) {
    w.str(f.path);
    w.u64(f.size);
    w.u64((uint64_t)f.mtime);
    w.u32(f.lines);
    w.u8(f.flags);
    w.u32(f.symBegin);
    w.u32(f.symEnd);
  }
  w.u32((uint32_t)symbols.size());
  for (auto& s : symbols) {
    w.u32(s.line);
    w.u16(s.depth);
    w.str(s.kind);
    w.str(s.name);
  }
  w.u32((uint32_t)triKeys.size());
  w.raw(triKeys.data(), triKeys.size() * 4);
  w.raw(triOffs.data(), triOffs.size() * 4);
  w.u64(triData.size());
  w.raw(triData.data(), triData.size());
  w.u64(fnv(w.buf.data(), w.buf.size()));
  std::error_code ec;
  fs::create_directories(toPath(path).parent_path(), ec);
  std::string dir = fromPath(toPath(path).parent_path());
  std::string ignore = joinPath(dir, ".gitignore");
  FileInfo fi;
  if (!statPath(ignore, fi)) {
    std::string e2;
    atomicWrite(ignore, "*\n", e2);  // keep the index out of version control
  }
  return atomicWrite(path, w.buf, err);
}

bool Index::load(const std::string& path, Index& out, std::string& err) {
  std::string raw;
  if (!readFile(path, raw, err)) return false;
  if (raw.size() < 16 || std::memcmp(raw.data(), kMagic, 8) != 0) {
    err = "not an acu index (or an older version); rebuild with `acu index`";
    return false;
  }
  uint64_t want;
  std::memcpy(&want, raw.data() + raw.size() - 8, 8);
  if (fnv(raw.data(), raw.size() - 8) != want) {
    err = "index is corrupt; rebuild with `acu index`";
    return false;
  }
  Reader r{raw.data() + 8, raw.data() + raw.size() - 8};
  Index ix;
  ix.root = r.str();
  uint32_t nf = r.u32();
  if (nf > 50000000) r.ok = false;
  if (r.ok) ix.files.resize(nf);
  for (uint32_t i = 0; r.ok && i < nf; ++i) {
    IndexedFile& f = ix.files[i];
    f.path = r.str();
    f.size = r.u64();
    f.mtime = (int64_t)r.u64();
    f.lines = r.u32();
    f.flags = r.u8();
    f.symBegin = r.u32();
    f.symEnd = r.u32();
  }
  uint32_t ns = r.u32();
  if (ns > 200000000) r.ok = false;
  if (r.ok) ix.symbols.resize(ns);
  for (uint32_t i = 0; r.ok && i < ns; ++i) {
    Symbol& s = ix.symbols[i];
    s.line = r.u32();
    s.depth = r.u16();
    s.kind = r.str();
    s.name = r.str();
  }
  uint32_t nk = r.u32();
  if (r.ok && r.need((size_t)nk * 4 + ((size_t)nk + 1) * 4)) {
    ix.triKeys.resize(nk);
    std::memcpy(ix.triKeys.data(), r.p, (size_t)nk * 4);
    r.p += (size_t)nk * 4;
    ix.triOffs.resize((size_t)nk + 1);
    std::memcpy(ix.triOffs.data(), r.p, ((size_t)nk + 1) * 4);
    r.p += ((size_t)nk + 1) * 4;
  }
  uint64_t nd = r.u64();
  if (r.ok && r.need(nd)) {
    ix.triData.assign((const uint8_t*)r.p, (const uint8_t*)r.p + nd);
    r.p += nd;
  }
  if (!r.ok || (!ix.triOffs.empty() && ix.triOffs.back() != ix.triData.size())) {
    err = "index is truncated; rebuild with `acu index`";
    return false;
  }
  out = std::move(ix);
  return true;
}

// ------------------------------------------------------------------- building --

void indexOneFile(const std::string& absPath, const std::string& rel, uint64_t size, int64_t mtime, IndexedFile& f,
                  std::vector<Symbol>& syms, std::vector<uint32_t>& tris) {
  // Scratch space is a per-thread pointer that is never freed (see the note in regex.cpp).
  thread_local auto* seenP = new std::vector<uint64_t>((1u << 24) / 64, 0);
  thread_local auto* rawP = new std::string;
  thread_local auto* textP = new std::string;
  std::vector<uint64_t>& seen = *seenP;
  std::string &raw = *rawP, &text = *textP;
  f = IndexedFile{};
  f.path = rel;
  f.size = size;
  f.mtime = mtime;
  syms.clear();
  tris.clear();
  if (isBinaryExtension(rel)) {
    f.flags = kIdxBinary;
    return;
  }
  if (size > kMaxReadFile) {
    f.flags = kIdxNoTrigrams;
    return;
  }
  std::string err;
  if (!readFile(absPath, raw, err)) {
    f.flags = kIdxNoTrigrams;  // unreadable now: let a direct read report it
    return;
  }
  TextInfo ti;
  toDisplayText(raw, text, ti);
  if (ti.binary) {
    f.flags = kIdxBinary;
    return;
  }
  f.lines = (uint32_t)countLines(text);
  LangInfo li = detectLanguage(rel);
  if (li.family != Family::None && size <= kMaxSymbolFile) extractSymbols(li.family, text, syms);
  if (size > kMaxTrigramFile) {
    f.flags = kIdxNoTrigrams;
    return;
  }
  const unsigned char* p = (const unsigned char*)text.data();
  const size_t n = text.size();
  unsigned a = 0, b = 0;
  if (n >= 1) a = (p[0] >= 'A' && p[0] <= 'Z') ? p[0] + 32u : p[0];
  if (n >= 2) b = (p[1] >= 'A' && p[1] <= 'Z') ? p[1] + 32u : p[1];
  for (size_t i = 2; i < n; ++i) {
    unsigned c = (p[i] >= 'A' && p[i] <= 'Z') ? p[i] + 32u : p[i];
    if (a != '\n' && b != '\n' && c != '\n') {
      uint32_t key = (a << 16) | (b << 8) | c;
      uint64_t& w = seen[key >> 6];
      uint64_t bit = 1ull << (key & 63);
      if (!(w & bit)) {
        w |= bit;
        tris.push_back(key);
      }
    }
    a = b;
    b = c;
  }
  for (uint32_t k : tris) seen[k >> 6] = 0;  // clear only what we touched (whole words are cheap to zero)
  std::sort(tris.begin(), tris.end());
}

bool buildIndex(const std::string& root, const Ctx& ctx, const Index* prev, Index& out, std::string& err,
                BuildStats* stats) {
  auto t0 = std::chrono::steady_clock::now();
  WalkOpts wo;
  wo.root = root;
  std::vector<WalkEntry> entries;
  if (!walk(wo, ctx, entries, err)) return false;
  const std::string prefix = root + "/";
  struct Per {
    IndexedFile f;
    std::vector<Symbol> syms;
    std::vector<uint32_t> tris;
    bool reuse = false;
    int prevIdx = -1;
  };
  std::vector<Per> per(entries.size());
  // Unchanged files are reused; their trigram lists come from inverting the previous postings once.
  std::vector<std::vector<uint32_t>> prevTris;
  size_t reused = 0;
  if (prev) {
    for (size_t i = 0; i < entries.size(); ++i) {
      std::string rel = entries[i].path.compare(0, prefix.size(), prefix) == 0 ? entries[i].path.substr(prefix.size()) : entries[i].path;
      int pi = prev->find(rel);
      if (pi >= 0 && prev->files[pi].size == entries[i].size && prev->files[pi].mtime == entries[i].mtimeNs) {
        per[i].reuse = true;
        per[i].prevIdx = pi;
        ++reused;
      }
    }
    if (reused) {
      prevTris.resize(prev->files.size());
      std::vector<char> want(prev->files.size(), 0);
      for (auto& p : per)
        if (p.reuse) want[(size_t)p.prevIdx] = 1;
      std::vector<uint32_t> list;
      for (size_t k = 0; k < prev->triKeys.size(); ++k) {
        prev->decodePostings(k, list);
        for (uint32_t id : list)
          if (want[id]) prevTris[id].push_back(prev->triKeys[k]);
      }
    }
  }
  parallelFor(entries.size(), [&](size_t i, size_t) {
    Per& p = per[i];
    if (p.reuse) {
      const IndexedFile& pf = prev->files[(size_t)p.prevIdx];
      p.f = pf;
      p.syms.assign(prev->symbols.begin() + pf.symBegin, prev->symbols.begin() + pf.symEnd);
      p.tris = std::move(prevTris[(size_t)p.prevIdx]);
      return;
    }
    std::string rel = entries[i].path.compare(0, prefix.size(), prefix) == 0 ? entries[i].path.substr(prefix.size()) : entries[i].path;
    indexOneFile(entries[i].path, rel, entries[i].size, entries[i].mtimeNs, p.f, p.syms, p.tris);
  });

  Index ix;
  ix.root = root;
  ix.files.reserve(per.size());
  size_t pairs = 0;
  for (size_t i = 0; i < per.size(); ++i) {
    IndexedFile f = per[i].f;
    f.symBegin = (uint32_t)ix.symbols.size();
    for (auto& s : per[i].syms) ix.symbols.push_back(std::move(s));
    f.symEnd = (uint32_t)ix.symbols.size();
    ix.files.push_back(std::move(f));
    pairs += per[i].tris.size();
  }
  size_t nkeys = 0;
  if (pairs <= (4u << 20)) {
    // small project: sort (trigram, file) pairs
    std::vector<uint64_t> all;
    all.reserve(pairs);
    for (size_t i = 0; i < per.size(); ++i)
      for (uint32_t t : per[i].tris) all.push_back((uint64_t)t << 32 | (uint32_t)i);
    std::sort(all.begin(), all.end());
    uint32_t curKey = 0, prevId = 0;
    bool open = false;
    for (uint64_t v : all) {
      uint32_t key = (uint32_t)(v >> 32), id = (uint32_t)v;
      if (!open || key != curKey) {
        ix.triKeys.push_back(key);
        ix.triOffs.push_back((uint32_t)ix.triData.size());
        curKey = key;
        prevId = 0;
        open = true;
        ++nkeys;
      }
      putVarint(ix.triData, id - prevId);
      prevId = id;
    }
  } else {
    // large project: counting sort over the 2^24 trigram space with a single array
    std::vector<uint32_t> pos((1u << 24) + 1, 0);
    for (auto& p : per)
      for (uint32_t t : p.tris) ++pos[t];
    uint32_t acc = 0;
    for (uint32_t t = 0; t < (1u << 24); ++t) {
      uint32_t c = pos[t];
      nkeys += c != 0;
      acc += c;
      pos[t] = acc;  // end offset of t's list
    }
    pos[1u << 24] = acc;
    std::vector<uint32_t> ids(pairs);
    for (size_t i = per.size(); i-- > 0;) {  // reverse order so each list ends up ascending
      for (uint32_t t : per[i].tris) ids[--pos[t]] = (uint32_t)i;
      std::vector<uint32_t>().swap(per[i].tris);
    }
    // now pos[t] is the start of t's list and pos[t+1] its end
    ix.triKeys.reserve(nkeys);
    ix.triOffs.reserve(nkeys + 1);
    for (uint32_t t = 0; t < (1u << 24); ++t) {
      uint32_t b0 = pos[t], e0 = t + 1 < (1u << 24) ? pos[t + 1] : pos[1u << 24];
      if (b0 == e0) continue;
      ix.triKeys.push_back(t);
      ix.triOffs.push_back((uint32_t)ix.triData.size());
      uint32_t prevId = 0;
      for (uint32_t j = b0; j < e0; ++j) {
        putVarint(ix.triData, ids[j] - prevId);
        prevId = ids[j];
      }
    }
  }
  ix.triOffs.push_back((uint32_t)ix.triData.size());
  out = std::move(ix);
  if (stats) {
    stats->files = entries.size();
    stats->reused = reused;
    stats->read = entries.size() - reused;
    stats->removed = 0;
    if (prev) {
      for (auto& pf : prev->files)
        if (out.find(pf.path) < 0) ++stats->removed;
    }
    stats->trigrams = nkeys;
    stats->ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
  }
  return true;
}

std::string indexPathFor(const std::string& root) { return joinPath(joinPath(root, ".acu"), "index"); }

bool findIndexRoot(const std::string& dir, std::string& root) {
  std::string cur = canonicalPath(dir);
  auto has = [&](const std::string& d) {
    FileInfo fi;
    return statPath(indexPathFor(d), fi) && !fi.isDir;
  };
  for (int i = 0; i < 64; ++i) {
    if (has(cur)) {
      root = cur;
      return true;
    }
    size_t slash = cur.rfind('/');
    if (slash == std::string::npos) break;
    if (slash == 0) {  // "/x" -> "/"
      if (cur.size() > 1 && has("/")) {
        root = "/";
        return true;
      }
      break;
    }
    cur = cur.substr(0, slash);
    if (cur.size() == 2 && cur[1] == ':') {  // "C:/x" -> "C:"
      if (has(cur)) {
        root = cur;
        return true;
      }
      break;
    }
  }
  return false;
}

}  // namespace acu
