; Rust syntax evidence. Call names are not resolved targets, and macro arguments are token
; trees, so a call there is recognized only by its `name(` spelling.
(function_item name: (_) @name) @declares.fn @container
(function_signature_item name: (_) @name) @declares.fn
(struct_item name: (_) @name) @declares.type
(enum_item name: (_) @name) @declares.type
(union_item name: (_) @name) @declares.type
(trait_item name: (_) @name) @declares.type
(type_item name: (_) @name) @declares.type
(const_item name: (_) @name) @declares.const
(static_item name: (_) @name) @declares.const
(mod_item name: (_) @name) @declares.module
(macro_definition name: (_) @name) @declares.macro

; An item body inside a function (impl, trait, module) does not run as that function.
(declaration_list) @boundary

; `f()`, `a::f()`, `x.f()`, `f::<T>()`.
(call_expression function: (identifier) @name) @invokes
(call_expression function: (scoped_identifier name: (_) @name)) @invokes
(call_expression function: (field_expression field: (field_identifier) @name)) @invokes
(call_expression function: (generic_function function: (identifier) @name)) @invokes
(call_expression
  function: (generic_function function: (scoped_identifier name: (_) @name))) @invokes
(call_expression
  function: (generic_function function: (field_expression field: (field_identifier) @name))) @invokes

; `name(` inside a macro invocation's arguments, outside attributes and macro definitions.
((token_tree (identifier) @name @invokes . (token_tree) @arguments)
  (#adjacent? @name @arguments)
  (#match? @arguments "^\\(")
  (#not-after? @name "fn")
  (#nearest? @name "macro_invocation" "attribute" "macro_definition"))

; A path to a function used as a value, `and_then(prompt::request)` or `let f = a::f;`. Only a
; lowercase last segment counts, so variants, constants and types are not taken for functions.
; A bare identifier is left out: it is far more often a local value than a function.
((arguments (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))
((array_expression (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))
((tuple_expression (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))
((return_expression (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))
((reference_expression (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))
((let_declaration value: (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))
((field_initializer value: (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))
((assignment_expression right: (scoped_identifier name: (identifier) @name) @references)
  (#match? @name "^[\\p{Ll}_]"))

; One imported path per `use` tree leaf.
(use_declaration argument: (_) @imports)
(use_list (_) @imports)
