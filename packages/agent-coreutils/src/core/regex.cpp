#include "core/regex.hpp"

#include <algorithm>
#include <array>
#include <cctype>
#include <atomic>
#include <cstdint>
#include <cstring>

namespace acu {

namespace {

constexpr uint32_t kInvalid = 0xFFFFFFFFu;
constexpr uint32_t kMaxCp = 0x10FFFF;

enum Op : uint8_t { CHAR, ANY, CLASS, SPLIT, JMP, ASSERT, MATCH };
enum Assertion : uint32_t { A_BOL, A_EOL, A_WORDB, A_NWORDB, A_NOWORD_BEFORE, A_NOWORD_AFTER };

struct Inst {
  Op op;
  uint8_t fold = 0;
  uint32_t x = 0, y = 0;
};

struct CC {
  std::vector<std::pair<uint32_t, uint32_t>> r;
  bool neg = false;
  uint64_t bits[2] = {0, 0};
};

struct Node;
using NP = std::unique_ptr<Node>;
struct Node {
  enum K { Empty, Char, Any, Class, Cat, Alt, Rep, Assert } k = Empty;
  uint32_t cp = 0;
  int cls = -1;
  std::vector<NP> kids;
  int mn = 0, mx = 0;
  uint32_t ak = 0;
};

inline bool isWordByte(unsigned char c) {
  return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_';
}

inline uint32_t foldAscii(uint32_t c) { return (c >= 'A' && c <= 'Z') ? c + 32 : c; }

// Decodes the code point at s[pos]; invalid bytes decode as kInvalid with length 1.
inline size_t decode(std::string_view s, size_t pos, uint32_t& cp) {
  unsigned char c = (unsigned char)s[pos];
  if (c < 0x80) {
    cp = c;
    return 1;
  }
  size_t n = (c >= 0xC2 && c < 0xE0) ? 2 : (c >= 0xE0 && c < 0xF0) ? 3 : (c >= 0xF0 && c < 0xF5) ? 4 : 0;
  if (!n || pos + n > s.size()) {
    cp = kInvalid;
    return 1;
  }
  uint32_t v = c & (0xFF >> (n + 1));
  for (size_t k = 1; k < n; ++k) {
    unsigned char d = (unsigned char)s[pos + k];
    if ((d & 0xC0) != 0x80) {
      cp = kInvalid;
      return 1;
    }
    v = (v << 6) | (d & 0x3F);
  }
  cp = v;
  return n;
}

void appendUtf8(std::string& o, uint32_t cp) {
  if (cp < 0x80) o.push_back((char)cp);
  else if (cp < 0x800) {
    o.push_back((char)(0xC0 | (cp >> 6)));
    o.push_back((char)(0x80 | (cp & 0x3F)));
  } else if (cp < 0x10000) {
    o.push_back((char)(0xE0 | (cp >> 12)));
    o.push_back((char)(0x80 | ((cp >> 6) & 0x3F)));
    o.push_back((char)(0x80 | (cp & 0x3F)));
  } else {
    o.push_back((char)(0xF0 | (cp >> 18)));
    o.push_back((char)(0x80 | ((cp >> 12) & 0x3F)));
    o.push_back((char)(0x80 | ((cp >> 6) & 0x3F)));
    o.push_back((char)(0x80 | (cp & 0x3F)));
  }
}

std::atomic<unsigned long long> g_skipped{0};

// ------------------------------------------------------------------ parser ---

struct Parser {
  std::vector<uint32_t> p;
  size_t i = 0;
  std::string err;
  bool icase = false;
  int depth = 0;
  std::vector<CC> classes;

  explicit Parser(const std::string& pat) {
    for (size_t k = 0; k < pat.size();) {
      uint32_t cp;
      k += decode(pat, k, cp);
      p.push_back(cp == kInvalid ? (uint32_t)(unsigned char)pat[k - 1] : cp);
    }
  }

