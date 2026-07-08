async def run(session):
    a = await session.call_tool("decide", arguments={})
    b = await session.call_tool("evaluate", arguments={})
    return (a, b)
