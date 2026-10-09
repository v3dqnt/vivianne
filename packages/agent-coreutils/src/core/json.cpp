#include "core/json.hpp"

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>

namespace acu {

static void putUtf8(std::string& o, uint32_t cp) {
  if (cp < 0x80) {
    o.push_back((char)cp);
  } else if (cp < 0x800) {
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

// Length of a valid UTF-8 sequence at s[i], or 0.
static size_t utf8Len(std::string_view s, size_t i) {
  unsigned char c = (unsigned char)s[i];
  size_t n = c < 0x80 ? 1 : (c >= 0xC2 && c < 0xE0) ? 2 : (c >= 0xE0 && c < 0xF0) ? 3 : (c >= 0xF0 && c < 0xF5) ? 4 : 0;
  if (!n || i + n > s.size()) return 0;
  for (size_t k = 1; k < n; ++k)
    if (((unsigned char)s[i + k] & 0xC0) != 0x80) return 0;
  if (n == 3) {
    unsigned char c1 = (unsigned char)s[i + 1];
    if ((c == 0xE0 && c1 < 0xA0) || (c == 0xED && c1 >= 0xA0)) return 0;
  }
  if (n == 4) {
    unsigned char c1 = (unsigned char)s[i + 1];
    if ((c == 0xF0 && c1 < 0x90) || (c == 0xF4 && c1 >= 0x90)) return 0;
  }
  return n;
}

std::string jsonQuote(std::string_view s) {
  std::string o = "\"";
  for (size_t i = 0; i < s.size();) {
    unsigned char c = (unsigned char)s[i];
    if (c == '"') o += "\\\"", ++i;
    else if (c == '\\') o += "\\\\", ++i;
    else if (c == '\n') o += "\\n", ++i;
    else if (c == '\r') o += "\\r", ++i;
    else if (c == '\t') o += "\\t", ++i;
    else if (c < 0x20) {
      char b[8];
      std::snprintf(b, sizeof b, "\\u%04x", c);
      o += b;
      ++i;
    } else if (c < 0x80) {
      o.push_back((char)c);
      ++i;
    } else {
      size_t n = utf8Len(s, i);
      if (!n) {
        o += "\xEF\xBF\xBD";
        ++i;
      } else {
        o.append(s.substr(i, n));
        i += n;
      }
    }
  }
  o.push_back('"');
  return o;
}

static void dumpTo(const Json& j, std::string& o) {
  switch (j.t) {
    case Json::Null: o += "null"; break;
    case Json::Bool: o += j.b ? "true" : "false"; break;
    case Json::Num: {
      char b[40];
      if (j.isInt && std::fabs(j.n) < 9e15) std::snprintf(b, sizeof b, "%lld", (long long)j.n);
      else std::snprintf(b, sizeof b, "%.17g", j.n);
      o += b;
      break;
    }
    case Json::Str: o += jsonQuote(j.s); break;
    case Json::Arr: {
      o.push_back('[');
      for (size_t i = 0; i < j.a.size(); ++i) {
        if (i) o.push_back(',');
        dumpTo(j.a[i], o);
      }
      o.push_back(']');
      break;
    }
    case Json::Obj: {
      o.push_back('{');
      for (size_t i = 0; i < j.o.size(); ++i) {
        if (i) o.push_back(',');
        o += jsonQuote(j.o[i].first);
        o.push_back(':');
        dumpTo(j.o[i].second, o);
      }
      o.push_back('}');
      break;
    }
  }
}

std::string Json::dump() const {
  std::string o;
  dumpTo(*this, o);
  return o;
}

namespace {
struct Parser {
  std::string_view s;
  size_t i = 0;
  std::string err;
  int depth = 0;

  void ws() {
    while (i < s.size() && (s[i] == ' ' || s[i] == '\t' || s[i] == '\n' || s[i] == '\r')) ++i;
  }
  bool fail(const std::string& m) {
    if (err.empty()) err = m + " at offset " + std::to_string(i);
    return false;
  }
  bool hex4(uint32_t& v) {
    if (i + 4 > s.size()) return fail("bad \\u escape");
    v = 0;
    for (int k = 0; k < 4; ++k) {
      char c = s[i++];
      v <<= 4;
      if (c >= '0' && c <= '9') v |= (uint32_t)(c - '0');
      else if (c >= 'a' && c <= 'f') v |= (uint32_t)(c - 'a' + 10);
      else if (c >= 'A' && c <= 'F') v |= (uint32_t)(c - 'A' + 10);
      else return fail("bad \\u escape");
    }
    return true;
  }
  bool string(std::string& out) {
    ++i;  // opening quote
    while (i < s.size()) {
      char c = s[i++];
      if (c == '"') return true;
      if (c != '\\') {
        out.push_back(c);
        continue;
      }
      if (i >= s.size()) break;
      char e = s[i++];
      switch (e) {
        case '"': out.push_back('"'); break;
        case '\\': out.push_back('\\'); break;
        case '/': out.push_back('/'); break;
        case 'b': out.push_back('\b'); break;
        case 'f': out.push_back('\f'); break;
        case 'n': out.push_back('\n'); break;
        case 'r': out.push_back('\r'); break;
        case 't': out.push_back('\t'); break;
        case 'u': {
          uint32_t cp;
          if (!hex4(cp)) return false;
          if (cp >= 0xD800 && cp < 0xDC00 && i + 1 < s.size() && s[i] == '\\' && s[i + 1] == 'u') {
            i += 2;
            uint32_t lo;
            if (!hex4(lo)) return false;
            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
          }
          putUtf8(out, cp);
          break;
        }
        default: return fail("bad escape");
      }
    }
    return fail("unterminated string");
  }
  bool value(Json& j) {
    ws();
    if (i >= s.size()) return fail("unexpected end");
    if (++depth > 200) return fail("too deeply nested");
    bool ok = valueInner(j);
    --depth;
    return ok;
  }
  bool valueInner(Json& j) {
    char c = s[i];
    if (c == '{') {
      ++i;
      j = Json::object();
      ws();
      if (i < s.size() && s[i] == '}') return ++i, true;
      for (;;) {
        ws();
        if (i >= s.size() || s[i] != '"') return fail("expected a key");
        std::string k;
        if (!string(k)) return false;
        ws();
        if (i >= s.size() || s[i] != ':') return fail("expected ':'");
        ++i;
        Json v;
        if (!value(v)) return false;
        j.o.emplace_back(std::move(k), std::move(v));
        ws();
        if (i < s.size() && s[i] == ',') {
          ++i;
          continue;
        }
        if (i < s.size() && s[i] == '}') return ++i, true;
        return fail("expected ',' or '}'");
      }
    }
    if (c == '[') {
      ++i;
      j = Json::array();
      ws();
      if (i < s.size() && s[i] == ']') return ++i, true;
      for (;;) {
        Json v;
        if (!value(v)) return false;
        j.a.push_back(std::move(v));
        ws();
        if (i < s.size() && s[i] == ',') {
          ++i;
          continue;
        }
        if (i < s.size() && s[i] == ']') return ++i, true;
        return fail("expected ',' or ']'");
      }
    }
    if (c == '"') {
      j.t = Json::Str;
      return string(j.s);
    }
    if (s.compare(i, 4, "true") == 0) return i += 4, j = Json::boolean(true), true;
    if (s.compare(i, 5, "false") == 0) return i += 5, j = Json::boolean(false), true;
    if (s.compare(i, 4, "null") == 0) return i += 4, j = Json(), true;
    if (c == '-' || (c >= '0' && c <= '9')) {
      size_t st = i;
      if (s[i] == '-') ++i;
      while (i < s.size() && ((s[i] >= '0' && s[i] <= '9') || s[i] == '.' || s[i] == 'e' || s[i] == 'E' || s[i] == '+' || s[i] == '-')) ++i;
      std::string tmp(s.substr(st, i - st));
      char* end = nullptr;
      double v = std::strtod(tmp.c_str(), &end);
      if (*end) return fail("bad number");
      j = Json::num(v);
      j.isInt = tmp.find_first_of(".eE") == std::string::npos;
      return true;
    }
    return fail("unexpected character");
  }
};
}  // namespace

bool parseJson(std::string_view text, Json& out, std::string& err) {
  Parser p{text};
  if (text.size() >= 3 && (unsigned char)text[0] == 0xEF && (unsigned char)text[1] == 0xBB && (unsigned char)text[2] == 0xBF) p.i = 3;
  if (!p.value(out)) {
    err = p.err;
    return false;
  }
  p.ws();
  if (p.i != text.size()) {
    err = "trailing characters at offset " + std::to_string(p.i);
    return false;
  }
  return true;
}

}  // namespace acu