  bool eof() const { return i >= p.size(); }
  bool fail(const std::string& m) {
    if (err.empty()) err = m;
    return false;
  }
  static NP mk(Node::K k) {
    NP n(new Node);
    n->k = k;
    return n;
  }

  NP parseAlt() {
    if (++depth > 200) {
      fail("pattern nested too deeply");
      return nullptr;
    }
    std::vector<NP> branches;
    for (;;) {
      NP c = parseCat();
      if (!c) return nullptr;
      branches.push_back(std::move(c));
      if (!eof() && p[i] == '|') {
        ++i;
        continue;
      }
      break;
    }
    --depth;
    if (branches.size() == 1) return std::move(branches[0]);
    NP a = mk(Node::Alt);
    a->kids = std::move(branches);
    return a;
  }

  NP parseCat() {
    NP cat = mk(Node::Cat);
    while (!eof() && p[i] != '|' && p[i] != ')') {
      NP r = parseRep();
      if (!r) return nullptr;
      if (r->k != Node::Empty) cat->kids.push_back(std::move(r));
    }
    if (cat->kids.empty()) return mk(Node::Empty);
    if (cat->kids.size() == 1) return std::move(cat->kids[0]);
    return cat;
  }

  bool parseCount(size_t& at, int& lo, int& hi) {
    // at points just after '{'. Accepts n, n, or n,m. Returns false (and leaves at) if not a quantifier.
    size_t j = at;
    auto num = [&](int& v) {
      if (j >= p.size() || p[j] < '0' || p[j] > '9') return false;
      long x = 0;
      while (j < p.size() && p[j] >= '0' && p[j] <= '9') {
        x = x * 10 + (p[j] - '0');
        if (x > 100000) x = 100000;
        ++j;
      }
      v = (int)x;
      return true;
    };
    if (!num(lo)) return false;
    hi = lo;
    if (j < p.size() && p[j] == ',') {
      ++j;
      if (j < p.size() && p[j] == '}') hi = -1;
      else if (!num(hi)) return false;
    }
    if (j >= p.size() || p[j] != '}') return false;
    at = j + 1;
    return true;
  }

  NP parseRep() {
    NP a = parseAtom();
    if (!a) return nullptr;
    for (;;) {
      if (eof()) break;
      uint32_t c = p[i];
      int lo, hi;
      if (c == '*') lo = 0, hi = -1, ++i;
      else if (c == '+') lo = 1, hi = -1, ++i;
      else if (c == '?') lo = 0, hi = 1, ++i;
      else if (c == '{') {
        size_t at = i + 1;
        if (!parseCount(at, lo, hi)) break;
        if (hi != -1 && hi < lo) {
          fail("bad repetition range");
          return nullptr;
        }
        if (lo > 1000 || hi > 1000) {
          fail("repetition count too large (max 1000)");
          return nullptr;
        }
        i = at;
      } else break;
      if (!eof() && (p[i] == '?' || p[i] == '+')) ++i;  // lazy/possessive: irrelevant for yes/no matching
      if (a->k == Node::Assert || a->k == Node::Empty) continue;
      NP r = mk(Node::Rep);
      r->mn = lo;
      r->mx = hi;
      r->kids.push_back(std::move(a));
      a = std::move(r);
    }
    return a;
  }

  void addRange(CC& cc, uint32_t a, uint32_t b) { cc.r.emplace_back(a, b); }

