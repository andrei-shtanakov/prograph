from mcp.server.fastmcp import FastMCP

mcp = FastMCP("declarer")


@mcp.tool()
def tool_real() -> str:
    return "ok"
