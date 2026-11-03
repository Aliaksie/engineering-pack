; JavaScript and TypeScript syntax evidence: no claims about runtime calls or module
; resolution. When two patterns describe one node, the earlier pattern names its kind.

; Functions, including those bound to a name, a property or an object key.
(function_declaration name: (identifier) @name) @declares.function @container
(generator_function_declaration name: (identifier) @name) @declares.function @container
(variable_declarator
  name: (identifier) @name
  value: [(arrow_function) (function_expression) (generator_function)]) @declares.function @container
(assignment_expression
  left: (member_expression property: (property_identifier) @name)
  right: [(arrow_function) (function_expression)]) @declares.function @container
(pair
  key: (property_identifier) @name
  value: [(arrow_function) (function_expression)]) @declares.method @container
(method_definition name: (_) @name) @declares.method @container
(class_declaration name: (_) @name) @declares.class
(variable_declarator name: (identifier) @name) @declares.variable

; Code directly in a class body runs when the class is built, not in a method.
(class_body) @boundary

; `f()`, `a.f()`, `a.#f()`, `new F()`. A name, not a resolved callee.
((call_expression function: (identifier) @name) @invokes
  (#not-eq? @name "require"))
(call_expression function: (member_expression property: (property_identifier) @name)) @invokes
(call_expression function: (member_expression property: (private_property_identifier) @name)) @invokes
(new_expression constructor: (identifier) @name) @invokes

(import_statement source: (string) @target) @imports
(export_statement source: (string) @target) @imports
((call_expression
  function: (identifier) @_require
  arguments: (arguments . (string) @target .)) @imports
  (#eq? @_require "require"))
(call_expression function: (import) arguments: (arguments . (string) @target)) @dynamic-import
(call_expression function: (import)) @dynamic-import

; One row per imported name, so "who imports X" finds `import { X } from './m'`.
(import_statement
  (import_clause (named_imports (import_specifier name: (_) @name) @imports.name))
  source: (string) @target)
(import_statement (import_clause (identifier) @name @imports.name) source: (string) @target)
