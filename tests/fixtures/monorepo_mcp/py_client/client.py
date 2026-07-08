async def run(session):
    result = await session.call_tool("decide", arguments={"query": "x"})
    return result
