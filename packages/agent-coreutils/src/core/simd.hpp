#pragma once
// SIMD substring search.
//
// Two bytes of the needle (the rarest in typical source text, as ripgrep picks them) are compared against 32 (AVX2)
// or 16 (SSE2) haystack positions at once; only positions where both match are verified. The instruction set is
// chosen at run time. Case-insensitive search folds ASCII letters in the vector compare instead of copying and
// lowering the haystack.
#include <cstddef>
#include <string>
#include <string_view>

namespace acu {

struct Needle {
  std::string bytes;  // lowercased when icase
  bool icase = false;
  size_t p1 = 0, p2 = 0;  // positions of the two probe bytes
  explicit Needle(std::string_view lit = {}, bool icase = false);
};

constexpr size_t kNoPos = (size_t)-1;

// First occurrence of `n` in hay[from, size), or kNoPos.
size_t findNeedle(const Needle& n, const char* hay, size_t size, size_t from);

// "avx2", "sse2" or "scalar": what findNeedle uses on this machine.
const char* simdLevel();

}  // namespace acu
