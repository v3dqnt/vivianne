// agent-guide
#include "cmd/cmd.hpp"

namespace acu {

namespace {

const char kGuide[] =
#include "agent_guide.inc"
    ;

int cmdGuide(Ctx&, Args&, Io& io) {
  io.out += kGuide;
  if (io.out.back() != '\n') io.out += "\n";
  return kOk;
}

}  // namespace

const Command cmd_guide = {"agent-guide", "agent-guide                               print a short guide to paste into an agent's instructions", {}, cmdGuide};

}  // namespace acu