  void addClassEscape(CC& cc, uint32_t e) {
    switch (e) {
      case 'd': addRange(cc, '0', '9'); break;
      case 'w':
        addRange(cc, '0', '9');
        addRange(cc, 'A', 'Z');
        addRange(cc, 'a', 'z');
        addRange(cc, '_', '_');
        break;
      case 's':
        addRange(cc, 9, 13);
        addRange(cc, ' ', ' ');
        break;
      case 'D':
        addRange(cc, 0, '0' - 1);
        addRange(cc, '9' + 1, kMaxCp);
        break;
      case 'W':
        addRange(cc, 0, '0' - 1);
        addRange(cc, '9' + 1, 'A' - 1);
        addRange(cc, 'Z' + 1, '_' - 1);
        addRange(cc, '_' + 1, 'a' - 1);
        addRange(cc, 'z' + 1, kMaxCp);
        break;
      case 'S':
        addRange(cc, 0, 8);
        addRange(cc, 14, 31);
        addRange(cc, 33, kMaxCp);
        break;
    }
  }

  // Escapes valid both in and outside classes that produce one code point. Returns false if not one.
  bool simpleEscape(uint32_t e, uint32_t& out) {
    switch (e) {
      case 'n': out = '\n'; return true;
      case 'r': out = '\r'; return true;
      case 't': out = '\t'; return true;
      case 'f': out = '\f'; return true;
      case 'v': out = '\v'; return true;
      case 'a': out = 7; return true;
      case 'e': out = 27; return true;
      case '0': out = 0; return true;
    }
    return false;
  }

  bool hexEscape(uint32_t& out) {  // after \x
    uint32_t v = 0;
    auto hv = [](uint32_t c) -> int {
      if (c >= '0' && c <= '9') return (int)(c - '0');
      if (c >= 'a' && c <= 'f') return (int)(c - 'a' + 10);
      if (c >= 'A' && c <= 'F') return (int)(c - 'A' + 10);
      return -1;
    };
    if (!eof() && p[i] == '{') {
      ++i;
      int n = 0;
      while (!eof() && p[i] != '}') {
        int h = hv(p[i]);
        if (h < 0 || ++n > 6) return fail("bad \\x{...} escape");
        v = v * 16 + (uint32_t)h;
        ++i;
      }
      if (eof()) return fail("bad \\x{...} escape");
      ++i;
    } else {
      for (int k = 0; k < 2; ++k) {
        if (eof() || hv(p[i]) < 0) return fail("bad \\x escape");
        v = v * 16 + (uint32_t)hv(p[i++]);
      }
    }
    out = v;
    return true;
  }

  NP parseClass() {  // after '['
    CC cc;
    if (!eof() && p[i] == '^') {
      cc.neg = true;
      ++i;
    }
    bool first = true;
    for (;;) {
      if (eof()) {
        fail("unclosed character class");
        return nullptr;
      }
      uint32_t c = p[i];
      if (c == ']' && !first) {
        ++i;
        break;
      }
      first = false;
      if (c == '[' && i + 1 < p.size() && p[i + 1] == ':') {
        size_t j = i + 2;
        std::string name;
        while (j < p.size() && p[j] != ':' && p[j] < 128) name.push_back((char)p[j++]);
        if (j + 1 < p.size() && p[j] == ':' && p[j + 1] == ']') {
          i = j + 2;
          if (name == "alpha") addRange(cc, 'A', 'Z'), addRange(cc, 'a', 'z');
          else if (name == "digit") addRange(cc, '0', '9');
          else if (name == "alnum") addRange(cc, '0', '9'), addRange(cc, 'A', 'Z'), addRange(cc, 'a', 'z');
          else if (name == "upper") addRange(cc, 'A', 'Z');
          else if (name == "lower") addRange(cc, 'a', 'z');
          else if (name == "space") addRange(cc, 9, 13), addRange(cc, ' ', ' ');
          else if (name == "blank") addRange(cc, 9, 9), addRange(cc, ' ', ' ');
          else if (name == "punct")
            addRange(cc, 33, 47), addRange(cc, 58, 64), addRange(cc, 91, 96), addRange(cc, 123, 126);
          else if (name == "word") addClassEscape(cc, 'w');
          else if (name == "xdigit") addRange(cc, '0', '9'), addRange(cc, 'A', 'F'), addRange(cc, 'a', 'f');
          else if (name == "print") addRange(cc, 32, 126);
          else if (name == "cntrl") addRange(cc, 0, 31), addRange(cc, 127, 127);
          else {
            fail("unknown POSIX class [:" + name + ":]");
            return nullptr;
          }
          continue;
        }
      }
      ++i;
      uint32_t lo = c;
      if (c == '\\') {
        if (eof()) {
          fail("trailing backslash");
          return nullptr;
        }
        uint32_t e = p[i++];
        if (e == 'd' || e == 'w' || e == 's' || e == 'D' || e == 'W' || e == 'S') {
          addClassEscape(cc, e);
          continue;
        }
        if (e == 'x') {
          if (!hexEscape(lo)) return nullptr;
        } else if (e == 'b') {
          lo = 8;
        } else if (!simpleEscape(e, lo)) {
          lo = e;
        }
      }
      uint32_t hi = lo;
      if (i + 1 < p.size() && p[i] == '-' && p[i + 1] != ']') {
        ++i;
        uint32_t h = p[i++];
        if (h == '\\') {
          if (eof()) {
            fail("trailing backslash");
            return nullptr;
          }
          uint32_t e = p[i++];
          if (e == 'x') {
            if (!hexEscape(h)) return nullptr;
          } else if (!simpleEscape(e, h)) {
            h = e;
          }
        }
        hi = h;
        if (hi < lo) {
          fail("bad range in character class");
          return nullptr;
        }
      }
      addRange(cc, lo, hi);
    }
    NP n = mk(Node::Class);
    classes.push_back(std::move(cc));
    n->cls = (int)classes.size() - 1;
    return n;
  }

