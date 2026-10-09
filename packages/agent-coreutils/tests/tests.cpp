// Self-contained tests: no framework, no network. Run: build/acu_tests (exit code 0 = all passed).
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <random>
#include <sstream>
#include <thread>

#include "cmd/cmd.hpp"
#include "core/common.hpp"
#include "core/glob.hpp"
#include "core/ignore.hpp"
#include "core/json.hpp"
#include "core/lang.hpp"
#include "core/platform.hpp"
#include "core/regex.hpp"
#include "core/text.hpp"
#include "mcp/mcp.hpp"

using namespace acu;

static int g_fail = 0, g_checks = 0;
static const char* g_section = "";

#define CHECK(cond)                                                                 \
  do {                                                                              \
    ++g_checks;                                                                     \
    if (!(cond)) {                                                                  \
      ++g_fail;                                                                     \
      std::printf("FAIL [%s] %s:%d  %s\n", g_section, __FILE__, __LINE__, #cond);   \
    }                                                                               \
  } while (0)

#define CHECK_EQ(a, b)                                                                              \
  do {                                                                                              \
    ++g_checks;                                                                                     \
    auto va = (a);                                                                                  \
    auto vb = (b);                                                                                  \
    if (!(va == vb)) {                                                                              \
      ++g_fail;                                                                                     \
      std::cout << "FAIL [" << g_section << "] " << __FILE__ << ":" << __LINE__ << "  " << #a << " == " << #b \
                << "\n   got:      " << va << "\n   expected: " << vb << "\n";                      \
    }                                                                                               \
  } while (0)

#define SECTION(name) g_section = name

// ------------------------------------------------------------------ helpers --

struct Result {
  int rc;
  std::string out, err;
  bool has(const std::string& s) const { return out.find(s) != std::string::npos; }
};

static Ctx g_ctx;

static Result run(const std::string& cmd, std::vector<std::string> args, const std::string& stdinText = "") {
  Io io;
  io.in = stdinText;
  io.haveIn = true;
  int rc = runCommand(g_ctx, cmd, args, io);
  return {rc, io.out, io.err};
}

static void put(const std::string& path, const std::string& content) {
  std::error_code ec;
  fs::create_directories(toPath(path).parent_path(), ec);
  std::ofstream f(toPath(path), std::ios::binary);
  f << content;
}

static std::string slurp(const std::string& path) {
  std::string s, e;
  readFile(path, s, e);
  return s;
}

static bool rx(const std::string& pat, const std::string& text, bool icase = false, bool word = false) {
  Regex re;
  std::string err;
  if (!Regex::compile(pat, icase, word, re, err)) {
    std::printf("regex compile failed for /%s/: %s\n", pat.c_str(), err.c_str());
    return false;
  }
  return re.search(text);
}

static bool rxFails(const std::string& pat) {
  Regex re;
  std::string err;
  return !Regex::compile(pat, false, false, re, err);
}

// ------------------------------------------------------------------- tests ---

static void testRegex() {
  SECTION("regex");
  CHECK(rx("abc", "xxabcxx"));
  CHECK(!rx("abc", "xxabxcxx"));
  CHECK(rx("a.c", "abc"));
  CHECK(!rx("a.c", "ac"));
  CHECK(rx("^abc", "abcdef"));
  CHECK(!rx("^abc", "xabc"));
  CHECK(rx("abc$", "xxabc"));
  CHECK(!rx("abc$", "abcx"));
  CHECK(rx("^$", ""));
  CHECK(!rx("^$", "a"));
  CHECK(rx("", "anything"));
  CHECK(rx("a*", ""));
  CHECK(rx("a+b", "caab"));
  CHECK(!rx("a+b", "cb"));
  CHECK(rx("colou?r", "color"));
  CHECK(rx("colou?r", "colour"));
  CHECK(!rx("colou?r", "colouur"));
  CHECK(rx("^a{3}$", "aaa"));
  CHECK(!rx("^a{3}$", "aa"));
  CHECK(!rx("^a{3}$", "aaaa"));
  CHECK(rx("^a{2,4}$", "aaa"));
  CHECK(!rx("^a{2,4}$", "a"));
  CHECK(rx("^a{2,}$", "aaaaaaaa"));
  CHECK(rx("a{,3}", "a{,3}"));  // not a quantifier: literal braces
  CHECK(rx("foo|bar", "xxbar"));
  CHECK(!rx("foo|bar", "xxbaz"));
  CHECK(rx("^(foo|bar)+$", "foobarfoo"));
  CHECK(rx("(?:ab)+c", "ababc"));
  CHECK(rx("(?<name>ab)c", "abc"));
  CHECK(rx("[abc]+", "xxbxx"));
  CHECK(!rx("^[abc]+$", "abd"));
  CHECK(rx("^[^abc]+$", "xyz"));
  CHECK(rx("^[a-c0-2]+$", "a1b2c0"));
  CHECK(rx("[]a]", "]"));
  CHECK(rx("[a\\]]", "]"));
  CHECK(rx("[a-]", "-"));
  CHECK(rx("^[[:alpha:]]+$", "abcXYZ"));
  CHECK(!rx("^[[:alpha:]]+$", "abc1"));
  CHECK(rx("^[[:digit:][:space:]]+$", "1 2 3"));
  CHECK(rx("\\d+", "abc123"));
  CHECK(!rx("\\d", "abc"));
  CHECK(rx("\\D", "abc"));
  CHECK(rx("\\w+", "..foo_1.."));
  CHECK(rx("\\W", "a b"));
  CHECK(rx("a\\sb", "a\tb"));
  CHECK(rx("a\\Sb", "axb"));
  CHECK(rx("\\bfoo\\b", "a foo b"));
  CHECK(!rx("\\bfoo\\b", "afoob"));
  CHECK(rx("\\Boo\\B", "afoob"));
  CHECK(rx("\\.", "a.b"));
  CHECK(!rx("\\.", "ab"));
  CHECK(rx("a\\tb", "a\tb"));
  CHECK(rx("\\x41", "A"));
  CHECK(rx("\\x{263A}", "\xE2\x98\xBA"));
  CHECK(rx("a.b", "a\xC3\xA9" "b"));      // . matches a whole UTF-8 character
  CHECK(!rx("a..b", "a\xC3\xA9" "b"));   // ... not two bytes
  CHECK(rx("[\xC3\xA9]", "caf\xC3\xA9"));  // non-ASCII class members
  CHECK(rx("^[^a]$", "\xC3\xA9"));
  CHECK(rx("(?i)hello", "HeLLo"));
  CHECK(rx("hello", "HELLO", true));
  CHECK(!rx("hello", "HELLO", false));
  CHECK(rx("[a-c]+", "ABC", true));
  CHECK(rx("a+?b", "aab"));  // lazy quantifiers match like greedy for yes/no
  CHECK(rx("(a|ab)(c|bcd)", "abcd"));
  // word mode (grep -w)
  CHECK(rx("type", "a type b", false, true));
  CHECK(!rx("type", "prototype", false, true));
  CHECK(!rx("type", "types", false, true));
  CHECK(rx("@foo", "x @foo y", false, true));
  CHECK(rx("foo", "bar-foo-baz", false, true));
  // errors
  CHECK(rxFails("(abc"));
  CHECK(rxFails("abc)"));
  CHECK(rxFails("[abc"));
  CHECK(rxFails("*a"));
  CHECK(rxFails("a**b") == false);  // a** is accepted as a repeat of a repeat
  CHECK(rxFails("(a)\\1"));         // back-reference
  CHECK(rxFails("(?=a)b"));         // look-ahead
  CHECK(rxFails("a{3,2}"));
  CHECK(rxFails("a{2000}"));
  CHECK(rxFails("\\p{L}"));
  CHECK(rxFails("[[:nope:]]"));
  CHECK(rxFails("abc\\"));

  // Pathological patterns must stay fast (no backtracking blow-up).
  auto t0 = std::chrono::steady_clock::now();
  CHECK(!rx("(a*)*b", std::string(5000, 'a')));
  CHECK(!rx("(a|aa)+$b", std::string(5000, 'a')));
  CHECK(!rx("^(a+)+$x", std::string(3000, 'a') + "!"));
  CHECK(rx("(x+x+)+y", std::string(2000, 'x') + "y"));
  double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
  CHECK(ms < 5000);

  // Prefilter literals
  Regex re;
  std::string err;
  CHECK(Regex::compile("foo", false, false, re, err));
  CHECK(re.pureLiterals && re.literals.size() == 1 && re.literals[0] == "foo");
  CHECK(Regex::compile("foo|bar|baz", false, false, re, err));
  CHECK(re.pureLiterals && re.literals.size() == 3);
  CHECK(Regex::compile("fo+bar\\d", false, false, re, err));
  CHECK(!re.pureLiterals && !re.literals.empty());
  CHECK(Regex::compile("hello", true, false, re, err));
  CHECK(re.litIcase && re.literals[0] == "hello");
  CHECK(Regex::compile("a.*", false, false, re, err));
  CHECK(Regex::compile("hello", false, true, re, err));
  CHECK(!re.pureLiterals);  // -w needs verification
  CHECK_EQ(Regex::escape("a.b*c"), std::string("a\\.b\\*c"));
}

static void testFuzzRegex() {
  SECTION("regex fuzz");
  std::mt19937 rng(12345);
  const std::string alphabet = "ab()[]{}|*+?.^$\\-,0123dws:<>=!";
  auto t0 = std::chrono::steady_clock::now();
  int compiled = 0;
  for (int i = 0; i < 20000; ++i) {
    std::string pat;
    int len = 1 + (int)(rng() % 14);
    for (int k = 0; k < len; ++k) pat.push_back(alphabet[rng() % alphabet.size()]);
    Regex re;
    std::string err;
    if (!Regex::compile(pat, (i & 1) != 0, (i & 2) != 0, re, err)) continue;
    ++compiled;
    for (const char* text : {"", "a", "ab", "abab", "a.b", "[ab]{2}", "aaaaaaaaaaaaaaaa", "\xC3\xA9x"}) re.search(text);
  }
  double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
  CHECK(compiled > 1000);
  CHECK(ms < 20000);
}

static void testGlob() {
  SECTION("glob");
  CHECK(globMatch("*.ts", "a.ts"));
  CHECK(!globMatch("*.ts", "dir/a.ts"));
  CHECK(globMatch("**/*.ts", "a.ts"));
  CHECK(globMatch("**/*.ts", "x/y/a.ts"));
  CHECK(!globMatch("**/*.ts", "x/y/a.tsx"));
  CHECK(globMatch("src/**/*.rs", "src/a.rs"));
  CHECK(globMatch("src/**/*.rs", "src/a/b/c.rs"));
  CHECK(!globMatch("src/**/*.rs", "lib/a.rs"));
  CHECK(globMatch("src/**", "src/a/b"));
  CHECK(globMatch("a?c", "abc"));
  CHECK(!globMatch("a?c", "a/c"));
  CHECK(globMatch("[a-c]x", "bx"));
  CHECK(!globMatch("[a-c]x", "dx"));
  CHECK(globMatch("[!a-c]x", "dx"));
  CHECK(globMatch("[^a-c]x", "dx"));
  CHECK(globMatch("\\*.txt", "*.txt"));
  CHECK(!globMatch("\\*.txt", "a.txt"));
  CHECK(globMatch("a*b*c", "aXXbYYc"));
  CHECK(!globMatch("a*b*c", "aXXbYY"));
  CHECK(globMatch("*", ""));
  CHECK(globMatch("", ""));
  auto e = expandBraces("src/{a,b}/*.{ts,tsx}");
  CHECK_EQ(e.size(), (size_t)4);
  CHECK(expandBraces("{a,b}").size() == 2);
  CHECK(expandBraces("x{a,{b,c}}").size() == 3);
  CHECK(expandBraces("{solo}").size() == 1);
  CHECK(expandBraces("plain").size() == 1);
  std::mt19937 rng(7);
  const std::string alpha = "ab/*?[]!{}\\.-,";
  for (int i = 0; i < 20000; ++i) {
    std::string p, t;
    for (int k = 0, n = (int)(rng() % 10); k < n; ++k) p.push_back(alpha[rng() % alpha.size()]);
    for (int k = 0, n = (int)(rng() % 10); k < n; ++k) t.push_back("ab/.-"[rng() % 5]);
    globMatch(p, t);
    expandBraces(p);
  }
  CHECK(true);  // reaching here without a crash or hang is the test
}

static void testIgnore() {
  SECTION("ignore");
  std::vector<IgnoreRule> rules;
  parseIgnoreFile("# comment\nnode_modules/\n*.log\n!keep.log\n/build\ndocs/*.tmp\n\\#hash\n  \nsrc/**/gen\n", rules);
  IgnoreSet set;
  set.rules = rules;
  CHECK_EQ(ignoreDecision(&set, "node_modules", true), 1);
  CHECK_EQ(ignoreDecision(&set, "node_modules", false), 0);   // dir-only rule
  CHECK_EQ(ignoreDecision(&set, "a/node_modules", true), 1);  // any depth
  CHECK_EQ(ignoreDecision(&set, "x.log", false), 1);
  CHECK_EQ(ignoreDecision(&set, "dir/x.log", false), 1);
  CHECK_EQ(ignoreDecision(&set, "keep.log", false), -1);  // re-included
  CHECK_EQ(ignoreDecision(&set, "build", true), 1);
  CHECK_EQ(ignoreDecision(&set, "sub/build", true), 0);  // anchored
  CHECK_EQ(ignoreDecision(&set, "docs/a.tmp", false), 1);
  CHECK_EQ(ignoreDecision(&set, "docs/x/a.tmp", false), 0);
  CHECK_EQ(ignoreDecision(&set, "#hash", false), 1);
  CHECK_EQ(ignoreDecision(&set, "src/a/b/gen", false), 1);
  CHECK_EQ(ignoreDecision(&set, "src/a.ts", false), 0);
  // nested set: a child .gitignore overrides its parent
  auto parent = std::make_shared<IgnoreSet>();
  parseIgnoreFile("*.tmp\n", parent->rules);
  IgnoreSet child;
  child.parent = parent;
  child.baseLen = 4;  // "sub/"
  parseIgnoreFile("!a.tmp\n", child.rules);
  CHECK_EQ(ignoreDecision(&child, "sub/a.tmp", false), -1);
  CHECK_EQ(ignoreDecision(&child, "sub/b.tmp", false), 1);
}

static void testJson() {
  SECTION("json");
  Json j;
  std::string err;
  CHECK(parseJson("{\"a\":[1,2.5,\"x\\n\\u00e9\\ud83d\\ude00\"],\"b\":true,\"c\":null,\"d\":{}}", j, err));
  CHECK(j.get("a") && j.get("a")->a.size() == 3);
  CHECK_EQ(j.get("a")->a[2].s, std::string("x\n\xC3\xA9\xF0\x9F\x98\x80"));
  CHECK(j.get("b")->b);
  CHECK(!parseJson("{\"a\":", j, err));
  CHECK(!parseJson("[1,]", j, err));
  CHECK(!parseJson("{} x", j, err));
  CHECK(!parseJson("\"abc", j, err));
  CHECK_EQ(jsonQuote("a\"b\\c\n\t\x01"), std::string("\"a\\\"b\\\\c\\n\\t\\u0001\""));
  CHECK_EQ(jsonQuote("ok\xFF" "x"), std::string("\"ok\xEF\xBF\xBD" "x\""));  // invalid UTF-8 is replaced
  Json o = Json::object();
  o.set("n", Json::num(3)).set("s", Json::str("t")).set("f", Json::num(1.5));
  CHECK_EQ(o.dump(), std::string("{\"n\":3,\"s\":\"t\",\"f\":1.5}"));
  Json deep;
  CHECK(!parseJson(std::string(1000, '[') + std::string(1000, ']'), deep, err));  // depth limit, no stack overflow
}

static void testText() {
  SECTION("text");
  CHECK_EQ(countLines(""), (size_t)0);
  CHECK_EQ(countLines("a"), (size_t)1);
  CHECK_EQ(countLines("a\n"), (size_t)1);
  CHECK_EQ(countLines("a\nb"), (size_t)2);
  CHECK_EQ(countLines("\n\n"), (size_t)2);
  CHECK(dominantCrlf("a\r\nb\r\nc\n"));
  CHECK(!dominantCrlf("a\nb\nc\r\n"));
  CHECK_EQ(toCrlf("a\nb\r\nc"), std::string("a\r\nb\r\nc"));
  CHECK_EQ(toLf("a\r\nb\nc"), std::string("a\nb\nc"));
  CHECK(looksBinary(std::string("ab\0cd", 5)));
  CHECK(!looksBinary("plain text"));
  TextInfo ti;
  std::string out;
  toDisplayText("\xEF\xBB\xBF" "hi\r\n", out, ti);
  CHECK(ti.bom && ti.crlf && out == "hi\r\n");
  std::string u16("\xFF\xFE" "h\0i\0\n\0", 8);
  TextInfo t2;
  toDisplayText(u16, out, t2);
  CHECK(t2.utf16 && out == "hi\n");
  CHECK_EQ(fmtInt(1234567), std::string("1,234,567"));
  CHECK_EQ(fmtInt(12), std::string("12"));
  CHECK_EQ(fmtTime(0), std::string("1970-01-01T00:00:00Z"));
  CHECK_EQ(fmtTime(1800000000), std::string("2027-01-15T08:00:00Z"));
  std::string t;
  appendTrunc(t, std::string(600, 'x'));
  CHECK(t.find("[+100 chars]") != std::string::npos && t.size() < 530);
  t.clear();
  appendTrunc(t, std::string(499, 'x') + "\xC3\xA9" + std::string(50, 'y'));  // cut must not split the 2-byte char
  CHECK(t.find("\xC3\xA9") == std::string::npos || t.find("\xC3\xA9") == 499);
}

static std::string outlineOf(const std::string& path, const std::string& src) {
  put(path, src);
  return run("outline", {path}).out;
}

static void testOutline() {
  SECTION("outline");
  std::string ts = outlineOf("o/a.ts",
                             "import x from 'y'\n"
                             "// function commented() {}\n"
                             "/* class InComment {} */\n"
                             "export default class App extends X {\n"
                             "  private state = 1\n"
                             "  constructor(a: string) {}\n"
                             "  async load(): Promise<void> {\n    if (x) { foo(1) }\n  }\n"
                             "  static get value() { return 1 }\n"
                             "}\n"
                             "export async function* gen() {}\n"
                             "export const f = async (a: number) => a\n"
                             "export const N = 5\n"
                             "interface I { a(): void }\n"
                             "export type T<A> = A | null\n"
                             "export enum E { A }\n"
                             "declare module 'm' {\n}\n");
  CHECK(ts.find("4:") != std::string::npos && ts.find("class App") != std::string::npos);
  CHECK(ts.find("method constructor") != std::string::npos);
  CHECK(ts.find("method load") != std::string::npos);
  CHECK(ts.find("method value") != std::string::npos);
  CHECK(ts.find("function gen") != std::string::npos);
  CHECK(ts.find("function f") != std::string::npos);
  CHECK(ts.find("const N") != std::string::npos);
  CHECK(ts.find("interface I") != std::string::npos);
  CHECK(ts.find("type T") != std::string::npos);
  CHECK(ts.find("enum E") != std::string::npos);
  CHECK(ts.find("namespace m") != std::string::npos);
  CHECK(ts.find("commented") == std::string::npos && ts.find("InComment") == std::string::npos);
  CHECK(ts.find(" foo") == std::string::npos);  // calls inside bodies are not symbols

  std::string py = outlineOf("o/p.py",
                             "def top(x):\n    def inner(): pass\n    return x\n\nclass K:\n    \"\"\"doc\n    def fake(self): pass\n    \"\"\"\n"
                             "    def m(self):\n        pass\n    @property\n    async def am(self): pass\n");
  CHECK(py.find("function top") != std::string::npos);
  CHECK(py.find("inner") == std::string::npos && py.find("fake") == std::string::npos);
  CHECK(py.find("class K") != std::string::npos && py.find("method m") != std::string::npos &&
        py.find("method am") != std::string::npos);

  std::string rs = outlineOf("o/a.rs",
                             "pub struct S { a: i32 }\nimpl S {\n    pub fn new() -> Self { S { a: 1 } }\n    fn helper(&self) {}\n}\n"
                             "pub(crate) async fn run() {}\ntrait T { fn f(&self); }\nenum E { A }\nmod m {\n    pub fn inner() {}\n}\nconst X: i32 = 1;\nmacro_rules! mm { () => {} }\n");
  CHECK(rs.find("struct S") != std::string::npos && rs.find("impl S") != std::string::npos);
  CHECK(rs.find("method new") != std::string::npos && rs.find("method helper") != std::string::npos);
  CHECK(rs.find("fn run") != std::string::npos && rs.find("trait T") != std::string::npos);
  CHECK(rs.find("mod m") != std::string::npos && rs.find("method inner") != std::string::npos);
  CHECK(rs.find("macro mm") != std::string::npos && rs.find("const X") != std::string::npos);

  std::string go = outlineOf("o/a.go", "package p\n\nfunc Top() {}\nfunc (s *Server) Handle(w int) {}\ntype Conf struct {\n a int\n}\ntype Iface interface {\n M()\n}\nconst K = 1\n");
  CHECK(go.find("func Top") != std::string::npos && go.find("method Server.Handle") != std::string::npos);
  CHECK(go.find("struct Conf") != std::string::npos && go.find("interface Iface") != std::string::npos);

  std::string java = outlineOf("o/A.java",
                               "package a;\npublic class A {\n    private int x;\n    public A() {}\n    public static void main(String[] args) {\n        foo(1);\n    }\n"
                               "    List<String> items(int n) {\n        return null;\n    }\n    interface Inner {\n        void run();\n    }\n}\n");
  CHECK(java.find("class A") != std::string::npos && java.find("method A") != std::string::npos);
  CHECK(java.find("method main") != std::string::npos && java.find("method items") != std::string::npos);
  CHECK(java.find("interface Inner") != std::string::npos && java.find("method run") != std::string::npos);
  CHECK(java.find("foo") == std::string::npos);

  std::string cpp = outlineOf("o/a.cpp",
                              "#include <x>\nnamespace ns {\nclass Foo {\n public:\n  void bar(int a);\n  int baz() const { return 1; }\n};\n}\n"
                              "int main(int argc, char** argv) {\n  return foo(1);\n}\nstatic void Foo::qux() {}\nstruct P { int a; };\n");
  CHECK(cpp.find("namespace ns") != std::string::npos && cpp.find("class Foo") != std::string::npos);
  CHECK(cpp.find("function bar") != std::string::npos && cpp.find("function baz") != std::string::npos);
  CHECK(cpp.find("function main") != std::string::npos && cpp.find("function Foo::qux") != std::string::npos);
  CHECK(cpp.find("struct P") != std::string::npos);
  CHECK(cpp.find("function foo") == std::string::npos);

  std::string rb = outlineOf("o/a.rb", "module M\n  class C\n    def go\n    end\n    def self.build\n    end\n  end\nend\n");
  CHECK(rb.find("module M") != std::string::npos && rb.find("class C") != std::string::npos);
  CHECK(rb.find("method go") != std::string::npos && rb.find("method self.build") != std::string::npos);

  std::string md = outlineOf("o/a.md", "# Title\ntext\n## Part\n### Sub\n```\n#not heading\n```\n");
  CHECK(md.find("h1 Title") != std::string::npos && md.find("h2 Part") != std::string::npos);

  put("o/none.xyz", "x\n");
  CHECK(run("outline", {"o/none.xyz"}).rc == 0);
  CHECK(run("outline", {"o/missing.ts"}).rc == kNotFound);
}

static void testCommands() {
  SECTION("read");
  std::string big;
  for (int i = 1; i <= 50; ++i) big += "line " + std::to_string(i) + "\n";
  put("t/big.txt", big);
  auto r = run("read", {"t/big.txt", "--lines", "3:5"});
  CHECK_EQ(r.out, std::string("3\tline 3\n4\tline 4\n5\tline 5\n"));
  r = run("read", {"t/big.txt", "--max", "10"});
  CHECK(r.has("10\tline 10\n") && !r.has("11\tline 11"));
  CHECK(r.has("40 more lines; use --lines 11:20"));
  r = run("read", {"t/big.txt", "--lines", "48:"});
  CHECK_EQ(r.out, std::string("48\tline 48\n49\tline 49\n50\tline 50\n"));
  r = run("read", {"t/big.txt", "--lines", ":2"});
  CHECK_EQ(r.out, std::string("1\tline 1\n2\tline 2\n"));
  r = run("read", {"t/big.txt", "--lines", "7"});
  CHECK_EQ(r.out, std::string("7\tline 7\n"));
  r = run("read", {"t/big.txt", "--lines", "99:100"});
  CHECK(r.rc == kNotFound && r.err.find("only 50 lines") != std::string::npos);
  r = run("read", {"t/big.txt", "--lines", "x"});
  CHECK(r.rc == kUsage);
  r = run("read", {"t/big.txt", "t/missing.txt"});
  CHECK(r.rc == kNotFound && r.has("== t/big.txt ==") && r.err.find("t/missing.txt: not found") != std::string::npos);
  r = run("read", {});
  CHECK(r.rc == kUsage);
  r = run("read", {"t"});
  CHECK(r.rc == kNotFound && r.err.find("is a directory") != std::string::npos);
  put("t/empty.txt", "");
  CHECK(run("read", {"t/empty.txt"}).has("(empty file)"));
  put("t/bin.dat", std::string("a\0b", 3));
  CHECK(run("read", {"t/bin.dat"}).err.find("binary") != std::string::npos);
  put("t/long.txt", std::string(900, 'z') + "\n");
  CHECK(run("read", {"t/long.txt"}).has("[+400 chars]"));
  put("t/crlf.txt", "a\r\nb\r\n");
  CHECK_EQ(run("read", {"t/crlf.txt"}).out, std::string("1\ta\n2\tb\n"));
  put("t/bom.txt", "\xEF\xBB\xBF" "x\n");
  CHECK_EQ(run("read", {"t/bom.txt"}).out, std::string("1\tx\n"));
  put("t/u16.txt", std::string("\xFF\xFE" "h\0i\0\n\0", 8));
  CHECK_EQ(run("read", {"t/u16.txt"}).out, std::string("1\thi\n"));
  CHECK_EQ(run("head", {"-n", "2", "t/big.txt"}).out, std::string("1\tline 1\n2\tline 2\n"));
  CHECK_EQ(run("tail", {"-n", "2", "t/big.txt"}).out, std::string("49\tline 49\n50\tline 50\n"));
  CHECK(run("tail", {"-n", "500", "t/big.txt"}).has("1\tline 1\n"));
  r = run("read", {"t/big.txt", "--lines", "1:2", "--json"});
  Json j;
  std::string e;
  CHECK(parseJson(r.out, j, e) && j.get("schema")->n == 1 && j.get("files")->a[0].get("content")->s == "line 1\nline 2\n");

  SECTION("stat");
  r = run("stat", {"t/big.txt", "t/crlf.txt", "t/bin.dat", "t", "t/nope"});
  CHECK(r.rc == kNotFound);
  CHECK(r.has("t/big.txt: 391 bytes, 50 lines, Text"));
  CHECK(r.has("CRLF") && r.has("binary") && r.has("t/: directory"));

  SECTION("write");
  r = run("write", {"w/new/f.txt"}, "hello\nworld\n");
  CHECK(r.rc == 0 && r.has("created w/new/f.txt (12 bytes, 2 lines)"));
  CHECK_EQ(slurp("w/new/f.txt"), std::string("hello\nworld\n"));
  put("w/crlf.txt", "a\r\nb\r\n");
  run("write", {"w/crlf.txt"}, "x\ny\nz\n");
  CHECK_EQ(slurp("w/crlf.txt"), std::string("x\r\ny\r\nz\r\n"));  // keeps CRLF
  run("write", {"w/crlf.txt", "--raw"}, "x\ny\n");
  CHECK_EQ(slurp("w/crlf.txt"), std::string("x\ny\n"));
  put("w/lf.txt", "a\nb\n");
  run("write", {"w/lf.txt"}, "x\r\ny\r\n");
  CHECK_EQ(slurp("w/lf.txt"), std::string("x\ny\n"));  // keeps LF
  put("w/bom.txt", "\xEF\xBB\xBF" "a\n");
  run("write", {"w/bom.txt"}, "b\n");
  CHECK_EQ(slurp("w/bom.txt"), std::string("\xEF\xBB\xBF" "b\n"));
  run("write", {"w/bom.txt", "--append"}, "c\n");
  CHECK_EQ(slurp("w/bom.txt"), std::string("\xEF\xBB\xBF" "b\nc\n"));
  CHECK(run("write", {"w"}, "x").rc == kUsage);  // a directory
  CHECK(run("write", {}, "x").rc == kUsage);
  put("w/u16.txt", std::string("\xFF\xFE" "a\0", 4));
  CHECK(run("write", {"w/u16.txt"}, "x").rc == kUsage);
  bool leftovers = false;
  for (auto& ent : fs::directory_iterator(toPath("w")))
    if (fromPath(ent.path()).find(".acu-tmp-") != std::string::npos) leftovers = true;
  CHECK(!leftovers);  // atomic write cleans up its temp files
  std::string huge(3 << 20, 'q');
  run("write", {"w/huge.txt"}, huge);
  CHECK(slurp("w/huge.txt") == huge);

  SECTION("edit");
  put("e/a.txt", "alpha\nbeta\ngamma\nbeta\n");
  r = run("edit", {"e/a.txt", "--old", "alpha", "--new", "ALPHA"});
  CHECK(r.rc == 0 && r.has("1 replacement (line 1)"));
  CHECK_EQ(slurp("e/a.txt"), std::string("ALPHA\nbeta\ngamma\nbeta\n"));
  std::string before = slurp("e/a.txt");
  r = run("edit", {"e/a.txt", "--old", "beta", "--new", "B"});
  CHECK(r.rc == kNotFound && r.err.find("found 2 times") != std::string::npos && r.err.find("lines 2, 4") != std::string::npos);
  CHECK(slurp("e/a.txt") == before);  // refused: file untouched
  r = run("edit", {"e/a.txt", "--old", "beta", "--new", "B", "--all"});
  CHECK(r.rc == 0 && r.has("2 replacements (lines 2, 4)"));
  r = run("edit", {"e/a.txt", "--old", "gamma", "--new", ""});
  CHECK(r.rc == 0 && slurp("e/a.txt") == "ALPHA\nB\n\nB\n");  // deletion with an empty --new
  before = slurp("e/a.txt");
  r = run("edit", {"e/a.txt", "--old", "ALPHX", "--new", "q"});
  CHECK(r.rc == kNotFound && r.err.find("not found") != std::string::npos && r.err.find("closest lines") != std::string::npos &&
        r.err.find("1: ALPHA") != std::string::npos);
  CHECK(slurp("e/a.txt") == before);
  put("e/ws.txt", "if (x) {\n\treturn 1;\n}\n");
  r = run("edit", {"e/ws.txt", "--old", "  return 1;", "--new", "x"});
  CHECK(r.rc == kNotFound && r.err.find("whitespace") != std::string::npos && r.err.find("line 2") != std::string::npos);
  CHECK(run("edit", {"e/a.txt", "--old", "", "--new", "x"}).rc == kNotFound);
  CHECK(run("edit", {"e/a.txt", "--old", "B", "--new", "B"}).rc == kNotFound);
  CHECK(run("edit", {"e/a.txt", "--old", "B"}).rc == kUsage);  // --new missing
  CHECK(run("edit", {"e/nope.txt", "--old", "a", "--new", "b"}).rc == kNotFound);
  put("e/crlf.txt", "one\r\ntwo\r\nthree\r\n");
  r = run("edit", {"e/crlf.txt", "--old", "one\ntwo", "--new", "1\n2\n2b"});
  CHECK(r.rc == 0 && slurp("e/crlf.txt") == "1\r\n2\r\n2b\r\nthree\r\n");  // LF in --old matches CRLF file
  put("e/bom.txt", "\xEF\xBB\xBF" "hello\n");
  run("edit", {"e/bom.txt", "--old", "hello", "--new", "bye"});
  CHECK(slurp("e/bom.txt") == "\xEF\xBB\xBF" "bye\n");
  put("e/m.txt", "a b c\n");
  r = run("edit", {"e/m.txt", "--edits-json", "[{\"old\":\"a\",\"new\":\"x\"},{\"old\":\"x b\",\"new\":\"y\"}]"});
  CHECK(r.rc == 0 && slurp("e/m.txt") == "y c\n");  // later edits see earlier ones
  r = run("edit", {"e/m.txt", "--edits-json", "[{\"old\":\"y\",\"new\":\"1\"},{\"old\":\"nope\",\"new\":\"2\"}]"});
  CHECK(r.rc == kNotFound && r.err.find("edit #2") != std::string::npos && slurp("e/m.txt") == "y c\n");  // all or nothing
  CHECK(run("edit", {"e/m.txt", "--edits-json", "{bad"}).rc == kUsage);
  r = run("edit", {"e/m.txt", "--old", "y", "--new", "z", "--dry-run"});
  CHECK(r.rc == 0 && r.has("would edit") && slurp("e/m.txt") == "y c\n");
  put("e/bin.dat", std::string("a\0b", 3));
  CHECK(run("edit", {"e/bin.dat", "--old", "a", "--new", "b"}).rc == kNotFound);

  SECTION("patch");
  put("p/f.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
  std::string diff =
      "--- a/p/f.txt\n+++ b/p/f.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n@@ -5,2 +5,3 @@\n five\n six\n+seven\n";
  r = run("patch", {}, diff);
  CHECK(r.rc == 0 && r.has("patched p/f.txt (2 hunks)"));
  CHECK_EQ(slurp("p/f.txt"), std::string("one\nTWO\nthree\nfour\nfive\nsix\nseven\n"));
  r = run("patch", {}, diff);  // already applied: context no longer matches
  CHECK(r.rc == kNotFound && r.err.find("rejected") != std::string::npos);
  put("p/g.txt", "x\ny\nz\n");
  std::string drift = "--- a/p/g.txt\n+++ b/p/g.txt\n@@ -2,2 +2,2 @@\n-y\n+Y\n z\n";
  put("p/g2.txt", "pre\npre\nx\ny\nz\n");
  CHECK(run("patch", {}, "--- a/p/g2.txt\n+++ b/p/g2.txt\n@@ -2,2 +2,2 @@\n-y\n+Y\n z\n").rc == kNotFound);  // drifted by 2: no fuzz
  CHECK(run("patch", {"--fuzz", "3"}, "--- a/p/g2.txt\n+++ b/p/g2.txt\n@@ -2,2 +2,2 @@\n-y\n+Y\n z\n").rc == 0);
  CHECK_EQ(slurp("p/g2.txt"), std::string("pre\npre\nx\nY\nz\n"));
  CHECK(run("patch", {}, drift).rc == 0 && slurp("p/g.txt") == "x\nY\nz\n");
  r = run("patch", {}, "--- /dev/null\n+++ b/p/new.txt\n@@ -0,0 +1,2 @@\n+n1\n+n2\n");
  CHECK(r.rc == 0 && r.has("created p/new.txt") && slurp("p/new.txt") == "n1\nn2\n");
  r = run("patch", {}, "--- a/p/new.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-n1\n-n2\n");
  CHECK(r.rc == 0 && r.has("deleted p/new.txt") && !fs::exists(toPath("p/new.txt")));
  put("p/crlf.txt", "a\r\nb\r\n");
  run("patch", {}, "--- a/p/crlf.txt\n+++ b/p/crlf.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n");
  CHECK(slurp("p/crlf.txt") == "a\r\nB\r\n");
  CHECK(run("patch", {}, "not a diff").rc == kUsage);
  put("p/two.txt", "1\n2\n3\n");
  r = run("patch", {}, "--- a/p/two.txt\n+++ b/p/two.txt\n@@ -1,1 +1,1 @@\n-1\n+ONE\n@@ -3,1 +3,1 @@\n-nope\n+THREE\n");
  CHECK(r.rc == kNotFound && slurp("p/two.txt") == "1\n2\n3\n");  // a rejected hunk leaves the file alone
  r = run("patch", {"--dry-run"}, "--- a/p/two.txt\n+++ b/p/two.txt\n@@ -1,1 +1,1 @@\n-1\n+ONE\n");
  CHECK(r.rc == 0 && r.has("would patch") && slurp("p/two.txt") == "1\n2\n3\n");
  put("p/noeol.txt", "a\nb");
  run("patch", {}, "--- a/p/noeol.txt\n+++ b/p/noeol.txt\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n");
  CHECK(slurp("p/noeol.txt") == "a\nc");

  SECTION("grep");
  put("g/a.ts", "const a = 1\nfunction foo() {\n  return a\n}\nconst b = foo()\n");
  put("g/b.py", "def foo():\n    pass\n\ndef bar():\n    foo()\n");
  put("g/c.md", "nothing here\n");
  put("g/.gitignore", "ignored/\n");
  put("g/ignored/x.ts", "foo ignored\n");
  put("g/bin.dat", std::string("foo\0bar", 7));
  r = run("grep", {"foo", "g"});
  CHECK(r.rc == 0);
  CHECK_EQ(r.out, std::string("g/a.ts:2:function foo() {\ng/a.ts:5:const b = foo()\ng/b.py:1:def foo():\ng/b.py:5:    foo()\n"));
  CHECK(run("grep", {"nomatchanywhere", "g"}).rc == kNotFound);
  r = run("grep", {"-F", "foo():", "g"});
  CHECK(r.rc == 0 && r.out == "g/b.py:1:def foo():\n");
  CHECK(run("grep", {"-w", "fo", "g"}).rc == kNotFound);
  CHECK(run("grep", {"-i", "FUNCTION", "g"}).rc == 0);
  r = run("grep", {"foo", "g", "--no-ignore"});
  CHECK(r.has("g/ignored/x.ts:1:"));
  r = run("grep", {"foo", "g", "--type", "py"});
  CHECK(r.rc == 0 && !r.has("a.ts") && r.has("b.py"));
  r = run("grep", {"foo", "g", "--glob", "*.ts"});
  CHECK(r.rc == 0 && r.has("a.ts") && !r.has("b.py"));
  r = run("grep", {"foo", "g", "-g", "!*.ts"});
  CHECK(r.rc == 0 && !r.has("a.ts") && r.has("b.py"));
  r = run("grep", {"-l", "foo", "g"});
  CHECK_EQ(r.out, std::string("g/a.ts\ng/b.py\n"));
  r = run("grep", {"-c", "foo", "g"});
  CHECK_EQ(r.out, std::string("g/a.ts:2\ng/b.py:2\n"));
  r = run("grep", {"-e", "bar", "-e", "return", "g"});
  CHECK(r.has("g/a.ts:3:") && r.has("g/b.py:4:"));
  r = run("grep", {"-C", "1", "return a", "g"});
  CHECK_EQ(r.out, std::string("g/a.ts-2-function foo() {\ng/a.ts:3:  return a\ng/a.ts-4-}\n"));
  r = run("grep", {"-A", "1", "foo", "g/a.ts"});
  CHECK_EQ(r.out, std::string("g/a.ts:2:function foo() {\ng/a.ts-3-  return a\n--\ng/a.ts:5:const b = foo()\n"));
  r = run("grep", {"-B", "1", "const", "g/a.ts"});
  CHECK_EQ(r.out, std::string("g/a.ts:1:const a = 1\n--\ng/a.ts-4-}\ng/a.ts:5:const b = foo()\n"));
  r = run("grep", {"-m", "1", "foo", "g"});
  CHECK(r.has("g/a.ts:2:") && !r.has("g/a.ts:5:") && r.has("3 more matches") && r.has("--max 4"));
  CHECK(!run("grep", {"foo", "g/bin.dat"}).has("bin.dat"));  // binary files are skipped
  r = run("grep", {"foo", "g", "--json"});
  CHECK(parseJson(r.out, j, e) && j.get("matches")->a.size() == 4 && j.get("total_matches")->n == 4);
  CHECK(run("grep", {"(", "g"}).rc == kUsage);
  CHECK(run("grep", {"x", "g/nope"}).rc == kUsage);
  CHECK(run("grep", {}).rc == kUsage);
  put("g/crlf.txt", "foo bar\r\nbaz foo\r\n");
  CHECK(run("grep", {"foo$", "g/crlf.txt"}).out == "g/crlf.txt:2:baz foo\n");
  put("g/long.txt", "pre " + std::string(700, 'x') + " needle\n");
  CHECK(run("grep", {"needle", "g/long.txt"}).has("[+"));
  put("g/u16.txt", std::string("\xFF\xFE" "n\0e\0e\0d\0l\0e\0\n\0", 14));
  CHECK(run("grep", {"needle", "g/u16.txt"}).has("g/u16.txt:1:needle"));
  put("g/uni.txt", "caf\xC3\xA9 \xE2\x80\x94 ok\n");
  CHECK(run("grep", {"-F", "\xE2\x80\x94", "g/uni.txt"}).rc == 0);
  CHECK(run("grep", {"caf.", "g/uni.txt"}).rc == 0);

  SECTION("glob+ls");
  put("l/src/a.ts", "");
  put("l/src/b.test.ts", "");
  put("l/src/deep/c.ts", "");
  put("l/lib/d.rs", "");
  put("l/lib/e.rs", "");
  put("l/.gitignore", "gen/\n");
  put("l/gen/x.ts", "");
  r = run("glob", {"l/src/**/*.ts"});
  CHECK_EQ(r.out, std::string("l/src/a.ts\nl/src/b.test.ts\nl/src/deep/c.ts\n"));
  r = run("glob", {"*.test.ts", "--in", "l"});
  CHECK_EQ(r.out, std::string("l/src/b.test.ts\n"));
  r = run("glob", {"l/lib/{d,e}.rs"});
  CHECK_EQ(r.out, std::string("l/lib/d.rs\nl/lib/e.rs\n"));
  CHECK(run("glob", {"*.nothing", "--in", "l"}).rc == kNotFound);
  CHECK(run("glob", {"**/x.ts", "--in", "l"}).rc == kNotFound);                       // ignored
  CHECK(run("glob", {"**/x.ts", "--in", "l", "--no-ignore"}).out == "l/gen/x.ts\n");
  r = run("glob", {"*.rs", "--in", "l", "-m", "1"});
  CHECK(r.has("1 more path"));
  r = run("ls", {"l"});
  CHECK_EQ(r.out, std::string("lib/\nsrc/\n.gitignore\n"));
  r = run("ls", {"l", "-R"});
  CHECK_EQ(r.out, std::string("lib/\n  d.rs\n  e.rs\nsrc/\n  deep/\n    c.ts\n  a.ts\n  b.test.ts\n.gitignore\n"));
  r = run("ls", {"l", "--depth", "1"});
  CHECK_EQ(r.out, std::string("lib/\nsrc/\n.gitignore\n"));
  r = run("ls", {"l", "-R", "-m", "3"});
  CHECK(r.has("more entries; use --max"));
  CHECK(run("ls", {"l/nope"}).rc == kNotFound);
  put("l/emptydir/.keep", "");
  fs::remove(toPath("l/emptydir/.keep"));
  CHECK(run("ls", {"l/emptydir"}).has("(empty)"));
  CHECK(run("ls", {"l/src/a.ts"}).rc == 0);

  SECTION("find-def+refs");
  put("d/a.ts", "export function widget(a: number) {\n  return a\n}\nexport class Panel {\n  widget() {}\n}\n");
  put("d/b.ts", "import { widget } from './a'\nconst w = widget(1)\n// widgetry is not widget\nwidget2()\n");
  r = run("find-def", {"widget", "d"});
  CHECK(r.rc == 0 && r.has("d/a.ts:1: [function]") && r.has("d/a.ts:5: [method]"));
  CHECK(run("find-def", {"nothing_like_this", "d"}).rc == kNotFound);
  CHECK(run("find-def", {"WIDGET", "d", "-i"}).rc == 0);
  CHECK(run("find-def", {"WIDGET", "d"}).rc == kNotFound);
  r = run("refs", {"widget", "d"});
  CHECK(r.rc == 0 && r.has("definitions (2):") && r.has("imports (1):") && r.has("references (2):"));
  CHECK(r.out.find("definitions") < r.out.find("imports") && r.out.find("imports") < r.out.find("references"));
  CHECK(!r.has("widget2()"));  // whole words only

  SECTION("skim");
  for (int i = 0; i < 30; ++i) put("s/pkg/mod" + std::to_string(i) + "/f.ts", "export function fn" + std::to_string(i) + "() {}\n");
  put("s/README.md", "# Title\n\n## Part\n");
  put("s/main.rs", "fn main() {}\nstruct A;\n");
  r = run("skim", {"s"});
  CHECK(r.rc == 0 && r.has("s/ (") && r.has("TypeScript") && r.has("main.rs (2)  main") && r.has("README.md (3)"));
  r = run("skim", {"s", "--depth", "1"});
  CHECK(r.has("pkg/ (30 files") && !r.has("mod0"));
  std::string small = run("skim", {"s", "--budget", "120"}).out;
  CHECK(small.size() <= 120 * 4 + 200);
  CHECK(run("skim", {"nope"}).rc == kNotFound);
  r = run("skim", {"s/main.rs"});  // a single file is outlined
  CHECK(r.has("fn main"));
  r = run("skim", {"s", "--json"});
  CHECK(parseJson(r.out, j, e) && j.get("tree")->get("files")->n == 32);

  SECTION("mv/cp/rm");
  put("m/a.txt", "A");
  put("m/dir/inner.txt", "I");
  CHECK(run("cp", {"m/a.txt", "m/b.txt"}).rc == 0 && slurp("m/b.txt") == "A");
  CHECK(run("cp", {"m/a.txt", "m/b.txt"}).rc == kNotFound);  // refuses to overwrite
  CHECK(run("cp", {"m/a.txt", "m/b.txt", "--force"}).rc == 0);
  CHECK(run("cp", {"m/dir", "m/dir2"}).rc == kNotFound);       // needs -r
  CHECK(run("cp", {"-r", "m/dir", "m/dir2"}).rc == 0 && slurp("m/dir2/inner.txt") == "I");
  CHECK(run("mv", {"m/b.txt", "m/c.txt"}).rc == 0 && !fs::exists(toPath("m/b.txt")) && fs::exists(toPath("m/c.txt")));
  CHECK(run("mv", {"m/c.txt", "m/a.txt"}).rc == kNotFound);   // would overwrite
  CHECK(run("mv", {"m/c.txt", "m/a.txt", "--force"}).rc == 0 && slurp("m/a.txt") == "A");
  CHECK(run("mv", {"m/a.txt", "m/dir2", "m/dir"}).rc == 0 && fs::exists(toPath("m/dir/a.txt")) && fs::exists(toPath("m/dir/dir2")));
  CHECK(run("mv", {"m/dir/a.txt", "m/dir/x.txt", "m/missing"}).rc == kUsage);  // several sources need a directory
  CHECK(run("rm", {"m/dir"}).rc == kNotFound);                                   // needs -r
  CHECK(run("rm", {"--permanent", "m/dir/x.txt"}).rc == kNotFound);              // does not exist
  CHECK(run("rm", {"-f", "--permanent", "m/dir/x.txt"}).rc == 0);
  CHECK(run("rm", {"--permanent", "m/dir/a.txt"}).rc == 0 && !fs::exists(toPath("m/dir/a.txt")));
  CHECK(run("rm", {"-r", "--permanent", "m/dir"}).rc == 0 && !fs::exists(toPath("m/dir")));
  CHECK(run("rm", {"--permanent", "."}).rc == kUsage);  // never removes the current directory
  CHECK(run("rm", {"--permanent", ".."}).rc == kUsage);
  put("m/trash.txt", "bye");
  r = run("rm", {"m/trash.txt"});  // trash
  CHECK(r.rc == 0 && r.has("moved to trash") && !fs::exists(toPath("m/trash.txt")));
}

static void testRoot() {
  SECTION("root");
  put("r/in/a.txt", "inside\n");
  put("r/out/b.txt", "outside\n");
  Ctx saved = g_ctx;
  g_ctx.root = canonicalPath("r/in");
  CHECK(run("read", {"r/in/a.txt"}).rc == 0);
  CHECK(run("read", {"r/out/b.txt"}).rc == kNotFound);
  CHECK(run("read", {"r/in/../out/b.txt"}).rc == kNotFound);
  CHECK(run("grep", {"x", "r/out"}).rc == kUsage);
  CHECK(run("write", {"r/out/new.txt"}, "x").rc == kUsage && !fs::exists(toPath("r/out/new.txt")));
  CHECK(run("edit", {"r/out/b.txt", "--old", "outside", "--new", "x"}).rc == kUsage && slurp("r/out/b.txt") == "outside\n");
  CHECK(run("rm", {"--permanent", "r/out/b.txt"}).rc == kUsage && fs::exists(toPath("r/out/b.txt")));
  CHECK(run("mv", {"r/in/a.txt", "r/out/a.txt"}).rc == kUsage);
  CHECK(run("ls", {"r/out"}).rc == kNotFound);
  CHECK(run("skim", {"r/out"}).rc == kNotFound);
  CHECK(run("stat", {"r/out/b.txt"}).rc == kNotFound);
  CHECK(run("patch", {}, "--- a/r/out/b.txt\n+++ b/r/out/b.txt\n@@ -1,1 +1,1 @@\n-outside\n+x\n").rc == kUsage);
  // a symlink inside the root that points outside it
  std::error_code ec;
  fs::create_symlink(fs::absolute(toPath("r/out")), toPath("r/in/link"), ec);
  if (!ec) {
    CHECK(run("read", {"r/in/link/b.txt"}).rc == kNotFound);
    CHECK(!run("grep", {"outside", "r/in"}).has("outside"));
    CHECK(!run("glob", {"*.txt", "--in", "r/in"}).has("b.txt"));
  } else {
    std::printf("note: symlink creation not permitted here; symlink confinement not exercised\n");
  }
#ifdef _WIN32
  // Directory junctions need no privilege and behave like symlinks for confinement purposes.
  if (std::system("cmd /c mklink /J r\\in\\junc r\\out >nul 2>&1") == 0) {
    CHECK(run("read", {"r/in/junc/b.txt"}).rc == kNotFound);
    CHECK(run("write", {"r/in/junc/new.txt"}, "x").rc == kUsage && !fs::exists(toPath("r/out/new.txt")));
    CHECK(!run("grep", {"outside", "r/in"}).has("outside"));
  } else {
    std::printf("note: could not create a junction; junction confinement not exercised\n");
  }
#endif
  g_ctx = saved;
}

static void testMcp() {
  SECTION("mcp");
  put("x/a.txt", "hello mcp\n");
  Json j;
  std::string e;
  auto call = [&](const std::string& line) {
    std::string out = handleMcpMessage(g_ctx, line);
    CHECK(parseJson(out, j, e));
    return out;
  };
  call("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\"}}");
  CHECK(j.get("result")->get("protocolVersion")->s == "2024-11-05");
  CHECK(j.get("result")->get("capabilities")->get("tools"));
  call("{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}");
  auto& tools = j.get("result")->get("tools")->a;
  CHECK(tools.size() >= 15);
  for (auto& t : tools) {
    CHECK(t.get("name") && t.get("description") && t.get("inputSchema"));
    CHECK(t.get("inputSchema")->get("type")->s == "object");
  }
  CHECK(handleMcpMessage(g_ctx, "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}").empty());
  call("{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"acu_grep\",\"arguments\":{\"pattern\":\"mcp\",\"paths\":[\"x\"]}}}");
  CHECK(j.get("result")->get("content")->a[0].get("text")->s == "x/a.txt:1:hello mcp\n");
  CHECK(!j.get("result")->get("isError")->b);
  call("{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"acu_edit\",\"arguments\":{\"path\":\"x/a.txt\",\"old\":\"hello\",\"new\":\"bye\"}}}");
  CHECK(slurp("x/a.txt") == "bye mcp\n");
  call("{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"acu_write\",\"arguments\":{\"path\":\"x/w.txt\",\"content\":\"a\\nb\\n\"}}}");
  CHECK(slurp("x/w.txt") == "a\nb\n");
  call("{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"acu_read\",\"arguments\":{\"paths\":[\"x/nope\"]}}}");
  CHECK(j.get("result")->get("isError")->b);
  call("{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":{\"name\":\"acu_read\",\"arguments\":{}}}");
  CHECK(j.get("result")->get("isError")->b);
  call("{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"tools/call\",\"params\":{\"name\":\"nope\",\"arguments\":{}}}");
  CHECK(j.get("error"));
  call("{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"what\"}");
  CHECK(j.get("error")->get("code")->n == -32601);
  call("not json");
  CHECK(j.get("error")->get("code")->n == -32700);
  call("{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"tools/call\",\"params\":{\"name\":\"acu_mv\",\"arguments\":{\"sources\":[\"x/w.txt\"],\"destination\":\"x/w2.txt\"}}}");
  CHECK(fs::exists(toPath("x/w2.txt")));
  call("{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"tools/call\",\"params\":{\"name\":\"acu_patch\",\"arguments\":{\"patch\":\"--- a/x/w2.txt\\n+++ b/x/w2.txt\\n@@ -1,2 +1,2 @@\\n-a\\n+A\\n b\\n\"}}}");
  CHECK(slurp("x/w2.txt") == "A\nb\n");
  // a batch
  std::string out = handleMcpMessage(g_ctx, "[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"method\":\"x\"}]");
  CHECK(parseJson(out, j, e) && j.t == Json::Arr && j.a.size() == 1);
}

static void testEditFuzz() {
  SECTION("edit/patch fuzz");
  std::mt19937 rng(99);
  auto rnd = [&](int n) {
    std::string s;
    for (int i = 0; i < n; ++i) s.push_back("ab \n\t\r{}"[rng() % 9]);
    return s;
  };
  for (int i = 0; i < 400; ++i) {
    std::string content = rnd(1 + (int)(rng() % 60)), oldS = rnd(1 + (int)(rng() % 4)), newS = rnd((int)(rng() % 5));
    put("f/e.txt", content);
    auto r = run("edit", {"f/e.txt", "--old", oldS, "--new", newS, "--all"});
    CHECK(r.rc == 0 || r.rc == kNotFound);
    if (r.rc != 0) CHECK(slurp("f/e.txt") == content);  // a refused edit never changes the file
    std::string d = "--- a/f/e.txt\n+++ b/f/e.txt\n@@ -" + std::to_string(rng() % 5) + "," + std::to_string(rng() % 4) + " +1,1 @@\n" + rnd(10);
    auto pr = run("patch", {"--dry-run"}, d);
    CHECK(pr.rc == 0 || pr.rc == kNotFound || pr.rc == kUsage);
  }
}

static void testErrorsAndHelp() {
  SECTION("cli");
  CHECK(run("nope", {}).rc == kUsage);
  CHECK(run("grep", {"--bogus", "x"}).rc == kUsage);
  CHECK(run("read", {"--max"}).rc == kUsage);
  CHECK(run("read", {"x", "--max", "abc"}).rc == kUsage);
  CHECK(run("read", {"--help"}).has("usage: acu read"));
  CHECK(run("agent-guide", {}).out.find("acu skim") != std::string::npos);
  CHECK(helpText().find("mcp") != std::string::npos);
  for (auto* c : allCommands()) CHECK(run(c->name, {"--help"}).rc == 0);
  Args a;
  std::string err;
  CHECK(parseArgs({"-iw", "pat", "-C3", "--type=ts", "--", "-x"}, findCommand("grep")->opts, a, err));
  CHECK(a.has("ignore-case") && a.has("word-regexp") && a.str("context") == "3" && a.str("type") == "ts");
  CHECK(a.pos.size() == 2 && a.pos[0] == "pat" && a.pos[1] == "-x");
}

int main() {
  initProcess();
  fs::path tmp = fs::temp_directory_path() / ("acu-tests-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
  fs::create_directories(tmp);
  std::string prev = currentDir();
  setCurrentDir(fromPath(tmp));

  testRegex();
  testFuzzRegex();
  testGlob();
  testIgnore();
  testJson();
  testText();
  testOutline();
  testCommands();
  testRoot();
  testMcp();
  testEditFuzz();
  testErrorsAndHelp();

  setCurrentDir(prev);
  std::error_code ec;
  fs::remove_all(tmp, ec);
  std::printf("\n%d checks, %d failed\n", g_checks, g_fail);
  return g_fail ? 1 : 0;
}
