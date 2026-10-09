// index
#include "cmd/cmd.hpp"
#include "core/platform.hpp"
#include "index/daemon.hpp"
#include "index/index.hpp"
#include "index/use.hpp"

namespace acu {

namespace {

int cmdIndex(Ctx& ctx, Args& a, Io& io) {
  if (a.pos.size() > 1) throw UsageError{"expected at most one directory"};
  std::string existingRoot;
  bool haveExisting = findIndexRoot(a.pos.empty() ? "." : a.pos[0], existingRoot);

  if (a.has("rm")) {
    if (!haveExisting) {
      io.err += "index: no index found here\n";
      return kNotFound;
    }
    std::error_code ec;
    fs::remove_all(toPath(joinPath(existingRoot, ".acu")), ec);
    io.out += "removed " + joinPath(existingRoot, ".acu") + "\n";
    return ec ? kNotFound : kOk;
  }

  if (a.has("status")) {
    if (!haveExisting) {
      io.out += "no index (run `acu index` to build one)\n";
      return kNotFound;
    }
    Index ix;
    std::string err;
    if (!Index::load(indexPathFor(existingRoot), ix, err)) {
      io.err += "index: " + err + "\n";
      return kNotFound;
    }
    FileInfo fi;
    statPath(indexPathFor(existingRoot), fi);
    size_t syms = ix.symbols.size(), bin = 0, big = 0;
    for (auto& f : ix.files) {
      bin += (f.flags & kIdxBinary) != 0;
      big += (f.flags & kIdxNoTrigrams) != 0;
    }
    io.out += "index at " + existingRoot + "/.acu/index (" + fmtTime(fi.mtime) + ")\n  " + fmtInt(ix.files.size()) +
              " files (" + fmtInt(bin) + " binary, " + fmtInt(big) + " too large to index), " + fmtInt(syms) + " symbols, " +
              fmtInt(ix.triKeys.size()) + " trigrams, " + fmtInt(fi.size / 1024) + " KB on disk\n";
    return kOk;
  }

  std::string root = haveExisting && a.pos.empty() ? existingRoot : canonicalPath(a.pos.empty() ? "." : a.pos[0]);
  if (!ctx.allow(root, *std::make_unique<std::string>())) {
    io.err += "index: " + root + ": outside the allowed root\n";
    return kUsage;
  }
  FileInfo rfi;
  if (!statPath(root, rfi) || !rfi.isDir) {
    io.err += "index: " + root + ": not a directory\n";
    return kNotFound;
  }
  Index prev;
  bool havePrev = false;
  if (!a.has("full")) {
    std::string err;
    havePrev = Index::load(indexPathFor(root), prev, err) && prev.root == root;
  }
  Index ix;
  BuildStats st;
  std::string err;
  if (!buildIndex(root, ctx, havePrev ? &prev : nullptr, ix, err, &st)) {
    io.err += "index: " + err + "\n";
    return kNotFound;
  }
  if (!ix.save(indexPathFor(root), err)) {
    io.err += "index: " + err + "\n";
    return kNotFound;
  }
  FileInfo fi;
  statPath(indexPathFor(root), fi);
  char ms[32];
  std::snprintf(ms, sizeof ms, "%.0f", st.ms);
  io.out += "indexed " + fmtInt(st.files) + " files in " + ms + " ms (" + fmtInt(st.reused) + " reused, " + fmtInt(st.read) +
            " read, " + fmtInt(st.removed) + " removed); " + fmtInt(st.trigrams) + " trigrams, " + fmtInt(fi.size / 1024) +
            " KB at " + root + "/.acu/index\n";
  return kOk;
}

}  // namespace

const Command cmd_index = {"index",
                           "index [dir] [--full] [--status] [--rm]    build or refresh the project index in dir/.acu (grep, skim, find-def and refs use it automatically)",
                           {{"full", 0, false}, {"status", 0, false}, {"rm", 0, false}},
                           cmdIndex};

const Command cmd_daemon = {"daemon",
                            "daemon start|stop|status [dir] [--idle MIN]   keep the index in memory, kept fresh by a file watcher; read-only commands then skip loading it and walking the tree",
                            {{"idle", 0, true}},
                            daemonCommand};

}  // namespace acu