  NP charNode(uint32_t cp) {
    NP n = mk(Node::Char);
    n->cp = cp;
    return n;
  }

  NP classNode(uint32_t esc) {
    CC cc;
    addClassEscape(cc, esc);
    NP n = mk(Node::Class);
    classes.push_back(std::move(cc));
    n->cls = (int)classes.size() - 1;
    return n;
  }

  NP assertNode(uint32_t kind) {
    NP n = mk(Node::Assert);
    n->ak = kind;
    return n;
  }

  NP parseAtom() {
    uint32_t c = p[i++];
    switch (c) {
      case '(': {
        if (!eof() && p[i] == '?') {
          ++i;
          if (!eof() && p[i] == 'i' && i + 1 < p.size() && p[i + 1] == ')') {
            i += 2;
            icase = true;
            return mk(Node::Empty);
          }
          if (!eof() && p[i] == ':') {
            ++i;
          } else if (!eof() && (p[i] == '<' || p[i] == 'P') && !(i + 1 < p.size() && (p[i + 1] == '=' || p[i + 1] == '!'))) {
            // named group: skip the name
            while (!eof() && p[i] != '>') ++i;
            if (eof()) {
              fail("bad group name");
              return nullptr;
            }
            ++i;
          } else if (!eof() && p[i] == 'i' && i + 1 < p.size() && p[i + 1] == ':') {
            i += 2;
            icase = true;
          } else {
            fail("look-around and other (?...) groups are not supported");
            return nullptr;
          }
        }
        NP inner = parseAlt();
        if (!inner) return nullptr;
        if (eof() || p[i] != ')') {
          fail("unclosed group");
          return nullptr;
        }
        ++i;
        return inner;
      }
      case ')': fail("unmatched ')'"); return nullptr;
      case '[': return parseClass();
      case '.': return mk(Node::Any);
      case '^': return assertNode(A_BOL);
      case '$': return assertNode(A_EOL);
      case '*':
      case '+':
      case '?': fail("nothing to repeat"); return nullptr;
      case '\\': {
        if (eof()) {
          fail("trailing backslash");
          return nullptr;
        }
        uint32_t e = p[i++];
        switch (e) {
          case 'd': case 'D': case 'w': case 'W': case 's': case 'S': return classNode(e);
          case 'b': return assertNode(A_WORDB);
          case 'B': return assertNode(A_NWORDB);
          case 'A': return assertNode(A_BOL);
          case 'z': case 'Z': return assertNode(A_EOL);
          case 'x': {
            uint32_t v;
            if (!hexEscape(v)) return nullptr;
            return charNode(v);
          }
          case 'p': case 'P': fail("Unicode property classes (\\p) are not supported"); return nullptr;
        }
        if (e >= '1' && e <= '9') {
          fail("back-references are not supported");
          return nullptr;
        }
        uint32_t v;
        if (simpleEscape(e, v)) return charNode(v);
        return charNode(e);
      }
      default: return charNode(c);
    }
  }
};

// ----------------------------------------------------------- analysis -------

bool nullable(const Node* n) {
  switch (n->k) {
    case Node::Empty: case Node::Assert: return true;
    case Node::Char: case Node::Any: case Node::Class: return false;
    case Node::Cat:
      for (auto& k : n->kids)
        if (!nullable(k.get())) return false;
      return true;
    case Node::Alt:
      for (auto& k : n->kids)
        if (nullable(k.get())) return true;
      return false;
    case Node::Rep: return n->mn == 0 || nullable(n->kids[0].get());
  }
  return true;
}

void flushRun(std::string& run, std::string& best) {
  if (run.size() > best.size()) best = run;
  run.clear();
}

void collect(const Node* n, std::string& run, std::string& best, bool icase) {
  switch (n->k) {
    case Node::Char: appendUtf8(run, icase ? foldAscii(n->cp) : n->cp); break;
    case Node::Assert: case Node::Empty: break;
    case Node::Cat:
      for (auto& k : n->kids) collect(k.get(), run, best, icase);
      break;
    case Node::Rep: {
      flushRun(run, best);
      if (n->mn >= 1) {
        std::string r2;
        collect(n->kids[0].get(), r2, best, icase);
        flushRun(r2, best);
      }
      break;
    }
    default: flushRun(run, best); break;
  }
}

// Is `n` a plain string (a Char or a Cat of Chars)? Appends it to `out`.
bool plainString(const Node* n, std::string& out, bool icase) {
  if (n->k == Node::Char) {
    appendUtf8(out, icase ? foldAscii(n->cp) : n->cp);
    return true;
  }
  if (n->k == Node::Cat) {
    for (auto& k : n->kids)
      if (k->k != Node::Char) return false;
    for (auto& k : n->kids) appendUtf8(out, icase ? foldAscii(k->cp) : k->cp);
    return true;
  }
  return false;
}

}  // namespace

// ------------------------------------------------------------------- impl ---

struct Regex::Impl {
  std::vector<Inst> prog;
  std::vector<CC> classes;
  bool icase = false;
  bool anchoredStart = false;
  bool firstAll = true;
  std::array<bool, 256> first{};
  bool matchesEmpty = false;
};

namespace {

struct Compiler {
  Regex::Impl& im;
  std::string err;
  bool icase;

