from mcp.server.fastmcp import FastMCP

server = FastMCP("py-server")


@server.tool()
def decide(query: str) -> dict:
    return {"answer": "yes"}
