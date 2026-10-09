#pragma once
// The line-search engine shared by grep and refs.
#include <functional>
#include <string>
#include <string_view>
#include <vector>

#include "core/regex.hpp"

namespace acu {

// Compiles one or more patterns (plain regex or fixed strings) into a single matcher.
bool buildMatcher(const std::vector<std::string>& patterns, bool fixed, bool icase, bool word, Regex& re,
                  std::string& err);

// Calls hit(lineStart, lineEnd) for each matching line of `buf`, in order, starting at byte `start`
// (lineEnd excludes the '\n'). Stops early if hit returns false.
void searchBuffer(const Regex& re, std::string_view buf, size_t start, const std::function<bool(size_t, size_t)>& hit);

}  // namespace acu
