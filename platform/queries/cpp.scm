; C++ syntax evidence: declarations, calls by name, and includes as written.
(function_definition
  declarator: (function_declarator
    declarator: [(identifier) (field_identifier) (destructor_name) (operator_name)] @name)) @declares.function @container
(function_definition
  declarator: (function_declarator
    declarator: (qualified_identifier name: (_) @name))) @declares.method @container
(function_definition
  declarator: (pointer_declarator
    declarator: (function_declarator declarator: (_) @name))) @declares.function @container
(function_definition
  declarator: (reference_declarator
    (function_declarator declarator: (_) @name))) @declares.function @container
(class_specifier name: (_) @name body: (_)) @declares.type
(struct_specifier name: (_) @name body: (_)) @declares.type
(union_specifier name: (_) @name body: (_)) @declares.type
(enum_specifier name: (_) @name body: (_)) @declares.type
(type_definition declarator: (type_identifier) @name) @declares.type
(alias_declaration name: (type_identifier) @name) @declares.type
(namespace_definition name: (_) @name) @declares.module
(preproc_def name: (identifier) @name) @declares.macro
(preproc_function_def name: (identifier) @name) @declares.macro

(call_expression function: (identifier) @name) @invokes
(call_expression function: (field_expression field: (field_identifier) @name)) @invokes
(call_expression function: (qualified_identifier name: (identifier) @name)) @invokes
(call_expression function: (template_function name: (identifier) @name)) @invokes

(preproc_include path: (_) @target) @imports
