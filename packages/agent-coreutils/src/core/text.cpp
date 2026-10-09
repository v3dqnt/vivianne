#include "core/text.hpp"

#include <algorithm>
#include <cstdint>
#include <cstring>

namespace acu {

static bool hasUtf16Bom(std::string_view r, bool& be) {
  if (r.size() >= 2 && (unsigned char)r[0] == 0xFF && (unsigned char)r[1] == 0xFE) {
    be = false;
    return true;
  }
  if (r.size() >= 2 && (unsigned char)r[0] == 0xFE && (unsigned char)r[1] == 0xFF) {
    be = true;
    return true;
  }
  return false;
}

bool looksBinary(std::string_view raw) {
  bool be;
  if (hasUtf16Bom(raw, be)) return false;
  size_t n = std::min<size_t>(raw.size(), 8192);
  return n && std::memchr(raw.data(), 0, n) != nullptr;
}

static void put(std::string& o, uint32_t cp) {
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

void toDisplayText(std::string_view raw, std::string& out, TextInfo& ti) {
  out.clear();
  bool be = false;
  if (hasUtf16Bom(raw, be)) {
    ti.utf16 = true;
    ti.utf16be = be;
    out.reserve(raw.size() / 2);
    auto unit = [&](size_t i) -> uint32_t {
      unsigned char a = (unsigned char)raw[i], b = (unsigned char)raw[i + 1];
      return be ? (uint32_t)(a << 8 | b) : (uint32_t)(b << 8 | a);
    };
    for (size_t i = 2; i + 1 < raw.size(); i += 2) {
      uint32_t u = unit(i);
      if (u >= 0xD800 && u < 0xDC00 && i + 3 < raw.size()) {
        uint32_t lo = unit(i + 2);
        if (lo >= 0xDC00 && lo < 0xE000) {
          put(out, 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00));
          i += 2;
          continue;
        }
      }
      put(out, u);
    }
  } else if (looksBinary(raw)) {
    ti.binary = true;
    return;
  } else {
    if (raw.size() >= 3 && (unsigned char)raw[0] == 0xEF && (unsigned char)raw[1] == 0xBB && (unsigned char)raw[2] == 0xBF) {
      ti.bom = true;
      raw.remove_prefix(3);
    }
    out.assign(raw);
  }
  ti.crlf = dominantCrlf(out);
}

size_t countLines(std::string_view t) {
  if (t.empty()) return 0;
  size_t n = (size_t)std::count(t.begin(), t.end(), '\n');
  return t.back() == '\n' ? n : n + 1;
}

std::vector<size_t> lineStarts(std::string_view t) {
  std::vector<size_t> s;
  size_t pos = 0;
  while (pos < t.size()) {
    s.push_back(pos);
    const void* nl = std::memchr(t.data() + pos, '\n', t.size() - pos);
    if (!nl) {
      pos = t.size();
      break;
    }
    pos = (size_t)((const char*)nl - t.data()) + 1;
  }
  s.push_back(t.size());
  return s;
}

std::string_view lineAt(std::string_view t, const std::vector<size_t>& st, size_t i) {
  size_t a = st[i], b = st[i + 1];
  if (b > a && t[b - 1] == '\n') --b;
  if (b > a && t[b - 1] == '\r') --b;
  return t.substr(a, b - a);
}

bool dominantCrlf(std::string_view t) {
  size_t crlf = 0, lf = 0;
  for (size_t p = 0; (p = t.find('\n', p)) != std::string_view::npos; ++p) {
    if (p > 0 && t[p - 1] == '\r') ++crlf;
    else ++lf;
  }
  return crlf > lf;
}

std::string toCrlf(std::string_view lf) {
  std::string o;
  o.reserve(lf.size() + lf.size() / 40);
  for (size_t i = 0; i < lf.size(); ++i) {
    if (lf[i] == '\n' && (i == 0 || lf[i - 1] != '\r')) o.push_back('\r');
    o.push_back(lf[i]);
  }
  return o;
}

std::string toLf(std::string_view s) {
  std::string o;
  o.reserve(s.size());
  for (size_t i = 0; i < s.size(); ++i) {
    if (s[i] == '\r' && i + 1 < s.size() && s[i + 1] == '\n') continue;
    o.push_back(s[i]);
  }
  return o;
}

}  // namespace acu
