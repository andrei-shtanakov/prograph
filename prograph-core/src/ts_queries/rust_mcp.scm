; Server-side tool declarations
;
; Pattern 1: `.tool("name", ...)`, `.register_tool("name", ...)`, `.add_tool("name", ...)`
; — method-call form where the first arg is a string literal.
(call_expression
  function: (field_expression field: (field_identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#match? @method "^(tool|register_tool|add_tool)$")) @tool_decl_method

; Pattern 2: `Tool::new("name")` or `ToolBuilder::new("name")` — associated function call.
(call_expression
  function: (scoped_identifier
    path: (identifier) @type
    name: (identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#match? @type "^(Tool|ToolBuilder|ToolHandler)$")
  (#eq? @method "new")) @tool_decl_assoc

; Client-side tool invocations
;
; Pattern: `.call_tool("name", ...)` or `.invoke_tool("name", ...)` method call.
(call_expression
  function: (field_expression field: (field_identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#match? @method "^(call_tool|invoke_tool)$")) @tool_use_method
