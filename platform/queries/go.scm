; Go syntax evidence. A call names a function or selector field, not a resolved callee.
(function_declaration name: (identifier) @name) @declares.function @container
(method_declaration name: (field_identifier) @name) @declares.method @container
(method_elem name: (field_identifier) @name) @declares.method
(type_spec name: (type_identifier) @name) @declares.type
(type_alias name: (type_identifier) @name) @declares.type
(const_spec name: (identifier) @name) @declares.const
(source_file (var_declaration (var_spec name: (identifier) @name) @declares.variable))
(source_file (var_declaration (var_spec_list (var_spec name: (identifier) @name) @declares.variable)))

(call_expression function: (identifier) @name) @invokes
(call_expression function: (selector_expression field: (field_identifier) @name)) @invokes

(import_spec path: (_) @target) @imports