  bool emit(Inst i) {
    if (im.prog.size() > 300000) {
      err = "pattern too large";
      return false;
    }
    im.prog.push_back(i);
    return true;
  }
  size_t pc() const { return im.prog.size(); }

  bool gen(const Node* n) {
    switch (n->k) {
      case Node::Empty: return true;
      case Node::Char: {
        Inst i{CHAR};
        i.fold = icase && n->cp < 128 ? 1 : 0;
        i.x = i.fold ? foldAscii(n->cp) : n->cp;
        return emit(i);
      }
      case Node::Any: return emit(Inst{ANY});
      case Node::Class: {
        Inst i{CLASS};
        i.x = (uint32_t)n->cls;
        return emit(i);
      }
      case Node::Assert: {
        Inst i{ASSERT};
        i.x = n->ak;
        return emit(i);
      }
      case Node::Cat:
        for (auto& k : n->kids)
          if (!gen(k.get())) return false;
        return true;
      case Node::Alt: {
        std::vector<size_t> jumps;
        for (size_t k = 0; k < n->kids.size(); ++k) {
          bool last = k + 1 == n->kids.size();
          size_t split = 0;
          if (!last) {
            split = pc();
            if (!emit(Inst{SPLIT})) return false;
            im.prog[split].x = (uint32_t)pc();
          }
          if (!gen(n->kids[k].get())) return false;
          if (!last) {
            jumps.push_back(pc());
            if (!emit(Inst{JMP})) return false;
            im.prog[split].y = (uint32_t)pc();
          }
        }
        for (size_t j : jumps) im.prog[j].x = (uint32_t)pc();
        return true;
      }
      case Node::Rep: {
        const Node* c = n->kids[0].get();
        for (int k = 0; k < n->mn; ++k)
          if (!gen(c)) return false;
        if (n->mx == -1) {
          size_t split = pc();
          if (!emit(Inst{SPLIT})) return false;
          im.prog[split].x = (uint32_t)pc();
          if (!gen(c)) return false;
          Inst j{JMP};
          j.x = (uint32_t)split;
          if (!emit(j)) return false;
          im.prog[split].y = (uint32_t)pc();
        } else {
          std::vector<size_t> splits;
          for (int k = n->mn; k < n->mx; ++k) {
            splits.push_back(pc());
            if (!emit(Inst{SPLIT})) return false;
            im.prog[splits.back()].x = (uint32_t)pc();
            if (!gen(c)) return false;
          }
          for (size_t s : splits) im.prog[s].y = (uint32_t)pc();
        }
        return true;
      }
    }
    return true;
  }
};

void finalizeClass(CC& cc, bool icase) {
  if (icase) {
    size_t n = cc.r.size();
    for (size_t k = 0; k < n; ++k) {
      auto [a, b] = cc.r[k];
      uint32_t lo = std::max<uint32_t>(a, 'A'), hi = std::min<uint32_t>(b, 'Z');
      if (lo <= hi) cc.r.emplace_back(lo + 32, hi + 32);
      lo = std::max<uint32_t>(a, 'a');
      hi = std::min<uint32_t>(b, 'z');
      if (lo <= hi) cc.r.emplace_back(lo - 32, hi - 32);
    }
  }
  for (uint32_t c = 0; c < 128; ++c) {
    bool in = false;
    for (auto& r : cc.r)
      if (c >= r.first && c <= r.second) {
        in = true;
        break;
      }
    if (in != cc.neg) cc.bits[c >> 6] |= 1ull << (c & 63);
  }
}

// Sets the possible first bytes of a match. Returns whether `n` can match the empty string.
bool firstBytes(const Node* n, std::array<bool, 256>& f, const std::vector<CC>& classes, bool icase, bool& all) {
  switch (n->k) {
    case Node::Empty: case Node::Assert: return true;
    case Node::Char: {
      std::string s;
      appendUtf8(s, n->cp);
      unsigned char b = (unsigned char)s[0];
      f[b] = true;
      if (icase && b < 128) {
        f[(unsigned char)std::toupper(b)] = true;
        f[(unsigned char)std::tolower(b)] = true;
      }
      return false;
    }
    case Node::Any: all = true; return false;
    case Node::Class: {
      const CC& cc = classes[(size_t)n->cls];
      for (unsigned c = 0; c < 128; ++c)
        if (cc.bits[c >> 6] >> (c & 63) & 1) f[c] = true;
      bool high = cc.neg;
      for (auto& r : cc.r)
        if (r.second >= 128) high = true;
      if (high)
        for (unsigned c = 128; c < 256; ++c) f[c] = true;
      return false;
    }
    case Node::Cat:
      for (auto& k : n->kids)
        if (!firstBytes(k.get(), f, classes, icase, all)) return false;
      return true;
    case Node::Alt: {
      bool nul = false;
      for (auto& k : n->kids) nul |= firstBytes(k.get(), f, classes, icase, all);
      return nul;
    }
    case Node::Rep: {
      bool nul = firstBytes(n->kids[0].get(), f, classes, icase, all);
      return nul || n->mn == 0;
    }
  }
  return true;
}

NP wrapWord(NP top) {
  NP cat = Parser::mk(Node::Cat);
  NP b = Parser::mk(Node::Assert);
  b->ak = A_NOWORD_BEFORE;
  NP a = Parser::mk(Node::Assert);
  a->ak = A_NOWORD_AFTER;
  cat->kids.push_back(std::move(b));
  cat->kids.push_back(std::move(top));
  cat->kids.push_back(std::move(a));
  return cat;
}

}  // namespace

bool Regex::compile(const std::string& pattern, bool icaseFlag, bool wordMode, Regex& out, std::string& err) {
  Parser ps(pattern);
  ps.icase = icaseFlag;
  NP top = ps.parseAlt();
  if (top && !ps.eof()) {
    ps.fail("unmatched ')'");
    top = nullptr;
  }
  if (!top) {
    err = ps.err.empty() ? "invalid pattern" : ps.err;
    return false;
  }
  bool icase = ps.icase;
  auto im = std::make_shared<Impl>();
  im->icase = icase;
  im->classes = std::move(ps.classes);
  for (auto& cc : im->classes) finalizeClass(cc, icase);

  // Prefilter literals come from the pattern as written, before the -w wrapper is added.
  out.literals.clear();
  out.litIcase = icase;
  out.pureLiterals = false;
  {
    std::string s;
    if (plainString(top.get(), s, icase) && !s.empty()) {
      out.literals.push_back(s);
      out.pureLiterals = !wordMode;
    } else if (top->k == Node::Alt && top->kids.size() <= 16) {
      std::vector<std::string> alts;
      bool allPlain = true;
      for (auto& k : top->kids) {
        std::string t;
        if (!plainString(k.get(), t, icase) || t.empty()) {
          allPlain = false;
          break;
        }
        alts.push_back(t);
      }
      if (allPlain) {
        out.literals = std::move(alts);
        out.pureLiterals = !wordMode;
      }
    }
    if (out.literals.empty()) {
      std::string run, best;
      collect(top.get(), run, best, icase);
      flushRun(run, best);
      if (!best.empty()) out.literals.push_back(best);
    }
  }

  std::array<bool, 256> first{};
  bool all = false;
  bool nul = firstBytes(top.get(), first, im->classes, icase, all);
  im->matchesEmpty = nul;
  im->firstAll = all || nul;
  im->first = first;
  {
    const Node* lead = top.get();
    while (lead->k == Node::Cat && !lead->kids.empty()) lead = lead->kids[0].get();
    im->anchoredStart = lead->k == Node::Assert && lead->ak == A_BOL;
  }

  if (wordMode) top = wrapWord(std::move(top));
  Compiler cmp{*im, "", icase};
  if (!cmp.gen(top.get())) {
    err = cmp.err;
    return false;
  }
  if (!cmp.emit(Inst{MATCH})) {
    err = cmp.err;
    return false;
  }
  out.impl_ = std::move(im);
  return true;
}

std::string Regex::escape(std::string_view lit) {
  std::string o;
  for (char c : lit) {
    if (std::strchr("\\.+*?()|[]{}^$", c) && c) o.push_back('\\');
    o.push_back(c);
  }
  return o;
}

unsigned long long Regex::skippedLines() { return g_skipped.load(); }

bool Regex::search(std::string_view s) const {
  const Impl& im = *impl_;
  const size_t n = s.size();
  const size_t plen = im.prog.size();
  const uint64_t cells = (uint64_t)plen * (n + 1);
  if (cells > (4ull << 30)) {  // would need more than 512 MB of state
    g_skipped.fetch_add(1);
    return false;
  }
  // thread_local objects with destructors corrupt the heap in static MinGW builds, so scratch space is a
  // per-thread pointer that is deliberately never freed.
  thread_local auto* visitedP = new std::vector<uint64_t>;
  thread_local auto* stackP = new std::vector<std::pair<uint32_t, uint32_t>>;
  std::vector<uint64_t>& visited = *visitedP;
  std::vector<std::pair<uint32_t, uint32_t>>& stack = *stackP;
  visited.assign((size_t)((cells + 63) / 64), 0);
  const Inst* prog = im.prog.data();

  auto run = [&](size_t start) -> bool {
    stack.clear();
    stack.emplace_back(0u, (uint32_t)start);
    while (!stack.empty()) {
      auto [pc, pos] = stack.back();
      stack.pop_back();
      for (;;) {
        uint64_t cell = (uint64_t)pc * (n + 1) + pos;
        uint64_t& w = visited[cell >> 6];
        uint64_t bit = 1ull << (cell & 63);
        if (w & bit) break;
        w |= bit;
        const Inst& in = prog[pc];
        switch (in.op) {
          case CHAR: {
            if (pos >= n) goto fail;
            if (in.x < 128) {
              unsigned char c = (unsigned char)s[pos];
              if (in.fold ? foldAscii(c) == in.x : c == in.x) {
                ++pos;
                ++pc;
                continue;
              }
              goto fail;
            }
            uint32_t cp;
            size_t len = decode(s, pos, cp);
            if (cp != in.x) goto fail;
            pos += (uint32_t)len;
            ++pc;
            continue;
          }
          case ANY: {
            if (pos >= n) goto fail;
            uint32_t cp;
            pos += (uint32_t)decode(s, pos, cp);
            ++pc;
            continue;
          }
          case CLASS: {
            if (pos >= n) goto fail;
            const CC& cc = im.classes[in.x];
            uint32_t cp;
            size_t len = decode(s, pos, cp);
            bool ok;
            if (cp < 128) {
              ok = cc.bits[cp >> 6] >> (cp & 63) & 1;
            } else {
              bool inr = false;
              if (cp != kInvalid)
                for (auto& r : cc.r)
                  if (cp >= r.first && cp <= r.second) {
                    inr = true;
                    break;
                  }
              ok = inr != cc.neg;
            }
            if (!ok) goto fail;
            pos += (uint32_t)len;
            ++pc;
            continue;
          }
          case SPLIT:
            stack.emplace_back(in.y, pos);
            pc = in.x;
            continue;
          case JMP: pc = in.x; continue;
          case ASSERT: {
            bool before = pos > 0 && isWordByte((unsigned char)s[pos - 1]);
            bool after = pos < n && isWordByte((unsigned char)s[pos]);
            bool ok = false;
            switch (in.x) {
              case A_BOL: ok = pos == 0; break;
              case A_EOL: ok = pos == n; break;
              case A_WORDB: ok = before != after; break;
              case A_NWORDB: ok = before == after; break;
              case A_NOWORD_BEFORE: ok = !before; break;
              case A_NOWORD_AFTER: ok = !after; break;
            }
            if (!ok) goto fail;
            ++pc;
            continue;
          }
          case MATCH: return true;
        }
      fail:
        break;
      }
    }
    return false;
  };

  if (im.anchoredStart) return run(0);
  for (size_t pos = 0; pos <= n; ++pos) {
    if (!im.firstAll) {
      while (pos < n && !im.first[(unsigned char)s[pos]]) ++pos;
      if (pos >= n) return false;
    }
    if (pos > 0 && pos < n && ((unsigned char)s[pos] & 0xC0) == 0x80) continue;  // stay on character boundaries
    if (run(pos)) return true;
  }
  return false;
}

}  // namespace acu
