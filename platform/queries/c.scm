; C syntax evidence: declarations, calls by name, and includes as written.
(function_definition
  declarator: (function_declarator declarator: (identifier) @name)) @declares.function @container
(function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator declarator: (identifier) @name))) @declares.function @container
(struct_specifier name: (type_identifier) @name body: (_)) @declares.type
(union_specifier name: (type_identifier) @name body: (_)) @declares.type
(enum_specifier name: (type_identifier) @name body: (_)) @declares.type
(type_definition declarator: (type_identifier) @name) @declares.type
(preproc_def name: (identifier) @name) @declares.macro
(preproc_function_def name: (identifier) @name) @declares.macro

(call_expression function: (identifier) @name) @invokes
(call_expression function: (field_expression field: (field_identifier) @name)) @invokes

(preproc_include path: (_) @target) @imports
