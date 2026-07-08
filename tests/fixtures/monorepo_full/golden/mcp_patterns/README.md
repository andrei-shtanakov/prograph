# MCP detection pattern overrides

Drop `python.scm` or `rust.scm` files here to extend the bundled
tree-sitter queries used by `detectors/mcp`. They are appended to the
built-in queries; queries are run with the same capture-name conventions
(`tool_name`, `tool_name_literal`, `tool_use_call`, `tool_use_method`).
