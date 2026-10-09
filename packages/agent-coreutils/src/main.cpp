#include <cstdio>
#include <cstdlib>

#include "cmd/cmd.hpp"
#include "core/platform.hpp"
#include "mcp/mcp.hpp"

using namespace acu;

int main(int argc, char** argv) {
  initProcess();
  std::vector<std::string> args = processArgs(argc, argv);

  // Global option: --root DIR / --root=DIR, anywhere before a bare `--`.
  std::string root = getEnv("ACU_ROOT");
  std::vector<std::string> rest;
  bool endOpts = false;
  for (size_t i = 0; i < args.size(); ++i) {
    const std::string& a = args[i];
    if (!endOpts && a == "--") endOpts = true;
    if (!endOpts && a == "--root") {
      if (i + 1 >= args.size()) {
        writeErr("acu: --root needs a directory\n");
        return kUsage;
      }
      root = args[++i];
    } else if (!endOpts && a.rfind("--root=", 0) == 0) {
      root = a.substr(7);
    } else {
      rest.push_back(a);
    }
  }
  if (rest.empty() || rest[0] == "--help" || rest[0] == "-h" || rest[0] == "help") {
    writeOut(helpText());
    return rest.empty() ? kUsage : kOk;
  }
  if (rest[0] == "--version" || rest[0] == "version") {
    writeOut("acu " ACU_VERSION "\n");
    return kOk;
  }

  Ctx ctx;
  if (!root.empty()) {
    FileInfo fi;
    if (!statPath(root, fi) || !fi.isDir) {
      writeErr("acu: --root " + root + ": not a directory\n");
      return kUsage;
    }
    ctx.root = canonicalPath(root);
  }

  if (rest[0] == "mcp") {
    if (!ctx.root.empty() && !setCurrentDir(ctx.root)) {
      writeErr("acu: cannot change to " + ctx.root + "\n");
      return kUsage;
    }
    return runMcpServer(ctx);
  }

  std::string name = rest[0];
  rest.erase(rest.begin());
  Io io;
  int rc = runCommand(ctx, name, rest, io);
  writeOut(io.out);
  writeErr(io.err);
  return rc;
}
