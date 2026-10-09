#pragma once
// A small regular-expression engine for line search.
//
// Matching is a backtracking walk with a visited-state bitmap (as in RE2's BitState), so it is linear in
// (program size x line length) and never explodes on patterns like (a*)*b. Back-references and look-around are
// not supported. `\w`, `\d`, `\s`, `\b` and case folding are ASCII-only; `.` and classes understand UTF-8.
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace acu {

class Regex {
 public:
  // wordMode: the match must not touch word characters on either side (grep -w).
  static bool compile(const std::string& pattern, bool icase, bool wordMode, Regex& out, std::string& err);
  static std::string escape(std::string_view literal);

  // True if the regex matches anywhere in `line` (which must not contain '\n'; strip a trailing '\r' first).
  bool search(std::string_view line) const;

  // Prefilter. When non-empty, every matching line contains at least one of these strings
  // (compare case-insensitively on ASCII when litIcase). If pureLiterals, containing one is also sufficient.
  std::vector<std::string> literals;
  bool litIcase = false;
  bool pureLiterals = false;

  static unsigned long long skippedLines();  // lines too long to search; reported by grep

  struct Impl;

 private:
  std::shared_ptr<const Impl> impl_;
};

}  // namespace acu
