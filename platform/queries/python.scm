; Python syntax evidence. A call names an attribute or identifier, not a resolved callee.
; When two patterns describe one node, the earlier pattern names its kind.
(class_definition
  body: (block (function_definition name: (identifier) @name) @declares.method @container))
(class_definition
  body: (block (decorated_definition
    definition: (function_definition name: (identifier) @name) @declares.method @container)))
(function_definition name: (identifier) @name) @declares.function @container
(class_definition name: (identifier) @name) @declares.class
(module (expression_statement (assignment left: (identifier) @name) @declares.variable))

; Code directly in a class body runs when the class is built, not in a method.
(class_definition body: (block) @boundary)

(call function: (identifier) @name) @invokes
(call function: (attribute attribute: (identifier) @name)) @invokes

(import_statement name: (dotted_name) @target @imports)
(import_statement name: (aliased_import name: (dotted_name) @target @imports))
(import_from_statement module_name: (_) @target @imports)

; One row per imported name, so "who imports X" finds `from m import X`.
(import_from_statement
  module_name: (_) @target name: (dotted_name) @name @imports.name)
(import_from_statement
  module_name: (_) @target name: (aliased_import name: (dotted_name) @name) @imports.name)
