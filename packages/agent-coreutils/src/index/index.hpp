#pragma once
// A persistent index of a project: the file list (with sizes and mtimes), per-file line counts and symbols, and a
// trigram inverted index for content search, as in Zoekt or Google Code Search.
//
// The index is never trusted blindly. Callers compare each file's current size and mtime with its entry; a file that
// differs, or is not in the index at all, is read directly. Results are therefore the same with or without the
// index; it only changes how many files have to be opened.
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include "core/common.hpp"
#include "core/lang.hpp"

namespace acu {

enum IndexFlags : uint8_t {
  kIdxBinary = 1,      // binary file: never searched
  kIdxNoTrigrams = 2,  // content not indexed (too large / unreadable): always a candidate
};

struct IndexedFile {
  std::string path;  // relative to the index root, '/' separators
  uint64_t size = 0;
  int64_t mtime = 0;
  uint32_t lines = 0;
  uint8_t flags = 0;
  uint32_t symBegin = 0, symEnd = 0;
};

struct BuildStats {
  size_t files = 0, reused = 0, read = 0, removed = 0;
  size_t trigrams = 0;
  double ms = 0;
};

class Index {
 public:
  std::string root;  // absolute, '/' separators, no trailing slash
  std::vector<IndexedFile> files;  // sorted by path
  std::vector<Symbol> symbols;
  std::vector<uint32_t> triKeys;   // distinct trigrams, sorted
  std::vector<uint32_t> triOffs;   // triKeys.size() + 1 offsets into triData
  std::vector<uint8_t> triData;    // per trigram: file ids as delta varints

  // Index of `rel` in files, or -1.
  int find(const std::string& rel) const;

  // Files that may contain any of `literals` (ASCII case-insensitively when icase is irrelevant: trigrams are
  // lowercased). Returns false if the literals are too short to narrow anything; otherwise fills `ids` (sorted).
  bool candidates(const std::vector<std::string>& literals, std::vector<uint32_t>& ids) const;

  void decodePostings(size_t keyIdx, std::vector<uint32_t>& out) const;

  bool save(const std::string& path, std::string& err) const;
  static bool load(const std::string& path, Index& out, std::string& err);

  size_t memoryBytes() const;
};

// Walks `root` (an absolute directory) with ignore rules and indexes it. `prev` lets unchanged files be reused
// without reading them. Fills `out` and (optionally) `stats`.
bool buildIndex(const std::string& root, const Ctx& ctx, const Index* prev, Index& out, std::string& err,
                BuildStats* stats);

// Reads one file the way the index does: returns its entry plus sorted unique lowercased trigrams.
void indexOneFile(const std::string& absPath, const std::string& rel, uint64_t size, int64_t mtime, IndexedFile& f,
                  std::vector<Symbol>& syms, std::vector<uint32_t>& trigrams);

// Where an index for a project rooted at `root` lives, and finding one by walking up from `dir`.
std::string indexPathFor(const std::string& root);
bool findIndexRoot(const std::string& dir, std::string& root);

}  // namespace acu
