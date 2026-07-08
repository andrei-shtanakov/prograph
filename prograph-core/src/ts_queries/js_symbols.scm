; Exported function declaration: `export function foo() {}`.
(export_statement
  declaration: (function_declaration
    name: (identifier) @symbol_name)) @symbol_function_export

; Exported class declaration.
(export_statement
  declaration: (class_declaration
    name: (identifier) @symbol_name)) @symbol_class_export

; Exported const: `export const FOO = ...`.
(export_statement
  declaration: (lexical_declaration
    (variable_declarator
      name: (identifier) @symbol_name))) @symbol_const_export

; Internal imports: `import x from './y'` or `import x from '../y'`.
(import_statement
  source: (string) @import_source
  (#match? @import_source "^['\"]\\.{1,2}/")) @import_relative
