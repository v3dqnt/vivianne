#pragma once
#include <string>
#include <string_view>
#include <vector>

namespace acu {

// Path-aware glob: `*` and `?` stop at '/', `**` crosses it (and `**/` may match zero directories),
// `[a-z]`/`[!a-z]` classes, `\x` escapes.
bool globMatch(std::string_view pattern, std::string_view text);

// `src/{a,b}/*.rs` -> two patterns. Capped at 256 results.
std::vector<std::string> expandBraces(const std::string& pattern);

bool hasGlobChars(std::string_view s);

}  // namespace acu
