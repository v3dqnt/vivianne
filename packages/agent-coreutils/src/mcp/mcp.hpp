#pragma once
#include <string>

#include "core/common.hpp"
#include "core/json.hpp"

namespace acu {

// Serves every command as an MCP tool over stdio (newline-delimited JSON-RPC 2.0). Returns the exit code.
int runMcpServer(Ctx& ctx);

// Handles one JSON-RPC message and returns the response to send ("" for notifications). Exposed for tests.
std::string handleMcpMessage(Ctx& ctx, const std::string& line);

}  // namespace acu
