; Public functions.
(function_item
  (visibility_modifier) @vis_pub
  name: (identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_function

; Public structs.
(struct_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_struct

; Public enums.
(enum_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_enum

; Public traits.
(trait_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_trait

; Public consts.
(const_item
  (visibility_modifier) @vis_pub
  name: (identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_const

; Public statics.
(static_item
  (visibility_modifier) @vis_pub
  name: (identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_const_static

; Public type aliases.
(type_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_type

; Internal imports: capture ALL use_declarations; Rust-side post-filter keeps
; only those whose first segment is `crate`.
(use_declaration) @import_use
