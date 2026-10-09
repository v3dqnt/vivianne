#include "core/glob.hpp"

namespace acu {

static bool gm(const char* p, const char* pe, const char* t, const char* te);

// Parses a [...] class starting at p (which points at '['). Returns the position after ']' or null.
static const char* classEnd(const char* p, const char* pe) {
  const char* q = p + 1;
  if (q < pe && (*q == '!' || *q == '^')) ++q;
  if (q < pe && *q == ']') ++q;
  while (q < pe && *q != ']') {
    if (*q == '\\' && q + 1 < pe) ++q;
    ++q;
  }
  return q < pe ? q + 1 : nullptr;
}

static bool classMatch(const char* p, const char* end, unsigned char c) {
  const char* q = p + 1;
  bool neg = false;
  if (*q == '!' || *q == '^') {
    neg = true;
    ++q;
  }
  bool hit = false, first = true;
  const char* stop = end - 1;  // the closing ']'
  while (q < stop) {
    unsigned char lo = (unsigned char)*q;
    if (lo == '\\' && q + 1 < stop) lo = (unsigned char)*++q;
    ++q;
    unsigned char hi = lo;
    if (q + 1 < stop && *q == '-') {
      hi = (unsigned char)q[1];
      if (hi == '\\' && q + 2 < stop) {
        hi = (unsigned char)q[2];
        ++q;
      }
      q += 2;
    }
    (void)first;
    first = false;
    if (c >= lo && c <= hi) hit = true;
  }
  return hit != neg;
}

static bool gm(const char* p, const char* pe, const char* t, const char* te) {
  while (p < pe) {
    char c = *p;
    if (c == '*') {
      const char* q = p;
      while (q < pe && *q == '*') ++q;
      if (q - p >= 2) {
        if (q == pe) return true;  // trailing ** swallows the rest
        if (*q == '/') {           // **/ : zero or more directories
          const char* rest = q + 1;
          if (gm(rest, pe, t, te)) return true;
          for (const char* s = t; s < te; ++s)
            if (*s == '/' && gm(rest, pe, s + 1, te)) return true;
          return false;
        }
        for (const char* s = t; s <= te; ++s)
          if (gm(q, pe, s, te)) return true;
        return false;
      }
      if (q == pe) {  // trailing single star: no more '/'
        for (const char* s = t; s < te; ++s)
          if (*s == '/') return false;
        return true;
      }
      for (const char* s = t;; ++s) {
        if (gm(q, pe, s, te)) return true;
        if (s == te || *s == '/') return false;
      }
    }
    if (c == '?') {
      if (t >= te || *t == '/') return false;
      ++p;
      ++t;
      continue;
    }
    if (c == '[') {
      const char* e = classEnd(p, pe);
      if (e) {
        if (t >= te || *t == '/' || !classMatch(p, e, (unsigned char)*t)) return false;
        p = e;
        ++t;
        continue;
      }
    }
    if (c == '\\' && p + 1 < pe) {
      ++p;
      c = *p;
    }
    if (t >= te || *t != c) return false;
    ++p;
    ++t;
  }
  return t == te;
}

bool globMatch(std::string_view pattern, std::string_view text) {
  return gm(pattern.data(), pattern.data() + pattern.size(), text.data(), text.data() + text.size());
}

bool hasGlobChars(std::string_view s) { return s.find_first_of("*?[{\\") != std::string_view::npos; }

static void expand(const std::string& p, std::vector<std::string>& out) {
  if (out.size() >= 256) return;
  size_t open = std::string::npos;
  for (size_t i = 0; i < p.size(); ++i) {
    if (p[i] == '\\') {
      ++i;
      continue;
    }
    if (p[i] == '{') {
      open = i;
      break;
    }
  }
  if (open == std::string::npos) {
    out.push_back(p);
    return;
  }
  int depth = 0;
  size_t close = std::string::npos;
  std::vector<size_t> commas;
  for (size_t i = open; i < p.size(); ++i) {
    if (p[i] == '\\') {
      ++i;
      continue;
    }
    if (p[i] == '{') ++depth;
    else if (p[i] == '}') {
      if (--depth == 0) {
        close = i;
        break;
      }
    } else if (p[i] == ',' && depth == 1) {
      commas.push_back(i);
    }
  }
  if (close == std::string::npos || commas.empty()) {  // not an alternation: keep the brace literal
    std::string rest = p.substr(open + 1);
    std::vector<std::string> tail;
    expand(rest, tail);
    for (auto& t : tail) out.push_back(p.substr(0, open + 1) + t);
    return;
  }
  std::string pre = p.substr(0, open), post = p.substr(close + 1);
  size_t start = open + 1;
  commas.push_back(close);
  for (size_t c : commas) {
    expand(pre + p.substr(start, c - start) + post, out);
    start = c + 1;
  }
}

std::vector<std::string> expandBraces(const std::string& pattern) {
  std::vector<std::string> out;
  expand(pattern, out);
  return out;
}

}  // namespace acu
