#pragma once
// Language detection and a line-based symbol outline.
//
// This is a heuristic outline (keywords and indentation), not a parse: it is fast, needs no grammars and is right
// for conventionally formatted code. Tree-sitter grammars can replace extractSymbols per language later.
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace acu {

enum class Family { None, TsJs, Python, Rust, Go, Jvm, CSharp, CLike, Ruby, Markdown, Generic };

struct LangInfo {
  const char* name = "";  // display name; "" if unknown
  Family family = Family::None;
};

LangInfo detectLanguage(std::string_view path);

// True for extensions that are always binary (images, fonts, archives, executables, media). Lets searches skip
// reading those files at all.
bool isBinaryExtension(std::string_view path);

// Extension lists for `--type` (e.g. "ts" -> ts,tsx,mts,cts). Unknown names are treated as an extension.
std::vector<std::string> typeExtensions(const std::string& type);

struct Symbol {
  uint32_t line = 0;   // 1-based
  uint16_t depth = 0;  // 0 = top level
  std::string kind;    // function, class, method, ...
  std::string name;
};

void extractSymbols(Family f, std::string_view text, std::vector<Symbol>& out);

}  // namespace acu
