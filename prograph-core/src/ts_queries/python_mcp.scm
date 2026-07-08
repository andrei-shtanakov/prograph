; Server-side tool declarations
;
; Pattern 1: `@server.tool(...)` or `@<expr>.tool(...)` decorator on a function definition.
; The decorator's last attribute name is `tool`, and we capture the function name.
(decorated_definition
  (decorator
    (call
      function: (attribute attribute: (identifier) @decorator_name)
      (#eq? @decorator_name "tool")))
  definition: (function_definition name: (identifier) @tool_name)) @tool_decl

; Pattern 2: bare attribute decorator like `@server.tool` (no call).
(decorated_definition
  (decorator
    (attribute attribute: (identifier) @decorator_name)
    (#eq? @decorator_name "tool"))
  definition: (function_definition name: (identifier) @tool_name)) @tool_decl_bare

; Pattern 3: `server.add_tool("name", ...)` or `server.register_tool("name", ...)`.
(call
  function: (attribute attribute: (identifier) @method)
  arguments: (argument_list . (string) @tool_name_literal)
  (#match? @method "^(add_tool|register_tool)$")) @tool_decl_call

; Client-side tool invocations
;
; Pattern: `<expr>.call_tool("name", ...)` / `_call_tool("name", ...)` /
; `<expr>.invoke_tool("name", ...)`. The leading underscore variant catches
; private retry/reconnect wrappers (common Python convention; see Maestro's
; arbiter_client.py for the original motivator).
(call
  function: (attribute attribute: (identifier) @method)
  arguments: (argument_list . (string) @tool_name_literal)
  (#match? @method "^(_?call_tool|invoke_tool)$")) @tool_use_call
