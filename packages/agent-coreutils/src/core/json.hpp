#pragma once
// A small JSON value, parser and writer. Enough for --json output, edits files and the MCP server.
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace acu {

struct Json {
  enum Type { Null, Bool, Num, Str, Arr, Obj } t = Null;
  bool b = false;
  double n = 0;
  bool isInt = false;
  std::string s;
  std::vector<Json> a;
  std::vector<std::pair<std::string, Json>> o;

  static Json str(std::string v) {
    Json j;
    j.t = Str;
    j.s = std::move(v);
    return j;
  }
  static Json num(double v) {
    Json j;
    j.t = Num;
    j.n = v;
    j.isInt = v == (double)(long long)v;
    return j;
  }
  static Json boolean(bool v) {
    Json j;
    j.t = Bool;
    j.b = v;
    return j;
  }
  static Json array() {
    Json j;
    j.t = Arr;
    return j;
  }
  static Json object() {
    Json j;
    j.t = Obj;
    return j;
  }
  Json& set(const std::string& k, Json v) {
    o.emplace_back(k, std::move(v));
    return *this;
  }
  Json& push(Json v) {
    a.push_back(std::move(v));
    return *this;
  }
  const Json* get(const std::string& k) const {
    for (auto& kv : o)
      if (kv.first == k) return &kv.second;
    return nullptr;
  }
  std::string dump() const;
};

bool parseJson(std::string_view text, Json& out, std::string& err);
std::string jsonQuote(std::string_view s);  // quoted; invalid UTF-8 becomes U+FFFD

}  // namespace acu
