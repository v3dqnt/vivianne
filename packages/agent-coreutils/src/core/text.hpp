#pragma once
// Text-file helpers: binary detection, BOM / UTF-16 handling, line splitting, line endings.
#include <string>
#include <string_view>
#include <vector>

namespace acu {

struct TextInfo {
  bool binary = false;
  bool bom = false;    // UTF-8 BOM present
  bool utf16 = false;  // UTF-16 (LE or BE) with BOM
  bool utf16be = false;
  bool crlf = false;   // dominant line ending is CRLF
};

// NUL byte in the first 8 KB means binary, unless the file starts with a UTF-16 BOM.
bool looksBinary(std::string_view raw);

// UTF-8 text for display: BOM removed, UTF-16 converted. Sets `binary` and leaves `out` empty for binary files.
void toDisplayText(std::string_view raw, std::string& out, TextInfo& ti);

// Number of lines: newline count, plus one if the last line has no terminator.
size_t countLines(std::string_view text);

// Offsets of the start of every line, plus a sentinel at the end (text.size()).
std::vector<size_t> lineStarts(std::string_view text);

// The i-th line (0-based) without its terminator.
std::string_view lineAt(std::string_view text, const std::vector<size_t>& starts, size_t i);

bool dominantCrlf(std::string_view text);

std::string toCrlf(std::string_view lf);    // every \n (not already preceded by \r) becomes \r\n
std::string toLf(std::string_view s);       // \r\n -> \n

}  // namespace acu
