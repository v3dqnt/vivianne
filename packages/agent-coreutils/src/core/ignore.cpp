#include "core/ignore.hpp"

#include "core/glob.hpp"

namespace acu {

void parseIgnoreFile(std::string_view content, std::vector<IgnoreRule>& out) {
  size_t pos = 0;
  while (pos <= content.size()) {
    size_t nl = content.find('\n', pos);
    std::string_view line = content.substr(pos, nl == std::string_view::npos ? std::string_view::npos : nl - pos);
    pos = nl == std::string_view::npos ? content.size() + 1 : nl + 1;
    if (!line.empty() && line.back() == '\r') line.remove_suffix(1);
    if (pos == 1 && line.size() >= 3 && (unsigned char)line[0] == 0xEF && (unsigned char)line[1] == 0xBB &&
        (unsigned char)line[2] == 0xBF)
      line.remove_prefix(3);
    // trailing spaces are dropped unless escaped
    while (!line.empty() && line.back() == ' ' && !(line.size() >= 2 && line[line.size() - 2] == '\\')) line.remove_suffix(1);
    if (line.empty() || line[0] == '#') continue;
    IgnoreRule r;
    if (line[0] == '!') {
      r.neg = true;
      line.remove_prefix(1);
    }
    if (line.size() >= 2 && line[0] == '\\' && (line[1] == '#' || line[1] == '!')) line.remove_prefix(1);
    if (!line.empty() && line.back() == '/') {
      r.dirOnly = true;
      line.remove_suffix(1);
    }
    if (line.empty()) continue;
    if (line.find('/') != std::string_view::npos) {
      r.anchored = true;
      if (line[0] == '/') line.remove_prefix(1);
    }
    r.pat = std::string(line);
    r.wild = r.pat.find_first_of("*?[\\") != std::string::npos;
    out.push_back(std::move(r));
  }
}

static bool ruleMatches(const IgnoreRule& r, std::string_view rel, bool isDir) {
  if (r.dirOnly && !isDir) return false;
  std::string_view target = rel;
  if (!r.anchored) {
    size_t s = rel.rfind('/');
    if (s != std::string_view::npos) target = rel.substr(s + 1);
  }
  if (!r.wild) return target == r.pat;
  return globMatch(r.pat, target);
}

int ignoreDecision(const IgnoreSet* set, std::string_view repoRel, bool isDir) {
  for (const IgnoreSet* s = set; s; s = s->parent.get()) {
    if (s->baseLen >= repoRel.size()) continue;
    std::string_view rel = repoRel.substr(s->baseLen);
    for (size_t i = s->rules.size(); i-- > 0;)
      if (ruleMatches(s->rules[i], rel, isDir)) return s->rules[i].neg ? -1 : 1;
  }
  return 0;
}

}  // namespace acu
