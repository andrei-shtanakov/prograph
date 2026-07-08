; Top-level function definitions (children of the module node).
(module
  (function_definition
    name: (identifier) @symbol_name)) @symbol_function

; Top-level class definitions.
(module
  (class_definition
    name: (identifier) @symbol_name)) @symbol_class

; Top-level assignments (potential constants like NAME = "...").
(module
  (expression_statement
    (assignment
      left: (identifier) @symbol_name))) @symbol_const

; Imports.
;
; Pattern 1: `import foo` or `import foo.bar`.
(import_statement
  name: (dotted_name) @import_target) @import_simple

; Pattern 2: `from foo.bar import x, y` — capture the module path AND each
; imported name. Tree-sitter emits one match per `name:` capture, so a list of
; n imports yields n matches with the same `import_target`.
(import_from_statement
  module_name: (dotted_name) @import_target
  name: (dotted_name) @import_symbol) @import_from

; Pattern 2b: `from foo import x as y` — aliased form. The original name `x`
; lives inside an `aliased_import` node; capture it via the inner dotted_name.
(import_from_statement
  module_name: (dotted_name) @import_target
  name: (aliased_import
    name: (dotted_name) @import_symbol)) @import_from_aliased

; Pattern 3: `from .relative import x`.
(import_from_statement
  module_name: (relative_import) @import_target) @import_from_relative
