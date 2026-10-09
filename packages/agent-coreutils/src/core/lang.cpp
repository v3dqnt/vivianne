#include "core/lang.hpp"

#include <algorithm>
#include <cctype>
#include <cstring>
#include <initializer_list>
#include <unordered_map>

#include "core/common.hpp"
#include "core/text.hpp"

namespace acu {

// ---------------------------------------------------------------- languages --

namespace {
struct ExtEntry {
  const char* ext;
  const char* name;
  Family fam;
};
const ExtEntry kExts[] = {
    {"ts", "TypeScript", Family::TsJs},  {"tsx", "TypeScript", Family::TsJs}, {"mts", "TypeScript", Family::TsJs},
    {"cts", "TypeScript", Family::TsJs}, {"js", "JavaScript", Family::TsJs},  {"jsx", "JavaScript", Family::TsJs},
    {"mjs", "JavaScript", Family::TsJs}, {"cjs", "JavaScript", Family::TsJs}, {"py", "Python", Family::Python},
    {"pyi", "Python", Family::Python},   {"rs", "Rust", Family::Rust},        {"go", "Go", Family::Go},
    {"java", "Java", Family::Jvm},       {"kt", "Kotlin", Family::Jvm},       {"kts", "Kotlin", Family::Jvm},
    {"scala", "Scala", Family::Jvm},     {"cs", "C#", Family::CSharp},        {"c", "C", Family::CLike},
    {"h", "C/C++ header", Family::CLike},{"cc", "C++", Family::CLike},        {"cpp", "C++", Family::CLike},
    {"cxx", "C++", Family::CLike},       {"hpp", "C++", Family::CLike},       {"hh", "C++", Family::CLike},
    {"hxx", "C++", Family::CLike},       {"m", "Objective-C", Family::CLike}, {"rb", "Ruby", Family::Ruby},
    {"md", "Markdown", Family::Markdown},{"markdown", "Markdown", Family::Markdown},
    {"php", "PHP", Family::Generic},     {"swift", "Swift", Family::Generic}, {"lua", "Lua", Family::Generic},
    {"sh", "Shell", Family::Generic},    {"bash", "Shell", Family::Generic},  {"ps1", "PowerShell", Family::Generic},
    {"sql", "SQL", Family::None},        {"css", "CSS", Family::None},        {"scss", "CSS", Family::None},
    {"html", "HTML", Family::None},      {"htm", "HTML", Family::None},       {"json", "JSON", Family::None},
    {"toml", "TOML", Family::None},      {"yaml", "YAML", Family::None},      {"yml", "YAML", Family::None},
    {"xml", "XML", Family::None},        {"vue", "Vue", Family::TsJs},        {"svelte", "Svelte", Family::TsJs},
    {"txt", "Text", Family::None},       {"cmake", "CMake", Family::None},    {"gradle", "Gradle", Family::None},
};
}  // namespace

LangInfo detectLanguage(std::string_view path) {
  size_t slash = path.find_last_of("/\\");
  std::string_view base = slash == std::string_view::npos ? path : path.substr(slash + 1);
  size_t dot = base.rfind('.');
  if (dot == std::string_view::npos || dot + 1 >= base.size()) {
    if (base == "CMakeLists.txt") return {"CMake", Family::None};
    if (base == "Makefile" || base == "makefile") return {"Make", Family::None};
    if (base == "Dockerfile") return {"Dockerfile", Family::None};
    return {};
  }
  std::string ext = lower(base.substr(dot + 1));
  for (auto& e : kExts)
    if (ext == e.ext) return {e.name, e.fam};
  return {};
}

bool isBinaryExtension(std::string_view path) {
  static const std::unordered_map<std::string, int> bin = [] {
    std::unordered_map<std::string, int> m;
    for (const char* e : {"png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "avif", "tif", "tiff", "psd", "woff", "woff2",
                          "ttf", "otf", "eot", "zip", "gz", "tgz", "bz2", "xz", "7z", "rar", "tar", "jar", "war", "class",
                          "exe", "dll", "so", "dylib", "o", "obj", "a", "lib", "pdf", "mp3", "mp4", "mov", "avi", "mkv",
                          "wav", "flac", "ogg", "webm", "wasm", "pyc", "pdb", "node", "sqlite", "db"})
      m[e] = 1;
    return m;
  }();
  size_t slash = path.find_last_of("/\\");
  std::string_view base = slash == std::string_view::npos ? path : path.substr(slash + 1);
  size_t dot = base.rfind('.');
  if (dot == std::string_view::npos || dot + 1 >= base.size()) return false;
  return bin.count(lower(base.substr(dot + 1))) != 0;
}

std::vector<std::string> typeExtensions(const std::string& t) {
  static const std::unordered_map<std::string, std::vector<std::string>> m = {
      {"ts", {"ts", "tsx", "mts", "cts"}},
      {"js", {"js", "jsx", "mjs", "cjs"}},
      {"py", {"py", "pyi"}},
      {"python", {"py", "pyi"}},
      {"rs", {"rs"}},
      {"rust", {"rs"}},
      {"go", {"go"}},
      {"java", {"java"}},
      {"kt", {"kt", "kts"}},
      {"c", {"c", "h"}},
      {"cpp", {"cc", "cpp", "cxx", "hpp", "hh", "hxx", "h"}},
      {"cs", {"cs"}},
      {"rb", {"rb"}},
      {"md", {"md", "markdown"}},
      {"json", {"json"}},
      {"css", {"css", "scss"}},
      {"html", {"html", "htm"}},
      {"yaml", {"yaml", "yml"}},
      {"toml", {"toml"}},
      {"sh", {"sh", "bash"}},
      {"web", {"ts", "tsx", "js", "jsx", "css", "html", "vue", "svelte"}},
  };
  auto it = m.find(lower(t));
  if (it != m.end()) return it->second;
  return {lower(t)};
}

// ------------------------------------------------------------------ helpers --

namespace {

using SV = std::string_view;

bool idStart(char c) { return std::isalpha((unsigned char)c) || c == '_' || c == '$' || (unsigned char)c >= 0x80; }
bool idChar(char c) { return idStart(c) || std::isdigit((unsigned char)c); }

void skipSpaces(SV& s) {
  while (!s.empty() && (s[0] == ' ' || s[0] == '\t')) s.remove_prefix(1);
}

SV takeId(SV& s) {
  skipSpaces(s);
  size_t n = 0;
  if (!s.empty() && idStart(s[0]))
    while (n < s.size() && idChar(s[n])) ++n;
  SV r = s.substr(0, n);
  s.remove_prefix(n);
  return r;
}

// Like takeId but allows `A::B`, `A.B` and a leading `~` or `operator...` (for C++, Go receivers, Ruby self.x).
SV takeQualified(SV& s) {
  skipSpaces(s);
  size_t n = 0;
  if (!s.empty() && s[0] == '~') ++n;
  while (n < s.size()) {
    if (idChar(s[n])) ++n;
    else if (s[n] == ':' && n + 1 < s.size() && s[n + 1] == ':' && n + 2 < s.size() && idStart(s[n + 2])) n += 2;
    else if (s[n] == '.' && n + 1 < s.size() && idStart(s[n + 1])) ++n;
    else break;
  }
  SV r = s.substr(0, n);
  s.remove_prefix(n);
  return r;
}

bool eatWord(SV& s, SV w) {
  if (s.size() < w.size() || s.compare(0, w.size(), w) != 0) return false;
  if (s.size() > w.size() && idChar(s[w.size()]) && idChar(w.back())) return false;
  s.remove_prefix(w.size());
  skipSpaces(s);
  return true;
}

bool peekWord(SV s, SV w) { return eatWord(s, w); }

bool anyWord(SV& s, std::initializer_list<const char*> ws) {
  for (const char* w : ws)
    if (eatWord(s, w)) return true;
  return false;
}

void stripModifiers(SV& s, std::initializer_list<const char*> mods) {
  while (anyWord(s, mods)) {
  }
}

bool isKeyword(SV w) {
  static const char* kw[] = {"if", "for", "while", "switch", "catch", "return", "else", "do", "try", "new", "throw",
                             "using", "lock", "foreach", "synchronized", "function", "typeof", "await", "yield",
                             "case", "sizeof", "delete", "defer", "go", "select", "super", "this", "elif", "with",
                             "assert", "except", "print", "when", "in", "is", "not", "and", "or"};
  for (const char* k : kw)
    if (w == k) return true;
  return false;
}

struct Parsed {
  std::string kind, name;
  bool container = false;
};

// Skips a balanced <...> at the start of s (generics). No-op if s doesn't start with '<'.
void skipGenerics(SV& s) {
  if (s.empty() || s[0] != '<') return;
  int depth = 0;
  size_t i = 0;
  for (; i < s.size(); ++i) {
    if (s[i] == '<') ++depth;
    else if (s[i] == '>' && --depth == 0) {
      ++i;
      break;
    }
  }
  s.remove_prefix(std::min(i, s.size()));
  skipSpaces(s);
}

// ---- per-family line parsers: return true and fill `p` if the (indent-stripped) line declares something.

bool parseTs(SV s, bool nested, Parsed& p) {
  stripModifiers(s, {"export", "default", "declare", "abstract", "public", "private", "protected", "static", "readonly",
                     "override", "async"});
  if (eatWord(s, "function")) {
    if (!s.empty() && s[0] == '*') {
      s.remove_prefix(1);
      skipSpaces(s);
    }
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {"function", std::string(id), false};
    return true;
  }
  if (eatWord(s, "class")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {"class", std::string(id), true};
    return true;
  }
  if (eatWord(s, "interface")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {"interface", std::string(id), true};
    return true;
  }
  if (peekWord(s, "const") || peekWord(s, "enum")) {
    SV t = s;
    if (eatWord(t, "const") && eatWord(t, "enum")) {
      SV id = takeId(t);
      if (id.empty()) return false;
      p = {"enum", std::string(id), true};
      return true;
    }
  }
  if (eatWord(s, "enum")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {"enum", std::string(id), true};
    return true;
  }
  if (!nested && peekWord(s, "type")) {
    SV t = s;
    eatWord(t, "type");
    SV id = takeId(t);
    skipSpaces(t);
    if (!id.empty() && !t.empty() && (t[0] == '=' || t[0] == '<')) {
      p = {"type", std::string(id), false};
      return true;
    }
    return false;
  }
  if (eatWord(s, "namespace") || eatWord(s, "module")) {
    SV id = takeId(s);
    if (id.empty() && !s.empty() && (s[0] == '"' || s[0] == '\'')) {
      size_t e = s.find(s[0], 1);
      if (e != SV::npos) id = s.substr(1, e - 1);
    }
    if (id.empty()) return false;
    p = {"namespace", std::string(id), true};
    return true;
  }
  if (!nested) {
    if (anyWord(s, {"const", "let", "var"})) {
      SV id = takeId(s);
      if (id.empty()) return false;
      bool fn = s.find("=>") != SV::npos || s.find("function") != SV::npos;
      p = {fn ? "function" : "const", std::string(id), false};
      return true;
    }
    return false;
  }
  // class / interface body member
  auto member = [&](SV t) -> bool {
    SV id = takeId(t);
    if (id.empty() || isKeyword(id)) return false;
    skipSpaces(t);
    if (!t.empty() && t[0] == '?') t.remove_prefix(1);
    skipGenerics(t);
    skipSpaces(t);
    if (!t.empty() && t[0] == '(') {
      p = {"method", std::string(id), false};
      return true;
    }
    if (!t.empty() && (t[0] == '=' || t[0] == ':') && t.find("=>") != SV::npos) {
      p = {"method", std::string(id), false};
      return true;
    }
    return false;
  };
  SV t = s;
  if ((eatWord(t, "get") || eatWord(t, "set")) && member(t)) return true;
  return member(s);
}

bool parsePython(SV s, bool nested, Parsed& p) {
  (void)nested;
  eatWord(s, "async");
  if (eatWord(s, "def")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {nested ? "method" : "function", std::string(id), false};
    return true;
  }
  if (eatWord(s, "class")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {"class", std::string(id), true};
    return true;
  }
  return false;
}

bool parseRust(SV s, bool nested, Parsed& p) {
  if (eatWord(s, "pub")) {
    if (!s.empty() && s[0] == '(') {
      size_t e = s.find(')');
      if (e == SV::npos) return false;
      s.remove_prefix(e + 1);
      skipSpaces(s);
    }
  }
  for (;;) {  // `const` is a modifier only in front of fn, e.g. `pub const unsafe fn`; `const X: T` is a constant
    SV t = s;
    if (eatWord(t, "const") && (peekWord(t, "fn") || peekWord(t, "unsafe") || peekWord(t, "async") || peekWord(t, "extern"))) {
      s = t;
      continue;
    }
    if (anyWord(s, {"unsafe", "async", "default"})) continue;
    break;
  }
  if (eatWord(s, "extern")) {
    if (!s.empty() && s[0] == '"') {
      size_t e = s.find('"', 1);
      if (e != SV::npos) {
        s.remove_prefix(e + 1);
        skipSpaces(s);
      }
    }
  }
  if (eatWord(s, "fn")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {nested ? "method" : "fn", std::string(id), false};
    return true;
  }
  struct K {
    const char* kw;
    const char* kind;
    bool container;
  };
  static const K ks[] = {{"struct", "struct", false}, {"enum", "enum", false},   {"trait", "trait", true},
                         {"mod", "mod", true},        {"type", "type", false},   {"union", "union", false},
                         {"static", "static", false}, {"const", "const", false}};
  if (!nested || true) {
    for (auto& k : ks) {
      SV t = s;
      if (eatWord(t, k.kw)) {
        SV id = takeId(t);
        if (id.empty()) return false;
        p = {k.kind, std::string(id), k.container};
        return true;
      }
    }
  }
  if (eatWord(s, "impl")) {
    size_t e = s.find('{');
    SV head = s.substr(0, e == SV::npos ? s.size() : e);
    size_t w = head.find(" where ");
    if (w != SV::npos) head = head.substr(0, w);
    std::string h = trim(head);
    if (h.size() > 80) h.resize(80);
    p = {"impl", h, true};
    return true;
  }
  if (eatWord(s, "macro_rules!")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {"macro", std::string(id), false};
    return true;
  }
  return false;
}

bool parseGo(SV s, bool, Parsed& p) {
  if (eatWord(s, "func")) {
    std::string recv;
    if (!s.empty() && s[0] == '(') {
      size_t e = s.find(')');
      if (e == SV::npos) return false;
      SV r = s.substr(1, e - 1);
      s.remove_prefix(e + 1);
      // "r *Type" or "Type" -> Type
      size_t sp = r.find_last_of(" *");
      recv = std::string(sp == SV::npos ? r : r.substr(sp + 1));
      skipSpaces(s);
    }
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {recv.empty() ? "func" : "method", recv.empty() ? std::string(id) : recv + "." + std::string(id), false};
    return true;
  }
  if (eatWord(s, "type")) {
    SV id = takeId(s);
    if (id.empty()) return false;
    std::string kind = "type";
    skipSpaces(s);
    if (peekWord(s, "struct")) kind = "struct";
    else if (peekWord(s, "interface")) kind = "interface";
    p = {kind, std::string(id), false};
    return true;
  }
  if (anyWord(s, {"const", "var"})) {
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {"const", std::string(id), false};
    return true;
  }
  return false;
}

bool parseJvm(SV s, bool nested, Parsed& p, const std::string& container) {
  if (s.empty() || s[0] == '@') return false;
  stripModifiers(s, {"public", "private", "protected", "internal", "static", "final", "abstract", "sealed", "partial",
                     "override", "virtual", "async", "unsafe", "readonly", "extern", "open", "data", "inline",
                     "suspend", "synchronized", "native", "default", "new", "const", "operator", "tailrec", "lateinit"});
  struct K {
    const char* kw;
    const char* kind;
  };
  static const K ks[] = {{"class", "class"},   {"interface", "interface"}, {"enum", "enum"},     {"record", "record"},
                         {"struct", "struct"}, {"object", "object"},       {"trait", "trait"},   {"namespace", "namespace"}};
  for (auto& k : ks) {
    SV t = s;
    if (eatWord(t, k.kw)) {
      SV id = takeQualified(t);
      if (id.empty()) return false;
      p = {k.kind, std::string(id), true};
      return true;
    }
  }
  if (eatWord(s, "fun")) {
    skipGenerics(s);
    SV id = takeQualified(s);
    if (id.empty()) return false;
    p = {nested ? "method" : "function", std::string(id), false};
    return true;
  }
  if (eatWord(s, "def")) {  // Scala
    SV id = takeId(s);
    if (id.empty()) return false;
    p = {nested ? "method" : "function", std::string(id), false};
    return true;
  }
  if (!nested) return false;
  // method: <type tokens> name ( ... ; constructors have just the name
  size_t paren = s.find('(');
  if (paren == SV::npos) return false;
  SV head = s.substr(0, paren);
  if (head.find('=') != SV::npos || head.find('.') != SV::npos) return false;
  SV t = head;
  std::vector<std::string> toks;
  while (true) {
    skipSpaces(t);
    if (t.empty()) break;
    size_t n = 0;
    int ang = 0;
    while (n < t.size() && (ang > 0 || (t[n] != ' ' && t[n] != '\t'))) {
      if (t[n] == '<') ++ang;
      else if (t[n] == '>') --ang;
      ++n;
    }
    toks.emplace_back(t.substr(0, n));
    t.remove_prefix(n);
  }
  if (toks.empty()) return false;
  std::string name = toks.back();
  if (name.empty() || !idStart(name[0]) || isKeyword(name)) return false;
  for (char c : name)
    if (!idChar(c)) return false;
  if (toks.size() == 1 && name != container) return false;  // a bare call like foo(x)
  if (isKeyword(toks[0])) return false;
  p = {"method", name, false};
  return true;
}

bool parseCLike(SV s, bool nested, Parsed& p) {
  (void)nested;
  if (s.empty() || s[0] == '#' || s[0] == '}' || s[0] == '{') return false;
  stripModifiers(s, {"typedef", "extern", "static", "inline", "constexpr", "virtual", "explicit", "friend", "export"});
  struct K {
    const char* kw;
    const char* kind;
    bool container;
  };
  static const K ks[] = {{"class", "class", true}, {"struct", "struct", true}, {"enum", "enum", false},
                         {"union", "union", false}, {"namespace", "namespace", true}};
  for (auto& k : ks) {
    SV t = s;
    if (eatWord(t, k.kw)) {
      eatWord(t, "class");  // enum class
      SV id = takeQualified(t);
      skipSpaces(t);
      // `struct foo x;` / `struct foo *f(` are not definitions; accept `{` or `:` or end of line
      if (id.empty() || (!t.empty() && t[0] != '{' && t[0] != ':' && t[0] != ';' && !peekWord(t, "final"))) return false;
      if (!t.empty() && t[0] == ';') return false;  // forward declaration
      p = {k.kind, std::string(id), k.container};
      return true;
    }
  }
  if (peekWord(s, "template") || peekWord(s, "using") || peekWord(s, "return") || peekWord(s, "typedef")) return false;
  size_t paren = s.find('(');
  if (paren == SV::npos) return false;
  SV head = s.substr(0, paren);
  if (head.find('=') != SV::npos || head.find('"') != SV::npos) return false;
  // tokens before '(': need a return type and a name, or a qualified name (ctors/dtors)
  std::vector<std::string> toks;
  SV t = head;
  while (true) {
    skipSpaces(t);
    if (t.empty()) break;
    size_t n = 0;
    int ang = 0;
    while (n < t.size() && (ang > 0 || (t[n] != ' ' && t[n] != '\t'))) {
      if (t[n] == '<') ++ang;
      else if (t[n] == '>') --ang;
      ++n;
    }
    toks.emplace_back(t.substr(0, n));
    t.remove_prefix(n);
  }
  if (toks.empty()) return false;
  std::string name = toks.back();
  while (!name.empty() && (name[0] == '*' || name[0] == '&')) name.erase(0, 1);
  if (toks.size() >= 2 && toks[toks.size() - 2] == "operator") name = "operator" + name;
  if (name.empty() || isKeyword(name)) return false;
  bool qualified = name.find("::") != std::string::npos;
  if (toks.size() == 1 && !qualified) return false;
  if (isKeyword(toks[0])) return false;
  if (name.rfind("operator", 0) != 0)
    for (char c : name)
      if (!idChar(c) && c != ':' && c != '~') return false;
  // declarations that end in ';' are kept only in headers' spirit: accept them, they still locate the symbol
  p = {"function", name, false};
  return true;
}

bool parseRuby(SV s, bool nested, Parsed& p) {
  if (eatWord(s, "def")) {
    SV id = takeQualified(s);
    if (id.empty()) return false;
    p = {nested ? "method" : "function", std::string(id), false};
    return true;
  }
  if (eatWord(s, "class")) {
    SV id = takeQualified(s);
    if (id.empty()) return false;
    p = {"class", std::string(id), true};
    return true;
  }
  if (eatWord(s, "module")) {
    SV id = takeQualified(s);
    if (id.empty()) return false;
    p = {"module", std::string(id), true};
    return true;
  }
  return false;
}

bool parseGeneric(SV s, bool, Parsed& p) {
  stripModifiers(s, {"export", "public", "private", "protected", "static", "final", "abstract", "async", "local"});
  struct K {
    const char* kw;
    const char* kind;
  };
  static const K ks[] = {{"function", "function"}, {"def", "function"}, {"class", "class"},   {"func", "function"},
                         {"fn", "function"},       {"sub", "function"}, {"interface", "interface"},
                         {"struct", "struct"},     {"enum", "enum"},    {"trait", "trait"},   {"protocol", "protocol"},
                         {"extension", "extension"}, {"Function", "function"}};
  for (auto& k : ks) {
    SV t = s;
    if (eatWord(t, k.kw)) {
      SV id = takeQualified(t);
      if (id.empty()) return false;
      p = {k.kind, std::string(id), false};
      return true;
    }
  }
  return false;
}

}  // namespace

// --------------------------------------------------------------- extraction --

void extractSymbols(Family fam, std::string_view text, std::vector<Symbol>& out) {
  if (fam == Family::None) return;
  struct Cont {
    size_t indent;
    long childIndent;
    std::string name;
  };
  std::vector<Cont> stack;
  bool inBlockComment = false, inDoc = false;
  const bool cstyle = fam == Family::TsJs || fam == Family::Rust || fam == Family::Go || fam == Family::Jvm ||
                      fam == Family::CSharp || fam == Family::CLike || fam == Family::Generic;
  size_t pos = 0;
  uint32_t lineNo = 0;
  while (pos < text.size()) {
    size_t nl = text.find('\n', pos);
    SV line = text.substr(pos, nl == SV::npos ? SV::npos : nl - pos);
    pos = nl == SV::npos ? text.size() : nl + 1;
    ++lineNo;
    if (!line.empty() && line.back() == '\r') line.remove_suffix(1);
    size_t indent = 0;
    while (indent < line.size() && (line[indent] == ' ' || line[indent] == '\t')) ++indent;
    SV s = line.substr(indent);
    if (s.empty()) continue;

    if (fam == Family::Markdown) {
      size_t h = 0;
      while (h < s.size() && s[h] == '#') ++h;
      if (h > 0 && h <= 6 && h < s.size() && s[h] == ' ' && indent == 0) {
        Symbol sym;
        sym.line = lineNo;
        sym.depth = (uint16_t)(h - 1);
        sym.kind = "h" + std::to_string(h);
        sym.name = trim(s.substr(h + 1));
        out.push_back(std::move(sym));
      }
      continue;
    }
    if (fam == Family::Python) {
      size_t q = 0, cnt = 0;
      while ((q = s.find("\"\"\"", q)) != SV::npos) ++cnt, q += 3;
      size_t q2 = 0;
      while ((q2 = s.find("'''", q2)) != SV::npos) ++cnt, q2 += 3;
      if (inDoc) {
        if (cnt % 2 == 1) inDoc = false;
        continue;
      }
      if (cnt % 2 == 1) {
        inDoc = true;
        continue;
      }
      if (s[0] == '#') continue;
    } else if (cstyle || fam == Family::Ruby) {
      if (inBlockComment) {
        if (s.find("*/") != SV::npos) inBlockComment = false;
        continue;
      }
      if (fam != Family::Ruby) {
        if (s.rfind("/*", 0) == 0) {
          if (s.find("*/") == SV::npos) inBlockComment = true;
          continue;
        }
        if (s.rfind("//", 0) == 0 || s[0] == '*') continue;
      } else if (s[0] == '#') {
        continue;
      }
    }

    // leave containers this line is not inside
    while (!stack.empty() && stack.back().indent >= indent) stack.pop_back();
    bool nested = !stack.empty();
    if (nested) {
      long ci = stack.back().childIndent;
      if (ci >= 0 && (long)indent != ci) continue;  // deeper statements, not members
    } else if (indent != 0) {
      continue;
    }
    Parsed p;
    bool ok = false;
    switch (fam) {
      case Family::TsJs: ok = parseTs(s, nested, p); break;
      case Family::Python: ok = parsePython(s, nested, p); break;
      case Family::Rust: ok = parseRust(s, nested, p); break;
      case Family::Go: ok = parseGo(s, nested, p); break;
      case Family::Jvm:
      case Family::CSharp: ok = parseJvm(s, nested, p, nested ? stack.back().name : std::string()); break;
      case Family::CLike: ok = parseCLike(s, nested, p); break;
      case Family::Ruby: ok = parseRuby(s, nested, p); break;
      case Family::Generic: ok = parseGeneric(s, nested, p); break;
      default: break;
    }
    if (!ok) continue;
    if (nested && stack.back().childIndent < 0) stack.back().childIndent = (long)indent;
    Symbol sym;
    sym.line = lineNo;
    sym.depth = (uint16_t)stack.size();
    sym.kind = p.kind;
    sym.name = p.name;
    out.push_back(std::move(sym));
    if (p.container) stack.push_back({indent, -1, p.name});
  }
}

}  // namespace acu
