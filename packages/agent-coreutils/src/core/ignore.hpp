#pragma once
// .gitignore / .ignore / .acuignore rules, compiled once per directory and chained to their parents.
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace acu {

struct IgnoreRule {
  std::string pat;
  bool neg = false, dirOnly = false, anchored = false, wild = false;
};

struct IgnoreSet {
  std::shared_ptr<const IgnoreSet> parent;
  std::vector<IgnoreRule> rules;
  size_t baseLen = 0;  // length of the "dir/" prefix to cut off a repo-relative path
};
using IgnorePtr = std::shared_ptr<const IgnoreSet>;

void parseIgnoreFile(std::string_view content, std::vector<IgnoreRule>& out);

// 1 = ignored, -1 = explicitly re-included, 0 = no rule matched.
int ignoreDecision(const IgnoreSet* set, std::string_view repoRel, bool isDir);

}  // namespace acu
