#pragma once
// The only place that talks to the operating system directly.
#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

namespace acu {
namespace fs = std::filesystem;

struct DirEntry {
  std::string name;
  bool isDir = false, isLink = false;
  uint64_t size = 0;
  int64_t mtime = 0;    // unix seconds
  int64_t mtimeNs = 0;  // unix nanoseconds, for change detection
};

struct FileInfo {
  bool exists = false, isDir = false, isLink = false;
  uint64_t size = 0;
  int64_t mtime = 0;
  int64_t mtimeNs = 0;
};

fs::path toPath(const std::string& utf8);
std::string fromPath(const fs::path& p);  // UTF-8, forward slashes

bool listDir(const std::string& dir, std::vector<DirEntry>& out);
bool statPath(const std::string& p, FileInfo& fi);  // follows symlinks; isLink says whether p itself is one
bool readFile(const std::string& p, std::string& out, std::string& err);
bool atomicWrite(const std::string& p, std::string_view data, std::string& err);
bool moveToTrash(const std::string& p, std::string& err);

std::string currentDir();
bool setCurrentDir(const std::string& p);
std::string getEnv(const char* name);
std::string absolutePath(const std::string& p);   // absolute, lexically normalised
std::string canonicalPath(const std::string& p);  // symlinks of the existing prefix resolved

void initProcess();  // binary stdio, UTF-8 console
std::vector<std::string> processArgs(int argc, char** argv);  // UTF-8, without argv[0]
bool readAllStdin(std::string& out);
void writeOut(const std::string& s);
void writeErr(const std::string& s);

}  // namespace acu
