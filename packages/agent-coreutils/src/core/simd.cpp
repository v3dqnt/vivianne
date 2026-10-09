#include "core/simd.hpp"

#include <algorithm>
#include <cstring>

#if defined(__x86_64__) || defined(_M_X64) || defined(__i386__)
#define ACU_X86 1
#include <immintrin.h>
#endif

namespace acu {

namespace {

// Rough frequency rank of each byte in source code and prose: higher = more common = a worse probe.
const unsigned char* rankTable() {
  static unsigned char t[256];
  static bool init = [] {
    for (int i = 0; i < 256; ++i) t[i] = 20;  // control and high bytes: fairly rare
    const char* common = " etaoinsrhldcumfpgwybvkxjqz";  // roughly most to least common letters
    for (int i = 0; common[i]; ++i) {
      int rank = 255 - i * 3;
      unsigned char c = (unsigned char)common[i];
      t[c] = (unsigned char)rank;
      if (c >= 'a' && c <= 'z') t[c - 32] = (unsigned char)(rank - 60);  // capitals are rarer
    }
    for (char c : std::string("0123456789")) t[(unsigned char)c] = 120;
    for (char c : std::string("_.,;:=-/\"'")) t[(unsigned char)c] = 150;
    for (char c : std::string("(){}[]<>*+&|!?#$%@\\^~`")) t[(unsigned char)c] = 100;
    t['\n'] = 200;
    t['\t'] = 150;
    return true;
  }();
  (void)init;
  return t;
}

inline unsigned char lowerAscii(unsigned char c) { return (c >= 'A' && c <= 'Z') ? (unsigned char)(c + 32) : c; }

inline bool verify(const Needle& n, const char* p) {
  const size_t m = n.bytes.size();
  if (!n.icase) return std::memcmp(p, n.bytes.data(), m) == 0;
  for (size_t i = 0; i < m; ++i)
    if (lowerAscii((unsigned char)p[i]) != (unsigned char)n.bytes[i]) return false;
  return true;
}

size_t findScalar(const Needle& n, const char* h, size_t size, size_t from) {
  const size_t m = n.bytes.size();
  if (size < m) return kNoPos;
  if (!n.icase) {
    const unsigned char first = (unsigned char)n.bytes[n.p1];
    for (size_t s = from; s + m <= size;) {
      const void* hit = std::memchr(h + s + n.p1, first, size - m - s + 1);
      if (!hit) return kNoPos;
      s = (size_t)((const char*)hit - h) - n.p1;
      if (verify(n, h + s)) return s;
      ++s;
    }
    return kNoPos;
  }
  for (size_t s = from; s + m <= size; ++s)
    if (verify(n, h + s)) return s;
  return kNoPos;
}

#ifdef ACU_X86

size_t findSse2(const Needle& n, const char* h, size_t size, size_t from) {
  const size_t m = n.bytes.size();
  if (size < m) return kNoPos;
  const size_t last = std::max(n.p1, n.p2);
  const unsigned char c1 = (unsigned char)n.bytes[n.p1], c2 = (unsigned char)n.bytes[n.p2];
  const bool f1 = n.icase && c1 >= 'a' && c1 <= 'z', f2 = n.icase && c2 >= 'a' && c2 <= 'z';
  const __m128i b1 = _mm_set1_epi8((char)c1), b2 = _mm_set1_epi8((char)c2), fold = _mm_set1_epi8(0x20);
  size_t i = from;
  while (i + 16 + last <= size) {
    __m128i v1 = _mm_loadu_si128((const __m128i*)(h + i + n.p1));
    __m128i v2 = _mm_loadu_si128((const __m128i*)(h + i + n.p2));
    if (f1) v1 = _mm_or_si128(v1, fold);
    if (f2) v2 = _mm_or_si128(v2, fold);
    unsigned mask = (unsigned)_mm_movemask_epi8(_mm_and_si128(_mm_cmpeq_epi8(v1, b1), _mm_cmpeq_epi8(v2, b2)));
    while (mask) {
#if defined(_MSC_VER)
      unsigned long bit;
      _BitScanForward(&bit, mask);
#else
      unsigned bit = (unsigned)__builtin_ctz(mask);
#endif
      size_t s = i + bit;
      if (s + m <= size && verify(n, h + s)) return s;
      mask &= mask - 1;
    }
    i += 16;
  }
  for (size_t s = i; s + m <= size; ++s)
    if (verify(n, h + s)) return s;
  return kNoPos;
}

#if defined(__GNUC__) || defined(__clang__)
__attribute__((target("avx2")))
size_t findAvx2(const Needle& n, const char* h, size_t size, size_t from) {
  const size_t m = n.bytes.size();
  if (size < m) return kNoPos;
  const size_t last = std::max(n.p1, n.p2);
  const unsigned char c1 = (unsigned char)n.bytes[n.p1], c2 = (unsigned char)n.bytes[n.p2];
  const bool f1 = n.icase && c1 >= 'a' && c1 <= 'z', f2 = n.icase && c2 >= 'a' && c2 <= 'z';
  const __m256i b1 = _mm256_set1_epi8((char)c1), b2 = _mm256_set1_epi8((char)c2), fold = _mm256_set1_epi8(0x20);
  size_t i = from;
  while (i + 32 + last <= size) {
    __m256i v1 = _mm256_loadu_si256((const __m256i*)(h + i + n.p1));
    __m256i v2 = _mm256_loadu_si256((const __m256i*)(h + i + n.p2));
    if (f1) v1 = _mm256_or_si256(v1, fold);
    if (f2) v2 = _mm256_or_si256(v2, fold);
    unsigned mask = (unsigned)_mm256_movemask_epi8(_mm256_and_si256(_mm256_cmpeq_epi8(v1, b1), _mm256_cmpeq_epi8(v2, b2)));
    while (mask) {
      unsigned bit = (unsigned)__builtin_ctz(mask);
      size_t s = i + bit;
      if (s + m <= size && verify(n, h + s)) return s;
      mask &= mask - 1;
    }
    i += 32;
  }
  for (size_t s = i; s + m <= size; ++s)
    if (verify(n, h + s)) return s;
  return kNoPos;
}
#define ACU_HAVE_AVX2 1
#endif

#endif  // ACU_X86

enum class Level { Scalar, Sse2, Avx2 };

Level detect() {
#ifdef ACU_X86
#ifdef ACU_HAVE_AVX2
  __builtin_cpu_init();
  if (__builtin_cpu_supports("avx2")) return Level::Avx2;
#endif
  return Level::Sse2;
#else
  return Level::Scalar;
#endif
}

Level level() {
  static const Level l = detect();
  return l;
}

}  // namespace

Needle::Needle(std::string_view lit, bool ic) : bytes(lit), icase(ic) {
  if (icase)
    for (auto& c : bytes) c = (char)lowerAscii((unsigned char)c);
  if (bytes.empty()) return;
  const unsigned char* rank = rankTable();
  size_t best = 0;
  for (size_t i = 1; i < bytes.size(); ++i)
    if (rank[(unsigned char)bytes[i]] < rank[(unsigned char)bytes[best]]) best = i;
  size_t second = best == 0 ? (bytes.size() > 1 ? 1 : 0) : 0;
  for (size_t i = 0; i < bytes.size(); ++i)
    if (i != best && rank[(unsigned char)bytes[i]] < rank[(unsigned char)bytes[second]]) second = i;
  p1 = best;
  p2 = second;
}

size_t findNeedle(const Needle& n, const char* hay, size_t size, size_t from) {
  if (n.bytes.empty()) return from <= size ? from : kNoPos;
  if (from >= size || size - from < n.bytes.size()) return kNoPos;
#ifdef ACU_X86
  switch (level()) {
#ifdef ACU_HAVE_AVX2
    case Level::Avx2: return findAvx2(n, hay, size, from);
#endif
    case Level::Sse2: return findSse2(n, hay, size, from);
    default: break;
  }
#endif
  return findScalar(n, hay, size, from);
}

const char* simdLevel() {
  switch (level()) {
    case Level::Avx2: return "avx2";
    case Level::Sse2: return "sse2";
    default: return "scalar";
  }
}

}  // namespace acu
